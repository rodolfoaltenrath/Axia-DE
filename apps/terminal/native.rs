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
use crate::native_view::{async_jobs::{Completed, Jobs}, clock, i18n::I18n, KeyFacts, NativeApp};
use crate::request as model;
use crate::workspace::{session_title, Dir, Meta, Rect, Removed, Sid, Workspace};
use serde_json::{json, Map, Value};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[path = "native_ui.rs"]
mod ui;
#[path = "triggers.rs"]
mod triggers;
#[path = "prefs.rs"]
mod prefs;
#[path = "prefs_ui.rs"]
mod prefs_ui;

const INFO_MS: i64 = 1500;
const EVENTS_MS: i64 = 250;
const UNDO_MS: i64 = 5000;
const MOVE_MS: i64 = 8;
const FLASH_MS: i64 = 140;
const CAPTURED_MAX: usize = 100;
const TOAST_MS: i64 = 4000;
const TOASTS_MAX: usize = 4;
const MATCHES_MAX: usize = 60000;
const BELT_WIDTH: f64 = 240.;
const COMMANDS_MAX: usize = 200;
const DIRS_MAX: usize = 25;
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
    /// Text typed once the shell has shown its prompt (a profile's, or an ssh line from another app).
    pending_text: String,
    /// Lines of the screen the triggers already looked at, and whether a look is on its way.
    seen: std::collections::HashSet<String>,
    scan_busy: bool,
    /// "Alert when the command finishes", the last command, the visual-bell flash.
    alert: bool,
    last_cmd: String,
    flash_until: i64,
    /// `user@host` as the shell reports it, the badge the program set and the profile before a rule changed it.
    host: String,
    osc_badge: String,
    auto_from: Option<String>,
}
#[derive(Clone, Default)]
struct Find {
    sid: Sid,
    query: String,
    cs: bool,
    regex: bool,
    count: i64,
    index: i64,
    error: bool,
}
#[derive(Clone)]
enum Confirm {
    Pane(Sid),
    Tab(String),
    Window,
}
#[derive(Clone)]
struct Dialog {
    text: Value,
    detail: Value,
    confirm: Confirm,
}
#[derive(Clone)]
struct Toast {
    text: Value,
    until: i64,
}
/// What a window shows on top of its panes. It lives with the workspace, like the tabs.
#[derive(Clone)]
enum Popup {
    Complete { prefix: String, items: Vec<String>, sel: usize },
    Paste { items: Vec<String>, raw: Vec<String>, sel: usize },
    History { items: Vec<String>, sel: usize },
    Quick { q: String, sel: usize },
    Title { value: String },
}
/// A pane's context menu: where it opened and for which pane.
#[derive(Clone)]
struct PaneMenu {
    sid: Sid,
    x: f64,
    y: f64,
}
#[derive(Clone, Default)]
struct Replay {
    sid: Sid,
    index: i64,
    count: i64,
    time: i64,
}
#[derive(Clone, Default)]
struct WinUi {
    menu: Option<PaneMenu>,
    popup: Option<Popup>,
    composer: Option<String>,
    broadcast: bool,
    toolbelt: bool,
    note: String,
    copy_mode: Option<Sid>,
    replay: Option<Replay>,
    clipboard: Vec<String>,
    find: Option<Find>,
    find_focus: u64,
    dialog: Option<Dialog>,
    toasts: Vec<Toast>,
    prefs: Option<prefs::Prefs>,
}
#[derive(Clone, Copy)]
enum PasteMode {
    Plain,
    OneLine,
    Escaped,
}
#[derive(Default)]
struct Shell {
    recent_dirs: Vec<String>,
    commands: Vec<String>,
    notes: Vec<String>,
    ui: HashMap<String, WinUi>,
    /// The installed fonts (family, PostScript name, monospaced, color), asked once for the Preferences.
    fonts: Option<Vec<Value>>,
    fonts_asked: bool,
    /// Lines the capture triggers took, newest first: (session, text).
    captured: Vec<(Sid, String)>,
    ws: Workspace,
    sessions: HashMap<Sid, Session>,
    /// Closed panes waiting out their undo window: (session, closes at).
    closing: Vec<(Sid, i64)>,
}
thread_local! { static SHELL: RefCell<Shell> = RefCell::new(Shell::default()); }
fn shell<R>(f: impl FnOnce(&mut Shell) -> R) -> R {
    SHELL.with(|s| f(&mut s.borrow_mut()))
}

/// What an event leaves to do after the shared state was changed.
enum Follow {
    Scan,
    Rules,
    Finished(i64, String),
}
/// This machine as the profile rules see it: the user and the host name.
fn local_identity() -> (&'static str, &'static str) {
    static IDENTITY: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
    let id = IDENTITY.get_or_init(|| {
        let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
        let host = std::fs::read_to_string("/proc/sys/kernel/hostname").map(|h| h.trim().to_owned()).or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default();
        (user, host)
    });
    (&id.0, &id.1)
}

