use super::*;

#[test]
fn desktop_swipes_choose_native_desktops_or_visible_windows_atomically() {
    let mut session = Session::default();
    let mut control = Controller::new(session.clone());
    let reader = session.reader();
    let swipe = |control: &mut Controller, direction: &str, rtl: bool| {
        control.request(
        &json!({"cmd":"desktopCommand","action":"swipe","direction":direction,"rtl":rtl,"desktops":[999],"active":999})
    )
    };
    session
        .call("setDesktops", &[json!({}), json!([10, 20, 30]), json!(20)])
        .unwrap();
    let revision = reader.revision();
    assert_eq!(
        swipe(&mut control, "left", false)["result"]["boundary"],
        false
    );
    assert_eq!(reader.read().active_desktop(), 30);
    assert_eq!(reader.revision(), revision + 1);
    assert_eq!(
        swipe(&mut control, "left", false)["result"]["boundary"],
        true
    );
    assert_eq!(reader.revision(), revision + 1);
    swipe(&mut control, "left", true);
    assert_eq!(reader.read().active_desktop(), 20);
    swipe(&mut control, "right", true);
    assert_eq!(reader.read().active_desktop(), 30);
    swipe(&mut control, "right", false);
    assert_eq!(reader.read().active_desktop(), 20);
    session
        .call("setDesktops", &[json!({}), json!([10]), json!(10)])
        .unwrap();
    for app in ["first", "second", "hidden", "fullscreen", "pip"] {
        session
            .call("open", &[json!({}), json!({"app":app})])
            .unwrap();
    }
    session
        .call("minimize", &[json!({}), json!("hidden")])
        .unwrap();
    // Exercise the same filtering used by the established cycle API.
    session
        .call(
            "setFullscreen",
            &[json!({}), json!("fullscreen"), json!(true)],
        )
        .unwrap();
    session
        .call(
            "setPip",
            &[
                json!({}),
                json!("pip"),
                json!({"x":1,"y":1,"w":200,"h":120}),
            ],
        )
        .unwrap();
    let revision = reader.revision();
    swipe(&mut control, "left", false);
    assert_eq!(
        reader.read().visible_windows().last().unwrap().id(),
        "first"
    );
    assert_eq!(reader.revision(), revision + 1);
    swipe(&mut control, "right", true);
    assert_eq!(
        reader.read().visible_windows().last().unwrap().id(),
        "second"
    );
    let revision = reader.revision();
    for input in [
        json!({"direction":"up","rtl":false}),
        json!({"direction":"left"}),
        json!({"direction":"right","rtl":"true"}),
    ] {
        assert_eq!(control.request(&json!({"cmd":"desktopCommand","action":"swipe","direction":input["direction"],"rtl":input["rtl"]}))["error"], "app.invalidRequest");
    }
    assert_eq!(reader.revision(), revision);
}

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
