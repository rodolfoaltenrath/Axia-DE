//! Mail: the three-pane window and the compose window. Everything comes from the sandboxed helper
//! axia-maild (docs/MAIL.md) through window-scoped jobs; the app decides what is asked and how it
//! is shown. A window is one or the other, chosen by its arguments.
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    civil_date, date_format,
    i18n::I18n,
    markdown, KeyFacts, NativeApp,
};
use serde_json::{json, Value};
use std::rc::Rc;

// Each literal owns one compiled pattern (the shared Notes editor uses it).
macro_rules! pattern {
    ($source:literal) => {{
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| regex::Regex::new($source).unwrap())
    }};
}

mod compose;
/// The wrapped Markdown editor of Notes, here for the message body.
#[path = "../notes/editor.rs"]
mod editor;
#[allow(dead_code)]
#[path = "../notes/emoji.rs"]
mod emoji;
mod html;
pub mod model;
#[cfg(test)]
mod model_tests;
mod sheets;
#[cfg(test)]
mod tests;
mod ui;
mod window;

/// How often the shell is asked for what happened (mail events, menu items, a close request).
const EVENTS_MS: i64 = 200;

fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn list_of(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

pub(crate) enum Tag {
    Ignore,
    /// A request whose only interesting answer is its failure.
    Report,
    Watch,
    Snapshot,
    Events,
    Main(window::Tag),
    Sheet(sheets::Tag),
    Compose(compose::Tag),
}

/// An error of the helper or of the shell, with the helper's code ("offline", "locked"…).
#[derive(Debug)]
pub(crate) struct Failure {
    pub code: String,
    pub message: String,
}
/// The answer of a service that replies `{value}` or `{error}`.
pub(crate) fn answer(result: Result<Value, String>) -> Result<Value, Failure> {
    match result {
        Err(message) => Err(Failure { code: message.clone(), message }),
        Ok(v) if v["error"].is_object() => Err(Failure {
            code: str_of(&v["error"]["code"]).to_owned(),
            message: str_of(&v["error"]["message"]).to_owned(),
        }),
        // other services answer with a plain value (a window id, null)
        Ok(mut v) => Ok(v.get_mut("value").map(Value::take).unwrap_or_default()),
    }
}

/// What the session knows about mail: asked at the start and again whenever it says it changed.
#[derive(Default)]
pub(crate) struct Facts {
    pub ready: bool,
    pub available: bool,
    /// In its own process: files come as descriptors from the system dialog, never as paths.
    pub isolated: bool,
    pub config: Value,
    pub status: Value,
    pub cache_dir: String,
}

/// What both kinds of window share.
pub(crate) struct Base {
    pub i18n: I18n,
    pub theme: Rc<Value>,
    pub dark: bool,
    pub region: String,
    pub hour: String,
    pub rtl: bool,
    pub width: f64,
    pub height: f64,
    pub focused: bool,
    pub home: String,
    /// The window manager's id of this window.
    pub window: String,
    pub jobs: Jobs<Tag>,
    pub effects: Vec<Value>,
    pub facts: Facts,
}
impl Default for Base {
    fn default() -> Self {
        Self {
            i18n: I18n::default(),
            theme: Rc::new(Value::Null),
            dark: true,
            region: String::new(),
            hour: String::new(),
            rtl: false,
            width: 1120.,
            height: 640.,
            focused: true,
            home: String::new(),
            window: String::new(),
            jobs: Jobs::default(),
            effects: Vec::new(),
            facts: Facts::default(),
        }
    }
}
impl Base {
    pub fn t(&mut self, key: &str) -> String {
        self.i18n.caption(&json!({"key":format!("mail.{key}")}))
    }
    pub fn tp(&mut self, key: &str, params: Value) -> String {
        self.i18n.caption(&json!({"key":format!("mail.{key}"),"params":params}))
    }
    pub fn token(&self, name: &str, fallback: &str) -> String {
        self.theme[name].as_str().unwrap_or(fallback).to_owned()
    }
    pub fn service(&mut self, tag: Tag, method: &'static str, args: Value) {
        self.jobs.enqueue(tag, method, args);
    }
    /// A request to the helper, with the files that travel with it.
    pub fn request(&mut self, tag: Tag, op: &str, args: Value, files: Value) {
        self.jobs.enqueue(tag, "mail.request", json!([op, args, files]));
    }
    pub fn ask(&mut self, tag: Tag, op: &str, args: Value) {
        self.request(tag, op, args, json!([]));
    }
    pub fn open_app(&mut self, app: &str, options: Value) {
        self.service(Tag::Report, "windows.open", json!([app, options]));
    }
    /// A new compose window with `draft` (null: an empty message).
    pub fn compose(&mut self, draft: Value) {
        let compose = if draft.is_null() { json!(true) } else { draft };
        self.open_app("mail", json!({"new":true,"args":{"compose":compose}}));
    }
    pub fn copy(&mut self, text: &str) {
        self.effects.push(json!({"host":true,"kind":"copy","text":text}));
    }

    // ---- the accounts file ----

    pub fn accounts(&self, kind: &str) -> Vec<Value> {
        list_of(&self.facts.config["accounts"]).iter().filter(|a| a["kind"] == kind).cloned().collect()
    }
    pub fn all_accounts(&self) -> Vec<Value> {
        list_of(&self.facts.config["accounts"]).to_vec()
    }
    pub fn account(&self, id: &str) -> Option<&Value> {
        list_of(&self.facts.config["accounts"]).iter().find(|a| a["id"] == id)
    }
    /// Saves the accounts file. The status store changes with it, so a new snapshot follows.
    pub fn save_config(&mut self, config: Value) {
        self.facts.config = config.clone();
        self.service(Tag::Report, "mail.saveConfig", json!([config]));
    }

    // ---- captions that follow the language and the region ----

    pub fn category_name(&mut self, label: u64) -> String {
        let custom = label.checked_sub(1).map(|i| str_of(&self.facts.config["categories"][i as usize]["name"]).to_owned());
        match custom.filter(|name| !name.is_empty()) {
            Some(name) => name,
            None => self.t(&format!("cat.{label}")),
        }
    }
    pub fn folder_name(&mut self, folder: &Value) -> String {
        match str_of(&folder["special"]) {
            "" | "all" | "flagged" => str_of(&folder["display"]).to_owned(),
            special => self.t(&format!("folder.{special}")),
        }
    }
    fn calendar_name(&mut self, catalog: &str, index: i32) -> String {
        let lang = self.i18n.lang.clone();
        self.i18n
            .entry(&lang, "format", catalog)
            .or_else(|| self.i18n.entry("en", "format", catalog))
            .and_then(|v| v[index.max(0) as usize].as_str().map(str::to_owned))
            .unwrap_or_default()
    }
    pub fn month_short(&mut self, index: i32) -> String {
        self.calendar_name("monthsShort", index)
    }
    pub fn date(&mut self, ms: i64, style: &str) -> String {
        date_format::date(&mut self.i18n, &self.region, &crate::clock::local(ms), style)
    }
    pub fn time(&mut self, ms: i64) -> String {
        date_format::time(&mut self.i18n, &self.region, &self.hour, &crate::clock::local(ms), false)
    }
    pub fn full_date(&mut self, ms: i64) -> String {
        format!("{} {}", self.date(ms, "long"), self.time(ms))
    }
    /// The short date of a list row: the time, "Yesterday", the weekday or the date.
    pub fn list_date(&mut self, ms: i64, now: i64) -> String {
        use model::ListDate;
        let then = crate::clock::local(ms);
        let today = crate::clock::local(now);
        let yesterday = crate::clock::local(civil_date::add_days(now, -1));
        match model::list_date(&then, &today, &yesterday, now - ms) {
            ListDate::Time => self.time(ms),
            ListDate::Yesterday => self.t("yesterday"),
            ListDate::Weekday => self.calendar_name("weekdays", then.tm_wday),
            ListDate::Short => self.date(ms, "short"),
            ListDate::Numeric => self.date(ms, "numeric"),
        }
    }
}

enum Screen {
    /// A compose window waiting for the accounts.
    Starting(Value),
    Main(Box<window::Main>),
    Compose(Box<compose::Compose>),
}

pub struct App {
    base: Base,
    screen: Screen,
    booted: bool,
    events_pending: bool,
    events_at: i64,
    snapshot_pending: bool,
    /// Something changed while a snapshot was on its way: ask once more.
    snapshot_stale: bool,
    menus: Value,
    last_tree: Value,
    locale_revision: u64,
}
impl Default for App {
    fn default() -> Self {
        Self {
            base: Base::default(),
            screen: Screen::Main(Box::default()),
            booted: false,
            events_pending: false,
            events_at: 0,
            snapshot_pending: false,
            snapshot_stale: false,
            menus: Value::Null,
            last_tree: Value::Null,
            locale_revision: 0,
        }
    }
}
impl App {
    fn want_snapshot(&mut self) {
        if self.snapshot_pending {
            self.snapshot_stale = true;
        } else {
            self.snapshot_pending = true;
            self.base.service(Tag::Snapshot, "mail.snapshot", json!([]));
        }
    }
    fn context(&mut self, input: &Value, now: i64) {
        let context = &input["context"];
        let revision = context["localeRevision"].as_u64().unwrap_or(0);
        if revision != self.locale_revision {
            self.base.i18n.invalidate();
            self.last_tree = Value::Null;
            self.locale_revision = revision;
        }
        let base = &mut self.base;
        base.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
        if context["theme"].is_object() {
            base.theme = Rc::new(context["theme"].clone());
        }
        base.dark = base.theme["mode"] != "light";
        base.rtl = context["direction"] == "rtl";
        if let Some(region) = context["region"].as_str() {
            base.region = region.to_owned();
        }
        if let Some(width) = context["width"].as_f64().filter(|w| w.is_finite() && *w > 1.) {
            base.width = width;
        }
        if let Some(height) = context["height"].as_f64().filter(|h| h.is_finite() && *h > 1.) {
            base.height = height;
        }
        base.focused = context["focused"] != false;
        let args = &input["args"];
        if !self.booted {
            let Some(owner) = args["__nativeOwner"].as_i64().filter(|o| *o > 0) else { return };
            base.jobs.bind(owner);
            base.window = str_of(&args["__nativeWindow"]).to_owned();
            self.booted = true;
            let draft = &args["compose"];
            if draft.is_object() || *draft == true {
                self.screen = Screen::Starting(draft.clone());
            }
            let events = json!([["changed", "folders", "status", "ready"]]);
            self.base.service(Tag::Watch, "pim.watch", events);
            self.want_snapshot();
        }
        // opened from a notification, or by another app
        if let Screen::Main(main) = &mut self.screen {
            main.arguments(&mut self.base, args, now);
        }
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        match done.tag {
            Tag::Ignore => {}
            Tag::Watch | Tag::Report => {
                if let Err(failure) = answer(done.result) {
                    self.report(failure);
                }
            }
            Tag::Snapshot => {
                self.snapshot_pending = false;
                if let Ok(v) = done.result {
                    let facts = &mut self.base.facts;
                    facts.ready = true;
                    facts.available = v["available"] == true;
                    facts.isolated = v["isolated"] == true;
                    facts.config = v["config"].clone();
                    facts.status = v["status"].clone();
                    facts.cache_dir = str_of(&v["cacheDir"]).to_owned();
                    match &mut self.screen {
                        Screen::Starting(draft) => {
                            let draft = draft.take();
                            self.screen = Screen::Compose(Box::new(compose::Compose::new(&mut self.base, &draft, now)));
                        }
                        Screen::Main(main) => main.facts_changed(&mut self.base),
                        Screen::Compose(_) => {}
                    }
                }
                if std::mem::take(&mut self.snapshot_stale) {
                    self.want_snapshot();
                }
            }
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + EVENTS_MS;
                for event in done.result.ok().as_ref().map(list_of).unwrap_or(&[]) {
                    if event["ev"] == "mailState" {
                        self.want_snapshot();
                    } else if event["kind"] == "action" {
                        self.command(str_of(&event["action"]), &Value::Null, now);
                    } else if event["kind"] == "closeRequested" {
                        if let Screen::Compose(compose) = &mut self.screen {
                            compose.close_requested(&mut self.base, now);
                        }
                    } else if let Screen::Main(main) = &mut self.screen {
                        main.mail_event(event, now);
                    }
                }
            }
            Tag::Main(tag) => {
                if let Screen::Main(main) = &mut self.screen {
                    main.done(&mut self.base, tag, done.result, now);
                }
            }
            Tag::Sheet(tag) => {
                if let Screen::Main(main) = &mut self.screen {
                    main.sheet_done(&mut self.base, tag, done.result, now);
                }
            }
            Tag::Compose(tag) => {
                if let Screen::Compose(compose) = &mut self.screen {
                    compose.done(&mut self.base, tag, done.result, now);
                }
            }
        }
    }
    fn report(&mut self, failure: Failure) {
        match &mut self.screen {
            Screen::Main(main) => main.load_error = failure.message,
            Screen::Compose(compose) => compose.error = failure.message,
            Screen::Starting(_) => {}
        }
    }
    fn command(&mut self, command: &str, input: &Value, now: i64) {
        match &mut self.screen {
            Screen::Main(main) => main.command(&mut self.base, command, input, now),
            Screen::Compose(compose) => compose.command(&mut self.base, command, input, now),
            Screen::Starting(_) => {}
        }
    }
    /// What goes back after any request.
    fn output(&mut self, now: i64) -> Value {
        let mut out = json!({});
        if self.booted {
            if let Screen::Compose(compose) = &mut self.screen {
                compose.sync_shell(&mut self.base);
            }
            if let Some(effect) = self.base.jobs.pump(now) {
                out["effect"] = effect;
            }
        }
        if !self.base.effects.is_empty() {
            out["effects"] = json!(std::mem::take(&mut self.base.effects));
        }
        match &self.screen {
            Screen::Main(main) => out["capture"] = json!(main.capturing()),
            Screen::Compose(compose) => {
                out["capture"] = json!(compose.capturing());
                let subject = compose.subject();
                out["title"] = if subject.is_empty() { json!({"key":"mail.compose.newMessage"}) } else { json!(subject) };
            }
            Screen::Starting(_) => {}
        }
        out
    }
    fn sync(&mut self, input: &Value, now: i64) -> Value {
        self.context(input, now);
        if self.booted {
            if !self.events_pending && now >= self.events_at {
                self.events_pending = true;
                self.base.service(Tag::Events, "pim.events", json!([]));
            }
            match &mut self.screen {
                Screen::Main(main) => main.tick(&mut self.base, now),
                Screen::Compose(compose) => compose.tick(&mut self.base, now),
                Screen::Starting(_) => {}
            }
        }
        // the tree first: drawing a message asks for the pictures it still needs
        let tree = match &mut self.screen {
            Screen::Main(main) => ui::tree(&mut self.base, main, now),
            Screen::Compose(compose) => compose.tree(&mut self.base, now),
            Screen::Starting(_) => json!({"kind":"view","key":"starting","style":{"flex":1,"backgroundColor":self.base.token("windowBg", "#242428")}}),
        };
        if let Screen::Main(main) = &mut self.screen {
            let menus = ui::menus(&mut self.base, main);
            if self.booted && !self.base.window.is_empty() && menus != self.menus {
                self.menus = menus.clone();
                let window = self.base.window.clone();
                self.base.service(Tag::Ignore, "windows.menus", json!([window, menus]));
            }
        }
        let mut out = self.output(now);
        if tree == self.last_tree {
            out["same"] = json!(true);
        } else {
            out["tree"] = tree.clone();
            self.last_tree = tree;
        }
        out
    }
    pub(crate) fn request_at(&mut self, input: &Value, now: i64) -> Value {
        match str_of(&input["cmd"]) {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => return self.sync(input, now),
            "next" => {
                for done in self.base.jobs.complete(&input["result"], now) {
                    self.apply(done, now);
                }
            }
            "noop" => {}
            command => self.command(command, input, now),
        }
        self.output(now)
    }
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool {
        true
    }
    fn set_native_appearance(&mut self, theme: Rc<Value>) {
        self.base.theme = theme;
    }
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.base.i18n = I18n::new(assets, home);
        self.base.home = home.unwrap_or("").into();
    }
    fn uses_preferences(&self) -> bool {
        true
    }
    fn sync_with_preferences(&mut self, input: &Value, preferences: &Value) -> Value {
        self.base.hour = str_of(&preferences["clock"]["hour24"]).to_owned();
        if let Some(region) = preferences["region"].as_str() {
            self.base.region = region.to_owned();
        }
        self.request_at(input, crate::clock::now())
    }
    fn refresh_ms(&self) -> Option<f64> {
        let typing = matches!(&self.screen, Screen::Compose(compose) if compose.takes_text());
        self.booted.then(|| if self.base.jobs.busy() || typing { 50. } else { EVENTS_MS as f64 })
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        // new mail shows up also while the window is not focused
        self.booted.then(|| if self.base.jobs.busy() { 100. } else { 1000. })
    }
    fn text_input(&self) -> bool {
        matches!(&self.screen, Screen::Compose(compose) if self.base.focused && compose.takes_text())
    }
    fn blur(&mut self) {
        if let Screen::Compose(compose) = &mut self.screen {
            compose.blur();
        }
    }
    fn paste(&mut self, text: &str) -> Option<Value> {
        let now = crate::clock::now();
        match &mut self.screen {
            Screen::Compose(compose) if compose.takes_text() => compose.paste(text, now),
            _ => return None,
        }
        Some(self.output(now))
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        self.key_event(key, chr, flags, &KeyFacts::default())
    }
    fn key_event(&mut self, key: &str, chr: &str, flags: i32, facts: &KeyFacts) -> Option<Value> {
        let now = crate::clock::now();
        let handled = match &mut self.screen {
            Screen::Main(main) => main.key(&mut self.base, key, flags, facts, now),
            Screen::Compose(compose) => compose.key(&mut self.base, key, chr, flags, facts, now),
            Screen::Starting(_) => false,
        };
        handled.then(|| self.output(now))
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
}
