//! Our own eSCL client («driverless» scanners, AirScan) over HTTP on the local network. No SANE.
//! Only LAN IPv4 addresses and HTTP (no TLS: `_uscans` is not supported).
//!
//! The scanner's XML is read with a bounded tag finder (no XML library): only the values that
//! matter are taken and the prefixes (`scan:`, `pwg:`) are ignored.
use crate::http::{self, Request, Response};
use crate::{jnum, jstr, lossy, strtol, Fail, Reply};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

const ESCL_MAX: usize = 256 << 20;

/// The path from the TXT «rs» (normally "eSCL").
pub fn valid_rs(rs: &str) -> bool {
    rs.len() <= 64 && !rs.contains("..") && rs.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'/' | b'_' | b'-' | b'.'))
}

/// The text of the next `<prefix:tag>` or `<tag>` element from `*pos`, up to `cap - 1` bytes.
fn next_tag(xml: &[u8], pos: &mut usize, tag: &str, cap: usize) -> Option<Vec<u8>> {
    let (tag, len) = (tag.as_bytes(), xml.len());
    let tl = tag.len();
    let mut i = *pos;
    while i + tl + 2 < len {
        let at = i;
        i += 1;
        if xml[at] != b'<' || xml[at + 1] == b'/' {
            continue;
        }
        let mut j = at + 1;
        while j < len && j < at + 40 && !matches!(xml[j], b':' | b'>' | b' ') {
            j += 1;
        }
        let name = if j < len && xml[j] == b':' { j + 1 } else { at + 1 };
        if name + tl >= len || &xml[name..name + tl] != tag || !matches!(xml[name + tl], b'>' | b' ') {
            continue;
        }
        let mut v = name + tl;
        while v < len && xml[v] != b'>' {
            v += 1;
        }
        v += 1;
        let mut out = Vec::new();
        while v < len && xml[v] != b'<' && out.len() + 1 < cap {
            out.push(xml[v]);
            v += 1;
        }
        *pos = v;
        return Some(out);
    }
    None
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn get(ip: &str, port: i32, path: &str, timeout_ms: u64) -> Result<Response, http::HttpError> {
    let fd = http::tcp(ip, port, 4000).map_err(|_| http::HttpError)?;
    http::request(
        &fd,
        &Request { method: "GET", host: &format!("{ip}:{port}"), path, headers: None, content_type: None, body: &[], doc: None, max: ESCL_MAX, timeout_ms },
    )
}

/// What the scanner can do: {model, platen, adf, maxW, maxH, colors, formats, formats2, res}.
pub fn caps(ip: &str, port: i32, rs: &str) -> Reply {
    if !http::lan_ipv4(ip) || !valid_rs(rs) {
        return Err(Fail::new("bad-request", "host"));
    }
    let r = get(ip, port, &format!("/{rs}/ScannerCapabilities"), 8000).map_err(|_| Fail::new("unreachable", ""))?;
    if r.status != 200 || r.body.is_empty() {
        return Err(Fail::new("http", &r.status.to_string()));
    }
    let x = r.body.as_slice();
    let mut pos = 0;
    let model = next_tag(x, &mut pos, "MakeAndModel", 256).unwrap_or_default();
    // what's on the glass (Platen) and the feeder (Adf)
    let platen = contains(x, "Platen>") || contains(x, "PlatenInputCaps");
    let adf = contains(x, "AdfSimplexInputCaps") || contains(x, "Adf>");
    let size = |tag: &str, def: i64, hi: i64| {
        let mut pos = 0;
        let v = next_tag(x, &mut pos, tag, 256).map_or(def, |v| strtol(&v));
        if (100..=hi).contains(&v) { v } else { def }
    };
    let (max_w, max_h) = (size("MaxWidth", 2550, 20000), size("MaxHeight", 3508, 40000));
    // lists without duplicates
    let list = |tag: &str, number: bool| -> Vec<Value> {
        let mut seen: Vec<Vec<u8>> = Vec::new();
        let mut pos = 0;
        while seen.len() < 32 {
            let Some(v) = next_tag(x, &mut pos, tag, 256) else { break };
            if seen.contains(&v) || v.is_empty() || v.len() >= 64 {
                continue;
            }
            seen.push(v);
        }
        seen.iter().map(|v| if number { json!(strtol(v)) } else { json!(lossy(v)) }).collect()
    };
    Ok(json!({
        "model": lossy(&model),
        "platen": platen,
        "adf": adf,
        "maxW": max_w,
        "maxH": max_h,
        "colors": list("ColorMode", false),
        "formats": list("DocumentFormatExt", false),
        "formats2": list("DocumentFormat", false),
        "res": list("XResolution", true),
    }))
}

fn ext_of(mime: &str) -> Option<&'static str> {
    if mime.starts_with("image/png") {
        Some("png")
    } else if mime.starts_with("application/pdf") {
        Some("pdf")
    } else if mime.starts_with("image/jpeg") {
        Some("jpg")
    } else {
        None
    }
}

