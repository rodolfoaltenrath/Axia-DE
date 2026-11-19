use super::*;
use std::{cell::RefCell, rc::Rc};

#[test]
fn native_wheel_touchpad_and_thumb_report_the_clamped_viewport() {
    struct App(Rc<RefCell<Vec<Value>>>);
    impl NativeApp for App {
        fn request(&mut self, input: &Value) -> Value {
            match input["cmd"].as_str() {
                Some("sync") => {
                    json!({"tree":{"kind":"scroll","key":"list","onScroll":["viewport","list"],
                    "children":[{"kind":"view","style":{"height":10000},"children":[{"kind":"text","label":"Row"}]}]}})
                }
                Some("viewport") => {
                    self.0.borrow_mut().push(input.clone());
                    json!({})
                }
                _ => json!({}),
            }
        }
    }
    let mount = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = Host::new(mount, Box::new(App(log.clone())), "", None);
    host.facts(json!({}));
    let scroll = host.root[0].node;
    let content = host.root[0].parts[0];
    let thumb = host.root[0].parts[1];
    unsafe {
        let node = crate::runtime::node_get(scroll);
        (*node).h = 400.;
        let node = crate::runtime::node_get(content);
        (*node).h = 10000.;
        let node = crate::runtime::node_get(thumb);
        (*node).h = 28.;
    }
    let row = host.find_text("Row");
    host.pointer("wheel", row, 10., 20., -2);
    assert_eq!(log.borrow().last().unwrap()["offset"], 96.);
    host.gesture("scroll", "update", row, 0., 901.);
    assert_eq!(log.borrow().last().unwrap()["offset"], 997.);
    host.pointer("wheel", row, 10., 20., -100000);
    assert_eq!(log.borrow().last().unwrap()["offset"], 9600.);
    let count = log.borrow().len();
    host.pointer("wheel", row, 10., 20., -100000);
    assert_eq!(
        log.borrow().len(),
        count,
        "clamped input does not invalidate the viewport again"
    );
    host.gesture("scroll", "update", row, 0., -1e8);
    assert_eq!(log.borrow().last().unwrap()["offset"], 0.);
    host.pointer("down", thumb, 2., 2., 0);
    host.pointer("move", thumb, 2., 200., 0);
    host.pointer("up", thumb, 2., 200., 0);
    let events = log.borrow();
    assert!(events.last().unwrap()["offset"].as_f64().unwrap() > 1000.);
    for event in events.iter() {
        assert_eq!(event["key"], "list");
        assert_eq!(event["height"], 400.);
    }
    drop(events);
    drop(host);
    scene::destroy(mount);
}
