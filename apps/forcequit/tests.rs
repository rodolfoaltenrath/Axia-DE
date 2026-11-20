use super::*;
use std::cell::{Cell, RefCell};

#[derive(Default)]
struct Source {
    revision: Cell<u64>,
    windows: RefCell<Vec<Value>>,
}
impl Source {
    fn set(&self, windows: Vec<Value>) {
        *self.windows.borrow_mut() = windows;
        self.revision.set(self.revision.get() + 1);
    }
}
impl WindowSource for Source {
    fn revision(&self) -> u64 { self.revision.get() }
    fn visit(&self, visitor: &mut dyn FnMut(WindowRef<'_>)) {
        for window in self.windows.borrow().iter() {
            visitor(WindowRef { id:s(window,"id"), app:s(window,"app"), title:s(window,"title"), args:&window["args"] });
        }
    }
}

#[test]
fn borrowed_groups_use_catalog_identity_and_all_windows_without_legacy_facts() {
    assert_eq!(pretty_id(".org.my_app."), "My app");
    let source = Rc::new(Source::default());
    source.set(vec![
        json!({"id":"panel","app":"panel"}),
        json!({"id":"forcequit","app":"forcequit"}),
        json!({"id":"notes#a","app":"notes","desktop":9,"minimized":true}),
        json!({"id":"notes#b","app":"notes"}),
        json!({"id":"linux#1","app":"linux","args":{"appId":"EDITOR","surface":11}}),
        json!({"id":"linux#2","app":"linux","args":{"appId":"isolated","surface":12,"pkg":true}}),
        json!({"id":"linux#3","app":"linux","args":{"appId":"org.unknown.some-app","surface":13}}),
        json!({"id":"linux#4","app":"linux","args":{}}),
    ]);
    let mut app = App::with_windows(source);
    let facts = json!({"cmd":"groups","registry":[
        {"id":"panel","builtin":true,"name":"Panel"}, {"id":"linux","builtin":true,"name":"Linux"},
        {"id":"notes","name":"Notas"}, {"id":"isolated","isolated":true,"name":"Isolated"}],
        "catalog":[{"id":"desktop:org.editor","kind":"linux","desktopId":"org.editor","wmClass":"Editor","shortName":"Editor"},
            {"id":"desktop:ambiguous","kind":"linux","desktopId":"another","wmClass":"Editor","shortName":"Another editor"}],
        "windows":[{"id":"stale","key":"stale","name":"Wrong"}]});
    let groups = app.request(&facts)["groups"].as_array().unwrap().clone();
    assert_eq!(groups.len(), 5);
    let by_key = |key| groups.iter().find(|g| g["key"] == key).unwrap();
    assert_eq!(by_key("notes")["name"], "Notas");
    assert_eq!(by_key("notes")["windows"], json!(["notes#a","notes#b"]));
    assert_eq!(by_key("desktop:org.editor")["name"], "Editor");
    assert_eq!(by_key("isolated")["name"], "Isolated");
    assert_eq!(by_key("linux:org.unknown.some-app")["name"], "Some app");
    assert_eq!(by_key("linux:linux#4")["name"], "Linux");
}

#[test]
fn borrowed_membership_invalidates_selection_and_reads_fresh_surface_handles() {
    use crate::native_view::NativeApp;
    let source = Rc::new(Source::default());
    source.set(vec![json!({"id":"linux#1","app":"linux","args":{"appId":"pkg","pkg":true,"surface":10}})]);
    let mut app = App::with_windows(source.clone());
    let facts = json!({"cmd":"sync","registry":[{"id":"pkg","package":true,"name":"Package"}]});
    NativeApp::request(&mut app, &facts);
    assert!(!app.invalidated());
    NativeApp::request(&mut app, &json!({"cmd":"select","key":"pkg"}));
    source.set(vec![]);
    assert!(app.invalidated());
    NativeApp::request(&mut app, &facts);
    assert!(!app.invalidated());
    assert!(NativeApp::request(&mut app, &json!({"cmd":"quit"})).is_null());
    source.set(vec![json!({"id":"linux#1","app":"linux","args":{"appId":"pkg","pkg":true,"surface":20}})]);
    let mut quit = facts.clone();quit["cmd"] = json!("quit");quit["key"] = json!("pkg");
    assert_eq!(app.request(&quit)["effect"], json!({"kind":"kill","surface":20}));
    drop(source);
    assert!(app.source.is_some(), "reader owns its source lifetime");
}

#[test]
fn cached_metadata_is_reused_and_invalid_updates_cannot_replace_it() {
    let source = Rc::new(Source::default());
    source.set(vec![json!({"id":"notes#a","app":"notes"})]);
    let mut app = App::with_windows(source.clone());
    let metadata = app.metadata_reader();
    let changes = metadata.subscribe();
    assert_eq!(app.request(&json!({"cmd":"metadata","registry":[{"id":"notes","name":"Notas"}],"catalog":[]}))["ok"], true);
    assert!(changes.take().is_some());
    assert_eq!(metadata.read()["registry"][0]["name"], "Notas");
    app.request(&json!({"cmd":"metadata","registry":[{"id":"notes","name":"Notas"}],"catalog":[]}));
    assert!(!changes.pending());
    assert_eq!(app.request(&json!({"cmd":"groups"}))["groups"][0]["name"], "Notas");
    assert_eq!(app.request(&json!({"cmd":"metadata","registry":null,"catalog":[]}))["error"], "app.invalidRequest");
    assert!(!changes.pending());
    assert_eq!(app.request(&json!({"cmd":"groups"}))["groups"][0]["name"], "Notas");
    source.set(vec![json!({"id":"notes#b","app":"notes"})]);
    let result = app.request(&json!({"cmd":"quit","key":"notes","windows":[{"id":"stale","key":"notes"}]}));
    assert_eq!(result["effect"], json!({"kind":"closeWindow","id":"notes#b"}));
}
fn snapshot() -> Value {
    json!([
        {"id":"panel","key":"panel","name":"Panel","builtin":true},
        {"id":"forcequit","key":"forcequit","name":"Force Quit","app":"forcequit"},
        {"id":"linux#1","key":"editor","name":"Editor","surface":1,"isolated":true,"builtin":true},
        {"id":"linux#2","key":"editor","name":"Editor","surface":2,"isolated":true},
        {"id":"linux#3","key":"editor","name":"Editor","surface":3},
        {"id":"notes#1","key":"notes","name":"Notes"},
        {"id":"notes#2","key":"notes","name":"Notes"},
        {"id":"linux#4","key":"xterm","name":"XTerm","surface":4,"x11":true}
    ])
}
fn request(app: &mut App, cmd: &str, extra: Value) -> Value {
    let mut input = extra;
    input["cmd"] = json!(cmd);
    app.request(&input)
}
#[test]
fn groups_sort_by_name_ignoring_accents_and_case() {
    let mut app = App::default();
    let windows = json!([
        {"id":"a#1","key":"zed","name":"zed"},
        {"id":"a#2","key":"area","name":"Área de trabalho"},
        {"id":"a#3","key":"browser","name":"Browser"},
        {"id":"a#4","key":"eclair","name":"éclair"}
    ]);
    let result = request(&mut app, "groups", json!({"windows":windows}));
    let keys: Vec<_> = result["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["area", "browser", "eclair", "zed"]);
}
#[test]
fn grouping_filters_shell_and_self_but_preserves_all_desktops_and_windows() {
    let mut app = App::default();
    let result = request(&mut app, "groups", json!({"windows":snapshot()}));
    assert_eq!(result["groups"].as_array().unwrap().len(), 3);
    assert_eq!(result["groups"][0]["key"], "editor");
    assert_eq!(
        result["groups"][0]["windows"],
        json!(["linux#1", "linux#2", "linux#3"])
    );
    assert_eq!(
        result["groups"][1]["windows"],
        json!(["notes#1", "notes#2"])
    );
}
#[test]
fn stale_selection_and_separate_app_instances_never_kill_another_group() {
    let mut a = App::default();
    let mut b = App::default();
    for app in [&mut a, &mut b] {
        request(app, "sync", json!({"windows":snapshot()}));
    }
    request(&mut a, "select", json!({"key":"notes"}));
    assert_eq!(a.selected.as_deref(), Some("notes"));
    assert_eq!(b.selected, None);
    let result = request(&mut a, "quit", json!({"windows":[]}));
    assert_eq!(result["effect"], Value::Null);
    assert_eq!(result["error"], "app.notFound");
    assert_eq!(a.selected, None);
}
#[test]
fn builtins_force_close_every_window_and_missing_keys_have_no_effect() {
    let mut app = App::default();
    let first = request(
        &mut app,
        "quit",
        json!({"key":"notes","windows":snapshot()}),
    );
    assert_eq!(
        first["effect"],
        json!({"kind":"closeWindow","id":"notes#1"})
    );
    assert_eq!(
        request(&mut app, "next", json!({"result":""}))["effect"]["id"],
        "notes#2"
    );
    assert_eq!(
        request(&mut app, "next", json!({"result":""}))["effect"],
        Value::Null
    );
    let absent = request(
        &mut app,
        "quit",
        json!({"key":"missing","windows":snapshot()}),
    );
    assert_eq!(absent["error"], "app.notFound");
    assert_eq!(absent["effect"], Value::Null);
}
#[test]
fn isolated_success_suppresses_only_its_windows_not_other_linux_clients() {
    let mut app = App::default();
    assert_eq!(
        request(
            &mut app,
            "quit",
            json!({"key":"editor","windows":snapshot()})
        )["effect"]["surface"],
        1
    );
    assert_eq!(
        request(&mut app, "next", json!({"result":""}))["effect"]["surface"],
        3
    );
    let done = request(&mut app, "next", json!({"result":""}));
    assert_eq!(done["effect"], Value::Null);
    assert_eq!(done["error"], "");
}
#[test]
fn failed_isolated_kill_retries_and_keeps_first_error() {
    let mut app = App::default();
    request(
        &mut app,
        "quit",
        json!({"key":"editor","windows":snapshot()}),
    );
    let retry = request(&mut app, "next", json!({"result":"app.forceQuitRefused"}));
    assert_eq!(retry["effect"]["surface"], 2);
    assert_eq!(retry["error"], "app.forceQuitRefused");
    let next = request(&mut app, "next", json!({"result":""}));
    assert_eq!(next["effect"]["surface"], 3);
    assert_eq!(
        request(&mut app, "next", json!({"result":"other.error"}))["error"],
        "app.forceQuitRefused"
    );
}
#[test]
fn x11_always_requests_close_and_returns_the_explanation() {
    let mut app = App::default();
    assert_eq!(
        request(
            &mut app,
            "quit",
            json!({"key":"xterm","windows":snapshot()})
        )["effect"],
        json!({"kind":"closeSurface","surface":4})
    );
    let result = request(&mut app, "next", json!({"result":""}));
    assert_eq!(result["effect"], Value::Null);
    assert_eq!(result["error"], "app.forceQuitX11");
}
#[test]
fn malformed_surfaces_never_produce_a_process_kill() {
    let windows = json!([
        {"id":"linux#1","key":"a","name":"A","surface":-1},
        {"id":"linux#2","key":"a","name":"A","surface":2147483648_u64},
        {"id":"linux#3","key":"a","name":"A","surface":"1"},
        {"id":"builtin","key":"a","name":"A","surface":1}
    ]);
    let mut app = App::default();
    let mut result = request(&mut app, "quit", json!({"key":"a","windows":windows}));
    for _ in 0..4 {
        assert_eq!(result["effect"]["kind"], "closeWindow");
        result = request(&mut app, "next", json!({"result":""}));
    }
    assert_eq!(result["effect"], Value::Null);
}
#[test]
fn native_view_owns_selection_labels_disabled_action_and_icon() {
    let mut app = App::default();
    let empty = request(&mut app, "sync", json!({"windows":[]}))["tree"].to_string();
    assert!(empty.contains("forcequit.none"));
    assert!(empty.contains("\"disabled\":true"));
    request(&mut app, "sync", json!({"windows":snapshot()}));
    request(&mut app, "select", json!({"key":"notes"}));
    let selected = request(&mut app, "sync", json!({"windows":snapshot()}))["tree"].to_string();
    assert!(selected.contains("forcequit.windows"));
    assert!(selected.contains("\"disabled\":false"));
    let icon = request(&mut app, "icon", json!({"size":64}));
    assert_eq!(icon["style"]["width"], 64.);
    let icon = request(&mut app, "icon", json!({"size":-50}));
    assert_eq!(icon["style"]["width"], 1.);
}

#[test]
fn native_window_sends_a_group_key_and_displays_the_session_controllers_result() {
    use crate::native_view::NativeApp;
    let mut app = App::default();
    NativeApp::request(&mut app, &json!({"cmd":"sync","windows":snapshot()}));
    NativeApp::request(&mut app, &json!({"cmd":"select","key":"editor"}));
    let reply = NativeApp::request(&mut app, &json!({"cmd":"quit"}));
    assert_eq!(reply["effect"], json!({"kind":"forceQuit","key":"editor"}));
    assert!(app.selected.is_none());
    assert!(app.queue.is_empty());
    assert!(app.pending.is_none());
    NativeApp::request(
        &mut app,
        &json!({"cmd":"next","result":"app.forceQuitRefused"}),
    );
    assert_eq!(app.error, "app.forceQuitRefused");
    NativeApp::request(&mut app, &json!({"cmd":"next","result":""}));
    assert!(app.error.is_empty());
    assert!(NativeApp::request(&mut app, &json!({"cmd":"quit"})).is_null());
}
