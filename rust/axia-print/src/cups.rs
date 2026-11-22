//! Our own IPP client for CUPS over its local socket (`/run/cups/cups.sock`), without libcups.
//!
//! CUPS manages the queue and the drivers; LordOS only speaks IPP to it. The administration
//! operations (add, remove, choose the default) are authorized by CUPS's own policy: the user has
//! to be in the lpadmin group (LordOS administrators are) and identifies with «PeerCred» (CUPS
//! looks at the socket credentials). There is no root action for this. See docs/PRINTING.md.
//!
//! Every operation blocks, so they run in the threads of `tasks`. A reply is the JSON for JS, or
//! the failure `{"error": ..., "detail": ...}`.
use crate::http::{self, Request};
use crate::ipp::{self, Builder, Msg};
use crate::{jnum, jstr, lossy, Fail, Reply};
use serde_json::{json, Value};
use std::os::fd::{AsFd, OwnedFd};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

pub const DEFAULT_SOCKET: &str = "/run/cups/cups.sock";

static SOCKET: Mutex<Option<String>> = Mutex::new(None);
static REQID: AtomicU32 = AtomicU32::new(1);

/// The socket CUPS listens on.
pub fn socket_path() -> String {
    SOCKET.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| DEFAULT_SOCKET.to_string())
}

/// Points the client at another socket (the tests); `None` goes back to CUPS's.
pub fn set_socket_path(path: Option<&str>) {
    *SOCKET.lock().unwrap_or_else(|e| e.into_inner()) = path.map(str::to_string);
}

fn user() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        let mut buf = [0 as libc::c_char; 1024];
        // SAFETY: getpwuid_r fills `pw` and `buf`, which outlive the call and the copy of the name.
        unsafe {
            let mut pw: libc::passwd = std::mem::zeroed();
            let mut res: *mut libc::passwd = std::ptr::null_mut();
            if libc::getpwuid_r(libc::getuid(), &mut pw, buf.as_mut_ptr(), buf.len(), &mut res) == 0 && !res.is_null() {
                let n = std::ffi::CStr::from_ptr(pw.pw_name).to_string_lossy().into_owned();
                return n.chars().take(63).collect();
            }
        }
        "lord".to_string()
    })
}

fn id() -> u32 {
    REQID.fetch_add(1, Ordering::Relaxed)
}

// ---- validations (js_print.c uses them too) ----

/// Letters, digits, `_` and `-`, up to 127.
pub fn valid_name(s: &[u8]) -> bool {
    !s.is_empty() && s.len() <= 127 && s.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// ipp:// ipps:// http:// https:// dnssd:// without spaces or quotes.
pub fn valid_uri(s: &[u8]) -> bool {
    let n = s.len();
    if !(8..=1000).contains(&n) {
        return false;
    }
    if !["ipp://", "ipps://", "http://", "https://", "dnssd://"].iter().any(|p| s.starts_with(p.as_bytes())) {
        return false;
    }
    s.iter().all(|&c| c > 0x20 && c != 0x7f && c != b'"' && c != b'\\')
}

/// Free text up to `max` bytes with no control characters (a missing text is fine).
pub fn valid_text(s: Option<&[u8]>, max: usize) -> bool {
    s.is_none_or(|s| s.len() <= max && s.iter().all(|&c| c >= 0x20))
}

/// "1-3,5,8-9" → ranges (at most `max`); `None` if it is not valid.
pub fn page_ranges(s: &str, max: usize) -> Option<Vec<(i32, i32)>> {
    let b = s.as_bytes();
    let (mut i, mut out) = (0, Vec::new());
    let skip = |i: &mut usize, set: &[u8]| {
        while *i < b.len() && set.contains(&b[*i]) {
            *i += 1;
        }
    };
    let number = |i: &mut usize| -> Option<i64> {
        if *i >= b.len() || !b[*i].is_ascii_digit() {
            return None;
        }
        let mut v: i64 = 0;
        while *i < b.len() && b[*i].is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add((b[*i] - b'0') as i64);
            *i += 1;
        }
        Some(v)
    };
    while i < b.len() {
        skip(&mut i, b" ,");
        if i >= b.len() {
            break;
        }
        if out.len() >= max {
            return None;
        }
        let a = number(&mut i)?;
        let mut e = a;
        skip(&mut i, b" ");
        if i < b.len() && b[i] == b'-' {
            i += 1;
            skip(&mut i, b" ");
            e = number(&mut i)?;
        }
        if a < 1 || e < a || e > 100000 {
            return None;
        }
        out.push((a as i32, e as i32));
        skip(&mut i, b" ");
        if i < b.len() && b[i] != b',' {
            return None;
        }
    }
    Some(out)
}

