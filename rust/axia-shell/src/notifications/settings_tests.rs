use super::*;

#[test]
fn native_preference_queries_and_edits_use_the_owner_without_draining_on_reads() {
    let path = std::env::temp_dir().join(format!("axia-notification-edit-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let home = crate::settings::profile(path.to_str().unwrap());
    let settings = crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.set("notifications", &json!({"dnd":true,"extra":7,"apps":{"clock":{"sound":false,"custom":3},"notes":{"lock":false}}}));
        s.reader()
    });
    let now = Instant::now();
    let mut service = Service::default();
    service.bind_settings(&home);
    service.effects.push(json!({"kind":"pending"}));
    let state = service.reader();
    let changes = state.subscribe();
    for input in [
        json!({"action":"preferences","prefs":{"extra":0}}),
        json!({"action":"appPreferences","app":"clock","prefs":{"apps":{"clock":{"sound":true}}}}),
        json!({"action":"policy","prefs":{"dnd":false},"wallMs":0,"minute":0}),
    ] {
        let reply = service.command(&input, now + Duration::from_secs(60));
        match input["action"].as_str().unwrap() {
            "preferences" => assert_eq!(reply["result"]["extra"], 7),
            "appPreferences" => assert_eq!(reply["result"]["sound"], false),
            _ => assert_eq!(reply["result"], true),
        }
        assert_eq!(reply["effects"], json!([]));
    }
    assert!(!changes.pending());
    assert_eq!(service.effects.len(), 1);
    let reply = service.command(
        &json!({"action":"setAppPreferences","app":"clock","patch":{"style":"alert"},"prefs":{}}),
        now,
    );
    assert_eq!(settings.read()["notifications"]["extra"], 7);
    assert_eq!(
        settings.read()["notifications"]["apps"]["clock"],
        json!({"custom":3,"sound":false,"style":"alert"})
    );
    assert_eq!(
        settings.read()["notifications"]["apps"]["notes"],
        json!({"lock":false})
    );
    assert!(reply["effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "settingsState"));
    let settings_changes = settings.subscribe();
    let no_op = service.command(
        &json!({"action":"setAppPreferences","app":"clock","patch":{"style":"alert"}}),
        now,
    );
    assert_eq!(no_op["effects"], json!([]));
    assert!(!settings_changes.pending());
    std::fs::remove_dir(&path).unwrap();
}

#[test]
fn resident_preferences_override_stale_transport_and_invalidate_grouping_and_ringing() {
    let path =
        std::env::temp_dir().join(format!("axia-notification-settings-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let home = crate::settings::profile(path.to_str().unwrap());
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.command(&json!({"cmd":"patch","patch":{"notifications":{"grouping":"off","apps":{"clock":{"sound":true}},"custom":7}}}));
    });
    let now = Instant::now();
    let mut service = Service::default();
    service.bind_settings(&home);
    let reader = service.reader();
    assert!(reader.read().ungrouped);
    let reply = service.command(
        &json!({"action":"push","prefs":{"dnd":true},"wallMs":0,"minute":0,
        "notice":{"app":"clock","title":"Resident","ring":true,"sound":true}}),
        now,
    );
    assert_eq!(reply["result"], 1);
    assert!(reader.read().shown[0].ring.is_some());
    assert!(reply["effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "settingsState"));
    assert!(!reply["effects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "settings"));
    crate::settings::with(&home, |s| {
        let state = s.reader();
        assert_eq!(state.read()["notifications"]["seen"], json!(["clock"]));
        assert_eq!(state.read()["notifications"]["custom"], 7);
        s.command(&json!({"cmd":"patch","patch":{"notifications":{"grouping":"app","dnd":true}}}));
    });
    assert!(service.settings_pending());
    service.expire(now);
    assert!(!service.settings_pending());
    assert!(!reader.read().ungrouped);
    assert!(reader.read().shown[0].ring.is_none());
    let quiet = reader.subscribe();
    service.expire(now);
    assert!(!quiet.pending());
    service.bind_settings(&home); // remount preserves the consumed subscription and state
    assert!(!service.settings_pending());
    let suppressed = service.command(
        &json!({"action":"push","prefs":{"dnd":false},"notice":{"title":"Hidden"}}),
        now,
    );
    assert_eq!(suppressed["result"], 2);
    assert_eq!(reader.read().shown.len(), 1);
    std::fs::remove_dir(&path).unwrap();
}

#[test]
fn unrelated_preference_changes_do_not_revise_notification_state() {
    let path = std::env::temp_dir().join(format!("axia-notification-quiet-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let home = crate::settings::profile(path.to_str().unwrap());
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize"}));
    });
    let mut service = Service::default();
    service.bind_settings(&home);
    let reader = service.reader();
    let changes = reader.subscribe();
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"patch","patch":{"accent":"green"}}));
    });
    assert!(service.settings_pending());
    service.expire(Instant::now());
    assert!(!service.settings_pending());
    assert!(!changes.pending());
    std::fs::remove_dir(&path).unwrap();
}
