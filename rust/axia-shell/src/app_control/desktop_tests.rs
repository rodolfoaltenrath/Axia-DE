use super::*;

#[test]
fn desktop_commands_resolve_current_state_and_publish_one_revision() {
    let mut session = Session::default();
    session
        .call("setDesktops", &[json!({}), json!([10, 20, 30]), json!(20)])
        .unwrap();
    session
        .call("open", &[json!({}), json!({"app":"notes"})])
        .unwrap();
    let reader = session.reader();
    let mut control = Controller::new(session.clone());
    let mut revision = reader.revision();
    let reply = control.request(
        &json!({"cmd":"desktopCommand","action":"step","step":1,"active":10,"desktops":[1,2]}),
    );
    assert_eq!(reply["result"]["boundary"], false);
    assert_eq!(reader.read().active_desktop(), 30);
    assert_eq!(reader.revision(), revision + 1);
    revision = reader.revision();
    for step in [1, i64::MAX] {
        let reply = control.request(&json!({"cmd":"desktopCommand","action":"step","step":step}));
        assert_eq!(reply["result"]["boundary"], true);
        assert_eq!(reader.revision(), revision);
    }
    session
        .call("setActiveDesktop", &[json!({}), json!(20)])
        .unwrap();
    revision = reader.revision();
    let reply = control.request(&json!({"cmd":"desktopCommand","action":"closeActive","id":30}));
    assert_eq!(reply["result"], true);
    assert_eq!(reader.revision(), revision + 1);
    assert_eq!(reader.read().active_desktop(), 30);
    assert_eq!(reader.read().window("notes").unwrap().desktop(), 30);
    assert_eq!(reader.read().desktops(), &[10, 30]);
    revision = reader.revision();
    let reply = control.request(&json!({"cmd":"desktopCommand","action":"add"}));
    assert_eq!(reply["result"], 31);
    assert_eq!(reader.read().active_desktop(), 31);
    assert_eq!(reader.revision(), revision + 1);
    for _ in 0..6 {
        control.request(&json!({"cmd":"desktopCommand","action":"add"}));
    }
    revision = reader.revision();
    assert!(control.request(&json!({"cmd":"desktopCommand","action":"add"}))["result"].is_null());
    assert_eq!(reader.revision(), revision);
    for input in [
        json!({"action":"unknown"}),
        json!({"action":"step","step":1.5}),
    ] {
        assert_eq!(
            control.request(
                &json!({"cmd":"desktopCommand","action":input["action"],"step":input["step"]})
            )["error"],
            "app.invalidRequest"
        );
        assert_eq!(reader.revision(), revision);
    }
    assert_eq!(
        Controller::default().request(&json!({"cmd":"desktopCommand","action":"add"}))["error"],
        "app.invalidRequest"
    );
}
