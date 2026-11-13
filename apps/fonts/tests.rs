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
fn complete(a: &mut App, tag: Tag, value: Value, now: i64) {
    a.apply(
        Completed {
            tag,
            result: Ok(value),
        },
        now,
    );
}
#[test]
fn grouping_deduplicates_faces_preserves_variants_and_only_marks_user_paths() {
    let list = json!([
        {"family":"Example","path":"/usr/share/fonts/a.ttc","index":0,"style":"Italic","italic":true,"weight":400},
        {"family":"Example","path":"/usr/share/fonts/a.ttc","index":1,"style":"Regular","italic":false,"weight":400,"mono":true},
        {"family":"Example","path":"/usr/share/fonts/a.ttc","index":1,"style":"Regular","italic":false,"weight":400},
        {"family":"User","path":"/home/test/.local/share/fonts/b.ttf","index":0,"color":true},
        {"family":"Missing","path":""}
    ]);
    let groups = group_fonts(&list, "/home/test");
    let example = groups.iter().find(|f| f.name == "Example").unwrap();
    assert_eq!(example.styles.len(), 2);
    assert_eq!(example.styles[0]["index"], 1);
    assert!(example.mono);
    assert!(!example.user);
    assert!(groups.iter().find(|f| f.name == "User").unwrap().user);
    assert!(!is_user(
        "/home/test/.local/share/fonts-other/a.ttf",
        "/home/test"
    ));
    assert!(valid_font("/home/test/a.OTC"));
    assert!(!valid_font("/home/test/a.ttf.exe"));
}
#[test]
fn codepoint_chunks_match_frozen_legacy_character_map() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../rust/axia-shell/tests/fixtures/fonts_chars.json"
    ))
    .unwrap();
    let mut a = app();
    for c in cases.as_array().unwrap() {
        a.ranges = serde_json::from_value(c["ranges"].clone()).unwrap();
        let idx = c["block"].as_u64().unwrap() as usize;
        a.block = a.blocks().iter().position(|i| *i == idx).unwrap();
        a.chunk = c["chunk"].as_u64().unwrap() as usize;
        assert_eq!(json!(a.codepoints()), c["expected"]);
    }
}
#[test]
fn stale_metadata_and_rasters_cannot_replace_a_new_font() {
    let mut a = app();
    a.font_epoch = 3;
    a.render_epoch = 5;
    a.grid_epoch = 7;
    a.tab = "sample".into();
    a.font = Some(json!({"path":"/home/test/a.ttf","index":0}));
    complete(
        &mut a,
        Tag::Info(2),
        json!({"family":"old","ranges":[[32,127]]}),
        100,
    );
    assert!(a.info.is_none());
    complete(
        &mut a,
        Tag::Sample(4, 0),
        json!({"image":42,"w":200,"h":60}),
        100,
    );
    assert!(a.samples[0].is_none());
    complete(
        &mut a,
        Tag::Sample(5, 0),
        json!({"image":43,"w":200,"h":60}),
        100,
    );
    assert_eq!(a.samples[0].as_ref().unwrap().id, 43);
    a.sample_changed(200);
    assert!(a.samples[0].is_none());
    assert_eq!(a.sample_at, Some(260));
}
#[test]
fn character_selection_requires_present_glyphs_and_copy_handles_non_bmp() {
    let mut a = app();
    a.ranges = vec![[0x1f300, 0x1faff]];
    a.block = 0;
    a.tab = "chars".into();
    a.grid = Some(Image {
        id: 12,
        w: 640.,
        h: 640.,
        present: vec![false, true],
    });
    a.request_at(&json!({"cmd":"glyph","key":0}), 100);
    assert!(a.pick.is_none());
    a.request_at(&json!({"cmd":"glyph","key":1}), 100);
    assert_eq!(a.pick, Some(0x1f301));
    let reply = a.request_at(&json!({"cmd":"copy"}), 100);
    assert_eq!(reply["effect"]["text"], "🌁");
    assert_eq!(reply["effect"]["host"], true);
    a.grid_changed();
    assert!(a.pick.is_none());
    assert!(a.grid.is_none());
}

