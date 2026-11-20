use super::*;
fn reset() {
    clock::with(|s| *s = Default::default());
}

#[test]
fn scheduler_observes_native_commands_and_delivers_completion_once_without_idle_polling() {
    reset();
    let mono = Instant::now();
    let mut scheduler = Scheduler::default();
    scheduler.sync_at(1000, mono);
    assert!(scheduler.refresh_ms().is_none());
    clock::with(|s| s.native(&json!({"cmd":"timerStart","seconds":1}), 1000));
    assert!(scheduler.invalidated());
    scheduler.sync_at(1000, mono);
    assert_eq!(scheduler.until, Some(mono + Duration::from_secs(1)));
    assert!(!scheduler.invalidated());
    let reply = scheduler.sync_at(2000, mono + Duration::from_secs(1));
    assert_eq!(
        reply["effects"][0]["reply"]["effects"],
        json!([{"kind":"timer"}])
    );
    assert!(scheduler.refresh_ms().is_none());
    assert!(!scheduler.invalidated());
    assert!(scheduler.sync_at(2000, mono)["effects"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn remount_keeps_the_original_deadline_and_pending_settings_are_not_lost() {
    reset();
    let mono = Instant::now();
    clock::with(|s| s.native(&json!({"cmd":"timerStart","seconds":6}), 1000));
    {
        let mut scheduler = Scheduler::default();
        scheduler.sync_at(1000, mono);
    }
    let mut scheduler = Scheduler::default();
    scheduler.sync_at(4000, mono);
    assert_eq!(scheduler.until, Some(mono + Duration::from_secs(3)));
    clock::with(|s| s.native(&json!({"cmd":"alarmAdd","alarm":{"h":8,"m":0}}), 4000));
    assert!(scheduler.invalidated());
    let reply = scheduler.sync_at(4000, mono);
    assert!(reply["effects"][0]["reply"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "settings"));
}

#[test]
fn status_borrows_timer_pause_and_focus_target_without_js_facts() {
    reset();
    let mono = Instant::now();
    let mut status = Status::default();
    let input = json!({"cmd":"sync","args":{"height":24,"color":"#fff"},"lang":"en"});
    assert_eq!(
        status.sync_at(&input, 1000, mono)["tree"]["style"]["width"],
        0
    );
    assert!(status.refresh_ms().is_none());
    clock::with(|s| s.native(&json!({"cmd":"timerStart","seconds":30}), 1000));
    assert!(status.invalidated());
    let reply = status.sync_at(&input, 1000, mono);
    assert_eq!(reply["tree"]["children"][1]["label"], "0:30");
    assert_eq!(status.until, Some(mono + Duration::from_secs(1)));
    clock::with(|s| s.native(&json!({"cmd":"timerPause"}), 5000));
    assert_eq!(
        status.sync_at(&input, 5000, mono)["tree"]["children"][1]["label"],
        "0:26"
    );
    assert!(status.refresh_ms().is_none());
    assert_eq!(
        status.request(&json!({"cmd":"open"}))["effects"][0]["args"]["tab"],
        "timer"
    );
    clock::with(|s| s.native(&json!({"cmd":"timerReset"}), 5000));
    assert!(status.request(&json!({"cmd":"open"}))["effects"].is_null());
    assert_eq!(timer_caption(3_600_001), "1:00:01");
}

#[test]
fn alarm_indicator_appears_at_the_24_hour_boundary_without_a_js_tick() {
    reset();
    let mono = Instant::now();
    let now = clock::now();
    let at = now + 90_000_000;
    clock::with(|s| s.sync(&json!({"alarms":[{"id":"one","on":true,"snooze":at}]})));
    let mut status = Status::default();
    let input = json!({"cmd":"sync","lang":"en"});
    assert_eq!(
        status.sync_at(&input, now, mono)["tree"]["style"]["width"],
        0
    );
    assert!(status.refresh_ms().is_some());
    let reply = status.sync_at(&input, at - 86_400_000 + 1, mono);
    assert!(reply["tree"]["children"][1]["label"]
        .as_str()
        .unwrap()
        .contains(":"));
    assert!(status.refresh_ms().is_none());
}
