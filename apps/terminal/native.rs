//! The native Terminal window (stage 2b): tabs, panes, dividers, status bar, session
//! lifecycle, keyboard and mouse. The workspace (windows → tabs → panes → sessions) is shared
//! by every window of the shell session, as `state.js` kept it at module level, so a window
//! that is minimized and mounted again finds its tabs and its shells.
//!
//! Transport contract. Jobs (answers come back): `term.create` → session id, `term.infos`
//! (`[[sid…]]` → `[{sid,info}]`), `term.events` (→ `[{sid,type,data}]`, kept while no view is
//! mounted), `term.configure`/`term.close`/`term.copy`-style calls with no answer. Input is NOT
//! a job: key bytes, mouse and wheel leave as `effects` of kind `term` that the mount runs at
//! once (`write`, `key`, `mouse`, `wheel`, `focus`, `scroll`, `clear`, `select`, `mark`).
use crate::native_view::{async_jobs::{Completed, Jobs}, clock, KeyFacts, NativeApp};
use crate::request as model;
use crate::workspace::{Dir, Meta, Rect, Removed, Sid, Workspace};
use serde_json::{json, Map, Value};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[path = "native_ui.rs"]
mod ui;

const INFO_MS: i64 = 1500;
const EVENTS_MS: i64 = 250;
const UNDO_MS: i64 = 5000;
const MOVE_MS: i64 = 8;
/// Physical codes of the modifier keys: they never reach a shell on their own.
const MODIFIER_CODES: [i32; 9] = [29, 97, 42, 54, 56, 100, 125, 126, 58];

#[derive(Clone, Default)]
struct Session {
    profile: String,
    meta: Meta,
    badge: String,
    bell: bool,
    activity: bool,
    last_exit: i64,
    info: Value,
}
#[derive(Default)]
struct Shell {
    ws: Workspace,
    sessions: HashMap<Sid, Session>,
    /// Closed panes waiting out their undo window: (session, closes at).
    closing: Vec<(Sid, i64)>,
}
thread_local! { static SHELL: RefCell<Shell> = RefCell::new(Shell::default()); }
fn shell<R>(f: impl FnOnce(&mut Shell) -> R) -> R {
    SHELL.with(|s| f(&mut s.borrow_mut()))
}

#[derive(Clone)]
enum Purpose {
    Window,
    Tab,
    Split(Dir),
}
enum Tag {
    Create { purpose: Purpose, profile: String, wid: String },
    Quiet,
    Infos,
    Events,
}
struct DividerDrag {
    tab: String,
    split: String,
    parent: Rect,
}
pub struct App {
    jobs: Jobs<Tag>,
    booted: bool,
    theme: Rc<Value>,
    width: f64,
    height: f64,
    focused: bool,
    window: String,
    settings: Value,
    home: String,
    effects: Vec<Value>,
    capture: Option<bool>,
    focus_sid: Option<Sid>,
    info_due: i64,
    events_due: i64,
    infos_pending: bool,
    events_pending: bool,
    announced: Option<String>,
    drag: Option<DividerDrag>,
    mouse: Option<Sid>,
    last_move: (Sid, i64, i64, i64),
    close_window: bool,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            booted: false,
            theme: Rc::new(Value::Null),
            width: 860.,
            height: 540.,
            focused: true,
            window: String::new(),
            settings: Value::Null,
            home: String::new(),
            effects: vec![],
            capture: None,
            focus_sid: None,
            info_due: 0,
            events_due: 0,
            infos_pending: false,
            events_pending: false,
            announced: None,
            drag: None,
            mouse: None,
            last_move: (0, 0, 0, 0),
            close_window: false,
            last_tree: Value::Null,
        }
    }
}
fn number(v: &Value, fallback: f64) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(fallback)
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool {
        true
    }
    fn set_native_appearance(&mut self, theme: Rc<Value>) {
        self.theme = theme;
    }
    fn uses_preferences(&self) -> bool {
        true
    }
    fn sync_with_preferences(&mut self, v: &Value, preferences: &Value) -> Value {
        self.settings = preferences["terminal"].clone();
        self.request_at(v, clock::now())
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted.then_some(if self.jobs.busy() { 50. } else { 200. })
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["settings"]["terminal"].is_object() {
            self.settings = input["settings"]["terminal"].clone();
        }
        self.request_at(input, clock::now())
    }
    fn key_event(&mut self, key: &str, chr: &str, flags: i32, facts: &KeyFacts) -> Option<Value> {
        self.key_at(key, chr, flags, facts, clock::now())
    }
}
impl App {
    // ---- settings and profiles ----

