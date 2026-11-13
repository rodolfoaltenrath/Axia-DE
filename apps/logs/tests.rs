use super::*;
#[path = "native_tests.rs"]
mod native;
fn app() -> App {
    let mut a = App::default();
    a.resources(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"), None);
    a
}
fn facts() -> Value {
    json!({"cmd":"sync","args":{"__nativeWindow":7},"lang":"en","context":{"width":1040,"height":640,"theme":{"text":"#ffffff","textDim":"#888888","accent":"#0088ff"}}})
}
fn job(a: &App, kind: Kind, now: i64) -> Job {
    Job {
        kind,
        epoch: a.epoch,
        args: json!([]),
        method: "logs.query",
        started: now,
        encoded: String::new(),
    }
}
#[test]
fn session_parser_matches_frozen_original_javascript() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../rust/axia-shell/tests/fixtures/logs_parser.json"
    ))
    .unwrap();
    for c in cases.as_array().unwrap() {
        let got = serde_json::to_value(parse_log(
            c["text"].as_str().unwrap(),
            c["file"].as_str().unwrap(),
        ))
        .unwrap();
        let mut expected = c["expected"].clone();
        for e in expected.as_array_mut().unwrap() {
            e["s"] = json!(0);
        }
        assert_eq!(got, expected);
    }
}
#[test]
fn transport_starts_once_polls_without_spinning_and_retains_window_ownership() {
    let mut a = app();
    let mut reply = a.request_at(&facts(), 1000);
    let mut token = 1;
    let mut methods = vec![];
    while reply["effect"]["method"] == "start" {
        assert_eq!(reply["effect"]["args"][0], 7);
        methods.push(reply["effect"]["args"][1].as_str().unwrap().to_owned());
        reply = a.request_at(&json!({"cmd":"next","result":token}), 1000);
        token += 1;
    }
    assert_eq!(
        methods,
        ["logs.sources", "logs.files", "logs.follow", "logs.query"]
    );
    assert_eq!(a.pending.len(), 4);
    assert!(reply["effect"].is_null());
    assert!(a.request_at(&facts(), 1020)["effect"].is_null());
    let read = a.request_at(&facts(), 1100);
    assert_eq!(read["effect"]["method"], "take");
    a.request_at(&json!({"cmd":"next","result":{"value":[{"token":4,"value":[{"t":100,"id":"test","m":"ready"}]}]}}),1100);
    assert_eq!(a.entries[0].m, "ready");
    assert!(!a.loading);
    assert!(a.request_at(&facts(), 1110)["effect"].is_null());
}
#[test]
fn filter_changes_reject_late_results_and_debounce_text() {
    let mut a = app();
    let old = job(&a, Kind::Full, 1000);
    a.request_at(&json!({"cmd":"search","value":"one"}), 1000);
    a.request_at(&json!({"cmd":"search","value":"two"}), 1100);
    assert_eq!(a.reload_at, Some(1400));
    a.entries = vec![Entry {
        m: "current".into(),
        ..Default::default()
    }];
    a.apply(old, &json!([{"m":"obsolete"}]), None, 1200);
    assert_eq!(a.entries[0].m, "current");
    let current = job(&a, Kind::Full, 1300);
    a.apply(current, &json!([{"m":"two","t":17}]), None, 1400);
    assert_eq!(a.entries[0].m, "two");
    assert_eq!(a.newest, 17);
}
#[test]
fn query_permissions_filters_and_invalid_process_match_preserve_service_contract() {
    let mut a = app();
    a.source = "kernel".into();
    a.level = "warning".into();
    a.process = "bad\nmatch".into();
    a.query = "text".into();
    let q = a.query_args(100_000_000, 0);
    assert_eq!(q["kernel"], true);
    assert_eq!(q["prio"], 4);
    assert!(q["scope"].is_null());
    assert!(q["match"].is_null());
    assert_eq!(q["grep"], "text");
    a.read(false, 1000);
    assert_eq!(a.queue.back().unwrap().method, "logs.system");
    a.queue.clear();
    a.system = true;
    a.process = "axia-maild.service".into();
    a.read(false, 1000);
    assert_eq!(a.queue.back().unwrap().method, "logs.query");
    assert_eq!(a.queue.back().unwrap().args[0]["scope"], "system");
    assert_eq!(a.query_args(1000, 123)["since"], 123);
    assert_eq!(a.query_args(1000, 123)["match"], "axia-maild.service");
    assert!(!valid_match(""));
    assert!(!valid_match(&"a".repeat(96)));
    assert!(valid_match("user@unit:1.service"));
}
#[test]
fn live_session_refresh_keeps_the_process_filter_and_caps_retained_rows() {
    let mut a = app();
    a.source = "lordos".into();
    a.file = "session.log".into();
    a.process = "core".into();
    a.level = "warning".into();
    let j = job(&a, Kind::Live, 0);
    a.apply(j, &json!("other: error\ncore: error\ncore: info"), None, 10);
    assert_eq!(a.entries.len(), 1);
    assert_eq!(a.entries[0].id, "core");
    a.source = "user".into();
    a.entries = vec![Entry::default(); 10_000];
    a.selected = Some(5);
    let j = job(&a, Kind::Live, 0);
    a.apply(j, &json!([{"m":"one","t":1},{"m":"two","t":2}]), None, 10);
    assert_eq!(a.entries.len(), 10_000);
    assert_eq!(a.selected, Some(3));
    assert_eq!(a.newest, 2);
}
#[test]
fn live_events_debounce_and_obsolete_query_errors_do_not_clear_current_rows() {
    let mut a = app();
    a.step = Some(Step::Events);
    a.next(
        &json!({"value":[{"kind":"file","name":"other.log"},{"kind":"journal"}]}),
        1000,
    );
    assert_eq!(a.live_at, Some(1700));
    a.step = Some(Step::Events);
    a.next(&json!({"value":[{"kind":"journal"}]}), 1200);
    assert_eq!(a.live_at, Some(1700));
    let old = job(&a, Kind::Full, 0);
    a.reload(1000, 0);
    a.entries = vec![Entry::default()];
    a.apply(old, &Value::Null, Some("error"), 2000);
    assert_eq!(a.entries.len(), 1);
    a.live = false;
    a.live_at = None;
    a.step = Some(Step::Events);
    a.next(&json!({"value":[{"kind":"journal"}]}), 3000);
    assert!(a.live_at.is_none());
}
fn walk<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(v);
    for c in v["children"].as_array().into_iter().flatten() {
        walk(c, out);
    }
}
#[test]
fn native_scene_virtualizes_long_logs_and_scroll_capture_is_bounded() {
    let mut a = app();
    a.owner = 7;
    a.live = false;
    a.entries = (0..10_000)
        .map(|i| Entry {
            m: format!("message {i}"),
            ..Default::default()
        })
        .collect();
    let r = a.request_at(&facts(), 1000);
    let mut nodes = vec![];
    walk(&r["tree"], &mut nodes);
    assert!(
        nodes
            .iter()
            .filter(|n| n["events"]["onClick"][0] == "select")
            .count()
            < 30
    );
    a.scroll(1e20);
    assert_eq!(a.offset, a.max_offset());
    a.scroll(-1.);
    assert_eq!(a.offset, 0.);
    assert_eq!(
        a.request_at(&json!({"cmd":"dragScroll","y":10}), 1000)["capture"],
        true
    );
    a.request_at(&json!({"cmd":"pointerMove","y":1e9}), 1000);
    assert_eq!(a.offset, a.max_offset());
    assert_eq!(
        a.request_at(&json!({"cmd":"dragEnd"}), 1000)["capture"],
        false
    );
}
#[test]
fn export_uses_current_filtered_snapshot_and_localized_labels() {
    let mut a = app();
    a.home = PathBuf::from("/home/test");
    a.entries = vec![Entry {
        line: 3,
        p: 3,
        id: "core".into(),
        pid: 27,
        m: "first\nsecond".into(),
        ..Default::default()
    }];
    a.export(1_800_000_000_000);
    let j = a.queue.pop_front().unwrap();
    assert_eq!(j.method, "logs.write");
    assert!(j.args[0].as_str().unwrap().starts_with("/home/test/Logs-"));
    assert_eq!(j.args[1], "#3 Error core[27]: first\nsecond\n");
    a.apply(j, &Value::Null, None, 100);
    assert_eq!(a.notice["key"], "logs.exported");
    assert_eq!(a.notice_until, 5100);
}
#[test]
fn errors_keep_the_helper_admin_message_and_failed_export_notice() {
    let mut a = app();
    let j = job(&a, Kind::Full, 0);
    a.apply(j, &Value::Null, Some("administrador required"), 100);
    assert!(a.error.contains("administrator"));
    assert!(!a.loading);
    let j = job(&a, Kind::Export, 0);
    a.apply(j, &Value::Null, Some("read-only"), 100);
    assert_eq!(a.notice["key"], "logs.exportFailed");
}

