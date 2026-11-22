//! The requests as `lord.print` makes them: an id at once, the result later with poll(), through
//! the same C ABI js_print.c uses. The state is one per process, so everything runs in one test.
mod support;

use axia_print::cups;
use axia_print::ipp::{self, Builder, G_JOB, G_OPERATION, G_PRINTER};
use serde_json::{json, Value};
use std::ffi::{c_char, c_int, CStr, CString};
use std::os::fd::IntoRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::{serve_tcp, serve_unix, temp_dir, Rep};

extern "C" {
    fn print_task(kind: *const c_char, json: *const c_char, len: usize, docfd: c_int, doclen: usize) -> c_int;
    fn print_poll() -> *mut c_char;
    fn print_pending() -> c_int;
    fn print_has_cups() -> c_int;
    fn cups_valid_name(s: *const c_char) -> c_int;
    fn cups_valid_uri(s: *const c_char) -> c_int;
    fn cups_valid_text(s: *const c_char, max: usize) -> c_int;
    fn phttp_lan_ipv4(s: *const c_char) -> c_int;
    fn escl_valid_rs(s: *const c_char) -> c_int;
}

fn task(kind: &str, json: Option<&str>) -> i32 {
    let k = CString::new(kind).unwrap();
    match json {
        Some(j) => unsafe { print_task(k.as_ptr(), j.as_ptr().cast(), j.len(), -1, 0) },
        None => unsafe { print_task(k.as_ptr(), std::ptr::null(), 0, -1, 0) },
    }
}