// ---- the exchange ----

/// A document sent after the request's IPP message.
pub struct Doc<'a> {
    pub fd: &'a OwnedFd,
    pub len: usize,
}

/// Sends an IPP request to CUPS and decodes the reply. On 401 it retries once with PeerCred.
fn exchange(socket: &str, path: &str, req: Builder, doc: Option<&Doc>, timeout_ms: u64) -> Result<Msg, Fail> {
    let Some(req) = req.finish() else { return Err(Fail::new("encode", "")) };
    for attempt in 0..2 {
        let fd = http::unix(socket).map_err(|_| Fail::new("no-cups", "socket"))?;
        let auth = (attempt > 0).then(|| format!("Authorization: PeerCred {}\r\n", user()));
        let r = http::request(
            &fd,
            &Request {
                method: "POST",
                host: "localhost",
                path,
                headers: auth.as_deref(),
                content_type: Some("application/ipp"),
                body: &req,
                doc: doc.map(|d| (d.fd.as_fd(), d.len)),
                max: 8 << 20,
                timeout_ms,
            },
        )
        .map_err(|_| Fail::new("no-cups", "http"))?;
        drop(fd);
        if r.status == 401 && attempt == 0 {
            continue;
        }
        if r.status == 401 || r.status == 403 {
            return Err(Fail::new("forbidden", "lpadmin"));
        }
        if r.status != 200 {
            return Err(Fail::new("http", &r.status.to_string()));
        }
        let m = ipp::parse(&r.body).ok_or_else(|| Fail::new("bad-response", ""))?;
        if m.code >= 0x0400 {
            let code = match m.code {
                0x0401..=0x0403 => "forbidden",
                0x0406 => "not-found",
                c => ipp::status_name(c),
            };
            let detail = m.find(ipp::G_OPERATION, "status-message").and_then(|a| a.str()).map(lossy).unwrap_or_default();
            return Err(Fail::new(code, &detail));
        }
        return Ok(m);
    }
    Err(Fail::new("no-cups", "http"))
}

fn texts(a: Option<&ipp::Attr>) -> Vec<String> {
    a.map(|a| a.vals.iter().take(200).map(|v| lossy(v.s.as_deref().unwrap_or(b""))).collect()).unwrap_or_default()
}

fn text(m: &Msg, gseq: i32, name: &str) -> String {
    m.find_in(gseq, name).and_then(|a| a.str()).map(lossy).unwrap_or_default()
}

fn int(m: &Msg, gseq: i32, name: &str, def: i32) -> i32 {
    m.find_in(gseq, name).map_or(def, |a| a.int(def))
}

fn requested(b: &mut Builder, names: &[&str]) {
    for (i, n) in names.iter().enumerate() {
        b.add_str(ipp::T_KEYWORD, (i == 0).then_some("requested-attributes"), n);
    }
}

