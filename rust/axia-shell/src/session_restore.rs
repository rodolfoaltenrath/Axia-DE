//! Context-owned session restore. Views may remount without restarting this service.
use crate::winmodel::{AppCommand, Session, WindowState};
use axia_native::windows::{dock_identity, WindowRef};
use axia_state::{Reader, Subscription};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::{Duration, Instant},
};
#[path = "session_restore_view.rs"]
mod view;
pub use view::View;

const AUTOSAVE: Duration = Duration::from_secs(2);
const START: Duration = Duration::from_millis(1500);
const STAGGER: Duration = Duration::from_millis(450);
const SETTLE: Duration = Duration::from_secs(10);
const CLAIM_TTL: Duration = Duration::from_secs(90);
#[cfg(test)]
#[path = "session_restore_tests.rs"]
mod tests;
fn rows(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn rules(cmd: &str, mut input: Value) -> Value {
    input["cmd"] = json!(cmd);
    crate::launcher::session_model::request(&input).unwrap_or(Value::Null)
}
fn index(v: &Value) -> usize {
    v.as_f64()
        .filter(|n| n.fract() == 0. && *n >= 0. && *n < 9.)
        .unwrap_or(0.) as usize
}
fn canonical(mut entry: Value) -> Value {
    if let Some(pairs) = entry["args"].as_array() {
        let object: Map<_, _> = pairs
            .iter()
            .filter_map(|pair| Some((pair[0].as_str()?.to_owned(), pair[1].clone())))
            .collect();
        entry["args"] = Value::Object(object);
    }
    entry
}
fn zones() -> Value {
    json!(crate::snap::Zone::ALL
        .iter()
        .map(|z| z.name())
        .collect::<Vec<_>>())
}
fn clean_list(list: &Value) -> Vec<Value> {
    rows(&rules("cleanList", json!({"list":list,"zones":zones()})))
        .iter()
        .cloned()
        .map(canonical)
        .collect()
}
#[derive(Clone)]
struct Step {
    entry: Value,
    number: usize,
    launch: bool,
}
struct Claim {
    step: Step,
    at: Instant,
}
struct Question {
    list: Vec<Value>,
    active: usize,
    unclean: bool,
}

pub struct Service {
    windows: Reader<WindowState>,
    changes: Subscription,
    settings: Option<(String, Reader<Value>, Subscription)>,
    save_pending: bool,
    publish_pending: bool,
    metadata: Value,
    model: Value,
    mode: String,
    browsers: bool,
    browser_policy: Option<String>,
    configuration_revision: Value,
    settings_revision: Option<u64>,
    started: bool,
    hold: bool,
    frozen: bool,
    saved: Value,
    initial: Vec<Value>,
    initial_active: usize,
    initial_unclean: bool,
    start_due: Option<(Instant, bool)>,
    autosave_due: Option<Instant>,
    step_due: Option<Instant>,
    settle_due: Option<Instant>,
    steps: VecDeque<Step>,
    claims: VecDeque<Claim>,
    pending: BTreeMap<u64, Step>,
    ticket: u64,
    top_entry: Option<usize>,
    top_window: Option<String>,
    restored: BTreeSet<String>,
    question: Option<Question>,
    effects: Vec<Value>,
    last: Option<(Vec<Value>, usize)>,
    revision: u64,
}
impl Service {
    pub fn new(windows: Reader<WindowState>) -> Self {
        let changes = windows.subscribe();
        Self {
            windows,
            changes,
            settings: None,
            save_pending: false,
            publish_pending: false,
            metadata: Value::Null,
            model: Value::Null,
            mode: "always".into(),
            browsers: true,
            browser_policy: None,
            configuration_revision: Value::Null,
            settings_revision: None,
            started: false,
            hold: true,
            frozen: false,
            saved: Value::Null,
            initial: Vec::new(),
            initial_active: 0,
            initial_unclean: false,
            start_due: None,
            autosave_due: None,
            step_due: None,
            settle_due: None,
            steps: VecDeque::new(),
            claims: VecDeque::new(),
            pending: BTreeMap::new(),
            ticket: 0,
            top_entry: None,
            top_window: None,
            restored: BTreeSet::new(),
            question: None,
            effects: Vec::new(),
            last: None,
            revision: 0,
        }
    }
    pub(crate) fn bind_settings(&mut self, home: &str) {
        let home = crate::settings::profile(home);
        if self.settings.as_ref().is_some_and(|(current, _, _)| current == &home) {
            return;
        }
        let reader = crate::settings::with(&home, |s| s.reader());
        let changes = reader.subscribe();
        self.settings = Some((home, reader, changes));
        self.settings_revision = None;
    }
    fn settings_pending(&self) -> bool {
        self.settings.as_ref().is_some_and(|(_, _, changes)| changes.pending())
    }
    fn preferences_reader(&self) -> Option<Reader<Value>> {
        self.settings.as_ref().map(|(_, reader, _)| reader.clone())
    }
    fn persist(&mut self) {
        if std::mem::take(&mut self.save_pending) {
            if let Some((home, _, _)) = &self.settings {
                let changed = crate::settings::with(home, |s| {
                    let changed = s.set("sessionSaved", &self.saved);
                    s.save(home, true);
                    changed
                });
                self.publish_pending |= changed;
            } else {
                self.effect("save", self.saved.clone());
            }
        }
        if std::mem::take(&mut self.publish_pending) {
            if let Some((home, _, _)) = &self.settings {
                let reply = crate::settings::with(home, |s| s.snapshot());
                self.emit(json!({"kind":"settingsState","reply":reply}));
            }
        }
    }
    fn emit(&mut self, effect: Value) {
        self.effects.push(effect);
        self.revision = self.revision.wrapping_add(1);
    }
    fn effect(&mut self, op: &str, value: Value) {
        self.emit(json!({"kind":"sessionRestore","op":op,"value":value}));
    }
    fn active_index(&self) -> usize {
        let state = self.windows.read();
        state
            .desktops()
            .iter()
            .position(|id| *id == state.active_desktop())
            .unwrap_or(0)
    }
    fn key(&self, w: &crate::winmodel::Window<'_>) -> String {
        dock_identity(
            &WindowRef {
                id: w.id(),
                app: w.app(),
                title: w.title(),
                args: w.args(),
            },
            rows(&self.metadata["registry"]),
            rows(&self.metadata["catalog"]),
        )
    }
    fn snapshot(&self) -> Vec<Value> {
        let state = self.windows.read();
        let registry = rows(&self.metadata["registry"]);
        let mut list = Vec::new();
        for w in state.windows() {
            if w.pip()
                || !crate::window_policy::remembers(crate::winmodel::truthy(&w.args()["dialog"]))
            {
                continue;
            }
            let linux = w.id().starts_with("linux#");
            let app = if linux {
                if w.args()["pkg"] == true {
                    registry.iter().find(|a| a["id"] == w.args()["appId"])
                } else {
                    None
                }
            } else {
                registry.iter().find(|a| a["id"] == w.app())
            };
            if (!linux && app.is_none_or(|a| a["builtin"] == true))
                || app.is_some_and(|a| a["hidden"] == true || a["live"] == true)
            {
                continue;
            }
            let Some((frame, maximized, snapped)) = w.session_geometry() else {
                continue;
            };
            let mut entry = json!({"id":self.key(&w),"f":frame.to_value()});
            if maximized {
                entry["max"] = json!(1);
            }
            if w.minimized() {
                entry["min"] = json!(1);
            }
            if let Some(zone) = snapped {
                entry["snap"] = json!(zone);
            }
            if let Some(desk) = state
                .desktops()
                .iter()
                .position(|id| *id == w.desktop())
                .filter(|i| *i > 0)
            {
                entry["desk"] = json!(desk);
            }
            if !linux {
                entry["args"] = w.args().clone();
            }
            let entry = rules("cleanEntry", json!({"e":entry,"zones":zones()}));
            if !entry.is_null() {
                list.push(canonical(entry));
            }
        }
        let extra = list.len().saturating_sub(12);
        list.drain(..extra);
        list
    }
    fn write(&mut self, list: Vec<Value>, clean: Value) {
        self.saved = json!({"windows":list,"active":self.active_index(),"clean":clean});
        self.save_pending = true;
    }
    fn saved_list(&self) -> Vec<Value> {
        rows(&self.saved["windows"]).to_vec()
    }
    fn visible(&mut self, question: Option<Question>) {
        let changed = self.question.is_some() != question.is_some();
        self.question = question;
        if changed {
            self.effect("visible", json!(self.question.is_some()));
        }
        self.revision = self.revision.wrapping_add(1);
    }
    fn configure(&mut self, metadata: &Value, input: &Value, now: Instant) {
        if let Some(reader) = self.preferences_reader() {
            if let Some((_, _, changes)) = &self.settings { changes.take(); }
            self.configure_preferences(metadata, input, &reader.read(), now);
        } else {
            self.configure_preferences(metadata, input, &input["settings"], now);
        }
    }
    fn configure_preferences(&mut self, metadata: &Value, input: &Value, preferences: &Value, now: Instant) {
        let configured = input["context"]["renderEpoch"].is_number()
            && self.configuration_revision != input["context"]["renderEpoch"];
        if configured {
            self.configuration_revision = input["context"]["renderEpoch"].clone();
        }
        if metadata["registry"].is_array() {
            for key in ["registry", "catalog"] {
                if self.metadata[key] != metadata[key] {
                    if !self.metadata.is_object() {
                        self.metadata = json!({});
                    }
                    self.metadata[key] = metadata[key].clone();
                    self.revision = self.revision.wrapping_add(1);
                }
            }
        }
        let model = if input["model"].is_object() {
            &input["model"]
        } else {
            &input["args"]["model"]
        };
        if model.is_object() && self.model != *model {
            self.model = model.clone();
        }
        let settings_revision = self.settings.as_ref().map(|(_, reader, _)| reader.revision())
            .or_else(|| input["settingsRevision"].as_u64())
            .or_else(|| input["context"]["preferenceRevision"].as_u64());
        let current = settings_revision
            .zip(self.settings_revision)
            .is_none_or(|(next, current)| next >= current);
        if let Some(settings) = preferences.as_object().filter(|_| current) {
            if settings_revision.is_some() {
                self.settings_revision = settings_revision;
            }
            let mode = settings
                .get("sessionRestore")
                .and_then(Value::as_str)
                .filter(|m| ["off", "ask", "always"].contains(m))
                .unwrap_or("always");
            let changed = self.mode != mode;
            let browsers = settings.get("sessionBrowsers") != Some(&Value::Bool(false));
            let policy_changed = changed || self.browsers != browsers;
            self.mode = mode.to_owned();
            self.browsers = browsers;
            if changed && self.started {
                self.revision = self.revision.wrapping_add(1);
                self.last = None;
                if mode == "off" {
                    self.autosave_due = None;
                    self.start_due = None;
                    self.step_due = None;
                    self.settle_due = None;
                    self.steps.clear();
                    self.claims.clear();
                    self.pending.clear();
                    self.hold = false;
                    self.visible(None);
                    self.saved = Value::Null;
                    self.save_pending = true;
                } else if !self.hold && !self.frozen {
                    self.autosave_due = Some(now + AUTOSAVE);
                }
            }
            if self.started && (policy_changed || configured) {
                self.apply_browser();
            }
        }
    }
    fn apply_browser(&mut self) {
        let policy = if self.browsers && self.mode != "off" {
            "on"
        } else {
            "off"
        };
        if self.browser_policy.as_deref() != Some(policy) {
            self.browser_policy = Some(policy.to_owned());
            self.effect("browser", json!(policy));
        }
    }
    fn start(&mut self, input: &Value, now: Instant) {
        if self.started {
            return;
        }
        self.started = true;
        self.revision = self.revision.wrapping_add(1);
        self.saved = self.settings.as_ref().map(|(_, reader, _)| reader.read()["sessionSaved"].clone())
            .unwrap_or_else(|| input["settings"]["sessionSaved"].clone());
        self.apply_browser();
        if self.mode == "off" {
            if !self.saved.is_null() {
                self.saved = Value::Null;
                self.save_pending = true;
            }
            self.hold = false;
            return;
        }
        let switched = self.saved["clean"] == "theme";
        let list = clean_list(&self.saved["windows"]);
        let decision = if switched {
            json!("none")
        } else {
            rules(
                "restoreDecision",
                json!({"mode":self.mode,"clean":self.saved["clean"]==true,"count":list.len()}),
            )
        };
        self.initial_active = index(&self.saved["active"]);
        self.initial_unclean = self.saved["clean"] != true;
        self.initial = list.clone();
        self.write(
            if decision == "none" && !switched {
                Vec::new()
            } else {
                list
            },
            json!(false),
        );
        if decision == "none" {
            self.hold = false;
            self.autosave_due = Some(now + AUTOSAVE);
        } else {
            self.start_due = Some((now + START, decision == "run"));
        }
    }
    fn mirror(&mut self, reply: Value) {
        if !reply["state"].is_null() {
            self.effect("state", reply);
        }
    }
    fn desktop(&mut self, session: &mut Session, position: usize) -> Option<i64> {
        if self.windows.read().desktops().len() <= position {
            if let Ok(reply) = session.app_command(AppCommand::PrepareDesktops(position, None), &[])
            {
                self.mirror(reply);
            }
        }
        self.windows.read().desktops().get(position).copied()
    }
    fn fit(&self, frame: &Value) -> Value {
        let displays = rows(&self.model["displays"]);
        let areas: Vec<_> = displays.iter().map(|d| d["work"].clone()).collect();
        rules(
            "fitFrame",
            json!({"f":frame,"displays":displays,"areas":areas,"main":displays.iter().position(|d|d["main"]==true).unwrap_or(0),"fallbackArea":self.model["fallbackArea"]}),
        )
    }
    fn once(&self, id: &str) -> bool {
        let value = json!(id);
        let Some((kind, key)) = crate::launcher::dock_model::parse_id(&value) else {
            return false;
        };
        kind == "lpk"
            || (kind == "app"
                && rows(&self.metadata["registry"])
                    .iter()
                    .any(|a| a["id"] == key && a["isolated"] == true))
            || (self.browsers
                && ["desktop:chromium", "desktop:firefox", "desktop:firefox-esr"].contains(&id))
    }
    fn begin(&mut self, session: &mut Session, list: Vec<Value>, active: usize, now: Instant) {
        self.hold = true;
        self.visible(None);
        self.pending.clear();
        self.claims.clear();
        self.top_window = None;
        let highest = list
            .iter()
            .map(|e| index(&e["desk"]))
            .max()
            .unwrap_or(0)
            .max(active);
        if let Ok(reply) =
            session.app_command(AppCommand::PrepareDesktops(highest, Some(active)), &[])
        {
            self.mirror(reply);
        }
        let mut open: BTreeMap<String, i64> = BTreeMap::new();
        {
            let state = self.windows.read();
            for w in state.windows() {
                *open.entry(self.key(&w)).or_default() += 1;
            }
        }
        let ids: Vec<_> = list.iter().map(|e| e["id"].clone()).collect();
        let once: BTreeSet<_> = ids
            .iter()
            .filter_map(Value::as_str)
            .filter(|id| self.once(id))
            .collect();
        let plan = rules(
            "planRestore",
            json!({"ids":ids,"open":open.into_iter().collect::<Vec<_>>(),"once":once}),
        );
        self.steps = rows(&plan)
            .iter()
            .filter_map(|s| {
                let i = s["index"].as_u64()? as usize;
                Some(Step {
                    entry: list.get(i)?.clone(),
                    number: i,
                    launch: s["launch"] == true,
                })
            })
            .collect();
        self.top_entry = self
            .steps
            .iter()
            .rev()
            .find(|s| s.entry["min"] != 1 && index(&s.entry["desk"]) == self.active_index())
            .map(|s| s.number);
        if self.steps.is_empty() {
            self.finish(now);
        } else {
            self.advance(session, now);
        }
    }
    fn next_ticket(&mut self) -> Option<u64> {
        self.ticket = self.ticket.checked_add(1)?;
        Some(self.ticket)
    }
    fn advance(&mut self, session: &mut Session, now: Instant) {
        self.step_due = None;
        let Some(step) = self.steps.pop_front() else {
            return;
        };
        let entry = &step.entry;
        let id = entry["id"].as_str().unwrap_or("");
        self.restored.insert(id.to_owned());
        let parsed = crate::launcher::dock_model::parse_id(&entry["id"]);
        if let Some((kind, key)) = parsed {
            let app = if kind == "desktop" {
                None
            } else {
                rows(&self.metadata["registry"])
                    .iter()
                    .find(|a| a["id"] == key)
                    .cloned()
            };
            if let Some(app) = app.filter(|a| a["isolated"] != true && a["package"] != true) {
                if app["builtin"] != true && app["hidden"] != true && app["live"] != true {
                    let new = self.windows.read().windows().any(|w| w.app() == key);
                    let desktop = self.desktop(session, index(&entry["desk"]));
                    let frame = self.fit(&entry["f"]);
                    if let Some(ticket) = self.next_ticket() {
                        self.effect("open",json!({"ticket":ticket,"app":key,"new":new,"frame":frame,"desktop":desktop,"args":entry["args"]}));
                        self.pending.insert(ticket, step.clone());
                    }
                }
            } else if kind == "desktop"
                || rows(&self.metadata["registry"])
                    .iter()
                    .any(|a| a["id"] == key)
            {
                self.claims.push_back(Claim {
                    step: step.clone(),
                    at: now,
                });
                if step.launch {
                    self.effect("launch", json!(id));
                }
            }
        }
        if self.steps.is_empty() {
            self.settle_due = Some(now + SETTLE);
        } else {
            self.step_due = Some(now + STAGGER);
        }
    }
    fn finish(&mut self, now: Instant) {
        self.hold = false;
        self.last = None;
        self.settle_due = None;
        self.autosave_due = Some(now + AUTOSAVE);
    }
    fn answer(&mut self, session: &mut Session, yes: bool, now: Instant) {
        let Some(question) = self.question.take() else {
            return;
        };
        self.effect("visible", json!(false));
        if yes {
            self.begin(session, question.list, question.active, now);
        } else {
            self.hold = false;
            self.last = None;
            self.write(self.snapshot(), json!(false));
        }
    }
    fn opened(&mut self, session: &mut Session, ticket: u64, id: &str) {
        let Some(step) = self.pending.remove(&ticket) else {
            return;
        };
        if !self
            .windows
            .read()
            .window(id)
            .is_some_and(|w| self.key(&w) == step.entry["id"])
        {
            return;
        }
        let mut commands = Vec::new();
        if let Some(zone) = step.entry["snap"].as_str() {
            commands.push(("snap", vec![json!(id), json!(zone)]));
        } else if step.entry["max"] == 1 {
            commands.push(("toggleMaximize", vec![json!(id)]));
        }
        if step.entry["min"] == 1 {
            commands.push(("minimize", vec![json!(id)]));
        }
        if self.top_entry == Some(step.number) {
            self.top_window = Some(id.to_owned());
        } else if let Some(top) = self
            .top_window
            .as_ref()
            .filter(|id| self.windows.read().window(id).is_some())
        {
            commands.push(("raise", vec![json!(top)]));
        }
        for (op, args) in commands {
            let mut input = vec![self.model.clone()];
            input.extend(args);
            if let Ok(reply) = session.call(op, &input) {
                self.mirror(reply);
            }
        }
    }
    fn claim(&mut self, session: &mut Session, id: &str, now: Instant) -> Value {
        self.claims
            .retain(|claim| now.saturating_duration_since(claim.at) <= CLAIM_TTL);
        let Some(at) = self
            .claims
            .iter()
            .position(|claim| claim.step.entry["id"] == id)
        else {
            return Value::Null;
        };
        let claim = self.claims.remove(at).unwrap();
        let frame = self.fit(&claim.step.entry["f"]);
        let desktop = self.desktop(session, index(&claim.step.entry["desk"]));
        let Some(ticket) = self.next_ticket() else {
            return Value::Null;
        };
        self.pending.insert(ticket, claim.step);
        json!({"ticket":ticket,"frame":frame,"desktop":desktop})
    }
    fn tick(&mut self, session: &mut Session, now: Instant) {
        if self.settings_pending() { self.configure(&Value::Null, &Value::Null, now); }
        self.tick_at(session, now);
        self.persist();
    }
    fn tick_at(&mut self, session: &mut Session, now: Instant) {
        let changed = self.changes.take().is_some();
        if !self.started || self.frozen || self.mode == "off" {
            return;
        }
        if changed && !self.hold {
            self.autosave_due.get_or_insert(now + AUTOSAVE);
        }
        if self.start_due.is_some_and(|(at, _)| now >= at) {
            let (_, run) = self.start_due.take().unwrap();
            let list = self.initial.clone();
            if run {
                self.begin(session, list, self.initial_active, now);
            } else {
                self.visible(Some(Question {
                    list,
                    active: self.initial_active,
                    unclean: self.initial_unclean,
                }));
            }
        }
        if self.step_due.is_some_and(|at| now >= at) {
            self.advance(session, now);
        }
        if self.settle_due.is_some_and(|at| now >= at) {
            self.finish(now);
        }
        if self.autosave_due.is_some_and(|at| now >= at) {
            self.autosave_due = None;
            if self.hold {
                return;
            }
            let list = self.snapshot();
            let active = self.active_index();
            if self.last.as_ref() != Some(&(list.clone(), active)) {
                self.last = Some((list.clone(), active));
                self.write(list, json!(false));
            }
        }
    }
    fn deadline(&self) -> Option<Instant> {
        if self.frozen || !self.started {
            return None;
        }
        [
            self.start_due.map(|d| d.0),
            self.step_due,
            self.settle_due,
            self.autosave_due,
        ]
        .into_iter()
        .flatten()
        .min()
    }
    pub fn command(
        &mut self,
        session: &mut Session,
        metadata: &Value,
        input: &Value,
        now: Instant,
    ) -> Value {
        if input["action"] == "preferences" {
            let reader = self.preferences_reader();
            let borrowed = reader.as_ref().map(Reader::read);
            let preferences = borrowed.as_deref().unwrap_or(&input["settings"]);
            let mode = preferences["sessionRestore"].as_str().filter(|m| ["off", "ask", "always"].contains(m)).unwrap_or("always");
            return json!({"result":{"mode":mode,"browsers":preferences["sessionBrowsers"]!=false},"effects":[]});
        }
        if matches!(input["action"].as_str(), Some("setMode" | "setBrowsers")) {
            let Some((home, _, _)) = &self.settings else { return json!({"error":"app.invalidRequest"}); };
            if input["action"] == "setMode" {
                let Some(mode) = input["mode"].as_str().filter(|m| ["off", "ask", "always"].contains(m)) else {
                    return json!({"error":"app.invalidRequest"});
                };
                self.publish_pending |= crate::settings::with(home, |s| {
                    let mut changed = s.set("sessionRestore", &json!(mode));
                    if mode == "off" { changed |= s.set("sessionSaved", &Value::Null); }
                    changed
                });
            } else {
                let Some(on) = input["on"].as_bool() else { return json!({"error":"app.invalidRequest"}); };
                self.publish_pending |= crate::settings::with(home, |s| s.set("sessionBrowsers", &json!(on)));
            }
        }
        self.configure(metadata, input, now);
        let mut result = Value::Null;
        match input["action"].as_str().unwrap_or("") {
            "start" => self.start(input, now),
            "answer" => self.answer(session, input["yes"] == true, now),
            "claim" => result = self.claim(session, input["id"].as_str().unwrap_or(""), now),
            "opened" => {
                if let (Some(ticket), Some(id)) = (input["ticket"].as_u64(), input["id"].as_str()) {
                    self.opened(session, ticket, id);
                }
            }
            "restored" => {
                let id = input["id"].as_str().unwrap_or("");
                result = json!(
                    self.restored.contains(id) || self.steps.iter().any(|s| s.entry["id"] == id)
                );
            }
            "flush" | "exit" if self.started && !self.frozen && self.mode != "off" => {
                self.frozen = true;
                self.autosave_due = None;
                let saved = self.saved_list();
                let list = if self.hold {
                    saved.clone()
                } else {
                    self.snapshot()
                };
                self.write(
                    if input["action"] == "exit" && list.len() < saved.len() {
                        saved
                    } else {
                        list
                    },
                    json!(true),
                );
            }
            "resume" if self.started && self.frozen => {
                self.frozen = false;
                if self.mode != "off" {
                    self.write(self.saved_list(), json!(false));
                    if !self.hold {
                        self.autosave_due = Some(now + AUTOSAVE);
                    }
                }
            }
            "theme" if self.started && !self.frozen => {
                self.frozen = true;
                self.autosave_due = None;
                self.visible(None);
                self.write(self.saved_list(), json!("theme"));
            }
            "configure" => {}
            "browserFailed" => {
                if self.browser_policy.as_deref() == input["policy"].as_str() {
                    self.browser_policy = None;
                }
            }
            _ => {}
        }
        self.persist();
        json!({"result":result,"effects":std::mem::take(&mut self.effects)})
    }
}