fn file_app() -> App {
    let mut a = app();
    a.booted = true;
    a.jobs.bind(7);
    a.file_path = "/home/test/Downloads/New.ttf".into();
    a.file_font = Some(Family {
        name: "New".into(),
        styles: vec![json!({"path":a.file_path,"index":0,"style":"Regular"})],
        mono: false,
        color: false,
        user: false,
        file: true,
    });
    a.selection = Some("file".into());
    a
}
fn fulfill(a: &mut App, method: &str, result: Result<Value, &str>, now: &mut i64) -> Value {
    let effect = a.jobs.pump(*now).expect("queued service call");
    assert_eq!(effect["method"], "start");
    assert_eq!(effect["args"][1], method);
    let args = effect["args"][2].clone();
    assert!(a.jobs.complete(&json!(91), *now).is_empty());
    *now += 100;
    assert_eq!(a.jobs.pump(*now).unwrap()["method"], "take");
    let packet = match result {
        Ok(value) => json!({"token":91,"value":value}),
        Err(error) => json!({"token":91,"error":error}),
    };
    let done = a.jobs.complete(&json!({"value":[packet]}), *now);
    for done in done {
        a.apply(done, *now);
    }
    *now += 100;
    args
}
#[test]
fn install_waits_for_successful_copy_before_rescanning_and_clearing_file() {
    let mut a = file_app();
    let mut now = 1000;
    a.install();
    assert_eq!(
        fulfill(&mut a, "fs.mkdir", Ok(Value::Null), &mut now)[0],
        "/home/test/.local"
    );
    fulfill(&mut a, "fs.mkdir", Err("EEXIST"), &mut now);
    fulfill(&mut a, "fs.stat", Ok(json!({"dir":true})), &mut now);
    fulfill(&mut a, "fs.mkdir", Ok(Value::Null), &mut now);
    fulfill(&mut a, "fs.stat", Err("ENOENT"), &mut now);
    let copy = fulfill(&mut a, "fs.copy", Ok(json!(77)), &mut now);
    assert_eq!(
        copy,
        json!([
            "/home/test/Downloads/New.ttf",
            "/home/test/.local/share/fonts/New.ttf"
        ])
    );
    assert!(a.install.is_some());
    assert!(!a.file_path.is_empty());
    assert_eq!(a.install.as_ref().unwrap().job, Some(77));
    complete(
        &mut a,
        Tag::CopyPoll,
        json!([{"id":77,"done":false,"failed":false}]),
        now,
    );
    assert!(a.install.is_some());
    complete(
        &mut a,
        Tag::CopyPoll,
        json!([{"id":77,"done":true,"failed":false}]),
        now,
    );
    let forget = a.jobs.pump(now).unwrap();
    assert_eq!(forget["args"][1], "fs.forgetJob");
    a.jobs.complete(&json!(91), now);
    let list = a.jobs.pump(now).unwrap();
    assert_eq!(list["args"][1], "fonts.list");
    a.jobs.complete(&json!(92), now);
    now += 100;
    a.jobs.pump(now);
    for done in a.jobs.complete(&json!({"value":[{"token":91,"value":null},{"token":92,"value":[{"family":"New","style":"Regular","path":"/home/test/.local/share/fonts/New.ttf","index":0}]}]}),now){a.apply(done,now);}
    assert!(a.install.is_none());
    assert!(a.file_path.is_empty());
    assert_eq!(a.selection.as_deref(), Some("New"));
    assert_eq!(a.notice["key"], "fonts.installed");
}
#[test]
fn existing_files_and_failed_copies_never_report_a_successful_install() {
    let mut a = file_app();
    a.install = Some(Installation {
        source: a.file_path.clone(),
        dest: "/home/test/.local/share/fonts/New.ttf".into(),
        name: "New".into(),
        job: Some(12),
        poll_at: 0,
    });
    complete(&mut a, Tag::Destination, json!({"size":10}), 100);
    assert!(a.install.is_none());
    assert_eq!(a.notice["key"], "fonts.already");
    a.install = Some(Installation {
        source: a.file_path.clone(),
        dest: "/home/test/.local/share/fonts/New.ttf".into(),
        name: "New".into(),
        job: Some(12),
        poll_at: 0,
    });
    complete(
        &mut a,
        Tag::CopyPoll,
        json!([{"id":12,"done":true,"failed":true,"error":"Permission denied"}]),
        200,
    );
    assert!(a.install.is_none());
    assert_eq!(a.notice["key"], "fonts.error");
    assert!(!a.file_path.is_empty());
}
#[test]
fn removing_a_mixed_family_trashes_only_user_files_once_per_collection() {
    let mut a = app();
    a.jobs.bind(7);
    a.families = group_fonts(
        &json!([
            {"family":"Mixed","path":"/usr/share/fonts/System.ttf","index":0},
            {"family":"Mixed","path":"/home/test/.fonts/User.ttc","index":0},
            {"family":"Mixed","path":"/home/test/.fonts/User.ttc","index":1}
        ]),
        &a.home,
    );
    a.remove();
    let mut now = 100;
    assert_eq!(
        fulfill(&mut a, "fs.trash", Ok(Value::Null), &mut now),
        json!(["/home/test/.fonts/User.ttc"])
    );
    fulfill(
        &mut a,
        "fonts.list",
        Ok(json!([{"family":"Mixed","path":"/usr/share/fonts/System.ttf","index":0}])),
        &mut now,
    );
    assert!(a.removal.is_none());
    assert!(!a.families[0].user);
    assert_eq!(a.notice["key"], "fonts.removed");
    a.jobs.cancel(|_| true);
    a.remove();
    assert!(a.removal.is_none());
}
#[test]
fn collections_style_changes_and_nfc_order_preserve_selection_rules() {
    let mut a = app();
    a.families = group_fonts(
        &json!([
            {"family":"E\u{301}","path":"/home/test/.fonts/A.ttf","index":0,"mono":true},
            {"family":"Z","path":"/usr/share/fonts/Z.ttf","index":0,"color":true},
            {"family":"É","path":"/usr/share/fonts/B.ttf","index":0}
        ]),
        &a.home,
    );
    assert_eq!(
        a.families
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["Z", "E\u{301}", "É"]
    );
    a.request_at(&json!({"cmd":"collection","key":"user"}), 100);
    assert_eq!(a.visible().len(), 1);
    a.request_at(&json!({"cmd":"search","value":"no such font"}), 100);
    assert!(a.family().is_none());
    a.request_at(&json!({"cmd":"collection","key":"color"}), 100);
    a.request_at(&json!({"cmd":"search","value":""}), 100);
    assert_eq!(a.family().unwrap().name, "Z");
}
#[test]
fn slider_clamps_and_grid_is_usable_at_minimum_window_width() {
    let mut a = app();
    a.width = 640.;
    assert!(230. + 48. + a.block_width() + 16. + a.cell() * 16. <= a.width);
    a.size_from_x(-1e9, 100);
    assert_eq!(a.size, 8.);
    a.size_from_x(1e9, 100);
    assert_eq!(a.size, 120.);
    assert_eq!(
        a.request_at(&json!({"cmd":"sizeDown","x":a.width-230.}), 100)["capture"],
        true
    );
    assert_eq!(
        a.request_at(&json!({"cmd":"dragEnd"}), 100)["capture"],
        false
    );
}
