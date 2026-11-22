use super::*;
#[test]
fn borrowed_preferences_keep_only_editor_roots_and_preserve_pending_rollback() {
    let mut app = fixture();
    let preferences = json!({"dockSize":60,"clock":{"hour24":"12"},
        "windowPlaces":{"huge":"x".repeat(100_000)},"sessionSaved":[{"private":"session"}],
        "notes":{"documents":["unrelated app state"]}});
    let facts = json!({"cmd":"sync","args":{},"context":{"theme":app.theme},"settings":{"dockSize":999}});
    app.sync_with_preferences(&facts, &preferences);
    assert_eq!(app.pref("dockSize"), 60);
    assert_eq!(app.pref("clock.hour24"), "12");
    for key in ["windowPlaces", "sessionSaved", "notes"] {
        assert!(app.observed_settings.get(key).is_none());
        assert!(app.settings.get(key).is_none());
    }
    app.patch("dockSize", json!(70));
    assert_eq!(app.output()["effect"]["patch"]["dockSize"], 70);
    app.sync_with_preferences(&facts, &preferences);
    assert_eq!(app.pref("dockSize"), 70);
    app.request_at(&json!({"cmd":"next","result":"save failed"}), app.now);
    assert_eq!(app.pref("dockSize"), 60);
    assert!(app.pending_settings.is_empty());
}
#[test]
fn choosing_the_current_popup_option_has_no_system_side_effect() {
    let mut app = fixture();
    app.popup = json!({"value":"argon-one-v5","binding":{"op":"fanCase"}});
    assert!(app.command(&json!({"op":"pick","value":"argon-one-v5"})));
    assert!(app.popup.is_null());
    assert!(!app.jobs.busy());
    assert!(app.effects.is_empty());
}
#[test]
fn replaced_language_packs_reload_without_changing_language() {
    let scratch = std::env::temp_dir().join(format!(
        "axia-settings-locale-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let catalog = scratch.join("locales/en");
    std::fs::create_dir_all(&catalog).unwrap();
    let path = catalog.join("settings.json");
    std::fs::write(
        &path,
        r#"{"section.general":"Before","nested":{"label":"Nested"},"nested.label":"Flat"}"#,
    )
    .unwrap();
    let mut app = fixture();
    app.resources(scratch.to_str().unwrap(), None);
    assert_eq!(app.tr("settings.nested.label"), "Flat");
    let mut facts = json!({"cmd":"sync","args":{},"lang":"en","context":{"width":880,"localeRevision":0,"theme":app.theme}});
    app.request_at(&facts, app.now);
    assert_eq!(app.tr("settings.section.general"), "Before");
    std::fs::write(
        &path,
        r#"{"section.general":"After","nested":{"label":"Nested"}}"#,
    )
    .unwrap();
    assert_eq!(app.tr("settings.section.general"), "Before");
    facts["context"]["localeRevision"] = json!(1);
    let reply = app.request_at(&facts, app.now);
    assert!(reply["tree"].is_object());
    assert_eq!(app.tr("settings.section.general"), "After");
    assert_eq!(app.tr("settings.nested.label"), "Nested");
    std::fs::remove_dir_all(scratch).unwrap();
}
#[test]
fn compound_preferences_are_persisted_in_one_effect() {
    let mut app = fixture();
    app.command(&json!({"op":"nativeAppearance"}));
    let effect = app.output();
    assert_eq!(effect["effect"]["kind"], "settings");
    assert_eq!(effect["effect"]["patch"]["nativeAppearance"], "axia");
    assert_eq!(effect["effect"]["patch"]["filesAppearance"], "axia");
    assert!(app.effects.is_empty());
    assert_eq!(app.direct_patch.as_ref().unwrap().len(), 2);
    assert!(app.output()["effect"].is_null());
}
#[test]
fn account_drafts_cannot_follow_another_selected_user() {
    let mut app = fixture();
    app.state["detailUser"] = json!("test");
    app.state["userPassword"] = json!({"old":"secret","pass":"new","pass2":"new"});
    app.state["deleteUser"] = json!(true);
    app.state["selectedUser"] = json!("other");
    app.reconcile_users();
    assert_eq!(app.state["userPassword"], json!({}));
    assert_eq!(app.state["deleteUser"], false);
    assert_eq!(app.state["detailUser"], "other");
    app.state["addingUser"] = json!(true);
    app.state["newUser"] = json!({"pass":"secret"});
    app.data["system.info"]["admin"] = json!(false);
    app.reconcile_users();
    assert_eq!(app.state["addingUser"], false);
    assert!(app.state["newUser"].is_null());
}

fn commands(value: &Value, output: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            if value["style"]["pointerEvents"] == "none" {
                return;
            }
            for value in map.values() {
                commands(value, output);
            }
        }
        Value::Array(items) => {
            if items.first() == Some(&json!("do")) {
                if let Some(command) = items
                    .get(1)
                    .and_then(Value::as_str)
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                {
                    if command["op"] == "popup" {
                        let mut binding = command["binding"].clone();
                        binding["value"] = command["options"][0][0].clone();
                        output.push(binding);
                    }
                    output.push(command);
                }
            } else if items.first() == Some(&json!("slider")) {
                if let Some(mut slider) = items
                    .get(1)
                    .and_then(Value::as_str)
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                {
                    let min = slider["min"].clone();
                    slider["binding"]["value"] = min;
                    output.push(slider["binding"].clone());
                }
            } else {
                for value in items {
                    commands(value, output);
                }
            }
        }
        _ => {}
    }
}
#[test]
fn rendered_controls_have_live_rust_handlers() {
    let mut app = fixture();
    let mut model = crate::settings_model::Model::default();
    let groups = model.request(&json!({"cmd":"catalog"}));
    let mut actions = vec![];
    for item in arr(&groups).iter().flat_map(arr) {
        app.section = s(&item["id"]).into();
        let tree = app.tree();
        commands(&tree, &mut actions);
    }
    app.state["addingUser"] = json!(true);
    app.section = "users".into();
    commands(&app.tree(), &mut actions);
    app.section = "network".into();
    app.state["networkPage"] = json!("vpn");
    app.state["vpnImport"] = json!(true);
    app.state["vpnType"] = json!("ipsec");
    app.state["vpnForm"] = json!({"auth":"eap"});
    app.state["vpnAuth"] = json!({"name":"test-vpn","user":"","pass":""});
    commands(&app.tree(), &mut actions);
    assert!(actions.len() > 500);
    for command in actions {
        let mut app = fixture();
        assert!(app.command(&command), "unhandled action: {command}");
        assert!(
            app.jobs.pump(0).is_none(),
            "test services must stay detached"
        );
    }
}
#[test]
fn failed_configuration_reads_never_enable_apply() {
    let mut app = fixture();
    app.section = "network".into();
    app.set_network_page("if:eth0");
    app.completed(Completed {
        tag: Tag::Read(app.epoch),
        result: Ok(json!({"interfaceConfig":null,"__errors":{"interfaceConfig":"unavailable"}})),
    });
    assert!(app.d("interfaceConfigName").is_null());
    app.command(&json!({"op":"netApply","name":"eth0"}));
    assert!(!app.jobs.busy());
}
#[test]
fn dock_actions_obey_host_and_hardware_capabilities() {
    let mut model = crate::settings_model::Model::default();
    let host = model.request(&json!({"cmd":"dockActions","standalone":true}));
    for id in ["users", "cooling", "firmware", "trackpad"] {
        assert!(!arr(&host).iter().any(|a| a["id"] == id));
    }
    let machine=model.request(&json!({"cmd":"dockActions","services":{"input":{"devices":[{"kind":"touchpad"}]},"system":{"info":{"board":"pi5"}}}}));
    for id in ["users", "cooling", "firmware", "trackpad"] {
        assert!(arr(&machine).iter().any(|a| a["id"] == id));
    }
}
#[test]
fn interface_changes_wait_for_the_matching_configuration() {
    let mut app = fixture();
    app.section = "network".into();
    app.set_network_page("if:eth0");
    let old_epoch = app.epoch;
    assert!(app.command(&json!({"op":"netApply","name":"eth0"})));
    assert!(!app.jobs.busy());
    app.set_network_page("if:eth1");
    app.completed(Completed {
        tag: Tag::Read(old_epoch),
        result: Ok(json!({"interfaceConfig":{"method":"static","addresses":["10.0.0.1/24"]}})),
    });
    assert!(app.d("interfaceConfigName").is_null());
    assert!(app.d("interfaceConfig").is_null());
    app.completed(Completed{tag:Tag::Read(app.epoch),result:Ok(json!({"interfaceConfig":{"method":"static","addresses":["10.1.0.1/24"],"gateway":"10.1.0.254","dns":["1.1.1.1"]}}))});
    assert_eq!(app.d("interfaceConfigName"), "eth1");
    app.command(&json!({"op":"netApply","name":"eth1"}));
    app.jobs.bind(1);
    let effect = app.jobs.pump(0).unwrap();
    assert_eq!(
        effect["args"][2][1][1],
        "eth1\nmethod=static\naddress=10.1.0.1/24\ngateway=10.1.0.254\ndns=1.1.1.1"
    );
}
#[test]
fn pending_preferences_survive_stale_facts_and_effects_are_serialized() {
    let mut app = fixture();
    app.observed_settings = json!({"dockSize":54});
    app.patch("dockSize", json!(60));
    app.patch("dockSize", json!(70));
    let first = app.output();
    assert_eq!(first["effect"]["kind"], "settings");
    assert_eq!(first["effect"]["patch"]["dockSize"], 60);
    assert!(
        app.output()["effect"].is_null(),
        "a second effect must wait for the first result"
    );
    app.request_at(
        &json!({"cmd":"sync","args":{},"settings":{"dockSize":60},"context":{"theme":app.theme}}),
        app.now,
    );
    assert_eq!(app.pref("dockSize"), 70);
    let next = app.request_at(&json!({"cmd":"next","result":""}), app.now);
    assert_eq!(next["effect"]["patch"]["dockSize"], 70);
    app.request_at(&json!({"cmd":"next","result":""}), app.now);
    assert!(app.pending_settings.is_empty());
}
#[test]
fn leaving_accounts_drops_secrets_and_stale_privacy_results() {
    let mut app = fixture();
    app.section = "users".into();
    app.state["userPassword"] = json!({"old":"old","pass":"secret","pass2":"secret"});
    app.state["newUser"] = json!({"pass":"secret"});
    app.select("sound");
    assert!(app.st("userPassword").is_null());
    assert!(app.st("newUser").is_null());
    app.privacy_pending = Some((app.epoch - 1, 0));
    app.request_at(&json!({"cmd":"next","result":{"value":[]}}), app.now);
    assert!(app.privacy_pending.is_none());
}
#[test]
fn rapid_volume_changes_keep_the_latest_value_without_filling_the_bridge() {
    let mut app = fixture();
    app.jobs.bind(9);
    app.command(&json!({"op":"soundVolume","kind":"sink","index":1,"value":0.1}));
    let first = app.jobs.pump(0).unwrap();
    assert_eq!(first["args"][2][1][2], 0.1);
    app.jobs.complete(&json!(1), 0);
    for i in 2..100 {
        app.command(&json!({"op":"soundVolume","kind":"sink","index":1,"value":i as f64/100.}));
    }
    let poll = app.jobs.pump(100).unwrap();
    assert_eq!(poll["kind"], "nativeRead");
    let done = app.jobs.complete(
        &json!({"value":[{"token":1,"value":{"ok":true,"value":true}}]}),
        100,
    );
    for done in done {
        app.completed(done);
    }
    let latest = app.jobs.pump(100).unwrap();
    assert_eq!(latest["args"][2][1][2], 0.99);
}
#[test]
fn night_schedule_crosses_midnight_and_names_keep_the_original_rules() {
    assert!(displays::night_active(60., 1320, 420));
    assert!(displays::night_active(1380., 1320, 420));
    assert!(!displays::night_active(800., 1320, 420));
    assert!(!displays::night_active(60., 0, 0));
    assert_eq!(users::suggest("João da Silva"), "joao");
    assert!(users::valid_name("user_name-1"));
    assert!(!users::valid_name("1user"));
    assert!(!users::valid_name("user\nadmin"));
}
use super::host_fixture::fixture;

