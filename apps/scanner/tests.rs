use super::*;
#[path = "native_tests.rs"]
mod native;
fn app() -> App {
    let mut a = App::default();
    a.resources(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    a.jobs.bind(7);
    a.booted = true;
    a
}
fn device() -> Value {
    json!({"addr":"192.168.1.2","port":80,"rs":"eSCL","name":"Desk scanner","lan":true})
}
fn caps() -> Value {
    json!({"platen":true,"adf":true,"res":[75,150,300],"colors":["RGB24","Grayscale8"],"formats":["image/jpeg"],"formats2":[],"maxW":2550,"maxH":3508})
}
fn ready() -> App {
    let mut a = app();
    a.scanner = Some(device());
    a.caps = Some(caps());
    a
}
fn complete(a: &mut App, tag: Tag, result: Result<Value, &str>, now: i64) {
    a.apply(
        Completed {
            tag,
            result: result.map_err(str::to_owned),
        },
        now,
    );
}
fn normalized(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(normalized).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), normalized(v))).collect())
        }
        _ => v.clone(),
    }
}
#[test]
fn scan_options_and_regions_match_frozen_original_policy() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../rust/axia-shell/tests/fixtures/scanner_policy.json"
    ))
    .unwrap();
    for c in cases.as_array().unwrap() {
        let mut a = ready();
        a.caps = Some(c["caps"].clone());
        a.area = serde_json::from_value(c["area"].clone()).unwrap();
        a.format = c["format"].as_str().unwrap().into();
        a.res = c["res"].as_u64().unwrap();
        a.color = c["color"].as_str().unwrap().into();
        a.source = c["source"].as_str().unwrap().into();
        a.start_scan();
        let effect = a.jobs.pump(100).unwrap();
        assert_eq!(effect["args"][1], "scanner.scan");
        assert_eq!(
            normalized(&effect["args"][2][1]),
            normalized(&c["expected"]["options"])
        );
        assert_eq!(
            a.save.as_ref().unwrap().direct,
            c["expected"]["direct"].as_bool().unwrap()
        );
    }
}
#[test]
fn discovery_keeps_only_supported_lan_devices_and_caps_choose_supported_defaults() {
    let mut a = app();
    complete(
        &mut a,
        Tag::Discover(0),
        Ok(json!({"scanners":[device(),{"lan":true,"tls":true},{"lan":false,"tls":false}]})),
        100,
    );
    assert_eq!(a.found.as_ref().unwrap().len(), 1);
    complete(
        &mut a,
        Tag::Caps(0),
        Ok(
            json!({"adf":true,"platen":false,"res":[25],"colors":["Grayscale8"],"formats":["image/jpeg"]}),
        ),
        100,
    );
    assert_eq!(a.source, "Feeder");
    assert_eq!(a.res, 25);
    assert_eq!(a.color, "Grayscale8");
    a.scanner = Some(device());
    a.start_preview();
    let effect = a.jobs.pump(100).unwrap();
    assert_eq!(effect["args"][2][1]["res"], 25);
    assert_eq!(effect["args"][2][1]["color"], "Grayscale8");
}
#[test]
fn preview_fits_the_window_and_drag_capture_clamps_including_rtl() {
    let mut a = ready();
    a.width = 640.;
    a.height = 460.;
    a.preview = Some(Image {
        id: 10,
        w: 520.,
        h: 560.,
    });
    let f = a.frame().unwrap();
    assert!(f.x >= 20.);
    assert!(f.x + f.w <= 340.);
    assert!(f.h <= 420.);
    assert_eq!(
        a.request_at(
            &json!({"cmd":"cropStart","x":f.x+f.w*0.8,"y":f.y+f.h*0.9}),
            100
        )["capture"],
        true
    );
    a.request_at(&json!({"cmd":"pointerMove","x":-1000,"y":-1000}), 100);
    let area = a.area.unwrap();
    assert!((area[2] - 0.8).abs() < 1e-9);
    assert!((area[3] - 0.9).abs() < 1e-9);
    assert_eq!(area[0], 0.);
    a.request_at(&json!({"cmd":"dragEnd"}), 100);
    assert!(a.drag.is_none());
    a.rtl = true;
    let rtl = a.frame().unwrap();
    assert!((rtl.x - f.x - 280.).abs() < 1e-9);
    a.request_at(&json!({"cmd":"cropStart","x":rtl.x,"y":rtl.y}), 100);
    a.request_at(&json!({"cmd":"dragEnd"}), 100);
    assert!(a.area.is_none());
}
#[test]
fn switching_devices_cannot_accept_old_caps_or_preview_images() {
    let mut a = ready();
    a.preview = Some(Image {
        id: 10,
        w: 200.,
        h: 200.,
    });
    a.choose(Some(json!({"addr":"192.168.1.3","port":80})));
    assert!(a.preview.is_none());
    let epoch = a.epoch;
    complete(&mut a, Tag::Caps(epoch - 1), Ok(caps()), 100);
    assert!(a.caps.is_none());
    complete(
        &mut a,
        Tag::Decode(epoch - 1),
        Ok(json!({"image":11,"w":200,"h":200})),
        100,
    );
    assert!(a.preview.is_none());
    assert_eq!(a.epoch, epoch);
}
#[test]
fn pdf_only_scanners_use_the_pdf_thumbnail_helper_for_preview() {
    let mut a = ready();
    a.busy = Some(Busy::Preview);
    complete(
        &mut a,
        Tag::Preview(0),
        Ok(json!({"file":"/home/test/.cache/scan.pdf","format":"application/pdf"})),
        100,
    );
    let effect = a.jobs.pump(100).unwrap();
    assert_eq!(effect["args"][1], "images.pdfThumb");
}

