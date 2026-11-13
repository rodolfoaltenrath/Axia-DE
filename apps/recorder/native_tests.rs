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
    fn background_refresh_ms(&self) -> Option<f64> {
        self.app.background_refresh_ms()
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
                        "media.dirs" => json!({"music":"/home/test/Music"}),
                        "media.sound" => {
                            json!({"sources":[{"name":"mic","description":"Microphone","monitorOf":-1,"default":true}]})
                        }
                        "media.events" => json!([]),
                        "fs.mkdir" | "media.close" | "media.control" => Value::Null,
                        "fs.list" => {
                            json!([{"name":"a.wav","mtime":1700000000000i64},{"name":"b.mp3","mtime":1600000000000i64}])
                        }
                        "media.waveform" => json!({"duration":10000,"peaks":[10,200,100,800]}),
                        "media.play" => json!(9),
                        "media.record" => json!(8),
                        "fs.stat" => json!({"__error":"ENOENT"}),
                        "media.transcode" => json!({"ok":true,"path":args[2]}),
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
                            self.ready.remove(&id).map(|value| {
                                if value["__error"].is_string() {
                                    json!({"token":id,"error":value["__error"]})
                                } else {
                                    json!({"token":id,"value":value})
                                }
                            })
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
fn native_recorder_selects_audio_renders_wave_and_exports_trimmed_copy() {
    assert!(crate::native_view::app("recorder").is_some());
    let now = Rc::new(Cell::new(1000));
    let mount = scene::create(scene::VIEW);
    unsafe {
        let n = crate::runtime::node_get(mount);
        (*n).w = 800.;
        (*n).h = 480.;
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
    host.facts(json!({"args":{"__nativeOwner":7},"lang":"en","width":800,"context":{"theme":{"text":"#ffffff","textDim":"#999999","accent":"#0066ff"}}}));
    let mut services = Services::new();
    services.drain(&mut host);
    let advance = |host: &mut Host, services: &mut Services| {
        for _ in 0..12 {
            now.set(now.get() + 100);
            if let Some(deadline) = host.deadline() {
                host.tick(deadline + 1.);
            }
            services.drain(host);
        }
    };
    advance(&mut host, &mut services);
    assert!(host.text().contains("All recordings"));
    click(&mut host, mount, "a");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(host.text().contains("0:10"));
    assert!(services
        .calls
        .iter()
        .any(|(m, a)| m == "media.waveform" && a[0] == "/home/test/Music/Recordings/a.wav"));
    click(&mut host, mount, "Play");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(host.text().contains("Pause"));
    click(&mut host, mount, "Trim");
    services.drain(&mut host);
    click(&mut host, mount, "Save trimmed copy");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(
        services.calls.iter().any(|(m, a)| m == "media.transcode"
            && a[3] == json!({"format":"wav","from":1000.,"to":9000.})),
        "{:?}",
        services.calls
    );
    assert!(host.text().contains("Saved"));
    click(&mut host, mount, "b");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(services.calls.iter().any(|(m, _)| m == "media.close"));
    drop(host);
    scene::destroy(mount);
}
