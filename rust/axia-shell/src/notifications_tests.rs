use super::*;

fn ring(service: &mut Service, now: Instant, extra: Value) -> Value {
    let mut input = json!({"notice":{"app":"clock","title":"Alarm","ring":true,"sound":true}});
    for (key, value) in extra.as_object().into_iter().flatten() {
        input[key] = value.clone();
    }
    push(service, now, "Alarm", input)
}

#[test]
fn ringing_has_one_monotonic_deadline_no_catch_up_burst_and_no_revisions_per_pulse() {
    let now = Instant::now();
    let mut service = Service::default();
    let reply = ring(&mut service, now, json!({}));
    assert!(kinds(&reply).contains(&"notificationSound"));
    let reader = service.reader();
    let revision = reader.revision();
    assert!(service.wait_ms(now).unwrap() >= 2500.);
    assert_eq!(reader.read().snapshot()["ringing"], json!([1]));
    service.expire_at(now + Duration::from_millis(2499), 0., 0.);
    assert!(service.effects().is_empty());
    service.expire_at(now + RING_STEP, 0., 0.);
    assert_eq!(
        service.effects(),
        vec![json!({"kind":"notificationSound","id":1,"repeat":true})]
    );
    assert_eq!(reader.revision(), revision);
    service.expire_at(now + Duration::from_secs(30), 0., 0.);
    assert_eq!(service.effects().len(), 1);
    service.expire_at(now + RING_FOR, 0., 0.);
    let effects = service.effects();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0]["state"]["ringing"], json!([]));
    assert_eq!(
        reader.read().shown.len(),
        1,
        "alarm stays available for acknowledgement"
    );
    assert!(service.wait_ms(now + RING_FOR).is_none());
    service.expire_at(now + Duration::from_secs(120), 0., 0.);
    assert!(service.effects().is_empty());
}

#[test]
fn suppressed_rings_never_arm_and_other_apps_cannot_request_clock_repetition() {
    let now = Instant::now();
    for extra in [
        json!({"appPrefs":{"allow":false}}),
        json!({"blocked":true}),
        json!({"prefs":{"dnd":true}}),
        json!({"appPrefs":{"sound":false}}),
        json!({"appPrefs":{"style":"none"}}),
    ] {
        let mut service = Service::default();
        let reply = ring(&mut service, now, extra);
        assert!(!kinds(&reply).contains(&"notificationSound"));
        assert_eq!(service.reader().read().snapshot()["ringing"], json!([]));
        assert!(service.wait_ms(now).is_none());
    }
    let mut service = Service::default();
    let reply = ring(
        &mut service,
        now,
        json!({"notice":{"app":"notes","ring":true,"sticky":true,"sound":true}}),
    );
    assert!(kinds(&reply).contains(&"notificationSound"));
    assert!(reply["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "notificationSound")
        .all(|e| e["repeat"] == false));
    assert_eq!(service.reader().read().snapshot()["ringing"], json!([]));
    assert!(service.wait_ms(now).is_none());
}

#[test]
fn muting_dnd_denial_and_firewall_cancel_native_ringing_without_resuming_old_alarms() {
    let now = Instant::now();
    for policy in [
        json!({"clock":{"sound":false}}),
        json!({"clock":{"allow":false}}),
        json!({"clock":{"style":"none"}}),
        json!({"prefs":{"dnd":true}}),
        json!({"blocked":true}),
    ] {
        let mut service = Service::default();
        ring(&mut service, now, json!({}));
        let mut input = json!({"action":"ringPolicy","prefs":{},"clock":{}});
        for (key, value) in policy.as_object().unwrap() {
            input[key] = value.clone();
        }
        let reply = service.command(&input, now + RING_STEP);
        assert!(!kinds(&reply).contains(&"notificationSound"));
        assert_eq!(service.reader().read().snapshot()["ringing"], json!([]));
        assert!(service.wait_ms(now).is_none());
        service.command(
            &json!({"action":"ringPolicy","prefs":{},"clock":{}}),
            now + Duration::from_secs(3),
        );
        service.expire_at(now + Duration::from_secs(10), 0., 0.);
        assert!(service.effects().is_empty());
    }
}

#[test]
fn dismissal_and_eviction_cancel_due_and_already_queued_pulses() {
    let now = Instant::now();
    let mut service = Service::default();
    ring(&mut service, now, json!({}));
    let reply = service.command(&json!({"action":"dismiss","id":1}), now + RING_STEP);
    assert_eq!(kinds(&reply), ["notificationState", "notificationClose"]);
    assert!(service.wait_ms(now).is_none());
    ring(&mut service, now, json!({}));
    service.expire_at(now + RING_STEP, 0., 0.);
    assert_eq!(service.effects()[0]["repeat"], true);
    service.command(&json!({"action":"dismiss","id":2}), now + RING_STEP);
    assert_eq!(
        service.command(&json!({"action":"ringAudible","id":2}), now + RING_STEP)["result"],
        false
    );
    ring(&mut service, now, json!({}));
    for _ in 0..4 {
        push(
            &mut service,
            now,
            "Other",
            json!({"notice":{"app":"notes","sticky":true,"sound":false}}),
        );
    }
    assert!(service.reader().read().snapshot()["ringing"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(service.wait_ms(now).is_none());
    service.expire_at(now + RING_STEP, 0., 0.);
    assert!(service.effects().is_empty());
}

#[test]
fn dnd_schedule_is_rechecked_by_the_native_pulse_at_midnight() {
    let now = Instant::now();
    let mut service = Service::default();
    ring(
        &mut service,
        now,
        json!({"prefs":{"schedule":{"on":true,"from":1320,"to":420}},"minute":1319.}),
    );
    service.expire_at(now + RING_STEP, 0., 1320.);
    assert_eq!(service.reader().read().snapshot()["ringing"], json!([]));
    assert!(!service
        .effects()
        .iter()
        .any(|e| e["kind"] == "notificationSound"));
    assert!(service.wait_ms(now).is_none());
}

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
    assert_eq!(reply["effects"][0]["revision"], 1);
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
