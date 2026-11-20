use super::*;

#[test]
fn shared_catalog_decides_access_and_live_changes_cancel_ringing() {
    let session = crate::winmodel::Session::default();
    let mut control = crate::app_control::Controller::new(session.clone());
    let registry = |clock_package: bool| {
        json!({"cmd":"metadata","catalog":[],"registry":[
        {"id":"package-probe","package":true}, {"id":"isolated-probe","isolated":true},
        {"id":"clock","package":clock_package}]})
    };
    assert_eq!(control.request(&registry(false))["ok"], true);
    access::host::set_permission("package-probe", 2);
    access::host::set_permission("clock", 2);
    access::host::set_blocked("lpk:isolated-probe", true);
    let now = Instant::now();
    let service = session.notification_service();
    let mut service = service.borrow_mut();
    let query = service.command(
        &json!({"action":"appPreferences","app":"package-probe","source":{"package":false}}),
        now,
    );
    assert_eq!(query["result"]["allow"], false);
    for app in ["package-probe", "isolated-probe"] {
        assert_eq!(
            service.command(
                &json!({"action":"push","source":{},"notice":{"app":app}}),
                now
            )["result"],
            0
        );
    }
    service.command(&json!({"action":"setAppPreferences","app":"package-probe","patch":{"allow":true},"source":{"package":false}}), now);
    assert_eq!(
        service.command(
            &json!({"action":"appPreferences","app":"package-probe"}),
            now
        )["result"]["allow"],
        true
    );
    let ring = service.command(&json!({"action":"push","source":{"package":true},"notice":{"app":"clock","ring":true,"sound":true}}), now);
    let id = ring["result"].as_u64().unwrap();
    let reader = service.reader();
    assert!(reader
        .read()
        .shown
        .iter()
        .any(|n| n.id() == id && n.ring.is_some()));
    drop(service);
    assert_eq!(control.request(&registry(true))["ok"], true);
    let service = session.notification_service();
    let mut service = service.borrow_mut();
    assert!(service.catalog_pending());
    service.expire(now);
    assert!(!service.catalog_pending());
    assert!(reader
        .read()
        .shown
        .iter()
        .any(|n| n.id() == id && n.ring.is_none()));
    let changes = reader.subscribe();
    control.request(&registry(true));
    assert!(!service.catalog_pending());
    assert!(!changes.pending());
}