// Exercise the actual token transport, including deferred filesystem completion.
fn service(a: &mut App, now: &mut i64, method: &str, result: Result<Value, &str>) -> Value {
    *now += 100;
    let effect = a.jobs.pump(*now).expect("queued request");
    assert_eq!(effect["args"][1], method, "{effect}");
    let args = effect["args"][2].clone();
    assert!(a.jobs.complete(&json!(91), *now).is_empty());
    *now += 100;
    let poll = a.jobs.pump(*now).expect("poll request");
    assert_eq!(poll["method"], "take");
    let packet = match result {
        Ok(value) => json!({"token":91,"value":value}),
        Err(error) => json!({"token":91,"error":error}),
    };
    let done = a
        .jobs
        .complete(&json!({"method":"take","value":[packet]}), *now);
    assert_eq!(done.len(), 1);
    for d in done {
        a.apply(d, *now);
    }
    args
}
fn scan_to_name(a: &mut App, now: &mut i64) {
    a.start_scan();
    service(
        a,
        now,
        "scanner.scan",
        Ok(json!({"file":"/tmp/scanned.jpg"})),
    );
    service(a, now, "fs.stat", Err("ENOENT"));
    service(a, now, "fs.stat", Ok(json!({"dir":true})));
}
#[test]
fn conversion_uses_requested_settings_and_avoids_existing_files() {
    let mut a = ready();
    let mut now = 1000;
    scan_to_name(&mut a, &mut now);
    let first = a.save.as_ref().unwrap().path.clone();
    service(&mut a, &mut now, "fs.stat", Ok(json!({"size":42})));
    let second = a.save.as_ref().unwrap().path.clone();
    assert_ne!(first, second);
    assert!(second.ends_with(" 2.pdf"));
    service(&mut a, &mut now, "fs.stat", Err("ENOENT"));
    a.format = "jpeg".into();
    a.res = 600;
    assert!(a.saved.is_none());
    let args = service(&mut a, &mut now, "images.convert", Ok(Value::Null));
    assert_eq!(
        args,
        json!(["/tmp/scanned.jpg",second,{"format":"pdf","native":true,"dpi":150}])
    );
    assert_eq!(a.saved.as_ref().unwrap().path, second);
    assert_eq!(a.saved.as_ref().unwrap().format, "pdf");
    assert!(a.busy.is_none());
    a.request_at(&json!({"cmd":"print"}), now);
    assert!(!a.jobs.busy(), "PDF must not go to the image print path");
}
#[test]
fn direct_save_waits_for_copy_completion_and_reports_failure() {
    for failed in [false, true] {
        let mut a = ready();
        let mut now = 1000;
        a.format = "jpeg".into();
        scan_to_name(&mut a, &mut now);
        service(&mut a, &mut now, "fs.stat", Err("ENOENT"));
        let args = service(&mut a, &mut now, "fs.copy", Ok(json!(42)));
        assert!(args[1].as_str().unwrap().ends_with(".jpg"));
        assert!(a.saved.is_none());
        assert!(a.notice.is_null());
        complete(
            &mut a,
            Tag::CopyPoll(0),
            Ok(json!([{"id":42,"done":false}])),
            now,
        );
        assert!(a.saved.is_none());
        complete(
            &mut a,
            Tag::CopyPoll(0),
            Ok(json!([{"id":42,"done":true,"failed":failed}])),
            now,
        );
        assert_eq!(a.saved.is_some(), !failed);
        assert_eq!(a.error.is_null(), !failed);
        assert!(a.busy.is_none());
        service(&mut a, &mut now, "fs.forgetJob", Ok(Value::Null));
        if !failed {
            a.format = "pdf".into();
            let effect = a.request_at(&json!({"cmd":"print"}), now);
            assert_eq!(effect["effect"]["args"][1], "print.open");
            assert!(effect["effect"]["args"][2][0]["image"]
                .as_str()
                .unwrap()
                .ends_with(".jpg"));
        }
    }
}
#[test]
fn failed_conversion_never_announces_a_saved_file() {
    let mut a = ready();
    let mut now = 1000;
    scan_to_name(&mut a, &mut now);
    service(&mut a, &mut now, "fs.stat", Err("ENOENT"));
    service(&mut a, &mut now, "images.convert", Err("disk full"));
    assert!(a.saved.is_none());
    assert!(a.notice.is_null());
    assert_eq!(a.error["params"]["error"], "disk full");
}
