//! The firewall's file (~/.config/axia/firewall.json): `fw_to_json`, `fw_from_json`, `fw_save` and `fw_load`
//! of server/src/fw/rules.h, on top of the rule store and decider (docs/FIREWALL.md).
use super::*;
use crate::json::{self, Json};
use std::ffi::CString;

const FW_FILE_MAX: usize = 4 << 20;

const MODE_NAMES: [&str; 4] = ["silent", "ask", "off", "deny"];

fn flag_name(bit: usize) -> &'static str {
    FLAG_NAMES[bit].to_str().unwrap_or("")
}

fn flag_from_name(n: &[u8]) -> u32 {
    FLAG_NAMES.iter().position(|f| f.to_bytes() == n).map_or(0, |i| 1 << i)
}

/// Compact JSON of the store (only the "forever" rules).
pub(super) fn to_json(s: &FwStore) -> Option<Vec<u8>> {
    let mut apps: Vec<&App> = s.apps.values().collect();
    apps.sort_by(|a, b| a.id.cmp(&b.id));
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(format!("{{\"v\":1,\"mode\":\"{}\",\"trusted\":[", MODE_NAMES[s.mode as usize]).as_bytes());
    let mut first = true;
    for a in apps.iter().filter(|a| a.trusted) {
        if !first {
            b.push(b',');
        }
        first = false;
        json::quote(&a.id, &mut b);
    }
    b.extend_from_slice(b"],\"restricted\":{");
    first = true;
    for a in apps.iter().filter(|a| a.restricted != 0) {
        if !first {
            b.push(b',');
        }
        first = false;
        json::quote(&a.id, &mut b);
        b.extend_from_slice(b":[");
        let mut f1 = true;
        for k in 0..FLAG_NAMES.len() {
            if a.restricted & (1 << k) != 0 {
                b.extend_from_slice(format!("{}\"{}\"", if f1 { "" } else { "," }, flag_name(k)).as_bytes());
                f1 = false;
            }
        }
        b.push(b']');
    }
    b.extend_from_slice(b"},\"rules\":[");
    first = true;
    for id in s.sorted(None) {
        let r = &s.ids[&id];
        if r.life != FW_LIFE_FOREVER {
            continue; // the others don't survive the session
        }
        let dst = format_dest(r)?;
        b.extend_from_slice(if first { b"{\"app\":" } else { b",{\"app\":" });
        first = false;
        json::quote(cs(&r.app), &mut b);
        b.extend_from_slice(format!(",\"dir\":\"{}\",\"dst\":", if r.dir == FW_IN { "in" } else { "out" }).as_bytes());
        json::quote(&dst, &mut b);
        if r.port_lo != 0 || r.port_hi != 65535 {
            if r.port_lo == r.port_hi {
                b.extend_from_slice(format!(",\"port\":\"{}\"", r.port_lo).as_bytes());
            } else {
                b.extend_from_slice(format!(",\"port\":\"{}-{}\"", r.port_lo, r.port_hi).as_bytes());
            }
        }
        if r.proto != FW_PROTO_ANY {
            b.extend_from_slice(format!(",\"proto\":\"{}\"", if r.proto == FW_PROTO_TCP { "tcp" } else { "udp" }).as_bytes());
        }
        b.extend_from_slice(format!(",\"action\":\"{}\",\"when\":{}}}", if r.action == FW_DENY { "deny" } else { "allow" }, r.when).as_bytes());
    }
    b.extend_from_slice(b"]}\n");
    Some(b)
}

// ---- reading ----

type Res = Result<(), String>;

fn lossy32(k: &[u8]) -> String {
    String::from_utf8_lossy(&k[..k.len().min(32)]).into_owned()
}

fn only_keys(o: &Json, keys: &[&str]) -> Result<(), String> {
    if let Json::Obj(m) = o {
        for (k, _) in m {
            if !keys.iter().any(|x| x.as_bytes() == k.as_slice()) {
                return Err(lossy32(k));
            }
        }
    }
    Ok(())
}

/// A text of fewer than `max` bytes.
fn jstr(v: Option<&Json>, max: usize) -> Option<&[u8]> {
    match v {
        Some(Json::Str(s)) if s.len() < max => Some(s),
        _ => None,
    }
}

fn parse_uint(s: &[u8], max: u32) -> Option<u32> {
    if s.is_empty() || s.len() > 5 || (s.len() > 1 && s[0] == b'0') || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = s.iter().fold(0u32, |v, &c| v * 10 + (c - b'0') as u32);
    (v <= max).then_some(v)
}

