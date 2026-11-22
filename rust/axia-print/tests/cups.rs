//! The CUPS client against a fake CUPS on a unix socket: what it asks (the IPP attributes), how it
//! reads the answers, the PeerCred retry and every way CUPS can say no.
mod support;

use axia_print::cups::{self, Doc};
use axia_print::ipp::{self, Builder, Msg, G_JOB, G_OPERATION, G_PRINTER};
use axia_print::Fail;
use serde_json::json;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use support::{serve_unix, temp_dir, Rep, Req};

fn start(name: &str, h: impl Fn(&Req) -> Rep + Send + Sync + 'static) -> String {
    let p: PathBuf = temp_dir(name).join("cups.sock");
    serve_unix(&p, h);
    p.to_str().unwrap().to_string()
}

/// A reply with a status and the standard operation group.
fn reply(status: u16, f: impl FnOnce(&mut Builder)) -> Rep {
    let mut b = Builder::new(status, 1);
    b.std(None, None);
    f(&mut b);
    b.end();
    Rep::ok(b.finish().unwrap()).with("Content-Type", "application/ipp")
}

fn request(r: &Req) -> Msg {
    ipp::parse(&r.body).expect("an IPP request")
}

fn s(m: &Msg, g: u8, n: &str) -> String {
    String::from_utf8_lossy(m.find(g, n).and_then(|a| a.str()).unwrap_or(b"<none>")).into_owned()
}

#[test]
fn printers_and_the_default_one() {
    let seen = Arc::new(Mutex::new(Vec::<(u16, String, String)>::new()));
    let seen2 = seen.clone();
    let sock = start("printers", move |r| {
        let m = request(r);
        seen2.lock().unwrap().push((m.code, r.path.clone(), s(&m, G_OPERATION, "requesting-user-name")));
        assert_eq!((r.method.as_str(), r.header("Content-Type")), ("POST", Some("application/ipp")));
        assert!(r.header("Authorization").is_none(), "no credentials until CUPS asks");
        match m.code {
            ipp::OP_CUPS_GET_DEFAULT => reply(0, |b| {
                b.group(G_PRINTER);
                b.add_str(ipp::T_NAME, Some("printer-name"), "Oficina");
            }),
            ipp::OP_CUPS_GET_PRINTERS => {
                let wanted = m.find(G_OPERATION, "requested-attributes").map_or(0, |a| a.vals.len());
                assert!(wanted >= 15, "it asks for what it shows");
                reply(0, |b| {
                    b.group(G_PRINTER);
                    b.add_str(ipp::T_NAME, Some("printer-name"), "PDF");
                    b.add_str(ipp::T_TEXT, Some("printer-info"), "Virtual PDF printer");
                    b.add_int(ipp::T_ENUM, Some("printer-state"), 3);
                    b.add_bool("printer-is-accepting-jobs", true);
                    b.add_bool("color-supported", true);
                    b.add_str(ipp::T_KEYWORD, Some("media-supported"), "iso_a4_210x297mm");
                    b.add_str(ipp::T_KEYWORD, None, "na_letter_8.5x11in");
                    b.add_str(ipp::T_KEYWORD, Some("printer-state-reasons"), "none");
                    b.group(G_PRINTER);
                    b.add_str(ipp::T_NAME, Some("printer-name"), "Oficina");
                    b.add_str(ipp::T_TEXT, Some("printer-make-and-model"), "HP LaserJet");
                    b.add_str(ipp::T_URI, Some("device-uri"), "ipp://192.168.1.20/ipp/print");
                    b.add_int(ipp::T_ENUM, Some("printer-state"), 5);
                    b.add_bool("printer-is-accepting-jobs", false);
                    b.add_int(ipp::T_INTEGER, Some("queued-job-count"), 4);
                    b.add_str(ipp::T_KEYWORD, Some("sides-supported"), "one-sided");
                    b.add_str(ipp::T_KEYWORD, Some("print-color-mode-default"), "monochrome");
                })
            }
            c => panic!("unexpected operation {c:#x}"),
        }
    });
    let j = cups::printers(&sock).unwrap();
    assert_eq!(j["default"], "Oficina");
    let p = j["printers"].as_array().unwrap();
    assert_eq!(p.len(), 2);
    assert_eq!(
        p[0],
        json!({ "name": "PDF", "info": "Virtual PDF printer", "location": "", "model": "", "message": "", "uri": "", "mediaDefault": "", "sidesDefault": "", "colorDefault": "",
                "state": 3, "accepting": true, "color": true, "queued": 0,
                "media": ["iso_a4_210x297mm", "na_letter_8.5x11in"], "sides": [], "colorModes": [], "reasons": ["none"] })
    );
    assert_eq!(
        p[1],
        json!({ "name": "Oficina", "info": "", "location": "", "model": "HP LaserJet", "message": "", "uri": "ipp://192.168.1.20/ipp/print", "mediaDefault": "", "sidesDefault": "",
                "colorDefault": "monochrome", "state": 5, "accepting": false, "color": false, "queued": 4,
                "media": [], "sides": ["one-sided"], "colorModes": [], "reasons": [] }),
        "each printer reads only its own group"
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|(_, path, u)| path == "/" && !u.is_empty() && *u != "<none>"));
}