    fn raw(&self) -> &Value {
        &self.settings
    }
    fn terminal_settings(&self) -> Value {
        model(&json!({"cmd":"settings","settings":self.raw()}))
    }
    fn profile(&self, id: &str) -> Value {
        model(&json!({"cmd":"profile","settings":{"profiles":self.raw()["profiles"],"defaultProfile":self.raw()["defaultProfile"]},"id":id}))
    }
    fn default_profile(&self) -> String {
        self.terminal_settings()["defaultProfile"].as_str().unwrap_or("default").to_owned()
    }
    fn config(&self, profile: &Value, timestamps: bool, font_delta: i32) -> Value {
        model(&json!({"cmd":"config","settings":{"schemes":self.raw()["schemes"]},"profile":profile,
            "extra":{"timestamps":timestamps,"fontDelta":font_delta}}))
    }
    fn session_profile(&self, sid: Sid) -> Value {
        let id = shell(|s| s.sessions.get(&sid).map(|x| x.profile.clone())).unwrap_or_default();
        self.profile(&id)
    }
    fn start_dir(&self, profile: &Value, reuse: Option<Sid>) -> Option<String> {
        match profile["workingDir"].as_str() {
            Some("home") => return None,
            Some("custom") => {
                if let Some(dir) = profile["customDir"].as_str().filter(|d| !d.is_empty()) {
                    return Some(match dir.strip_prefix('~') {
                        Some(rest) => format!("{}{rest}", self.home),
                        None => dir.to_owned(),
                    });
                }
            }
            _ => {}
        }
        let sid = reuse?;
        shell(|s| s.sessions.get(&sid).map(|x| (x.meta.cwd.clone(), x.info["cwd"].as_str().unwrap_or("").to_owned())))
            .and_then(|(meta, info)| [meta, info].into_iter().find(|d| !d.is_empty()))
    }

    // ---- sessions ----

    fn create(&mut self, purpose: Purpose, profile_id: &str, cwd: Option<String>) {
        let profile = self.profile(profile_id);
        let mut options = Map::new();
        if let Some(dir) = cwd.or_else(|| {
            let reuse = shell(|s| s.ws.current_sid(&self.window));
            self.start_dir(&profile, reuse)
        }) {
            options.insert("cwd".into(), json!(dir));
        }
        options.insert("scrollback".into(), profile["scrollback"].clone());
        let profile = profile["id"].as_str().unwrap_or(profile_id).to_owned();
        self.jobs.enqueue(Tag::Create { purpose, profile, wid: self.window.clone() }, "term.create", json!([Value::Object(options)]));
    }
    fn configure(&mut self, sid: Sid) {
        let found = shell(|s| s.ws.find_session(sid).and_then(|(wid, _)| s.ws.window(wid).map(|w| (w.timestamps, w.font_delta))));
        let (timestamps, delta) = found.unwrap_or((false, 0));
        let cfg = self.config(&self.session_profile(sid), timestamps, delta);
        self.jobs.enqueue(Tag::Quiet, "term.configure", json!([sid, cfg]));
    }
    fn configure_window(&mut self) {
        for sid in shell(|s| s.ws.sessions(&self.window)) {
            self.configure(sid);
        }
    }
    fn on_created(&mut self, purpose: Purpose, profile: String, wid: String, sid: Sid) {
        let placed = shell(|s| {
            let ok = match purpose {
                Purpose::Window => {
                    s.ws.ensure_window(&wid, sid);
                    true
                }
                Purpose::Tab => s.ws.add_tab(&wid, sid).is_some(),
                Purpose::Split(dir) => s.ws.split(&wid, dir, sid).is_some(),
            };
            if ok {
                s.sessions.insert(sid, Session { profile, ..Default::default() });
            }
            ok
        });
        if placed {
            self.configure(sid);
        } else {
            // the window went away while the session was starting
            self.jobs.enqueue(Tag::Quiet, "term.close", json!([sid]));
        }
    }
    /// A session ends (its shell exited or its pane closed for good): out of the tree, closed.
    fn remove_session(&mut self, sid: Sid) {
        let removed = shell(|s| {
            let removed = s.ws.detach(sid);
            s.sessions.remove(&sid);
            s.closing.retain(|(c, _)| *c != sid);
            removed
        });
        self.jobs.enqueue(Tag::Quiet, "term.close", json!([sid]));
        self.window_emptied(removed);
    }
    /// The last tab went away: the window leaves the workspace and closes.
    fn window_emptied(&mut self, removed: Removed) {
        if let Removed::WindowEmpty(wid) = removed {
            shell(|s| s.ws.drop_window(&wid));
            if wid == self.window {
                self.close_window = true;
            }
        }
    }
    fn close_pane(&mut self, sid: Sid, now: i64) {
        let profile = shell(|s| s.sessions.get(&sid).map(|x| x.profile.clone()));
        let removed = shell(|s| s.ws.close_pane(sid, profile));
        let Some(removed) = removed else { return };
        shell(|s| s.closing.push((sid, now + UNDO_MS)));
        self.window_emptied(removed);
    }
    fn expire_closed(&mut self, now: i64) {
        let due: Vec<Sid> = shell(|s| {
            let due: Vec<Sid> = s.closing.iter().filter(|(_, at)| *at <= now).map(|(sid, _)| *sid).collect();
            s.closing.retain(|(_, at)| *at > now);
            due
        });
        for sid in due {
            if shell(|s| s.ws.expire_closed(sid)) {
                shell(|s| s.sessions.remove(&sid));
                self.jobs.enqueue(Tag::Quiet, "term.close", json!([sid]));
            }
        }
    }
    fn effect(&mut self, op: &str, sid: Sid, args: Value) {
        let mut fx = json!({"kind":"term","op":op,"sid":sid});
        for (k, v) in args.as_object().into_iter().flatten() {
            fx[k] = v.clone();
        }
        self.effects.push(fx);
    }
    /// Input for the focused pane, or for every pane of the tab while broadcasting.
    fn to_session(&mut self, op: &str, args: Value) {
        if let Some(sid) = shell(|s| s.ws.current_sid(&self.window)) {
            self.effect(op, sid, args);
        }
    }

