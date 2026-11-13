use super::*;
use crate::{native_view::Host, runtime::native as scene};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};
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
    let node = find(mount, label);
    assert_ne!(node, 0, "missing {label}");
    host.pointer("down", node, 0., 0., 0);
    host.pointer("up", node, 0., 0., 0);
}
struct Services {
    token: u64,
    ready: BTreeMap<u64, Value>,
    calls: Vec<(String, Value)>,
}
impl Services {
    fn new() -> Self {
        Self {
            token: 0,
            ready: BTreeMap::new(),
            calls: vec![],
        }
    }
    fn drain(&mut self, host: &mut Host) {
        for _ in 0..64 {
            let Some(effects) = host.take() else {
                return;
            };
            for effect in effects.as_array().unwrap() {
                if effect["host"] == true {
                    continue;
                }
                if effect["method"] == "start" {
                    self.token += 1;
                    let method = effect["args"][1].as_str().unwrap();
                    let args = effect["args"][2].clone();
                    self.calls.push((method.into(), args.clone()));
                    let value = match method {
                        "scanner.discover" => json!({"scanners":[super::device()]}),
                        "scanner.caps" => super::caps(),
                        "scanner.scan" => json!({"file":"/tmp/preview.jpg","format":"image/jpeg"}),
                        "images.decode" => json!({"image":100+self.token,"w":520,"h":560}),
                        "images.release" => Value::Null,
                        _ => panic!("unexpected service {method}"),
                    };
                    self.ready.insert(self.token, value);
                    host.result(json!(self.token));
                } else if effect["method"] == "take" {
                    let packets: Vec<_> = effect["args"][1]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|t| {
                            let id = t.as_u64()?;
                            self.ready
                                .remove(&id)
                                .map(|value| json!({"token":id,"value":value}))
                        })
                        .collect();
                    for token in effect["args"][2].as_array().unwrap() {
                        self.ready.remove(&token.as_u64().unwrap());
                    }
                    host.result(json!({"method":"take","value":packets}));
                } else {
                    panic!("unexpected effect {effect}");
                }
            }
        }
        panic!("native service drain did not yield");
    }
}
#[test]
fn native_scanner_window_discovers_selects_previews_and_changes_options() {
    assert!(crate::native_view::app("scanner").is_some());
    let now = Rc::new(Cell::new(1000));
    let mount = scene::create(scene::VIEW);
    unsafe {
        let node = crate::runtime::node_get(mount);
        (*node).w = 860.;
        (*node).h = 620.;
    }
    let mut host = Host::new(
        mount,
        Box::new(Timed {
            app: App::default(),
            now: now.clone(),
        }),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    host.facts(json!({"args":{"__nativeOwner":7},"lang":"en","width":860,"context":{"theme":{"text":"#ffffff","textDim":"#999999","accent":"#0066ff"}}}));
    let mut services = Services::new();
    services.drain(&mut host);
    let advance = |host: &mut Host, services: &mut Services| {
        for _ in 0..6 {
            now.set(now.get() + 100);
            if let Some(deadline) = host.deadline() {
                host.tick(deadline + 1.);
            }
            services.drain(host);
        }
    };
    advance(&mut host, &mut services);
    assert!(host.text().contains("Desk scanner"));
    click(&mut host, mount, "Desk scanner");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(host.text().contains("300"));
    click(&mut host, mount, "PNG");
    click(&mut host, mount, "Preview");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(services.calls.iter().any(|(m, _)| m == "images.decode"));
    assert!(!host.text().contains("Scanning…"));
    click(&mut host, mount, "Another scanner…");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(services.calls.iter().any(|(m, _)| m == "images.release"));
    assert!(host.text().contains("Desk scanner"));
    drop(host);
    scene::destroy(mount);
}
