use super::*;

fn app() -> App {
    let mut a = App::default();
    a.resources(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    a.state.cursor = date::parse("2026-11-17").unwrap();
    a.state.cals =
        vec![json!({"id":"a","name":"Personal","kind":"local","events":true,"color":"#007aff"})];
    a
}
fn find<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    if v["key"] == k {
        return Some(v);
    }
    v["children"].as_array()?.iter().find_map(|v| find(v, k))
}
fn event() -> Value {
    let start = date::at_minutes(date::parse("2026-11-17").unwrap(), 540);
    json!({"uid":"e","cal":"a","title":"Meeting","start":start,"end":start+3600000,"allDay":false,"rrule":"FREQ=WEEKLY;BYDAY=TU","occ":start,"alarms":[7,15],"att":[{"email":"a@b","name":"Alice","ps":"ACCEPTED"}],"org":{"email":"org@b","name":"Organizer"}})
}
#[test]
fn day_week_month_and_narrow_sidebar_use_native_scenes() {
    let mut a = app();
    for view in ["day", "week", "month"] {
        a.state.view = view.into();
        let tree = ui::tree(&mut a, 0);
        assert!(find(
            &tree,
            if view == "month" {
                "month-grid"
            } else {
                "time-grid"
            }
        )
        .is_some());
    }
    a.width = 640.;
    let tree = ui::tree(&mut a, 0);
    assert!(find(&tree, "sidebar").is_none());
    a.request_at(&json!({"cmd":"sidebar"}), 0);
    assert!(a.state.narrow_sidebar);
    assert!(a.state.sidebar);
    assert!(find(&ui::tree(&mut a, 0), "sidebar").is_some());
}
#[test]
fn slots_keep_half_hour_actions_and_month_more_opens_day() {
    let mut a = app();
    a.state.view = "day".into();
    let tree = ui::tree(&mut a, 0);
    let slot = find(&tree, "slot-19").unwrap();
    assert_eq!(slot["events"]["onClick"][1]["min"], 570);
    a.state.view = "month".into();
    a.height = 440.;
    a.state.events = (0..8)
        .map(|i| {
            let mut e = event();
            e["uid"] = json!(i.to_string());
            e
        })
        .collect();
    a.state.days = by_day(&a.state.events);
    let tree = ui::tree(&mut a, 0);
    let cell = find(find(&tree, "month-grid").unwrap(), "2026-11-17").unwrap();
    assert_eq!(
        find(cell, "more").unwrap()["events"]["onClick"],
        json!(["day", a.state.cursor])
    );
}
#[test]
fn editor_keeps_custom_recurrence_alert_and_participation() {
    let mut a = app();
    let e = event();
    a.state.events = vec![e.clone()];
    a.request_at(&json!({"cmd":"event","key":key(&e)}), 0);
    let tree = ui::tree(&mut a, 0);
    let body = tree.to_string();
    assert!(body.contains("Alice"));
    assert!(body.contains("ps.accepted"));
    assert!(body.contains("seriesNote"));
    a.request_at(&json!({"cmd":"picker","key":"rrule"}), 0);
    let tree = ui::tree(&mut a, 0);
    assert!(find(&tree, "options")
        .unwrap()
        .to_string()
        .contains("FREQ=WEEKLY;BYDAY=TU"));
    a.request_at(&json!({"cmd":"pick","key":{"field":"alarm","value":7}}), 0);
    assert_eq!(a.state.event.as_ref().unwrap().draft["alarm"], 7);
    assert!(a.picker.is_empty());
}
#[test]
fn keyboard_and_unicode_notes_obey_modal_focus() {
    let mut a = app();
    a.request_at(&json!({"cmd":"new"}), 0);
    a.request_at(&json!({"cmd":"notesFocus"}), 0);
    a.key_at("x", "🙂", 0, 1);
    a.key_at("Backspace", "", 0, 2);
    a.paste_at("hello", 3);
    a.key_at("Enter", "", 0, 4);
    a.key_at("x", "世界", 0, 5);
    assert_eq!(
        a.state.event.as_ref().unwrap().draft["notes"],
        "hello\n世界"
    );
    a.key_at("Escape", "", 0, 6);
    a.key_at("Escape", "", 0, 7);
    assert!(a.state.event.is_none());
    a.key_at("3", "", 2, 8);
    assert_eq!(a.state.view, "month");
    let cursor = a.state.cursor;
    a.rtl = true;
    a.key_at("ArrowLeft", "", 0, 9);
    assert_eq!(a.state.cursor, date::month(cursor, 1));
}
#[test]
fn calendar_color_and_date_picker_actions_pass_structured_values() {
    let mut a = app();
    a.request_at(&json!({"cmd":"calNew"}), 0);
    a.request_at(
        &json!({"cmd":"calDraft","key":{"field":"color","value":"#ff9500"}}),
        0,
    );
    assert_eq!(a.state.calendar.as_ref().unwrap().color, "#ff9500");
    a.request_at(&json!({"cmd":"new"}), 0);
    a.request_at(&json!({"cmd":"picker","key":"sDay"}), 0);
    let tree = ui::tree(&mut a, 0);
    assert!(find(&tree, "date-picker").is_some());
    let day = date::parse("2026-11-20").unwrap();
    a.request_at(&json!({"cmd":"pick","key":{"field":"sDay","value":day}}), 0);
    assert_eq!(a.state.event.as_ref().unwrap().draft["sDay"], day);
    assert_eq!(a.state.event.as_ref().unwrap().draft["eDay"], day);
}
