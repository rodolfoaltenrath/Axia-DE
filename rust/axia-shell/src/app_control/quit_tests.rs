use super::*;
use serde_json::json;

fn call(session: &mut Session, op: &str, args: Vec<Value>) {
    session.call(op, &[vec![json!({})], args].concat()).unwrap();
}
fn controller(session: &Session) -> Controller {
    let mut control = Controller::new(session.clone());
    control.request(
        &json!({"cmd":"metadata","registry":[{"id":"notes","name":"Notes"},
        {"id":"panel","builtin":true},{"id":"pkg","name":"Package","isolated":true}],"catalog":[]}),
    );
    control
}
#[test]
fn menu_group_reads_native_membership_and_returns_no_windows() {
    let mut session = Session::default();
    call(
        &mut session,
        "open",
        vec![json!({"app":"notes","instance":"a"})],
    );
    call(
        &mut session,
        "open",
        vec![json!({"app":"notes","instance":"b"})],
    );
    let mut control = controller(&session);
    let reader = session.reader();
    let revision = reader.revision();
    assert_eq!(
        control.request(&json!({"cmd":"groupInfo","id":"notes#a","windows":[]})),
        json!({"group":{"key":"notes","name":"Notes"}})
    );
    assert_eq!(reader.revision(), revision);
    assert_eq!(
        control.request(&json!({"cmd":"group","id":"notes#b"}))["group"]["windows"],
        json!(["notes#a", "notes#b"])
    );
    call(&mut session, "close", vec![json!("notes#a")]);
    assert_eq!(
        control.request(&json!({"cmd":"groupInfo","id":"notes#a"}))["group"],
        Value::Null
    );
    assert_eq!(
        control.request(&json!({"cmd":"groupInfo"}))["group"]["key"],
        "notes"
    );
    call(&mut session, "open", vec![json!({"app":"panel"})]);
    assert_eq!(
        control.request(&json!({"cmd":"groupInfo"}))["group"],
        Value::Null
    );
    assert!(Controller::default().request(&json!({"cmd":"groupInfo"}))["error"].is_string());
}
#[test]
fn graceful_quit_uses_guards_and_live_surface_handles_and_skips_removed_members() {
    let mut session = Session::default();
    for (id, surface) in [(1, 11), (2, 22), (3, 33)] {
        call(
            &mut session,
            "open",
            vec![json!({"app":"linux","instance":id,"args":{"appId":"native","surface":surface}})],
        );
    }
    let mut control = controller(&session);
    let reply = control.request(&json!({"cmd":"quitApp","key":"linux:native"}));
    let ticket = reply["ticket"].clone();
    assert_eq!(reply["effect"], json!({"kind":"closeSurface","surface":11}));
    call(
        &mut session,
        "setArgs",
        vec![json!("linux#2"), json!({"surface":222})],
    );
    call(&mut session, "close", vec![json!("linux#3")]);
    let reply = control.request(&json!({"cmd":"quitAppNext","ticket":ticket,"result":""}));
    assert_eq!(
        reply["effect"],
        json!({"kind":"closeSurface","surface":222})
    );
    assert!(
        control.request(&json!({"cmd":"quitAppNext","ticket":ticket,"result":""}))["effect"]
            .is_null()
    );
    call(
        &mut session,
        "open",
        vec![json!({"app":"notes","instance":"a"})],
    );
    call(
        &mut session,
        "open",
        vec![json!({"app":"notes","instance":"b"})],
    );
    let reply = control.request(&json!({"cmd":"quitApp"}));
    assert_eq!(
        reply["effect"],
        json!({"kind":"requestCloseWindow","id":"notes#a"})
    );
    // A false close-guard result keeps that window and continues with its sibling.
    let reply =
        control.request(&json!({"cmd":"quitAppNext","ticket":reply["ticket"],"result":false}));
    assert_eq!(
        reply["effect"],
        json!({"kind":"requestCloseWindow","id":"notes#b"})
    );
    assert!(session.reader().read().window("notes#a").is_some());
}
#[test]
fn package_quit_acknowledgement_avoids_fallback_and_tickets_cancel_old_work() {
    let mut session = Session::default();
    for id in [1, 2] {
        call(
            &mut session,
            "open",
            vec![
                json!({"app":"linux","instance":id,"args":{"appId":"pkg","pkg":true,"surface":id}}),
            ],
        );
    }
    let mut control = controller(&session);
    let reply = control.request(&json!({"cmd":"quitApp","key":"pkg"}));
    assert_eq!(reply["effect"], json!({"kind":"quitPackage","app":"pkg"}));
    let old_ticket = reply["ticket"].clone();
    assert!(
        control.request(&json!({"cmd":"quitAppNext","ticket":old_ticket,"result":true}))["effect"]
            .is_null()
    );
    let reply = control.request(&json!({"cmd":"quitApp","id":"linux#1"}));
    let ticket = reply["ticket"].clone();
    assert_ne!(ticket, old_ticket);
    assert!(
        control.request(&json!({"cmd":"quitAppNext","ticket":old_ticket,"result":false}))["effect"]
            .is_null()
    );
    assert_eq!(
        control.request(&json!({"cmd":"quitAppNext","ticket":ticket,"result":false}))["effect"],
        json!({"kind":"closeSurface","surface":1})
    );
    // Changing identity between effects must not close the new owner of a queued window.
    call(
        &mut session,
        "setArgs",
        vec![json!("linux#2"), json!({"appId":"other","pkg":false})],
    );
    assert!(
        control.request(&json!({"cmd":"quitAppNext","ticket":ticket,"result":""}))["effect"]
            .is_null()
    );
    let mut other = controller(&Session::default());
    assert!(other.request(&json!({"cmd":"quitApp","key":"pkg"}))["effect"].is_null());
    assert!(Controller::default().request(&json!({"cmd":"quitApp"}))["error"].is_string());
}
