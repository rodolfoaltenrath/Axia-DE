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
                        "fonts.list" => {
                            json!([{"family":"Example","style":"Regular","path":"/fonts/a.ttf","index":0},{"family":"Other","style":"Bold","path":"/fonts/b.ttf","index":0}])
                        }
                        "fonts.info" => {
                            json!({"family":"Example","style":"Regular","glyphs":100,"chars":95,"scalable":true,"ranges":[[32,126]]})
                        }
                        "fs.stat" => json!({"size":1024}),
                        "fonts.sample" => json!({"image":100+self.token,"w":300,"h":40}),
                        "fonts.grid" => {
                            json!({"image":100+self.token,"w":464,"h":174,"data":{"present":[true,false,true]}})
                        }
                        _ => Value::Null,
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
fn native_font_window_selects_families_changes_tabs_and_receives_helper_images() {
    assert!(crate::native_view::app("fonts").is_some());
    let now = Rc::new(Cell::new(1000));
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(
        mount,
        Box::new(Timed {
            app: App::default(),
            now: now.clone(),
        }),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    host.facts(json!({"args":{"__nativeOwner":7},"lang":"en","width":960,"context":{"theme":{"text":"#ffffff","textDim":"#999999","accent":"#0066ff"}}}));
    let mut service = Services::new();
    service.drain(&mut host);
    for time in [1100, 1200, 1300, 1400] {
        now.set(time);
        if let Some(deadline) = host.deadline() {
            host.tick(deadline + 1.);
        }
        service.drain(&mut host);
    }
    assert!(host.text().contains("Example"));
    assert!(host.text().contains("Preview"));
    assert!(service.calls.iter().any(|(m, _)| m == "fonts.sample"));
    click(&mut host, mount, "Other");
    service.drain(&mut host);
    assert!(service
        .calls
        .iter()
        .any(|(m, a)| m == "fonts.info" && a[0] == "/fonts/b.ttf"));
    click(&mut host, mount, "Info");
    service.drain(&mut host);
    for time in [1500, 1600] {
        now.set(time);
        if let Some(deadline) = host.deadline() {
            host.tick(deadline + 1.);
        }
        service.drain(&mut host);
    }
    assert!(host.text().contains("PostScript name"));
    assert!(host.text().contains("/fonts/b.ttf"));
    click(&mut host, mount, "Characters");
    service.drain(&mut host);
    for time in [1700, 1800, 1900] {
        now.set(time);
        if let Some(deadline) = host.deadline() {
            host.tick(deadline + 1.);
        }
        service.drain(&mut host);
    }
    assert!(host.text().contains("Basic Latin"));
    assert!(service.calls.iter().any(|(m, _)| m == "fonts.grid"));
    assert!(service.calls.iter().any(|(m, _)| m == "fonts.release"));
    drop(host);
    scene::destroy(mount);
}