/// The printers CUPS has, and the default one.
pub fn printers(socket: &str) -> Reply {
    const ATTRS: &[&str] = &[
        "printer-name", "printer-info", "printer-location", "printer-make-and-model", "printer-state", "printer-state-message",
        "printer-state-reasons", "printer-is-accepting-jobs", "device-uri", "printer-uri-supported", "media-supported",
        "media-default", "sides-supported", "sides-default", "print-color-mode-supported", "print-color-mode-default",
        "color-supported", "printer-type", "queued-job-count",
    ];
    // the default one
    let mut def = String::new();
    let mut q = Builder::new(ipp::OP_CUPS_GET_DEFAULT, id());
    q.std(None, Some(user()));
    q.add_str(ipp::T_KEYWORD, Some("requested-attributes"), "printer-name");
    q.end();
    match exchange(socket, "/", q, None, 8000) {
        Ok(m) => {
            if let Some(n) = m.find(ipp::G_PRINTER, "printer-name").and_then(|a| a.str()) {
                def = lossy(&n[..n.len().min(127)]);
            }
        }
        Err(e) if e.code() == "no-cups" => return Err(e),
        Err(_) => {}
    }
    let mut q = Builder::new(ipp::OP_CUPS_GET_PRINTERS, id());
    q.std(None, Some(user()));
    requested(&mut q, ATTRS);
    q.end();
    let m = match exchange(socket, "/", q, None, 10000) {
        Ok(m) => m,
        // with no printers CUPS answers not-found: an empty list
        Err(e) if e.code() == "not-found" => return Ok(json!({ "printers": [], "default": "" })),
        Err(e) => return Err(e),
    };
    let mut list = Vec::new();
    for a in &m.attrs {
        if a.group != ipp::G_PRINTER || !a.is("printer-name") {
            continue;
        }
        let g = a.gseq; // this printer's group
        list.push(json!({
            "name": lossy(a.str().unwrap_or(b"")),
            "info": text(&m, g, "printer-info"),
            "location": text(&m, g, "printer-location"),
            "model": text(&m, g, "printer-make-and-model"),
            "message": text(&m, g, "printer-state-message"),
            "uri": text(&m, g, "device-uri"),
            "mediaDefault": text(&m, g, "media-default"),
            "sidesDefault": text(&m, g, "sides-default"),
            "colorDefault": text(&m, g, "print-color-mode-default"),
            "state": int(&m, g, "printer-state", 3),
            "accepting": int(&m, g, "printer-is-accepting-jobs", 1) != 0,
            "color": int(&m, g, "color-supported", 0) != 0,
            "queued": int(&m, g, "queued-job-count", 0),
            "media": texts(m.find_in(g, "media-supported")),
            "sides": texts(m.find_in(g, "sides-supported")),
            "colorModes": texts(m.find_in(g, "print-color-mode-supported")),
            "reasons": texts(m.find_in(g, "printer-state-reasons")),
        }));
    }
    Ok(json!({ "default": def, "printers": list }))
}

/// The jobs that are not completed, from every user.
pub fn jobs(socket: &str) -> Reply {
    let mut q = Builder::new(ipp::OP_GET_JOBS, id());
    q.std(Some("ipp://localhost/"), Some(user()));
    q.add_str(ipp::T_KEYWORD, Some("which-jobs"), "not-completed");
    requested(
        &mut q,
        &[
            "job-id", "job-name", "job-state", "job-originating-user-name", "job-printer-uri", "time-at-creation", "job-k-octets",
            "job-media-sheets-completed", "job-state-reasons",
        ],
    );
    q.end();
    let m = match exchange(socket, "/", q, None, 10000) {
        Ok(m) => m,
        Err(e) if e.code() == "not-found" => return Ok(json!({ "jobs": [] })),
        Err(e) => return Err(e),
    };
    let mut list = Vec::new();
    for a in &m.attrs {
        if a.group != ipp::G_JOB || !a.is("job-id") {
            continue;
        }
        let g = a.gseq;
        let puri = text(&m, g, "job-printer-uri");
        let printer = puri.rfind('/').map_or("", |i| &puri[i + 1..]).to_string();
        list.push(json!({
            "id": a.int(0),
            "state": int(&m, g, "job-state", 3),
            "created": int(&m, g, "time-at-creation", 0),
            "kb": int(&m, g, "job-k-octets", 0),
            "sheets": int(&m, g, "job-media-sheets-completed", 0),
            "name": text(&m, g, "job-name"),
            "user": text(&m, g, "job-originating-user-name"),
            "printer": printer,
            "reason": text(&m, g, "job-state-reasons"),
        }));
    }
    Ok(json!({ "jobs": list }))
}

pub fn cancel(socket: &str, job: i32) -> Reply {
    if job <= 0 {
        return Err(Fail::new("bad-request", "job"));
    }
    let mut q = Builder::new(ipp::OP_CANCEL_JOB, id());
    q.group(ipp::G_OPERATION);
    q.add_str(ipp::T_CHARSET, Some("attributes-charset"), "utf-8");
    q.add_str(ipp::T_LANGUAGE, Some("attributes-natural-language"), "en");
    q.add_str(ipp::T_URI, Some("job-uri"), &format!("ipp://localhost/jobs/{job}"));
    q.add_str(ipp::T_NAME, Some("requesting-user-name"), user());
    q.end();
    exchange(socket, "/jobs", q, None, 10000)?;
    Ok(json!({ "job": job }))
}

