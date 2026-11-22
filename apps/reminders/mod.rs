//! Reminders owns its model, scene, editors, calendar and service scheduling in
//! Rust. axia-maild remains the authoritative local/CalDAV store and alarm source.
pub mod model;
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    date_format,
    i18n::I18n,
    NativeApp,
};
use model::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
enum Tag {
    Lists(u64),
    Items(u64),
    Watch,
    Events,
    Ignore,
    Paste(u64),
    NewList(Value),
    SaveTodo,
    WriteList(bool),
    Toggle(String, u64),
    Mutate,
}
struct Inspector {
    item: Value,
    draft: Value,
}
struct ListDraft {
    id: Option<String>,
    name: String,
    color: String,
    error: bool,
}
pub struct App {
    jobs: Jobs<Tag>,
    booted: bool,
    epoch: u64,
    i18n: I18n,
    theme: Value,
    region: String,
    hour: String,
    rtl: bool,
    window: String,
    sel: String,
    show_done: bool,
    lists: Vec<Value>,
    items: Vec<Value>,
    loaded: bool,
    items_loaded: bool,
    unavailable: bool,
    events_pending: bool,
    events_at: i64,
    reload_at: Option<i64>,
    last_tree: Value,
    list_draft: Option<ListDraft>,
    inspector: Option<Inspector>,
    menu: Option<String>,
    picker: String,
    picker_month: i64,
    new_active: bool,
    new_text: String,
    new_error: bool,
    editing: Option<(String, String)>,
    hover: String,
    notes_focus: bool,
    notes_since: i64,
    toggles: BTreeMap<String, (u64, bool)>,
    next_toggle: u64,
    inspector_seq: u64,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            booted: false,
            epoch: 0,
            i18n: I18n::default(),
            theme: Value::Null,
            region: String::new(),
            hour: String::new(),
            rtl: false,
            window: String::new(),
            sel: "today".into(),
            show_done: false,
            lists: vec![],
            items: vec![],
            loaded: false,
            items_loaded: false,
            unavailable: false,
            events_pending: false,
            events_at: 0,
            reload_at: None,
            last_tree: Value::Null,
            list_draft: None,
            inspector: None,
            menu: None,
            picker: String::new(),
            picker_month: 0,
            new_active: false,
            new_text: String::new(),
            new_error: false,
            editing: None,
            hover: String::new(),
            notes_focus: false,
            notes_since: 0,
            toggles: BTreeMap::new(),
            next_toggle: 0,
            inspector_seq: 0,
        }
    }
}
impl NativeApp for App {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted
            .then_some(if self.jobs.busy() || self.notes_focus {
                50.
            } else {
                200.
            })
    }
    fn uses_preferences(&self) -> bool { true }
    fn sync_with_preferences(&mut self, v: &Value, preferences: &Value) -> Value {
        self.request_with_preferences_at(v, preferences, crate::clock::now())
    }
    fn request(&mut self, v: &Value) -> Value {
        self.request_at(v, crate::clock::now())
    }
    fn text_input(&self) -> bool {
        self.notes_focus
    }
    fn blur(&mut self) {
        self.notes_focus = false;
    }
    fn paste(&mut self, text: &str) -> Option<Value> {
        self.paste_at(text, crate::clock::now())
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        self.key_at(key, chr, flags, crate::clock::now())
    }
}
impl App {
    pub fn paste_at(&mut self, text: &str, now: i64) -> Option<Value> {
        if !self.notes_focus {
            return None;
        }
        self.notes_since = now;
        if let Some(i) = &mut self.inspector {
            i.draft["notes"] = json!(format!("{}{text}", string(&i.draft["notes"])));
        }
        Some(self.output(now))
    }
    pub fn key_at(&mut self, key: &str, chr: &str, flags: i32, now: i64) -> Option<Value> {
        if self.notes_focus {
            if key == "Escape" {
                self.notes_focus = false;
            } else if flags & (2 | 16) != 0 {
                if key.eq_ignore_ascii_case("v") {
                    self.jobs
                        .enqueue(Tag::Paste(self.inspector_seq), "clipboard.read", json!([]));
                    return Some(self.output(now));
                }
                return None;
            } else if let Some(i) = &mut self.inspector {
                let mut text = string(&i.draft["notes"]).to_owned();
                if key == "Backspace" {
                    text.pop();
                } else if key == "Enter" {
                    text.push('\n');
                } else if !chr.is_empty() {
                    text.push_str(chr);
                } else {
                    return None;
                }
                i.draft["notes"] = json!(text);
                self.notes_since = now;
            }
            return Some(self.output(now));
        }
        if key == "Escape" {
            if !self.picker.is_empty() {
                self.picker.clear();
            } else if self.list_draft.take().is_some() {
            } else if self.menu.take().is_some() {
            } else if self.editing.take().is_some() {
            } else if self.new_active {
                self.new_active = false;
                self.new_text.clear();
            } else {
                return None;
            }
            return Some(self.output(now));
        }
        if flags & (2 | 16) != 0
            && key.eq_ignore_ascii_case("n")
            && self.list_draft.is_none()
            && self.sel != "completed"
            && !self.unavailable
        {
            self.new_active = true;
            return Some(self.output(now));
        }
        None
    }
}
impl App {
    fn tr(&mut self, k: &str) -> String {
        self.i18n.caption(&json!({"key":format!("reminders.{k}")}))
    }
    fn date(&mut self, day: i64, style: &str) -> String {
        date_format::date(
            &mut self.i18n,
            &self.region,
            &crate::clock::local(day),
            style,
        )
    }
    fn time(&mut self, day: i64) -> String {
        date_format::time(
            &mut self.i18n,
            &self.region,
            &self.hour,
            &crate::clock::local(day),
            false,
        )
    }
    fn call(&mut self, tag: Tag, op: &str, args: Value) {
        self.jobs.enqueue(tag, "pim.request", json!([op, args]));
    }
    fn reload(&mut self) {
        self.epoch += 1;
        self.jobs
            .cancel(|t| matches!(t, Tag::Lists(_) | Tag::Items(_)));
        self.call(Tag::Lists(self.epoch), "cal-list", json!({}));
    }
    fn remember(&mut self) {
        self.jobs.enqueue(
            Tag::Ignore,
            "windows.state",
            json!([self.window,{"sel":self.sel,"showDone":self.show_done}]),
        );
    }
    fn item(&self, k: &str) -> Option<Value> {
        self.items.iter().find(|it| key(it) == k).cloned()
    }
    fn commit_title(&mut self) {
        if let Some((k, title)) = self.editing.take() {
            if let Some(mut it) = self.item(&k) {
                if !title.trim().is_empty() && title != string(&it["title"]) {
                    it["title"] = json!(title.trim());
                    self.call(
                        Tag::Mutate,
                        "todo-save",
                        json!({"list":it["list"],"todo":it}),
                    );
                }
            }
        }
    }
    fn add(&mut self, now: i64) {
        let title = self.new_text.trim().to_owned();
        if title.is_empty() {
            self.new_active = false;
            return;
        }
        if self.sel == "completed" || self.unavailable {
            return;
        }
        self.new_text.clear();
        let mut todo = json!({"title":title,"notes":"","prio":0,"flag":self.sel=="flagged","done":false,"alarms":[],"due":null,"dueAllDay":false});
        if self.sel == "today" || self.sel == "scheduled" {
            todo["due"] = json!(midnight(now));
            todo["dueAllDay"] = json!(true);
        }
        let list = if let Some(id) = self.sel.strip_prefix("list:") {
            self.lists.iter().find(|l| l["id"] == id)
        } else {
            self.lists.first()
        };
        if let Some(list) = list {
            self.call(
                Tag::SaveTodo,
                "todo-save",
                json!({"list":list["id"],"todo":todo}),
            );
        } else {
            let name = self.tr("defaultList");
            self.call(
                Tag::NewList(todo),
                "cal-create",
                json!({"name":name,"color":COLORS[5],"todos":true,"events":false}),
            );
        }
    }
    fn inspect(&mut self, k: &str, now: i64) {
        let Some(it) = self.item(k) else {
            return;
        };
        self.inspector_seq += 1;
        self.commit_title();
        self.picker.clear();
        self.notes_focus = false;
        let due = it["due"].as_i64();
        let day = due
            .map(|d| {
                if it["dueAllDay"] == true {
                    parse_day(string(&it["dueDate"])).unwrap_or_else(|| midnight(d))
                } else {
                    midnight(d)
                }
            })
            .unwrap_or_else(|| midnight(now));
        let min = due
            .filter(|_| it["dueAllDay"] != true)
            .map(|d| {
                let t = crate::clock::local(d);
                t.tm_hour * 60 + t.tm_min
            })
            .unwrap_or(540);
        let draft = json!({"title":it["title"].as_str().unwrap_or(""),"notes":it["notes"].as_str().unwrap_or(""),"prio":it["prio"].as_i64().unwrap_or(0),"flag":it["flag"]==true,"list":it["list"],"mode":if due.is_none(){"none"}else if it["dueAllDay"]==true{"date"}else{"time"},"day":day,"min":min});
        self.inspector = Some(Inspector { item: it, draft });
    }
    fn save_inspector(&mut self) {
        self.notes_focus = false;
        self.picker.clear();
        let Some(Inspector {
            item: mut todo,
            draft: d,
        }) = self.inspector.take()
        else {
            return;
        };
        let old = todo["list"].clone();
        for k in ["notes", "prio", "flag"] {
            todo[k] = d[k].clone();
        }
        if !string(&d["title"]).trim().is_empty() {
            todo["title"] = json!(string(&d["title"]).trim());
        }
        let mode = string(&d["mode"]);
        todo["dueAllDay"] = json!(mode == "date");
        todo["due"] = match mode {
            "date" => json!(midnight(d["day"].as_i64().unwrap_or(0))),
            "time" => json!(at_minutes(
                d["day"].as_i64().unwrap_or(0),
                d["min"].as_i64().unwrap_or(540) as i32
            )),
            _ => Value::Null,
        };
        if mode != "time" {
            todo["alarms"] = json!([]);
        } else if !todo["alarms"].as_array().is_some_and(|a| !a.is_empty()) {
            todo["alarms"] = json!([0]);
        }
        todo.as_object_mut().unwrap().remove("dueDate");
        let mut args = json!({"list":d["list"],"todo":todo});
        if d["list"] != old {
            args["from"] = old;
        }
        self.call(Tag::Mutate, "todo-save", args);
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        match done.tag {
            Tag::Lists(e) if e == self.epoch => match done.result {
                Ok(v) => {
                    self.lists = v["cals"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|l| l["todos"] == true)
                        .cloned()
                        .collect();
                    self.loaded = true;
                    self.unavailable = false;
                    if self.sel.starts_with("list:")
                        && !self
                            .lists
                            .iter()
                            .any(|l| format!("list:{}", string(&l["id"])) == self.sel)
                    {
                        self.sel = "today".into();
                        self.new_active = false;
                        self.editing = None;
                        self.remember();
                    }
                    if self.lists.is_empty() {
                        self.items.clear();
                        self.items_loaded = true;
                    } else {
                        self.call(Tag::Items(e),"todo-list",json!({"lists":self.lists.iter().map(|l|l["id"].clone()).collect::<Vec<_>>()}));
                    }
                }
                Err(_) => self.unavailable = !self.loaded,
            },
            Tag::Items(e) if e == self.epoch => {
                if let Ok(v) = done.result {
                    self.items = v["todos"].as_array().cloned().unwrap_or_default();
                    self.items_loaded = true;
                }
            }
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + 200;
                if done.result.ok().as_ref().is_some_and(|v| {
                    v.as_array().is_some_and(|a| {
                        a.iter()
                            .any(|e| ["pim-changed", "ready", "crash"].contains(&string(&e["ev"])))
                    })
                }) {
                    self.reload_at = Some(now);
                }
            }
            Tag::NewList(todo) => match done.result {
                Ok(v) if v["uid"].is_string() => self.call(
                    Tag::SaveTodo,
                    "todo-save",
                    json!({"list":v["uid"],"todo":todo}),
                ),
                _ => self.new_error = true,
            },
            Tag::SaveTodo => {
                self.new_error = done.result.is_err();
                if !self.new_error {
                    self.reload_at = Some(now);
                }
            }
            Tag::WriteList(created) => match done.result {
                Ok(v) => {
                    if created && v["uid"].is_string() {
                        self.sel = format!("list:{}", string(&v["uid"]));
                        self.remember();
                    }
                    self.list_draft = None;
                    self.reload_at = Some(now);
                }
                Err(_) => {
                    if let Some(d) = &mut self.list_draft {
                        d.error = true;
                    }
                }
            },
            Tag::Toggle(k, seq) => {
                if self.toggles.get(&k).is_some_and(|t| t.0 == seq) {
                    let (_, done_value) = self.toggles.remove(&k).unwrap();
                    if done.result.is_ok() {
                        if let Some(it) = self.items.iter_mut().find(|it| key(it) == k) {
                            it["done"] = json!(done_value);
                        }
                    }
                }
                if done.result.is_ok() {
                    self.reload_at = Some(now);
                }
            }
            Tag::Mutate => {
                if done.result.is_ok() {
                    self.reload_at = Some(now);
                }
            }
            Tag::Paste(seq) => {
                if self.notes_focus && seq == self.inspector_seq {
                    if let Ok(v) = done.result {
                        if let Some(i) = &mut self.inspector {
                            i.draft["notes"] = json!(format!(
                                "{}{}",
                                string(&i.draft["notes"]),
                                string(&v["text"])
                            ));
                            self.notes_since = now;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn output(&mut self, now: i64) -> Value {
        if self.reload_at.is_some_and(|t| t <= now) {
            self.reload_at = None;
            self.reload();
        }
        if self.booted && !self.events_pending && now >= self.events_at {
            self.events_pending = true;
            self.jobs.enqueue(Tag::Events, "pim.events", json!([]));
        }
        let effect = self.jobs.pump(now);
        let mut out = json!({});
        if let Some(effect) = effect {
            out["effect"] = effect;
        }
        out
    }
    pub fn request_at(&mut self, v: &Value, now: i64) -> Value {
        self.request_with_preferences_at(v, &v["settings"], now)
    }
    fn request_with_preferences_at(&mut self, v: &Value, preferences: &Value, now: i64) -> Value {
        let cmd = string(&v["cmd"]);
        let k = string(&v["key"]);
        match cmd {
            "icon" => return ui::icon(v["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                let lang = v["lang"].as_str().unwrap_or("en");
                if self.i18n.lang != lang {
                    self.last_tree = Value::Null;
                }
                self.i18n.lang = lang.into();
                self.theme = v["context"]["theme"].clone();
                self.region = v["context"]["region"]
                    .as_str()
                    .unwrap_or(string(&preferences["region"]))
                    .into();
                self.hour = string(&preferences["clock"]["hour24"]).into();
                let lang = self.i18n.lang.clone();
                self.rtl = v["context"]["direction"]
                    .as_str()
                    .map(|d| d == "rtl")
                    .unwrap_or_else(|| {
                        self.i18n.entry(&lang, "meta", "direction") == Some(json!("rtl"))
                    });
                if !self.booted {
                    if let Some(owner) = v["args"]["__nativeOwner"].as_i64().filter(|i| *i > 0) {
                        self.jobs.bind(owner);
                        self.booted = true;
                        self.window = string(&v["args"]["__nativeWindow"]).into();
                        let s = &v["args"]["__nativeState"];
                        if let Some(sel) = s["sel"].as_str() {
                            self.sel = sel.into();
                        }
                        self.show_done = s["showDone"] == true;
                        self.jobs.enqueue(Tag::Watch, "pim.watch", json!([]));
                        self.reload();
                    }
                }
            }
            "next" => {
                for done in self.jobs.complete(&v["result"], now) {
                    self.apply(done, now);
                }
            }
            "select" => {
                self.commit_title();
                self.sel = k.into();
                self.new_active = false;
                self.new_text.clear();
                self.inspector = None;
                self.menu = None;
                self.picker.clear();
                self.remember();
            }
            "showDone" => {
                self.show_done = !self.show_done;
                self.remember();
            }
            "new" => {
                if self.sel != "completed" && !self.unavailable {
                    self.new_active = true;
                }
            }
            "newText" => self.new_text = string(&v["value"]).into(),
            "add" => self.add(now),
            "cancelNew" => {
                self.new_text.clear();
                self.new_active = false;
            }
            "editTitle" => {
                self.commit_title();
                if let Some(it) = self.item(k) {
                    self.editing = Some((k.into(), string(&it["title"]).into()));
                }
            }
            "titleText" => {
                if let Some((_, text)) = &mut self.editing {
                    *text = string(&v["value"]).into();
                }
            }
            "commitTitle" => self.commit_title(),
            "cancelTitle" => self.editing = None,
            "toggle" => {
                if let Some(mut it) = self.item(k) {
                    let done = !self
                        .toggles
                        .get(k)
                        .map(|t| t.1)
                        .unwrap_or(it["done"] == true);
                    it["done"] = json!(done);
                    self.next_toggle += 1;
                    self.toggles.insert(k.into(), (self.next_toggle, done));
                    self.call(
                        Tag::Toggle(k.into(), self.next_toggle),
                        "todo-save",
                        json!({"list":it["list"],"todo":it}),
                    );
                }
            }
            "hover" => self.hover = k.into(),
            "leave" => {
                if self.hover == k {
                    self.hover.clear();
                }
            }
            "inspect" => self.inspect(k, now),
            "saveInspector" => self.save_inspector(),
            "draft" => {
                if let Some(i) = &mut self.inspector {
                    if [
                        "title", "notes", "prio", "flag", "list", "mode", "day", "min",
                    ]
                    .contains(&k)
                    {
                        i.draft[k] = v["value"].clone();
                    }
                }
            }
            "draftPick" => {
                if let Some(i) = &mut self.inspector {
                    i.draft[k] = v["value"].clone();
                }
                self.picker.clear();
            }
            "setMode" => {
                if let Some(i) = &mut self.inspector {
                    i.draft["mode"] = json!(k);
                }
            }
            "toggleFlag" => {
                if let Some(i) = &mut self.inspector {
                    i.draft["flag"] = json!(i.draft["flag"] != true);
                }
            }
            "notesFocus" => {
                self.notes_focus = true;
                self.notes_since = now;
            }
            "deleteTodo" => {
                if let Some(i) = self.inspector.take() {
                    self.notes_focus = false;
                    self.picker.clear();
                    self.call(
                        Tag::Mutate,
                        "todo-delete",
                        json!({"list":i.item["list"],"uid":i.item["uid"]}),
                    );
                }
            }
            "listMenu" => {
                self.menu = Some(k.into());
            }
            "closeMenu" => self.menu = None,
            "editList" | "newList" => {
                self.menu = None;
                self.list_draft = if cmd == "newList" {
                    Some(ListDraft {
                        id: None,
                        name: String::new(),
                        color: COLORS[5].into(),
                        error: false,
                    })
                } else {
                    self.lists.iter().find(|l| l["id"] == k).map(|l| ListDraft {
                        id: Some(k.into()),
                        name: string(&l["name"]).into(),
                        color: l["color"].as_str().unwrap_or(COLORS[5]).into(),
                        error: false,
                    })
                };
            }
            "listName" => {
                if let Some(d) = &mut self.list_draft {
                    d.name = string(&v["value"]).into();
                }
            }
            "listColor" => {
                if let Some(d) = &mut self.list_draft {
                    d.color = k.into();
                }
            }
            "cancelList" => self.list_draft = None,
            "saveList" => {
                if let Some(d) = &self.list_draft {
                    if !d.name.trim().is_empty() {
                        let mut args = json!({"name":d.name.trim(),"color":d.color});
                        let created = d.id.is_none();
                        if let Some(id) = &d.id {
                            args["id"] = json!(id);
                        } else {
                            args["todos"] = json!(true);
                            args["events"] = json!(false);
                        }
                        self.call(
                            Tag::WriteList(created),
                            if created { "cal-create" } else { "cal-update" },
                            args,
                        );
                    }
                }
            }
            "deleteList" => {
                self.menu = None;
                if self
                    .lists
                    .iter()
                    .any(|l| l["id"] == k && l["kind"] == "local")
                {
                    self.call(Tag::Mutate, "cal-delete", json!({"id":k}));
                }
            }
            "picker" => {
                self.picker = if self.picker == k {
                    String::new()
                } else {
                    k.into()
                };
                if k == "day" {
                    self.picker_month = month(
                        self.inspector
                            .as_ref()
                            .and_then(|i| i.draft["day"].as_i64())
                            .unwrap_or(now),
                        0,
                    );
                }
            }
            "closePicker" => self.picker.clear(),
            "month" => {
                self.picker_month = if k == "today" {
                    month(now, 0)
                } else {
                    month(self.picker_month, k.parse().unwrap_or(0))
                }
            }
            "pickDay" => {
                if let Some(i) = &mut self.inspector {
                    i.draft["day"] = v["key"].clone();
                }
                self.picker.clear();
            }
            "pickValue" => {
                if let Some(i) = &mut self.inspector {
                    let field = self.picker.clone();
                    i.draft[&field] = v["key"].clone();
                }
                self.picker.clear();
            }
            _ => {}
        }
        let mut out = self.output(now);
        if cmd == "sync" {
            let tree = ui::tree(self, now);
            if tree != self.last_tree {
                self.last_tree = tree.clone();
                out["tree"] = tree;
            } else {
                out["same"] = json!(true);
            }
        }
        out
    }
}
