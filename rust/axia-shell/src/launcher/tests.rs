use super::*;

fn app(name: &str, categories: &str) -> Value {
    json!({"sortKey":name,"heading":name.chars().next().unwrap_or('#').to_uppercase().to_string(),
        "search":[name.to_lowercase()],"categories":categories})
}
fn model(items: Vec<Value>) -> AxiaLauncher {
    let mut state = AxiaLauncher::default();
    state.replace(&json!(items)).unwrap();
    state
}
fn indices(reply: &Value) -> Vec<usize> {
    reply["items"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| {
            if let Some(indices) = r["indices"].as_array() {
                indices
                    .iter()
                    .map(|i| i.as_u64().unwrap() as usize)
                    .collect()
            } else {
                vec![r["index"].as_u64().unwrap() as usize]
            }
        })
        .collect()
}

#[test]
fn categories_follow_existing_precedence_and_aliases() {
    assert_eq!(category("Network", "Office;"), "Network");
    assert_eq!(category("invalid", "Audio;Office;Network;"), "Office");
    assert_eq!(category("", "WebBrowser;FileManager;"), "Network");
    for (alias, expected) in [
        ("Audio", "AudioVideo"),
        ("Video", "AudioVideo"),
        ("TerminalEmulator", "System"),
        ("FileManager", "System"),
        ("TextEditor", "Utility"),
    ] {
        assert_eq!(category("", alias), expected);
    }
    assert_eq!(category("", ";Unknown;;"), "Utility");
}

#[test]
fn stable_unicode_order_and_category_presence() {
    let mut s = model(vec![
        app("Zulu", "Network"),
        app("Árvore", "Office"),
        app("Alpha", ""),
        app("Alpha", "Game"),
        app("😀", "Settings"),
    ]);
    let r = s.view(&json!({"height":1000})).unwrap();
    assert_eq!(indices(&r), [2, 3, 0, 1, 4]);
    assert_eq!(
        r["categories"],
        json!(["Game", "Network", "Office", "Settings", "Utility"])
    );
    assert_eq!(r["first"], 2);
}

#[test]
fn search_matches_each_field_without_cross_field_false_positives() {
    let item = json!({"sortKey":"Editor","heading":"E", "search":["editor","editor original",
        "desktop:org.editor","texto simples","código;programação","editar documentos"],"category":"Utility"});
    let mut s = model(vec![item, app("Navegador", "Network")]);
    for q in [
        "editor original",
        "desktop:org",
        "texto",
        "programação",
        "documentos",
    ] {
        assert_eq!(indices(&s.view(&json!({"query":q})).unwrap()), [0]);
    }
    assert_eq!(
        s.view(&json!({"query":"editor originaldesktop"})).unwrap()["total"],
        0
    );
    // Searching ignores the current category and clearing the query restores it.
    assert_eq!(
        indices(
            &s.view(&json!({"query":"código","category":"Network"}))
                .unwrap()
        ),
        [0]
    );
    assert_eq!(
        indices(&s.view(&json!({"category":"Network"})).unwrap()),
        [1]
    );
}

#[test]
fn empty_and_missing_results_have_no_launch_target() {
    let mut s = model(vec![]);
    for layout in ["grid", "list"] {
        let r = s.view(&json!({"layout":layout,"offset":1e9})).unwrap();
        assert_eq!(r["total"], 0);
        assert!(r["first"].is_null());
        assert!(indices(&r).is_empty());
        assert_eq!(r["offset"], 0.);
    }
    s.replace(&json!([app("Alpha", "")])).unwrap();
    assert_eq!(s.view(&json!({"query":"nothing"})).unwrap()["total"], 0);
}

#[test]
fn grid_keeps_original_geometry_and_last_partial_row() {
    let mut s = model((0..13).map(|i| app(&format!("App {i:04}"), "")).collect());
    let r = s.view(&json!({"height":1000})).unwrap();
    assert_eq!(r["height"], 378.); // 2*16 + 3*98 + 2*26
    assert_eq!(r["items"][0]["top"], 16.);
    assert_eq!(r["items"][1]["top"], 140.);
    assert_eq!(r["items"][2]["indices"], json!([12]));
    assert_eq!(indices(&r), (0..13).collect::<Vec<_>>());
}