fn translation_keys(value: &Value, keys: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if !map.contains_key("kind") {
                if let Some(key) = map.get("key").and_then(Value::as_str) {
                    if key.contains('.') {
                        keys.push(key.into());
                    }
                }
            }
            for value in map.values() {
                translation_keys(value, keys);
            }
        }
        Value::Array(items) => {
            for item in items {
                translation_keys(item, keys);
            }
        }
        _ => {}
    }
}
#[test]
fn every_settings_section_and_editor_renders_with_existing_translations() {
    let mut app = fixture();
    let mut navigation = crate::settings_model::Model::default();
    let groups = navigation.request(&json!({"cmd":"catalog"}));
    let mut missing = std::collections::BTreeSet::new();
    for item in arr(&groups).iter().flat_map(arr) {
        let id = s(&item["id"]);
        app.section = id.into();
        let tree = app.tree();
        assert!(
            !tree["children"][1]["children"][1]["children"][0]["children"][0]["children"][0]
                ["children"][0]
                .is_null(),
            "empty page {id}"
        );
        let mut keys = vec![];
        translation_keys(&tree, &mut keys);
        for key in keys {
            if app.tr(&key) == key {
                missing.insert(key);
            }
        }
    }
    for (path, value) in [
        ("addingUser", json!(true)),
        ("manualTime", json!(true)),
        ("zonePicker", json!(true)),
        ("regionPicker", json!(true)),
        ("addKeyboard", json!(true)),
        ("vpnImport", json!(true)),
        (
            "manualPrinter",
            json!({"uri":"ipp://","info":"","location":""}),
        ),
    ] {
        put(&mut app.state, path, value);
    }
    for section in [
        "users", "datetime", "language", "keyboard", "printers", "network",
    ] {
        app.section = section.into();
        app.state["networkPage"] = json!("vpn");
        app.state["vpnType"] = json!("ipsec");
        app.state["vpnForm"] = json!({"auth":"eap"});
        let tree = app.tree();
        let mut keys = vec![];
        translation_keys(&tree, &mut keys);
        for key in keys {
            if app.tr(&key) == key {
                missing.insert(key);
            }
        }
    }
    assert!(missing.is_empty(), "untranslated captions: {missing:?}");
}
#[test]
fn account_validation_and_last_administrator_are_preserved() {
    assert!(users::valid_name("user_1"));
    assert!(!users::valid_name("Root"));
    assert!(!users::valid_name("-root"));
    assert!(!users::valid_name(&"a".repeat(33)));
    assert_eq!(
        users::add_error(&json!({"name":"ana","full":"Ana","pass":"a","pass2":"b"})),
        "settings.users.mismatch"
    );
    let mut app = App::default();
    app.data=serde_json::from_str(r#"{"admin.available":true,"system.info":{"admin":true},"users.list":{"users":[{"name":"ana","admin":true,"self":true}],"autologin":""}}"#).unwrap();
    app.command(&op("userAdmin"));
    assert!(!app.jobs.busy());
    app.state["deleteUser"] = json!(true);
    app.command(&op("userDelete"));
    assert!(!app.jobs.busy());
}
#[test]
fn firmware_replies_are_strict_and_never_infer_an_update() {
    assert_eq!(
        system::eeprom_status("available 123 456 latest").unwrap()["latest"],
        456
    );
    for line in [
        "available 1 2 latest\n",
        "available 1 2 latest extra",
        "uptodate 1 -2 latest",
        "available 11111111111 2 latest",
        "available 1 2 Latest",
    ] {
        assert!(system::eeprom_status(line).is_none(), "{line}");
    }
}
#[test]
fn silent_password_actions_require_current_validation() {
    let mut app = App::default();
    app.data=serde_json::from_str(r#"{"admin.available":true,"system.info":{"admin":true},"users.list":{"users":[{"name":"ana","admin":true,"self":true}]}}"#).unwrap();
    app.state["userPassword"] = json!({"old":"","pass":"secret","pass2":"secret"});
    app.command(&op("userPassword"));
    assert!(!app.jobs.busy());
    app.state["userPassword"]["old"] = json!("old-secret");
    app.command(&op("userPassword"));
    app.jobs.bind(1);
    let effect = app.jobs.pump(0).unwrap();
    assert_eq!(effect["args"][1], "desktop.call");
    assert_eq!(effect["args"][2][1][0], "user-passwd");
    assert_eq!(effect["args"][2][1][2], true);
    assert_eq!(effect["args"][2][1][1], "ana\nsecret\nold-secret");
}