fn parse_ports(v: &Json, r: &mut FwRule) -> bool {
    if let Json::Num(n) = v {
        if *n < 1.0 || *n > 65535.0 || *n != (*n as u32) as f64 {
            return false;
        }
        r.port_lo = *n as u16;
        r.port_hi = *n as u16;
        return true;
    }
    let Some(s) = jstr(Some(v), 16) else { return false };
    let (lo, hi) = match s.iter().position(|&c| c == b'-') {
        Some(d) => match (parse_uint(&s[..d], 65535), parse_uint(&s[d + 1..], 65535)) {
            (Some(lo), Some(hi)) if lo <= hi => (lo, hi),
            _ => return false,
        },
        None => match parse_uint(s, 65535) {
            Some(lo) => (lo, lo),
            None => return false,
        },
    };
    if lo == 0 {
        return false; // port 0 doesn't exist; "any" = no "port"
    }
    r.port_lo = lo as u16;
    r.port_hi = hi as u16;
    true
}

/// An app id that is already in its canonical form.
fn canonical_app(s: &[u8]) -> Option<Vec<u8>> {
    let c = app_canon(s)?;
    (c == s && c.len() < FW_APP_MAX).then_some(c)
}

fn load_rule(t: &mut FwStore, o: &Json, idx: usize) -> Res {
    if !matches!(o, Json::Obj(_)) {
        return Err(format!("rule {idx}: not an object"));
    }
    if let Err(bad) = only_keys(o, &["app", "dir", "dst", "port", "proto", "action", "when"]) {
        return Err(format!("rule {idx}: unknown key \"{bad}\""));
    }
    let mut r = zeroed_rule();
    r.port_hi = 65535;
    let Some(app) = jstr(o.get(b"app"), FW_APP_MAX).and_then(canonical_app) else {
        return Err(format!("rule {idx}: invalid app"));
    };
    set(&mut r.app, &app);
    if let Some(v) = o.get(b"dir") {
        match jstr(Some(v), 8) {
            Some(b"out") => r.dir = FW_OUT,
            Some(b"in") => r.dir = FW_IN,
            _ => return Err(format!("rule {idx}: invalid dir")),
        }
    }
    match jstr(o.get(b"dst"), FW_NAME_MAX + 4) {
        Some(d) if parse_dest(d, &mut r) => {}
        _ => return Err(format!("rule {idx}: invalid dst")),
    }
    if let Some(v) = o.get(b"port") {
        if !parse_ports(v, &mut r) {
            return Err(format!("rule {idx}: invalid port"));
        }
    }
    if let Some(v) = o.get(b"proto") {
        match jstr(Some(v), 8) {
            Some(b"tcp") => r.proto = FW_PROTO_TCP,
            Some(b"udp") => r.proto = FW_PROTO_UDP,
            Some(b"any") => r.proto = FW_PROTO_ANY,
            _ => return Err(format!("rule {idx}: invalid proto")),
        }
    }
    match jstr(o.get(b"action"), 8) {
        Some(b"allow") => r.action = FW_ALLOW,
        Some(b"deny") => r.action = FW_DENY,
        _ => return Err(format!("rule {idx}: invalid action")),
    }
    if let Some(v) = o.get(b"when") {
        match v {
            Json::Num(n) if *n >= 0.0 && *n <= 9007199254740992.0 && *n == (*n as i64) as f64 => r.when = *n as i64,
            _ => return Err(format!("rule {idx}: invalid when")),
        }
    }
    r.life = FW_LIFE_FOREVER;
    match validate(&r).and_then(|v| t.add(v)) {
        Some(_) => Ok(()),
        None => Err(format!("rule {idx}: doesn't fit")),
    }
}

