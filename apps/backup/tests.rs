use super::*;
#[test]
fn changing_destination_invalidates_directories_and_confirmations_even_with_repeated_ids() {
    let mut a = app();
    a.items = vec![json!({"name":"old-disk.txt"})];
    a.ask("delete");
    let previous = a.browse_gen;
    a.set_status(json!({"state":"idle","dest":"/another-disk","destAvailable":true}));
    assert!(a.ask.is_none());
    assert!(a.items.is_empty());
    assert!(a.selected.is_none());
    complete(&mut a, Tag::Ls(previous), Ok(json!([{"name":"obsolete"}])));
    assert!(a.items.is_empty());
    let generation = a.catalog_gen;
    complete(
        &mut a,
        Tag::List(generation),
        Ok(json!([{"id":"new","time":200}])),
    );
    assert_eq!(a.selected.as_deref(), Some("new"));
    assert!(a.browse_gen > previous);
}
#[test]
fn status_lines_match_frozen_original_header_policy() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../rust/axia-shell/tests/fixtures/backup_header.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let mut a = app();
        a.status = case["status"].clone();
        let mut caption = a.header_line(1000);
        if caption["params"]["when"].is_object() {
            caption["params"]["when"] = json!("WHEN");
        }
        assert_eq!(
            a.i18n.caption(&caption),
            case["expected"].as_str().unwrap(),
            "{case}"
        );
    }
}
#[path = "native_tests.rs"]
mod native;
#[test]
fn successful_configuration_submits_key_creation_in_order() {
    let mut a = app();
    a.after_config = Some("password".into());
    a.saving = true;
    complete(
        &mut a,
        Tag::Configure,
        Ok(json!({"dest":"/backup","encrypt":true})),
    );
    assert!(!a.saving);
    assert!(a.unlock_pending);
    assert!(a.after_config.is_none());
    let effect = a.jobs.pump(1000).unwrap();
    assert_eq!(effect["args"][1], "backup.unlock");
    assert_eq!(effect["args"][2], json!(["password", true]));
}
fn app() -> App {
    let mut a = App::default();
    a.resources(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    a.jobs.bind(7);
    a.booted = true;
    a.config_ready = true;
    a.tab = "snapshots".into();
    a.status = json!({"state":"idle","dest":"/backup","destAvailable":true});
    a.config["dest"] = json!("/backup");
    a.snapshots = vec![
        json!({"id":"new","time":200}),
        json!({"id":"old","time":100}),
    ];
    a.selected = Some("new".into());
    a
}
fn complete(a: &mut App, tag: Tag, result: Result<Value, &str>) {
    a.apply(
        Completed {
            tag,
            result: result.map_err(str::to_owned),
        },
        1000,
    );
}
#[test]
fn native_policy_never_restores_without_a_selected_item_or_folder() {
    let mut a = app();
    a.request_at(&json!({"cmd":"restore"}), 1000);
    assert!(!a.jobs.busy());
    a.rel = "Documents".into();
    let r = a.request_at(&json!({"cmd":"restore"}), 1000);
    assert_eq!(r["effect"]["args"][1], "backup.restore");
    assert_eq!(
        r["effect"]["args"][2],
        json!(["new","Documents",{"replace":false,"suffix":" (restored)"}])
    );
}
#[test]
fn confirmation_captures_snapshot_and_replace_policy_and_blocks_background_commands() {
    let mut a = app();
    a.replace = true;
    a.ask("all");
    a.request_at(&json!({"cmd":"snapshot","key":"old"}), 1000);
    a.request_at(&json!({"cmd":"replace"}), 1000);
    assert_eq!(a.selected.as_deref(), Some("new"));
    assert!(a.replace);
    a.selected = Some("old".into());
    a.replace = false;
    let r = a.request_at(&json!({"cmd":"confirm"}), 1000);
    assert_eq!(
        r["effect"]["args"][2],
        json!(["new","",{"replace":true,"suffix":" (restored)"}])
    );
}
#[test]
fn confirmation_waits_while_engine_is_busy_and_can_be_cancelled() {
    let mut a = app();
    a.ask("delete");
    a.status["state"] = json!("running");
    a.confirm();
    assert!(a.ask.is_some());
    assert!(!a.jobs.busy());
    a.key("Escape", "", 0);
    assert!(a.ask.is_none());
    a.status["state"] = json!("idle");
    a.ask("delete");
    a.snapshots.clear();
    a.confirm();
    assert!(!a.jobs.busy());
}
#[test]
fn stale_directory_responses_do_not_publish_into_new_navigation() {
    let mut a = app();
    a.browse_gen = 2;
    a.rel = "Other".into();
    complete(&mut a, Tag::Ls(1), Ok(json!([{"name":"wrong"}])));
    assert!(a.items.is_empty());
    complete(
        &mut a,
        Tag::Ls(2),
        Ok(json!([{"name":"ok","dir":true},{"name":"../bad"},{"name":".."}])),
    );
    assert_eq!(a.items.len(), 1);
}
#[test]
fn removed_snapshot_moves_to_latest_and_clears_old_directory() {
    let mut a = app();
    a.rel = "gone".into();
    a.catalog_gen = 1;
    complete(&mut a, Tag::List(1), Ok(json!([{"id":"other","time":300}])));
    assert_eq!(a.selected.as_deref(), Some("other"));
    assert!(a.rel.is_empty());
    assert!(a.pick.is_none());
}
#[test]
fn encryption_validates_then_configures_before_unlocking() {
    let mut a = app();
    a.p1 = "short".into();
    a.p2 = a.p1.clone();
    a.turn_on();
    assert_eq!(a.encryption_error, key("backup.encShort"));
    assert!(!a.jobs.busy());
    a.p1 = "password".into();
    a.turn_on();
    assert_eq!(a.encryption_error, key("backup.encMismatch"));
    a.p2 = a.p1.clone();
    a.turn_on();
    let effect = a.jobs.pump(1000).unwrap();
    assert_eq!(effect["args"][1], "backup.configure");
    assert_eq!(effect["args"][2][0]["encrypt"], true);
    assert!(!a.unlock_pending);
    complete(&mut a, Tag::Configure, Err("backup.badDest"));
    assert!(!a.saving);
    assert!(a.after_config.is_none());
    assert!(!a.unlock_pending);
}
#[test]
fn successful_key_setup_clears_passphrases_but_unrelated_updates_keep_draft() {
    let mut a = app();
    a.p1 = "password".into();
    a.p2 = a.p1.clone();
    a.want_encrypt = true;
    a.status["unlock"] = json!({"done":false,"ok":0});
    let mut v = a.status.clone();
    v["unlock"] = json!({"done":true,"ok":1});
    a.set_status(v.clone());
    assert!(a.p1.is_empty() && a.p2.is_empty());
    assert!(!a.want_encrypt);
    a.p1 = "new draft".into();
    a.set_status(v);
    assert_eq!(a.p1, "new draft");
}
#[test]
fn folder_expansion_and_exclusion_cleanup_preserve_original_policy() {
    let mut a = app();
    a.folder = " ~/Archives ".into();
    a.use_folder();
    assert_eq!(
        a.jobs.pump(1000).unwrap()["args"][2][0]["dest"],
        "/home/test/Archives"
    );
    let mut a = app();
    a.exclude = " ~/Videos/Large/// ".into();
    a.exclusion_add();
    assert_eq!(
        a.jobs.pump(1000).unwrap()["args"][2][0]["exclude"],
        json!(["Videos/Large"])
    );
}
