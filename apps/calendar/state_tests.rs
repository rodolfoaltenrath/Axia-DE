use super::*;
fn day(s: &str) -> i64 {
    date::parse(s).unwrap()
}
fn event(uid: &str, when: &str) -> Value {
    let start = date::at_minutes(day(when), 540);
    json!({"uid":uid,"cal":"a","title":uid,"start":start,"end":start+3600000,"allDay":false,"alarms":[]})
}
fn ready() -> State {
    let mut s = State::default();
    s.jobs.bind(7);
    s.cals = vec![
        json!({"id":"a","name":"Personal","events":true,"kind":"local"}),
        json!({"id":"r","name":"Remote","events":true,"kind":"dav"}),
    ];
    s.cursor = day("2026-11-17");
    s
}
fn complete(s: &mut State, tag: Tag, result: Result<Value, String>) {
    s.apply(Completed { tag, result }, 1000);
}
fn write(s: &State) -> Write {
    let e = s.event.as_ref().unwrap();
    Write {
        seq: e.seq,
        original: e.original.clone(),
        draft: e.draft.clone(),
        new: e.new,
        first: None,
    }
}
fn next(s: &mut State) -> Value {
    let e = s.jobs.pump(0).expect("request queued");
    assert_eq!(e["args"][1], "pim.request");
    s.jobs.complete(&json!({"error":"dispatched"}), 0);
    e["args"][2].clone()
}
#[test]
fn range_and_calendar_generations_discard_stale_responses() {
    let mut s = ready();
    s.range_epoch = 2;
    s.cal_epoch = 3;
    complete(
        &mut s,
        Tag::Range(2),
        Ok(json!({"events":[event("new","2026-11-17")]})),
    );
    complete(
        &mut s,
        Tag::Range(1),
        Ok(json!({"events":[event("old","2026-11-03")]})),
    );
    assert_eq!(s.events[0]["uid"], "new");
    complete(&mut s, Tag::Cals(2), Err("offline".into()));
    assert!(!s.unavailable);
}
#[test]
fn initial_calendar_failure_is_unavailable_but_later_errors_keep_cached_data() {
    let mut s = ready();
    complete(&mut s, Tag::Cals(0), Err("offline".into()));
    assert!(s.unavailable);
    complete(
        &mut s,
        Tag::Cals(0),
        Ok(json!({"cals":[{"id":"a","events":true}]})),
    );
    complete(&mut s, Tag::Cals(0), Err("offline".into()));
    assert!(!s.unavailable);
    assert_eq!(s.cals[0]["id"], "a");
}
#[test]
fn failed_save_keeps_editor_and_success_cannot_close_a_newer_editor() {
    let mut s = ready();
    s.edit(event("old", "2026-11-17"), false);
    let w = write(&s);
    complete(&mut s, Tag::Save(w.clone()), Err("offline".into()));
    assert_eq!(s.event.as_ref().unwrap().error, "calendar.saveFailed");
    assert!(!s.event.as_ref().unwrap().busy);
    s.edit(event("new", "2026-11-18"), false);
    complete(&mut s, Tag::Save(w), Ok(json!({"uid":"old"})));
    assert_eq!(s.event.as_ref().unwrap().original["uid"], "new");
}
#[test]
fn default_calendar_creation_is_retained_when_event_save_needs_retry() {
    let mut s = ready();
    s.cals.clear();
    s.edit(new_event(&[], s.cursor, Some(540), s.cursor), true);
    let w = write(&s);
    complete(&mut s, Tag::CreateCal(w), Ok(json!({"uid":"created"})));
    assert_eq!(s.event.as_ref().unwrap().draft["cal"], "created");
    let a = next(&mut s);
    assert_eq!(a[0], "ev-save");
    assert_eq!(a[1]["cal"], "created");
    let w = write(&s);
    complete(&mut s, Tag::Save(w), Err("offline".into()));
    s.save();
    let a = next(&mut s);
    assert_eq!(a[0], "ev-save");
    assert_eq!(a[1]["cal"], "created");
}
#[test]
fn series_lookup_ignores_detached_instances_and_does_not_replace_visible_range() {
    let mut s = ready();
    let mut e = event("series", "2026-11-17");
    e["rrule"] = json!("FREQ=WEEKLY");
    e["occ"] = e["start"].clone();
    s.events = vec![e.clone()];
    s.edit(e, false);
    let w = write(&s);
    let first = event("series", "2026-11-03");
    let mut detached = event("series", "2026-10-01");
    detached["rid"] = json!("detached");
    complete(
        &mut s,
        Tag::Series(w),
        Ok(json!({"events":[detached,first.clone(),event("other","2026-09-01")]})),
    );
    let a = next(&mut s);
    assert_eq!(a[1]["event"]["start"], first["start"]);
    assert_eq!(s.events[0]["start"], event("series", "2026-11-17")["start"]);
}
#[test]
fn occurrence_deletion_requires_explicit_scope_and_remote_calendars_cannot_be_deleted() {
    let mut s = ready();
    let mut e = event("series", "2026-11-17");
    e["occ"] = e["start"].clone();
    e["rrule"] = json!("FREQ=WEEKLY");
    s.edit(e.clone(), false);
    s.request(&json!({"cmd":"eventDelete"}), 0);
    assert!(!s.jobs.busy());
    s.request(&json!({"cmd":"eventDelete","key":"this"}), 0);
    let e = s.jobs.complete(&json!({"error":"test"}), 0);
    assert!(!e.is_empty());
    let mut s = ready();
    s.request(&json!({"cmd":"calDelete","key":"r"}), 0);
    assert!(!s.jobs.busy());
}
#[test]
fn alarm_opens_exact_occurrence_and_nonce_reopens_without_navigation_loop() {
    let mut s = ready();
    s.view = "month".into();
    let mut e = event("series", "2026-11-17");
    e["occ"] = e["start"].clone();
    s.events = vec![e.clone()];
    let args = json!({"at":e["start"],"uid":"series","cal":"a","nonce":1});
    s.alarm(&args);
    assert_eq!(s.view, "day");
    s.resolve_alarm();
    assert_eq!(s.event.as_ref().unwrap().original["uid"], "series");
    s.close();
    s.alarm(&args);
    s.resolve_alarm();
    assert!(s.event.is_none());
    let mut again = args;
    again["nonce"] = json!(2);
    s.alarm(&again);
    s.resolve_alarm();
    assert!(s.event.is_some());
}
#[test]
fn clipboard_replies_do_not_reach_replaced_event_or_blurred_notes() {
    let mut s = ready();
    s.edit(event("a", "2026-11-17"), false);
    let seq = s.event.as_ref().unwrap().seq;
    s.notes_focus = true;
    s.edit(event("b", "2026-11-18"), false);
    s.notes_focus = true;
    complete(&mut s, Tag::Paste(seq), Ok(json!({"text":"late"})));
    assert_eq!(s.event.as_ref().unwrap().draft["notes"], "");
}
#[test]
fn failed_calendar_save_preserves_name_and_color_and_ignores_stale_acknowledgements() {
    let mut s = ready();
    s.edit_cal(None);
    s.calendar.as_mut().unwrap().name = "Team".into();
    let seq = s.calendar.as_ref().unwrap().seq;
    complete(&mut s, Tag::SaveCal(seq), Err("offline".into()));
    assert_eq!(s.calendar.as_ref().unwrap().name, "Team");
    assert_eq!(s.calendar.as_ref().unwrap().error, "calendar.saveFailed");
    s.edit_cal(None);
    complete(&mut s, Tag::SaveCal(seq), Ok(json!({"uid":"old"})));
    assert!(s.calendar.is_some());
}
#[test]
fn mounted_controller_restores_state_issues_bounded_queries_and_tracks_navigation() {
    let mut s = State::default();
    let mut token = 0;
    let mut packets = vec![];
    let mut calls = vec![];
    let args = json!({"__nativeOwner":7,"__nativeWindow":"calendar#2","__nativeState":{"view":"week","cursor":day("2026-11-17"),"sidebar":false,"narrowSidebar":true}});
    for tick in 0..30 {
        let now = tick * 100;
        let mut out = s.request(&json!({"cmd":"sync","args":args}), now);
        for round in 0..32 {
            let e = &out["effect"];
            if e.is_null() {
                break;
            }
            assert!(round < 31, "must yield between polls");
            let result = if e["kind"] == "nativeCall" {
                assert_eq!(e["args"][0], 7);
                let m = super::s(&e["args"][1]);
                let a = &e["args"][2];
                calls.push((m.to_owned(), a.clone()));
                let value = match m {
                    "pim.watch" | "windows.state" => Value::Null,
                    "pim.events" => json!([]),
                    "pim.request" => match super::s(&a[0]) {
                        "cal-list" => json!({"cals":[{"id":"a","events":true}]}),
                        "ev-list" => json!({"events":[event("e","2026-11-17")]}),
                        _ => panic!("{a}"),
                    },
                    _ => panic!("{m}"),
                };
                token += 1;
                packets.push(json!({"token":token,"value":value}));
                json!(token)
            } else {
                json!({"value":std::mem::take(&mut packets)})
            };
            out = s.request(&json!({"cmd":"next","result":result}), now);
        }
    }
    assert!(!s.sidebar);
    assert!(s.narrow_sidebar);
    assert_eq!(s.events.len(), 1);
    assert_eq!(calls.iter().filter(|(m, _)| m == "pim.request").count(), 2);
    assert_eq!(calls.iter().filter(|(m, _)| m == "pim.watch").count(), 1);
    assert!(calls.iter().filter(|(m, _)| m == "pim.events").count() < 15);
}
