use super::*;

fn controller() -> Controller {
    axia_clock::with(|s| *s = Default::default());
    Controller::new(Session::default())
}
fn command(controller: &mut Controller, action: &str, extra: Value) -> Value {
    let mut input = json!({"cmd":"clockCommand","action":action});
    input.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    controller.request(&input)
}

#[test]
fn session_mutations_ignore_caller_time_for_timer_stopwatch_and_expiry() {
    let mut controller = controller();
    let before = axia_clock::now();
    let started = command(&mut controller, "timerStart", json!({"seconds":60,"now":0}));
    let after = axia_clock::now();
    let ends = started["timer"]["endsAt"].as_i64().unwrap();
    assert!((before + 60000..=after + 60000).contains(&ends));
    let armed = command(&mut controller, "arm", json!({"now":i64::MAX}));
    assert_eq!(armed["timer"]["state"], "running", "a caller timestamp cannot expire the native timer");
    assert!(armed["notificationEffects"].is_null());
    let before = axia_clock::now();
    let paused = command(&mut controller, "timerPause", json!({"now":i64::MAX}));
    let after = axia_clock::now();
    let left = paused["timer"]["left"].as_i64().unwrap();
    assert!((ends - after..=ends - before).contains(&left));
    assert_eq!(paused["timer"]["state"], "paused");
    let before = axia_clock::now();
    let resumed = command(&mut controller, "timerResume", json!({"now":i64::MIN}));
    let after = axia_clock::now();
    assert!((before + left..=after + left).contains(&resumed["timer"]["endsAt"].as_i64().unwrap()));
    let before = axia_clock::now();
    let started = command(&mut controller, "swStart", json!({"now":0}));
    let after = axia_clock::now();
    let start = started["stopwatch"]["start"].as_i64().unwrap();
    assert!((before..=after).contains(&start));
    let before = axia_clock::now();
    let stopped = command(&mut controller, "swStop", json!({"now":i64::MAX}));
    let after = axia_clock::now();
    assert!((before - start..=after - start).contains(&stopped["stopwatch"]["acc"].as_i64().unwrap()));
    assert_eq!(stopped["stopwatch"]["running"], false);
}

#[test]
fn explicit_time_calculations_remain_pure_and_snapshot_uses_native_time() {
    let mut controller = controller();
    command(&mut controller, "timerStart", json!({"seconds":60}));
    let before = axia_clock::with(|s| (s.timer.clone(), s.stopwatch.clone()));
    let changes = axia_clock::subscribe();
    assert_eq!(command(&mut controller, "nextFire", json!({"alarm":{"on":true,"snooze":2000},"now":1000}))["value"], 2000);
    assert_eq!(command(&mut controller, "timerLeft", json!({"state":{"state":"running","endsAt":10000},"now":7500}))["value"], 2500);
    assert_eq!(command(&mut controller, "swElapsed", json!({"state":{"running":true,"start":1000,"acc":50},"now":1500}))["value"], 550);
    let snapshot = command(&mut controller, "snapshot", json!({"now":0}));
    let wait = snapshot["wait"].as_i64().unwrap();
    assert!(wait > 0 && wait <= 60000, "snapshot must use current time: {snapshot}");
    assert_eq!(axia_clock::with(|s| (s.timer.clone(), s.stopwatch.clone())), before);
    assert!(!changes.pending(), "queries must not invalidate the engine or deliver completions");
}
