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
    events: Vec<Value>,
}
impl Services {
    fn new() -> Self {
        Self {
            token: 0,
            ready: BTreeMap::new(),
            calls: vec![],
            events: vec![],
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
                        "backup.watch" => {
                            json!({"state":"idle","dest":"/backup","destAvailable":true})
                        }
                        "backup.events" => json!(std::mem::take(&mut self.events)),
                        "backup.config" => {
                            json!({"dest":"/backup","interval":"hourly","pauseOnBattery":true,"exclude":[]})
                        }
                        "backup.configure" => args[0].clone(),
                        "backup.list" => json!([{"id":"old","time":100},{"id":"new","time":200}]),
                        "backup.ls" => {
                            if args[1] == "Folder" {
                                json!([{"name":"nested.txt","size":1024}])
                            } else {
                                json!([{"name":"Folder","dir":true},{"name":"alpha.txt","size":42}])
                            }
                        }
                        "fs.volumes" => {
                            json!([{"path":"/backup","name":"External","free":1000000000,"total":2000000000,"fs":"ext4"}])
                        }
                        "backup.path" => json!("/backup/selected/home"),
                        "backup.restore" | "backup.remove" | "backup.unlock" | "backup.lock"
                        | "backup.run" | "backup.pause" | "backup.cancel" | "windows.open" => {
                            json!(true)
                        }
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
fn native_backup_browses_restores_confirms_and_configures() {
    assert!(crate::native_view::app("backup").is_some());
    let now = Rc::new(Cell::new(1000));
    let mount = scene::create(scene::VIEW);
    unsafe {
        let n = crate::runtime::node_get(mount);
        (*n).w = 980.;
        (*n).h = 640.;
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
    host.facts(json!({"args":{"__nativeOwner":7},"lang":"en","width":980,"context":{"theme":{"text":"#ffffff","textDim":"#999999","accent":"#0066ff"}}}));
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
    assert!(host.text().contains("alpha.txt"));
    click(&mut host, mount, "alpha.txt");
    services.drain(&mut host);
    click(&mut host, mount, "Restore “alpha.txt”");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(services.calls.iter().any(|(m, a)| m == "backup.restore"
        && a == &json!(["new","alpha.txt",{"replace":false,"suffix":" (restored)"}])));
    host.action(&json!({"action":["enter","Folder"]}));
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(host.text().contains("nested.txt"));
    click(&mut host, mount, "Open in Files");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(services.calls.iter().any(|(m, a)| m == "windows.open"
        && a == &json!(["files",{"new":true,"args":{"path":"/backup/selected/home/Folder"}}])));
    click(&mut host, mount, "Restore All");
    services.drain(&mut host);
    assert!(host.text().contains("Restore your whole home folder?"));
    click(&mut host, mount, "Cancel");
    services.drain(&mut host);
    assert!(!host.text().contains("Restore your whole home folder?"));
    click(&mut host, mount, "Delete Backup");
    services.drain(&mut host);
    assert!(host.text().contains("Delete this backup?"));
    host.action(&json!({"action":["confirm"]}));
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(services
        .calls
        .iter()
        .any(|(m, a)| m == "backup.remove" && a == &json!(["new"])));
    click(&mut host, mount, "Settings");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(host.text().contains("External"));
    click(&mut host, mount, "Daily");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(services
        .calls
        .iter()
        .any(|(m, a)| m == "backup.configure" && a[0]["interval"] == "daily"));
    services.events.push(json!({"kind":"backup","status":{"state":"running","dest":"/backup","destAvailable":true,"progress":{"files":2,"totalFiles":4,"bytes":1024,"totalBytes":2048}}}));
    advance(&mut host, &mut services);
    assert!(host.text().contains("Backing up… 2 of 4 files"));
    click(&mut host, mount, "Pause");
    services.drain(&mut host);
    advance(&mut host, &mut services);
    assert!(services
        .calls
        .iter()
        .any(|(m, a)| m == "backup.pause" && a == &json!([true])));
    drop(host);
    scene::destroy(mount);
}
