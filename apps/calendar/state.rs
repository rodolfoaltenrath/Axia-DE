//! Window-owned PIM requests. Generations isolate navigation, dialogs and alarms.
use super::model::*;
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    civil_date as date,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[derive(Clone)]
struct Write {
    seq: u64,
    original: Value,
    draft: Value,
    new: bool,
    first: Option<i64>,
}
enum Tag {
    Cals(u64),
    Range(u64),
    Watch,
    Poll,
    Ignore,
    Series(Write),
    CreateCal(Write),
    Save(Write),
    Delete(u64),
    SaveCal(u64),
    Mutate,
    Paste(u64),
}
pub(super) struct EventEdit {
    pub original: Value,
    pub draft: Value,
    pub new: bool,
    pub busy: bool,
    pub error: String,
    seq: u64,
}
pub(super) struct CalEdit {
    pub id: Option<String>,
    pub name: String,
    pub color: String,
    pub busy: bool,
    pub error: String,
    seq: u64,
}
pub(super) struct State {
    jobs: Jobs<Tag>,
    pub cals: Vec<Value>,
    pub events: Vec<Value>,
    pub days: BTreeMap<String, Vec<usize>>,
    pub view: String,
    pub cursor: i64,
    pub sidebar: bool,
    pub narrow_sidebar: bool,
    pub event: Option<EventEdit>,
    pub calendar: Option<CalEdit>,
    pub menu: String,
    pub notes_focus: bool,
    pub booted: bool,
    pub unavailable: bool,
    pub default_name: String,
    window: String,
    seq: u64,
    cal_epoch: u64,
    range_epoch: u64,
    loaded: bool,
    poll_pending: bool,
    poll_at: i64,
    reload_at: Option<i64>,
    requested: Option<(i64, i64)>,
    alarm_signature: Value,
    pending_alarm: Value,
}
impl Default for State {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            cals: vec![],
            events: vec![],
            days: BTreeMap::new(),
            view: "week".into(),
            cursor: 0,
            sidebar: true,
            narrow_sidebar: false,
            event: None,
            calendar: None,
            menu: String::new(),
            notes_focus: false,
            booted: false,
            unavailable: false,
            default_name: String::new(),
            window: String::new(),
            seq: 0,
            cal_epoch: 0,
            range_epoch: 0,
            loaded: false,
            poll_pending: false,
            poll_at: 0,
            reload_at: None,
            requested: None,
            alarm_signature: Value::Null,
            pending_alarm: Value::Null,
        }
    }
}
impl State {
    pub fn busy(&self) -> bool {
        self.jobs.busy()
    }
    pub fn event_cals(&self) -> Vec<Value> {
        self.cals
            .iter()
            .filter(|c| c["events"] == true)
            .cloned()
            .collect()
    }
    fn call(&mut self, tag: Tag, op: &str, args: Value) {
        self.jobs.enqueue(tag, "pim.request", json!([op, args]));
    }
    fn remember(&mut self) {
        self.jobs.enqueue(Tag::Ignore,"windows.state",json!([self.window,{"view":self.view,"cursor":self.cursor,"sidebar":self.sidebar,"narrowSidebar":self.narrow_sidebar}]));
    }
    fn ensure_range(&mut self) {
        if !self.booted {
            return;
        }
        let wanted = range(&self.view, self.cursor);
        if self.requested == Some(wanted) {
            return;
        }
        self.requested = Some(wanted);
        self.range_epoch += 1;
        self.jobs.cancel(|t| matches!(t, Tag::Range(_)));
        self.call(
            Tag::Range(self.range_epoch),
            "ev-list",
            json!({"from":wanted.0,"to":wanted.1}),
        );
    }
    fn reload(&mut self) {
        self.cal_epoch += 1;
        self.jobs.cancel(|t| matches!(t, Tag::Cals(_)));
        self.call(Tag::Cals(self.cal_epoch), "cal-list", json!({}));
        self.requested = None;
        self.ensure_range();
    }
    fn close(&mut self) {
        self.seq += 1;
        self.event = None;
        self.calendar = None;
        self.notes_focus = false;
        self.menu.clear();
    }
    fn edit(&mut self, e: Value, new: bool) {
        self.close();
        let draft = draft(&e, &self.event_cals());
        self.event = Some(EventEdit {
            original: e,
            draft,
            new,
            busy: false,
            error: String::new(),
            seq: self.seq,
        });
    }
    fn edit_cal(&mut self, c: Option<Value>) {
        self.close();
        self.calendar = Some(CalEdit {
            id: c.as_ref().map(|c| s(&c["id"]).to_owned()),
            name: c
                .as_ref()
                .map(|c| s(&c["name"]).to_owned())
                .unwrap_or_default(),
            color: c
                .as_ref()
                .map(|c| c["color"].as_str().unwrap_or(COLORS[5]).to_owned())
                .unwrap_or(COLORS[5].into()),
            busy: false,
            error: String::new(),
            seq: self.seq,
        });
    }
    fn fail(&mut self, seq: u64, key: &str) {
        if let Some(e) = self.event.as_mut().filter(|e| e.seq == seq) {
            e.busy = false;
            e.error = format!("calendar.{key}");
        }
    }
    fn write(&mut self, w: Write) {
        if s(&w.draft["cal"]).is_empty() {
            self.call(
                Tag::CreateCal(w),
                "cal-create",
                json!({"name":self.default_name,"color":COLORS[5],"events":true,"todos":false}),
            );
        } else {
            let args = save_args(&w.original, &w.draft, w.new, w.first);
            self.call(Tag::Save(w), "ev-save", args);
        }
    }
    fn save(&mut self) {
        let Some(e) = self.event.as_mut().filter(|e| !e.busy) else {
            return;
        };
        e.busy = true;
        e.error.clear();
        let w = Write {
            seq: e.seq,
            original: e.original.clone(),
            draft: e.draft.clone(),
            new: e.new,
            first: None,
        };
        if recurring(&w.original, w.new) {
            let args = json!({"from":0,"to":num(&w.original["occ"]).saturating_add(1),"cals":[w.original["cal"]]});
            self.call(Tag::Series(w), "ev-list", args);
        } else {
            self.write(w);
        }
    }
    fn alarm(&mut self, args: &Value) {
        let Some(at) = args["at"].as_i64().filter(|n| *n != 0) else {
            return;
        };
        let signature = json!([args["nonce"], at]);
        if signature == self.alarm_signature {
            return;
        }
        self.alarm_signature = signature;
        self.pending_alarm = args.clone();
        self.cursor = date::day(at.clamp(-62135596800000, 253402300799999));
        if self.view == "month" {
            self.view = "day".into();
        }
        self.remember();
    }
    fn resolve_alarm(&mut self) {
        let p = &self.pending_alarm;
        if s(&p["uid"]).is_empty() {
            return;
        }
        if let Some(e) = self
            .events
            .iter()
            .find(|e| {
                e["uid"] == p["uid"]
                    && (s(&p["cal"]).is_empty() || e["cal"] == p["cal"])
                    && (e["start"] == p["at"] || num(&e["occ"]) == 0)
            })
            .cloned()
        {
            self.pending_alarm = Value::Null;
            self.edit(e, false);
        }
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        match done.tag {
            Tag::Cals(epoch) if epoch == self.cal_epoch => match done.result {
                Ok(v) => {
                    self.cals = v["cals"].as_array().cloned().unwrap_or_default();
                    self.loaded = true;
                    self.unavailable = false;
                }
                Err(_) => self.unavailable = !self.loaded,
            },
            Tag::Range(epoch) if epoch == self.range_epoch => {
                if let Ok(v) = done.result {
                    self.events = v["events"].as_array().cloned().unwrap_or_default();
                    self.days = by_day(&self.events);
                    self.resolve_alarm();
                }
            }
            Tag::Poll => {
                self.poll_pending = false;
                self.poll_at = now + 200;
                if done.result.ok().as_ref().is_some_and(|v| {
                    v.as_array().is_some_and(|a| {
                        a.iter()
                            .any(|e| ["pim-changed", "ready", "crash"].contains(&s(&e["ev"])))
                    })
                }) {
                    self.reload_at = Some(now + 50);
                }
            }
            Tag::Series(mut w) => {
                w.first = done
                    .result
                    .ok()
                    .and_then(|v| {
                        v["events"].as_array().map(|events| {
                            events
                                .iter()
                                .filter(|e| {
                                    e["uid"] == w.original["uid"]
                                        && (e["rid"].is_null()
                                            || e["rid"] == false
                                            || e["rid"] == "")
                                })
                                .filter_map(|e| e["start"].as_i64())
                                .min()
                        })
                    })
                    .flatten();
                self.write(w);
            }
            Tag::CreateCal(mut w) => {
                if let Some(id) = done.result.ok().and_then(|v| {
                    v["uid"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                }) {
                    w.draft["cal"] = json!(id);
                    if let Some(e) = self.event.as_mut().filter(|e| e.seq == w.seq) {
                        e.draft["cal"] = json!(id);
                    }
                    self.write(w);
                } else {
                    self.fail(w.seq, "saveFailed");
                }
            }
            Tag::Save(w) => {
                if done.result.is_ok() {
                    if self.event.as_ref().is_some_and(|e| e.seq == w.seq) {
                        self.close();
                    }
                    self.reload_at = Some(now);
                } else {
                    self.fail(w.seq, "saveFailed");
                }
            }
            Tag::Delete(seq) => {
                if done.result.is_ok() {
                    if self.event.as_ref().is_some_and(|e| e.seq == seq) {
                        self.close();
                    }
                    self.reload_at = Some(now);
                } else {
                    self.fail(seq, "deleteFailed");
                }
            }
            Tag::SaveCal(seq) => {
                if done.result.is_ok() {
                    if self.calendar.as_ref().is_some_and(|c| c.seq == seq) {
                        self.close();
                    }
                    self.reload_at = Some(now);
                } else if let Some(c) = self.calendar.as_mut().filter(|c| c.seq == seq) {
                    c.busy = false;
                    c.error = "calendar.saveFailed".into();
                }
            }
            Tag::Mutate => {
                if done.result.is_ok() {
                    self.reload_at = Some(now);
                }
            }
            Tag::Paste(seq) => {
                if self.notes_focus {
                    if let (Some(e), Ok(v)) = (
                        self.event.as_mut().filter(|e| e.seq == seq && !e.busy),
                        done.result,
                    ) {
                        e.draft["notes"] =
                            json!(format!("{}{}", s(&e.draft["notes"]), s(&v["text"])));
                    }
                }
            }
            _ => {}
        }
    }
    pub fn request(&mut self, v: &Value, now: i64) -> Value {
        let cmd = s(&v["cmd"]);
        let k = s(&v["key"]);
        match cmd {
            "sync" => {
                if !self.booted {
                    if let Some(owner) = v["args"]["__nativeOwner"].as_i64().filter(|i| *i > 0) {
                        self.booted = true;
                        self.jobs.bind(owner);
                        self.window = s(&v["args"]["__nativeWindow"]).into();
                        let state = &v["args"]["__nativeState"];
                        self.cursor = date::day(
                            state["cursor"]
                                .as_i64()
                                .unwrap_or(now)
                                .clamp(-62135596800000, 253402300799999),
                        );
                        if let Some(view) = state["view"]
                            .as_str()
                            .filter(|s| ["day", "week", "month"].contains(s))
                        {
                            self.view = view.into();
                        }
                        self.sidebar = state["sidebar"] != false;
                        self.narrow_sidebar = state["narrowSidebar"] == true;
                        self.jobs.enqueue(
                            Tag::Watch,
                            "pim.watch",
                            json!([["pim-changed", "ready", "crash"]]),
                        );
                        self.alarm(&v["args"]);
                        self.reload();
                    }
                } else {
                    self.alarm(&v["args"]);
                }
            }
            "next" => {
                for done in self.jobs.complete(&v["result"], now) {
                    self.apply(done, now);
                }
            }
            "view" => {
                if ["day", "week", "month"].contains(&k) {
                    self.view = k.into();
                    self.remember();
                }
            }
            "today" => {
                self.cursor = date::day(now);
                self.remember();
            }
            "go" => {
                let n = v["key"]
                    .as_i64()
                    .unwrap_or_else(|| k.parse().unwrap_or(0))
                    .clamp(-120, 120) as i32;
                self.cursor = if self.view == "month" {
                    date::month(self.cursor, n)
                } else {
                    date::add_days(self.cursor, n * if self.view == "day" { 1 } else { 7 })
                };
                self.remember();
            }
            "cursor" | "day" => {
                if let Some(day) = v["key"].as_i64() {
                    self.cursor = date::day(day.clamp(-62135596800000, 253402300799999));
                    if cmd == "day" {
                        self.view = "day".into();
                    }
                    self.remember();
                }
            }
            "sidebar" => {
                if v["narrow"] == true {
                    self.narrow_sidebar = !self.narrow_sidebar;
                } else {
                    self.sidebar = !self.sidebar;
                }
                self.remember();
            }
            "new" if !self.unavailable => {
                let day = v["key"]["day"].as_i64().unwrap_or(self.cursor);
                let min = v["key"]["min"].as_i64().map(|n| n.clamp(0, 1439) as i32);
                self.edit(new_event(&self.cals, day, min, now), true);
            }
            "event" => {
                if let Some(e) = self.events.iter().find(|e| key(e) == k).cloned() {
                    self.edit(e, false);
                }
            }
            "close" => self.close(),
            "draft" => {
                if let Some(e) = self.event.as_mut().filter(|e| !e.busy) {
                    match k {
                        "sDay" => {
                            if let Some(day) = v["value"].as_i64() {
                                set_start_day(&mut e.draft, day);
                            }
                        }
                        "eDay" => {
                            if let Some(day) = v["value"].as_i64() {
                                e.draft[k] = json!(date::day(day).max(num(&e.draft["sDay"])));
                            }
                        }
                        "sMin" => {
                            if let Some(min) = v["value"].as_i64() {
                                set_start_min(&mut e.draft, min.clamp(0, 1439) as i32);
                            }
                        }
                        "eMin" => {
                            if let Some(min) = v["value"].as_i64() {
                                e.draft[k] = json!(min.clamp(0, 1439));
                            }
                        }
                        "title" | "loc" | "notes" | "allDay" | "rrule" | "alarm" | "cal" => {
                            e.draft[k] = v["value"].clone()
                        }
                        _ => {}
                    }
                }
            }
            "allDay" => {
                if let Some(e) = self.event.as_mut().filter(|e| !e.busy) {
                    e.draft["allDay"] = json!(e.draft["allDay"] != true);
                }
            }
            "save" => self.save(),
            "eventDelete" => {
                if let Some(e) = self.event.as_mut().filter(|e| !e.new && !e.busy) {
                    let recur = recurring(&e.original, false);
                    if !recur || ["this", "all"].contains(&k) {
                        e.busy = true;
                        e.error.clear();
                        let mut args = json!({"cal":e.original["cal"],"uid":e.original["uid"]});
                        if k == "this" {
                            args["occ"] = e.original["occ"].clone();
                        }
                        if k == "all" {
                            args["all"] = json!(true);
                        }
                        let seq = e.seq;
                        self.call(Tag::Delete(seq), "ev-delete", args);
                    }
                }
            }
            "calNew" => self.edit_cal(None),
            "calEdit" => {
                if let Some(c) = self.cals.iter().find(|c| c["id"] == k).cloned() {
                    self.edit_cal(Some(c));
                }
            }
            "calDraft" => {
                if let Some(c) = self.calendar.as_mut().filter(|c| !c.busy) {
                    match k {
                        "name" => c.name = s(&v["value"]).into(),
                        "color" => c.color = s(&v["value"]).into(),
                        _ => {}
                    }
                }
            }
            "calSave" => {
                if let Some(c) = self
                    .calendar
                    .as_mut()
                    .filter(|c| !c.busy && !c.name.trim().is_empty())
                {
                    c.busy = true;
                    c.error.clear();
                    let mut args = json!({"name":c.name.trim(),"color":c.color});
                    let op = if let Some(id) = &c.id {
                        args["id"] = json!(id);
                        "cal-update"
                    } else {
                        args["events"] = json!(true);
                        args["todos"] = json!(false);
                        "cal-create"
                    };
                    let seq = c.seq;
                    self.call(Tag::SaveCal(seq), op, args);
                }
            }
            "calToggle" | "calDelete" => {
                if let Some(c) = self.cals.iter().find(|c| c["id"] == k).cloned() {
                    if cmd == "calToggle" {
                        self.call(
                            Tag::Mutate,
                            "cal-update",
                            json!({"id":c["id"],"hidden":c["hidden"]!=true}),
                        );
                    } else if c["kind"] == "local" {
                        self.call(Tag::Mutate, "cal-delete", json!({"id":c["id"]}));
                    }
                }
                self.menu.clear();
            }
            "menu" => self.menu = k.into(),
            "notesFocus" => self.notes_focus = self.event.as_ref().is_some_and(|e| !e.busy),
            "notesPaste" => {
                if let Some(e) = self.event.as_ref().filter(|e| !e.busy && self.notes_focus) {
                    self.jobs
                        .enqueue(Tag::Paste(e.seq), "clipboard.read", json!([]));
                }
            }
            "refresh" => self.reload_at = Some(now),
            _ => {}
        }
        if self.reload_at.is_some_and(|at| at <= now) {
            self.reload_at = None;
            self.reload();
        }
        self.ensure_range();
        if self.booted && !self.poll_pending && now >= self.poll_at {
            self.poll_pending = true;
            self.jobs.enqueue(Tag::Poll, "pim.events", json!([]));
        }
        self.jobs
            .pump(now)
            .map_or_else(|| json!({}), |effect| json!({"effect":effect}))
    }
}
#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