    // ---- events ----

    fn is_active(&self, sid: Sid) -> bool {
        self.focused && shell(|s| s.ws.window(&self.window).and_then(|w| w.active_sid())) == Some(sid)
    }
    fn on_event(&mut self, event: &Value) {
        let (Some(sid), Some(kind)) = (event["sid"].as_i64(), event["type"].as_str()) else { return };
        let data = &event["data"];
        let active = self.is_active(sid);
        if kind == "exit" {
            self.remove_session(sid);
            return;
        }
        shell(|s| {
            let Some(session) = s.sessions.get_mut(&sid) else { return };
            match kind {
                "title" => session.meta.title = data.as_str().unwrap_or("").to_owned(),
                "cwd" => session.meta.cwd = data.as_str().unwrap_or("").to_owned(),
                "bell" => session.bell = !active,
                "badge" => session.badge = data.as_str().unwrap_or("").to_owned(),
                "command" => session.last_exit = data.as_i64().or_else(|| data.as_str().and_then(|t| t.parse().ok())).unwrap_or(0),
                "output" => {
                    if !active {
                        session.activity = true;
                    }
                }
                _ => {}
            }
        });
    }
    fn on_infos(&mut self, list: &Value) {
        shell(|s| {
            for item in list.as_array().into_iter().flatten() {
                let Some(sid) = item["sid"].as_i64() else { continue };
                if let Some(session) = s.sessions.get_mut(&sid) {
                    let info = &item["info"];
                    session.meta.process = info["process"].as_str().unwrap_or("").to_owned();
                    session.meta.busy = info["busy"] == true;
                    session.info = info.clone();
                }
            }
        });
    }
    fn apply(&mut self, done: Completed<Tag>) {
        match done.tag {
            Tag::Create { purpose, profile, wid } => {
                if let Some(sid) = done.result.ok().and_then(|v| v.as_i64()) {
                    self.on_created(purpose, profile, wid, sid);
                }
            }
            Tag::Infos => {
                self.infos_pending = false;
                if let Ok(list) = done.result {
                    self.on_infos(&list);
                }
            }
            Tag::Events => {
                self.events_pending = false;
                for event in done.result.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default() {
                    self.on_event(&event);
                }
            }
            Tag::Quiet => {}
        }
    }
    fn timers(&mut self, now: i64) {
        self.expire_closed(now);
        let sids = shell(|s| s.ws.sessions(&self.window));
        if sids.is_empty() {
            return;
        }
        if !self.infos_pending && now >= self.info_due {
            self.infos_pending = true;
            self.info_due = now + INFO_MS;
            self.jobs.enqueue(Tag::Infos, "term.infos", json!([sids]));
        }
        if !self.events_pending && now >= self.events_due {
            self.events_pending = true;
            self.events_due = now + EVENTS_MS;
            self.jobs.enqueue(Tag::Events, "term.events", json!([]));
        }
    }

