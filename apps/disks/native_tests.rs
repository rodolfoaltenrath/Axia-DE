use super::*;
fn disk() -> Value {
    json!({"name":"sdb","model":"USB disk","serial":"TEST-USB","size":32000000000u64,"removable":true,"transport":"usb","system":false,"ro":false,"partitions":[{"name":"sdb1","number":1,"size":16000000000u64,"start":1048576,"fstype":"vfat","label":"DATA","uuid":"TEST-UUID","mountpoint":""}]})
}
fn fixture() -> App {
    let mut app = App::default();
    app.booted = true;
    app.ready = true;
    app.available = true;
    app.jobs.bind(1);
    app.tools = json!({"fat32":1,"ext4":1,"smartctl":true});
    app.update_list(json!([disk()]));
    app.theme = Rc::new(
        json!({"text":"#eee","textDim":"#aaa","windowBg":"#222","sidebarBg":"#333","control":"#444","separator":"#555","accent":"#3584e4"}),
    );
    app
}
#[test]
fn erasing_a_whole_disk_requires_the_visible_name_after_confirmation() {
    let mut app = fixture();
    app.request_at(&json!({"cmd":"act","key":"erase"}), 0);
    app.request_at(&json!({"cmd":"typed","key":"sdb"}), 0);
    app.request_at(&json!({"cmd":"confirm"}), 0);
    assert!(
        !app.busy,
        "a forged confirm in the choose step cannot erase"
    );
    app.request_at(&json!({"cmd":"dialogNext"}), 0);
    assert_eq!(app.dialog.as_ref().unwrap().step, "confirm");
    app.request_at(&json!({"cmd":"typed","key":"sda"}), 0);
    app.request_at(&json!({"cmd":"confirm"}), 0);
    assert!(!app.busy);
    app.request_at(&json!({"cmd":"typed","key":"sdb"}), 0);
    let reply = app.request_at(&json!({"cmd":"confirm"}), 0);
    assert!(app.busy);
    assert_eq!(
        reply["effect"]["args"][1], "disks.snapshot",
        "validate current identity before invoking a destructive helper action"
    );
}
#[test]
fn a_replaced_or_protected_disk_never_reaches_the_helper() {
    for field in ["serial", "system", "ro"] {
        let mut app = fixture();
        let original = disk();
        let operation = Operation {
            action: "erase".into(),
            arg: "sdb\nquick".into(),
            disk: original.clone(),
            part: Value::Null,
            dialog: true,
            revision: 0,
            generation: 1,
        };
        app.busy = true;
        let mut live = original;
        if field == "serial" {
            live[field] = json!("REPLACEMENT");
        } else {
            live[field] = json!(true);
        }
        app.apply(
            Completed {
                tag: Tag::Validate(operation),
                result: Ok(json!({"list":[live],"admin":true})),
            },
            100,
        );
        assert!(!app.busy);
        assert_eq!(app.job_error, key("changed"));
        assert!(!app.jobs.busy());
    }
}
#[test]
fn mounting_and_renaming_obey_the_partition_and_admin_constraints() {
    let mut internal = disk();
    internal["removable"] = json!(false);
    let part = internal["partitions"][0].clone();
    assert!(!policy::available("format", &internal, &part, false));
    assert!(policy::available("format", &internal, &part, true));
    internal["system"] = json!(true);
    assert!(!policy::available("unmount", &internal, &part, true));
    for bad in ["-option", "bad\nname", "bad/name"] {
        assert!(!policy::label_valid(bad, "ext4"));
    }
    assert_eq!(policy::clean_label("usb data", "fat32"), "USB DATA");
    assert!(!policy::label_valid("twelve chars!", "fat32"));
}
#[test]
fn stale_snapshots_and_progress_cannot_replace_newer_events() {
    let mut app = fixture();
    app.list_revision = 2;
    let mut fresh = disk();
    fresh["model"] = json!("New USB");
    app.update_list(json!([fresh]));
    app.apply(
        Completed {
            tag: Tag::Snapshot(1),
            result: Ok(json!({"available":true,"list":[disk()],"tools":{},"admin":false})),
        },
        0,
    );
    assert_eq!(app.disk()["model"], "New USB");
    app.operation_generation = 5;
    app.apply(Completed{tag:Tag::Events,result:Ok(json!([{"kind":"progress","generation":4,"line":"progress done=100 total=100"},{"kind":"progress","generation":5,"line":"progress done=20 total=100 phase=write"}]))},0);
    assert_eq!(app.progress["done"], 20.);
    assert_eq!(app.progress["phase"], "write");
}
#[test]
fn idle_windows_do_not_poll_smart_and_running_tests_keep_their_deadline() {
    let mut app = fixture();
    assert_eq!(app.refresh_ms(), None);
    assert!(!app.jobs.busy());
    app.health
        .insert("sdb".into(), json!({"m":{"selfTest":{"running":true}}}));
    app.now = 1000;
    app.self_test_at = 16000;
    assert_eq!(app.refresh_ms(), Some(15000.));
    app.now = 7000;
    assert_eq!(app.refresh_ms(), Some(9000.));
}
#[test]
fn native_pages_keep_details_and_confirmation_scope_visible() {
    let mut app = fixture();
    let tree = ui::tree(&mut app).to_string();
    assert!(tree.contains("USB disk"));
    assert!(tree.contains("disks.checkHealth"));
    app.request_at(&json!({"cmd":"act","key":"format"}), 0);
    app.request_at(&json!({"cmd":"dialogNext"}), 0);
    let tree = ui::tree(&mut app).to_string();
    for value in [
        "disks.confirm.disk",
        "disks.confirm.irreversible",
        "sdb1",
        "DATA",
        "disks.confirm.typeName",
    ] {
        assert!(tree.contains(value), "missing confirmation detail {value}");
    }
}

