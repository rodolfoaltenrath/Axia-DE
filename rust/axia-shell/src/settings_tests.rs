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
fn dirty_state_survives_remount_and_stale_save_acknowledgements() {
    let mut model = initialized(Value::Null);
    let first = model.command(&json!({"cmd":"patch","patch":{"accent":"green"}}));
    let second = model.command(&json!({"cmd":"patch","patch":{"mode":"light"}}));
    assert_eq!(
        model.command(&json!({"cmd":"initialize","text":"{}"})),
        second
    );
    assert_eq!(
        model.command(&json!({"cmd":"saved","revision":first["revision"]}))["dirty"],
        true
    );
    let reader = model.reader();
    let changes = reader.subscribe();
    assert_eq!(
        model.command(&json!({"cmd":"saved","revision":second["revision"]}))["dirty"],
        false
    );
    assert!(!changes.pending());
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
