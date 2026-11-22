//! Context-owned notification delivery, history, grouping and monotonic expiry.
use axia_state::{Reader, Store, Subscription};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
mod access;
mod clock_delivery;
mod preferences;
mod presentation;
mod sound;
#[cfg(test)]
#[path = "notifications/settings_tests.rs"]
mod settings_tests;
#[cfg(test)]
#[path = "notifications/catalog_tests.rs"]
mod catalog_tests;

const MAX_SHOWN: usize = 4;
const HISTORY: usize = 50;
const RING_FOR: Duration = Duration::from_secs(60);
const RING_STEP: Duration = Duration::from_millis(2500);

#[derive(Clone)]
struct Ring {
    until: Instant,
    next: Instant,
}

#[derive(Clone)]
pub struct Notice {
    pub data: Value,
    until: Option<Instant>,
    ring: Option<Ring>,
}
impl Notice {
    pub fn id(&self) -> u64 {
        self.data["id"].as_u64().unwrap()
    }
    fn group_key(&self) -> &str {
        if self.data["app"] == "" {
            self.data["label"].as_str().unwrap_or("")
        } else {
            self.data["app"].as_str().unwrap_or("")
        }
    }
}
#[derive(Default)]
pub struct State {
    pub shown: Vec<Notice>,
    pub history: Vec<Value>,
    pub ungrouped: bool,
    pub bottom: bool,
}
impl State {
    pub fn grouped(&self) -> Vec<(&Notice, usize)> {
        let mut out: Vec<(&Notice, usize)> = Vec::new();
        for notice in &self.shown {
            if let Some((_, more)) = out
                .iter_mut()
                .find(|(n, _)| !self.ungrouped && n.group_key() == notice.group_key())
            {
                *more += 1;
            } else {
                out.push((notice, 0));
            }
        }
        if self.bottom {
            out.reverse();
        }
        out
    }
    fn snapshot(&self) -> Value {
        json!({"shown":self.shown.iter().map(|n|&n.data).collect::<Vec<_>>(),"history":self.history,
            "placement":if self.bottom {"bottom"} else {"top"},
            "ringing":self.shown.iter().filter(|n|n.ring.is_some()).map(Notice::id).collect::<Vec<_>>()})
    }
}

#[derive(Default)]
pub struct Service {
    pub(crate) resident: bool,
    state: Store<State>,
    next: u64,
    sent: Option<u64>,
    effects: Vec<Value>,
    ring_prefs: Value,
    clock_source: access::Source,
    presentation: presentation::Presentation,
    clock_actions: std::collections::BTreeMap<u64, Vec<clock_delivery::Action>>,
    sounds: Vec<(u64, bool)>,
    settings: Option<(String, Reader<Value>, Subscription)>,
    catalog: Option<(Reader<Value>, Subscription)>,
}

// Wall time is used for user schedules/history; elapsed banner time is monotonic.
pub fn dnd(prefs: &Value, wall_ms: f64, minute: f64) -> bool {
    if prefs["dnd"] == true || prefs["dnd"].as_f64().is_some_and(|until| wall_ms < until) {
        return true;
    }
    let schedule = &prefs["schedule"];
    if schedule["on"] != true {
        return false;
    }
    let (Some(from), Some(to)) = (schedule["from"].as_f64(), schedule["to"].as_f64()) else {
        return false;
    };
    let len = (to - from).rem_euclid(1440.);
    len > 0. && (minute - from).rem_euclid(1440.) < len
}

