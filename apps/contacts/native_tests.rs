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
    fn paste(&mut self, t: &str) -> Option<Value> {
        self.app.paste_at(t, self.now.get())
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
    books: Vec<Value>,
    contacts: Vec<Value>,
    events: Vec<Value>,
    state: Value,
    fail: bool,
    pick: Value,
    bindings: Vec<Value>,
}
impl Services {
    fn new() -> Self {
        Self {
            token: 0,
            ready: BTreeMap::new(),
            calls: vec![],
            books: vec![json!({"id":"a","name":"Personal","kind":"local"})],
            contacts: vec![contact()],
            events: vec![],
            state: Value::Null,
            fail: false,
            pick: Value::Null,
            bindings: vec![],
        }
    }
    fn answer(&mut self, m: &str, a: &Value) -> Result<Value, String> {
        Ok(match m {
            "pim.watch" | "pim.fileRelease" | "pim.imageRelease" | "windows.open" => Value::Null,
            "pim.events" => json!(std::mem::take(&mut self.events)),
            "windows.state" => {
                self.state = a[1].clone();
                Value::Null
            }
            "clipboard.read" => json!({"text":"pasted notes"}),
            "pim.filePick" => std::mem::take(&mut self.pick),
            "pim.image" => json!({"image":100+self.token}),
            "pim.request" => {
                let op = string(&a[0]);
                let args = &a[1];
                if self.fail
                    && [
                        "contact-save",
                        "book-create",
                        "contact-import",
                        "contact-delete",
                    ]
                    .contains(&op)
                {
                    return Err("offline".into());
                }
                match op {
                    "book-list" => json!({"books":self.books}),
                    "contact-list" => json!({"contacts":self.contacts}),
                    "book-create" => {
                        self.books
                            .push(json!({"id":"created-book","name":args["name"],"kind":"local"}));
                        json!({"uid":"created-book"})
                    }
                    "contact-save" => {
                        let mut c = args["contact"].clone();
                        let uid = c["uid"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("created-{}", self.contacts.len()));
                        c["uid"] = json!(uid);
                        c["book"] = args["book"].clone();
                        self.contacts.retain(|old| key(old) != key(&c));
                        self.contacts.push(c);
                        json!({"uid":uid})
                    }
                    "contact-delete" => {
                        self.contacts
                            .retain(|c| !(c["book"] == args["book"] && c["uid"] == args["uid"]));
                        json!({})
                    }
                    "contact-import" => json!({"added":3}),
                    _ => panic!("unexpected PIM operation {op}"),
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
                    if e["kind"] == "mailImage" {
                        self.bindings.push(e.clone());
                    }
                    continue;
                }
                match string(&e["method"]) {
                    "start" => {
                        assert_eq!(e["args"][0], 7);
                        self.token += 1;
                        let m = string(&e["args"][1]);
                        let a = &e["args"][2];
                        self.calls.push((m.into(), a.clone()));
                        let reply = match self.answer(m, a) {
                            Ok(v) => json!({"token":self.token,"value":v}),
                            Err(e) => json!({"token":self.token,"error":e}),
                        };
                        self.ready.insert(self.token, reply);
                        h.result(json!(self.token));
                    }
                    "take" => {
                        let packets: Vec<_> = e["args"][1]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|t| self.ready.remove(&t.as_u64().unwrap()))
                            .collect();
                        for t in e["args"][2].as_array().unwrap() {
                            self.ready.remove(&t.as_u64().unwrap());
                        }
                        h.result(json!({"value":packets}));
                    }
                    _ => panic!("unexpected effect {e}"),
                }
            }
        }
        panic!("native service loop did not yield");
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
    fn operations(&self, op: &str) -> Vec<&Value> {
        self.calls
            .iter()
            .filter(|(m, a)| m == "pim.request" && a[0] == op)
            .map(|(_, a)| &a[1])
            .collect()
    }
}
fn mount(now: Rc<Cell<i64>>, state: Value) -> (Host, i32) {
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
    h.facts(json!({"keyboard":true,"args":{"__nativeOwner":7,"__nativeWindow":"contacts#2","__nativeState":state},"lang":"en","width":880,"context":{"region":"en_US","theme":{"mode":"light","text":"#111111","textDim":"#666666","accent":"#007aff","separator":"#cccccc","control":"#dddddd","windowBg":"#ffffff"}}}));
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
fn click_last(h: &mut Host, root: i32, s: &str) {
    fn last(node: i32, s: &str, out: &mut i32) {
        if scene::gather_text(node) == s {
            *out = node;
        }
        unsafe {
            let n = crate::runtime::node_get(node);
            if n.is_null() {
                return;
            }
            let mut child = (*n).first;
            while !child.is_null() {
                last((*child).id, s, out);
                child = (*child).next;
            }
        }
    }
    let mut n = 0;
    last(root, s, &mut n);
    assert_ne!(n, 0, "missing {s}");
    h.pointer("down", n, 0., 0., 0);
    h.pointer("up", n, 0., 0., 0);
}
#[test]
fn native_window_edits_real_fields_saves_then_opens_mail() {
    let now = Rc::new(Cell::new(0));
    let (mut h, root) = mount(now.clone(), json!({}));
    let mut s = Services::new();
    s.settle(&mut h, &now);
    assert!(h.text().contains("Ada Lovelace"));
    assert!(h.text().contains("February 29"));
    click(&mut h, "Edit");
    click_last(&mut h, root, "Ada");
    h.key("a", "", 2);
    h.key("x", "Augusta", 0);
    click(&mut h, "Done");
    s.settle(&mut h, &now);
    assert_eq!(s.contacts[0]["n"]["given"], "Augusta");
    assert!(h.text().contains("Augusta Lovelace"));
    click(&mut h, "ada@example.org");
    s.settle(&mut h, &now);
    let open = s.calls.iter().find(|(m, _)| m == "windows.open").unwrap();
    assert_eq!(open.1[0], "mail");
    assert_eq!(
        open.1[1]["args"]["compose"]["to"][0]["email"],
        "ada@example.org"
    );
    drop(h);
    scene::destroy(root);
}
#[test]
fn groups_default_book_and_contact_join_follow_native_actions() {
    let now = Rc::new(Cell::new(0));
    let (mut h, root) = mount(now.clone(), json!({}));
    let mut s = Services::new();
    s.books.clear();
    s.contacts.clear();
    s.settle(&mut h, &now);
    click(&mut h, "New group");
    h.key("x", "Team", 0);
    action(&mut h, "groupSave", Value::Null);
    s.settle(&mut h, &now);
    assert_eq!(s.operations("book-create").len(), 1);
    assert_eq!(s.operations("book-create")[0]["name"], "Contacts");
    assert!(s.state["src"]
        .as_str()
        .unwrap()
        .starts_with("group:created-book/"));
    click(&mut h, "New contact");
    h.key("x", "Grace", 0);
    click(&mut h, "Done");
    s.settle(&mut h, &now);
    let group = s.contacts.iter().find(|c| c["kind"] == "group").unwrap();
    let person = s.contacts.iter().find(|c| c["kind"] != "group").unwrap();
    assert_eq!(person["book"], "created-book");
    assert_eq!(group["members"], json!([person["uid"]]));
    assert_eq!(s.state["cur"], key(person));
    action(&mut h, "menu", json!("membership"));
    click_last(&mut h, root, "Team");
    s.settle(&mut h, &now);
    let group = s.contacts.iter().find(|c| c["kind"] == "group").unwrap();
    assert_eq!(group["members"], json!([]));
    // A group can also be created while a new contact's editor is open. The
    // save must use that newly selected group, not the source at editor open.
    click(&mut h, "New contact");
    h.key("x", "Ada", 0);
    click(&mut h, "New group");
    h.key("x", "Second team", 0);
    action(&mut h, "groupSave", Value::Null);
    s.settle(&mut h, &now);
    click(&mut h, "Done");
    s.settle(&mut h, &now);
    let group = s
        .contacts
        .iter()
        .find(|c| c["fn"] == "Second team")
        .unwrap();
    let person = s.contacts.iter().find(|c| c["fn"] == "Ada").unwrap();
    assert_eq!(group["members"], json!([person["uid"]]));
    drop(h);
    scene::destroy(root);
}
#[test]
fn photos_import_and_failure_keep_resources_and_editor_scoped() {
    let now = Rc::new(Cell::new(0));
    let (mut h, root) = mount(now.clone(), json!({}));
    let mut s = Services::new();
    s.contacts[0]["photo"] = json!(true);
    s.settle(&mut h, &now);
    let b = s
        .bindings
        .iter()
        .find(|b| b["image"].as_i64().is_some_and(|id| id > 0))
        .unwrap();
    let node = b["node"].as_i64().unwrap() as i32;
    unsafe {
        assert_eq!((*crate::runtime::node_get(node)).kind, scene::IMAGE);
    }
    let count = s.calls.iter().filter(|(m, _)| m == "pim.image").count();
    s.settle(&mut h, &now);
    assert_eq!(
        s.calls.iter().filter(|(m, _)| m == "pim.image").count(),
        count,
        "unchanged photo must not reload each tick"
    );
    click(&mut h, "Edit");
    s.pick = json!({"file":41,"name":"photo.png"});
    action(&mut h, "photoPick", Value::Null);
    s.settle(&mut h, &now);
    assert!(h.text().contains("new photo"));
    s.fail = true;
    click(&mut h, "Done");
    s.settle(&mut h, &now);
    assert!(h.text().contains("Could not save"));
    assert!(!s
        .calls
        .iter()
        .any(|(m, a)| m == "pim.fileRelease" && a[0] == 41));
    s.fail = false;
    click(&mut h, "Cancel");
    s.settle(&mut h, &now);
    assert!(s
        .calls
        .iter()
        .any(|(m, a)| m == "pim.fileRelease" && a[0] == 41));
    s.pick = json!({"file":42,"name":"people.vcf"});
    click(&mut h, "Import…");
    s.settle(&mut h, &now);
    assert!(h.text().contains("3 contacts imported."));
    assert!(s
        .calls
        .iter()
        .any(|(m, a)| m == "pim.fileRelease" && a[0] == 42));
    click(&mut h, "OK");
    s.pick = json!({"file":43,"name":"invalid.vcf"});
    s.fail = true;
    click(&mut h, "Import…");
    s.settle(&mut h, &now);
    assert!(h.text().contains("The file could not be imported."));
    assert!(s
        .calls
        .iter()
        .any(|(m, a)| m == "pim.fileRelease" && a[0] == 43));
    drop(h);
    scene::destroy(root);
}