    // ---- geometry ----

    fn status_height(&self) -> f64 {
        let Some(sid) = shell(|s| s.ws.current_sid(&self.window)) else { return 0. };
        if self.session_profile(sid)["statusBar"] == true { ui::STATUS_BAR } else { 0. }
    }
    fn area(&self) -> Rect {
        Rect { x: 0., y: 0., w: self.width, h: (self.height - ui::TAB_BAR - self.status_height()).max(0.) }
    }
    fn pane_rect(&self, sid: Sid) -> Option<Rect> {
        let tab = shell(|s| s.ws.window(&self.window).and_then(|w| w.active_tab().cloned()))?;
        Workspace::layout(&tab, self.area()).0.into_iter().find(|(s, _)| *s == sid).map(|(_, r)| r)
    }
    fn local(&self, sid: Sid, v: &Value) -> Option<(f64, f64)> {
        let r = self.pane_rect(sid)?;
        Some((number(&v["x"], 0.) - r.x, number(&v["y"], 0.) - ui::TAB_BAR - r.y))
    }
    fn mods(v: &Value) -> i64 {
        i64::from(v["shift"] == true) | (i64::from(v["alt"] == true) << 1) | (i64::from(v["ctrl"] == true) << 2) | (i64::from(v["meta"] == true) << 3)
    }

    // ---- actions ----