#[derive(Clone)]
enum Purpose {
    Window,
    Tab,
    Split(Dir),
}
enum Tag {
    Create { purpose: Purpose, profile: String, wid: String },
    Copied,
    Url { sid: Sid, x: f64, y: f64, mods: i64 },
    Log { on: bool },
    Export,
    Prefix,
    Words { prefix: String },
    PasteList,
    ClipList,
    ReplayState { sid: Sid, entering: bool },
    CopyKey,
    Pasted(PasteMode),
    Output,
    Found,
    Logical { query: String, cs: bool, sid: Sid },
    Quiet,
    Infos,
    Events,
    Fonts,
    Screen { sid: Sid },
    OscCopy { sid: Sid },
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
    i18n: I18n,
    locale_revision: u64,
    menus: Value,
    guard: Option<bool>,
    last_args: Value,
    first_typed: Option<String>,
    slide: Option<prefs::Slide>,
    reconfig: bool,
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
            i18n: I18n::default(),
            locale_revision: 0,
            menus: Value::Null,
            guard: None,
            last_args: Value::Null,
            first_typed: None,
            slide: None,
            reconfig: false,
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
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
    }
    fn sync_with_preferences(&mut self, v: &Value, preferences: &Value) -> Value {
        self.settings = preferences["terminal"].clone();
        self.request_at(v, clock::now())
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted.then_some(if self.jobs.busy() { 50. } else { 200. })
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["settings"]["terminal"].is_object() && !self.reconfig {
            self.settings = input["settings"]["terminal"].clone();
        }
        self.request_at(input, clock::now())
    }
    fn key_event(&mut self, key: &str, chr: &str, flags: i32, facts: &KeyFacts) -> Option<Value> {
        self.key_at(key, chr, flags, facts, clock::now())
    }
}
/// The only lines another app may leave typed in a new window: `ssh` or `sftp` to a host.
fn ssh_line(text: &str) -> bool {
    regex::Regex::new(r"^(ssh|sftp)( -[pP] \d{1,5})? [A-Za-z0-9_][A-Za-z0-9._@-]{0,300}$").map(|re| re.is_match(text)).unwrap_or(false)
}
/// The matches of a regular expression in the logical lines of a session, as the engine takes
/// them: (line, first character, character count) triples, at most MATCHES_MAX numbers.
fn regex_matches(text: &str, query: &str, case_sensitive: bool) -> Option<Vec<i64>> {
    let re = regex::RegexBuilder::new(query).case_insensitive(!case_sensitive).unicode(true).build().ok()?;
    let mut trip: Vec<i64> = vec![];
    for (i, line) in text.split('\n').enumerate() {
        if line.is_empty() || trip.len() >= MATCHES_MAX {
            continue;
        }
        for m in re.find_iter(line) {
            if m.as_str().is_empty() {
                continue;
            }
            if trip.len() >= MATCHES_MAX {
                break;
            }
            trip.extend([i as i64, line[..m.start()].chars().count() as i64, m.as_str().chars().count() as i64]);
        }
    }
    Some(trip)
}
impl App {
    fn ui<R>(&self, f: impl FnOnce(&mut WinUi) -> R) -> R {
        shell(|s| f(s.ui.entry(self.window.clone()).or_default()))
    }
    fn toast(&mut self, text: Value, now: i64) {
        self.ui(|u| {
            u.toasts.push(Toast { text, until: now + TOAST_MS });
            let extra = u.toasts.len().saturating_sub(TOASTS_MAX);
            u.toasts.drain(..extra);
        });
    }
    /// Closing something that runs a program asks first (the profile can turn that off).
    fn ask_then(&mut self, sids: &[Sid], what: &str, confirm: Confirm, now: i64) {
        let mut busy: Vec<String> = vec![];
        for sid in sids {
            let (alive, running, process) = shell(|s| {
                s.sessions.get(sid).map(|x| (x.info["alive"] == true, x.info["busy"] == true, x.info["process"].as_str().unwrap_or("").to_owned()))
            })
            .unwrap_or_default();
            if alive && running && self.session_profile(*sid)["confirmClose"] == true && !busy.contains(&process) {
                busy.push(process);
            }
        }
        if busy.is_empty() {
            self.confirmed(confirm, now);
            return;
        }
        self.ui(|u| {
            u.dialog = Some(Dialog {
                text: json!({"key":if what == "window" { "terminal.dialog.closeWindow".to_owned() } else { format!("terminal.dialog.close.{what}") }}),
                detail: json!({"key":"terminal.dialog.busy","params":{"list":busy.join(", ")}}),
                confirm,
            })
        });
    }
    fn confirmed(&mut self, confirm: Confirm, now: i64) {
        match confirm {
            Confirm::Pane(sid) => {
                self.close_pane(sid, now);
                self.toast(json!({"key":"terminal.toast.paneClosed"}), now);
            }
            Confirm::Tab(id) => self.close_tab(&id),
            Confirm::Window => self.effects.push(json!({"kind":"closeWindow","id":self.window,"force":true})),
        }
    }
    fn copy(&mut self) {
        if let Some(sid) = shell(|s| s.ws.current_sid(&self.window)) {
            self.jobs.enqueue(Tag::Copied, "term.copy", json!([sid]));
        }
    }
    fn paste(&mut self, mode: PasteMode) {
        self.jobs.enqueue(Tag::Pasted(mode), "clipboard.read", json!([]));
    }
    fn popup_key(&mut self, popup: &Popup, key: &str, now: i64) -> bool {
        let (len, sel) = match popup {
            Popup::Complete { items, sel, .. } | Popup::History { items, sel } | Popup::Paste { items, sel, .. } => (items.len(), *sel),
            // the open-quickly and title popups are fields: they take their own keys
            _ => return false,
        };
        match key {
            "Escape" => self.ui(|u| u.popup = None),
            "ArrowUp" | "ArrowDown" => {
                let next = (sel as i64 + if key == "ArrowUp" { -1 } else { 1 }).rem_euclid(len.max(1) as i64) as usize;
                self.ui(|u| match u.popup.as_mut() {
                    Some(Popup::Complete { sel, .. } | Popup::History { sel, .. } | Popup::Paste { sel, .. }) => *sel = next,
                    _ => {}
                });
            }
            "Enter" => {
                match popup {
                    Popup::Complete { prefix, items, sel } => {
                        if let Some(item) = items.get(*sel) {
                            let rest: String = item.chars().skip(prefix.chars().count()).collect();
                            self.to_session("write", json!({"text":rest}));
                        }
                    }
                    Popup::Paste { raw, sel, .. } => {
                        if let Some(text) = raw.get(*sel) {
                            self.to_session("paste", json!({"text":text}));
                        }
                    }
                    Popup::History { items, sel } => {
                        if let Some(cmd) = items.get(*sel) {
                            self.to_session("write", json!({"text":cmd}));
                        }
                    }
                    _ => {}
                }
                self.ui(|u| u.popup = None);
            }
            _ => {}
        }
        let _ = now;
        true
    }
    fn replay_exit(&mut self) {
        if let Some(replay) = self.ui(|u| u.replay.take()) {
            self.jobs.enqueue(Tag::Quiet, "term.replay", json!([replay.sid, "exit"]));
        }
    }
    fn replay_step(&mut self, replay: &Replay, delta: i64) {
        let index = (replay.index + delta).clamp(0, (replay.count - 1).max(0));
        self.jobs.enqueue(Tag::ReplayState { sid: replay.sid, entering: false }, "term.replay", json!([replay.sid, index]));
    }
    fn quick_items(&self, q: &str) -> Vec<(String, Sid, String)> {
        let q = q.to_lowercase();
        shell(|s| {
            let meta: HashMap<Sid, Meta> = s.sessions.iter().map(|(k, v)| (*k, v.meta.clone())).collect();
            s.ws.windows()
                .flat_map(|(wid, w)| w.tabs.iter().flat_map(move |t| t.root.leaves().into_iter().map(move |sid| (wid.clone(), sid))))
                .map(|(wid, sid)| (wid, sid, session_title(meta.get(&sid))))
                .filter(|(_, _, title)| title.to_lowercase().contains(&q))
                .collect()
        })
    }
    /// Gives a pane (of this or another window) the focus.
    fn focus_anywhere(&mut self, sid: Sid) {
        let wid = shell(|s| s.ws.find_session(sid).map(|(w, _)| w.to_owned()));
        self.focus(sid);
        if let Some(wid) = wid.filter(|w| *w != self.window) {
            self.effects.push(json!({"kind":"focusWindow","id":wid}));
        }
    }
    fn find_open(&mut self) {
        let Some(sid) = shell(|s| s.ws.current_sid(&self.window)) else { return };
        let fresh = self.ui(|u| {
            u.find_focus += 1;
            if u.find.is_none() {
                u.find = Some(Find { sid, index: -1, ..Default::default() });
                true
            } else {
                false
            }
        });
        let _ = fresh;
    }
    /// Searches the current query in the session of the bar.
    fn find_run(&mut self) {
        let Some(find) = self.ui(|u| u.find.clone()) else { return };
        if find.query.is_empty() {
            self.jobs.enqueue(Tag::Found, "term.search", json!([find.sid, "", false]));
        } else if find.regex {
            self.jobs.enqueue(Tag::Logical { query: find.query, cs: find.cs, sid: find.sid }, "term.logicalText", json!([find.sid]));
        } else {
            self.jobs.enqueue(Tag::Found, "term.search", json!([find.sid, find.query, find.cs]));
        }
    }
    fn find_move(&mut self, delta: i64) {
        if let Some(find) = self.ui(|u| u.find.clone()) {
            self.jobs.enqueue(Tag::Found, "term.searchMove", json!([find.sid, delta]));
        }
    }
    fn find_close(&mut self) {
        if let Some(find) = self.ui(|u| u.find.take()) {
            self.jobs.enqueue(Tag::Quiet, "term.search", json!([find.sid, "", false]));
        }
    }
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
        let purpose_kind = if matches!(purpose, Purpose::Window) { 'w' } else { 'o' };
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
            let profile_id = shell(|s| s.sessions.get(&sid).map(|x| x.profile.clone())).unwrap_or_default();
            let typed = self.first_typed.take().filter(|_| matches!(purpose_kind, 'w'));
            let initial = self.profile(&profile_id)["initialText"].as_str().unwrap_or("").to_owned();
            let pending = typed.unwrap_or(initial);
            if !pending.is_empty() {
                shell(|s| {
                    if let Some(session) = s.sessions.get_mut(&sid) {
                        session.pending_text = pending;
                    }
                });
            }
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
        for sid in self.targets() {
            self.effect(op, sid, args.clone());
        }
    }
    fn targets(&self) -> Vec<Sid> {
        let all = self.ui(|u| u.broadcast);
        shell(|s| {
            let Some(tab) = s.ws.window(&self.window).and_then(|w| w.active_tab()) else { return vec![] };
            if all { tab.root.leaves() } else { vec![tab.focus] }
        })
    }

    // ---- events ----

    fn is_active(&self, sid: Sid) -> bool {
        self.focused && shell(|s| s.ws.window(&self.window).and_then(|w| w.active_sid())) == Some(sid)
    }
    fn on_event(&mut self, event: &Value) {
        match event["kind"].as_str() {
            Some("action") => {
                let action = &event["action"];
                self.command(&json!({"cmd":action[0],"key":action[1]}), clock::now());
                return;
            }
            Some("closeRequested") => {
                let sids = shell(|s| s.ws.sessions(&self.window));
                self.ask_then(&sids, "window", Confirm::Window, clock::now());
                return;
            }
            _ => {}
        }
        let (Some(sid), Some(kind)) = (event["sid"].as_i64(), event["type"].as_str()) else { return };
        let data = &event["data"];
        let active = self.is_active(sid);
        let now = clock::now();
        if kind == "exit" {
            self.remove_session(sid);
            return;
        }
        // events that need no session state of their own
        match kind {
            "notify" => return self.toast_for(sid, json!(data.as_str().unwrap_or("")), now),
            "attention" => {
                self.toast_for(sid, json!({"key":"terminal.toast.attention"}), now);
                self.focus_window_of(sid);
                return;
            }
            "profile" => {
                let wanted = data.as_str().unwrap_or("");
                let found = self.terminal_settings()["profiles"].as_array().and_then(|l| l.iter().find(|p| p["name"] == wanted || p["id"] == wanted).cloned());
                if let Some(id) = found.and_then(|p| p["id"].as_str().map(str::to_owned)) {
                    self.set_session_profile(sid, &id);
                }
                return;
            }
            "copy" => return self.jobs.enqueue(Tag::OscCopy { sid }, "clipboard.read", json!([])),
            _ => {}
        }
        let profile = self.session_profile(sid);
        let bell_on = profile["bell"] != "none";
        let wants_scan = kind == "output" && profile["triggers"].as_array().is_some_and(|l| l.iter().any(|t| t["enabled"] != false));
        let mut pending: Option<String> = None;
        let mut follow: Vec<Follow> = vec![];
        shell(|s| {
            let Some(session) = s.sessions.get_mut(&sid) else { return };
            match kind {
                "title" => session.meta.title = data.as_str().unwrap_or("").to_owned(),
                "cwd" => {
                    session.meta.cwd = data.as_str().unwrap_or("").to_owned();
                    let dir = session.meta.cwd.clone();
                    if !dir.is_empty() {
                        s.recent_dirs.retain(|d| *d != dir);
                        s.recent_dirs.insert(0, dir);
                        s.recent_dirs.truncate(DIRS_MAX);
                        follow.push(Follow::Rules);
                    }
                }
                "host" => {
                    session.host = data.as_str().unwrap_or("").to_owned();
                    follow.push(Follow::Rules);
                }
                "run" => {
                    let cmd = data.as_str().unwrap_or("").to_owned();
                    if !cmd.is_empty() {
                        session.last_cmd = cmd.clone();
                        s.commands.retain(|c| *c != cmd);
                        s.commands.insert(0, cmd);
                        s.commands.truncate(COMMANDS_MAX);
                    }
                }
                "bell" => {
                    if bell_on {
                        session.bell = !active;
                        session.flash_until = now + FLASH_MS;
                    }
                }
                "badge" => {
                    session.badge = data.as_str().unwrap_or("").to_owned();
                    session.osc_badge = session.badge.clone();
                }
                "command" => {
                    let code = data.as_i64().or_else(|| data.as_str().and_then(|t| t.parse().ok())).unwrap_or(0);
                    session.last_exit = code;
                    if session.alert {
                        session.alert = false;
                        session.flash_until = now + FLASH_MS;
                        session.bell = !active;
                        follow.push(Follow::Finished(code, session.last_cmd.clone()));
                    }
                }
                "output" => {
                    if !active {
                        session.activity = true;
                    }
                    let text = std::mem::take(&mut session.pending_text);
                    if !text.is_empty() {
                        pending = Some(text);
                    }
                    if wants_scan && !session.scan_busy {
                        session.scan_busy = true;
                        follow.push(Follow::Scan);
                    }
                }
                _ => {}
            }
        });
        // the shell has shown something (its prompt): the text meant for it goes in now
        if let Some(text) = pending {
            self.effect("write", sid, json!({"text":text.replace("\\n", "\r").replace('\n', "\r")}));
        }
        for step in follow {
            match step {
                Follow::Scan => self.jobs.enqueue(Tag::Screen { sid }, "term.screenText", json!([sid])),
                Follow::Rules => self.auto_profile(sid),
                Follow::Finished(code, cmd) => {
                    let toast = if cmd.is_empty() {
                        json!({"key":"terminal.toast.finished","params":{"code":code}})
                    } else {
                        json!({"key":"terminal.toast.finishedCmd","params":{"cmd":cmd,"code":code}})
                    };
                    self.toast_for(sid, toast, now);
                }
            }
        }
    }
    /// The window a session lives in gets the toast (events arrive in whichever window polls).
    fn toast_for(&mut self, sid: Sid, text: Value, now: i64) {
        if let Some(wid) = shell(|s| s.ws.find_session(sid).map(|(wid, _)| wid.to_owned())) {
            shell(|s| {
                let u = s.ui.entry(wid).or_default();
                u.toasts.push(Toast { text, until: now + TOAST_MS });
                let extra = u.toasts.len().saturating_sub(TOASTS_MAX);
                u.toasts.drain(..extra);
            });
        }
    }
    fn focus_window_of(&mut self, sid: Sid) {
        if let Some(wid) = shell(|s| s.ws.find_session(sid).map(|(wid, _)| wid.to_owned())) {
            self.effects.push(json!({"kind":"focusWindow","id":wid}));
        }
    }
    /// A session takes a profile: its badge (or the one the program set) and its configuration follow.
    fn set_session_profile(&mut self, sid: Sid, id: &str) {
        let badge = self.profile(id)["badge"].as_str().unwrap_or("").to_owned();
        shell(|s| {
            if let Some(x) = s.sessions.get_mut(&sid) {
                x.profile = id.to_owned();
                x.badge = if badge.is_empty() { x.osc_badge.clone() } else { badge };
            }
        });
        self.configure(sid);
    }
    /// Automatic profile switching: the rules of the profiles against where the session is.
    fn auto_profile(&mut self, sid: Sid) {
        let Some((host, cwd, current, from)) = shell(|s| s.sessions.get(&sid).map(|x| (x.host.clone(), x.meta.cwd.clone(), x.profile.clone(), x.auto_from.clone()))) else { return };
        let (local_user, local_host) = local_identity();
        let (user, machine) = triggers::who(&host, local_user, local_host);
        let profiles = self.terminal_settings()["profiles"].as_array().cloned().unwrap_or_default();
        let at = triggers::Place { user, host: machine, cwd: &cwd, home: &self.home };
        let found = triggers::best_profile(&profiles, &at);
        match found {
            Some(id) if id != current => {
                shell(|s| {
                    if let Some(x) = s.sessions.get_mut(&sid) {
                        x.auto_from = from.or(Some(current));
                    }
                });
                self.set_session_profile(sid, &id);
                let name = profiles.iter().find(|p| p["id"] == id.as_str()).map(|p| self.i18n.caption(&prefs::profile_caption(p))).unwrap_or_default();
                self.toast_for(sid, json!({"key":"terminal.toast.autoProfile","params":{"name":name}}), clock::now());
            }
            None => {
                if let Some(back) = from {
                    shell(|s| {
                        if let Some(x) = s.sessions.get_mut(&sid) {
                            x.auto_from = None;
                        }
                    });
                    self.set_session_profile(sid, &back);
                }
            }
            Some(_) => {}
        }
    }
    /// The screen of a session as the profile's triggers see it: each new line fires them.
    fn run_triggers(&mut self, sid: Sid, screen: &str, now: i64) {
        let triggers = self.session_profile(sid)["triggers"].as_array().cloned().unwrap_or_default();
        let seen = shell(|s| s.sessions.get_mut(&sid).map(|x| std::mem::take(&mut x.seen))).unwrap_or_default();
        let (actions, lines) = triggers::evaluate(screen, &seen, &triggers);
        shell(|s| {
            if let Some(x) = s.sessions.get_mut(&sid) {
                x.seen = lines;
            }
        });
        for action in actions {
            match action {
                triggers::Action::Notify(text) => self.toast_for(sid, json!(text), now),
                triggers::Action::Bell => shell(|s| {
                    if let Some(x) = s.sessions.get_mut(&sid) {
                        x.bell = true;
                        x.flash_until = now + FLASH_MS;
                    }
                }),
                triggers::Action::Attention(text) => {
                    self.toast_for(sid, json!(text), now);
                    self.focus_window_of(sid);
                }
                triggers::Action::Send(text) => self.effect("write", sid, json!({"text":text})),
                triggers::Action::Capture(text) => shell(|s| {
                    s.captured.insert(0, (sid, text));
                    s.captured.truncate(CAPTURED_MAX);
                }),
                triggers::Action::Highlight { text, fg, bg } => self.jobs.enqueue(Tag::Quiet, "term.highlight", json!([sid, text, fg, bg])),
                triggers::Action::Mark => self.effect("mark", sid, json!({"what":"set"})),
                triggers::Action::Title(text) => shell(|s| {
                    if let Some(x) = s.sessions.get_mut(&sid) {
                        x.meta.title = text;
                    }
                }),
            }
        }
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
            Tag::Url { sid, x, y, mods } => {
                match done.result.ok().and_then(|v| v.as_str().map(str::to_owned)).filter(|u| !u.is_empty()) {
                    Some(url) => self.open_link(sid, &url),
                    None => {
                        // not a link: the click goes to the program, as a plain click would
                        self.effect("mouse", sid, json!({"type":"down","x":x,"y":y,"button":0,"mods":mods}));
                        self.mouse = Some(sid);
                        self.capture = Some(true);
                    }
                }
            }
            Tag::Log { on } => {
                let path = done.result.ok().and_then(|v| v.as_str().map(str::to_owned)).filter(|p| !p.is_empty());
                let shown = path.map(|p| if self.home.is_empty() { p } else { p.replacen(&self.home, "~", 1) });
                let toast = match (on, shown) {
                    (true, Some(path)) => json!({"key":"terminal.toast.logging","params":{"path":path}}),
                    (true, None) => json!({"key":"terminal.toast.logFailed"}),
                    (false, _) => json!({"key":"terminal.toast.logStopped"}),
                };
                self.toast(toast, clock::now());
            }
            Tag::Export => {
                let path = done.result.ok().and_then(|v| v.as_str().map(str::to_owned)).filter(|p| !p.is_empty());
                let toast = match path {
                    Some(p) => json!({"key":"terminal.toast.saved","params":{"path":if self.home.is_empty() { p } else { p.replacen(&self.home, "~", 1) }}}),
                    None => json!({"key":"terminal.toast.saveFailed"}),
                };
                self.toast(toast, clock::now());
            }
            Tag::Prefix => {
                let prefix = done.result.ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
                if let Some(sid) = shell(|s| s.ws.current_sid(&self.window)) {
                    self.jobs.enqueue(Tag::Words { prefix: prefix.clone() }, "term.words", json!([sid, prefix]));
                }
            }
            Tag::Words { prefix } => {
                let items: Vec<String> = done.result.ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
                    .split('\n').filter(|l| !l.is_empty()).map(str::to_owned).collect();
                self.ui(|u| u.popup = Some(Popup::Complete { prefix, items, sel: 0 }));
            }
            Tag::PasteList => {
                let raw: Vec<String> = done.result.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default()
                    .iter().filter_map(|t| t.as_str().map(str::to_owned)).collect();
                let items = raw.iter().map(|t| t.replace('\n', "⏎ ")).collect();
                self.ui(|u| u.popup = Some(Popup::Paste { items, raw, sel: 0 }));
            }
            Tag::ClipList => {
                let raw: Vec<String> = done.result.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default()
                    .iter().filter_map(|t| t.as_str().map(str::to_owned)).collect();
                self.ui(|u| u.clipboard = raw);
            }
            Tag::ReplayState { sid, entering } => match done.result {
                Ok(r) if r["count"].as_i64().unwrap_or(0) > 0 => {
                    self.ui(|u| {
                        u.replay = Some(Replay { sid, index: r["index"].as_i64().unwrap_or(0), count: r["count"].as_i64().unwrap_or(0), time: r["time"].as_i64().unwrap_or(0) });
                    });
                }
                // nothing recorded: say so only when asking for the replay, not when stepping in it
                Ok(_) if entering => self.toast(json!({"key":"terminal.toast.noReplay"}), clock::now()),
                _ => {}
            },
            Tag::CopyKey => {
                if let Ok(r) = done.result {
                    if r["status"].as_i64() == Some(2) {
                        if let Some(text) = r["text"].as_str().filter(|t| !t.is_empty()) {
                            self.jobs.enqueue(Tag::Quiet, "clipboard.remember", json!([text]));
                            self.toast(json!({"key":"terminal.toast.copied"}), clock::now());
                        }
                    }
                    if r["status"].as_i64().unwrap_or(0) != 0 {
                        self.ui(|u| u.copy_mode = None);
                    }
                }
            }
            Tag::Copied => {
                if let Some(text) = done.result.ok().and_then(|v| v.as_str().map(str::to_owned)).filter(|t| !t.is_empty()) {
                    self.jobs.enqueue(Tag::Quiet, "clipboard.remember", json!([text]));
                }
            }
            Tag::Pasted(mode) => {
                let text = done.result.ok().and_then(|v| v["text"].as_str().map(str::to_owned)).unwrap_or_default();
                let text = match mode {
                    PasteMode::Plain => text,
                    PasteMode::OneLine => text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "),
                    PasteMode::Escaped => format!("'{}'", text.replace('\'', "'\\''")),
                };
                if !text.is_empty() {
                    self.to_session("paste", json!({"text":text}));
                }
            }
            Tag::Output => {
                match done.result.ok().and_then(|v| v.as_str().map(str::to_owned)).filter(|t| !t.is_empty()) {
                    Some(text) => {
                        self.jobs.enqueue(Tag::Quiet, "clipboard.remember", json!([text]));
                        self.toast(json!({"key":"terminal.toast.outputCopied"}), clock::now());
                    }
                    None => self.toast(json!({"key":"terminal.toast.noOutput"}), clock::now()),
                }
            }
            Tag::Found => {
                if let Ok(found) = done.result {
                    self.ui(|u| {
                        if let Some(find) = u.find.as_mut() {
                            find.count = found["count"].as_i64().unwrap_or(0);
                            find.index = found["index"].as_i64().unwrap_or(-1);
                            find.error = found["error"] == true;
                        }
                    });
                }
            }
            Tag::Logical { query, cs, sid } => {
                let text = done.result.ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
                match regex_matches(&text, &query, cs) {
                    Some(trip) => self.jobs.enqueue(Tag::Found, "term.setMatches", json!([sid, trip])),
                    None => self.ui(|u| {
                        if let Some(find) = u.find.as_mut() {
                            (find.count, find.index, find.error) = (0, -1, true);
                        }
                    }),
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
            Tag::Screen { sid } => {
                shell(|s| {
                    if let Some(x) = s.sessions.get_mut(&sid) {
                        x.scan_busy = false;
                    }
                });
                if let Some(text) = done.result.ok().and_then(|v| v.as_str().map(str::to_owned)) {
                    self.run_triggers(sid, &text, clock::now());
                }
            }
            Tag::OscCopy { sid } => {
                if let Some(text) = done.result.ok().and_then(|v| v["text"].as_str().map(str::to_owned)).filter(|t| !t.is_empty()) {
                    self.jobs.enqueue(Tag::Quiet, "clipboard.remember", json!([text]));
                    self.toast_for(sid, json!({"key":"terminal.toast.copied"}), clock::now());
                }
            }
            Tag::Fonts => {
                let list = done.result.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
                shell(|s| s.fonts = Some(list));
            }
            Tag::Quiet => {}
        }
    }
    fn timers(&mut self, now: i64) {
        self.expire_closed(now);
        self.ui(|u| u.toasts.retain(|t| t.until > now));
        // the find bar follows the focused pane
        let current = shell(|s| s.ws.current_sid(&self.window));
        let moved = self.ui(|u| match (&mut u.find, current) {
            (Some(find), Some(sid)) if find.sid != sid => {
                let old = find.sid;
                find.sid = sid;
                (find.count, find.index) = (0, -1);
                Some(old)
            }
            _ => None,
        });
        if let Some(old) = moved {
            self.jobs.enqueue(Tag::Quiet, "term.search", json!([old, "", false]));
            self.find_run();
        }
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
        let belt = if self.ui(|u| u.toolbelt) { BELT_WIDTH } else { 0. };
        let extra = self.ui(|u| u32::from(u.replay.is_some()) as f64 * ui::REPLAY_BAR + u32::from(u.copy_mode.is_some()) as f64 * ui::COPY_BAR);
        Rect { x: 0., y: 0., w: (self.width - belt).max(0.), h: (self.height - ui::TAB_BAR - self.status_height() - extra).max(0.) }
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

    /// A link clicked with Ctrl or Super: folders and files of the home folder open in Files, anything
    /// else is copied (there is no browser).
    fn open_link(&mut self, sid: Sid, url: &str) {
        let home = self.home.clone();
        let mut path = match url.strip_prefix("file://") {
            Some(rest) => {
                let rest = rest.find('/').map_or("", |i| &rest[i..]);
                rest.to_owned()
            }
            None => url.to_owned(),
        };
        if let Some(rest) = path.strip_prefix('~') {
            path = format!("{home}{rest}");
        } else if let Some(rest) = path.strip_prefix("./") {
            let cwd = shell(|s| s.sessions.get(&sid).map(|x| x.info["cwd"].as_str().unwrap_or("").to_owned())).unwrap_or_default();
            let cwd = if cwd.is_empty() { home.clone() } else { cwd };
            path = format!("{cwd}/{rest}");
        }
        let inside = !home.is_empty() && (path == home || path.starts_with(&format!("{home}/")));
        if path.starts_with('/') && inside {
            let dir = if std::path::Path::new(&path).is_dir() {
                path
            } else {
                std::path::Path::new(&path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or(home)
            };
            self.jobs.enqueue(Tag::Quiet, "windows.open", json!(["files", {"args":{"path":dir}}]));
            return;
        }
        self.jobs.enqueue(Tag::Quiet, "clipboard.remember", json!([url]));
        let shown = if url.chars().count() > 60 { format!("{}…", url.chars().take(57).collect::<String>()) } else { url.to_owned() };
        self.toast(json!({"key":"terminal.toast.linkCopied","params":{"url":shown}}), clock::now());
    }
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
                    self.ask_then(&[sid], "pane", Confirm::Pane(sid), now);
                }
            }
            "closeTab" => {
                let found = shell(|s| s.ws.window(&self.window).and_then(|w| w.active_tab().map(|t| (t.id.clone(), t.root.leaves()))));
                if let Some((id, sids)) = found {
                    self.ask_then(&sids, "tab", Confirm::Tab(id), now);
                }
            }
            "newWindow" => self.jobs.enqueue(Tag::Quiet, "windows.open", json!(["terminal", {"new":true}])),
            "logSession" => {
                if let Some(sid) = sid {
                    let on = !shell(|s| s.sessions.get(&sid).is_some_and(|x| x.info["logging"] == true));
                    self.jobs.enqueue(Tag::Log { on }, "term.log", json!([sid, on]));
                }
            }
            "exportText" => {
                if let Some(sid) = sid {
                    self.jobs.enqueue(Tag::Export, "term.export", json!([sid]));
                }
            }
            "preferences" => self.prefs_open(),
            "alertNextMark" => {
                if let Some(sid) = sid {
                    let on = shell(|s| s.sessions.get_mut(&sid).map(|x| {
                        x.alert = !x.alert;
                        x.alert
                    })).unwrap_or(false);
                    self.toast(json!({"key":if on {"terminal.toast.alertOn"} else {"terminal.toast.alertOff"}}), now);
                }
            }
            "copy" => self.copy(),
            "paste" => self.paste(PasteMode::Plain),
            "pasteOneLine" => self.paste(PasteMode::OneLine),
            "pasteEscaped" => self.paste(PasteMode::Escaped),
            "selectOutput" => {
                if let Some(sid) = sid {
                    self.jobs.enqueue(Tag::Output, "term.selectOutput", json!([sid]));
                }
            }
            "find" => self.find_open(),
            "autocomplete" => {
                if let Some(sid) = sid {
                    self.jobs.enqueue(Tag::Prefix, "term.cursorWord", json!([sid]));
                }
            }
            "commandHistory" => {
                let items = shell(|s| s.commands.clone());
                self.ui(|u| u.popup = Some(Popup::History { items, sel: 0 }));
            }
            "pasteHistory" => self.jobs.enqueue(Tag::PasteList, "clipboard.history", json!([])),
            "openQuickly" => self.ui(|u| u.popup = Some(Popup::Quick { q: String::new(), sel: 0 })),
            "editTitle" => {
                let value = shell(|s| s.ws.window(&self.window).and_then(|w| w.active_tab().map(|t| t.title.clone().unwrap_or_default()))).unwrap_or_default();
                self.ui(|u| u.popup = Some(Popup::Title { value }));
            }
            "composer" => self.ui(|u| u.composer = if u.composer.is_some() { None } else { Some(String::new()) }),
            "broadcast" => self.ui(|u| u.broadcast = !u.broadcast),
            "toolbelt" => {
                let open = self.ui(|u| {
                    u.toolbelt = !u.toolbelt;
                    u.toolbelt
                });
                if open {
                    self.jobs.enqueue(Tag::ClipList, "clipboard.history", json!([]));
                }
            }
            "copyMode" => {
                if let Some(sid) = sid {
                    self.effect("copyMode", sid, json!({"on":true}));
                    self.ui(|u| u.copy_mode = Some(sid));
                }
            }
            "instantReplay" => {
                if let Some(sid) = sid {
                    self.jobs.enqueue(Tag::ReplayState { sid, entering: true }, "term.replay", json!([sid, "enter"]));
                }
            }
            "findNext" | "findPrev" => {
                if let Some(sid) = sid {
                    let delta = if action == "findNext" { -1 } else { 1 };
                    self.jobs.enqueue(if self.ui(|u| u.find.is_some()) { Tag::Found } else { Tag::Quiet }, "term.searchMove", json!([sid, delta]));
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
                let back = shell(|s| {
                    let back = s.ws.undo_close(&window);
                    if let Some((_, sid)) = &back {
                        s.closing.retain(|(c, _)| c != sid);
                    }
                    back
                });
                if back.is_none() {
                    self.toast(json!({"key":"terminal.toast.nothingToUndo"}), now);
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
        if self.prefs_open_now() {
            return Some(self.prefs_key(key, flags, facts, now));
        }
        let sid = shell(|s| s.ws.current_sid(&self.window))?;
        if let Some(dialog) = self.ui(|u| u.dialog.clone()) {
            // a dialog takes every key: Escape cancels, Enter confirms
            if key == "Escape" {
                self.ui(|u| u.dialog = None);
            } else if key == "Enter" {
                self.ui(|u| u.dialog = None);
                self.confirmed(dialog.confirm, now);
            }
            return Some(self.out(now));
        }
        if let Some(popup) = self.ui(|u| u.popup.clone()) {
            if self.popup_key(&popup, key, now) {
                return Some(self.out(now));
            }
        }
        if let Some(csid) = self.ui(|u| u.copy_mode) {
            // copy mode keeps every key for its own movements
            let k = if !chr.is_empty() { chr } else if flags & 2 != 0 && !facts.base.is_empty() { facts.base } else { key };
            if !k.is_empty() && !MODIFIER_CODES.contains(&facts.code) {
                self.jobs.enqueue(Tag::CopyKey, "term.copyKey", json!([csid, k, flags & 2 != 0]));
            }
            return Some(self.out(now));
        }
        if let Some(replay) = self.ui(|u| u.replay.clone()) {
            match key {
                "Escape" | "Enter" => self.replay_exit(),
                "ArrowLeft" => self.replay_step(&replay, -1),
                "ArrowRight" => self.replay_step(&replay, 1),
                _ => {}
            }
            return Some(self.out(now));
        }
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
        if self.prefs_command(v["cmd"].as_str().unwrap_or(""), v) {
            return;
        }
        let key = &v["key"];
        match v["cmd"].as_str().unwrap_or("") {
            "tab" => {
                if let Some(id) = key.as_str() {
                    self.select_tab(id);
                }
            }
            "closeTab" => {
                if let Some(id) = key.as_str() {
                    let sids = shell(|s| s.ws.window(&self.window).and_then(|w| w.tabs.iter().find(|t| t.id == id).map(|t| t.root.leaves()))).unwrap_or_default();
                    self.ask_then(&sids, "tab", Confirm::Tab(id.to_owned()), now);
                }
            }
            "do" => {
                if let Some(action) = key.as_str() {
                    self.ui(|u| u.menu = None);
                    self.run(action, now);
                }
            }
            "newTabWith" => {
                if let Some(id) = key.as_str() {
                    self.create(Purpose::Tab, id, None);
                }
            }
            "paneMenu" => {
                let Some(sid) = key.as_i64() else { return };
                self.focus(sid);
                let (x, y) = (number(&v["x"], 0.), number(&v["y"], 0.));
                self.ui(|u| u.menu = Some(PaneMenu { sid, x, y }));
            }
            "menuClose" => self.ui(|u| u.menu = None),
            "popupClose" => self.ui(|u| u.popup = None),
            "quickChange" => self.ui(|u| u.popup = Some(Popup::Quick { q: v["value"].as_str().unwrap_or("").to_owned(), sel: 0 })),
            "quickKey" => {
                let delta = if key == "ArrowUp" { -1 } else { 1 };
                let q = self.ui(|u| match &u.popup { Some(Popup::Quick { q, .. }) => Some(q.clone()), _ => None });
                if let Some(q) = q {
                    let len = self.quick_items(&q).len().min(10) as i64;
                    self.ui(|u| {
                        if let Some(Popup::Quick { sel, .. }) = u.popup.as_mut() {
                            *sel = (*sel as i64 + delta).clamp(0, (len - 1).max(0)) as usize;
                        }
                    });
                }
            }
            "quickPick" => {
                let picked = self.ui(|u| match &u.popup { Some(Popup::Quick { q, sel }) => Some((q.clone(), *sel)), _ => None });
                if let Some((q, sel)) = picked {
                    let at = key.as_u64().map_or(sel, |i| i as usize);
                    if let Some((_, sid, _)) = self.quick_items(&q).get(at).cloned() {
                        self.focus_anywhere(sid);
                    }
                }
                self.ui(|u| u.popup = None);
            }
            "titleChange" => self.ui(|u| u.popup = Some(Popup::Title { value: v["value"].as_str().unwrap_or("").to_owned() })),
            "titleDone" => {
                let value = self.ui(|u| match u.popup.take() { Some(Popup::Title { value }) => Some(value), _ => None });
                if let Some(value) = value {
                    shell(|s| {
                        if let Some(id) = s.ws.window(&self.window).and_then(|w| w.active_tab().map(|t| t.id.clone())) {
                            s.ws.set_tab_title(&self.window, &id, value.trim());
                        }
                    });
                }
            }
            "composerChange" => self.ui(|u| u.composer = Some(v["value"].as_str().unwrap_or("").to_owned())),
            "composerKey" => {
                if let Some(text) = self.ui(|u| u.composer.clone()) {
                    self.to_session("paste", json!({"text":if v["shift"] == true { text } else { format!("{text}\n") }}));
                    self.ui(|u| u.composer = Some(String::new()));
                }
            }
            "composerClose" => self.ui(|u| u.composer = None),
            "beltJob" => {
                if let Some(sid) = key.as_i64() {
                    self.focus_anywhere(sid);
                }
            }
            "beltCommand" => {
                if let Some(cmd) = key.as_u64().and_then(|i| shell(|s| s.commands.get(i as usize).cloned())) {
                    self.to_session("write", json!({"text":cmd}));
                }
            }
            "beltClip" => {
                if let Some(text) = key.as_u64().and_then(|i| self.ui(|u| u.clipboard.get(i as usize).cloned())) {
                    self.to_session("paste", json!({"text":text}));
                }
            }
            "beltDir" => {
                if let Some(dir) = key.as_u64().and_then(|i| shell(|s| s.recent_dirs.get(i as usize).cloned())) {
                    self.to_session("write", json!({"text":format!("cd '{}'\r", dir.replace('\'', "'\\''"))}));
                }
            }
            "beltProfile" => {
                if let Some(id) = key.as_str() {
                    self.create(Purpose::Tab, id, None);
                }
            }
            "beltNoteDelete" => {
                if let Some(i) = key.as_u64() {
                    shell(|s| {
                        if (i as usize) < s.notes.len() {
                            s.notes.remove(i as usize);
                        }
                    });
                }
            }
            "beltNoteChange" => self.ui(|u| u.note = v["value"].as_str().unwrap_or("").to_owned()),
            "beltNoteAdd" => {
                let note = self.ui(|u| std::mem::take(&mut u.note));
                let note = note.trim().to_owned();
                if !note.is_empty() {
                    shell(|s| s.notes.push(note));
                }
            }
            "replayStep" => {
                if let Some(replay) = self.ui(|u| u.replay.clone()) {
                    self.replay_step(&replay, key.as_i64().unwrap_or(0));
                }
            }
            "replayExit" => self.replay_exit(),
            "findChange" => {
                self.ui(|u| {
                    if let Some(find) = u.find.as_mut() {
                        find.query = v["value"].as_str().unwrap_or("").to_owned();
                    }
                });
                self.find_run();
            }
            "findCs" | "findRegex" => {
                let regex = v["cmd"] == "findRegex";
                self.ui(|u| {
                    if let Some(find) = u.find.as_mut() {
                        if regex { find.regex = !find.regex } else { find.cs = !find.cs }
                    }
                });
                self.find_run();
            }
            "findMove" => self.find_move(key.as_i64().unwrap_or(0).signum()),
            "findKey" => match key.as_str() {
                Some("ArrowUp") => self.find_move(-1),
                Some("ArrowDown") => self.find_move(1),
                Some("Enter") => self.find_move(if v["shift"] == true { 1 } else { -1 }),
                _ => {}
            },
            "findClose" => self.find_close(),
            "dialogCancel" => self.ui(|u| u.dialog = None),
            "dialogConfirm" => {
                if let Some(dialog) = self.ui(|u| u.dialog.take()) {
                    self.confirmed(dialog.confirm, now);
                }
            }
            "newTab" => {
                self.run("newTab", now);
            }
            "paneDown" => {
                let Some(sid) = key.as_i64() else { return };
                self.focus(sid);
                if v["button"].as_i64() == Some(2) {
                    self.command(&json!({"cmd":"paneMenu","key":sid,"x":v["x"],"y":v["y"]}), now);
                    return;
                }
                if let Some((x, y)) = self.local(sid, v) {
                    let mods = Self::mods(v);
                    if v["button"].as_i64().unwrap_or(0) == 0 && (v["ctrl"] == true || v["meta"] == true) {
                        // a link under the pointer opens; otherwise the click is the program's
                        self.jobs.enqueue(Tag::Url { sid, x, y, mods }, "term.urlAt", json!([sid, x, y]));
                        return;
                    }
                    self.effect("mouse", sid, json!({"type":"down","x":x,"y":y,"button":v["button"].as_i64().unwrap_or(0),"mods":mods}));
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
                if self.slide.is_some() {
                    self.slide_to(number(&v["x"], 0.));
                    return;
                }
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
                if self.slide.is_some() {
                    self.slide_to(number(&v["x"], 0.));
                    self.slide = None;
                    return;
                }
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
        self.sync_menus();
        self.pf_flush();
        self.sync_guard();
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
        let revision = v["context"]["localeRevision"].as_u64().unwrap_or(0);
        if revision != self.locale_revision {
            self.i18n.invalidate();
            self.locale_revision = revision;
            self.menus = Value::Null;
        }
        self.i18n.lang = v["lang"].as_str().unwrap_or("en").into();
        self.width = number(&v["context"]["width"], self.width).max(1.);
        self.height = number(&v["context"]["height"], self.height).max(1.);
        self.focused = v["context"]["focused"].as_bool().unwrap_or(self.focused);
        if v["settings"]["terminal"].is_object() && !self.reconfig {
            self.settings = v["settings"]["terminal"].clone();
        }
        if self.booted {
            let args = &v["args"];
            if *args != self.last_args {
                self.last_args = args.clone();
                if args["action"] == "newTab" {
                    self.run("newTab", now);
                }
            }
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
        self.last_args = v["args"].clone();
        if !known {
            let id = self.default_profile();
            let cwd = v["args"]["cwd"].as_str().filter(|d| d.starts_with('/')).map(str::to_owned);
            // another app can leave an ssh or sftp line typed, never run
            let typed = v["args"]["typed"].as_str().filter(|t| ssh_line(t)).map(str::to_owned);
            self.first_typed = typed;
            self.create(Purpose::Window, &id, cwd);
        } else {
            self.configure_window();
        }
    }
    fn bindings(&self) -> Value {
        model(&json!({"cmd":"bindings","shortcuts":self.raw()["shortcuts"]}))
    }
    /// The first shortcut of an action as the menus show it (`bindings` is a list of [action, [combo…]]).
    fn hint(&self, bindings: &Value, action: &str) -> String {
        let first = bindings
            .as_array()
            .and_then(|list| list.iter().find(|entry| entry[0] == action))
            .map_or(Value::Null, |entry| entry[1][0].clone());
        if first.is_null() {
            return String::new();
        }
        model(&json!({"cmd":"comboLabel","value":first})).as_str().unwrap_or("").to_owned()
    }
    fn action_item(&mut self, bindings: &Value, action: &str, checked: Option<bool>, disabled: bool) -> Value {
        let mut item = json!({"label":self.i18n.caption(&json!({"key":format!("terminal.action.{action}")})),"action":["do", action]});
        let hint = self.hint(bindings, action);
        if !hint.is_empty() {
            item["hint"] = json!(hint);
        }
        if let Some(on) = checked {
            item["checked"] = json!(on);
        }
        if disabled {
            item["disabled"] = json!(true);
        }
        item
    }
    fn plain_items(&mut self, bindings: &Value, actions: &[&str]) -> Vec<Value> {
        actions.iter().map(|a| if *a == "-" { json!("-") } else { self.action_item(bindings, a, None, false) }).collect()
    }
    fn build_menus(&mut self) -> Value {
        let b = self.bindings();
        let (zoomed, multi, timestamps) = shell(|s| {
            let w = s.ws.window(&self.window);
            let tab = w.and_then(|w| w.active_tab());
            (tab.is_some_and(|t| t.zoom), tab.is_some_and(|t| t.root.leaves().len() > 1), w.is_some_and(|w| w.timestamps))
        });
        let (broadcast, composer, toolbelt) = self.ui(|u| (u.broadcast, u.composer.is_some(), u.toolbelt));
        let mut shell_menu = self.plain_items(&b, &["newTab", "newWindow", "-", "splitRight", "splitDown", "-"]);
        shell_menu.push(self.action_item(&b, "broadcast", Some(broadcast), false));
        shell_menu.push(self.action_item(&b, "composer", Some(composer), false));
        let alert = shell(|s| s.ws.current_sid(&self.window).and_then(|sid| s.sessions.get(&sid)).is_some_and(|x| x.alert));
        shell_menu.push(self.action_item(&b, "alertNextMark", Some(alert), false));
        shell_menu.extend(self.plain_items(&b, &["-", "logSession", "exportText", "-", "closePane", "closeTab", "undoClose"]));
        let edit = self.plain_items(&b, &["copy", "paste", "pasteHistory", "pasteOneLine", "pasteEscaped", "-", "copyMode", "commandHistory", "-",
            "selectAll", "selectOutput", "-", "find", "findNext", "findPrev", "-", "autocomplete", "clearBuffer", "reset"]);
        let mut view = self.plain_items(&b, &["zoomIn", "zoomOut", "zoomReset", "-"]);
        view.push(self.action_item(&b, "maximizePane", Some(zoomed), !multi));
        view.push(self.action_item(&b, "equalize", None, !multi));
        view.push(self.action_item(&b, "paneToTab", None, !multi));
        view.extend(self.plain_items(&b, &["-", "instantReplay", "prevMark", "nextMark", "setMark", "-"]));
        view.push(self.action_item(&b, "timestamps", Some(timestamps), false));
        view.push(self.action_item(&b, "toolbelt", Some(toolbelt), false));
        view.extend(self.plain_items(&b, &["openQuickly", "editTitle"]));
        let profiles: Vec<Value> = self.terminal_settings()["profiles"].as_array().cloned().unwrap_or_default().iter().map(|p| {
            let name = match p["name"].as_str() {
                Some(n) if !n.is_empty() && !(p["id"] == "default" && n == "Predeterminado") => n.to_owned(),
                _ => self.i18n.caption(&json!({"key":"terminal.profile.default"})),
            };
            json!({"label":self.i18n.caption(&json!({"key":"terminal.menu.newTabWith","params":{"name":name}})),"action":["newTabWith", p["id"]]})
        }).collect();
        let mut profiles = profiles;
        profiles.push(json!("-"));
        profiles.push(self.action_item(&b, "preferences", None, false));
        let top = |app: &mut App, key: &str| app.i18n.caption(&json!({"key":format!("terminal.menu.{key}")}));
        json!([
            {"label":top(self, "shell"),"items":shell_menu},
            {"label":top(self, "edit"),"items":edit},
            {"label":top(self, "view"),"items":view},
            {"label":top(self, "profiles"),"items":profiles},
        ])
    }
    fn sync_menus(&mut self) {
        if self.window.is_empty() || !self.booted || shell(|s| s.ws.window(&self.window).is_none()) {
            return;
        }
        let menus = self.build_menus();
        if menus != self.menus {
            self.menus = menus.clone();
            self.jobs.enqueue(Tag::Quiet, "windows.menus", json!([self.window, menus]));
        }
    }
    /// The window may not close while a program runs: the transport asks Rust first.
    fn sync_guard(&mut self) {
        let sids = shell(|s| s.ws.sessions(&self.window));
        if sids.is_empty() {
            return;
        }
        let busy = sids.iter().any(|sid| {
            let running = shell(|s| s.sessions.get(sid).is_some_and(|x| x.info["alive"] == true && x.info["busy"] == true));
            running && self.session_profile(*sid)["confirmClose"] == true
        });
        if self.guard != Some(busy) {
            self.guard = Some(busy);
            self.jobs.enqueue(Tag::Quiet, "windows.guard", json!([self.window, busy]));
        }
    }
    fn menu_facts(&self) -> Option<ui::MenuFacts> {
        let menu = self.ui(|u| u.menu.clone())?;
        let selection = shell(|s| s.sessions.get(&menu.sid).is_some_and(|x| x.info["hasSelection"] == true));
        let bindings = self.bindings();
        let row = |action: &str, disabled: bool| ui::MenuRow::Item {
            label: json!({"key":format!("terminal.action.{action}")}),
            hint: self.hint(&bindings, action),
            action: json!(["do", action]),
            disabled,
        };
        let rows = vec![
            row("copy", !selection),
            row("paste", false),
            ui::MenuRow::Separator,
            row("splitRight", false),
            row("splitDown", false),
            ui::MenuRow::Separator,
            row("selectAll", false),
            row("clearBuffer", false),
            row("find", false),
            ui::MenuRow::Separator,
            row("preferences", false),
            row("closePane", false),
        ];
        Some(ui::MenuFacts { x: menu.x, y: menu.y, rows })
    }
    fn popup_facts(&self) -> Option<ui::PopupFacts> {
        let popup = self.ui(|u| u.popup.clone())?;
        Some(match popup {
            Popup::Complete { prefix, items, sel } => ui::PopupFacts::List {
                title: json!({"key":"terminal.popup.complete","params":{"prefix":prefix}}),
                items,
                sel,
                empty: json!({"key":"terminal.popup.completeEmpty"}),
            },
            Popup::Paste { items, sel, .. } => ui::PopupFacts::List { title: json!({"key":"terminal.popup.paste"}), items, sel, empty: json!({"key":"terminal.popup.pasteEmpty"}) },
            Popup::History { items, sel } => ui::PopupFacts::List { title: json!({"key":"terminal.popup.history"}), items, sel, empty: json!({"key":"terminal.popup.historyEmpty"}) },
            Popup::Quick { q, sel } => {
                let items = self.quick_items(&q).into_iter().take(10).map(|(_, _, title)| title).collect();
                ui::PopupFacts::Quick { query: q, items, sel }
            }
            Popup::Title { value } => ui::PopupFacts::Title { value },
        })
    }
    fn belt_facts(&self, tab: &crate::workspace::Tab) -> ui::BeltFacts {
        let captured = shell(|s| s.captured.clone());
        let (jobs, commands, dirs, notes, profiles_raw) = shell(|s| {
            let jobs: Vec<(Sid, String, String)> = s.ws.sessions(&self.window).into_iter().map(|sid| {
                let session = s.sessions.get(&sid).cloned().unwrap_or_default();
                let sub = if session.info["busy"] == true { format!("▶ {}", session.info["process"].as_str().unwrap_or("")) } else { session.info["shell"].as_str().unwrap_or("").to_owned() };
                (sid, session_title(Some(&session.meta)), sub)
            }).collect();
            (jobs, s.commands.clone(), s.recent_dirs.clone(), s.notes.clone(), ())
        });
        let _ = (tab, profiles_raw);
        let profiles = self.terminal_settings()["profiles"].as_array().cloned().unwrap_or_default().iter().map(|p| {
            let name = match p["name"].as_str() {
                Some(n) if !n.is_empty() && !(p["id"] == "default" && n == "Predeterminado") => json!(n),
                _ => json!({"key":"terminal.profile.default"}),
            };
            (p["id"].as_str().unwrap_or("").to_owned(), name)
        }).collect();
        let (clipboard, note) = self.ui(|u| (u.clipboard.clone(), u.note.clone()));
        ui::BeltFacts {
            jobs,
            commands,
            captured: captured.into_iter().map(|(sid, text)| (sid, text)).collect(),
            clipboard,
            dirs: dirs.into_iter().map(|d| if self.home.is_empty() { d } else { d.replacen(&self.home, "~", 1) }).collect(),
            profiles,
            notes,
            note,
        }
    }
    fn scene(&mut self, now: i64) -> Value {
        let Some((tabs, active, font)) = shell(|s| {
            let w = s.ws.window(&self.window)?;
            Some((w.tabs.clone(), w.active.clone(), w.font_delta))
        }) else {
            return ui::tree(&ui::Facts { theme: &self.theme, pane_bg: "#000000".into(), width: self.width, tabs: vec![], broadcast: false,
                panes: vec![], dividers: &[], status: None, find: None, find_focus: 0, dialog: None, toasts: vec![],
                popup: None, composer: None, belt: None, replay: None, copy_mode: false, menu: None });
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
                ui::PaneFacts { sid: *sid, rect: *rect, dim: multi && !tab.zoom && *sid != tab.focus && profile["dimInactive"] == true, flash: now < session.flash_until, badge: session.badge }
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
        let popup = self.popup_facts();
        let (composer, copy_mode, replay, belt_on) = self.ui(|u| {
            (
                u.composer.clone().map(|text| (text, u.broadcast)),
                u.copy_mode.is_some(),
                u.replay.clone().map(|r| ui::ReplayFacts { index: r.index, count: r.count, time: r.time, ago: 0 }),
                u.toolbelt,
            )
        });
        let belt = belt_on.then(|| self.belt_facts(&tab));
        let replay = replay.map(|mut r| {
            r.ago = ((now - r.time) / 1000).max(0);
            r
        });
        let broadcast = self.ui(|u| u.broadcast);
        let (find, dialog, toasts, find_focus) = self.ui(|u| {
            (
                u.find.clone().map(|f| ui::FindFacts { query: f.query, cs: f.cs, regex: f.regex, count: f.count, index: f.index, error: f.error }),
                u.dialog.clone().map(|d| (d.text, d.detail)),
                u.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>(),
                u.find_focus,
            )
        });
        let mut tree = ui::tree(&ui::Facts { theme: &theme, pane_bg, width: self.width, tabs: tab_facts, broadcast, panes, dividers: &dividers, status,
            find, find_focus, dialog, toasts, popup, composer, belt, replay, copy_mode, menu: self.menu_facts() });
        // the Preferences cover the window, above everything else
        if let Some(prefs) = self.prefs_scene() {
            if let Some(children) = tree["children"].as_array_mut() {
                children.push(prefs);
            }
        }
        tree
    }
}

#[cfg(test)]
#[path = "native_window_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "prefs_tests.rs"]
mod prefs_tests;

#[cfg(test)]
#[path = "events_tests.rs"]
mod events_tests;
