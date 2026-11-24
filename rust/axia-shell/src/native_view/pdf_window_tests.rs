//! The PDF window inside the real native host: scene nodes, translated captions,
//! pointer input and the helper effects it asks the shell for.
use axia_app_pdf::App;
use axia_native::NativeApp;
use serde_json::{json, Value};
use crate::{native_view::Host, runtime::native as scene};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};

struct Timed {
    app: App,
    now: Rc<Cell<i64>>,
}
impl NativeApp for Timed {
    fn uses_native_appearance(&self) -> bool {
        self.app.uses_native_appearance()
    }
    fn set_native_appearance(&mut self, theme: Rc<Value>) {
        self.app.set_native_appearance(theme);
    }
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
struct Helper {
    token: u64,
    ready: BTreeMap<u64, Value>,
    calls: Vec<(String, Value)>,
}
impl Helper {
    fn answer(&mut self, method: &str, args: &Value) -> Value {
        match method {
            "pdf.open" => json!({"doc":7,"pages":3,"title":"Quarterly report","sizes":[[612,792]]}),
            "pdf.sizes" => json!({"sizes":[[612,792],[612,792],[612,792]],"forms":false}),
            "pdf.render" => {
                self.token += 1000;
                json!({"image":100 + self.token as i64 / 1000,"w":args[2].as_f64().unwrap().round(),"h":(args[2].as_f64().unwrap() * 1.29).round()})
            }
            "pdf.pageInfo" => json!({"links":[],"fields":[],"notes":[]}),
            "pdf.outline" => json!({"items":[]}),
            "pdf.events" => json!([]),
            _ => Value::Null,
        }
    }
    fn drain(&mut self, host: &mut Host) {
        for _ in 0..64 {
            let Some(effects) = host.take() else { return };
            for effect in effects.as_array().unwrap() {
                if effect["host"] == true {
                    continue;
                }
                if effect["method"] == "start" {
                    self.token += 1;
                    let method = effect["args"][1].as_str().unwrap().to_owned();
                    let args = effect["args"][2].clone();
                    self.calls.push((method.clone(), args.clone()));
                    let value = self.answer(&method, &args);
                    self.ready.insert(self.token, value);
                    host.result(json!(self.token));
                } else if effect["method"] == "take" {
                    let packets: Vec<_> = effect["args"][1]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|t| {
                            let id = t.as_u64()?;
                            self.ready.remove(&id).map(|value| json!({"token":id,"value":value}))
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
    fn settle(&mut self, host: &mut Host, now: &Rc<Cell<i64>>, rounds: i64) {
        for _ in 0..rounds {
            now.set(now.get() + 100);
            if let Some(deadline) = host.deadline() {
                host.tick(deadline + 1.);
            }
            self.drain(host);
        }
    }
}
fn percent(text: &str) -> i64 {
    let end = text.find(" %").expect("the zoom percentage is shown");
    let digits: String = text[..end].chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse().unwrap()
}
#[test]
fn the_pdf_window_opens_a_document_in_the_real_host_and_zooms_with_a_click() {
    assert!(crate::native_view::app("pdf").is_some(), "the compiled registry mounts the viewer");
    let now = Rc::new(Cell::new(1000));
    let mount = scene::create(scene::VIEW);
    unsafe {
        let n = crate::runtime::node_get(mount);
        (*n).w = 980.;
        (*n).h = 680.;
    }
    let mut host = Host::new(
        mount,
        Box::new(Timed { app: App::default(), now: now.clone() }),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    host.facts(json!({"args":{"__nativeOwner":7,"__nativeWindow":"pdf","path":"/home/test/report.pdf"},"lang":"en","width":980,
        "context":{"theme":{"text":"#ffffff","textDim":"#999999","accent":"#0a84ff","separator":"#444444","mode":"dark","windowBg":"#222222",
            "sidebarBg":"#2a2a2e","control":"#333333","controlRadiusSmall":6}}}));
    let mut helper = Helper { token: 0, ready: BTreeMap::new(), calls: vec![] };
    helper.drain(&mut host);
    helper.settle(&mut host, &now, 12);
    let text = host.text();
    assert!(text.contains("Quarterly report"), "the title is in the status bar: {text}");
    assert!(text.contains("Page 1 of 3"), "captions are translated by the host: {text}");
    assert!(text.contains("Pages") && text.contains("Contents"), "the sidebar tabs are drawn");
    assert!(helper.calls.iter().any(|(m, a)| m == "pdf.open" && a[0] == "/home/test/report.pdf"));
    assert!(helper.calls.iter().any(|(m, _)| m == "pdf.render"), "visible pages are rendered");
    assert!(helper.calls.iter().any(|(m, _)| m == "windows.menus"), "the menus are published");
    let before = percent(&text);
    click(&mut host, mount, "+");
    helper.settle(&mut host, &now, 6);
    let after = percent(&host.text());
    assert!(after > before, "clicking + raises the zoom ({before} -> {after})");
    click(&mut host, mount, "Contents");
    helper.settle(&mut host, &now, 6);
    assert!(helper.calls.iter().any(|(m, _)| m == "pdf.outline"), "the outline tab asks for the outline");
    assert!(host.text().contains("This document has no outline."));
    drop(host);
    scene::destroy(mount);
}
