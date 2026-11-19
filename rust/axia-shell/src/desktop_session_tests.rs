use super::*;
use axia_native::NativeApp;

#[test]
fn nonvisual_observer_keeps_pending_count_across_remount_without_an_idle_timer() {
    let mut session = Session::default();
    command(
        &mut session,
        json!({"action":"start","settings":{"desktopCount":2}}),
    );
    let shared = session.desktop_service();
    let mut observer = Observer::new(shared.clone());
    assert!(!observer.request(&json!({"cmd":"sync"}))["tree"].is_null());
    assert!(observer.refresh_ms().is_none());
    assert!(observer.background_refresh_ms().is_none());
    call(&mut session, "createDesktop", &[]);
    assert!(observer.invalidated());
    drop(observer);
    let mut observer = Observer::new(shared);
    assert_eq!(
        observer.request(&json!({"cmd":"sync"}))["effects"][0]["patch"]["desktopCount"],
        3
    );
    assert!(!observer.invalidated());
    assert_eq!(
        observer.request(&json!({"cmd":"sync"})),
        json!({"same":true,"effects":[]})
    );
    drop(observer);
    let weak = std::rc::Rc::downgrade(&session.desktop_service());
    drop(session);
    assert!(
        weak.upgrade().is_none(),
        "the service must not retain its owning Session"
    );
}

fn command(session: &mut Session, input: Value) -> Value {
    session
        .desktop_service()
        .borrow_mut()
        .command(session, &input)
}
fn call(session: &mut Session, op: &str, args: &[Value]) {
    let mut all = vec![json!({})];
    all.extend_from_slice(args);
    session.call(op, &all).unwrap();
}

#[test]
fn start_is_atomic_and_shared_across_remounts_without_reinitializing_windows() {
    let mut session = Session::default();
    let reader = session.reader();
    let revision = reader.revision();
    let reply = command(
        &mut session,
        json!({"action":"start","settings":{"desktopCount":4,"themeDesktops":{"active":2}}}),
    );
    assert_eq!(reader.read().desktops(), &[1, 2, 3, 4]);
    assert_eq!(reader.read().active_desktop(), 3);
    assert_eq!(reader.revision(), revision + 1);
    assert_eq!(
        reply["effects"],
        json!([{"kind":"settings","patch":{"themeDesktops":null}}])
    );
    let revision = reader.revision();
    let clone = session.clone();
    assert!(std::rc::Rc::ptr_eq(
        &session.desktop_service(),
        &clone.desktop_service()
    ));
    call(
        &mut session,
        "open",
        &[json!({"app":"notes","instance":"first"})],
    );
    let before = reader.revision();
    command(
        &mut session,
        json!({"action":"start","settings":{"desktopCount":1}}),
    );
    assert_eq!(reader.revision(), before);
    assert!(before > revision);
    assert_eq!(reader.read().desktops(), &[1, 2, 3, 4]);
    assert_eq!(reader.read().windows().count(), 1);
    assert!(!std::rc::Rc::ptr_eq(
        &session.desktop_service(),
        &Session::default().desktop_service()
    ));
}

#[test]
fn saved_counts_and_theme_positions_are_validated_without_inventing_desktops() {
    for count in [
        json!(0),
        json!(10),
        json!(2.5),
        json!("3"),
        Value::Null,
        json!(true),
    ] {
        let mut session = Session::default();
        command(
            &mut session,
            json!({"action":"start","settings":{"desktopCount":count,"themeDesktops":{"active":8}}}),
        );
        assert_eq!(session.reader().read().desktops(), &[1]);
        assert_eq!(session.reader().read().active_desktop(), 1);
    }
    let mut session = Session::default();
    command(
        &mut session,
        json!({"action":"start","settings":{"desktopCount":9,"themeDesktops":{"active":7}}}),
    );
    assert_eq!(session.reader().read().active_desktop(), 8);
}

