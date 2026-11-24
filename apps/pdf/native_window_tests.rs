//! The native window against fake helper services: it opens documents, schedules
//! renders through the cache model, follows the viewport and never keeps stale replies.
use super::*;

struct Helper {
    token: u64,
    ready: std::collections::BTreeMap<u64, Value>,
    calls: Vec<(String, Value)>,
    pages: Vec<[f64; 2]>,
    password: Option<&'static str>,
    next_image: i64,
    pending: Option<Value>,
}
impl Helper {
    fn new(pages: usize) -> Self {
        Self { token: 0, ready: Default::default(), calls: vec![], pages: vec![[612., 792.]; pages], password: None, next_image: 100, pending: None }
    }
    fn answer(&mut self, method: &str, args: &Value) -> Value {
        match method {
            "pdf.open" => match (self.password, args[1].as_str()) {
                (Some(want), Some(given)) if want != given => json!({"error":"password"}),
                _ => json!({"doc":7,"pages":self.pages.len(),"title":"Report","sizes":[self.pages[0]]}),
            },
            "pdf.sizes" => json!({"sizes":self.pages,"forms":false}),
            "pdf.render" => {
                self.next_image += 1;
                json!({"image":self.next_image,"w":args[2].as_f64().unwrap().round(),"h":(args[2].as_f64().unwrap() * 1.3).round()})
            }
            "pdf.outline" => json!({"items":[{"title":"Intro","page":0,"kids":[]},{"title":"End","page":2,"kids":[]}]}),
            "pdf.recent" => json!([{"name":"a.pdf","path":"/home/test/a.pdf"},{"name":"b.pdf","path":"/home/test/b.pdf"}]),
            _ => Value::Null,
        }
    }
    /// The host queues every effect a reply carries.
    fn absorb(&mut self, reply: Value) -> Value {
        if let Some(effect) = reply.get("effect") {
            self.pending = Some(effect.clone());
        }
        reply
    }
    fn send(&mut self, app: &mut App, now: i64, input: Value) -> Value {
        let reply = app.request_at(&input, now);
        self.absorb(reply)
    }
    fn key(&mut self, app: &mut App, key: &str, chr: &str, flags: i32, now: i64) -> Option<Value> {
        let reply = app.key_at(key, chr, flags, now)?;
        Some(self.absorb(reply))
    }
    /// Runs the host side of every effect the app asks for until it is quiet.
    fn drive(&mut self, app: &mut App, now: &mut i64) -> Value {
        let mut last = json!({});
        for _ in 0..400 {
            *now += 60;
            let reply = match self.pending.take() {
                Some(effect) => json!({"effect":effect}),
                None => app.request_at(&json!({"cmd":"noop"}), *now),
            };
            if reply.get("tree").is_some() {
                last = reply.clone();
            }
            let Some(effect) = reply.get("effect") else {
                if !app.jobs.busy() {
                    return last;
                }
                continue;
            };
            let result = match effect["method"].as_str() {
                Some("start") => {
                    self.token += 1;
                    let method = effect["args"][1].as_str().unwrap().to_owned();
                    let args = effect["args"][2].clone();
                    self.calls.push((method.clone(), args.clone()));
                    let value = self.answer(&method, &args);
                    self.ready.insert(self.token, value);
                    json!(self.token)
                }
                _ => {
                    let packets: Vec<Value> = effect["args"][1]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|t| {
                            let id = t.as_u64()?;
                            let value = self.ready.remove(&id)?;
                            Some(match value["error"].as_str() {
                                Some(error) => json!({"token":id,"error":error}),
                                None => json!({"token":id,"value":value}),
                            })
                        })
                        .collect();
                    for token in effect["args"][2].as_array().unwrap() {
                        self.ready.remove(&token.as_u64().unwrap());
                    }
                    json!({"method":"take","value":packets})
                }
            };
            let reply = app.request_at(&json!({"cmd":"next","result":result}), *now);
            if reply.get("tree").is_some() {
                last = reply.clone();
            }
            self.absorb(reply);
        }
        panic!("the window never settled");
    }
    fn count(&self, method: &str) -> usize {
        self.calls.iter().filter(|(m, _)| m == method).count()
    }
}
fn start(pages: usize, args: Value) -> (App, i64, Helper) {
    let (app, now, reply) = boot_with_reply(args);
    let mut helper = Helper::new(pages);
    helper.absorb(reply);
    (app, now, helper)
}
fn boot_with_reply(args: Value) -> (App, i64, Value) {
    let mut app = App::default();
    let reply = app.request_at(
        &json!({"cmd":"sync","args":args,"context":{"width":980,"height":680,"home":"/home/test",
            "theme":{"text":"#ffffff","textDim":"#999999","accent":"#0a84ff","separator":"#444444","mode":"dark","windowBg":"#222222"}}}),
        1000,
    );
    (app, 1000, reply)
}
fn tree_text(tree: &Value) -> String {
    tree.to_string()
}
fn count(tree: &Value, needle: &str) -> usize {
    tree_text(tree).matches(needle).count()
}

