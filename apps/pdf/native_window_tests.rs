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
    events: Vec<Value>,
    tree: Value,
    exact: bool,
}
impl Helper {
    fn new(pages: usize) -> Self {
        Self { token: 0, ready: Default::default(), calls: vec![], pages: vec![[612., 792.]; pages], password: None, next_image: 100, pending: None, events: vec![], tree: Value::Null, exact: false }
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
            "pdf.pageInfo" => json!({
                "links":[{"r":[0,0,100,50],"page":2},{"r":[0,60,100,20],"uri":"https://example.test/a"}],
                "fields":[{"r":[10,100,50,20],"f":{"id":1,"type":"text","ro":false,"value":""}}],
                "notes":[]}),
            "pdf.select" => json!({"text":"hello","rects":[[10,200,30,10]]}),
            "pdf.events" => Value::Array(std::mem::take(&mut self.events)),
            "pdf.recent" => json!([{"name":"a.pdf","path":"/home/test/a.pdf"},{"name":"b.pdf","path":"/home/test/b.pdf"}]),
            _ => Value::Null,
        }
    }
    /// The host queues every effect a reply carries.
    fn absorb(&mut self, reply: Value) -> Value {
        if let Some(effect) = reply.get("effect") {
            self.pending = Some(effect.clone());
        }
        if let Some(tree) = reply.get("tree") {
            self.tree = tree.clone();
        }
        reply
    }
    fn absorb_tree(&mut self, reply: &Value) {
        if let Some(tree) = reply.get("tree") {
            self.tree = tree.clone();
        }
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
        self.drive_for(app, now, 0)
    }
    /// Lets exactly `ms` milliseconds pass (a search that has not ended keeps polling forever).
    fn pass(&mut self, app: &mut App, now: &mut i64, ms: i64) -> Value {
        self.exact = true;
        let reply = self.drive_for(app, now, ms);
        self.exact = false;
        reply
    }
    /// Like `drive`, but lets at least `min` milliseconds pass so timers (debounce, polling) fire.
    fn drive_for(&mut self, app: &mut App, now: &mut i64, min: i64) -> Value {
        let begin = *now;
        for _ in 0..400 {
            *now += 60;
            if self.exact && *now - begin >= min && self.pending.is_none() {
                return json!({"tree":self.tree});
            }
            let reply = match self.pending.take() {
                Some(effect) => json!({"effect":effect}),
                None => app.request_at(&json!({"cmd":"noop"}), *now),
            };
            self.absorb_tree(&reply);
            let Some(effect) = reply.get("effect") else {
                if !app.jobs.busy() && app.find_due.is_none() && *now - begin >= min {
                    return json!({"tree":self.tree});
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

/// Pointer position over a page point, in the window's own coordinates.
fn over(app: &App, page: usize, x: f64, y: f64) -> (f64, f64) {
    let frame = app.frames.iter().find(|f| f["page"] == page).expect("page is visible");
    let ax = if app.side.is_some() { ui::SIDEBAR } else { 0. };
    (ax + frame["left"].as_f64().unwrap() + x * app.scale, ui::TOOLBAR + frame["top"].as_f64().unwrap() + y * app.scale)
}
fn pointer(app: &mut App, helper: &mut Helper, now: i64, cmd: &str, at: (f64, f64)) -> Value {
    helper.send(app, now, json!({"cmd":cmd,"x":at.0,"y":at.1,"button":0}))
}
#[test]
fn page_links_and_form_fields_come_from_the_helper_and_links_are_followed() {
    let (mut app, mut now, mut helper) = start(4, json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/report.pdf"}));
    let reply = helper.drive(&mut app, &mut now);
    assert!(helper.count("pdf.pageInfo") >= 1);
    assert!(tree_text(&reply["tree"]).contains("field-0"), "an editable field is outlined");
    let at = over(&app, 0, 10., 10.);
    pointer(&mut app, &mut helper, now, "down", at);
    helper.drive(&mut app, &mut now);
    assert_eq!(app.page, 2, "an internal link goes to its page");
    let at = over(&app, app.page, 10., 70.);
    pointer(&mut app, &mut helper, now, "down", at);
    let reply = helper.drive(&mut app, &mut now);
    assert!(helper.calls.iter().any(|(m, a)| m == "clipboard.write" && a[0] == "https://example.test/a"));
    assert!(tree_text(&reply["tree"]).contains("pdf.linkCopied"), "the person is told the link was copied");
}
#[test]
fn dragging_selects_text_and_copying_sends_it_to_the_clipboard() {
    let (mut app, mut now, mut helper) = start(2, json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/report.pdf"}));
    helper.drive(&mut app, &mut now);
    let start_at = over(&app, 0, 20., 300.);
    let down = pointer(&mut app, &mut helper, now, "down", start_at);
    assert_eq!(down["capture"], true, "the drag follows the pointer wherever it goes");
    now += 80;
    let to = over(&app, 0, 120., 320.);
    pointer(&mut app, &mut helper, now, "pointerMove", to);
    let end = pointer(&mut app, &mut helper, now, "dragEnd", to);
    assert_eq!(end["capture"], false);
    let reply = helper.drive(&mut app, &mut now);
    assert!(helper.count("pdf.select") >= 1);
    assert!(tree_text(&reply["tree"]).contains("selection-0"));
    assert!(tree_text(&reply["tree"]).contains("pdf.selected"));
    assert!(helper.key(&mut app, "c", "c", 16, now).is_some());
    let reply = helper.drive(&mut app, &mut now);
    assert!(helper.calls.iter().any(|(m, a)| m == "clipboard.write" && a[0] == "hello"));
    assert!(tree_text(&reply["tree"]).contains("pdf.copied"));
    helper.key(&mut app, "Escape", "", 0, now);
    assert!(app.selection.is_none(), "Escape clears the selection");
}
#[test]
fn search_streams_hits_page_by_page_goes_to_the_first_and_cycles() {
    let (mut app, mut now, mut helper) = start(4, json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/report.pdf"}));
    helper.drive(&mut app, &mut now);
    helper.send(&mut app, now, json!({"cmd":"findChange","value":"hello"}));
    helper.pass(&mut app, &mut now, 600);
    let (_, args) = helper.calls.iter().rev().find(|(m, _)| m == "pdf.find").cloned().expect("the search was debounced then sent");
    assert_eq!(args[1], "hello");
    let token = args[2].clone();
    helper.events.push(json!({"kind":"find","token":token,"page":2,"rects":[[5,5,20,10],[5,40,20,10]]}));
    let reply = helper.pass(&mut app, &mut now, 600);
    assert_eq!(app.page, 2, "the first hit brings its page into view");
    assert!(tree_text(&reply["tree"]).contains("hit-0"));
    assert!(tree_text(&reply["tree"]).contains("pdf.matches"));
    helper.events.push(json!({"kind":"findDone","token":token}));
    helper.events.push(json!({"kind":"find","token":999,"page":1,"rects":[[1,1,1,1]]}));
    helper.pass(&mut app, &mut now, 600);
    assert_eq!(app.hits.as_ref().unwrap()["list"].as_array().unwrap().len(), 2, "results of an old search are ignored");
    assert_eq!(app.hits.as_ref().unwrap()["done"], true);
    helper.key(&mut app, "g", "g", 16, now);
    assert_eq!(app.hits.as_ref().unwrap()["cur"], 1);
    helper.key(&mut app, "g", "g", 16 | 1, now);
    assert_eq!(app.hits.as_ref().unwrap()["cur"], 0);
    helper.key(&mut app, "Escape", "", 0, now);
    assert!(app.hits.is_none(), "Escape ends the search");
}
#[test]
fn go_to_page_opens_a_field_and_a_valid_number_navigates() {
    let (mut app, mut now, mut helper) = start(6, json!({"__nativeOwner":9,"__nativeWindow":"pdf","path":"/home/test/report.pdf"}));
    helper.drive(&mut app, &mut now);
    assert!(helper.key(&mut app, "g", "g", 16 | 4, now).is_some());
    let reply = helper.drive(&mut app, &mut now);
    assert!(tree_text(&reply["tree"]).contains("pdf.goPlaceholder"));
    helper.send(&mut app, now, json!({"cmd":"gotoChange","value":"4"}));
    helper.send(&mut app, now, json!({"cmd":"gotoSubmit"}));
    assert_eq!(app.page, 3);
    assert!(app.goto.is_none());
    helper.send(&mut app, now, json!({"cmd":"gotoOpen"}));
    helper.send(&mut app, now, json!({"cmd":"gotoChange","value":"nope"}));
    helper.send(&mut app, now, json!({"cmd":"gotoSubmit"}));
    assert_eq!(app.page, 3, "an invalid number goes nowhere");
}