#[test]
fn no_printers_and_no_jobs_are_empty_lists() {
    let sock = start("empty", |_| reply(0x0406, |b| {
        b.add_str(ipp::T_TEXT, Some("status-message"), "No destinations added.");
    }));
    assert_eq!(cups::printers(&sock).unwrap(), json!({ "printers": [], "default": "" }));
    assert_eq!(cups::jobs(&sock).unwrap(), json!({ "jobs": [] }));
}

#[test]
fn unfinished_jobs() {
    let sock = start("jobs", |r| {
        let m = request(r);
        assert_eq!(m.code, ipp::OP_GET_JOBS);
        assert_eq!(s(&m, G_OPERATION, "printer-uri"), "ipp://localhost/");
        assert_eq!(s(&m, G_OPERATION, "which-jobs"), "not-completed");
        reply(0, |b| {
            b.group(G_JOB);
            b.add_int(ipp::T_INTEGER, Some("job-id"), 17);
            b.add_str(ipp::T_NAME, Some("job-name"), "Informe");
            b.add_int(ipp::T_ENUM, Some("job-state"), 5);
            b.add_str(ipp::T_NAME, Some("job-originating-user-name"), "ana");
            b.add_str(ipp::T_URI, Some("job-printer-uri"), "ipp://localhost/printers/Oficina");
            b.add_int(ipp::T_INTEGER, Some("time-at-creation"), 1700000000);
            b.add_int(ipp::T_INTEGER, Some("job-k-octets"), 12);
            b.add_str(ipp::T_KEYWORD, Some("job-state-reasons"), "job-printing");
            b.group(G_JOB);
            b.add_int(ipp::T_INTEGER, Some("job-id"), 18);
        })
    });
    let j = cups::jobs(&sock).unwrap();
    assert_eq!(
        j["jobs"],
        json!([
            { "id": 17, "state": 5, "created": 1700000000, "kb": 12, "sheets": 0, "name": "Informe", "user": "ana", "printer": "Oficina", "reason": "job-printing" },
            { "id": 18, "state": 3, "created": 0, "kb": 0, "sheets": 0, "name": "", "user": "", "printer": "", "reason": "" }
        ])
    );
}

