use super::*;
fn initialized(text: Value) -> Model {
    let mut model = Model::default();
    model.command(&json!({"cmd":"initialize","text":text}));
    model
}
#[test]
fn defaults_and_migrations_keep_user_fields_and_future_schemas() {
    let mut model = initialized(Value::Null);
    assert_eq!(model.snapshot()["state"]["wallpaper"], "infinite-galaxy");
    assert_eq!(model.snapshot()["state"]["schema"], 3);
    assert!(!model.snapshot()["dirty"].as_bool().unwrap());
    let migrated=model.command(&json!({"cmd":"migrate","value":{"schema":1,"externalWindowStyle":true,"filesAppearance":"axia","motion":{"a":1},"remoteDesks":[1],"custom":{"app":3}}}))["value"].clone();
    assert_eq!(migrated["nativeAppearance"], "axia");
    assert_eq!(migrated["externalWindowStyle"], false);
    assert!(migrated.get("motion").is_none());
    assert!(migrated.get("remoteDesks").is_none());
    assert_eq!(migrated["custom"], json!({"app":3}));
    assert!(
        migrated.get("mode").is_none(),
        "pure migration does not add defaults"
    );
    let future = json!({"schema":10,"externalWindowStyle":true,"future":{"values":[1,2]}});
    assert_eq!(migrate(future.clone()), future);
    for version in [json!(" 3 "), json!("future-version"), json!({"future":10})] {
        let future = json!({"schema":version,"externalWindowStyle":true,"custom":{"value":1}});
        assert_eq!(migrate(future.clone()), future);
    }
}
#[test]
fn observable_updates_are_atomic_and_noops_and_queries_are_quiet() {
    let mut model = initialized(json!(r#"{"schema":3,"custom":{"value":1}}"#));
    let reader = model.reader();
    let changes = reader.subscribe();
    let rev = reader.revision();
    model.command(&json!({"cmd":"patch","patch":{"mode":"light","custom":{"value":2}}}));
    assert!(changes.pending());
    assert_eq!(reader.revision(), rev + 1);
    assert_eq!(reader.read()["custom"]["value"], 2);
    changes.take();
    model.command(&json!({"cmd":"patch","patch":{"mode":"light"}}));
    model.command(&json!({"cmd":"snapshot"}));
    model.command(&json!({"cmd":"migrate","value":{"schema":1}}));
    assert!(!changes.pending());
    assert_eq!(reader.revision(), rev + 1);
    let before = model.snapshot();
    assert!(model
        .command(&json!({"cmd":"replace","value":null}))
        .get("error")
        .is_some());
    assert_eq!(model.snapshot(), before);
}
#[test]
fn dirty_state_survives_remount_until_the_native_save_succeeds() {
    let mut model = initialized(Value::Null);
    model.command(&json!({"cmd":"patch","patch":{"accent":"green"}}));
    let second = model.command(&json!({"cmd":"patch","patch":{"mode":"light"}}));
    assert_eq!(
        model.command(&json!({"cmd":"initialize","text":"{}"})),
        second
    );
    let reader = model.reader();
    let changes = reader.subscribe();
    assert_eq!(model.save("dirty-profile", true)["dirty"], false);
    assert!(!changes.pending());
    assert!(model.wait_ms(Instant::now()).is_none());
    assert!(io::host::writes()[0].2);
}

#[test]
fn native_bootstrap_reads_the_private_profile_file_instead_of_js_text() {
    io::host::set_file("profile-read", br#"{"schema":3,"custom":{"value":7}}"#);
    let mut model = Model::default();
    let reply = model.request("profile-read", &json!({"cmd":"initialize","text":"{bad"}));
    assert_eq!(reply["state"]["custom"]["value"], 7);
    assert_eq!(reply["dirty"], false);
    assert_eq!(reply["pending"], false);
}
#[test]
fn native_deadline_coalesces_changes_without_extending_for_noops_and_flushes_once() {
    let now = Instant::now();
    let mut model = initialized(Value::Null);
    model.command_at(&json!({"cmd":"patch","patch":{"mode":"light"}}), now);
    model.command_at(
        &json!({"cmd":"patch","patch":{"accent":"green"}}),
        now + Duration::from_millis(100),
    );
    model.command_at(
        &json!({"cmd":"patch","patch":{"accent":"green"}}),
        now + Duration::from_millis(200),
    );
    model.rearm(now + Duration::from_millis(250));
    model.flush_due("deadline-profile", now + Duration::from_millis(399));
    assert!(io::host::writes().is_empty());
    let reader = model.reader();
    let changes = reader.subscribe();
    let revision = reader.revision();
    model.flush_due("deadline-profile", now + Duration::from_millis(400));
    model.flush_due("deadline-profile", now + Duration::from_secs(10));
    let writes = io::host::writes();
    assert_eq!(writes.len(), 1);
    assert!(!writes[0].2);
    let stored: Value = serde_json::from_slice(&writes[0].1).unwrap();
    assert_eq!(stored["mode"], "light");
    assert_eq!(stored["accent"], "green");
    assert!(!model.dirty);
    assert!(model.wait_ms(now).is_none());
    assert!(!changes.pending());
    assert_eq!(reader.revision(), revision);
}
#[test]
fn failed_or_oversized_saves_keep_dirty_state_without_a_retry_loop() {
    let now = Instant::now();
    let mut model = initialized(Value::Null);
    model.command_at(&json!({"cmd":"patch","patch":{"mode":"light"}}), now);
    io::host::set_file("failure-profile", b"previous bytes");
    io::host::fail(true);
    model.flush_due("failure-profile", now + SAVE_AFTER);
    assert!(model.dirty);
    assert!(model.until.is_none());
    model.flush_due("failure-profile", now + Duration::from_secs(30));
    assert_eq!(io::host::writes().len(), 1);
    assert_eq!(io::load("failure-profile").unwrap(), b"previous bytes");
    io::host::fail(false);
    model.rearm(now + Duration::from_secs(30));
    assert!(model.wait_ms(now + Duration::from_secs(30)).unwrap() >= 300.);
    assert_eq!(model.save("failure-profile", true)["saved"], true);
    assert!(!model.dirty);
    assert!(model.until.is_none());
    model.command(&json!({"cmd":"patch","patch":{"oversized":"x".repeat(MAX_BYTES)}}));
    let count = io::host::writes().len();
    assert_eq!(
        model.save("failure-profile", false)["error"],
        "settings.tooLarge"
    );
    assert_eq!(io::host::writes().len(), count);
    assert!(model.dirty);
    assert!(model.until.is_none());
}
#[test]
fn successful_legacy_writes_update_the_owner_and_cancel_a_pending_save() {
    let now = Instant::now();
    let mut model = initialized(Value::Null);
    model.command_at(&json!({"cmd":"patch","patch":{"mode":"light"}}), now);
    let reader = model.reader();
    let changes = reader.subscribe();
    model.external_saved(r#"{"schema":3,"mode":"dark","legacy":7}"#);
    assert!(changes.pending());
    assert_eq!(reader.read()["legacy"], 7);
    assert!(!model.dirty);
    assert!(model.until.is_none());
    model.flush_due("legacy-profile", now + Duration::from_secs(10));
    assert!(io::host::writes().is_empty());
}
#[test]
fn malformed_files_use_defaults_without_discarding_future_valid_updates() {
    let mut model = Model::default();
    let reply = model.command(&json!({"cmd":"initialize","text":"{bad json"}));
    assert_eq!(reply["invalid"], true);
    assert_eq!(reply["state"]["mode"], "dark");
    assert!(reply["state"].get("schema").is_none());
    let changed = model.command(&json!({"cmd":"patch","patch":{"schema":3,"custom":"kept"}}));
    assert_eq!(changed["state"]["custom"], "kept");
    assert_eq!(changed["dirty"], true);
}
#[test]
fn profiles_and_readers_remain_independent() {
    with("settings-test-profile-a", |m| {
        m.command(&json!({"cmd":"initialize"}));
        m.command(&json!({"cmd":"patch","patch":{"mode":"light"}}));
    });
    with("settings-test-profile-b", |m| {
        m.command(&json!({"cmd":"initialize"}));
    });
    assert_eq!(
        with("settings-test-profile-a", |m| m.reader()).read()["mode"],
        "light"
    );
    assert_eq!(
        with("settings-test-profile-b", |m| m.reader()).read()["mode"],
        "dark"
    );
}

#[path = "session_restore_settings_tests.rs"]
mod restore;