#[test]
fn count_effects_follow_native_changes_once_and_do_not_require_a_js_subscriber() {
    let mut session = Session::default();
    let service = session.desktop_service();
    assert!(service.borrow_mut().effects().is_empty());
    command(
        &mut session,
        json!({"action":"start","settings":{"desktopCount":2}}),
    );
    assert!(service.borrow_mut().effects().is_empty());
    call(&mut session, "createDesktop", &[]);
    call(&mut session, "createDesktop", &[]);
    assert!(service.borrow().invalidated());
    assert_eq!(
        service.borrow_mut().effects(),
        vec![json!({"kind":"settings","patch":{"desktopCount":4}})]
    );
    assert!(!service.borrow().invalidated());
    assert!(service.borrow_mut().effects().is_empty());
    call(&mut session, "setActiveDesktop", &[json!(2)]);
    assert!(service.borrow_mut().effects().is_empty());
    call(&mut session, "closeDesktop", &[json!(2)]);
    assert_eq!(
        service.borrow_mut().effects()[0]["patch"]["desktopCount"],
        3
    );
}

#[test]
fn positional_queries_and_preparation_use_current_native_state_in_one_revision() {
    let mut session = Session::default();
    command(
        &mut session,
        json!({"action":"start","settings":{"desktopCount":2}}),
    );
    let reader = session.reader();
    let revision = reader.revision();
    let reply = command(&mut session, json!({"action":"at","index":8}));
    assert_eq!(reply["result"], 9);
    assert_eq!(reply["effects"][0]["patch"]["desktopCount"], 9);
    assert_eq!(reader.revision(), revision + 1);
    assert_eq!(reader.read().active_desktop(), 1);
    call(
        &mut session,
        "setDesktops",
        &[json!([10, 30, 60]), json!(60)],
    );
    assert_eq!(
        command(&mut session, json!({"action":"at","index":1}))["result"],
        30
    );
    assert_eq!(
        command(&mut session, json!({"action":"index","id":30}))["result"],
        1
    );
    assert_eq!(
        command(&mut session, json!({"action":"index","id":999}))["result"],
        0
    );
    assert_eq!(
        command(&mut session, json!({"action":"active"}))["result"],
        2
    );
    let revision = reader.revision();
    for index in [json!(-1), json!(10), json!(1.5), json!("2"), Value::Null] {
        assert_eq!(
            command(&mut session, json!({"action":"at","index":index}))["result"],
            10
        );
    }
    assert_eq!(reader.revision(), revision);
}

#[test]
fn surface_hand_off_uses_native_membership_and_validates_the_saved_mapping() {
    let mut session = Session::default();
    command(
        &mut session,
        json!({"action":"start","settings":{"desktopCount":2,"themeDesktops":{
        "active":1,"surfaces":{"71":4,"72":1,"73":-1,"74":"2","75":9,"__proto__":2,"-9":1}}}}),
    );
    assert_eq!(
        command(&mut session, json!({"action":"surface","surface":72}))["result"],
        2
    );
    assert_eq!(
        command(&mut session, json!({"action":"surface","surface":71}))["result"],
        5
    );
    assert_eq!(session.reader().read().active_desktop(), 2);
    for id in [
        json!(73),
        json!(74),
        json!(75),
        json!(-9),
        json!("71"),
        json!(71.5),
    ] {
        assert!(
            command(&mut session, json!({"action":"surface","surface":id}))["result"].is_null()
        );
    }
    call(
        &mut session,
        "open",
        &[json!({"app":"linux","instance":"71","desktop":5,"args":{"surface":71}})],
    );
    call(
        &mut session,
        "open",
        &[json!({"app":"linux","instance":"72","desktop":2,"args":{"surface":72}})],
    );
    call(
        &mut session,
        "open",
        &[json!({"app":"notes","instance":"private","desktop":1})],
    );
    let revision = session.reader().revision();
    let reply = command(&mut session, json!({"action":"theme"}));
    assert_eq!(
        reply["result"],
        json!({"active":1,"surfaces":{"71":4,"72":1}})
    );
    assert_eq!(session.reader().revision(), revision);
    assert!(reply["state"].is_null());
}

#[test]
fn controller_requires_the_bound_context_and_rejects_unknown_actions() {
    let mut control = crate::app_control::Controller::default();
    assert_eq!(
        control.request(&json!({"cmd":"desktopSession","action":"start"}))["error"],
        "app.invalidRequest"
    );
    let mut session = Session::default();
    assert_eq!(
        command(&mut session, json!({"action":"unknown"}))["error"],
        "app.invalidRequest"
    );
    assert!(!session.desktop_service().borrow().started);
}