/// Prints the PDF in `doc` on `printer`. `opts`: {name, copies, pages:"1-3,5", sides, color, media, orientation}.
pub fn submit(socket: &str, printer: Option<&str>, doc: Option<&Doc>, opts: &Value) -> Reply {
    let Some(printer) = printer.filter(|p| valid_name(p.as_bytes())) else { return Err(Fail::new("bad-request", "printer")) };
    let mut q = Builder::new(ipp::OP_PRINT_JOB, id());
    q.std(Some(&format!("ipp://localhost/printers/{printer}")), Some(user()));
    let name = jstr(opts, "name").filter(|n| valid_text(Some(n.as_bytes()), 200) && !n.is_empty()).unwrap_or("Axia DE");
    q.add_str(ipp::T_NAME, Some("job-name"), name);
    q.add_str(ipp::T_MIMETYPE, Some("document-format"), "application/pdf");
    q.group(ipp::G_JOB);
    let copies = jnum(opts, "copies", 1.0) as i32;
    q.add_int(ipp::T_INTEGER, Some("copies"), if (1..=999).contains(&copies) { copies } else { 1 });
    if let Some(pages) = jstr(opts, "pages").filter(|p| !p.is_empty()) {
        let Some(ranges) = page_ranges(pages, 32) else { return Err(Fail::new("bad-pages", pages)) };
        for (i, (lo, hi)) in ranges.into_iter().enumerate() {
            q.add_range((i == 0).then_some("page-ranges"), lo, hi);
        }
    }
    if let Some(s) = jstr(opts, "sides").filter(|s| ["one-sided", "two-sided-long-edge", "two-sided-short-edge"].contains(s)) {
        q.add_str(ipp::T_KEYWORD, Some("sides"), s);
    }
    if let Some(c) = jstr(opts, "color").filter(|c| ["color", "monochrome", "auto"].contains(c)) {
        q.add_str(ipp::T_KEYWORD, Some("print-color-mode"), c);
    }
    if let Some(m) = jstr(opts, "media").filter(|m| !m.is_empty() && m.len() < 64 && m.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))) {
        q.add_str(ipp::T_KEYWORD, Some("media"), m);
    }
    if jstr(opts, "orientation") == Some("landscape") {
        q.add_int(ipp::T_ENUM, Some("orientation-requested"), 4);
    }
    q.end();
    let m = exchange(socket, &format!("/printers/{printer}"), q, doc, 60000)?;
    let job = m.find(ipp::G_JOB, "job-id").map_or(0, |a| a.int(0));
    Ok(json!({ "job": job }))
}

fn admin_op(socket: &str, op: u16, name: Option<&str>, more: impl FnOnce(&mut Builder), timeout_ms: u64) -> Reply {
    let Some(name) = name.filter(|n| valid_name(n.as_bytes())) else { return Err(Fail::new("bad-request", "name")) };
    let mut q = Builder::new(op, id());
    q.std(Some(&format!("ipp://localhost/printers/{name}")), Some(user()));
    more(&mut q);
    q.end();
    exchange(socket, "/admin/", q, None, timeout_ms)?;
    Ok(json!({ "name": name }))
}

/// Adds a driverless (IPP Everywhere) printer.
pub fn add(socket: &str, name: Option<&str>, uri: Option<&str>, info: Option<&str>, location: Option<&str>) -> Reply {
    let uri = match uri.filter(|u| valid_uri(u.as_bytes())) {
        Some(u) if valid_text(info.map(str::as_bytes), 127) && valid_text(location.map(str::as_bytes), 127) => u,
        _ => return Err(Fail::new("bad-request", "uri")),
    };
    admin_op(
        socket,
        ipp::OP_CUPS_ADD_MODIFY_PRINTER,
        name,
        |q| {
            // the "everywhere" model: CUPS builds the PPD by asking the printer
            q.add_str(ipp::T_NAME, Some("ppd-name"), "everywhere");
            q.group(ipp::G_PRINTER);
            q.add_str(ipp::T_URI, Some("device-uri"), uri);
            if let Some(i) = info.filter(|i| !i.is_empty()) {
                q.add_str(ipp::T_TEXT, Some("printer-info"), i);
            }
            if let Some(l) = location.filter(|l| !l.is_empty()) {
                q.add_str(ipp::T_TEXT, Some("printer-location"), l);
            }
            q.add_bool("printer-is-accepting-jobs", true);
            q.add_int(ipp::T_ENUM, Some("printer-state"), 3);
        },
        120000,
    )
}

