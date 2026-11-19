use crate::native_apps_host::*;
use serde_json::Value;
use std::ffi::CStr;

#[test]
fn forcequit_borrows_membership_and_revalidates_removals_before_actions() {
    use super::Host;
    use crate::runtime::native as scene;
    use serde_json::json;
    let mut session = crate::winmodel::Session::default();
    session.call("open", &[json!({}), json!({"app":"notes","instance":"a"})]).unwrap();
    let reader = session.reader();
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(mount, Box::new(crate::forcequit::App::with_windows(
        std::rc::Rc::new(crate::winmodel::WindowReader(reader)))), "", None);
    host.facts(json!({"registry":[{"id":"notes","name":"Notes"}],"catalog":[],
        "windows":[{"id":"stale","key":"stale","name":"Stale"}]}));
    assert_ne!(host.find_text("Notes"), 0);
    assert_eq!(host.find_text("Stale"), 0, "bound views ignore legacy window facts");
    assert!(host.deadline().is_none());
    host.action(&json!({"action":["select","notes"]}));
    session.call("close", &[json!({}), json!("notes#a")]).unwrap();
    assert!(host.deadline().is_some());
    // No tick or new JS facts between the model mutation and this action.
    host.action(&json!({"action":["quit"]}));
    assert!(host.take().is_none(), "a removed selection cannot launch a quit effect");
    assert_eq!(host.find_text("Notes"), 0);
    assert!(host.deadline().is_none());
    drop(session);
    drop(host);
    scene::destroy(mount);
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
