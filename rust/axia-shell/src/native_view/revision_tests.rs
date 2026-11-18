use super::{host::Host, retained::RetainedTree, NativeApp};
use crate::runtime::{self, native as scene};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};

struct Rows(Rc<RefCell<Value>>);
impl NativeApp for Rows {
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] == "sync" {
            json!({"tree": *self.0.borrow()})
        } else {
            json!({})
        }
    }
}
fn rows(keys: &[usize]) -> Value {
    json!({"kind":"view","children":keys.iter().map(|key| json!({"kind":"text","key":key,"label":format!("row {key}")})).collect::<Vec<_>>()})
}
#[test]
fn ordered_reversed_inserted_and_removed_siblings_keep_their_scene_nodes() {
    let keys: Vec<_> = (0..2000).collect();
    let model = Rc::new(RefCell::new(rows(&keys)));
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(mount, Box::new(Rows(model.clone())), "", None);
    host.facts(json!({"args":{}}));
    let ids: Vec<_> = keys
        .iter()
        .map(|i| host.find_text(&format!("row {i}")))
        .collect();
    host.result(Value::Null);
    for &i in &keys {
        assert_eq!(host.find_text(&format!("row {i}")), ids[i]);
    }
    let reversed: Vec<_> = keys.iter().copied().rev().collect();
    *model.borrow_mut() = rows(&reversed);
    host.result(Value::Null);
    for &i in &keys {
        assert_eq!(host.find_text(&format!("row {i}")), ids[i]);
    }
    let mut changed: Vec<_> = reversed.into_iter().filter(|i| i % 3 != 0).collect();
    changed.insert(5, 2001);
    *model.borrow_mut() = rows(&changed);
    host.result(Value::Null);
    for &i in &changed {
        if i < ids.len() {
            assert_eq!(host.find_text(&format!("row {i}")), ids[i]);
        }
    }
    assert_ne!(host.find_text("row 2001"), 0);
    assert_eq!(host.find_text("row 0"), 0);
    drop(host);
    scene::destroy(mount);
}
#[test]
fn duplicate_keys_match_the_first_remaining_node_of_the_same_kind() {
    let model = Rc::new(RefCell::new(json!({"kind":"view","children":[
        {"kind":"text","key":"dup","label":"first"},
        {"kind":"view","key":"dup","children":[{"kind":"text","label":"inside"}]},
        {"kind":"text","key":"dup","label":"second"}]})));
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(mount, Box::new(Rows(model.clone())), "", None);
    host.facts(json!({"args":{}}));
    let first = host.find_text("first");
    let second = host.find_text("second");
    let inside = host.find_text("inside");
    *model.borrow_mut() = json!({"kind":"view","children":[
        {"kind":"view","key":"dup","children":[{"kind":"text","label":"inside"}]},
        {"kind":"text","key":"dup","label":"A"},
        {"kind":"text","key":"dup","label":"B"}]});
    host.result(Value::Null);
    assert_eq!(host.find_text("A"), first);
    assert_eq!(host.find_text("B"), second);
    assert_eq!(host.find_text("inside"), inside);
    drop(host);
    scene::destroy(mount);
}
struct Region {
    cache: RetainedTree,
    state: Rc<RefCell<(bool, u64, usize)>>,
}
impl NativeApp for Region {
    fn retained_tree(&self, key: &str) -> Option<&Value> {
        self.cache.tree(key)
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] != "sync" {
            return json!({});
        }
        let mut state = self.state.borrow_mut();
        if !state.0 {
            return json!({"tree":{"kind":"view"}});
        }
        let marker = self.cache.render("region", json!([state.1, input["context"],input["lang"]]), || {
            state.2 += 1;
            json!({"kind":"view","children":[{"kind":"text","label":"retained","style":{"opacity":if state.1 == 0 {1.} else {0.8}}}]})
        });
        json!({"tree":{"kind":"view","children":[marker]}})
    }
}
#[test]
fn retained_regions_skip_work_and_recover_after_removal_focus_resize_and_context_changes() {
    let mount = scene::create(scene::VIEW);
    unsafe {
        let n = runtime::node_get(mount);
        (*n).w = 640.;
        (*n).h = 480.;
    }
    let state = Rc::new(RefCell::new((true, 0, 0)));
    let mut host = Host::new(
        mount,
        Box::new(Region {
            cache: RetainedTree::default(),
            state: state.clone(),
        }),
        "",
        None,
    );
    host.facts(json!({"lang":"en","context":{"theme":{"text":"#fff"}}}));
    let id = host.find_text("retained");
    assert_ne!(id, 0);
    unsafe {
        (*runtime::node_get(id)).s.opacity = 0.37;
    }
    host.result(Value::Null);
    assert_eq!(state.borrow().2, 1);
    unsafe {
        assert_eq!((*runtime::node_get(id)).s.opacity, 0.37);
    }
    state.borrow_mut().1 += 1;
    host.result(Value::Null);
    assert_eq!(host.find_text("retained"), id);
    unsafe {
        assert_eq!((*runtime::node_get(id)).s.opacity, 0.8);
    }
    state.borrow_mut().0 = false;
    host.result(Value::Null);
    assert_eq!(host.find_text("retained"), 0);
    let builds = state.borrow().2;
    state.borrow_mut().0 = true;
    host.result(Value::Null);
    assert_ne!(host.find_text("retained"), 0);
    assert_eq!(
        state.borrow().2,
        builds,
        "remount uses the retained full specification"
    );
    host.window_focus(false);
    host.window_focus(true);
    assert_eq!(
        state.borrow().2,
        builds + 2,
        "both focus transitions reach the app"
    );
    unsafe {
        (*runtime::node_get(mount)).h = 600.;
    }
    host.tick(1000.);
    assert_eq!(state.borrow().2, builds + 3);
    host.facts(json!({"lang":"pt","context":{"theme":{"text":"#111"}}}));
    assert_eq!(state.borrow().2, builds + 4);
    drop(host);
    scene::destroy(mount);
}