#[test]
fn cancel_and_the_administration_operations() {
    let log = Arc::new(Mutex::new(Vec::<(u16, String, Msg)>::new()));
    let log2 = log.clone();
    let sock = start("admin", move |r| {
        let m = request(r);
        let (code, path) = (m.code, r.path.clone());
        log2.lock().unwrap().push((code, path, m));
        reply(0, |_| {})
    });
    assert_eq!(cups::cancel(&sock, 17).unwrap(), json!({ "job": 17 }));
    assert_eq!(cups::add(&sock, Some("Oficina"), Some("ipp://192.168.1.20:631/ipp/print"), Some("Planta baja"), Some("")).unwrap(), json!({ "name": "Oficina" }));
    assert_eq!(cups::set_default(&sock, Some("Oficina")).unwrap(), json!({ "name": "Oficina" }));
    assert_eq!(cups::remove(&sock, Some("Oficina")).unwrap(), json!({ "name": "Oficina" }));
    let log = log.lock().unwrap();
    let (c, p, m) = &log[0];
    assert_eq!((*c, p.as_str(), s(m, G_OPERATION, "job-uri").as_str()), (ipp::OP_CANCEL_JOB, "/jobs", "ipp://localhost/jobs/17"));
    let (c, p, m) = &log[1];
    assert_eq!((*c, p.as_str()), (ipp::OP_CUPS_ADD_MODIFY_PRINTER, "/admin/"));
    assert_eq!(s(m, G_OPERATION, "printer-uri"), "ipp://localhost/printers/Oficina");
    assert_eq!(s(m, G_OPERATION, "ppd-name"), "everywhere", "driverless: CUPS asks the printer");
    assert_eq!((s(m, G_PRINTER, "device-uri").as_str(), s(m, G_PRINTER, "printer-info").as_str()), ("ipp://192.168.1.20:631/ipp/print", "Planta baja"));
    assert!(m.find(G_PRINTER, "printer-location").is_none(), "an empty text is not sent");
    assert_eq!(m.find(G_PRINTER, "printer-is-accepting-jobs").map(|a| a.int(0)), Some(1));
    assert_eq!(m.find(G_PRINTER, "printer-state").map(|a| a.int(0)), Some(3));
    assert_eq!((log[2].0, log[2].1.as_str()), (ipp::OP_CUPS_SET_DEFAULT, "/admin/"));
    assert_eq!((log[3].0, log[3].1.as_str()), (ipp::OP_CUPS_DELETE_PRINTER, "/admin/"));
}

#[test]
fn a_pdf_follows_the_message_and_the_options_become_attributes() {
    let doc: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
    let dir = temp_dir("doc");
    std::fs::write(dir.join("a.pdf"), &doc).unwrap();
    let got = Arc::new(Mutex::new(None::<(String, Msg, bool)>));
    let got2 = got.clone();
    let expected = doc.clone();
    let sock = start("submit", move |r| {
        let m = request(r);
        let same = r.body[m.used..] == expected[..];
        *got2.lock().unwrap() = Some((r.path.clone(), m, same));
        let mut rep = reply(0, |b| {
            b.group(G_JOB);
            b.add_int(ipp::T_INTEGER, Some("job-id"), 42);
        });
        rep.continue_first = true;
        rep
    });
    let fd: OwnedFd = std::fs::File::open(dir.join("a.pdf")).unwrap().into();
    let opts = json!({ "name": "Informe ñ", "copies": 2, "pages": "1-3, 5", "sides": "two-sided-long-edge", "color": "monochrome", "media": "iso_a4_210x297mm", "orientation": "landscape" });
    let j = cups::submit(&sock, Some("Oficina"), Some(&Doc { fd: &fd, len: doc.len() }), &opts).unwrap();
    assert_eq!(j, json!({ "job": 42 }));
    let (path, m, same) = got.lock().unwrap().take().unwrap();
    assert_eq!(path, "/printers/Oficina");
    assert!(same, "the document arrives whole, right after the message");
    assert_eq!(m.code, ipp::OP_PRINT_JOB);
    assert_eq!((s(&m, G_OPERATION, "job-name").as_str(), s(&m, G_OPERATION, "document-format").as_str()), ("Informe ñ", "application/pdf"));
    assert_eq!(s(&m, G_OPERATION, "printer-uri"), "ipp://localhost/printers/Oficina");
    assert_eq!(m.find(G_JOB, "copies").map(|a| a.int(0)), Some(2));
    let pr = m.find(G_JOB, "page-ranges").unwrap();
    assert_eq!(pr.vals.iter().map(|v| (v.i, v.i2)).collect::<Vec<_>>(), [(1, 3), (5, 5)]);
    assert_eq!((s(&m, G_JOB, "sides").as_str(), s(&m, G_JOB, "print-color-mode").as_str(), s(&m, G_JOB, "media").as_str()), ("two-sided-long-edge", "monochrome", "iso_a4_210x297mm"));
    assert_eq!(m.find(G_JOB, "orientation-requested").map(|a| a.int(0)), Some(4));
}

