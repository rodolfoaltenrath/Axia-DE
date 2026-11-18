use super::*;
fn call(s: &mut Engine, cmd: &str, now: i64) -> Value {
    s.command(&json!({"cmd":cmd}), now)
}
fn instant(h: i32, m: i32) -> i64 {
    let mut t = local(1_700_000_000_000);
    t.tm_hour = h;
    t.tm_min = m;
    t.tm_sec = 0;
    t.tm_isdst = -1;
    (unsafe { libc::mktime(&mut t) }) * 1000
}
#[test]
fn timer_pause_resume_reset_and_completion_preserve_repeat_duration() {
    let mut s = Engine::default();
    s.command(&json!({"cmd":"timerStart","seconds":10}), 1000);
    assert_eq!(timer_left(&s.timer, 3000), 8000);
    call(&mut s, "timerPause", 3000);
    assert_eq!(timer_left(&s.timer, 100000), 8000);
    call(&mut s, "timerResume", 50000);
    assert_eq!(s.timer["endsAt"], 58000);
    let reply = call(&mut s, "arm", 58000);
    assert_eq!(s.timer["state"], "idle");
    assert_eq!(s.timer["duration"], 10000);
    assert_eq!(reply["effects"][0]["kind"], "timer");
    assert!(call(&mut s, "arm", 59000)["effects"]
        .as_array()
        .unwrap()
        .is_empty());
    call(&mut s, "timerReset", 60000);
    assert_eq!(s.timer["duration"], 10000);
}
#[test]
fn stopwatch_resumes_accumulates_laps_and_keeps_only_two_hundred() {
    let mut s = Engine::default();
    call(&mut s, "swStart", 1000);
    call(&mut s, "swStart", 2000);
    assert_eq!(elapsed(&s.stopwatch, 3500), 2500);
    call(&mut s, "swLap", 4000);
    call(&mut s, "swStop", 5000);
    assert_eq!(elapsed(&s.stopwatch, 8000), 4000);
    call(&mut s, "swStart", 10000);
    assert_eq!(elapsed(&s.stopwatch, 12000), 6000);
    for i in 0..250 {
        call(&mut s, "swLap", 12000 + i);
    }
    assert_eq!(s.stopwatch["laps"].as_array().unwrap().len(), 200);
    call(&mut s, "swReset", 15000);
    assert_eq!(elapsed(&s.stopwatch, 20000), 0);
    assert_eq!(s.stopwatch["laps"], json!([]));
}
#[test]
fn one_shot_recurring_snoozed_and_late_alarms_ring_once() {
    let now = instant(7, 0);
    let alarm =
        json!({"id":"a1","h":7,"m":0,"days":[],"on":true,"fired":now-60000,"label":"Wake up"});
    let mut s = Engine::default();
    s.sync(&json!({"alarms":[alarm],"cities":["UTC"],"futureField":42}));
    let reply = call(&mut s, "arm", now);
    assert_eq!(reply["effects"][0]["kind"], "alarm");
    assert_eq!(s.config["alarms"][0]["on"], false);
    assert_eq!(s.config["futureField"], 42);
    assert_eq!(s.config["cities"], json!(["UTC"]));
    assert!(call(&mut s, "arm", now + 2000)["effects"]
        .as_array()
        .unwrap()
        .is_empty());
    let reply = s.command(&json!({"cmd":"alarmSnooze","id":"a1"}), now + 3000);
    assert_eq!(
        next_fire(&s.config["alarms"][0], now + 3000),
        Some(now + 3000 + 9 * MIN)
    );
    assert!(!reply["effects"].as_array().unwrap().is_empty());
    let late = json!({"id":"late","h":7,"m":0,"days":[0,1,2,3,4,5,6],"on":true,"fired":0});
    s.sync(&json!({"alarms":[late]}));
    assert!(call(&mut s, "arm", now + LATE + 2000)["effects"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn next_fire_weekday_snooze_off_and_scheduler_caps() {
    let now = instant(6, 0);
    let wd = local(now).tm_wday;
    let alarm = json!({"on":true,"h":7,"m":0,"days":[wd]});
    assert_eq!(next_fire(&alarm, now), Some(now + 60 * MIN));
    assert!(next_fire(&alarm, now + 120 * MIN).unwrap() > now + 6 * 24 * 60 * MIN);
    assert_eq!(next_fire(&json!({"on":false}), now), None);
    let mut s = Engine::default();
    s.sync(&json!({"alarms":[alarm]}));
    assert_eq!(s.snapshot(now)["wait"], LATE);
    assert_eq!(s.snapshot(now + 30 * MIN)["wait"], MIN);
    s.config = json!({});
    assert!(s.snapshot(now)["wait"].is_null());
}
#[test]
fn alarm_edits_reset_fired_and_preserve_other_settings() {
    let now = instant(6, 0);
    let mut s = Engine::default();
    s.sync(&json!({"cities":["UTC"],"custom":true}));
    let reply = s.command(
        &json!({"cmd":"alarmAdd","alarm":{"h":7,"m":15,"label":"A"}}),
        now,
    );
    let id = reply["value"].clone();
    s.command(
        &json!({"cmd":"alarmUpdate","id":id,"patch":{"m":20}}),
        now + 100,
    );
    assert_eq!(s.config["alarms"][0]["fired"], now + 100);
    assert_eq!(s.config["alarms"][0]["snooze"], 0);
    s.command(&json!({"cmd":"alarmRemove","id":id}), now + 200);
    assert_eq!(s.config["alarms"], json!([]));
    assert_eq!(s.config["cities"], json!(["UTC"]));
    assert_eq!(s.config["custom"], true);
    assert_eq!(duration(3_661_239, true, ","), "1:01:01,23");
    assert_eq!(duration(-10, false, "."), "00:00");
}
#[test]
fn native_commands_keep_effects_until_the_shell_drains_them() {
    let mut s = Engine::default();
    s.native(&json!({"cmd":"alarmAdd","alarm":{"h":23}}), instant(6, 0));
    assert!(call(&mut s, "snapshot", instant(6, 0))["effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "settings"));
}

#[test]
fn daylight_saving_recurrence_matches_local_calendar_days() {
    if std::env::var_os("AXIA_CLOCK_DST_CHILD").is_none() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "clock::tests::daylight_saving_recurrence_matches_local_calendar_days",
                "--nocapture",
            ])
            .env("AXIA_CLOCK_DST_CHILD", "1")
            .env("TZ", "America/New_York")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        return;
    }
    fn utc(y: i32, mo: i32, d: i32, h: i32, m: i32) -> i64 {
        let mut t = unsafe { std::mem::zeroed::<libc::tm>() };
        t.tm_year = y - 1900;
        t.tm_mon = mo - 1;
        t.tm_mday = d;
        t.tm_hour = h;
        t.tm_min = m;
        (unsafe { libc::timegm(&mut t) }) * 1000
    }
    let a = json!({"on":true,"h":1,"m":30,"days":[]});
    assert_eq!(
        next_fire(&a, utc(2026, 11, 1, 4, 0)),
        Some(utc(2026, 11, 1, 5, 30))
    );
    let gap = json!({"on":true,"h":2,"m":30,"days":[]});
    assert_eq!(
        next_fire(&gap, utc(2026, 3, 8, 5, 0)),
        Some(utc(2026, 3, 8, 7, 30))
    );
}
