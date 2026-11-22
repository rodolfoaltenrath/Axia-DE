use super::*;
use crate::winmodel::Session;
use axia_state::Store;

fn configured_memory(session: &Session, input: &Value) -> Memory {
    let displays = Store::new(rows(&input["args"]["displays"]).to_vec());
    let mut memory = Memory::new(session.reader(), displays.reader());
    memory.bind_catalog(Store::new(input.clone()).reader());
    memory.enable(input["args"]["enabled"] == true);
    memory
}

fn facts() -> Value {
    json!({"registry":[{"id":"notes"},{"id":"panel","builtin":true},{"id":"pkg","package":true}],
        "catalog":[{"id":"desktop:org.Editor","kind":"linux","desktopId":"org.Editor","wmClass":"Editor"}],
        "args":{"enabled":true,"places":[["app:old",{"w":100,"h":100}]],
        "displays":[{"id":0,"name":"Panel","connector":"DP-1","x":0,"y":0,"w":1000,"h":800,
            "work":{"x":0,"y":20,"w":1000,"h":740}}]}})
}

#[test]
fn memory_coalesces_native_changes_and_skips_dialogs_panels_and_transient_frames() {
    let mut session = Session::default();
    for input in [
        json!({"app":"notes","frame":{"x":30,"y":50,"w":300,"h":220}}),
        json!({"app":"panel"}),
        json!({"app":"notes","instance":"dialog","args":{"dialog":true}}),
        json!({"app":"linux","instance":"1","args":{"appId":"Editor"}}),
        json!({"app":"linux","instance":"2","args":{"pkg":true,"appId":"pkg"}}),
        json!({"app":"linux","instance":"3"}),
    ] {
        session.call("open", &[json!({}), input]).unwrap();
    }
    let input = facts();
    let mut memory = configured_memory(&session, &input);
    let now = Instant::now();
    assert!(memory.tick(now)["effects"].is_null());
    session
        .call(
            "setFrame",
            &[
                json!({}),
                json!("notes"),
                json!({"x":77,"y":50,"w":300,"h":220}),
            ],
        )
        .unwrap();
    assert!(memory.invalidated());
    memory.tick(now + Duration::from_millis(600));
    assert_eq!(memory.due, Some(now + SAVE_DELAY));
    let effect = memory.tick(now + SAVE_DELAY);
    let merged = merge(&input["args"]["places"], &effect["effects"][0]["updates"]);
    let entries = rows(&merged["entries"]);
    let keys: Vec<_> = entries
        .iter()
        .map(|entry| entry[0].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        vec!["app:old", "app:notes", "desktop:org.Editor", "lpk:pkg"]
    );
    assert_eq!(entries[1][1]["x"], 77);
    assert_eq!(entries[1][1]["d"], "Panel|DP-1");
    assert!(memory.wait_ms(Instant::now()).is_none());
    assert!(!memory.invalidated());
    session
        .call(
            "patch",
            &[json!({}), json!("notes"), json!({"title":"Only a title"})],
        )
        .unwrap();
    memory.tick(now + Duration::from_secs(1));
    assert!(memory.tick(now + Duration::from_secs(2))["effects"].is_null());
    session
        .call(
            "setPip",
            &[
                json!({}),
                json!("notes"),
                json!({"x":999,"y":0,"w":200,"h":150}),
            ],
        )
        .unwrap();
    memory.tick(now + Duration::from_secs(3));
    assert!(memory.tick(now + Duration::from_secs(4))["effects"].is_null());
}

#[test]
fn restore_counts_native_twins_and_retains_saved_monitor_and_fixed_size_rules() {
    let displays = json!([{"id":0,"x":0,"y":0,"w":10,"h":10},{"id":1,"x":20,"y":0,"w":10,"h":10}]);
    assert_eq!(
        display_for(Rect::new(14., 0., 2., 1.), rows(&displays)).unwrap()["id"],
        1
    );
    let mut session = Session::default();
    for instance in ["a", "b"] {
        session
            .call(
                "open",
                &[json!({}), json!({"app":"notes","instance":instance})],
            )
            .unwrap();
    }
    let metadata = facts();
    let mut input = json!({"key":"app:notes","windows":[],"places":[["app:notes",{
        "d":"Panel|DP-1","x":30,"y":50,"w":300,"h":220,"max":true}]],
        "displays":metadata["args"]["displays"],"options":{"main":0}});
    let reply = restore(&session.reader(), &metadata, &input);
    assert_eq!(reply["frame"]["x"], 90.);
    assert_eq!(reply["frame"]["y"], 110.);
    assert_eq!(reply["frame"]["maximized"], true);
    input["options"] = json!({"fixed":true,"size":[200,180],"shift":0});
    let reply = restore(&session.reader(), &metadata, &input);
    assert_eq!(reply["frame"]["w"], 200.);
    assert_eq!(reply["frame"]["maximized"], false);
    input["places"][0][1]["d"] = json!("Missing|HDMI");
    assert_eq!(
        restore(&session.reader(), &metadata, &input)["frame"]["x"],
        400.
    );
    input["places"][0][1]["w"] = json!(-1);
    assert!(restore(&session.reader(), &metadata, &input)["frame"].is_null());
}