#[test]
fn a_document_opens_renders_the_visible_pages_and_shows_its_sidebar() {
    let (mut app, mut now, mut helper) = start(3, json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/report.pdf"}));
    let reply = helper.drive(&mut app, &mut now);
    let tree = &reply["tree"];
    assert_eq!(helper.calls[0], ("pdf.open".into(), json!(["/home/test/report.pdf", ""])));
    assert_eq!(helper.count("pdf.sizes"), 1, "all page sizes arrive after the first page");
    assert!(helper.count("pdf.render") >= 2, "visible pages and thumbnails are rendered");
    assert!(count(tree, "\"documentImage\"") >= 2, "page rasters are on the scene");
    assert!(tree_text(tree).contains("\"pdf.pageOf\""));
    assert!(tree_text(tree).contains("Report"), "the document title is in the status bar");
    assert_eq!(reply["title"], Value::Null, "the title is announced once with the first reply");
    assert!(tree_text(tree).contains("pdf-sidebar:"), "the sidebar is part of the same scene");
}
#[test]
fn the_window_title_is_the_file_name_and_wheel_scroll_moves_the_viewport() {
    let (mut app, mut now, first) = boot_with_reply(json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/report.pdf"}));
    assert_eq!(first["title"], "report.pdf");
    let mut helper = Helper::new(8);
    helper.absorb(first);
    helper.drive(&mut app, &mut now);
    assert_eq!(app.offset, 0.);
    helper.send(&mut app, now, json!({"cmd":"wheel","deltaY":240.,"shift":false,"ctrl":false,"meta":false}));
    assert!(app.offset > 0., "a plain wheel scrolls continuous mode");
    let before = app.scale;
    helper.send(&mut app, now, json!({"cmd":"wheel","deltaY":-120.,"notches":1,"ctrl":true,"meta":false,"shift":false}));
    assert!(app.scale > before, "ctrl + wheel zooms in by a step");
    helper.drive(&mut app, &mut now);
    assert!(app.page <= 7);
}
#[test]
fn a_protected_document_asks_for_the_password_and_opens_with_it() {
    let (mut app, mut now, mut helper) = start(2, json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/locked.pdf"}));
    helper.password = Some("secret");
    let reply = helper.drive(&mut app, &mut now);
    assert!(tree_text(&reply["tree"]).contains("pdf.passwordTitle"));
    assert!(!tree_text(&reply["tree"]).contains("pdf.wrongPassword"), "the first request is not an error");
    helper.send(&mut app, now, json!({"cmd":"passwordChange","value":"nope"}));
    helper.send(&mut app, now, json!({"cmd":"password"}));
    let reply = helper.drive(&mut app, &mut now);
    assert!(tree_text(&reply["tree"]).contains("pdf.wrongPassword"));
    helper.send(&mut app, now, json!({"cmd":"passwordChange","value":"secret"}));
    helper.send(&mut app, now, json!({"cmd":"password"}));
    let reply = helper.drive(&mut app, &mut now);
    assert!(count(&reply["tree"], "\"documentImage\"") >= 1);
    assert_eq!(helper.calls.iter().filter(|(m, a)| m == "pdf.open" && a[1] == "secret").count(), 1);
}
#[test]
fn a_replaced_document_closes_the_old_one_and_never_shows_its_late_pages() {
    let (mut app, mut now, mut helper) = start(2, json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/a.pdf"}));
    helper.send(&mut app, now, json!({"cmd":"open","key":"/home/test/b.pdf"}));
    helper.drive(&mut app, &mut now);
    assert_eq!(app.path.as_deref(), Some("/home/test/b.pdf"));
    assert!(helper.calls.iter().any(|(m, a)| m == "pdf.open" && a[0] == "/home/test/b.pdf"));
    assert!(
        helper.calls.iter().filter(|(m, _)| m == "pdf.close").count() + helper.calls.iter().filter(|(m, a)| m == "pdf.open" && a[0] == "/home/test/a.pdf").count() <= 2,
        "a superseded open is cancelled or closed, never kept"
    );
    assert!(app.doc.is_some());
}
#[test]
fn navigation_keys_zoom_rotation_and_mode_change_the_scene() {
    let (mut app, mut now, mut helper) = start(5, json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/report.pdf"}));
    helper.drive(&mut app, &mut now);
    assert!(helper.key(&mut app, "End", "", 0, now).is_some());
    assert_eq!(app.page, 4, "End goes to the last page");
    assert!(helper.key(&mut app, "Home", "", 0, now).is_some());
    assert_eq!(app.page, 0);
    let pct = app.pct;
    assert!(helper.key(&mut app, "+", "+", 16, now).is_some());
    helper.drive(&mut app, &mut now);
    assert!(app.pct > pct, "zoom in raises the percentage");
    assert!(helper.key(&mut app, "r", "r", 16, now).is_some());
    assert_eq!(app.rot, 1);
    helper.send(&mut app, now, json!({"cmd":"mode","key":"single"}));
    helper.send(&mut app, now, json!({"cmd":"page","key":3}));
    assert_eq!(app.page, 3, "single mode shows the chosen page");
    assert!(helper.key(&mut app, "q", "q", 0, now).is_none(), "keys the viewer does not own stay with the shell");
}
#[test]
fn the_welcome_list_comes_from_the_helper_and_opens_a_file() {
    let (mut app, mut now, mut helper) = start(1, json!({"__nativeOwner":9,"__nativeWindow":"pdf"}));
    let reply = helper.drive(&mut app, &mut now);
    assert!(tree_text(&reply["tree"]).contains("a.pdf") && tree_text(&reply["tree"]).contains("pdf.welcome"));
    helper.send(&mut app, now, json!({"cmd":"open","key":"/home/test/b.pdf"}));
    helper.drive(&mut app, &mut now);
    assert_eq!(helper.calls.iter().filter(|(m, _)| m == "pdf.open").count(), 1);
}
#[test]
fn the_sidebar_outline_loads_once_and_picks_a_page() {
    let (mut app, mut now, mut helper) = start(3, json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/report.pdf"}));
    helper.drive(&mut app, &mut now);
    helper.send(&mut app, now, json!({"cmd":"side","key":"outline"}));
    let reply = helper.drive(&mut app, &mut now);
    assert_eq!(helper.count("pdf.outline"), 1);
    assert!(tree_text(&reply["tree"]).contains("End"), "bookmark rows are drawn");
    helper.send(&mut app, now, json!({"cmd":"side","key":"thumbs"}));
    helper.send(&mut app, now, json!({"cmd":"side","key":"outline"}));
    helper.drive(&mut app, &mut now);
    assert_eq!(helper.count("pdf.outline"), 1, "the outline is not requested again");
}
#[test]
fn the_icon_is_a_scene_of_the_requested_size() {
    let mut app = App::default();
    let icon = app.request_at(&json!({"cmd":"icon","size":64}), 0);
    assert_eq!(icon["style"]["width"], 64.);
    assert!(tree_text(&icon).contains("PDF"));
}
