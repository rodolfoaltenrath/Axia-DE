use super::*;
use crate::native_apps_host::*;
use std::ffi::CStr;
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
fn native_handle_rejects_unknown_ids_and_invalid_json() {
    unsafe {
        assert!(axia_native_app_new(c"unknown".as_ptr()).is_null());
        let app = axia_native_app_new(c"forcequit".as_ptr());
        assert!(!app.is_null());
        let reply = axia_native_app_request(app, c"{invalid".as_ptr());
        let value: Value = serde_json::from_slice(CStr::from_ptr(reply).to_bytes()).unwrap();
        assert_eq!(value["error"], "app.invalidRequest");
        axia_native_app_reply_free(reply);
        axia_native_app_free(app);
        axia_native_app_free(std::ptr::null_mut());
    }
}