#[test]
fn late_smart_results_do_not_describe_a_replacement_device() {
    let mut app = fixture();
    let identity = policy::fingerprint(&disk(), &Value::Null);
    let mut replacement = disk();
    replacement["serial"] = json!("NEW-DEVICE");
    app.update_list(json!([replacement]));
    app.apply(
        Completed {
            tag: Tag::Health {
                disk: "sdb".into(),
                purpose: "read".into(),
                identity,
            },
            result: Ok(json!({"src":"own","lines":["smart {\"health\":\"ok\"}"]})),
        },
        100,
    );
    assert!(!app.health.contains_key("sdb"));
}

#[test]
fn failed_self_test_polls_keep_the_fifteen_second_retry_interval() {
    let mut app = fixture();
    app.health
        .insert("sdb".into(), json!({"m":{"selfTest":{"running":true}}}));
    app.now = 10000;
    app.apply(
        Completed {
            tag: Tag::Health {
                disk: "sdb".into(),
                purpose: "poll".into(),
                identity: policy::fingerprint(&disk(), &Value::Null),
            },
            result: Err("unavailable".into()),
        },
        10000,
    );
    assert_eq!(app.refresh_ms(), Some(15000.));
    assert!(app.self_test_running());
}

#[test]
fn an_overflow_menu_activates_the_highlighted_item() {
    let mut app = fixture();
    app.width = 500.;
    app.menu = true;
    ui::tree(&mut app);
    app.menu_active = app
        .menu_actions
        .iter()
        .position(|action| action == "erase")
        .expect("erase must overflow") as i64;
    app.key("Enter", "", 0);
    assert_eq!(app.dialog.as_ref().unwrap().kind, "erase");
    assert!(!app.busy);
}

#[test]
fn changed_partition_layout_requires_a_new_confirmation() {
    let original = disk();
    let mut changed = original.clone();
    changed["partitions"][0]["uuid"] = json!("NEW-VOLUME");
    assert!(policy::current(&json!([changed]), &original, &Value::Null).is_none());
}

#[test]
fn starting_a_smart_test_retains_capabilities_and_reads_its_live_status() {
    let mut app = fixture();
    app.health.insert("sdb".into(),json!({"src":"smartctl","m":{"supported":true,"selfTest":{"supported":true,"running":false,"polling":{"short":2,"long":54}}}}));
    app.apply(Completed{tag:Tag::Health{disk:"sdb".into(),purpose:"short".into(),identity:policy::fingerprint(&disk(),&Value::Null)},result:Ok(json!({"src":"smartctl","status":"1 0","lines":["{\"smartctl\":{\"exit_status\":0}}"]}))},1000);
    let test = &app.health["sdb"]["m"]["selfTest"];
    assert_eq!(test["supported"], true);
    assert_eq!(test["running"], true);
    assert_eq!(test["polling"]["short"], 2);
    assert_eq!(app.jobs.pump(1000).unwrap()["args"][1], "disks.smartHealth");
}

#[test]
fn text_fields_use_the_native_edit_payload_before_submitting() {
    let mut app = fixture();
    app.request_at(&json!({"cmd":"act","key":"format"}), 0);
    app.request_at(&json!({"cmd":"label","value":"new usb"}), 0);
    assert_eq!(app.dialog.as_ref().unwrap().label, "new usb");
    app.request_at(&json!({"cmd":"dialogNext"}), 0);
    app.request_at(&json!({"cmd":"typed","value":"sdb"}), 0);
    let reply = app.request_at(&json!({"cmd":"confirm"}), 0);
    assert!(app.busy);
    assert_eq!(reply["effect"]["args"][1], "disks.snapshot");
}

#[test]
fn installing_smart_does_not_read_a_device_that_replaced_the_requested_disk() {
    let mut app = fixture();
    let identity = policy::fingerprint(&disk(), &Value::Null);
    let mut replacement = disk();
    replacement["serial"] = json!("NEW-USB");
    app.update_list(json!([replacement]));
    app.apply(
        Completed {
            tag: Tag::Install("sdb".into(), identity),
            result: Ok(json!({"tools":{"smartctl":true}})),
        },
        0,
    );
    assert!(!app.jobs.busy());
}
