use axia_app_reminders::model::*;
use axia_app_reminders::*;
use axia_native::NativeApp;
use serde_json::{json, Value};
use crate::{native_view::Host, runtime::native as scene};
use std::{cell::Cell, rc::Rc};
struct Timed {
    app: App,
    now: Rc<Cell<i64>>,
}
impl NativeApp for Timed {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.app.resources(assets, home);
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.app.refresh_ms()
    }
    fn request(&mut self, v: &Value) -> Value {
        self.app.request_at(v, self.now.get())
    }
    fn key(&mut self, k: &str, c: &str, f: i32) -> Option<Value> {
        self.app.key_at(k, c, f, self.now.get())
    }
    fn paste(&mut self, text: &str) -> Option<Value> {
        self.app.paste_at(text, self.now.get())
    }
    fn text_input(&self) -> bool {
        self.app.text_input()
    }
    fn blur(&mut self) {
        self.app.blur();
    }
}
struct Services {
    token: u64,
    ready: BTreeMap<u64, Value>,
    calls: Vec<(String, Value)>,
    lists: Vec<Value>,
    items: Vec<Value>,
    fail_save: bool,
    events: Vec<Value>,
    state: Value,
}
impl Services {
    fn new() -> Self {
        Self {
            token: 0,
            ready: BTreeMap::new(),
            calls: vec![],
            lists: lists(),
            items: sample(),
            fail_save: false,
            events: vec![],
            state: Value::Null,
        }
    }
    fn answer(&mut self, method: &str, args: &Value) -> Result<Value, String> {
        match method {
            "pim.watch" => Ok(Value::Null),
            "pim.events" => Ok(json!(std::mem::take(&mut self.events))),
            "windows.state" => {
                self.state = args[1].clone();
                Ok(Value::Null)
            }
            "clipboard.read" => Ok(json!({"kind":"text","text":"pasted"})),
            "pim.request" => {
                let op = string(&args[0]);
                let a = &args[1];
                if self.fail_save && ["todo-save", "cal-create", "cal-update"].contains(&op) {
                    return Err("unavailable".into());
                }
                match op {
                    "cal-list" => Ok(json!({"cals":self.lists})),
                    "todo-list" => Ok(json!({"todos":self.items})),
                    "todo-save" => {
                        let mut todo = a["todo"].clone();
                        let uid = todo["uid"].as_str().unwrap_or("created").to_owned();
                        let from = a["from"].as_str().unwrap_or(string(&a["list"]));
                        self.items
                            .retain(|it| !(it["list"] == from && it["uid"] == uid));
                        todo["list"] = a["list"].clone();
                        todo["uid"] = json!(uid);
                        self.items.push(todo);
                        Ok(json!({"uid":uid}))
                    }
                    "todo-delete" => {
                        self.items
                            .retain(|it| !(it["list"] == a["list"] && it["uid"] == a["uid"]));
                        Ok(json!({}))
                    }
                    "cal-create" => {
                        let mut l = a.clone();
                        l["id"] = json!("new-list");
                        l["kind"] = json!("local");
                        self.lists.push(l);
                        Ok(json!({"uid":"new-list"}))
                    }
                    "cal-update" => {
                        if let Some(l) = self.lists.iter_mut().find(|l| l["id"] == a["id"]) {
                            l["name"] = a["name"].clone();
                            l["color"] = a["color"].clone();
                        }
                        Ok(json!({}))
                    }
                    "cal-delete" => {
                        self.lists.retain(|l| l["id"] != a["id"]);
                        Ok(json!({}))
                    }
                    _ => panic!("unexpected op {op}"),
                }
            }
            _ => panic!("unexpected method {method}"),
        }
    }
    fn drain(&mut self, host: &mut Host) {
        for _ in 0..128 {
            let Some(effects) = host.take() else {
                return;
            };
            for e in effects.as_array().unwrap() {
                if e["host"] == true {
                    continue;
                }
                match string(&e["method"]) {
                    "start" => {
                        self.token += 1;
                        let method = string(&e["args"][1]);
                        let args = &e["args"][2];
                        self.calls.push((method.into(), args.clone()));
                        let result = match self.answer(method, args) {
                            Ok(v) => json!({"token":self.token,"value":v}),
                            Err(e) => json!({"token":self.token,"error":e}),
                        };
                        self.ready.insert(self.token, result);
                        host.result(json!(self.token));
                    }
                    "take" => {
                        let packets = e["args"][1]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|t| self.ready.remove(&t.as_u64().unwrap()))
                            .collect::<Vec<_>>();
                        for t in e["args"][2].as_array().unwrap() {
                            self.ready.remove(&t.as_u64().unwrap());
                        }
                        host.result(json!({"value":packets}));
                    }
                    _ => panic!("unexpected effect {e}"),
                }
            }
        }
        panic!("service did not yield");
    }
    fn settle(&mut self, host: &mut Host, now: &Cell<i64>) {
        for _ in 0..30 {
            self.drain(host);
            now.set(now.get() + 100);
            if let Some(d) = host.deadline() {
                host.tick(d + 1.);
            }
        }
        self.drain(host);
    }
    fn operation(&self, op: &str) -> Vec<&Value> {
        self.calls
            .iter()
            .filter(|(m, a)| m == "pim.request" && a[0] == op)
            .map(|(_, a)| &a[1])
            .collect()
    }
}
fn mount(now: Rc<Cell<i64>>, state: Value) -> (Host, i32) {
    let root = scene::create(scene::VIEW);
    let mut host = Host::new(
        root,
        Box::new(Timed {
            app: App::default(),
            now,
        }),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    host.facts(json!({"args":{"__nativeOwner":7,"__nativeWindow":"reminders","__nativeState":state},"lang":"en","width":820,"settings":{"region":"en_US","clock":{"hour24":"24"}},"context":{"theme":{"mode":"light","text":"#111111","textDim":"#666666","accent":"#007aff","separator":"#cccccc","control":"#dddddd","windowBg":"#ffffff"}}}));
    (host, root)
}
fn click(host: &mut Host, label: &str) {
    let node = host.find_text(label);
    assert_ne!(node, 0, "missing {label}: {}", host.text());
    host.pointer("down", node, 0., 0., 0);
    host.pointer("up", node, 0., 0., 0);
}
fn click_last(host: &mut Host, root: i32, label: &str) {
    fn find(node: i32, label: &str, out: &mut i32) {
        if scene::gather_text(node) == label {
            *out = node;
        }
        unsafe {
            let n = crate::runtime::node_get(node);
            if n.is_null() {
                return;
            }
            let mut child = (*n).first;
            while !child.is_null() {
                find((*child).id, label, out);
                child = (*child).next;
            }
        }
    }
    let mut node = 0;
    find(root, label, &mut node);
    assert_ne!(node, 0, "missing {label}");
    host.pointer("down", node, 0., 0., 0);
    host.pointer("up", node, 0., 0., 0);
}
fn action(host: &mut Host, name: &str, key: Value) {
    host.action(&json!({"action":[name,key]}));
}
fn write(host: &mut Host, value: &str) {
    host.key("a", "", 2);
    host.key("x", value, 0);
}
#[test]
fn real_native_nodes_edit_toggle_move_pick_dates_and_keep_window_state() {
    let now = Rc::new(Cell::new(date("2026-11-17")));
    let (mut host, root) = mount(now.clone(), json!({}));
    let mut service = Services::new();
    service.settle(&mut host, &now);
    assert!(host.text().contains("Parent"));
    assert!(!host.text().contains("Later"));
    click(&mut host, "All");
    service.settle(&mut host, &now);
    assert!(host.text().contains("Later"));
    assert_eq!(service.state["sel"], "all");
    click(&mut host, "Parent");
    write(&mut host, "Renamed");
    host.key("Enter", "", 0);
    service.settle(&mut host, &now);
    assert_eq!(
        service.operation("todo-save").last().unwrap()["todo"]["title"],
        "Renamed"
    );
    // The inspector's fields, segmented mode, flag switch, dropdowns and popup
    // calendar all create actual native widgets, not React nodes.
    action(&mut host, "inspect", json!("a/parent"));
    assert!(host.text().contains("Priority"));
    click(&mut host, "Date & time");
    action(&mut host, "notesFocus", Value::Null);
    assert!(host.owns_text_input());
    host.paste("first\nsecond");
    host.key("Escape", "", 0);
    action(&mut host, "picker", json!("day"));
    assert!(host.text().contains("November"));
    action(&mut host, "pickDay", json!(date("2026-11-20")));
    action(&mut host, "picker", json!("min"));
    action(&mut host, "pickValue", json!(547));
    action(&mut host, "picker", json!("prio"));
    click(&mut host, "High");
    // Click the native switch itself (its neighboring label is plain text).
    action(&mut host, "toggleFlag", Value::Null);
    action(&mut host, "picker", json!("list"));
    click_last(&mut host, root, "Work");
    click(&mut host, "Done");
    service.settle(&mut host, &now);
    let saved = (*service.operation("todo-save").last().unwrap()).clone();
    assert_eq!(saved["from"], "a");
    assert_eq!(saved["list"], "b");
    assert_eq!(saved["todo"]["notes"], "first\nsecond");
    assert_eq!(saved["todo"]["prio"], 1);
    assert_eq!(saved["todo"]["flag"], true);
    assert_eq!(saved["todo"]["alarms"], json!([0]));
    assert_eq!(saved["todo"]["due"], at_minutes(date("2026-11-20"), 547));
    assert!(saved["todo"].get("dueDate").is_none());
    action(&mut host, "toggle", json!("b/parent"));
    service.settle(&mut host, &now);
    assert_eq!(
        service.operation("todo-save").last().unwrap()["todo"]["done"],
        true
    );
    click(&mut host, "Completed");
    service.settle(&mut host, &now);
    assert!(host.text().contains("Renamed"));
    drop(host);
    scene::destroy(root);
    let (mut restored, root) = mount(now.clone(), service.state.clone());
    service.settle(&mut restored, &now);
    assert!(restored.text().contains("Renamed"));
    assert_eq!(service.state["sel"], "completed");
    drop(restored);
    scene::destroy(root);
}
#[test]
fn new_reminder_default_list_failure_list_crud_and_remote_delete_guard() {
    let now = Rc::new(Cell::new(date("2026-11-17")));
    let (mut host, root) = mount(now.clone(), json!({}));
    let mut service = Services::new();
    service.lists.clear();
    service.items.clear();
    service.settle(&mut host, &now);
    click(&mut host, "New Reminder");
    write(&mut host, "  first task  ");
    host.key("Enter", "", 0);
    service.settle(&mut host, &now);
    assert_eq!(
        service.operation("cal-create")[0],
        &json!({"name":"Reminders","color":"#007aff","todos":true,"events":false})
    );
    assert_eq!(
        service.operation("todo-save")[0]["todo"]["title"],
        "first task"
    );
    assert_eq!(service.operation("todo-save")[0]["todo"]["dueAllDay"], true);
    service.fail_save = true;
    write(&mut host, "failed task");
    host.key("Enter", "", 0);
    service.settle(&mut host, &now);
    assert!(host.text().contains("Could not save"));
    service.fail_save = false;
    click(&mut host, "Add List");
    write(&mut host, "Shopping");
    click(&mut host, "Create");
    service.settle(&mut host, &now);
    assert_eq!(service.state["sel"], "list:new-list");
    action(&mut host, "editList", json!("new-list"));
    write(&mut host, "Shopping edited");
    click(&mut host, "Save");
    service.settle(&mut host, &now);
    assert_eq!(
        service.operation("cal-update").last().unwrap()["name"],
        "Shopping edited"
    );
    let before = service.operation("cal-delete").len();
    action(&mut host, "deleteList", json!("b"));
    service.settle(&mut host, &now);
    assert_eq!(service.operation("cal-delete").len(), before);
    action(&mut host, "deleteList", json!("new-list"));
    service.settle(&mut host, &now);
    assert_eq!(service.operation("cal-delete").len(), before + 1);
    assert_eq!(service.state["sel"], "today");
    drop(host);
    scene::destroy(root);
}

#[test]
fn clicking_outside_commits_escape_cancels_and_service_events_refresh_the_scene() {
    let now = Rc::new(Cell::new(date("2026-11-17")));
    let (mut host, root) = mount(now.clone(), json!({}));
    let mut service = Services::new();
    service.settle(&mut host, &now);
    click(&mut host, "Parent");
    write(&mut host, "Outside commit");
    host.pointer("down", root, 0., 0., 0);
    service.settle(&mut host, &now);
    assert_eq!(
        service.operation("todo-save").last().unwrap()["todo"]["title"],
        "Outside commit"
    );
    let writes = service.operation("todo-save").len();
    click(&mut host, "Outside commit");
    write(&mut host, "Cancelled");
    host.key("Escape", "", 0);
    service.settle(&mut host, &now);
    assert_eq!(service.operation("todo-save").len(), writes);
    assert!(host.text().contains("Outside commit"));
    assert!(!host.text().contains("Cancelled"));
    service.fail_save = true;
    action(&mut host, "toggle", json!("a/parent"));
    service.settle(&mut host, &now);
    assert_ne!(
        service
            .items
            .iter()
            .find(|it| it["uid"] == "parent")
            .unwrap()["done"],
        true
    );
    service.fail_save = false;
    service.items.push(
        json!({"uid":"external","list":"a","title":"Changed elsewhere","due":date("2026-11-17")}),
    );
    service.events.push(json!({"ev":"pim-changed"}));
    service.settle(&mut host, &now);
    assert!(host.text().contains("Changed elsewhere"));
    drop(host);
    scene::destroy(root);
}

fn lists() -> Vec<Value> {
    vec![
        json!({"id":"a","name":"Personal","color":"#007aff","kind":"local","todos":true}),
        json!({"id":"b","name":"Work","color":"#ff9500","kind":"dav","todos":true}),
    ]
}

fn date(s: &str) -> i64 {
    parse_day(s).unwrap()
}

fn sample() -> Vec<Value> {
    vec![
        json!({"uid":"later","list":"a","title":"Later","due":date("2026-11-19"),"done":false,"flag":true}),
        json!({"uid":"child","list":"a","title":"Child","parent":"parent","due":date("2026-11-16"),"done":false}),
        json!({"uid":"parent","list":"a","title":"Parent","due":date("2026-11-17"),"dueAllDay":true,"done":false}),
        json!({"uid":"old","list":"b","title":"Older","due":date("2026-11-15"),"done":false}),
        json!({"uid":"undated","list":"b","title":"Undated","due":null,"done":false,"prio":9}),
        json!({"uid":"done-old","list":"a","title":"Done old","done":true,"doneAt":10}),
        json!({"uid":"done-new","list":"b","title":"Done new","done":true,"doneAt":20}),
    ]
}

use std::collections::BTreeMap;
