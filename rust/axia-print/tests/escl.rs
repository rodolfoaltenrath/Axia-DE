//! The eSCL client against a fake scanner on 127.0.0.1: its capabilities, a whole scan (the job,
//! the wait while it is busy, the file) and the replies it must not trust.
mod support;

use axia_print::escl;
use axia_print::Fail;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use support::{serve_tcp, temp_dir, Rep};

const CAPS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<scan:ScannerCapabilities xmlns:scan="http://schemas.hp.com/imaging/escl/2011/05/03" xmlns:pwg="http://www.pwg.org/schemas/2010/12/sm">
<pwg:Version>2.63</pwg:Version><pwg:MakeAndModel>HP Smart Tank 7300</pwg:MakeAndModel>
<scan:Platen><scan:PlatenInputCaps><scan:MaxWidth>2550</scan:MaxWidth><scan:MaxHeight>3507</scan:MaxHeight>
<scan:SettingProfiles><scan:SettingProfile>
<scan:ColorModes><scan:ColorMode>BlackAndWhite1</scan:ColorMode><scan:ColorMode>Grayscale8</scan:ColorMode><scan:ColorMode>RGB24</scan:ColorMode><scan:ColorMode>RGB24</scan:ColorMode></scan:ColorModes>
<scan:SupportedResolutions><scan:DiscreteResolutions>
<scan:DiscreteResolution><scan:XResolution>200</scan:XResolution><scan:YResolution>200</scan:YResolution></scan:DiscreteResolution>
<scan:DiscreteResolution><scan:XResolution>300</scan:XResolution><scan:YResolution>300</scan:YResolution></scan:DiscreteResolution>
<scan:DiscreteResolution><scan:XResolution>300</scan:XResolution><scan:YResolution>300</scan:YResolution></scan:DiscreteResolution>
</scan:DiscreteResolutions></scan:SupportedResolutions>
<scan:DocumentFormats><pwg:DocumentFormat>application/pdf</pwg:DocumentFormat><pwg:DocumentFormat>image/jpeg</pwg:DocumentFormat><scan:DocumentFormatExt>image/jpeg</scan:DocumentFormatExt></scan:DocumentFormats>
</scan:SettingProfile></scan:SettingProfiles></scan:PlatenInputCaps></scan:Platen>
<scan:Adf><scan:AdfSimplexInputCaps><scan:MaxWidth>2550</scan:MaxWidth></scan:AdfSimplexInputCaps></scan:Adf>
</scan:ScannerCapabilities>"#;

#[test]
fn what_the_scanner_can_do() {
    let port = serve_tcp(|r| {
        assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/eSCL/ScannerCapabilities"));
        Rep::ok(CAPS.as_bytes().to_vec())
    });
    let j = escl::caps("127.0.0.1", port as i32, "eSCL").unwrap();
    assert_eq!(
        j,
        json!({
            "model": "HP Smart Tank 7300", "platen": true, "adf": true, "maxW": 2550, "maxH": 3507,
            "colors": ["BlackAndWhite1", "Grayscale8", "RGB24"],
            "formats": ["image/jpeg"], "formats2": ["application/pdf", "image/jpeg"], "res": [200, 300]
        })
    );
}

#[test]
fn a_scanner_that_lies_about_its_size_gets_the_defaults() {
    let port = serve_tcp(|_| Rep::ok(b"<MaxWidth>99999999</MaxWidth><MaxHeight>5</MaxHeight><Make>x</Make>".to_vec()));
    let j = escl::caps("127.0.0.1", port as i32, "eSCL").unwrap();
    assert_eq!((j["maxW"].as_i64(), j["maxH"].as_i64(), j["platen"].as_bool(), j["adf"].as_bool()), (Some(2550), Some(3508), Some(false), Some(false)));
    assert_eq!((j["model"].as_str(), j["colors"].as_array().map(Vec::len)), (Some(""), Some(0)));
}

#[test]
fn caps_failures() {
    let port = serve_tcp(|_| Rep::status(404));
    assert_eq!(escl::caps("127.0.0.1", port as i32, "eSCL").unwrap_err(), Fail::new("http", "404"));
    let port = serve_tcp(|_| Rep::status(200));
    assert_eq!(escl::caps("127.0.0.1", port as i32, "eSCL").unwrap_err(), Fail::new("http", "200"), "an empty body");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    assert_eq!(escl::caps("127.0.0.1", port as i32, "eSCL").unwrap_err(), Fail::new("unreachable", ""));
}