pub fn remove(socket: &str, name: Option<&str>) -> Reply {
    admin_op(socket, ipp::OP_CUPS_DELETE_PRINTER, name, |_| {}, 20000)
}

pub fn set_default(socket: &str, name: Option<&str>) -> Reply {
    admin_op(socket, ipp::OP_CUPS_SET_DEFAULT, name, |_| {}, 20000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges_are_numbers_and_ranges() {
        assert_eq!(page_ranges("1-3,5, 8 - 9", 32), Some(vec![(1, 3), (5, 5), (8, 9)]));
        assert_eq!(page_ranges("", 32), Some(vec![]));
        assert_eq!(page_ranges(" , 2 ,, 4", 32), Some(vec![(2, 2), (4, 4)]));
        for bad in ["3-1", "0", "1;2", "a", "1-", "1 2", "100001", "1-100001", "-3", "99999999999999999999"] {
            assert_eq!(page_ranges(bad, 32), None, "{bad:?}");
        }
        assert_eq!(page_ranges("1,2,3", 3).map(|v| v.len()), Some(3));
        assert_eq!(page_ranges("1,2,3,4", 3), None, "more than the limit");
        assert_eq!(page_ranges("100000", 1), Some(vec![(100000, 100000)]));
    }

    #[test]
    fn names_uris_and_texts() {
        assert!(valid_name(b"Oficina_2-B") && !valid_name(b"a b") && !valid_name(b"../x") && !valid_name(b""));
        assert!(valid_name("a".repeat(127).as_bytes()) && !valid_name("a".repeat(128).as_bytes()));
        assert!(valid_uri(b"ipp://192.168.1.20:631/ipp/print") && valid_uri(b"dnssd://x._ipp._tcp.local/") && valid_uri(b"https://a.b/c"));
        for bad in ["file:///etc/passwd", "ipp://a b", "cups-pdf:/", "ipp://", "ipp://a\"b", "ipp://a\\b", "ipp://a\u{7f}b", "ipp://a\nb"] {
            assert!(!valid_uri(bad.as_bytes()), "{bad:?}");
        }
        assert!(!valid_uri(format!("ipp://{}", "a".repeat(1000)).as_bytes()));
        assert!(valid_text(None, 5) && valid_text(Some(b"abcde"), 5) && !valid_text(Some(b"abcdef"), 5) && !valid_text(Some(b"a\nb"), 5));
        assert!(valid_text(Some("ñ".as_bytes()), 5));
    }

    #[test]
    fn requests_that_are_wrong_never_reach_cups() {
        // the socket does not exist: anything that got that far would say "no-cups"
        let s = "/nonexistent/cups.sock";
        let bad = |r: Reply| r.unwrap_err();
        assert_eq!(bad(cancel(s, 0)), Fail::new("bad-request", "job"));
        assert_eq!(bad(submit(s, Some("../x"), None, &Value::Null)), Fail::new("bad-request", "printer"));
        assert_eq!(bad(submit(s, None, None, &Value::Null)), Fail::new("bad-request", "printer"));
        assert_eq!(bad(submit(s, Some("P"), None, &json!({ "pages": "3-1" }))), Fail::new("bad-pages", "3-1"));
        assert_eq!(bad(add(s, Some("P"), Some("file:///x"), None, None)), Fail::new("bad-request", "uri"));
        assert_eq!(bad(add(s, Some("P"), None, None, None)), Fail::new("bad-request", "uri"));
        assert_eq!(bad(add(s, Some("P"), Some("ipp://1.2.3.4/x"), Some("a\nb"), None)), Fail::new("bad-request", "uri"));
        assert_eq!(bad(add(s, Some("a b"), Some("ipp://1.2.3.4/x"), None, None)), Fail::new("bad-request", "name"));
        assert_eq!(bad(remove(s, None)), Fail::new("bad-request", "name"));
        assert_eq!(bad(set_default(s, Some(""))), Fail::new("bad-request", "name"));
        assert_eq!(bad(remove(s, Some("P"))), Fail::new("no-cups", "socket"));
        assert_eq!(bad(printers(s)), Fail::new("no-cups", "socket"));
        assert_eq!(bad(jobs(s)), Fail::new("no-cups", "socket"));
    }
}