fn poll_all(ids: usize) -> Vec<Value> {
    let end = Instant::now() + Duration::from_secs(20);
    let mut out = Vec::new();
    while out.len() < ids {
        assert!(Instant::now() < end, "timed out with {out:?}");
        let p = unsafe { print_poll() };
        if p.is_null() {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let s = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
        unsafe { libc::free(p.cast()) };
        let v: Value = serde_json::from_str(&s).expect("poll gives a JSON array");
        out.extend(v.as_array().unwrap().iter().cloned());
    }
    out
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

#[test]
fn tasks_run_in_threads_and_their_results_are_polled() {
    let dir = temp_dir("tasks");
    let sock = dir.join("cups.sock");
    let gate = Arc::new(AtomicBool::new(false));
    let gate2 = gate.clone();
    serve_unix(&sock, move |r| {
        let m = ipp::parse(&r.body).unwrap();
        let mut b = Builder::new(0, 1);
        b.std(None, None);
        match m.code {
            ipp::OP_CUPS_GET_DEFAULT => {
                // the slow one: holds the tasks until the test lets go
                while !gate2.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                b.group(G_PRINTER).add_str(ipp::T_NAME, Some("printer-name"), "P1");
            }
            ipp::OP_CUPS_GET_PRINTERS => {
                b.group(G_PRINTER).add_str(ipp::T_NAME, Some("printer-name"), "P1");
            }
            ipp::OP_PRINT_JOB => {
                assert_eq!(r.body.len() - m.used, 6, "the document of the task");
                assert_eq!(&r.body[m.used..], b"%PDF-1");
                b.group(G_JOB).add_int(ipp::T_INTEGER, Some("job-id"), 9);
            }
            c => {
                assert_eq!(m.find(G_OPERATION, "requesting-user-name").is_some(), true, "{c:#x}");
            }
        }
        b.end();
        Rep::ok(b.finish().unwrap())
    });
    cups::set_socket_path(Some(sock.to_str().unwrap()));
    unsafe { assert_eq!(print_has_cups(), 1) };

    // eight at once is the limit; the ninth is refused while they run
    let ids: Vec<i32> = (0..8).map(|_| task("printers", None)).collect();
    assert!(ids.iter().all(|&i| i > 0) && ids.windows(2).all(|w| w[1] == w[0] + 1), "{ids:?}");
    assert_eq!(unsafe { print_pending() }, 8);
    assert_eq!(task("printers", None), -1, "too many at once");
    gate.store(true, Ordering::SeqCst);
    let done = poll_all(8);
    assert_eq!(done.iter().map(|e| e["id"].as_i64().unwrap() as i32).collect::<std::collections::BTreeSet<_>>(), ids.iter().copied().collect());
    assert!(done.iter().all(|e| e["kind"] == "printers" && e["ok"] == true && e["data"]["default"] == "P1" && e["data"]["printers"][0]["name"] == "P1"), "{done:?}");
    assert_eq!(unsafe { print_pending() }, 0);
    assert!(unsafe { print_poll() }.is_null(), "nothing is left to read");

    // arguments that are wrong, a kind nobody knows
    let bad = task("cancel", Some("{not json"));
    let unknown = task("explode", None);
    let name = task("remove", Some(r#"{"name":"a b"}"#));
    let long = task("a-kind-that-is-too-long", None);
    assert_eq!(long, -1);
    let done = poll_all(3);
    let by = |id: i32| done.iter().find(|e| e["id"] == id).unwrap().clone();
    assert_eq!((by(bad)["ok"].clone(), by(bad)["data"].clone()), (json!(false), json!({ "error": "bad-request" })));
    assert_eq!((by(unknown)["ok"].clone(), by(unknown)["data"].clone()), (json!(false), json!({ "error": "unknown" })));
    assert_eq!(by(name)["data"], json!({ "error": "bad-request", "detail": "name" }));

    // a print with its document: the descriptor is the task's from the call on
    let pdf = dir.join("a.pdf");
    std::fs::write(&pdf, b"%PDF-1").unwrap();
    let fd = std::fs::File::open(&pdf).unwrap().into_raw_fd();
    let (k, j) = (c("submit"), r#"{"printer":"P1","options":{"copies":1}}"#);
    let id = unsafe { print_task(k.as_ptr(), j.as_ptr().cast(), j.len(), fd, 6) };
    let e = poll_all(1).remove(0);
    assert_eq!((e["id"].as_i64(), e["ok"].clone(), e["data"].clone()), (Some(id as i64), json!(true), json!({ "job": 9 })));
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1, "the task closed it");
    // refused tasks close the descriptor too
    let fd = std::fs::File::open(&pdf).unwrap().into_raw_fd();
    assert_eq!(unsafe { print_task(c("a-kind-that-is-too-long").as_ptr(), std::ptr::null(), 0, fd, 6) }, -1);
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);

    // a scanner through the tasks, and the folder the scans go to
    let home = dir.join("home");
    std::fs::create_dir(&home).unwrap();
    std::env::set_var("HOME", &home);
    let port = serve_tcp(|r| match r.method.as_str() {
        "POST" => Rep::status(201).with("Location", "/eSCL/ScanJobs/1"),
        _ => Rep::ok(b"JPEGDATA".to_vec()).with("Content-Type", "image/jpeg"),
    });
    let id = task("scan", Some(&format!(r#"{{"addr":"127.0.0.1","port":{port},"settings":{{"res":150}}}}"#)));
    let e = poll_all(1).remove(0);
    assert_eq!((e["id"].as_i64(), e["ok"].clone(), e["data"]["bytes"].clone()), (Some(id as i64), json!(true), json!(8)), "{e}");
    let file = e["data"]["file"].as_str().unwrap();
    assert!(file.starts_with(home.join(".cache/axia/scan/").to_str().unwrap()) && file.ends_with(".jpg"), "{file}");
    let id = task("scancaps", Some(r#"{"addr":"8.8.8.8"}"#));
    assert_eq!(poll_all(1).remove(0)["data"], json!({ "error": "bad-request", "detail": "host" }), "{id}");
    std::env::set_var("HOME", "relative");
    task("scan", Some(r#"{"addr":"127.0.0.1"}"#));
    assert_eq!(poll_all(1).remove(0)["data"], json!({ "error": "write" }));

    // the validators JS reaches through the same ABI
    unsafe {
        assert_eq!((cups_valid_name(c("Oficina_2").as_ptr()), cups_valid_name(c("a b").as_ptr()), cups_valid_name(std::ptr::null())), (1, 0, 0));
        assert_eq!((cups_valid_uri(c("ipp://1.2.3.4/x").as_ptr()), cups_valid_uri(c("file:///etc/passwd").as_ptr()), cups_valid_uri(std::ptr::null())), (1, 0, 0));
        assert_eq!((cups_valid_text(std::ptr::null(), 3), cups_valid_text(c("abc").as_ptr(), 3), cups_valid_text(c("abcd").as_ptr(), 3)), (1, 1, 0));
        assert_eq!((phttp_lan_ipv4(c("192.168.0.1").as_ptr()), phttp_lan_ipv4(c("1.1.1.1").as_ptr()), phttp_lan_ipv4(std::ptr::null())), (1, 0, 0));
        assert_eq!((escl_valid_rs(c("eSCL").as_ptr()), escl_valid_rs(c("../x").as_ptr())), (1, 0));
    }
    cups::set_socket_path(Some("/nonexistent/cups.sock"));
    assert_eq!(unsafe { print_has_cups() }, 0);
    let id = task("jobs", None);
    let e = poll_all(1).remove(0);
    assert_eq!((e["id"].as_i64(), e["data"].clone()), (Some(id as i64), json!({ "error": "no-cups", "detail": "socket" })));
}