#[test]
fn grid_covers_every_visible_row_at_all_scroll_positions() {
    let mut s = model((0..1024).map(|i| app(&format!("App {i:04}"), "")).collect());
    for columns in [1, 3, 6, 9] {
        for height in [100., 480., 900.] {
            for wanted in (0..140000).step_by(137) {
                let r = s
                    .view(&json!({"columns":columns,"height":height,"offset":wanted}))
                    .unwrap();
                let offset = r["offset"].as_f64().unwrap();
                let rows = r["items"].as_array().unwrap();
                for row in 0..1024_usize.div_ceil(columns) {
                    let top = 16. + row as f64 * 124.;
                    if top < offset + height && top + 98. > offset {
                        assert!(
                            rows.iter().any(|r| r["row"] == row),
                            "missing visible row {row} at {offset}"
                        );
                    }
                }
                assert!(rows.len() <= (height / 124.).ceil() as usize + 3);
                assert!(indices(&r).iter().all(|i| *i < 1024));
            }
        }
    }
}

#[test]
fn list_headings_survive_windows_starting_inside_a_group() {
    let mut s = model(vec![
        app("Alpha", ""),
        app("Apple", ""),
        app("Beta", ""),
        app("Bravo", ""),
    ]);
    let all = s.view(&json!({"layout":"list","height":1000})).unwrap();
    assert_eq!(all["height"], 226.);
    assert_eq!(all["items"][0]["heading"], "A");
    assert_eq!(all["items"][1]["heading"], Value::Null);
    assert_eq!(all["items"][2]["top"], 113.);
    assert_eq!(all["items"][2]["heading"], "B");
    let middle = s
        .view(&json!({"layout":"list","height":20,"offset":120}))
        .unwrap();
    assert_eq!(middle["items"][0]["index"], 1);
    assert_eq!(middle["items"][0]["heading"], Value::Null);
    assert_eq!(middle["items"][1]["heading"], "B");
}

#[test]
fn list_covers_visible_items_and_uses_bounded_windows() {
    let mut s = model((0..4096).map(|i| app(&format!("{i:04}"), "")).collect());
    let all = s.view(&json!({"layout":"list","height":16384})).unwrap();
    let total = all["height"].as_f64().unwrap();
    for wanted in (0..200000).step_by(431) {
        let r = s
            .view(&json!({"layout":"list","height":400,"offset":wanted}))
            .unwrap();
        let offset = r["offset"].as_f64().unwrap();
        assert_eq!(r["height"], total);
        assert!(r["items"].as_array().unwrap().len() <= 13);
        for row in &s.list_rows {
            if row.top < offset + 400. && row.top + row.height > offset {
                assert!(indices(&r).contains(&row.index));
            }
        }
    }
    // Scrolling does not rebuild the cached list/filter storage.
    let pointer = s.list_rows.as_ptr();
    let filter = s.matches.as_ptr();
    s.view(&json!({"layout":"list","height":400,"offset":100}))
        .unwrap();
    assert_eq!(pointer, s.list_rows.as_ptr());
    assert_eq!(filter, s.matches.as_ptr());
}

#[test]
fn replacement_and_filter_changes_invalidate_geometry() {
    let mut s = model(vec![app("Alpha", "Game"), app("Beta", "Network")]);
    assert_eq!(s.view(&json!({"layout":"list"})).unwrap()["total"], 2);
    assert_eq!(
        s.view(&json!({"layout":"list","category":"Game"})).unwrap()["height"],
        69.
    );
    s.replace(&json!([app("Zeta", "Office")])).unwrap();
    let r = s.view(&json!({"layout":"list","offset":1e9})).unwrap();
    assert_eq!(indices(&r), [0]);
    assert_eq!(r["categories"], json!(["Office"]));
    assert_eq!(r["items"][0]["heading"], "Z");
    assert_eq!(r["offset"], 0.);
    let resized = s
        .view(&json!({"layout":"list","itemHeight":60,"headerHeight":30}))
        .unwrap();
    assert_eq!(resized["height"], 90.);
}

