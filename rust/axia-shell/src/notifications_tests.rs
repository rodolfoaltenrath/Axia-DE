use super::*;

fn push(service: &mut Service, now: Instant, title: &str, extra: Value) -> Value {
    let mut input = json!({"action":"push","notice":{"app":"notes","title":title,"label":"Notes","sound":true},
        "prefs":{},"appPrefs":{},"wallMs":10000.,"minute":60.});
    for (key, value) in extra.as_object().into_iter().flatten() {
        input[key] = value.clone();
    }
    service.command(&input, now)
}
fn kinds(reply: &Value) -> Vec<&str> {
    reply["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["kind"].as_str())
        .collect()
}

#[test]
fn permission_firewall_dnd_and_history_have_distinct_delivery_effects() {
    let now = Instant::now();
    let mut service = Service::default();
    for extra in [json!({"appPrefs":{"allow":false}}), json!({"blocked":true})] {
        let reply = push(&mut service, now, "Denied", extra);
        assert_eq!(reply["result"], 0);
        assert!(kinds(&reply).contains(&"settings"));
        assert!(!kinds(&reply).contains(&"notificationSound"));
        assert!(service.reader().read().history.is_empty());
    }
    for extra in [
        json!({"prefs":{"dnd":true}}),
        json!({"appPrefs":{"style":"none"}}),
    ] {
        let reply = push(&mut service, now, "History", extra);
        assert!(reply["result"].as_u64().unwrap() > 0);
        assert!(!kinds(&reply).contains(&"notificationSound"));
        assert!(service.reader().read().shown.is_empty());
    }
    assert_eq!(service.reader().read().history.len(), 2);
    assert!(service.wait_ms(now).is_none());
    let reply = push(
        &mut service,
        now,
        "Delivered",
        json!({"prefs":{"seen":["notes"]}}),
    );
    assert!(kinds(&reply).contains(&"notificationSound"));
    assert!(!kinds(&reply).contains(&"settings"));
}

#[test]
fn schedules_wrap_midnight_and_wall_time_does_not_drive_elapsed_expiry() {
    let prefs = json!({"schedule":{"on":true,"from":1320,"to":420}});
    for minute in [1320., 1439., 0., 419.] {
        assert!(dnd(&prefs, 0., minute));
    }
    for minute in [420., 1319.] {
        assert!(!dnd(&prefs, 0., minute));
    }
    assert!(!dnd(
        &json!({"schedule":{"on":true,"from":60,"to":60}}),
        0.,
        60.
    ));
    assert!(dnd(&json!({"dnd":1000}), 999., 0.));
    assert!(!dnd(&json!({"dnd":1000}), 1000., 0.));
    let now = Instant::now();
    let mut service = Service::default();
    push(
        &mut service,
        now,
        "Timed",
        json!({"notice":{"title":"Timed","timeout":1}}),
    );
    assert!(service.wait_ms(now).unwrap() >= 1500.);
    service.expire(now + Duration::from_millis(1499));
    assert_eq!(service.reader().read().shown.len(), 1);
    service.expire(now + Duration::from_millis(1500));
    assert!(service.reader().read().shown.is_empty());
    let effects = service.effects();
    assert_eq!(
        effects[1],
        json!({"kind":"notificationClose","id":1,"reason":"closed"})
    );
    assert!(service.wait_ms(now).is_none());
    assert!(service.effects().is_empty());
}

#[test]
fn grouping_limits_eviction_sticky_and_clear_history_preserve_live_banners() {
    let now = Instant::now();
    let mut service = Service::default();
    for i in 0..55 {
        push(
            &mut service,
            now,
            &i.to_string(),
            json!({"appPrefs":{"style":"alert"}}),
        );
    }
    let reader = service.reader();
    let state = reader.read();
    assert_eq!(state.shown.len(), 4);
    assert_eq!(state.history.len(), 50);
    assert_eq!(state.grouped().len(), 1);
    assert_eq!(state.grouped()[0].1, 3);
    drop(state);
    assert!(service.wait_ms(now).is_none());
    service.configure(true);
    assert_eq!(reader.read().grouped().len(), 4);
    service.command(&json!({"action":"placement","placement":"bottom"}), now);
    assert_eq!(reader.read().grouped()[0].0.id(), 52);
    service.command(&json!({"action":"clearHistory"}), now);
    assert!(reader.read().history.is_empty());
    assert_eq!(reader.read().shown.len(), 4);
    service.expire(now + Duration::from_secs(100));
    assert_eq!(reader.read().shown.len(), 4);
    service.command(&json!({"action":"dismiss","id":55}), now);
    assert_eq!(reader.read().shown.len(), 3);
}

#[test]
fn action_revalidates_live_ids_and_orders_close_before_callback() {
    let now = Instant::now();
    let mut service = Service::default();
    push(
        &mut service,
        now,
        "Actions",
        json!({"notice":{"title":"Actions","sticky":true,
        "actions":[{"id":"first","label":"First"},{"id":"second","label":"Second"},{"id":"third"},{"id":"fourth"}]}}),
    );
    assert_eq!(
        service.reader().read().shown[0].data["actions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let reply = service.command(&json!({"action":"act","id":1,"index":1}), now);
    assert_eq!(
        kinds(&reply),
        [
            "notificationState",
            "notificationClose",
            "notificationAction"
        ]
    );
    assert_eq!(reply["effects"][1]["reason"], "second");
    assert!(
        service.command(&json!({"action":"act","id":1,"index":1}), now)["effects"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let reply = service.command(
        &json!({"action":"act","id":1,"index":1,"history":true}),
        now,
    );
    assert_eq!(kinds(&reply), ["notificationAction"]);
    for input in [
        json!({"action":"act","id":1,"index":99,"history":true}),
        json!({"action":"dismiss","id":999}),
    ] {
        assert!(service.command(&input, now)["effects"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn observable_revisions_are_quiet_when_idle_and_sessions_do_not_share_notifications() {
    let now = Instant::now();
    let a = crate::winmodel::Session::default();
    let clone = a.clone();
    let b = crate::winmodel::Session::default();
    let service = a.notification_service();
    let reader = service.borrow().reader();
    let changes = reader.subscribe();
    let reply = push(&mut service.borrow_mut(), now, "One", json!({}));
    assert_eq!(reader.revision(), 1);
    assert!(changes.pending());
    assert_eq!(
        clone
            .notification_service()
            .borrow()
            .reader()
            .read()
            .shown
            .len(),
        1
    );
    assert!(b
        .notification_service()
        .borrow()
        .reader()
        .read()
        .shown
        .is_empty());
    changes.take();
    service.borrow_mut().expire(now);
    assert!(!changes.pending());
    assert!(!reply["effects"].as_array().unwrap().is_empty());
    assert_eq!(
        crate::app_control::Controller::default()
            .request(&json!({"cmd":"notifications","action":"snapshot"}))["error"],
        "app.invalidRequest"
    );
}
