use super::*;
use crate::{native_view::Host, runtime::native as scene};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};
struct Timed {
    app: App,
    now: Rc<Cell<i64>>,
}
impl NativeApp for Timed {
    fn resources(&mut self, a: &str, h: Option<&str>) {
        self.app.resources(a, h);
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
    fn text_input(&self) -> bool {
        self.app.text_input()
    }
    fn blur(&mut self) {
        self.app.blur();
    }
}
struct Services {
    token: u64,
    packets: BTreeMap<u64, Value>,
    calls: Vec<(String, Value)>,
    cals: Vec<Value>,
    events: Vec<Value>,
    fail: bool,
    state: Value,
}
impl Services {
    fn new() -> Self {
        Self {
            token: 0,
            packets: BTreeMap::new(),
            calls: vec![],
            cals: vec![json!({"id":"a","name":"Personal","events":true,"kind":"local"})],
            events: vec![],
            fail: false,
            state: Value::Null,
        }
    }
    fn answer(&mut self, m: &str, a: &Value) -> Result<Value, String> {
        Ok(match m {
            "pim.watch" => Value::Null,
            "pim.events" => json!([]),
            "windows.state" => {
                self.state = a[1].clone();
                Value::Null
            }
            "clipboard.read" => json!({"text":"pasted"}),
            "pim.request" => {
                let args = &a[1];
                let op = s(&a[0]);
                if self.fail && ["ev-save", "cal-create", "cal-update", "ev-delete"].contains(&op) {
                    return Err("offline".into());
                }
                match op {
                    "cal-list" => json!({"cals":self.cals}),
                    "ev-list" => json!({"events":self.events}),
                    "cal-create" => {
                        let mut c = args.clone();
                        c["id"] = json!("created");
                        c["kind"] = json!("local");
                        self.cals.push(c);
                        json!({"uid":"created"})
                    }
                    "cal-update" => {
                        if let Some(c) = self.cals.iter_mut().find(|c| c["id"] == args["id"]) {
                            for k in ["hidden", "name", "color"] {
                                if let Some(v) = args.get(k) {
                                    c[k] = v.clone();
                                }
                            }
                        }
                        json!({})
                    }
                    "cal-delete" => {
                        self.cals.retain(|c| c["id"] != args["id"]);
                        json!({})
                    }
                    "ev-save" => {
                        let mut e = args["event"].clone();
                        let uid = e["uid"].as_str().unwrap_or("created-event").to_owned();
                        e["uid"] = json!(uid);
                        e["cal"] = args["cal"].clone();
                        self.events.retain(|old| old["uid"] != uid);
                        self.events.push(e);
                        json!({"uid":uid})
                    }
                    "ev-delete" => {
                        self.events.retain(|e| e["uid"] != args["uid"]);
                        json!({})
                    }
                    _ => panic!("{op}"),
                }
            }
            _ => panic!("unexpected service {m}"),
        })
    }
    fn drain(&mut self, h: &mut Host) {
        for _ in 0..128 {
            let Some(effects) = h.take() else {
                return;
            };
            for e in effects.as_array().unwrap() {
                if e["host"] == true {
                    continue;
                }
                match s(&e["method"]) {
                    "start" => {
                        assert_eq!(e["args"][0], 7);
                        self.token += 1;
                        let m = s(&e["args"][1]);
                        let a = &e["args"][2];
                        self.calls.push((m.into(), a.clone()));
                        let packet = match self.answer(m, a) {
                            Ok(v) => json!({"token":self.token,"value":v}),
                            Err(e) => json!({"token":self.token,"error":e}),
                        };
                        self.packets.insert(self.token, packet);
                        h.result(json!(self.token));
                    }
                    "take" => {
                        let replies: Vec<_> = e["args"][1]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|t| self.packets.remove(&t.as_u64().unwrap()))
                            .collect();
                        for t in e["args"][2].as_array().unwrap() {
                            self.packets.remove(&t.as_u64().unwrap());
                        }
                        h.result(json!({"value":replies}));
                    }
                    _ => panic!("{e}"),
                }
            }
        }
        panic!("native jobs failed to yield");
    }
    fn settle(&mut self, h: &mut Host, now: &Cell<i64>) {
        for _ in 0..25 {
            self.drain(h);
            now.set(now.get() + 100);
            if let Some(d) = h.deadline() {
                h.tick(d + 1.);
            }
        }
        self.drain(h);
    }
    fn ops(&self, op: &str) -> Vec<&Value> {
        self.calls
            .iter()
            .filter(|(m, a)| m == "pim.request" && a[0] == op)
            .map(|(_, a)| &a[1])
            .collect()
    }
}
fn mount(now: Rc<Cell<i64>>, extra: Value) -> (Host, i32) {
    let root = scene::create(scene::VIEW);
    let mut h = Host::new(
        root,
        Box::new(Timed {
            app: App::default(),
            now,
        }),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    let mut args = json!({"__nativeOwner":7,"__nativeWindow":"calendar#2","__nativeState":{"cursor":date::parse("2026-11-17").unwrap(),"view":"week"}});
    for (k, v) in extra.as_object().into_iter().flatten() {
        args[k] = v.clone();
    }
    h.facts(json!({"keyboard":true,"args":args,"lang":"en","width":960,"context":{"region":"en_US","theme":{"mode":"light","text":"#111111","textDim":"#666666","accent":"#007aff","separator":"#cccccc","control":"#dddddd","windowBg":"#ffffff"}}}));
    (h, root)
}
fn click(h: &mut Host, s: &str) {
    let n = h.find_text(s);
    assert_ne!(n, 0, "missing {s}: {}", h.text());
    h.pointer("down", n, 0., 0., 0);
    h.pointer("up", n, 0., 0., 0);
}
fn action(h: &mut Host, cmd: &str, key: Value) {
    h.action(&json!({"action":[cmd,key]}));
}
#[test]
fn native_title_field_default_calendar_creation_and_view_switches() {
    let now = Rc::new(Cell::new(date::parse("2026-11-17").unwrap()));
    let (mut h, root) = mount(now.clone(), json!({}));
    let mut svc = Services::new();
    svc.cals.clear();
    svc.settle(&mut h, &now);
    click(&mut h, "+");
    h.key("x", "Native meeting", 0);
    click(&mut h, "Add");
    svc.settle(&mut h, &now);
    assert_eq!(svc.ops("cal-create").len(), 1);
    assert_eq!(svc.ops("ev-save")[0]["cal"], "created");
    assert_eq!(svc.events[0]["title"], "Native meeting");
    assert!(h.text().contains("Native meeting"));
    click(&mut h, "Month");
    svc.settle(&mut h, &now);
    assert_eq!(svc.state["view"], "month");
    click(&mut h, "Day");
    svc.settle(&mut h, &now);
    assert_eq!(svc.state["view"], "day");
    drop(h);
    scene::destroy(root);
}
#[test]
fn native_recurring_delete_popup_sends_occurrence_scope() {
    let now = Rc::new(Cell::new(date::parse("2026-11-17").unwrap()));
    let (mut h, root) = mount(now.clone(), json!({}));
    let mut svc = Services::new();
    let e = event();
    svc.events = vec![e.clone()];
    svc.settle(&mut h, &now);
    click(&mut h, "Meeting");
    assert!(h.text().contains("Changes apply to all events"));
    assert!(h.text().contains("Alice"));
    click(&mut h, "Delete");
    click(&mut h, "Delete this event only");
    svc.settle(&mut h, &now);
    assert_eq!(svc.ops("ev-delete")[0]["occ"], e["occ"]);
    assert!(svc.ops("ev-delete")[0].get("all").is_none());
    drop(h);
    scene::destroy(root);
}
#[test]
fn alarm_opens_event_and_failed_save_keeps_fields_for_retry() {
    let now = Rc::new(Cell::new(date::parse("2026-11-17").unwrap()));
    let e = event();
    let (mut h, root) = mount(
        now.clone(),
        json!({"at":e["start"],"uid":"e","cal":"a","nonce":1}),
    );
    let mut svc = Services::new();
    svc.events = vec![e.clone()];
    svc.settle(&mut h, &now);
    assert!(h.text().contains("Alice"));
    svc.fail = true;
    click(&mut h, "Save");
    svc.settle(&mut h, &now);
    assert!(h.text().contains("Could not save. Try again."));
    assert!(h.text().contains("Alice"));
    svc.fail = false;
    click(&mut h, "Save");
    svc.settle(&mut h, &now);
    let writes = svc.ops("ev-save");
    assert_eq!(writes.last().unwrap()["event"]["att"], e["att"]);
    drop(h);
    scene::destroy(root);
}