#[test]
fn a_whole_scan_waits_while_busy_and_keeps_the_file() {
    let dir = temp_dir("scan");
    let posted = Arc::new(Mutex::new(String::new()));
    let (posted2, polls) = (posted.clone(), Arc::new(AtomicUsize::new(0)));
    let polls2 = polls.clone();
    let image: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
    let img = image.clone();
    let port = serve_tcp(move |r| match (r.method.as_str(), r.path.as_str()) {
        ("POST", "/eSCL/ScanJobs") => {
            assert_eq!((r.header("Content-Type"), r.header("Content-Length").is_some()), (Some("text/xml"), true));
            *posted2.lock().unwrap() = String::from_utf8_lossy(&r.body).into_owned();
            Rep::status(201).with("Location", "http://127.0.0.1:1/eSCL/ScanJobs/abc-123")
        }
        ("GET", "/eSCL/ScanJobs/abc-123/NextDocument") => {
            if polls2.fetch_add(1, Ordering::SeqCst) < 2 {
                Rep::status(503)
            } else {
                Rep { chunked: true, ..Rep::ok(img.clone()) }.with("Content-Type", "image/png")
            }
        }
        other => panic!("unexpected {other:?}"),
    });
    let s = json!({ "source": "Feeder", "color": "Grayscale8", "res": 600, "format": "application/pdf", "x": 10, "y": 20, "w": 1000, "h": 2000 });
    let j = escl::scan("127.0.0.1", port as i32, "eSCL", &s, &dir).unwrap();
    let file = std::path::PathBuf::from(j["file"].as_str().unwrap());
    assert_eq!((file.parent().unwrap(), j["format"].as_str(), j["bytes"].as_u64()), (dir.as_path(), Some("png"), Some(200_000)), "the type comes from what the scanner says");
    assert!(file.file_name().unwrap().to_str().unwrap().starts_with("scan-") && file.extension().unwrap() == "png");
    assert_eq!(std::fs::read(&file).unwrap(), image);
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(polls.load(Ordering::SeqCst), 3, "twice busy, then the document");
    let xml = posted.lock().unwrap().clone();
    for want in [
        "<pwg:InputSource>Feeder</pwg:InputSource>", "<scan:ColorMode>Grayscale8</scan:ColorMode>", "<scan:XResolution>600</scan:XResolution>", "<scan:YResolution>600</scan:YResolution>",
        "<pwg:DocumentFormat>application/pdf</pwg:DocumentFormat>", "<pwg:Height>2000</pwg:Height>", "<pwg:Width>1000</pwg:Width>", "<pwg:XOffset>10</pwg:XOffset>", "<pwg:YOffset>20</pwg:YOffset>",
    ] {
        assert!(xml.contains(want), "{want} in {xml}");
    }
    // a second scan in the same second never overwrites the first
    let j2 = escl::scan("127.0.0.1", port as i32, "eSCL", &json!({}), &dir).unwrap();
    assert_ne!(j2["file"], j["file"]);
}

#[test]
fn defaults_apply_to_what_is_not_valid() {
    let dir = temp_dir("scan-defaults");
    let posted = Arc::new(Mutex::new(String::new()));
    let posted2 = posted.clone();
    let port = serve_tcp(move |r| match r.method.as_str() {
        "POST" => {
            *posted2.lock().unwrap() = String::from_utf8_lossy(&r.body).into_owned();
            Rep::status(201).with("Location", "/eSCL/ScanJobs/z/")
        }
        _ => {
            assert_eq!(r.path, "/eSCL/ScanJobs/z/NextDocument");
            Rep::ok(b"\xff\xd8jpeg".to_vec()).with("Content-Type", "application/octet-stream")
        }
    });
    let s = json!({ "source": "Tray", "color": "Pink", "res": 5, "format": "text/html" });
    let j = escl::scan("127.0.0.1", port as i32, "eSCL", &s, &dir).unwrap();
    assert_eq!(j["format"], "jpg", "an unknown type falls back to the one asked for");
    let xml = posted.lock().unwrap().clone();
    for want in ["<pwg:InputSource>Platen</pwg:InputSource>", "<scan:ColorMode>RGB24</scan:ColorMode>", "<scan:XResolution>300</scan:XResolution>", "image/jpeg", "<pwg:Width>2550</pwg:Width>", "<pwg:Height>3508</pwg:Height>"] {
        assert!(xml.contains(want), "{want} in {xml}");
    }
}

#[test]
fn scan_failures() {
    let dir = temp_dir("scan-fail");
    let run = |h: fn(&support::Req) -> Rep| {
        let port = serve_tcp(h);
        escl::scan("127.0.0.1", port as i32, "eSCL", &json!({}), &dir).unwrap_err()
    };
    assert_eq!(run(|_| Rep::status(503)), Fail::new("busy", ""));
    assert_eq!(run(|_| Rep::status(409)), Fail::new("http", "409"));
    assert_eq!(run(|_| Rep::status(201)), Fail::new("http", "201"), "a job with no Location");
    assert_eq!(run(|_| Rep::status(201).with("Location", "/eSCL/../../etc")), Fail::new("bad-response", "location"));
    assert_eq!(run(|_| Rep::status(201).with("Location", "relative")), Fail::new("bad-response", "location"));
    assert_eq!(run(|r| if r.method == "POST" { Rep::status(201).with("Location", "/eSCL/ScanJobs/a") } else { Rep::status(404) }), Fail::new("http", "404"));
    // no folder to write into
    let port = serve_tcp(|r| if r.method == "POST" { Rep::status(201).with("Location", "/j") } else { Rep::ok(b"x".to_vec()) });
    let e = escl::scan("127.0.0.1", port as i32, "eSCL", &json!({}), &dir.join("missing")).unwrap_err();
    assert_eq!(e.code(), "write");
    assert!(e.detail().contains("No such file"), "{}", e.detail());
}
