use super::*;

fn app(name: &str, categories: &str) -> Value {
    json!({"id":name,"name":name,"categories":categories,"kind":"lordos"})
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
    let item = json!({"id":"desktop:org.editor","name":"Editor original","displayName":"Editor","genericName":"texto simples",
        "keywords":"código;programação","comment":"editar documentos","category":"Utility"});
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
        json!([{"id":"bad","name":123}]),
        json!([{"id":"","name":"bad"}]),
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
        let input = json!({"op":"sync","native":[app("A\0B", "Network")],"linux":[]}).to_string();
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
fn dock_policy_is_reachable_through_the_production_ffi_without_a_catalog() {
    unsafe {
        let state = axia_launcher_new();
        assert_eq!(
            call(
                state,
                r#"{"op":"dock","cmd":"parse","id":"desktop:firefox"}"#
            ),
            json!({"kind":"desktop","key":"firefox"})
        );
        assert_eq!(
            call(
                state,
                r#"{"op":"dock","cmd":"pin","list":["app:files","app:last"],"id":"lpk:notes","index":0,"shown":["app:files","app:last"]}"#
            ),
            json!(["app:files", "lpk:notes", "app:last"])
        );
        assert!(call(state, r#"{"op":"dock","cmd":"unknown"}"#)["error"].is_string());
        assert_eq!(call(state, r#"{"op":"catalog"}"#)["source"], json!([]));
        axia_launcher_free(state);
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

fn shell() -> AxiaLauncher {
    let mut s = AxiaLauncher::default();
    s.request(&json!({"op":"sync","native":[
        {"id":"files","name":"Files","category":"System"},
        {"id":"terminal","name":"Terminal","category":"System"},
        {"id":"other","name":"Other","category":"Utility"},
        {"id":"package","name":"Package","package":true}],"linux":[
        {"id":"org.example.Editor","name":"Text editor","wmClass":"ExampleEditor","mimeTypes":"text/plain;","icon":"/icon.png"}]})).unwrap();
    s
}
fn context() -> Value {
    json!({"theme":{"mode":"dark","effects":true,"text":"#fff","windowBorder":"#444","tileBorder":"#444"},
    "area":{"x":0,"y":0,"w":1000,"h":700,"left":0,"top":0,"width":640,"height":700},"settings":{},"session":{}})
}
fn open_menu(s: &mut AxiaLauncher, mode: &str) -> u64 {
    s.request(&json!({"op":"open","mode":mode,"context":context()}))
        .unwrap()["id"]
        .as_u64()
        .unwrap()
}
fn event(s: &mut AxiaLauncher, id: u64, action: Value) -> Value {
    s.request(&json!({"op":"event","id":id,"action":action}))
        .unwrap()
}
fn effect(reply: &Value, name: &str) -> Option<Value> {
    reply["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == name)
        .cloned()
}

#[test]
fn raw_catalog_owns_translation_visibility_and_linux_identity() {
    let mut s = AxiaLauncher::default();
    let r=s.request(&json!({"op":"sync","native":[
        {"id":"files","name":"Files"},{"id":"terminal","name":"Console"},
        {"id":"hidden","name":"Hidden","hidden":true},{"id":"linux","name":"Internal","builtin":true},
        {"id":"installer","name":"Install","live":true},{"id":"pack","name":"Package","package":true}],
        "linux":[{"id":"org.example.App","name":"External","mimeTypes":"application/pdf;"}],
        "names":"{\"files\":\"Arquivos\",\"desktop:org.example.App\":\"Externo\"}","fallbackNames":"{\"terminal\":\"Terminal\"}"})).unwrap();
    assert_eq!(r["source"].as_array().unwrap().len(), 4);
    assert_eq!(r["source"][0]["name"], "Arquivos");
    assert_eq!(r["source"][1]["name"], "Terminal");
    assert_eq!(r["source"][2]["kind"], "package");
    assert_eq!(r["source"][2]["dockId"], "lpk:pack");
    assert_eq!(r["source"][3]["id"], "desktop:org.example.App");
    assert_eq!(r["source"][3]["desktopId"], "org.example.App");
    assert_eq!(r["source"][3]["mimeTypes"], "application/pdf;");
    assert_eq!(r["source"][3]["name"], "External");
    assert_eq!(r["source"][3]["displayName"], "Externo");
    let live=s.request(&json!({"op":"sync","native":[{"id":"installer","name":"Install","live":true}],"linux":[],"live":true})).unwrap();
    assert_eq!(live["source"].as_array().unwrap().len(), 1);
}

#[test]
fn rust_unicode_matches_search_and_canonical_sort_contracts() {
    assert_eq!(unicode::nfc("A\u{301}rvore"), "Árvore");
    assert_eq!(unicode::nfc("a\u{301}\0o\u{308}"), "á\0ö");
    assert_eq!(unicode::query("\u{feff}\u{a0}ΟΣ \n"), "ος");
    assert_eq!(unicode::query("\u{85}A\u{85}"), "\u{85}a\u{85}");
    let mut s = model(vec![app("Árvore", ""), app("A\u{301}rvore", "")]);
    assert_eq!(s.entries[0].sort_key, s.entries[1].sort_key);
    assert_eq!(indices(&s.view(&json!({})).unwrap()), [0, 1]);
    assert_eq!(s.view(&json!({"query":"ÁRVORE"})).unwrap()["total"], 1);
}

#[test]
fn window_identity_uses_canonical_catalog_metadata() {
    let mut s = shell();
    for id in ["org.example.editor", "ExampleEditor", "EDITOR"] {
        assert_eq!(
            s.request(&json!({"op":"window","id":id})).unwrap()["id"],
            "desktop:org.example.Editor"
        );
    }
    assert!(s.request(&json!({"op":"window","id":""})).unwrap()["id"].is_null());
}

#[test]
fn flow_state_restores_category_and_submits_only_real_results() {
    let mut s = shell();
    let id = open_menu(&mut s, "flow");
    assert_eq!(event(&mut s, id, json!(["category", "System"]))["total"], 2);
    assert_eq!(
        event(&mut s, id, json!(["query", "editor"]))["first"],
        "desktop:org.example.Editor"
    );
    let open = event(&mut s, id, json!(["submit"]));
    assert_eq!(
        effect(&open, "open").unwrap()["id"],
        "desktop:org.example.Editor"
    );
    assert!(effect(&open, "close").is_some());
    assert_eq!(event(&mut s, id, json!(["query", ""]))["total"], 2);
    assert_eq!(event(&mut s, id, json!(["query", "missing"]))["total"], 0);
    assert!(effect(&event(&mut s, id, json!(["submit"])), "open").is_none());
}

#[test]
fn menu_handles_are_isolated_bounded_and_released() {
    let mut s = shell();
    let a = open_menu(&mut s, "flow");
    let b = open_menu(&mut s, "classic");
    event(&mut s, a, json!(["query", "Files"]));
    assert_eq!(event(&mut s, b, json!(["noop"]))["query"], "");
    for _ in 0..14 {
        open_menu(&mut s, "flow");
    }
    assert!(s
        .request(&json!({"op":"open","mode":"flow","context":context()}))
        .is_err());
    s.request(&json!({"op":"close","id":a})).unwrap();
    assert!(s
        .request(&json!({"op":"event","id":a,"action":["submit"]}))
        .is_err());
    let c = open_menu(&mut s, "flow");
    assert!(c > b);
    assert!(s
        .request(&json!({"op":"scroll","id":b,"key":"invalid"}))
        .is_err());
    assert!(s.menus.contains_key(&b));
}

#[test]
fn classic_keyboard_uses_latest_service_results_and_clamps_selection() {
    let mut s = shell();
    let id = open_menu(&mut s, "classic");
    let query = event(&mut s, id, json!(["query", "term"]));
    assert_eq!(effect(&query, "search").unwrap()["query"], "term");
    s.request(&json!({"op":"event","id":id,"action":["results"],"query":"term","sections":[{"id":"apps","items":[{"key":"app:terminal","title":"Terminal"},{"key":"app:files","title":"Files"}]}]})).unwrap();
    assert_eq!(
        event(&mut s, id, json!(["key", "ArrowDown"]))["handled"],
        true
    );
    let enter = event(&mut s, id, json!(["key", "Enter"]));
    assert_eq!(effect(&enter, "runSearch").unwrap()["key"], "app:files");
    event(&mut s, id, json!(["query", "next"]));
    s.request(&json!({"op":"event","id":id,"action":["results"],"query":"term","sections":[{"id":"apps","items":[{"key":"stale"}]}]})).unwrap();
    assert!(effect(&event(&mut s, id, json!(["key", "Enter"])), "runSearch").is_none());
    event(&mut s, id, json!(["key", "ArrowDown"]));
    assert_eq!(s.menus[&id].selected, 0);
}

#[test]
fn popup_actions_work_after_the_material_closes_the_popup() {
    let mut s = shell();
    let id = open_menu(&mut s, "classic");
    event(&mut s, id, json!(["context", "other"]));
    event(&mut s, id, json!(["closeContext"]));
    let pinned = event(&mut s, id, json!(["pinStart", "other"]));
    let patch = effect(&pinned, "settings").unwrap()["patch"].clone();
    assert!(patch["startPinned"]
        .as_array()
        .unwrap()
        .contains(&json!("other")));
    let mut ctx = context();
    ctx["settings"] = patch;
    s.request(&json!({"op":"context","id":id,"context":ctx}))
        .unwrap();
    let unpin = event(&mut s, id, json!(["pinStart", "other"]));
    assert!(!effect(&unpin, "settings").unwrap()["patch"]["startPinned"]
        .as_array()
        .unwrap()
        .contains(&json!("other")));
    let dock=s.request(&json!({"op":"event","id":id,"action":["dock","other"],"dock":{"id":"app:other","pinned":false,"removable":true}})).unwrap();
    assert_eq!(effect(&dock, "dock").unwrap()["pin"], true);
    let locked=s.request(&json!({"op":"event","id":id,"action":["dock","files"],"dock":{"id":"app:files","pinned":true,"removable":false}})).unwrap();
    assert!(effect(&locked, "dock").is_none());
}

#[test]
fn classic_pins_recents_and_drag_are_catalog_validated() {
    let mut s = shell();
    let id = open_menu(&mut s, "classic");
    let mut ctx = context();
    ctx["settings"] = json!({"startPinned":["other","removed","files"],"dockRecentApps":["desktop:org.example.Editor","app:files","files","lpk:package","unknown"]});
    s.request(&json!({"op":"context","id":id,"context":ctx}))
        .unwrap();
    assert_eq!(s.menus[&id].pins(&s), ["other", "files"]);
    assert_eq!(
        s.menus[&id].recent(&s),
        ["desktop:org.example.Editor", "files", "package"]
    );
    let left = s
        .request(&json!({"op":"event","id":id,"action":["drag","files"],"button":0,"x":12,"y":34}))
        .unwrap();
    assert_eq!(effect(&left, "drag").unwrap()["x"], 12);
    assert!(effect(
        &s.request(&json!({"op":"event","id":id,"action":["drag","files"],"button":2}))
            .unwrap(),
        "drag"
    )
    .is_none());
    let click = s
        .request(&json!({"op":"event","id":id,"action":["open","files"],"dragJustEnded":true}))
        .unwrap();
    assert!(effect(&click, "open").is_none());
}

#[test]
fn outside_cancel_and_power_follow_menu_mode_and_capabilities() {
    let mut s = shell();
    let id = open_menu(&mut s, "classic");
    let popup = s
        .request(&json!({"op":"event","id":id,"action":["close"],"insidePopup":true}))
        .unwrap();
    assert!(effect(&popup, "close").is_none());
    assert!(effect(&event(&mut s, id, json!(["close"])), "close").is_some());
    event(&mut s, id, json!(["query", "something"]));
    event(&mut s, id, json!(["query", ""]));
    assert!(effect(&event(&mut s, id, json!(["cancel", "something"])), "close").is_none());
    assert!(effect(&event(&mut s, id, json!(["cancel", ""])), "close").is_some());
    for facts in [
        json!({"inside":true,"onBar":false}),
        json!({"inside":false,"onBar":true}),
    ] {
        let mut args = facts;
        args["op"] = json!("event");
        args["id"] = json!(id);
        args["action"] = json!(["outside"]);
        assert!(effect(&s.request(&args).unwrap(), "close").is_none());
    }
    assert!(effect(&event(&mut s, id, json!(["outside"])), "close").is_some());
    assert!(effect(
        &event(&mut s, id, json!(["session", "shutdown"])),
        "session"
    )
    .is_none());
    assert!(effect(&event(&mut s, id, json!(["session", "quit"])), "session").is_some());
    let mut ctx = context();
    ctx["session"] = json!({"managed":true,"canPower":true});
    s.request(&json!({"op":"context","id":id,"context":ctx}))
        .unwrap();
    assert_eq!(
        effect(&event(&mut s, id, json!(["session", "restart"])), "session").unwrap()["action"],
        "restart"
    );
}

#[test]
fn classic_placement_matches_bars_multimonitor_and_rtl() {
    let d = json!({"x":1920,"y":-200,"w":1280,"h":720});
    let bottom = session::placement(&json!({"display":d,"position":"bottom","alignment":"center"}));
    assert_eq!(
        bottom,
        json!({"left":2240.,"top":-188.,"width":640.,"height":648.})
    );
    let rtl =
        session::placement(&json!({"display":d,"position":"top","alignment":"start","rtl":true}));
    assert_eq!(rtl["left"], 2548.);
    assert_eq!(rtl["top"], -140.);
    let side = session::placement(&json!({"display":d,"position":"left"}));
    assert_eq!(side["left"], 1988.);
    assert_eq!(side["height"], 696.);
}