/// Scans with `s` = {source, color, res, format, x, y, w, h (in 1/300 inch)} into a new file in
/// `dir`; the reply is {file, format, bytes}.
pub fn scan(ip: &str, port: i32, rs: &str, s: &Value, dir: &Path) -> Reply {
    if !http::lan_ipv4(ip) || !valid_rs(rs) {
        return Err(Fail::new("bad-request", "host"));
    }
    let color = jstr(s, "color").filter(|c| ["RGB24", "Grayscale8", "BlackAndWhite1"].contains(c)).unwrap_or("RGB24");
    let format = jstr(s, "format").filter(|f| ext_of(f).is_some()).unwrap_or("image/jpeg");
    let source = jstr(s, "source").filter(|c| ["Platen", "Feeder"].contains(c)).unwrap_or("Platen");
    let res = jnum(s, "res", 300.0) as i32;
    let res = if (50..=2400).contains(&res) { res } else { 300 };
    let (x, y, w, h) = (jnum(s, "x", 0.0) as i64, jnum(s, "y", 0.0) as i64, jnum(s, "w", 2550.0) as i64, jnum(s, "h", 3508.0) as i64);
    if x < 0 || y < 0 || w < 30 || h < 30 || w > 40000 || h > 40000 {
        return Err(Fail::new("bad-request", "area"));
    }
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <scan:ScanSettings xmlns:scan=\"http://schemas.hp.com/imaging/escl/2011/05/03\" xmlns:pwg=\"http://www.pwg.org/schemas/2010/12/sm\">\n\
         <pwg:Version>2.6</pwg:Version>\n\
         <pwg:ScanRegions><pwg:ScanRegion><pwg:Height>{h}</pwg:Height><pwg:ContentRegionUnits>escl:ThreeHundredthsOfInches</pwg:ContentRegionUnits>\
         <pwg:Width>{w}</pwg:Width><pwg:XOffset>{x}</pwg:XOffset><pwg:YOffset>{y}</pwg:YOffset></pwg:ScanRegion></pwg:ScanRegions>\n\
         <pwg:InputSource>{source}</pwg:InputSource>\n<scan:ColorMode>{color}</scan:ColorMode>\n\
         <scan:XResolution>{res}</scan:XResolution>\n<scan:YResolution>{res}</scan:YResolution>\n\
         <pwg:DocumentFormat>{format}</pwg:DocumentFormat>\n<scan:DocumentFormatExt>{format}</scan:DocumentFormatExt>\n\
         </scan:ScanSettings>\n"
    );
    let unreachable = || Fail::new("unreachable", "");
    let fd = http::tcp(ip, port, 4000).map_err(|_| unreachable())?;
    let r = http::request(
        &fd,
        &Request {
            method: "POST",
            host: &format!("{ip}:{port}"),
            path: &format!("/{rs}/ScanJobs"),
            headers: None,
            content_type: Some("text/xml"),
            body: body.as_bytes(),
            doc: None,
            max: 1 << 16,
            timeout_ms: 15000,
        },
    )
    .map_err(|_| unreachable())?;
    drop(fd);
    if r.status == 503 {
        return Err(Fail::new("busy", ""));
    }
    if r.status != 201 || r.location.is_empty() {
        return Err(Fail::new("http", &r.status.to_string()));
    }
    // Location: http://ip:port/eSCL/ScanJobs/uuid or just the path; the path is used, always with
    // the same address (it doesn't follow to another machine)
    let mut loc = r.location.as_str();
    if loc.starts_with("http://") {
        loc = loc[7..].find('/').map_or("/", |i| &loc[7 + i..]);
    }
    let job: String = loc.chars().take_while(|&c| c as u32 > 0x20).collect();
    if !job.starts_with('/') || job.contains("..") {
        return Err(Fail::new("bad-response", "location"));
    }
    let next = format!("{job}{}NextDocument", if job.ends_with('/') { "" } else { "/" });
    let mut doc = None;
    for _ in 0..40 {
        let d = get(ip, port, &next, 120000).map_err(|_| unreachable())?;
        if d.status == 200 && !d.body.is_empty() {
            doc = Some(d);
            break;
        }
        if d.status != 503 {
            return Err(Fail::new("http", &d.status.to_string()));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let Some(d) = doc else { return Err(Fail::new("timeout", "")) };
    let ext = ext_of(&d.content_type).or_else(|| ext_of(format)).unwrap_or("jpg");
    // a new file in the scanner's cache folder
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_secs());
    let mut last = None;
    let mut created = None;
    for i in 0..100 {
        let file = dir.join(format!("scan-{now}-{i}.{ext}"));
        match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(&file) {
            Ok(f) => {
                created = Some((f, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                last = Some(e);
                break;
            }
        }
    }
    let Some((mut f, file)) = created else {
        return Err(Fail::new("write", &last.map(|e| crate::lossy(strerror(&e).as_bytes())).unwrap_or_default()));
    };
    let complete = f.write_all(&d.body).is_ok();
    drop(f);
    let out = json!({ "file": file.to_string_lossy(), "format": ext, "bytes": d.body.len() });
    if complete { Ok(out) } else { Err(Fail(out)) }
}

/// The text of an OS error as strerror gives it ("No space left on device").
fn strerror(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(n) => {
            // SAFETY: strerror returns a NUL-terminated string valid until the next call.
            unsafe { std::ffi::CStr::from_ptr(libc::strerror(n)) }.to_string_lossy().into_owned()
        }
        None => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resource_path_is_a_plain_path() {
        for ok in ["eSCL", "a/b_c-d.e", ""] {
            assert!(valid_rs(ok), "{ok:?}");
        }
        for bad in ["../x", "a..b", "a b", "a?b", "a%2e", &"a".repeat(65)] {
            assert!(!valid_rs(bad), "{bad:?}");
        }
    }

    #[test]
    fn tags_are_found_with_or_without_a_prefix() {
        let x = b"<scan:ScannerCapabilities><pwg:Version>2.6</pwg:Version><scan:Make Foo='1'>HP &amp; co</scan:Make><Make>second</Make></scan:ScannerCapabilities>";
        let mut pos = 0;
        assert_eq!(next_tag(x, &mut pos, "Version", 256).as_deref(), Some(&b"2.6"[..]));
        assert_eq!(next_tag(x, &mut pos, "Make", 256).as_deref(), Some(&b"HP &amp; co"[..]));
        assert_eq!(next_tag(x, &mut pos, "Make", 256).as_deref(), Some(&b"second"[..]));
        assert_eq!(next_tag(x, &mut pos, "Make", 256), None);
        let mut pos = 0;
        assert_eq!(next_tag(x, &mut pos, "Make", 4).as_deref(), Some(&b"HP "[..]), "cut to the capacity");
        let mut pos = 0;
        assert_eq!(next_tag(x, &mut pos, "Mak", 256), None, "a prefix of the name is not the name");
        let mut pos = 0;
        assert_eq!(next_tag(b"<Make", &mut pos, "Make", 256), None, "cut in the middle of a tag");
        assert_eq!(next_tag(b"", &mut pos, "Make", 256), None);
    }

    #[test]
    fn what_a_bad_request_never_sends() {
        let w = Value::Null;
        let d = Path::new("/nonexistent");
        assert_eq!(caps("8.8.8.8", 80, "eSCL").unwrap_err(), Fail::new("bad-request", "host"));
        assert_eq!(caps("127.0.0.1", 80, "../x").unwrap_err(), Fail::new("bad-request", "host"));
        assert_eq!(scan("1.1.1.1", 80, "eSCL", &w, d).unwrap_err(), Fail::new("bad-request", "host"));
        assert_eq!(scan("127.0.0.1", 80, "eSCL", &json!({ "w": 10 }), d).unwrap_err(), Fail::new("bad-request", "area"));
        assert_eq!(scan("127.0.0.1", 80, "eSCL", &json!({ "x": -1 }), d).unwrap_err(), Fail::new("bad-request", "area"));
        assert_eq!(scan("127.0.0.1", 80, "eSCL", &json!({ "h": 40001 }), d).unwrap_err(), Fail::new("bad-request", "area"));
        assert_eq!(ext_of("image/jpeg; q=1"), Some("jpg"));
        assert_eq!(ext_of("text/html"), None);
    }
}
