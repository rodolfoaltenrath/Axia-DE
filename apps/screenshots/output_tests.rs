use super::*;
fn locale(lang: &str) -> I18n {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("assets");
    let mut i18n = I18n::new(assets.to_str().unwrap(), None);
    i18n.lang = lang.into();
    i18n
}
fn next(outputs: &mut Outputs, reply: &Value, result: Value) -> Value {
    outputs.next(&json!({"id":reply["id"],"step":reply["step"],"result":result}))
}
fn stat(outputs: &mut Outputs, mut reply: Value) -> Value {
    while reply["effect"]["op"] != "stat" {
        assert!(reply.get("error").is_none(), "{reply}");
        reply = next(
            outputs,
            &reply,
            if reply["effect"]["op"] == "dirs" {
                json!({"pictures":"/home/test/My Pictures"})
            } else {
                json!({"error":"fs.exists"})
            },
        );
    }
    reply
}
#[test]
fn translated_names_unicode_annotations_and_concurrent_missing_stats_remain_distinct() {
    let mut outputs = Outputs::default();
    let mut pt = locale("pt");
    let one = outputs.begin(&json!({"kind":"image"}), &json!({}), &mut pt, 0);
    let one = stat(&mut outputs, one);
    assert!(one["effect"]["path"]
        .as_str()
        .unwrap()
        .starts_with("/home/test/My Pictures/Capturas/Captura de tela "));
    let two = outputs.begin(&json!({"kind":"image"}), &json!({}), &mut pt, 0);
    let two = stat(&mut outputs, two);
    assert_eq!(one["effect"]["path"], two["effect"]["path"]);
    let ready = next(&mut outputs, &one, json!({"exists":false}));
    let collision = next(&mut outputs, &two, json!({"exists":false}));
    assert!(collision["effect"]["path"]
        .as_str()
        .unwrap()
        .ends_with(" (2).png"));
    let second = next(&mut outputs, &collision, json!({"exists":false}));
    assert_ne!(ready["target"]["path"], second["target"]["path"]);
    for source in [
        "/original/ação.PNG",
        "/original/💙🦀",
        "/original/abcá",
        "/original/áaá",
    ] {
        let plan = outputs.begin(
            &json!({"kind":"annotated","source":source}),
            &json!({"clipboard":true}),
            &mut pt,
            0,
        );
        let plan = stat(&mut outputs, plan);
        let path = plan["effect"]["path"].as_str().unwrap();
        assert!(path.ends_with(" (anotada).png"));
        assert_ne!(path, source);
        assert!(!path.contains(".PNG ("));
    }
}
#[test]
fn clipboard_and_completed_failures_do_not_become_saved_paths_or_keep_reservations() {
    let mut outputs = Outputs::default();
    let mut en = locale("en");
    let copied = outputs.begin(
        &json!({"kind":"image"}),
        &json!({"clipboard":true}),
        &mut en,
        0,
    );
    assert!(copied.get("effect").is_none());
    assert_eq!(copied["target"]["clipboard"], true);
    let result = outputs.complete(&json!({"id":copied["id"],"result":{"ok":false}}));
    assert_eq!(result["ok"], false);
    assert!(result["path"].is_null());
    let begin = outputs.begin(&json!({"kind":"image"}), &json!({}), &mut en, 0);
    let checked = stat(&mut outputs, begin);
    let ready = next(&mut outputs, &checked, json!({"exists":false}));
    let failed = outputs.complete(
        &json!({"id":ready["id"],"result":{"ok":false,"path":"/should-not-be-saved.png"}}),
    );
    assert!(failed["path"].is_null());
    assert!(outputs.reserved.is_empty());
    assert!(outputs
        .complete(&json!({"id":ready["id"],"result":{"ok":true}}))
        .get("error")
        .is_some());
    let begin = outputs.begin(
        &json!({"kind":"record"}),
        &json!({"clipboard":true,"mic":true}),
        &mut en,
        0,
    );
    let checked = stat(&mut outputs, begin);
    let ready = next(&mut outputs, &checked, json!({"exists":false}));
    assert_eq!(ready["target"]["clipboard"], false);
    assert_eq!(ready["target"]["mic"], true);
    assert!(ready["target"]["path"].as_str().unwrap().ends_with(".mp4"));
    outputs.release(ready["id"].as_u64().unwrap());
    assert!(outputs.jobs.is_empty());
}
#[test]
fn abandoning_a_pending_stat_cannot_release_another_exports_chosen_name() {
    let mut outputs = Outputs::default();
    let mut en = locale("en");
    let first = outputs.begin(&json!({"kind":"image"}), &json!({}), &mut en, 0);
    let first = stat(&mut outputs, first);
    let pending = outputs.begin(&json!({"kind":"image"}), &json!({}), &mut en, 0);
    let pending = stat(&mut outputs, pending);
    let chosen = next(&mut outputs, &first, json!({"exists":false}));
    outputs.release(pending["id"].as_u64().unwrap());
    let third = outputs.begin(&json!({"kind":"image"}), &json!({}), &mut en, 0);
    let third = stat(&mut outputs, third);
    assert_ne!(third["effect"]["path"], chosen["target"]["path"]);
    outputs.release(chosen["id"].as_u64().unwrap());
    outputs.release(third["id"].as_u64().unwrap());
    assert!(outputs.reserved.is_empty());
}
#[test]
fn numbered_collisions_are_bounded_and_old_callbacks_cannot_advance_or_finish_a_new_job() {
    let mut outputs = Outputs::default();
    let mut en = locale("en");
    let mut reply = outputs.begin(&json!({"kind":"image"}), &json!({}), &mut en, 0);
    let old = reply.clone();
    reply = stat(&mut outputs, reply);
    assert!(outputs
        .next(&json!({"id":old["id"],"step":old["step"],"result":{}}))
        .get("error")
        .is_some());
    for _ in 0..999 {
        reply = next(&mut outputs, &reply, json!({"exists":true}));
    }
    assert!(reply.get("effect").is_none());
    assert!(reply["target"]["path"]
        .as_str()
        .unwrap()
        .ends_with(" 0.png"));
    outputs.release(reply["id"].as_u64().unwrap());
    let new = outputs.begin(&json!({"kind":"image"}), &json!({}), &mut en, 0);
    assert_ne!(old["id"], new["id"]);
    assert!(outputs
        .complete(&json!({"id":old["id"],"result":{"ok":true}}))
        .get("error")
        .is_some());
    let invalid = next(&mut outputs, &new, json!({"pictures":"relative/path"}));
    assert!(invalid.get("error").is_some());
    assert!(outputs.jobs.is_empty());
}