    fn go_pane(&mut self, dx: i32, dy: i32) {
        let area = self.area();
        let target = shell(|s| {
            let tab = s.ws.window(&self.window)?.active_tab()?.clone();
            Workspace::neighbor(&tab, area, tab.focus, dx, dy)
        });
        if let Some(sid) = target {
            self.focus(sid);
        }
    }
    fn cycle_pane(&mut self, delta: i64) {
        let target = shell(|s| {
            let tab = s.ws.window(&self.window)?.active_tab()?;
            let leaves = tab.root.leaves();
            let at = leaves.iter().position(|l| *l == tab.focus)? as i64;
            Some(leaves[(at + delta).rem_euclid(leaves.len() as i64) as usize])
        });
        if let Some(sid) = target {
            self.focus(sid);
        }
    }
    fn focus(&mut self, sid: Sid) {
        shell(|s| {
            s.ws.focus_pane(sid);
            if let Some(session) = s.sessions.get_mut(&sid) {
                session.bell = false;
                session.activity = false;
            }
        });
    }
    fn select_tab(&mut self, id: &str) {
        if let Some(sid) = shell(|s| s.ws.select_tab(&self.window, id)) {
            shell(|s| {
                if let Some(session) = s.sessions.get_mut(&sid) {
                    session.bell = false;
                    session.activity = false;
                }
            });
        }
    }
    fn close_tab(&mut self, id: &str) {
        let sids = shell(|s| s.ws.window(&self.window).and_then(|w| w.tabs.iter().find(|t| t.id == id).map(|t| t.root.leaves())));
        for sid in sids.unwrap_or_default() {
            self.remove_session(sid);
        }
    }
    /// A shortcut or menu action. False: not handled here (yet), the key goes on.
    fn run(&mut self, action: &str, now: i64) -> bool {
        let sid = shell(|s| s.ws.current_sid(&self.window));
        if let Some(n) = action.strip_prefix("tab").and_then(|d| d.parse::<usize>().ok()).filter(|n| (1..=9).contains(n)) {
            let id = shell(|s| s.ws.window(&self.window).and_then(|w| w.tabs.get(n - 1).map(|t| t.id.clone())));
            if let Some(id) = id {
                self.select_tab(&id);
            }
            return true;
        }
        match action {
            "newTab" => {
                let id = self.default_profile();
                self.create(Purpose::Tab, &id, None);
            }
            "splitRight" | "splitDown" => {
                let profile = sid.map(|s| self.session_profile(s)["id"].as_str().unwrap_or("").to_owned()).unwrap_or_default();
                self.create(if action == "splitRight" { Purpose::Split(Dir::Row) } else { Purpose::Split(Dir::Column) }, &profile, None);
            }
            "closePane" => {
                if let Some(sid) = sid {
                    self.close_pane(sid, now);
                }
            }
            "closeTab" => {
                let id = shell(|s| s.ws.window(&self.window).and_then(|w| w.active_tab().map(|t| t.id.clone())));
                if let Some(id) = id {
                    self.close_tab(&id);
                }
            }
            "nextPane" => self.cycle_pane(1),
            "prevPane" => self.cycle_pane(-1),
            "paneLeft" => self.go_pane(-1, 0),
            "paneRight" => self.go_pane(1, 0),
            "paneUp" => self.go_pane(0, -1),
            "paneDown" => self.go_pane(0, 1),
            "nextTab" => {
                shell(|s| s.ws.cycle_tab(&self.window, 1));
            }
            "prevTab" => {
                shell(|s| s.ws.cycle_tab(&self.window, -1));
            }
            "moveTabLeft" | "moveTabRight" => {
                let delta = if action == "moveTabLeft" { -1 } else { 1 };
                shell(|s| {
                    if let Some(id) = s.ws.window(&self.window).and_then(|w| w.active_tab().map(|t| t.id.clone())) {
                        s.ws.move_tab(&self.window, &id, delta);
                    }
                });
            }
            "maximizePane" => shell(|s| s.ws.toggle_zoom(&self.window)),
            "equalize" => shell(|s| s.ws.equalize(&self.window)),
            "paneToTab" => {
                shell(|s| s.ws.pane_to_tab(&self.window));
            }
            "undoClose" => {
                let window = self.window.clone();
                if let Some((_, sid)) = shell(|s| {
                    let back = s.ws.undo_close(&window);
                    if let Some((_, sid)) = &back {
                        s.closing.retain(|(c, _)| c != sid);
                    }
                    back
                }) {
                    let _ = sid;
                }
            }
            "zoomIn" | "zoomOut" | "zoomReset" => {
                let delta = match action {
                    "zoomIn" => 1,
                    "zoomOut" => -1,
                    _ => 0,
                };
                if shell(|s| s.ws.zoom_font(&self.window, delta)) {
                    self.configure_window();
                }
            }
            "timestamps" => {
                shell(|s| s.ws.toggle_timestamps(&self.window));
                self.configure_window();
            }
            "clearBuffer" | "reset" => {
                if let Some(sid) = sid {
                    self.effect("clear", sid, json!({"what":if action == "reset" { "reset" } else { "buffer" }}));
                }
            }
            "selectAll" => {
                if let Some(sid) = sid {
                    self.effect("select", sid, json!({"what":"all"}));
                }
            }
            "scrollTop" | "scrollBottom" | "scrollPageUp" | "scrollPageDown" | "scrollLineUp" | "scrollLineDown" => {
                let what = match action {
                    "scrollTop" => json!("top"),
                    "scrollBottom" => json!("bottom"),
                    "scrollPageUp" => json!("pageUp"),
                    "scrollPageDown" => json!("pageDown"),
                    "scrollLineUp" => json!(1),
                    _ => json!(-1),
                };
                if let Some(sid) = sid {
                    self.effect("scroll", sid, json!({"what":what}));
                }
            }
            "prevMark" | "nextMark" | "setMark" => {
                if let Some(sid) = sid {
                    self.effect("mark", sid, json!({"what":match action { "prevMark" => "prev", "nextMark" => "next", _ => "set" }}));
                }
            }
            _ => return false,
        }
        true
    }

    // ---- keyboard ----

