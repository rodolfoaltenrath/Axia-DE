use super::*;
fn app() -> App {
    let mut a = App::default();
    a.resources(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    a.jobs.bind(7);
    a.booted = true;
    a.folder = "/home/test/Pictures/Camera".into();
    a.folder_ready = true;
    a.device = Some("/dev/video0".into());
    a.cameras = vec![
        json!({"device":"/dev/video0","name":"Desk camera"}),
        json!({"device":"/dev/video1","name":"USB camera"}),
    ];
    a.id = 10;
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
fn photo_collision_names_and_mirror_are_captured_at_shutter_time() {
    let mut a = app();
    a.shoot(1000);
    let base = a.target.as_ref().unwrap().base.clone();
    a.mirror = false;
    complete(&mut a, Tag::Name(0), Ok(json!({"size":42})));
    assert!(a.target.as_ref().unwrap().path.ends_with(" (2).jpg"));
    assert!(a.target.as_ref().unwrap().mirror);
    assert_eq!(a.target.as_ref().unwrap().base, base);
}
#[test]
fn switching_devices_releases_stream_and_rejects_late_open() {
    let mut a = app();
    a.rec_start = Some(500);
    a.video_pending = true;
    a.switch(Some("/dev/video1".into()));
    assert_eq!(a.id, 0);
    assert!(a.rec_start.is_none());
    assert!(!a.video_pending);
    let e = a.jobs.pump(1000).unwrap();
    assert_eq!(e["args"][1], "media.close");
    assert_eq!(e["args"][2], json!([10]));
    complete(&mut a, Tag::Open(0), Ok(json!(99)));
    assert_eq!(a.id, 0);
}
#[test]
fn no_camera_and_permission_denial_are_distinct() {
    let mut a = app();
    a.id = 0;
    a.cameras.clear();
    assert!(ui::tree(&a, 1000).to_string().contains("camera.none"));
    a.cameras = vec![json!({"device":"/dev/video0"})];
    complete(&mut a, Tag::Open(0), Ok(json!(0)));
    assert!(a.denied);
    assert!(ui::tree(&a, 1000)
        .to_string()
        .contains("camera.deniedTitle"));
}
#[test]
fn video_completion_before_start_reply_never_restarts_timer() {
    let mut a = app();
    a.video_pending = true;
    a.event(
        &json!({"kind":"media","id":10,"type":"video","data":{"ok":true,"path":"/capture.mp4"}}),
        1000,
    );
    complete(&mut a, Tag::Video(0), Ok(json!(true)));
    assert!(a.rec_start.is_none());
    assert_eq!(a.shots.len(), 1);
}
#[test]
fn camera_events_before_open_result_are_replayed() {
    let mut a = app();
    a.id = 0;
    a.opening = true;
    a.event(
        &json!({"kind":"media","id":11,"type":"ready","data":{"width":1280,"height":720}}),
        1000,
    );
    assert_eq!(a.deferred.len(), 1);
    complete(&mut a, Tag::Open(0), Ok(json!(11)));
    assert_eq!(a.info["width"], 1280);
    assert!(a.deferred.is_empty());
}
#[test]
fn recent_shots_are_bounded_and_thumbnails_retry_on_revision() {
    let mut a = app();
    for n in 0..20 {
        a.add_shot(&format!("/{n}.jpg"));
    }
    assert_eq!(a.shots.len(), 12);
    assert_eq!(a.shots[0].path, "/19.jpg");
    complete(&mut a, Tag::Thumb("/19.jpg".into()), Ok(Value::Null));
    assert!(!a.shots[0].pending);
    a.event(&json!({"kind":"thumb"}), 1000);
    assert!(a.shots[0].pending);
}
#[test]
fn shutter_rejects_repeated_requests_while_finding_name() {
    let mut a = app();
    a.shoot(1000);
    let first = a.target.as_ref().unwrap().base.clone();
    a.shoot(2000);
    assert_eq!(a.target.as_ref().unwrap().base, first);
    a.rec_start = Some(1);
    a.target = None;
    a.shoot(2000);
    assert!(a.stopping);
    a.shoot(2000);
    assert!(a.stopping);
}
#[test]
fn recording_blocks_mode_change_and_stop_waits_for_video_event() {
    let mut a = app();
    a.mode = "video".into();
    a.rec_start = Some(1);
    a.request_at(&json!({"cmd":"mode","key":"photo"}), 1000);
    assert_eq!(a.mode, "video");
    a.shoot(1000);
    assert!(a.stopping);
    complete(&mut a, Tag::Stop(0), Ok(json!(true)));
    assert!(a.rec_start.is_some());
    a.event(
        &json!({"kind":"media","id":10,"type":"video","data":{"ok":false}}),
        1000,
    );
    assert!(a.rec_start.is_none());
    assert!(!a.stopping);
}