fn load_doc(t: &mut FwStore, root: &Json) -> Res {
    if !matches!(root, Json::Obj(_)) {
        return Err("the root is not an object".into());
    }
    if let Err(bad) = only_keys(root, &["v", "mode", "trusted", "restricted", "rules"]) {
        return Err(format!("unknown key \"{bad}\""));
    }
    if !matches!(root.get(b"v"), Some(Json::Num(n)) if *n == 1.0) {
        return Err("unsupported version".into());
    }
    if let Some(v) = root.get(b"mode") {
        let k = jstr(Some(v), 8).and_then(|m| MODE_NAMES.iter().rposition(|n| n.as_bytes() == m));
        match k {
            Some(k) => t.mode = k as c_int,
            None => return Err("invalid mode".into()),
        }
    }
    if let Some(v) = root.get(b"trusted") {
        let Json::Arr(list) = v else { return Err("trusted is not a list".into()) };
        if list.len() > FW_MAX_APPS {
            return Err("trusted: too many apps".into());
        }
        for (i, k) in list.iter().enumerate() {
            let Some(c) = jstr(Some(k), FW_APP_MAX).and_then(canonical_app) else {
                return Err(format!("trusted {i}: invalid app"));
            };
            if t.apps.get(&c).is_some_and(|a| a.trusted) {
                return Err(format!("trusted {i}: repeated"));
            }
            if !t.set_trusted(&c, true) {
                return Err(format!("trusted {i}: doesn't fit"));
            }
        }
    }
    if let Some(v) = root.get(b"restricted") {
        let Json::Obj(m) = v else { return Err("restricted is not an object".into()) };
        if m.len() > FW_MAX_APPS {
            return Err("restricted: too many apps".into());
        }
        for (key, k) in m {
            let Some(c) = (key.len() < FW_APP_MAX).then(|| canonical_app(key)).flatten() else {
                return Err("restricted: invalid app".into());
            };
            let name = String::from_utf8_lossy(&c[..c.len().min(64)]).into_owned();
            if t.apps.get(&c).is_some_and(|a| a.trusted) {
                return Err(format!("restricted: {name} is also trusted"));
            }
            let flags_list = match k {
                Json::Arr(l) if !l.is_empty() && l.len() <= FLAG_NAMES.len() => l,
                _ => return Err(format!("restricted {name}: invalid list")),
            };
            let mut flags = 0u32;
            for f in flags_list {
                let bit = jstr(Some(f), 32).map_or(0, flag_from_name);
                if bit == 0 || flags & bit != 0 {
                    return Err(format!("restricted {name}: invalid or repeated restriction"));
                }
                flags |= bit;
            }
            if !t.set_restricted(&c, flags) {
                return Err(format!("restricted {name}: doesn't fit"));
            }
        }
    }
    if let Some(v) = root.get(b"rules") {
        let Json::Arr(list) = v else { return Err("rules is not a list".into()) };
        if list.len() > FW_MAX_RULES {
            return Err("too many rules".into());
        }
        for (i, k) in list.iter().enumerate() {
            load_rule(t, k, i)?;
        }
    }
    Ok(())
}

/// Replaces the contents of `s` with those of the JSON text. If anything fails validation, `s` stays the same.
pub(super) fn from_json(s: &mut FwStore, text: &[u8]) -> Res {
    if text.len() > FW_FILE_MAX {
        return Err("file too large".into());
    }
    if text.contains(&0) {
        return Err("byte 0 in the file".into());
    }
    let doc = json::parse(text).map_err(|e| format!("invalid JSON: {}", e.message()))?;
    let mut t = FwStore::new();
    load_doc(&mut t, &doc)?;
    // everything validated: the contents are swapped in one go
    std::mem::swap(s, &mut t);
    Ok(())
}

fn errno_text() -> String {
    // SAFETY: strerror returns a valid C string for any errno.
    unsafe { std::ffi::CStr::from_ptr(libc::strerror(*libc::__errno_location())).to_string_lossy().into_owned() }
}

/// Per-user file: an atomic write with mode 0600.
pub(super) fn save(s: &FwStore, path: &[u8]) -> bool {
    let Some(text) = to_json(s) else { return false };
    let mut tmp = path.to_vec();
    tmp.extend_from_slice(b".tmp");
    if tmp.len() >= 4096 {
        return false;
    }
    let (Ok(tmpc), Ok(pathc)) = (CString::new(tmp), CString::new(path)) else { return false };
    // SAFETY: plain system calls over C strings that live through them.
    unsafe {
        let fd = libc::open(tmpc.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600);
        if fd < 0 {
            return false;
        }
        let mut off = 0;
        while off < text.len() {
            let w = libc::write(fd, text[off..].as_ptr().cast(), text.len() - off);
            if w < 0 && *libc::__errno_location() == libc::EINTR {
                continue;
            }
            if w <= 0 {
                break;
            }
            off += w as usize;
        }
        let mut ok = off == text.len() && libc::fsync(fd) == 0;
        if libc::close(fd) != 0 {
            ok = false;
        }
        if !ok || libc::rename(tmpc.as_ptr(), pathc.as_ptr()) != 0 {
            libc::unlink(tmpc.as_ptr());
            return false;
        }
    }
    true
}