    fn key_at(&mut self, key: &str, chr: &str, flags: i32, facts: &KeyFacts, now: i64) -> Option<Value> {
        let sid = shell(|s| s.ws.current_sid(&self.window))?;
        let event = json!({"code":facts.code,"key":key,"meta":flags & 16 != 0,"ctrl":flags & 2 != 0,"alt":flags & 4 != 0,"shift":facts.shift});
        let modifier = MODIFIER_CODES.contains(&facts.code);
        let profile = self.session_profile(sid);
        if !modifier {
            let mapping = model(&json!({"cmd":"keyMapping","event":event,"profile":{"keyMap":profile["keyMap"]},"keyMap":self.raw()["keyMap"]}));
            if mapping.is_object() {
                let bytes = model(&json!({"cmd":"mappingBytes","mapping":mapping}));
                match bytes["kind"].as_str() {
                    // raw bytes, or UTF-16 units (a mapping may hold a lone surrogate JSON strings cannot carry)
                    Some("hex") => self.to_session("write", json!({"bytes":bytes["bytes"]})),
                    Some("text") => self.to_session("write", json!({"units":bytes["units"]})),
                    _ if bytes["error"].is_null() && mapping["action"] != "ignore" => {
                        self.run(mapping["action"].as_str().unwrap_or(""), now);
                    }
                    _ => {}
                }
                return Some(self.out(now));
            }
        }
        let action = model(&json!({"cmd":"action","event":event,"shortcuts":self.raw()["shortcuts"]}));
        if let Some(action) = action.as_str() {
            self.run(action, now);
            return Some(self.out(now));
        }
        if modifier || flags & 16 != 0 {
            return None;
        }
        let named = if key.chars().count() > 1 { key } else { "" };
        let base = if flags & (2 | 4) != 0 { facts.base } else { "" };
        if named.is_empty() && chr.is_empty() && base.is_empty() {
            return None;
        }
        let mods = i64::from(facts.shift) | (i64::from(flags & 4 != 0) << 1) | (i64::from(flags & 2 != 0) << 2);
        self.to_session("key", json!({"key":named,"char":chr,"base":base,"mods":mods}));
        Some(self.out(now))
    }

    // ---- requests ----