impl Service {
    fn configure_presentation(&mut self, input: &Value, config: &Value) {
        let settings = self.settings.as_ref().map(|(home, reader, _)| (home, reader.read()));
        self.presentation.configure(input, config, settings.as_ref().map(|(home, value)| (home.as_str(), &**value)));
    }
    pub(crate) fn bind_catalog(&mut self, reader: Reader<Value>) {
        let changes = reader.subscribe();
        self.catalog = Some((reader, changes));
    }
    pub(crate) fn catalog_pending(&self) -> bool {
        self.catalog.as_ref().is_some_and(|(_, changes)| changes.pending())
    }
    fn source(&self, app: &str, fallback: &Value) -> access::Source {
        if let Some((reader, _)) = &self.catalog {
            let metadata = reader.read();
            let entry = metadata["registry"].as_array().into_iter().flatten().rev()
                .find(|entry| entry["id"].as_str() == Some(app));
            access::Source::from(entry.unwrap_or(&Value::Null))
        } else {
            access::Source::from(fallback)
        }
    }
    fn clock_source(&self) -> access::Source {
        if self.catalog.is_some() {
            self.source("clock", &Value::Null)
        } else {
            self.clock_source.clone()
        }
    }
    fn with_preferences<T>(&self, fallback: &Value, read: impl FnOnce(&Value) -> T) -> T {
        if let Some((_, reader, _)) = &self.settings {
            read(&reader.read()["notifications"])
        } else {
            read(fallback)
        }
    }
    pub(crate) fn bind_settings(&mut self, home: &str) {
        let home = crate::settings::profile(home);
        if self.settings.as_ref().is_some_and(|(current, _, _)| current == &home) {
            return;
        }
        let reader = crate::settings::with(&home, |s| s.reader());
        let changes = reader.subscribe();
        self.ring_prefs = preferences::normalize(&reader.read()["notifications"]);
        self.settings = Some((home, reader, changes));
        self.configure(self.ring_prefs["grouping"] == "off");
    }
    pub(crate) fn settings_pending(&self) -> bool {
        self.settings.as_ref().is_some_and(|(_, _, changes)| changes.pending())
    }
    fn sync_preferences(&mut self) {
        if let Some((_, reader, changes)) = &self.settings {
            if changes.take().is_none() {
                return;
            }
            self.ring_prefs = preferences::normalize(&reader.read()["notifications"]);
            self.configure(self.ring_prefs["grouping"] == "off");
        }
    }
    fn policy(&mut self, input: &Value) {
        if self.settings.is_none() {
            self.ring_prefs = preferences::normalize(&input["prefs"]);
        }
        self.clock_source = access::Source::from(&input["source"]);
    }
    pub fn reader(&self) -> Reader<State> {
        self.state.reader()
    }
    pub fn wait_ms(&self, now: Instant) -> Option<f64> {
        self.state
            .read()
            .shown
            .iter()
            .flat_map(|n| {
                [n.until, n.ring.as_ref().map(|r| r.next.min(r.until))]
                    .into_iter()
                    .flatten()
            })
            .min()
            .map(|until| until.saturating_duration_since(now).as_secs_f64() * 1000.)
    }
    pub(crate) fn background_wait(&self, now: Instant) -> Option<f64> {
        if !self.resident { return None; }
        if self.settings_pending() || self.catalog_pending() || self.sent != Some(self.state.revision())
            || !self.effects.is_empty() || !self.sounds.is_empty() { return Some(0.); }
        self.wait_ms(now)
    }
    pub(crate) fn background_tick(&mut self, now: Instant) -> Vec<Value> {
        self.expire(now);
        self.effects()
    }
    pub fn configure(&self, ungrouped: bool) {
        self.state.update(|s| {
            let changed = s.ungrouped != ungrouped;
            s.ungrouped = ungrouped;
            ((), changed)
        });
    }
    pub fn expire(&mut self, now: Instant) {
        self.sync_preferences();
        if let Some((_, changes)) = &self.catalog { changes.take(); }
        let wall = axia_clock::now();
        let local = axia_clock::local(wall);
        self.expire_at(now, wall as f64, (local.tm_hour * 60 + local.tm_min) as f64);
    }
    fn audible_policy(&self, wall: f64, minute: f64) -> bool {
        let source = self.clock_source();
        let prefs = preferences::app(
            &self.ring_prefs,
            "clock",
            &access::permission("clock", &source),
        );
        access::allowed("clock", &source)
            && prefs["allow"] != false
            && prefs["sound"] != false
            && prefs["style"] != "none"
            && !dnd(&self.ring_prefs, wall, minute)
    }
    fn expire_at(&mut self, now: Instant, wall: f64, minute: f64) {
        let ids: Vec<_> = self
            .state
            .read()
            .shown
            .iter()
            .filter(|n| n.until.is_some_and(|until| now >= until))
            .map(Notice::id)
            .collect();
        let allowed = self.audible_policy(wall, minute);
        let sounds = self.state.update(|s| {
            s.shown.retain(|n| !ids.contains(&n.id()));
            let mut changed = !ids.is_empty();
            let mut sounds = Vec::new();
            for notice in &mut s.shown {
                if let Some(ring) = &mut notice.ring {
                    if !allowed || now >= ring.until {
                        notice.ring = None;
                        changed = true;
                    } else if now >= ring.next {
                        ring.next = now + RING_STEP;
                        sounds.push(notice.id());
                    }
                }
            }
            (sounds, changed)
        });
        self.sounds.extend(sounds.into_iter().map(|id| (id, true)));
        self.effects.extend(
            ids.into_iter()
                .map(|id| json!({"kind":"notificationClose","id":id,"reason":"closed"})),
        );
    }
    fn dismiss(&mut self, id: u64, reason: &str) {
        let removed = self.state.update(|s| {
            let before = s.shown.len();
            s.shown.retain(|n| n.id() != id);
            let changed = before != s.shown.len();
            (changed, changed)
        });
        if removed {
            self.effects
                .push(json!({"kind":"notificationClose","id":id,"reason":reason}));
        }
    }
    pub fn effects(&mut self) -> Vec<Value> {
        let state = self.state.read();
        self.clock_actions.retain(|id, _| {
            state.shown.iter().any(|n| n.id() == *id)
                || state.history.iter().any(|n| n["id"] == *id)
        });
        drop(state);
        let mut effects = Vec::new();
        if self.sent != Some(self.state.revision()) {
            self.sent = Some(self.state.revision());
            effects.push(json!({"kind":"notificationState","revision":self.state.revision(),"state":self.state.read().snapshot()}));
        }
        // Drain audio directly, after commands have dismissed/evicted any due banner.
        // No pending sound crosses the JS bridge or survives a disconnected audio server.
        let state = self.state.read();
        let clock_allowed = access::allowed("clock", &self.clock_source());
        for (id, repeat) in self.sounds.drain(..) {
            if (!repeat || clock_allowed)
                && state
                    .shown
                    .iter()
                    .any(|n| n.id() == id && (!repeat || n.ring.is_some()))
            {
                sound::notify();
            }
        }
        effects.append(&mut self.effects);
        effects
    }
    pub fn command(&mut self, input: &Value, now: Instant) -> Value {
        match input["action"].as_str() {
            Some("preferences") => {
                return json!({"result":self.with_preferences(&input["prefs"],preferences::normalize),"effects":[]})
            }
            Some("appPreferences") => {
                let app = input["app"].as_str().unwrap_or("");
                let result = self.with_preferences(&input["prefs"], |prefs| preferences::app(prefs,app,&access::permission(app,&self.source(app,&input["source"]))));
                return json!({"result":result,"effects":[]});
            }
            Some("policy") => {
                let result = self.with_preferences(&input["prefs"], |prefs| dnd(prefs,
                    input["wallMs"].as_f64().unwrap_or(0.),input["minute"].as_f64().unwrap_or(0.)));
                return json!({"result":result,"effects":[]});
            }
            Some("setAppPreferences") => {
                let app = input["app"].as_str().unwrap_or("");
                let source = self.source(app, &input["source"]);
                let result = self.with_preferences(&input["prefs"], |prefs| preferences::patch(prefs, app, &input["patch"]));
                let permission = access::update(app, &source, &input["patch"]);
                let state = self.settings.as_ref().and_then(|(home, _, _)| {
                    crate::settings::with(home, |s| s.set("notifications", &result).then(|| s.snapshot()))
                });
                let mut effects = Vec::new();
                if state.is_some() || permission.is_some() {
                    self.expire(now);
                    effects = self.effects();
                }
                if let Some(reply) = state {
                    effects.push(json!({"kind":"settingsState","reply":reply}));
                }
                if let Some(saved) = permission {
                    effects.push(
                        json!({"kind":"notificationPermissionChanged","app":app,"saved":saved}),
                    );
                }
                return json!({"result":if self.settings.is_some() {Value::Null} else {result},"effects":effects});
            }
            _ => {}
        }
        if input["action"] == "ringPolicy" {
            self.policy(input);
        } else if input["action"] == "push" && input["notice"]["app"] == "clock" {
            self.policy(input);
        }
        self.expire(now);
        let result = match input["action"].as_str() {
            Some("ringPolicy") => Value::Null,
            Some("push") => {
                let n = &input["notice"];
                let native = self.settings.is_some() || self.catalog.is_some();
                let wall = if native { axia_clock::now() } else { input["wallMs"].as_i64().unwrap_or(0) };
                let local = axia_clock::local(wall);
                let minute = if native { (local.tm_hour * 60 + local.tm_min) as f64 } else { input["minute"].as_f64().unwrap_or(0.) };
                let prefs = &if self.settings.is_some() {
                    self.ring_prefs.clone()
                } else {
                    preferences::normalize(&input["prefs"])
                };
                let app = n["app"].as_str().unwrap_or("");
                let source = self.source(app, &input["source"]);
                let ap = &preferences::app(prefs, app, &access::permission(app, &source));
                // Remember attempted apps even when permission/firewall denies delivery.
                let mut seen: Vec<Value> = prefs["seen"].as_array().cloned().unwrap_or_default();
                if !app.is_empty() && !seen.iter().any(|id| id == app) {
                    seen.insert(0, json!(app));
                    seen.truncate(64);
                    let mut patch = prefs.clone();
                    if !patch.is_object() {
                        patch = json!({});
                    }
                    patch["seen"] = json!(seen);
                    if let Some((home, _, _)) = &self.settings {
                        let reply = crate::settings::with(home, |s| s.command(
                            &json!({"cmd":"patch","patch":{"notifications":patch}})));
                        self.effects.push(json!({"kind":"settingsState","reply":reply}));
                    } else {
                        self.effects.push(json!({"kind":"settings","patch":{"notifications":patch}}));
                    }
                }
                if ap["allow"] == false || access::blocked(app, &source) {
                    json!(0)
                } else {
                    self.next = self.next.checked_add(1).expect("notification ID exhausted");
                    let id = self.next;
                    let actions: Vec<_>=n["actions"].as_array().into_iter().flatten().take(3).map(|a|json!({"id":a["id"],"label":a["label"],"primary":a["primary"]==true})).collect();
                    let ring_requested = n["ring"] == true && app == "clock";
                    let sticky = n["sticky"] == true || ap["style"] == "alert" || ring_requested;
                    self.configure_presentation(input, &Value::Null);
                    let catalog = self.catalog.as_ref().map(|(reader, _)| reader.read());
                    let entry = catalog.as_ref().and_then(|catalog| catalog["registry"].as_array())
                        .and_then(|apps| apps.iter().rev().find(|entry| entry["id"].as_str() == Some(app)));
                    let (display_label, has_app) = if self.catalog.is_some() {
                        self.presentation.app_name(app, entry, n["label"].as_str().unwrap_or(""))
                    } else {
                        (n["displayLabel"].as_str().unwrap_or("").to_owned(), n["hasApp"] == true)
                    };
                    let time_label = if native {self.presentation.time(wall)} else {n["timeLabel"].as_str().unwrap_or("").to_owned()};
                    let data = json!({"id":id,"app":app,"label":n["label"].as_str().unwrap_or(""),
                        "displayLabel":display_label,"hasApp":has_app,
                        "title":n["title"].as_str().unwrap_or(""),"body":n["body"].as_str().unwrap_or(""),
                        "actions":actions,"sticky":sticky,"time":wall,"timeLabel":time_label});
                    let visible = ap["style"] != "none"
                        && !dnd(
                            prefs,
                            wall as f64,
                            minute,
                        );
                    let timeout = n["timeout"]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .unwrap_or(6000.)
                        .clamp(1500., i32::MAX as f64);
                    self.state.update(|s| {
                        s.history.insert(0, data.clone());
                        s.history.truncate(HISTORY);
                        if visible {
                            s.shown.insert(
                                0,
                                Notice {
                                    data,
                                    until: (!sticky)
                                        .then(|| now + Duration::from_millis(timeout as u64)),
                                    ring: (ring_requested
                                        && n["sound"] != false
                                        && ap["sound"] != false)
                                        .then(|| Ring {
                                            until: now + RING_FOR,
                                            next: now + RING_STEP,
                                        }),
                                },
                            );
                            s.shown.truncate(MAX_SHOWN);
                        }
                        ((), true)
                    });
                    if visible && n["sound"] != false && ap["sound"] != false {
                        self.sounds.push((id, ring_requested));
                    }
                    json!(id)
                }
            }
            Some("dismiss") => {
                let Some(id) = input["id"].as_u64() else {
                    return json!({"error":"app.invalidRequest"});
                };
                self.dismiss(id, input["reason"].as_str().unwrap_or("closed"));
                Value::Null
            }
            Some("act") => {
                let (Some(id), Some(index)) = (input["id"].as_u64(), input["index"].as_u64())
                else {
                    return json!({"error":"app.invalidRequest"});
                };
                let action = {
                    let s = self.state.read();
                    let notice = s
                        .shown
                        .iter()
                        .find(|n| n.id() == id)
                        .map(|n| &n.data)
                        .or_else(|| {
                            (input["history"] == true)
                                .then(|| s.history.iter().find(|n| n["id"] == id))
                                .flatten()
                        });
                    notice
                        .and_then(|n| n["actions"].as_array())
                        .and_then(|a| a.get(usize::try_from(index).ok()?))
                        .cloned()
                };
                if let Some(action) = action {
                    self.dismiss(
                        id,
                        action["id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .unwrap_or("action"),
                    );
                    if let Some(native) = self
                        .clock_actions
                        .get(&id)
                        .and_then(|a| a.get(index as usize))
                        .cloned()
                    {
                        native.run(axia_clock::now());
                    } else {
                        self.effects
                            .push(json!({"kind":"notificationAction","id":id,"index":index}));
                    }
                }
                Value::Null
            }
            Some("clearHistory") => {
                self.state.update(|s| {
                    let changed = !s.history.is_empty();
                    s.history.clear();
                    ((), changed)
                });
                Value::Null
            }
            Some("placement") => {
                self.state.update(|s| {
                    let bottom = input["placement"] == "bottom";
                    let changed = s.bottom != bottom;
                    s.bottom = bottom;
                    ((), changed)
                });
                Value::Null
            }
            Some("snapshot") => self.state.read().snapshot(),
            _ => return json!({"error":"app.invalidRequest"}),
        };
        json!({"result":result,"effects":self.effects()})
    }
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;
