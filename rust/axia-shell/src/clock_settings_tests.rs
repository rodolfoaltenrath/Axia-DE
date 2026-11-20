use super::*;

#[test]
fn native_clock_edits_persist_without_a_js_ack_and_ignore_old_configuration() {
    let path = std::env::temp_dir().join(format!("axia-clock-preferences-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let home = crate::settings::profile(path.to_str().unwrap());
    let reader = crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.command(
            &json!({"cmd":"patch","patch":{"custom":"kept","clock":{"cities":["UTC"],"future":7}}}),
        );
        s.reader()
    });
    bind(&home);
    let wall = axia_clock::now();
    let reply = axia_clock::with(|s| {
        s.command(
            &json!({"cmd":"cities","cities":["Europe/Berlin"],"config":{"future":0}}),
            wall,
        )
    });
    assert_eq!(reader.read()["custom"], "kept");
    assert_eq!(reader.read()["clock"]["future"], 7);
    assert_eq!(reader.read()["clock"]["cities"], json!(["Europe/Berlin"]));
    assert!(!reply["effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "settings"));
    assert!(pending());
    take();
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"patch","patch":{"clock":{"alarms":[{"id":"native","on":true,"snooze":wall+600000}],"hour24":"24"}}}));
    });
    assert!(pending());
    let armed = axia_clock::with(|s| s.command(&json!({"cmd":"arm"}), wall));
    take();
    assert!(!pending());
    assert_eq!(
        axia_clock::with(|s| s.config["alarms"][0]["id"].clone()),
        "native"
    );
    assert!(armed["wait"].as_i64().unwrap() > 0);
    let changes = reader.subscribe();
    axia_clock::with(|s| s.command(&json!({"cmd":"arm"}), wall));
    assert!(!changes.pending());
    assert!(!crate::settings::with(&home, |s| s.set("clock", &armed["config"])));
    assert!(!changes.pending());
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"patch","patch":{"clock":null}}));
    });
    assert_eq!(axia_clock::with(|s| s.config.clone()), json!({}));
    std::fs::remove_dir(&path).unwrap();
}
