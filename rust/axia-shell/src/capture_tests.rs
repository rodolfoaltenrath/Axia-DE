use super::*;

struct Profile(String);
impl Drop for Profile {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn fixture(name: &str, timer: u64) -> (Profile, Session) {
    let root = std::env::temp_dir().join(format!("axia-capture-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let home = root.to_str().unwrap().to_owned();
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize","text":json!({"schema":3,"screenshots":{"timer":timer}}).to_string()}));
    });
    let session = Session::default();
    session.capture_service().borrow_mut().bind_settings(&home);
    session.activate_services();
    (Profile(home), session)
}
fn command(session: &Session, input: Value, now: Instant) -> Value {
    session
        .capture_service()
        .borrow_mut()
        .command(session, &input, 1000, now)
}
fn tick(session: &mut Session, now: Instant) -> Vec<Value> {
    session.service_tick(now);
    session
        .take_service_effects()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}
#[test]
fn resident_deadlines_run_without_views_or_javascript_timers() {
    let (_home, mut session) = fixture("resident", 5);
    let now = Instant::now();
    command(&session, json!({"action":"start","mode":"area"}), now);
    assert_eq!(crate::native_view::services::timeout(now), Some(160.));
    assert!(tick(&mut session, now + Duration::from_millis(159)).is_empty());
    let count = tick(&mut session, now + Duration::from_millis(160));
    assert_eq!(count[0]["op"], "countdown");
    assert_eq!(count[0]["secs"], 5);
    assert_eq!(
        crate::native_view::services::timeout(now + Duration::from_millis(160)),
        Some(5000.)
    );
    // No scene is mounted and no countdown callback is delivered.
    let closed = tick(&mut session, now + Duration::from_millis(5160));
    assert_eq!(closed[0]["op"], "closeCountdown");
    assert_eq!(
        session.service_wait(now + Duration::from_millis(5160)),
        Some(180.)
    );
    let work = tick(&mut session, now + Duration::from_millis(5340));
    assert_eq!(work[0]["op"], "work");
    assert_eq!(work[0]["mode"], "area");
    assert_eq!(work[0]["ticket"], count[0]["ticket"]);
    assert!(session
        .service_wait(now + Duration::from_millis(5340))
        .is_none());
    assert!(tick(&mut session, now + Duration::from_secs(20)).is_empty());
}
#[test]
fn superseded_work_and_old_countdown_cleanup_cannot_capture_or_cancel_a_new_attempt() {
    let (_home, mut session) = fixture("generations", 5);
    let now = Instant::now();
    let first =
        command(&session, json!({"action":"start","mode":"full"}), now)["result"]["ticket"].clone();
    tick(&mut session, now + Duration::from_millis(160));
    command(&session, json!({"action":"cancel","ticket":first}), now);
    let new = command(&session, json!({"action":"start","mode":"window"}), now)["result"]["ticket"]
        .clone();
    command(
        &session,
        json!({"action":"countClosed","ticket":first}),
        now,
    );
    command(&session, json!({"action":"cancel","ticket":first}), now);
    tick(&mut session, now + Duration::from_millis(160));
    tick(&mut session, now + Duration::from_millis(5160));
    // The normal close after expiry also cannot cancel work waiting to settle.
    command(&session, json!({"action":"countClosed","ticket":new}), now);
    let work = tick(&mut session, now + Duration::from_millis(5340));
    assert_eq!(work[0]["ticket"], new);
    assert_eq!(
        command(&session, json!({"action":"accept","ticket":first}), now)["result"]["valid"],
        false
    );
    assert_eq!(
        command(&session, json!({"action":"accept","ticket":new}), now)["result"]["valid"],
        true
    );
    assert_eq!(
        command(&session, json!({"action":"accept","ticket":new}), now)["result"]["valid"],
        false
    );
}
#[test]
fn native_window_policy_preserves_editors_and_a_running_recording() {
    let (_home, mut session) = fixture("windows", 0);
    let now = Instant::now();
    for (app, instance) in [
        ("screenshots", None),
        ("screenshots", Some("edit42")),
        ("screenshots", Some("thumb")),
        ("screenshots", Some("rec")),
        ("notes", None),
    ] {
        session
            .call("open", &[json!({}), json!({"app":app,"instance":instance})])
            .unwrap();
    }
    command(&session, json!({"action":"start","mode":"record"}), now);
    let work = tick(&mut session, now + Duration::from_millis(220));
    let token = work[0]["recordToken"].clone();
    assert_eq!(
        session.capture_service().borrow_mut().session(
            &json!({"op":"recordReady","token":token,"id":7,"path":"/video.mp4"}),
            1100
        )["accepted"],
        true
    );
    let next = command(
        &session,
        json!({"action":"start","mode":"full","windows":[{"id":"spoof"}]}),
        now,
    );
    let ids = next["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["op"] == "hide")
        .unwrap()["ids"]
        .clone();
    assert_eq!(
        ids,
        json!(["screenshots", "screenshots#thumb", "screenshots#rec"])
    );
    let old_ticket = next["result"]["ticket"].clone();
    session
        .call("close", &[json!({}), json!("screenshots#rec")])
        .unwrap();
    let replacement = command(&session, json!({"action":"start","mode":"area"}), now);
    let ticket = replacement["result"]["ticket"].clone();
    assert_eq!(
        command(
            &session,
            json!({"action":"restoreRecording","ticket":old_ticket,"id":7}),
            now
        )["result"]["valid"],
        false
    );
    tick(&mut session, now + Duration::from_millis(160));
    let accepted = command(&session, json!({"action":"accept","ticket":ticket}), now);
    assert!(accepted["effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["op"] == "restoreRecording" && e["id"] == 7));
    assert_eq!(
        command(
            &session,
            json!({"action":"restoreRecording","ticket":ticket,"id":7}),
            now
        )["result"]["valid"],
        true
    );
    let stopped = command(&session, json!({"action":"start","mode":"record"}), now);
    assert_eq!(stopped["effects"][0]["op"], "stop");
    assert_eq!(stopped["effects"][0]["id"], 7);
    assert_eq!(
        session
            .capture_service()
            .borrow_mut()
            .session(&json!({"op":"recordDone","id":7,"ok":true}), 1200)["state"]["recent"][0]
            ["path"],
        "/video.mp4"
    );
    assert_eq!(
        command(
            &session,
            json!({"action":"restoreRecording","ticket":ticket,"id":7}),
            now
        )["result"]["valid"],
        false
    );
}
#[test]
fn cancelled_record_permission_cannot_be_adopted_by_another_capture_or_context() {
    let (_home, mut session) = fixture("late", 0);
    let now = Instant::now();
    command(&session, json!({"action":"start","mode":"record"}), now);
    let work = tick(&mut session, now + Duration::from_millis(220));
    let token = work[0]["recordToken"].clone();
    command(&session, json!({"action":"start","mode":"area"}), now);
    let service = session.capture_service();
    assert_eq!(
        service
            .borrow_mut()
            .session(&json!({"op":"recordReady","token":token,"id":8}), 1300)["accepted"],
        false
    );
    assert_eq!(
        service
            .borrow_mut()
            .session(&json!({"op":"recordCurrent","token":token}), 1300)["valid"],
        false
    );
    service
        .borrow_mut()
        .session(&json!({"op":"recentAdd","path":"/one.png"}), 1400);
    let other = Session::default();
    assert_eq!(
        other
            .capture_service()
            .borrow_mut()
            .session(&json!({"op":"snapshot"}), 1400)["state"]["recent"],
        json!([])
    );
    assert_eq!(
        service
            .borrow_mut()
            .session(&json!({"op":"snapshot"}), 1400)["state"]["recent"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn queued_work_is_fenced_before_the_sdk_can_execute_a_superseded_generation() {
    let (_home, mut session) = fixture("queued", 0);
    let now = Instant::now();
    command(&session, json!({"action":"start","mode":"full"}), now);
    session.service_tick(now + Duration::from_millis(160));
    assert!(session.service_pending());
    command(&session, json!({"action":"start","mode":"window"}), now);
    assert!(session.take_service_effects().is_none());
    assert_eq!(
        tick(&mut session, now + Duration::from_millis(160))[0]["mode"],
        "window"
    );
}

#[test]
fn output_completion_uses_native_preferences_and_records_only_successful_new_images_once() {
    let (_home, session) = fixture("output", 0);
    let service = session.capture_service();
    let mut service = service.borrow_mut();
    let plan = |service: &mut Service, kind: &str| {
        let mut reply = service.output(
            &json!({"op":"begin","kind":kind,"prefs":{"clipboard":true}}),
            1000,
        );
        assert_eq!(reply["effect"]["op"], "dirs"); // Caller preferences do not override the native profile.
        while reply.get("effect").is_some() {
            let result = match reply["effect"]["op"].as_str().unwrap() {
                "dirs" => json!({"pictures":"/home/test/Pictures"}),
                "stat" => json!({"exists":false}),
                _ => Value::Null,
            };
            reply = service.output(
                &json!({"op":"next","id":reply["id"],"step":reply["step"],"result":result}),
                1000,
            );
        }
        reply
    };
    let failed = plan(&mut service, "image");
    let result = service.output(
        &json!({"op":"complete","id":failed["id"],"result":{"ok":false}}),
        2000,
    );
    assert_eq!(result["capture"]["recent"], json!([]));
    assert!(result["path"].is_null());
    let success = plan(&mut service, "image");
    let result = service.output(
        &json!({"op":"complete","id":success["id"],"result":{"ok":true,"path":"/saved.png"}}),
        3000,
    );
    assert_eq!(result["capture"]["recent"][0]["path"], "/saved.png");
    assert_eq!(result["capture"]["recent"][0]["time"], 3000);
    let revision = result["capture"]["revision"].clone();
    let duplicate = service.output(
        &json!({"op":"complete","id":success["id"],"result":{"ok":true}}),
        4000,
    );
    assert!(duplicate.get("error").is_some());
    assert_eq!(duplicate["capture"]["revision"], revision);
    let annotated = plan(&mut service, "annotated");
    let result = service.output(
        &json!({"op":"complete","id":annotated["id"],"result":{"ok":true}}),
        5000,
    );
    assert_eq!(result["capture"]["recent"].as_array().unwrap().len(), 1);
}

#[test]
fn native_shortcuts_route_fresh_preferences_and_keep_our_recording_priority() {
    let (home, mut session) = fixture("shortcuts", 0);
    let now = Instant::now();
    let key = |session: &Session, id: &str| {
        command(
            session,
            json!({"action":"shortcut","key":id,"media":true}),
            now,
        )
    };
    crate::settings::with(&home.0, |s| {
        s.set("screenshotApp", &json!("org.capture.desktop"));
    });
    assert_eq!(
        key(&session, "capture.selector")["effects"][0]["app"],
        "desktop:org.capture"
    );
    assert_eq!(key(&session, "capture.panel")["effects"][0]["op"], "launch");
    command(&session, json!({"action":"start","mode":"record"}), now);
    let cancelled = key(&session, "capture.panel");
    assert_eq!(cancelled["capture"]["pendingRecord"], false);
    command(&session, json!({"action":"start","mode":"record"}), now);
    let work = tick(&mut session, now + Duration::from_millis(220));
    session.capture_service().borrow_mut().session(
        &json!({"op":"recordReady","token":work[0]["recordToken"],"id":7}),
        1000,
    );
    assert_eq!(key(&session, "capture.panel")["effects"][0]["id"], 7);
    crate::settings::with(&home.0, |s| {
        s.set("screenshotApp", &json!("lordos-screenshots.desktop"));
    });
    assert_eq!(
        command(
            &session,
            json!({"action":"shortcutUsable","key":"capture.full","media":false}),
            now
        )["result"]["enabled"],
        false
    );
    assert_eq!(
        key(&session, "capture.selector")["result"]
            .get("ticket")
            .is_some(),
        true
    );
    assert_eq!(
        tick(&mut session, now + Duration::from_millis(160))[0]["mode"],
        "area"
    );
}

#[test]
fn refusal_uses_native_delivery_policy_deduplicates_and_owns_privacy_actions() {
    let (home, mut session) = fixture("refusal", 0);
    let now = Instant::now();
    let notifications = session.notification_service();
    notifications.borrow_mut().bind_settings(&home.0);
    let mut first_id = 0;
    for (prefs, fallback) in [
        (json!({}), false),
        (json!({"dnd":true}), true),
        (json!({"apps":{"screenshots":{"style":"none"}}}), true),
        (json!({"apps":{"screenshots":{"allow":false}}}), true),
    ] {
        crate::settings::with(&home.0, |s| {
            s.set("notifications", &prefs);
        });
        let begin = command(&session, json!({"action":"start","mode":"full"}), now);
        let ticket = begin["result"]["ticket"].clone();
        tick(&mut session, now + Duration::from_millis(160));
        let result = command(&session, json!({"action":"refused","ticket":ticket}), now);
        let effects = result["effects"].as_array().unwrap();
        assert_eq!(effects.iter().any(|e| e["op"] == "toast"), fallback);
        let history = notifications
            .borrow_mut()
            .command(&json!({"action":"snapshot"}), now)["result"]["history"]
            .clone();
        if first_id == 0 {
            first_id = history[0]["id"].as_u64().unwrap();
        }
        assert_eq!(
            command(&session, json!({"action":"refused","ticket":ticket}), now)["effects"],
            json!([])
        );
    }
    let action = command(
        &session,
        json!({"action":"noticeAction","id":first_id,"index":0}),
        now,
    );
    assert_eq!(action["effects"][0]["app"], "settings");
    assert_eq!(action["effects"][0]["args"]["section"], "privacy");
    assert_eq!(
        command(
            &session,
            json!({"action":"noticeAction","id":first_id,"index":1}),
            now
        )["effects"],
        json!([])
    );
    assert_eq!(
        command(&session, json!({"action":"refused","ticket":0}), now)["effects"],
        json!([])
    );
}

#[test]
fn terminal_recording_state_releases_its_output_without_touching_a_new_source() {
    let (_home, mut session) = fixture("terminal-output", 0);
    let now = Instant::now();
    let owner = session.capture_service();
    let target = |service: &mut Service| {
        let mut reply = service.output(&json!({"op":"begin","kind":"record"}), 1000);
        while reply.get("effect").is_some() {
            let result = if reply["effect"]["op"] == "dirs" {
                json!({"pictures":"/home/test/Pictures"})
            } else if reply["effect"]["op"] == "stat" {
                json!({"exists":false})
            } else {
                Value::Null
            };
            reply = service.output(
                &json!({"op":"next","id":reply["id"],"step":reply["step"],"result":result}),
                1000,
            );
        }
        reply
    };
    command(&session, json!({"action":"start","mode":"record"}), now);
    let first = tick(&mut session, now + Duration::from_millis(220));
    let output = target(&mut owner.borrow_mut());
    owner.borrow_mut().session(
        &json!({"op":"recordReady","token":first[0]["recordToken"],"id":7,"output":output["id"]}),
        1000,
    );
    let failed = owner.borrow_mut().session(
        &json!({"op":"recordEvent","id":7,"type":"state","data":{"state":"failed"}}),
        2000,
    );
    assert_eq!(failed["terminal"], true);
    assert_eq!(failed["state"]["recent"], json!([]));
    assert!(owner.borrow().record_outputs.is_empty());
    command(&session, json!({"action":"start","mode":"record"}), now);
    let next = tick(&mut session, now + Duration::from_millis(220));
    let replacement = target(&mut owner.borrow_mut());
    assert_eq!(output["target"]["path"], replacement["target"]["path"]);
    owner.borrow_mut().session(&json!({"op":"recordReady","token":next[0]["recordToken"],"id":8,"output":replacement["id"]}),2000);
    let old = owner.borrow_mut().session(
        &json!({"op":"recordEvent","id":7,"type":"done","data":{"ok":true}}),
        3000,
    );
    assert_eq!(old["completed"], false);
    assert_eq!(old["state"]["recording"]["id"], 8);
    assert_eq!(owner.borrow().record_outputs.len(), 1);
}
