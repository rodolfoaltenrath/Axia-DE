use super::*;
use crate::{native_view::Host, runtime::native as scene};
use std::{cell::Cell, rc::Rc};
struct Timed {
    app: App,
    now: Rc<Cell<i64>>,
}
impl NativeApp for Timed {
    fn refresh_ms(&self) -> Option<f64> {
        self.app.refresh_ms()
    }
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.app.resources(assets, home);
    }
    fn request(&mut self, input: &Value) -> Value {
        self.app.request_at(input, self.now.get())
    }
}
fn find(node: i32, label: &str) -> i32 {
    if scene::gather_text(node) == label {
        return node;
    }
    unsafe {
        let n = crate::runtime::node_get(node);
        if n.is_null() {
            return 0;
        }
        let mut child = (*n).first;
        while !child.is_null() {
            let found = find((*child).id, label);
            if found != 0 {
                return found;
            }
            child = (*child).next;
        }
    }
    0
}
fn click(host: &mut Host, mount: i32, label: &str) {
    let n = find(mount, label);
    assert_ne!(n, 0, "missing {label}");
    host.pointer("down", n, 0., 0., 0);
    host.pointer("up", n, 0., 0., 0);
}
#[test]
fn real_native_scene_reads_async_results_selects_entries_and_edits_filters() {
    assert!(crate::native_view::app("logs").is_some());
    let now = Rc::new(Cell::new(1000));
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(
        mount,
        Box::new(Timed {
            app: App::default(),
            now: now.clone(),
        }),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        None,
    );
    host.facts(facts());
    let mut jobs = BTreeMap::new();
    let mut token = 0_u64;
    while let Some(effects) = host.take() {
        for effect in effects.as_array().unwrap() {
            assert_eq!(effect["method"], "start");
            token += 1;
            jobs.insert(token, effect["args"][1].as_str().unwrap().to_owned());
            host.result(json!(token));
        }
    }
    assert_eq!(jobs.len(), 4);
    assert!(host.text().contains("Loading"));
    now.set(1100);
    host.tick(host.deadline().unwrap() + 1.);
    let effects = host.take().unwrap();
    assert_eq!(effects[0]["method"], "take");
    let packets:Vec<_>=jobs.iter().map(|(token,method)|json!({"token":token,"value":match method.as_str(){
        "logs.sources"=>json!({"system":false}),"logs.files"=>json!([{"name":"session.log","mtime":1}]),
        "logs.query"=>json!([{"t":1700000000000000_i64,"id":"test","p":3,"pid":42,"m":"detail\nline"}]),_=>Value::Null
    }})).collect();
    host.result(json!({"method":"take","value":packets}));
    assert!(host.text().contains("detail ⏎ line"));
    assert!(host.text().contains("session"));
    click(&mut host, mount, "detail ⏎ line");
    assert!(host.text().contains("detail\nline"));
    assert!(host.text().contains("42"));
    host.action(&json!({"action":["search"],"extra":{"value":"needle"}}));
    assert!(host.text().contains("needle"));
    now.set(1500);
    host.tick(host.deadline().unwrap() + 1.);
    let effects = host.take().unwrap();
    assert_eq!(effects[0]["args"][1], "logs.query");
    assert_eq!(effects[0]["args"][2][0]["grep"], "needle");
    drop(host);
    scene::destroy(mount);
}