#[test]
fn invalid_catalog_preserves_previous_snapshot() {
    let mut s = model(vec![app("Keep", "Network")]);
    for bad in [
        json!(null),
        json!([{}]),
        json!([{"sortKey":"bad","heading":"B","search":123}]),
        json!([{"sortKey":"bad","heading":"B","search":vec!["x";7]}]),
        json!([app(&"x".repeat(16385), "")]),
        json!(vec![app("x", ""); 4097]),
    ] {
        assert!(s.replace(&bad).is_err());
        assert_eq!(s.entries[0].sort_key, "Keep");
        assert_eq!(s.view(&json!({})).unwrap()["total"], 1);
    }
}

#[test]
fn malformed_geometry_is_clamped_without_panics() {
    let mut s = model(vec![app("Alpha", "")]);
    for args in [
        json!({"height":-1,"offset":-1,"columns":0}),
        json!({"height":null,"offset":"oops","columns":u64::MAX,"gap":1e300}),
        json!({"layout":"list","height":-1,"offset":1e300,"itemHeight":-1,"headerHeight":-1}),
    ] {
        let r = s.view(&args).unwrap();
        assert!(r["height"].as_f64().unwrap().is_finite());
        assert_eq!(indices(&r), [0]);
    }
    assert!(s.view(&json!({"layout":"unknown"})).is_err());
    assert!(s.view(&json!({"query":"x".repeat(16385)})).is_err());
}

unsafe fn call(state: *mut AxiaLauncher, input: &str) -> Value {
    let input = CString::new(input).unwrap();
    let reply = axia_launcher_request(state, input.as_ptr());
    assert!(!reply.is_null());
    let result = serde_json::from_slice(CStr::from_ptr(reply).to_bytes()).unwrap();
    axia_launcher_reply_free(reply);
    result
}

#[test]
fn ffi_replies_own_their_memory_and_contexts_are_independent() {
    unsafe {
        let a = axia_launcher_new();
        let b = axia_launcher_new();
        let input = json!({"op":"catalog","items":[app("A\0B", "Network")]}).to_string();
        assert!(call(a, &input).get("error").is_none());
        assert_eq!(call(a, r#"{"op":"view","query":"\u0000"}"#)["total"], 1);
        assert_eq!(call(b, r#"{"op":"view"}"#)["total"], 0);
        assert!(call(a, "not JSON")["error"].is_string());
        assert!(call(a, r#"{"op":"oops"}"#)["error"].is_string());
        assert!(call(std::ptr::null_mut(), "{}")["error"].is_string());
        let reply = axia_launcher_request(a, std::ptr::null());
        assert!(!reply.is_null());
        axia_launcher_reply_free(reply);
        assert!(call(a, &" ".repeat(MAX_REQUEST + 1))["error"].is_string());
        axia_launcher_free(a);
        axia_launcher_free(b);
        axia_launcher_free(std::ptr::null_mut());
        axia_launcher_reply_free(std::ptr::null_mut());
    }
}

#[test]
#[ignore = "manual release-mode measurement; no timing threshold on CI hosts"]
fn measure_large_catalog() {
    use std::time::Instant;
    for count in [128, 1024, 4096] {
        let started = Instant::now();
        let mut s = model(
            (0..count)
                .rev()
                .map(|i| app(&format!("App {i:04}"), "Utility"))
                .collect(),
        );
        let preparation = started.elapsed();
        let request = CString::new(r#"{"op":"view","height":510,"offset":900}"#).unwrap();
        let started = Instant::now();
        for _ in 0..1000 {
            unsafe {
                axia_launcher_reply_free(axia_launcher_request(&mut s, request.as_ptr()));
            }
        }
        let avg = started.elapsed().as_secs_f64() * 1000. / 1000.;
        let r = s.view(&json!({"height":510,"offset":900})).unwrap();
        println!("launcher: {count} apps; prepare={preparation:?}; view incl JSON={avg:.3} ms; mounted={}", indices(&r).len());
    }
}
