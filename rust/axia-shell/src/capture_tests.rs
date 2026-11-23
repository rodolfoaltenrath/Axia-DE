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