#[test]
fn memory_keeps_normal_geometry_and_the_two_hundred_most_recent_keys() {
    let mut session = Session::default();
    session
        .call("open", &[json!({}), json!({"app":"notes"})])
        .unwrap();
    session
        .call(
            "setFrame",
            &[
                json!({}),
                json!("notes"),
                json!({"x":80,"y":60,"w":340,"h":250}),
            ],
        )
        .unwrap();
    session
        .call(
            "liftForKeyboard",
            &[
                json!({}),
                json!("notes"),
                json!({"x":80,"y":0,"w":340,"h":180}),
            ],
        )
        .unwrap();
    let mut input = facts();
    input["args"]["places"] = json!((0..200)
        .map(|i| json!([format!("app:{i}"), {}]))
        .collect::<Vec<_>>());
    let mut memory = configured_memory(&session, &input);
    let now = Instant::now();
    memory.tick(now);
    let reply = memory.tick(now + SAVE_DELAY);
    let merged = merge(&input["args"]["places"], &reply["effects"][0]["updates"]);
    let entries = rows(&merged["entries"]);
    assert_eq!(entries.len(), 200);
    assert_eq!(entries[0][0], "app:1");
    assert_eq!(
        entries[199],
        json!(["app:notes",{"d":"Panel|DP-1","x":80,"y":60,"w":340,"h":250,"max":false}])
    );
}

#[test]
fn merging_rebases_on_current_preferences_and_disabled_service_has_no_deadline() {
    let mut session = Session::default();
    session
        .call("open", &[json!({}), json!({"app":"notes"})])
        .unwrap();
    let mut input = facts();
    input["args"]["enabled"] = json!(false);
    let mut memory = configured_memory(&session, &input);
    assert!(memory.tick(Instant::now())["effects"].is_null());
    assert!(memory.wait_ms(Instant::now()).is_none());
    assert!(!memory.invalidated());
    let merged = merge(
        &json!([["app:external",{"x":40}],["app:notes",{"x":1}]]),
        &json!([["app:notes",{"x":70}]]),
    );
    assert_eq!(
        merged["entries"],
        json!([["app:external",{"x":40}],["app:notes",{"x":70}]])
    );
}

#[test]
fn profile_rebases_atomically_and_preserves_recency_across_sorted_json_and_restart() {
    let root = std::env::temp_dir().join(format!("axia-memory-order-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let home = root.to_str().unwrap();
    let keys: Vec<_> = (1..=200).rev().map(|i|format!("app:{i:03}")).collect();
    let reader = crate::settings::with(home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.set("windowPlaces", &Value::Object(keys.iter().map(|k|(k.clone(),json!({"x":1,"w":300,"h":200}))).collect()));
        s.set("windowPlacesOrder", &json!(keys));
        s.set("unrelated", &json!({"keep":7}));
        s.reader()
    });
    let revision = reader.revision();
    let reply = save_profile(home, &json!([["app:100",{"x":70,"w":300,"h":200}]]));
    assert_eq!(reply["effects"][0]["kind"], "settingsState");
    assert_eq!(reader.revision(), revision+1, "map and order are one mutation");
    assert_eq!(reader.read()["windowPlacesOrder"][199], "app:100");
    assert_eq!(reader.read()["windowPlacesOrder"][0], "app:200");
    // No Memory instance carries the order; a fresh writer must read it from the profile.
    save_profile(home, &json!([["app:new",{"x":80}]]));
    assert!(reader.read()["windowPlaces"].get("app:200").is_none());
    assert!(reader.read()["windowPlaces"].get("app:001").is_some());
    assert_eq!(reader.read()["windowPlacesOrder"][199], "app:new");
    let mut external = reader.read()["windowPlaces"].clone();
    external["app:external"] = json!({"x":44});
    crate::settings::with(home, |s| { s.set("windowPlaces", &external); });
    save_profile(home, &json!([["app:notes",{"x":99}]]));
    assert_eq!(reader.read()["windowPlaces"]["app:external"]["x"], 44);
    assert_eq!(reader.read()["windowPlacesOrder"][199], "app:notes");
    assert_eq!(reader.read()["windowPlaces"].as_object().unwrap().len(), 200);
    assert_eq!(reader.read()["unrelated"]["keep"], 7);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_restore_borrows_one_place_and_ignores_stale_transport() {
    let mut session = Session::default();
    session.call("open", &[json!({}),json!({"app":"notes"})]).unwrap();
    let metadata = facts();
    let input = json!({"key":"app:notes","places":[["app:notes",{"x":999}]],
        "displays":metadata["args"]["displays"],"options":{"main":0}});
    let profile = json!({"windowPlaces":{"app:notes":{"d":"Panel|DP-1","x":30,"y":50,"w":300,"h":220}},
        "unrelated":{"large":"x".repeat(100_000)}});
    let reply = restore_profile(&session.reader(), &metadata, &input, &profile);
    assert_eq!(reply["frame"]["x"], 60.);
    assert_eq!(reply["frame"]["y"], 80.);
    assert_eq!(reply["frame"]["w"], 300.);
}
