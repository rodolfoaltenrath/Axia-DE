use crate::notifications::*;

#[test]
fn native_profile_and_catalog_determine_notification_caption_and_timestamp() {
    let path = std::env::temp_dir().join(format!(
        "axia-notification-presentation-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    let home = crate::settings::profile(path.to_str().unwrap());
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let assets = std::ffi::CString::new(assets.to_str().unwrap()).unwrap();
    unsafe {
        crate::native_view::axia_view_init(assets.as_ptr(), std::ptr::null());
    }
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.set("language", &json!("en"));
        s.set("region", &json!("en_US"));
        s.set("clock", &json!({"hour24":"24"}));
    });
    let catalog = axia_state::Store::new(
        json!({"registry":[{"id":"clock","name":"Wrong manifest caption"}]}),
    );
    let mut service = Service::default();
    service.bind_settings(&home);
    service.bind_catalog(catalog.reader());
    let now = Instant::now();
    service.command(&json!({"action":"push","lang":"fr","region":"fr_FR","wallMs":0,"minute":0,
        "notice":{"app":"clock","title":"Caption","sound":false,"hasApp":false,"displayLabel":"Forged caption","timeLabel":"Forged time"}}), now);
    let reader = service.reader();
    let state = reader.read();
    let notice = &state.history[0];
    assert_eq!(notice["displayLabel"], "Clock");
    assert_eq!(notice["hasApp"], true);
    let stamp = notice["time"].as_i64().unwrap();
    assert!(stamp > 0);
    let local = axia_clock::local(stamp);
    assert_eq!(
        notice["timeLabel"],
        format!("{:02}:{:02}", local.tm_hour, local.tm_min)
    );
    drop(state);
    service.command(&json!({"action":"push","notice":{"app":"unknown","label":"Caller label","hasApp":true,"displayLabel":"Forged","sound":false}}), now);
    assert_eq!(reader.read().history[0]["displayLabel"], "Caller label");
    assert_eq!(reader.read().history[0]["hasApp"], false);
    catalog.set(json!({"registry":[{"id":"custom","name":"Manifest name"}]}));
    service.command(
        &json!({"action":"push","notice":{"app":"custom","sound":false}}),
        now,
    );
    assert_eq!(reader.read().history[0]["displayLabel"], "Manifest name");
    std::fs::remove_dir(&path).unwrap();
}
