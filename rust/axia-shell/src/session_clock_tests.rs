use super::*;
fn reset() {
    clock::with(|s| *s = Default::default());
}

#[test]
fn scheduler_observes_native_commands_and_delivers_completion_once_without_idle_polling() {
    reset();
    let mono = Instant::now();
    let mut scheduler = Scheduler::default();
    scheduler.tick_at(1000, mono);
    assert!(scheduler.wait_ms(mono).is_none());
    clock::with(|s| s.native(&json!({"cmd":"timerStart","seconds":1}), 1000));
    assert!(scheduler.invalidated());
    scheduler.tick_at(1000, mono);
    assert_eq!(scheduler.until, Some(mono + Duration::from_secs(1)));
    assert!(!scheduler.invalidated());
    let reply = scheduler.tick_at(2000, mono + Duration::from_secs(1));
    assert_eq!(
        reply[0]["reply"]["effects"],
        json!([{"kind":"timer"}])
    );
    assert!(scheduler.wait_ms(mono).is_none());
    assert!(!scheduler.invalidated());
    assert!(scheduler.tick_at(2000, mono).is_empty());
}

#[test]
fn recreation_keeps_the_engine_deadline_and_pending_settings_are_not_lost() {
    reset();
    let mono = Instant::now();
    clock::with(|s| s.native(&json!({"cmd":"timerStart","seconds":6}), 1000));
    {
        let mut scheduler = Scheduler::default();
        scheduler.tick_at(1000, mono);
    }
    let mut scheduler = Scheduler::default();
    scheduler.tick_at(4000, mono);
    assert_eq!(scheduler.until, Some(mono + Duration::from_secs(3)));
    clock::with(|s| s.native(&json!({"cmd":"alarmAdd","alarm":{"h":8,"m":0}}), 4000));
    assert!(scheduler.invalidated());
    let reply = scheduler.tick_at(4000, mono);
    assert!(reply[0]["reply"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "settings"));
}