    fn command(&mut self, v: &Value, now: i64) {
        let key = &v["key"];
        match v["cmd"].as_str().unwrap_or("") {
            "tab" => {
                if let Some(id) = key.as_str() {
                    self.select_tab(id);
                }
            }
            "closeTab" => {
                if let Some(id) = key.as_str() {
                    self.close_tab(id);
                }
            }
            "newTab" => {
                self.run("newTab", now);
            }
            "paneDown" => {
                let Some(sid) = key.as_i64() else { return };
                self.focus(sid);
                if v["button"].as_i64() == Some(2) {
                    return;
                }
                if let Some((x, y)) = self.local(sid, v) {
                    self.effect("mouse", sid, json!({"type":"down","x":x,"y":y,"button":v["button"].as_i64().unwrap_or(0),"mods":Self::mods(v)}));
                    self.mouse = Some(sid);
                    self.capture = Some(true);
                }
            }
            "paneMove" => {
                let Some(sid) = key.as_i64() else { return };
                if self.mouse.is_some() {
                    return;
                }
                if let Some((x, y)) = self.local(sid, v) {
                    let (px, py) = (x.round() as i64, y.round() as i64);
                    let last = self.last_move;
                    if (last.0, last.1, last.2) != (sid, px, py) && now - last.3 >= MOVE_MS {
                        self.last_move = (sid, px, py, now);
                        self.effect("mouse", sid, json!({"type":"move","x":x,"y":y,"button":0,"mods":Self::mods(v)}));
                    }
                }
            }
            "paneWheel" => {
                let (Some(sid), Some(notches)) = (key.as_i64(), v["notches"].as_i64().filter(|n| *n != 0)) else { return };
                if let Some((x, y)) = self.local(sid, v) {
                    self.effect("wheel", sid, json!({"notches":notches * 3,"x":x,"y":y,"mods":Self::mods(v)}));
                }
            }
            "divider" => {
                let Some(split) = key.as_str() else { return };
                let area = self.area();
                let found = shell(|s| {
                    let tab = s.ws.window(&self.window)?.active_tab()?.clone();
                    let (_, dividers) = Workspace::layout(&tab, area);
                    dividers.into_iter().find(|d| d.split == split).map(|d| (tab.id, d.parent))
                });
                if let Some((tab, parent)) = found {
                    self.drag = Some(DividerDrag { tab, split: split.to_owned(), parent });
                    self.capture = Some(true);
                }
            }
            "pointerMove" => {
                if let Some(drag) = &self.drag {
                    let area = self.area();
                    let dir = shell(|s| {
                        let tab = s.ws.window(&self.window)?.active_tab()?.clone();
                        Workspace::layout(&tab, area).1.into_iter().find(|d| d.split == drag.split).map(|d| d.dir)
                    });
                    let ratio = match dir {
                        Some(Dir::Row) => (number(&v["x"], 0.) - drag.parent.x) / drag.parent.w.max(1.),
                        Some(Dir::Column) => (number(&v["y"], 0.) - ui::TAB_BAR - drag.parent.y) / drag.parent.h.max(1.),
                        None => return,
                    };
                    let (tab, split) = (drag.tab.clone(), drag.split.clone());
                    shell(|s| s.ws.set_ratio(&self.window, &tab, &split, ratio));
                } else if let Some(sid) = self.mouse {
                    if let Some((x, y)) = self.local(sid, v) {
                        self.effect("mouse", sid, json!({"type":"move","x":x,"y":y,"button":0,"mods":Self::mods(v)}));
                    }
                }
            }
            "dragEnd" => {
                self.capture = Some(false);
                if self.drag.take().is_some() {
                    return;
                }
                if let Some(sid) = self.mouse.take() {
                    if let Some((x, y)) = self.local(sid, v) {
                        self.effect("mouse", sid, json!({"type":"up","x":x,"y":y,"button":v["button"].as_i64().unwrap_or(0),"mods":Self::mods(v),"rememberCopy":true}));
                    }
                }
            }
            _ => {}
        }
    }
    pub fn request_at(&mut self, v: &Value, now: i64) -> Value {
        match v["cmd"].as_str().unwrap_or("") {
            "icon" => return ui::icon(number(&v["size"], 32.)),
            "sync" => self.context(v, now),
            "next" => {
                for done in self.jobs.complete(&v["result"], now) {
                    self.apply(done);
                }
            }
            "noop" => {}
            _ => self.command(v, now),
        }
        self.after(now);
        let mut out = self.out(now);
        if v["cmd"] == "sync" {
            let tree = self.scene(now);
            if tree != self.last_tree {
                self.last_tree = tree.clone();
                out["tree"] = tree;
            } else {
                out["same"] = json!(true);
            }
        }
        out
    }
    /// Work every request ends with: timers, keyboard focus of the sessions.
    fn after(&mut self, now: i64) {
        if !self.booted {
            return;
        }
        self.timers(now);
        let wanted = if self.focused { shell(|s| s.ws.current_sid(&self.window)) } else { None };
        if wanted != self.focus_sid {
            if let Some(old) = self.focus_sid {
                self.effect("focus", old, json!({"on":false}));
            }
            if let Some(new) = wanted {
                self.effect("focus", new, json!({"on":true}));
            }
            self.focus_sid = wanted;
        }
    }
    fn out(&mut self, now: i64) -> Value {
        let mut out = json!({});
        if let Some(effect) = self.jobs.pump(now) {
            out["effect"] = effect;
        }
        if !self.effects.is_empty() {
            out["effects"] = json!(std::mem::take(&mut self.effects));
        }
        if let Some(on) = self.capture.take() {
            out["capture"] = json!(on);
        }
        if std::mem::take(&mut self.close_window) {
            let close = json!({"kind":"closeWindow","id":self.window});
            match out["effects"].as_array_mut() {
                Some(list) => list.push(close),
                None => out["effects"] = json!([close]),
            }
        }
        let title = shell(|s| {
            let tab = s.ws.window(&self.window)?.active_tab()?;
            let meta: HashMap<Sid, Meta> = s.sessions.iter().map(|(k, v)| (*k, v.meta.clone())).collect();
            Some(crate::workspace::tab_title(tab, &meta))
        });
        if let Some(title) = title.filter(|t| self.announced.as_ref() != Some(t)) {
            out["title"] = json!(title);
            self.announced = Some(title);
        }
        out
    }
    fn context(&mut self, v: &Value, now: i64) {
        if v["context"]["theme"].is_object() {
            self.theme = Rc::new(v["context"]["theme"].clone());
        }
        self.width = number(&v["context"]["width"], self.width).max(1.);
        self.height = number(&v["context"]["height"], self.height).max(1.);
        self.focused = v["context"]["focused"].as_bool().unwrap_or(self.focused);
        if v["settings"]["terminal"].is_object() {
            self.settings = v["settings"]["terminal"].clone();
        }
        if self.booted {
            return;
        }
        let Some(owner) = v["args"]["__nativeOwner"].as_i64().filter(|i| *i > 0) else { return };
        self.booted = true;
        self.jobs.bind(owner);
        self.window = v["args"]["__nativeWindow"].as_str().unwrap_or("terminal").to_owned();
        self.home = v["context"]["home"].as_str().map(str::to_owned).or_else(|| std::env::var("HOME").ok()).unwrap_or_default();
        self.info_due = now + INFO_MS;
        self.events_due = now;
        let known = shell(|s| s.ws.window(&self.window).is_some());
        if !known {
            let id = self.default_profile();
            let cwd = v["args"]["cwd"].as_str().filter(|d| d.starts_with('/')).map(str::to_owned);
            self.create(Purpose::Window, &id, cwd);
        } else {
            self.configure_window();
        }
    }
    fn scene(&mut self, now: i64) -> Value {
        let Some((tabs, active, font)) = shell(|s| {
            let w = s.ws.window(&self.window)?;
            Some((w.tabs.clone(), w.active.clone(), w.font_delta))
        }) else {
            return ui::tree(&ui::Facts { theme: &self.theme, pane_bg: "#000000".into(), width: self.width, tabs: vec![], broadcast: false,
                panes: vec![], dividers: &[], status: None });
        };
        let _ = font;
        let tab = tabs.iter().find(|t| t.id == active).or_else(|| tabs.first()).cloned().unwrap();
        let focus_profile = self.session_profile(tab.focus);
        let scheme = model(&json!({"cmd":"scheme","settings":{"schemes":self.raw()["schemes"]},"profile":focus_profile}));
        let pane_bg = scheme["bg"].as_str().unwrap_or("#000000").to_owned();
        let area = self.area();
        let (rects, dividers) = Workspace::layout(&tab, area);
        let multi = rects.len() > 1;
        let tab_facts: Vec<ui::TabFacts> = shell(|s| {
            let meta: HashMap<Sid, Meta> = s.sessions.iter().map(|(k, v)| (*k, v.meta.clone())).collect();
            tabs.iter()
                .map(|t| {
                    let leaves = t.root.leaves();
                    let color = s.sessions.get(&t.focus).map(|x| self.profile(&x.profile)["tabColor"].clone()).and_then(|c| c.as_str().filter(|c| !c.is_empty()).map(str::to_owned));
                    let any = |f: &dyn Fn(&Session) -> bool| leaves.iter().any(|sid| s.sessions.get(sid).is_some_and(f));
                    ui::TabFacts {
                        id: t.id.clone(),
                        title: crate::workspace::tab_title(t, &meta),
                        active: t.id == tab.id,
                        bell: any(&|x| x.bell),
                        activity: any(&|x| x.activity),
                        failed: s.sessions.get(&t.focus).is_some_and(|x| x.last_exit > 0),
                        color,
                    }
                })
                .collect()
        });
        let panes: Vec<ui::PaneFacts> = rects
            .iter()
            .map(|(sid, rect)| {
                let session = shell(|s| s.sessions.get(sid).cloned()).unwrap_or_default();
                let profile = self.profile(&session.profile);
                ui::PaneFacts { sid: *sid, rect: *rect, dim: multi && !tab.zoom && *sid != tab.focus && profile["dimInactive"] == true, badge: session.badge }
            })
            .collect();
        let status = (focus_profile["statusBar"] == true).then(|| {
            let session = shell(|s| s.sessions.get(&tab.focus).cloned()).unwrap_or_default();
            let info = &session.info;
            let cwd = if session.meta.cwd.is_empty() { info["cwd"].as_str().unwrap_or("").to_owned() } else { session.meta.cwd.clone() };
            let cwd = if self.home.is_empty() { cwd } else { cwd.replacen(&self.home, "~", 1) };
            let local = clock::local(now);
            ui::StatusFacts {
                cwd,
                program: if info["busy"] == true { info["process"].as_str().unwrap_or("").to_owned() } else { info["shell"].as_str().unwrap_or("").to_owned() },
                busy: info["busy"] == true,
                last_exit: session.last_exit,
                history: info["view"].as_i64().unwrap_or(0),
                alt_screen: info["altScreen"] == true,
                size: (info["cols"].as_i64().unwrap_or(0), info["rows"].as_i64().unwrap_or(0)),
                clock: format!("{:02}:{:02}:{:02}", local.tm_hour, local.tm_min, local.tm_sec),
            }
        });
        let theme = self.theme.clone();
        ui::tree(&ui::Facts { theme: &theme, pane_bg, width: self.width, tabs: tab_facts, broadcast: false, panes, dividers: &dividers, status })
    }
}

#[cfg(test)]
#[path = "native_window_tests.rs"]
mod tests;