/// 0 loaded, 1 there is no file (the store stays as it is), -1 error (the message in the Err).
pub(super) fn load(s: &mut FwStore, path: &[u8]) -> Result<bool, String> {
    let pathc = CString::new(path).map_err(|_| "no path".to_string())?;
    // SAFETY: plain system calls over a C string that lives through them and a buffer of the size read.
    let text = unsafe {
        let fd = libc::open(pathc.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        if fd < 0 {
            if *libc::__errno_location() == libc::ENOENT {
                return Ok(false);
            }
            return Err(format!("could not open: {}", errno_text()));
        }
        let mut st: libc::stat = std::mem::zeroed();
        if libc::fstat(fd, &mut st) != 0 || st.st_mode & libc::S_IFMT != libc::S_IFREG || st.st_size < 0 || st.st_size as u64 > FW_FILE_MAX as u64 {
            libc::close(fd);
            return Err("not a regular file or too large".into());
        }
        let want = st.st_size as usize;
        let mut buf = vec![0u8; want];
        let mut got = 0;
        while got < want {
            let r = libc::read(fd, buf[got..].as_mut_ptr().cast(), want - got);
            if r < 0 && *libc::__errno_location() == libc::EINTR {
                continue;
            }
            if r <= 0 {
                break;
            }
            got += r as usize;
        }
        libc::close(fd);
        if got != want {
            return Err("incomplete read".into());
        }
        buf
    };
    from_json(s, &text)?;
    Ok(true)
}

// ---- C ABI ----

unsafe fn put_err(err: *mut c_char, n: usize, msg: &str) {
    if err.is_null() || n == 0 {
        return;
    }
    let b = msg.as_bytes();
    let k = b.len().min(n - 1);
    ptr::copy_nonoverlapping(b.as_ptr(), err.cast(), k);
    *err.add(k) = 0;
}

/// Compact JSON (only the "forever" rules); malloc'd memory with a NUL at the end. NULL if it fails.
///
/// # Safety
/// `s` is NULL or a store; `len` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn fw_to_json(s: *const FwStore, len: *mut usize) -> *mut c_char {
    let Some(j) = store(s).and_then(to_json) else { return ptr::null_mut() };
    let p = libc::malloc(j.len() + 1).cast::<u8>();
    if p.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping(j.as_ptr(), p, j.len());
    *p.add(j.len()) = 0;
    if !len.is_null() {
        *len = j.len();
    }
    p.cast()
}

/// Replaces the contents of `s` with those of the JSON. If anything fails validation, `s` stays the same and
/// `err` says what. 0 ok, -1 invalid.
///
/// # Safety
/// `s` is NULL or a store, `text` NULL or valid for `len` bytes, `err` NULL or has `errlen` bytes.
#[no_mangle]
pub unsafe extern "C" fn fw_from_json(s: *mut FwStore, text: *const c_char, len: usize, err: *mut c_char, errlen: usize) -> c_int {
    if !err.is_null() && errlen > 0 {
        *err = 0;
    }
    let (Some(s), false) = (store_mut(s), text.is_null()) else {
        put_err(err, errlen, "no data");
        return -1;
    };
    match from_json(s, std::slice::from_raw_parts(text.cast(), len)) {
        Ok(()) => 0,
        Err(m) => {
            put_err(err, errlen, &m);
            -1
        }
    }
}

/// # Safety
/// `s` is NULL or a store, `path` NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fw_save(s: *const FwStore, path: *const c_char) -> c_int {
    match (store(s), path.is_null()) {
        (Some(s), false) if save(s, cstr(path)) => 0,
        _ => -1,
    }
}

/// 0 loaded, 1 there is no file yet (`s` stays as it is), -1 error.
///
/// # Safety
/// `s` is NULL or a store, `path` NULL or a C string, `err` NULL or has `errlen` bytes.
#[no_mangle]
pub unsafe extern "C" fn fw_load(s: *mut FwStore, path: *const c_char, err: *mut c_char, errlen: usize) -> c_int {
    if !err.is_null() && errlen > 0 {
        *err = 0;
    }
    let (Some(s), false) = (store_mut(s), path.is_null()) else {
        put_err(err, errlen, "no path");
        return -1;
    };
    match load(s, cstr(path)) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(m) => {
            put_err(err, errlen, &m);
            -1
        }
    }
}
