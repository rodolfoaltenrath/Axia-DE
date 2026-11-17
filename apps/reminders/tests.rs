use super::*;
#[path = "native_tests.rs"]
mod native;
fn app() -> App {
    let mut a = App::default();
    a.resources(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    a
}
fn lists() -> Vec<Value> {
    vec![
        json!({"id":"a","name":"Personal","color":"#007aff","kind":"local","todos":true}),
        json!({"id":"b","name":"Work","color":"#ff9500","kind":"dav","todos":true}),
    ]
}
fn date(s: &str) -> i64 {
    parse_day(s).unwrap()
}
fn sample() -> Vec<Value> {
    vec![
        json!({"uid":"later","list":"a","title":"Later","due":date("2026-11-19"),"done":false,"flag":true}),
        json!({"uid":"child","list":"a","title":"Child","parent":"parent","due":date("2026-11-16"),"done":false}),
        json!({"uid":"parent","list":"a","title":"Parent","due":date("2026-11-17"),"dueAllDay":true,"done":false}),
        json!({"uid":"old","list":"b","title":"Older","due":date("2026-11-15"),"done":false}),
        json!({"uid":"undated","list":"b","title":"Undated","due":null,"done":false,"prio":9}),
        json!({"uid":"done-old","list":"a","title":"Done old","done":true,"doneAt":10}),
        json!({"uid":"done-new","list":"b","title":"Done new","done":true,"doneAt":20}),
    ]
}
fn complete(a: &mut App, tag: Tag, result: Result<Value, String>) {
    a.apply(Completed { tag, result }, date("2026-11-17"));
}
#[test]
fn smart_lists_counts_subtasks_and_sections_match_original() {
    let now = date("2026-11-17");
    let items = sample();
    assert_eq!(
        counts(&items, now),
        json!({"all":5,"today":3,"scheduled":4,"flagged":1,"completed":2,"lists":{"a":3,"b":2}})
    );
    let ids = |rows: Vec<(Value, usize)>| {
        rows.iter()
            .map(|(it, d)| (string(&it["uid"]).to_owned(), *d))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(sort_items("today", items.clone())),
        vec![
            ("old".into(), 0),
            ("parent".into(), 0),
            ("child".into(), 1),
            ("later".into(), 0),
            ("undated".into(), 0),
            ("done-new".into(), 0),
            ("done-old".into(), 0)
        ]
    );
    assert_eq!(
        ids(sort_items("list:a", items[..3].to_vec())),
        vec![
            ("later".into(), 0),
            ("parent".into(), 0),
            ("child".into(), 1)
        ]
    );
    let scheduled = sections("scheduled", &lists(), &items, false, now);
    assert_eq!(
        scheduled.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["overdue", "2026-11-17", "2026-11-19"]
    );
    assert_eq!(scheduled[0].rows.len(), 2);
    assert!(matches("today", &items[3], now));
    assert!(!matches("today", &items[0], now));
    assert!(!overdue(&items[2], now));
    assert!(overdue(&items[2], add_days(now, 1)));
    assert_eq!(
        [0, 1, 2, 5, 6, 9].map(marks),
        ["", "!!!", "!!", "!!", "!", "!"]
    );
}
#[test]
fn calendar_dates_roundtrip_and_month_grid_dont_add_fixed_24_hour_days() {
    for s in ["2024-02-29", "2026-03-08", "2026-11-01", "2026-12-31"] {
        assert_eq!(ymd(date(s)), s);
    }
    assert_eq!(ymd(add_days(date("2026-12-31"), 1)), "2027-01-01");
    assert_eq!(ymd(month(date("2026-12-31"), 1)), "2027-01-01");
    assert_eq!(ymd(month(date("2026-01-31"), 1)), "2026-02-01");
    assert_eq!(ymd(date("2026-02-31")), "2026-03-03");
    assert!(parse_day("invalid").is_none());
    let t = crate::clock::local(at_minutes(date("2026-11-17"), 547));
    assert_eq!((t.tm_hour, t.tm_min), (9, 7));
}
#[test]
fn refresh_generation_and_failures_preserve_cached_lists() {
    let mut a = app();
    a.epoch = 2;
    complete(&mut a, Tag::Lists(1), Ok(json!({"cals":lists()})));
    assert!(a.lists.is_empty());
    complete(&mut a, Tag::Lists(2), Err("unavailable".into()));
    assert!(a.unavailable);
    complete(&mut a, Tag::Lists(2), Ok(json!({"cals":lists()})));
    assert!(!a.unavailable);
    assert!(a.loaded);
    complete(&mut a, Tag::Lists(2), Err("unavailable".into()));
    assert!(!a.unavailable);
    assert_eq!(a.lists.len(), 2);
    complete(&mut a, Tag::Items(1), Ok(json!({"todos":sample()})));
    assert!(a.items.is_empty());
    complete(&mut a, Tag::Items(2), Ok(json!({"todos":sample()})));
    assert_eq!(a.items.len(), 7);
    a.sel = "list:gone".into();
    complete(&mut a, Tag::Lists(2), Ok(json!({"cals":lists()})));
    assert_eq!(a.sel, "today");
}
#[test]
fn inspector_dates_priorities_notes_flags_move_and_cancellation() {
    let mut a = app();
    a.lists = lists();
    a.items = sample();
    let now = date("2026-11-17");
    a.inspect("a/parent", now);
    assert_eq!(a.inspector.as_ref().unwrap().draft["mode"], "date");
    for (k, v) in [
        ("title", json!("  Renamed  ")),
        ("notes", json!("First\nSecond")),
        ("prio", json!(1)),
        ("list", json!("b")),
        ("mode", json!("time")),
        ("min", json!(547)),
    ] {
        a.request_at(&json!({"cmd":"draft","key":k,"value":v}), now);
    }
    a.request_at(&json!({"cmd":"toggleFlag"}), now);
    assert_eq!(a.inspector.as_ref().unwrap().draft["flag"], true);
    a.notes_focus = true;
    a.key("Backspace", "", 0);
    a.paste("界");
    assert!(string(&a.inspector.as_ref().unwrap().draft["notes"]).ends_with('界'));
    a.key("Escape", "", 0);
    assert!(!a.notes_focus);
    a.editing = Some(("a/parent".into(), "should cancel".into()));
    a.key("Escape", "", 0);
    assert!(a.editing.is_none());
    a.save_inspector();
    assert!(a.inspector.is_none());
    assert!(a.jobs.busy());
}
#[test]
fn original_scene_preserves_layout_and_native_icon() {
    let mut a = app();
    a.lists = lists();
    a.items = sample();
    a.items_loaded = true;
    a.theme = json!({"mode":"light","text":"#111111","textDim":"#777777","windowBg":"#ffffff","sidebarBg":"#eeeeee","separator":"#cccccc","accent":"#007aff","control":"#dddddd"});
    let tree = ui::tree(&mut a, date("2026-11-17"));
    assert_eq!(tree["children"][0]["style"]["width"], 236);
    assert_eq!(tree["children"][1]["children"][0]["style"]["height"], 44);
    assert_eq!(
        tree["children"][0]["children"][1]["children"][0]["children"][0]["children"][0]["style"]
            ["width"],
        104
    );
    for expected in [
        "Personal",
        "Work",
        "Parent",
        "Child",
        "Older",
        "reminders.newReminder",
    ] {
        assert!(tree.to_string().contains(expected), "{expected}");
    }
    assert!(!tree.to_string().contains("Later"));
    a.inspect("a/parent", date("2026-11-17"));
    let tree = ui::tree(&mut a, date("2026-11-17"));
    assert!(tree.to_string().contains("\"width\":340.0"));
    a.picker = "day".into();
    a.picker_month = date("2026-11-01");
    let tree = ui::tree(&mut a, date("2026-11-17"));
    assert_eq!(tree.to_string().matches("day-2026").count(), 42);
    let icon = ui::icon(64.);
    assert_eq!(icon["children"].as_array().unwrap().len(), 3);
    assert_eq!(
        icon["children"][2]["children"][1]["style"]["width"],
        64. * 0.3
    );
}