#[test]
fn retained_controls_keep_scroll_focus_and_event_routes_when_other_nodes_change() {
    struct Controls {
        cache: RetainedTree,
        log: Rc<RefCell<Vec<Value>>>,
        value: String,
        changed: bool,
    }
    impl NativeApp for Controls {
        fn retained_tree(&self, key: &str) -> Option<&Value> {
            self.cache.tree(key)
        }
        fn request(&mut self, input: &Value) -> Value {
            self.log.borrow_mut().push(input.clone());
            if input["cmd"] == "query" {
                self.value = input["key"].as_str().unwrap().into();
            }
            if input["cmd"] == "next" {
                self.changed = !self.changed;
            }
            if input["cmd"] != "sync" {
                return json!({});
            }
            let marker =
                self.cache
                    .render("controls", json!([self.value, input["context"]]), || {
                        json!({"kind":"view","children":[
                            {"kind":"input","value":self.value,"placeholder":"type"},
                            {"kind":"button","label":"Run","action":["run"]},
                            {"kind":"scroll","children":[{"kind":"text","label":"scroll content"}]}
                        ]})
                    });
            json!({"tree":{"kind":"view","children":[{"kind":"text","label":if self.changed{"changed"}else{"idle"}},marker]}})
        }
    }
    let mount = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(vec![]));
    let mut host = Host::new(
        mount,
        Box::new(Controls {
            cache: RetainedTree::default(),
            log: log.clone(),
            value: String::new(),
            changed: false,
        }),
        "",
        None,
    );
    host.facts(json!({"args":{}}));
    let field = host.find_text("type");
    let button = host.find_text("Run");
    let text = host.find_text("scroll content");
    let content = scene::parent(text);
    let scroll = scene::parent(content);
    unsafe {
        (*runtime::node_get(content)).h = 900.;
        (*runtime::node_get(scroll)).h = 200.;
    }
    host.pointer("wheel", text, 0., 0., -1);
    let offset = unsafe { (*runtime::node_get(content)).s.translate_y };
    assert!(offset < 0.);
    host.pointer("down", field, 0., 0., 0);
    assert!(host.owns_text_input());
    host.result(Value::Null); // Another region changes; this region is retained.
    assert_eq!(host.find_text("type"), field);
    assert_eq!(host.find_text("Run"), button);
    assert!(host.owns_text_input());
    unsafe {
        assert_eq!((*runtime::node_get(content)).s.translate_y, offset);
    }
    assert!(host.key("a", "a", 0));
    assert!(log
        .borrow()
        .iter()
        .any(|v| v["cmd"] == "query" && v["key"] == "a"));
    host.pointer("down", button, 0., 0., 0);
    host.pointer("up", button, 0., 0., 0);
    assert!(log.borrow().iter().any(|v| v["cmd"] == "run"));
    unsafe {
        assert_eq!((*runtime::node_get(content)).s.translate_y, offset);
    }
    drop(host);
    scene::destroy(mount);
}
