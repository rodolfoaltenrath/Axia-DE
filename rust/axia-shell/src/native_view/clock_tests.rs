use super::*;
fn reset() { clock::with(|s| *s = Default::default()); }

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

#[test]
fn native_appearance_preserves_timer_phase_state_and_open_action() {
    reset();
    let mono = Instant::now();
    let mut status = Status::default();
    let stale = json!({"cmd":"sync","lang":"en","args":{"height":24,"color":"#ff0000"},"context":{"theme":{"menuText":"#ff0000"}}});
    clock::with(|s| s.native(&json!({"cmd":"timerStart","seconds":30}), 1000));
    let timer = clock::with(|s| s.timer.clone());
    status.sync_at(&stale, 1000, mono);
    let first_deadline = status.until;
    for (mode, elapsed) in [("light",250),("dark",750)] {
        let colors = std::rc::Rc::new(crate::native_style::shell_theme::tokens(&json!({"mode":mode,"effects":false})));
        status.set_native_appearance(colors.clone());
        let tree = status.sync_at(&stale, 1000 + elapsed, mono + Duration::from_millis(elapsed as u64))["tree"].clone();
        assert_eq!(tree["children"][1]["label"], "0:30");
        assert_eq!(tree["children"][1]["style"]["color"], colors["menuText"]);
        assert_eq!(tree["children"][0]["children"][1]["style"]["backgroundColor"], colors["menuText"]);
        assert_eq!(status.until, first_deadline, "recoloring cannot postpone the next countdown label");
        assert!(std::rc::Rc::ptr_eq(status.appearance.as_ref().unwrap(), &colors));
        assert!(!status.invalidated());
        assert_eq!(clock::with(|s| s.timer.clone()), timer);
    }
    assert_eq!(status.sync_at(&stale, 2000, mono + Duration::from_secs(1))["tree"]["children"][1]["label"], "0:29");
    assert_eq!(status.until, Some(mono + Duration::from_secs(2)));
    assert_eq!(status.request(&json!({"cmd":"open"}))["effects"][0]["args"]["tab"], "timer");
    clock::with(|s| s.native(&json!({"cmd":"timerPause"}), 2250));
    status.sync_at(&stale, 2250, mono + Duration::from_millis(1250));
    assert!(status.until.is_none());
    clock::with(|s| s.native(&json!({"cmd":"timerReset"}), 2250));
    status.sync_at(&stale, 2250, mono + Duration::from_millis(1250));
    status.set_native_appearance(std::rc::Rc::new(json!({"menuText":"#ffffff"})));
    assert_eq!(status.sync_at(&stale, 2500, mono + Duration::from_millis(1500))["same"], true);
    assert!(status.until.is_none());
}