#[test]
fn options_that_are_not_allowed_are_dropped_and_defaults_apply() {
    let got = Arc::new(Mutex::new(None::<Msg>));
    let got2 = got.clone();
    let sock = start("defaults", move |r| {
        *got2.lock().unwrap() = Some(request(r));
        reply(0, |b| {
            b.group(G_JOB);
            b.add_int(ipp::T_INTEGER, Some("job-id"), 1);
        })
    });
    let opts = json!({ "name": "bad\nname", "copies": 5000, "sides": "diagonal", "color": "rainbow", "media": "a b", "orientation": "portrait", "pages": "" });
    cups::submit(&sock, Some("P"), None, &opts).unwrap();
    let m = got.lock().unwrap().take().unwrap();
    assert_eq!(s(&m, G_OPERATION, "job-name"), "Axia DE");
    assert_eq!(m.find(G_JOB, "copies").map(|a| a.int(0)), Some(1));
    for n in ["page-ranges", "sides", "print-color-mode", "media", "orientation-requested"] {
        assert!(m.find(G_JOB, n).is_none(), "{n} must not be sent");
    }
}

#[test]
fn cups_asks_for_credentials_once() {
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    let sock = start("peercred", move |r| {
        n2.fetch_add(1, Ordering::SeqCst);
        match r.header("Authorization") {
            None => Rep::status(401).with("WWW-Authenticate", "PeerCred"),
            Some(a) => {
                assert!(a.starts_with("PeerCred ") && a.len() > 9, "{a}");
                reply(0, |_| {})
            }
        }
    });
    assert_eq!(cups::remove(&sock, Some("P")).unwrap(), json!({ "name": "P" }));
    assert_eq!(n.load(Ordering::SeqCst), 2, "one try without, one with PeerCred");
    // a second 401 is a no
    let sock = start("peercred2", |_| Rep::status(401));
    assert_eq!(cups::remove(&sock, Some("P")).unwrap_err(), Fail::new("forbidden", "lpadmin"));
    let sock = start("forbidden", |_| Rep::status(403));
    assert_eq!(cups::remove(&sock, Some("P")).unwrap_err(), Fail::new("forbidden", "lpadmin"));
}

#[test]
fn every_way_cups_can_say_no() {
    let status = |code: u16, msg: &'static str| {
        let sock = start(&format!("status{code:x}"), move |_| reply(code, |b| {
            b.add_str(ipp::T_TEXT, Some("status-message"), msg);
        }));
        cups::remove(&sock, Some("P")).unwrap_err()
    };
    assert_eq!(status(0x0404, "not possible here"), Fail::new("client-error-not-possible", "not possible here"));
    assert_eq!(status(0x0403, "nope"), Fail::new("forbidden", "nope"));
    assert_eq!(status(0x0401, ""), Fail::new("forbidden", ""));
    assert_eq!(status(0x0406, "gone"), Fail::new("not-found", "gone"));
    assert_eq!(status(0x0507, "busy"), Fail::new("server-error-busy", "busy"));
    let sock = start("http500", |_| Rep::status(500));
    assert_eq!(cups::remove(&sock, Some("P")).unwrap_err(), Fail::new("http", "500"));
    let sock = start("garbage", |_| Rep::ok(b"this is not IPP at all".to_vec()));
    assert_eq!(cups::remove(&sock, Some("P")).unwrap_err(), Fail::new("bad-response", ""));
    let sock = start("hangup", |_| Rep { hang_up: true, ..Rep::default() });
    assert_eq!(cups::remove(&sock, Some("P")).unwrap_err(), Fail::new("no-cups", "http"));
    assert_eq!(cups::printers("/nonexistent").unwrap_err(), Fail::new("no-cups", "socket"));
    // a printer list that cannot be told apart from CUPS being gone is still an error
    let sock = start("jobs500", |_| Rep::status(500));
    assert_eq!(cups::jobs(&sock).unwrap_err(), Fail::new("http", "500"));
}
