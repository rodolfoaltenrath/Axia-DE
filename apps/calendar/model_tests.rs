use super::*;
fn day(s: &str) -> i64 {
    date::parse(s).unwrap()
}
fn event(start: &str, end: &str) -> Value {
    json!({"uid":"e","cal":"a","start":day(start),"end":day(end),"allDay":false,"title":"Event","alarms":[]})
}
#[test]
fn visible_ranges_use_monday_and_six_full_weeks() {
    let d = day("2026-11-17");
    for (view, from, to) in [
        ("day", "2026-11-17", "2026-11-18"),
        ("week", "2026-11-16", "2026-11-23"),
        ("month", "2026-10-26", "2026-12-07"),
    ] {
        let (a, b) = range(view, d);
        assert_eq!(date::ymd(a), from);
        assert_eq!(date::ymd(b), to);
    }
}
#[test]
fn event_days_respect_exclusive_end_dates_midnight_and_bounds() {
    let e = event("2026-11-17", "2026-11-19");
    assert_eq!(
        event_days(&e)
            .iter()
            .map(|d| date::ymd(*d))
            .collect::<Vec<_>>(),
        vec!["2026-11-17", "2026-11-18"]
    );
    let mut e = e;
    e["allDay"] = json!(true);
    e["sdate"] = json!("2026-11-10");
    e["edate"] = json!("2026-11-12");
    assert_eq!(event_days(&e), vec![day("2026-11-10"), day("2026-11-11")]);
    e["edate"] = json!("2026-11-09");
    assert_eq!(event_days(&e), vec![day("2026-11-10")]);
    e["edate"] = json!("2030-01-01");
    assert_eq!(event_days(&e).len(), 367);
}
#[test]
fn short_overlapping_events_share_columns_until_the_whole_group_ends() {
    let d = day("2026-11-17");
    let ev = |a, b| json!({"start":date::at_minutes(d,a),"end":date::at_minutes(d,b)});
    let events = vec![ev(540, 600), ev(550, 560), ev(570, 585), ev(600, 660)];
    let layout = layout(d, &events, &[0, 1, 2, 3]);
    assert_eq!(
        layout.iter().map(|s| (s.col, s.cols)).collect::<Vec<_>>(),
        vec![(0, 2), (1, 2), (1, 2), (0, 1)]
    );
    assert_eq!(layout[1].bottom, 565.);
}
#[test]
fn all_day_editor_uses_inclusive_end_and_saves_exclusive_civil_dates() {
    let mut e = event("2026-11-17", "2026-11-20");
    e["allDay"] = json!(true);
    e["sdate"] = json!("2026-11-17");
    e["edate"] = json!("2026-11-20");
    let d = draft(&e, &[]);
    assert_eq!(date::ymd(num(&d["eDay"])), "2026-11-19");
    let saved = build(&e, &d, false, None);
    assert_eq!(saved["sdate"], "2026-11-17");
    assert_eq!(saved["edate"], "2026-11-20");
}
#[test]
fn moving_start_keeps_day_span_and_time_span_with_midnight_guard() {
    let e = event("2026-11-17", "2026-11-19");
    let mut d = draft(&e, &[]);
    set_start_day(&mut d, day("2026-12-31"));
    assert_eq!(date::ymd(num(&d["eDay"])), "2027-01-02");
    d["eDay"] = d["sDay"].clone();
    d["sMin"] = json!(540);
    d["eMin"] = json!(600);
    set_start_min(&mut d, 1380);
    assert_eq!(d["eMin"], 1439);
}
#[test]
fn recurring_save_moves_first_occurrence_and_preserves_invitation_metadata() {
    let first = day("2026-11-03");
    let mut e = event("2026-11-17", "2026-11-18");
    e["occ"] = e["start"].clone();
    e["rrule"] = json!("FREQ=WEEKLY;BYDAY=TU;COUNT=8");
    e["tz"] = json!("America/Sao_Paulo");
    e["url"] = json!("https://example.org/meeting");
    e["status"] = json!("CONFIRMED");
    e["alarms"] = json!([5, 15, 60]);
    e["att"] = json!([{"email":"a@b","ps":"ACCEPTED"}]);
    e["org"] = json!({"email":"org@b","name":"Organizer"});
    let mut d = draft(&e, &[]);
    set_start_day(&mut d, day("2026-11-18"));
    d["cal"] = json!("b");
    d["alarm"] = json!(10);
    let args = save_args(&e, &d, false, Some(first));
    assert_eq!(date::ymd(num(&args["event"]["start"])), "2026-11-04");
    assert_eq!(args["from"], "a");
    let saved = &args["event"];
    for k in ["tz", "url", "status", "att", "rrule"] {
        assert_eq!(saved[k], e[k]);
    }
    assert_eq!(saved["alarms"], json!([10, 15, 60]));
    assert_eq!(saved["orgEmail"], "org@b");
    assert!(saved.get("occ").is_none());
}
#[test]
fn invalid_timed_end_defaults_to_one_hour_and_alert_none_removes_all() {
    let e = event("2026-11-17", "2026-11-18");
    let mut d = draft(&e, &[]);
    d["sMin"] = json!(600);
    d["eDay"] = d["sDay"].clone();
    d["eMin"] = json!(500);
    let saved = build(&e, &d, false, None);
    assert_eq!(num(&saved["end"]) - num(&saved["start"]), 3600000);
    assert_eq!(saved["alarms"], json!([]));
}
#[test]
fn day_index_puts_all_day_first_then_start_and_nfc_title() {
    let mut e = event("2026-11-17", "2026-11-18");
    let mut all = e.clone();
    all["allDay"] = json!(true);
    e["title"] = json!("Z");
    let mut a = e.clone();
    a["title"] = json!("A");
    let events = vec![e, all, a];
    assert_eq!(by_day(&events)["2026-11-17"], vec![1, 2, 0]);
}
#[test]
fn new_events_choose_visible_calendar_and_do_not_offer_todo_only_lists() {
    let cals = vec![
        json!({"id":"todo","todos":true}),
        json!({"id":"hidden","events":true,"hidden":true}),
        json!({"id":"visible","events":true}),
    ];
    let e = new_event(&cals, day("2026-11-17"), Some(570), 0);
    assert_eq!(e["cal"], "visible");
    assert_eq!(date::minutes(num(&e["start"])), 570);
    assert_eq!(num(&e["end"]) - num(&e["start"]), 3600000);
    assert_eq!(participation("tentative").2, "tentative");
    assert_eq!(tint("bad", true), "rgba(0,122,255,0.2)");
}
