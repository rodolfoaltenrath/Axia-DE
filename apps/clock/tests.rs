use super::*;
fn app() -> App {
    clock::with(|s| *s = Default::default());
    let mut a = App::default();
    a.set_tab("world");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    a.resources(&root.to_string_lossy(), None);
    a.request(&json!({"cmd":"sync","lang":"en","settings":{"clock":{"hour24":"24"}},"context":{"width":560,"theme":{"text":"#fff","textDim":"#aaa","accent":"#0a84ff","mode":"dark","control":"#333","controlRadius":7,"controlRadiusSmall":6}}}));
    a
}
#[test]
fn all_four_tabs_are_native_and_the_icon_uses_the_same_face() {
    let mut a = app();
    for (tab, label) in [
        ("world", "clock.world.local"),
        ("alarms", "clock.alarm.empty"),
        ("timer", "clock.timer.start"),
        ("stopwatch", "clock.sw.start"),
    ] {
        a.request(&json!({"cmd":"tab","key":tab}));
        let tree = ui::tree(&mut a, clock::now()).to_string();
        assert!(tree.contains(label), "{tab}: {tree}");
    }
    let icon = a.request(&json!({"cmd":"icon","size":64}));
    assert_eq!(icon["style"]["width"], 64.);
    assert!(icon["children"][0]["children"].as_array().unwrap().len() > 12);
}
#[test]
fn alarm_editor_saves_days_label_toggle_delete_and_preserves_settings() {
    let mut a = app();
    a.request(&json!({"cmd":"tab","key":"alarms"}));
    a.request(&json!({"cmd":"add"}));
    assert_eq!(a.editing.as_ref().unwrap().days, vec![1, 2, 3, 4, 5]);
    a.request(&json!({"cmd":"label","value":"Rust alarm"}));
    a.request(&json!({"cmd":"day","key":"0"}));
    a.request(&json!({"cmd":"step","key":"h:1"}));
    let result = a.request(&json!({"cmd":"alarmSave"}));
    assert_eq!(result["effect"]["kind"], "clock");
    let alarm = clock::with(|s| s.config["alarms"][0].clone());
    assert_eq!(alarm["h"], 8);
    assert_eq!(alarm["label"], "Rust alarm");
    assert_eq!(clock::with(|s| s.config["hour24"].clone()), "24");
    let id = alarm["id"].as_str().unwrap();
    a.request(&json!({"cmd":"alarmToggle","key":id}));
    assert_eq!(clock::with(|s| s.config["alarms"][0]["on"].clone()), false);
    a.request(&json!({"cmd":"alarmEdit","key":id}));
    a.request(&json!({"cmd":"alarmDelete"}));
    assert_eq!(clock::with(|s| s.config["alarms"].clone()), json!([]));
}
#[test]
fn timer_and_stopwatch_survive_window_close_and_use_dynamic_cadence() {
    let mut a = app();
    a.set_tab("timer");
    assert!(a.refresh_ms().is_none());
    a.request(&json!({"cmd":"preset","key":"3"}));
    assert_eq!(clock::with(|s| s.timer["duration"].clone()), 180000);
    assert_eq!(a.refresh_ms(), Some(250.));
    a.request(&json!({"cmd":"timerPause"}));
    assert!(a.refresh_ms().is_none());
    a.request(&json!({"cmd":"timerResume"}));
    a.request(&json!({"cmd":"close"}));
    drop(a);
    let mut b = App::default();
    assert_eq!(b.tab, "timer");
    assert_eq!(clock::with(|s| s.timer["state"].clone()), "running");
    b.set_tab("stopwatch");
    b.request(&json!({"cmd":"swStart"}));
    assert_eq!(b.refresh_ms(), Some(47.));
    b.request(&json!({"cmd":"swLap"}));
    assert_eq!(
        clock::with(|s| s.stopwatch["laps"].as_array().unwrap().len()),
        1
    );
    b.request(&json!({"cmd":"swStop"}));
    assert!(b.refresh_ms().is_none());
}
#[test]
fn timezone_queries_keep_the_process_zone_and_accept_fractional_offsets() {
    let mut a = app();
    assert!(a.inflight.is_some());
    a.request(&json!({"cmd":"next","result":{"value":"America/Sao_Paulo"}}));
    a.request(
        &json!({"cmd":"next","result":{"value":["UTC","Asia/Kathmandu","America/Sao_Paulo"]}}),
    );
    assert_eq!(a.zone, "America/Sao_Paulo");
    a.request(&json!({"cmd":"add"}));
    a.request(&json!({"cmd":"query","value":"kathmandu"}));
    assert_eq!(a.zone_list(), vec!["Asia/Kathmandu"]);
    a.request(&json!({"cmd":"zoneFirst"}));
    assert_eq!(
        clock::with(|s| s.config["cities"].clone()),
        json!(["Asia/Kathmandu"])
    );
    a.offsets
        .insert("Asia/Kathmandu".into(), (20700, "+0545".into()));
    let (time, off, _) = a.zone_time("Asia/Kathmandu", 0).unwrap();
    assert_eq!((time.tm_hour, time.tm_min, off), (5, 45, 20700));
    a.request(&json!({"cmd":"zoneRemove","key":"Asia/Kathmandu"}));
    assert_eq!(clock::with(|s| s.config["cities"].clone()), json!([]));
}
#[test]
fn locale_hour_cycle_and_day_labels_keep_the_existing_preferences() {
    let mut a = app();
    let mut tm = clock::local(0);
    tm.tm_hour = 13;
    tm.tm_min = 7;
    assert_eq!(a.time(&tm), "13:07");
    a.settings["clock"]["hour24"] = json!("12");
    assert_eq!(a.time(&tm), "1:07 PM");
    assert_eq!(a.days(&[])["key"], "clock.alarm.once");
    assert_eq!(a.days(&[0, 6])["key"], "clock.alarm.weekends");
    assert_eq!(a.days(&[1, 2, 3, 4, 5])["key"], "clock.alarm.weekdays");
}