#[test]
fn large_results_reassemble_before_publication_and_obsolete_work_is_discarded() {
    let mut a = app();
    a.owner = 7;
    a.pending.insert(9, job(&a, Kind::Full, 0));
    a.step = Some(Step::Take);
    a.next(
        &json!({"value":[{"token":9,"part":"[{\"m\":\"日本語 ","done":false}]}),
        100,
    );
    assert!(a.entries.is_empty());
    assert!(a.pending.contains_key(&9));
    a.step = Some(Step::Take);
    a.next(
        &json!({"value":[{"token":9,"part":"😀\"}]","done":true}]}),
        150,
    );
    assert_eq!(a.entries[0].m, "日本語 😀");
    assert!(!a.pending.contains_key(&9));
    a.pending.insert(10, job(&a, Kind::Full, 0));
    a.reload(200, 0);
    let effect = a.pump(300).unwrap();
    assert_eq!(effect["args"][2], json!([10]));
    assert!(!a.pending.contains_key(&10));
}
#[test]
fn empty_live_replies_do_not_move_the_since_cursor_past_unseen_entries() {
    let mut a = app();
    a.newest = 123;
    let j = job(&a, Kind::Live, 0);
    a.apply(j, &json!([]), None, 1000);
    assert_eq!(a.newest, 123);
}
