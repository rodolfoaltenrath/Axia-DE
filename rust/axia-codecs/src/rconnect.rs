//! What rconnect.c checks and writes, with no process around it: the arguments of a
//! "Connect to Server", rclone's configuration, and reading what ssh-keyscan,
//! ssh-keygen and openssl answered (text a server chose). C ABI of the pure part of
//! server/src/lan/rconnect.h (tests/lan_test.c).
use crate::lanparse::{buf, cstr};
use std::ffi::{c_char, c_int};

const RC_TLS_CHANGED: c_int = -1;
const RC_TLS_SYSTEM: c_int = 0;
const RC_TLS_PIN: c_int = 1;
const RC_TLS_ASK: c_int = 2;

fn alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

pub fn host_ok(s: &[u8]) -> bool {
    !s.is_empty() && s.len() <= 253 && s[0] != b'-' && s[0] != b'.' && s.iter().all(|&c| alnum(c) || c == b'.' || c == b'-')
}

pub fn user_ok(s: &[u8]) -> bool {
    s.len() <= 64 && s.first() != Some(&b'-') && s.iter().all(|&c| alnum(c) || b"._-".contains(&c))
}

pub fn path_ok(s: &[u8]) -> bool {
    if s.is_empty() {
        return true;
    }
    if s[0] != b'/' || s.len() > 200 || find(s, b"/../").is_some() || find(s, b"//").is_some() || s.ends_with(b"/..") {
        return false;
    }
    s.iter().all(|&c| alnum(c) || b"/._-".contains(&c))
}

fn clean(s: &[u8]) -> bool {
    s.iter().all(|&c| c >= 0x20 && c != 0x7f)
}

/// rclone's configuration for one remote; None if refused.
pub fn config(kind: &[u8], host: &[u8], port: i32, user: &[u8], ob: &[u8], path: &[u8], known: &[u8]) -> Option<Vec<u8>> {
    // every value is one line: nothing can start another key
    if !clean(host) || !clean(user) || !clean(ob) || !clean(path) || !clean(known) {
        return None;
    }
    let mut o = Vec::new();
    let mut put = |parts: &[&[u8]]| parts.iter().for_each(|p| o.extend_from_slice(p));
    match kind {
        b"sftp" => {
            if known.is_empty() {
                return None; // never without a host key to check against
            }
            // nothing is run on the server, and nothing is learned that rclone would try to save
            let port = (if port != 0 { port } else { 22 }).to_string();
            put(&[b"[r]\ntype = sftp\nhost = ", host, b"\nuser = ", user, b"\nport = ", port.as_bytes(), b"\npass = ", ob]);
            put(&[b"\nknown_hosts_file = ", known, b"\nshell_type = none\nmd5sum_command = none\nsha1sum_command = none\n"]);
        }
        b"ftp" | b"ftps" => {
            let tls: &[u8] = if kind.len() == 3 {
                b"explicit_tls = false"
            } else if port == 990 {
                b"tls = true" // 990: TLS from the first byte
            } else {
                b"explicit_tls = true"
            };
            let port = (if port != 0 { port } else { 21 }).to_string();
            let user: &[u8] = if user.is_empty() { b"anonymous" } else { user };
            put(&[b"[r]\ntype = ftp\nhost = ", host, b"\nuser = ", user, b"\nport = ", port.as_bytes(), b"\npass = ", ob, b"\n", tls, b"\n"]);
        }
        b"webdav" | b"davs" => {
            let pt = if port != 0 { format!(":{port}") } else { String::new() };
            let scheme: &[u8] = if kind[0] == b'w' { b"http" } else { b"https" };
            put(&[b"[r]\ntype = webdav\nurl = ", scheme, b"://", host, pt.as_bytes(), path, b"\nvendor = other\nuser = ", user]);
            put(&[b"\npass = ", ob, b"\n"]);
        }
        _ => return None,
    }
    Some(o)
}

/// One "host type base64" line of ssh-keyscan's, for the host asked about.
pub fn keyline_ok(l: &[u8], tag: &[u8]) -> bool {
    let t = tag.len();
    if !l.starts_with(tag) || l.get(t) != Some(&b' ') {
        return false;
    }
    let p = &l[t + 1..];
    let Some(sp) = p.iter().position(|&c| c == b' ') else { return false };
    if sp == 0 || sp > 40 || !p[..sp].iter().all(|&c| alnum(c) || b"-@.".contains(&c)) {
        return false;
    }
    let key = &p[sp + 1..];
    (40..=8192).contains(&key.len()) && key.iter().all(|&c| alnum(c) || b"+/=".contains(&c))
}

pub fn host_tag(host: &[u8], port: i32) -> Vec<u8> {
    if port == 0 || port == 22 {
        host.to_vec()
    } else {
        let mut v = b"[".to_vec();
        v.extend_from_slice(host);
        v.extend_from_slice(format!("]:{port}").as_bytes());
        v
    }
}

/// 1 the file holds this exact line, 0 it has other lines for the tag, -1 nothing for the tag.
pub fn known(file: &[u8], tag: &[u8], line: &[u8]) -> i32 {
    let mut other = false;
    let mut lines: Vec<&[u8]> = file.split(|&c| c == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop(); // the C loop stops at the end of the text, not on an empty last line
    }
    for l in lines {
        if l == line {
            return 1;
        }
        if l.len() > tag.len() && l.starts_with(tag) && l[tag.len()] == b' ' {
            other = true;
        }
    }
    if other { 0 } else { -1 }
}

/// s_client's output: the server's certificate and whether the system trusts it.
pub fn tls_parse(out: &[u8]) -> Option<(&[u8], bool)> {
    let a = find(out, b"-----BEGIN CERTIFICATE-----")?;
    let b = a + find(&out[a..], b"-----END CERTIFICATE-----")?;
    let pem = &out[a..b + 25];
    // the last one: what comes before it includes text the server chose
    const V: &[u8] = b"Verify return code: ";
    let last = (0..out.len()).rev().find(|&i| out[i..].starts_with(V));
    Some((pem, last.is_some_and(|v| out[v + V.len()..].starts_with(b"0 (ok)"))))
}

/// ssh-keygen -lf's lines as "ED25519 SHA256:… · RSA SHA256:…". Writes into `out`
/// like successive snprintf calls; false on a line it can't read or on no room.
pub fn prints(k: &[u8], out: &mut [u8]) -> bool {
    let cap = out.len();
    out[0] = 0;
    let mut o = 0;
    if k.is_empty() {
        return false;
    }
    let mut lines: Vec<&[u8]> = k.split(|&c| c == b'\n').collect();
    if k.ends_with(b"\n") {
        lines.pop();
    }
    for line in lines {
        let Some(fp) = line.iter().position(|&c| c == b' ') else { return false };
        let Some(ty) = line.iter().rposition(|&c| c == b'(').map(|i| i + 1) else { return false };
        let fp = fp + 1;
        let Some(fe) = line[fp..].iter().position(|&c| c == b' ').map(|i| fp + i) else { return false };
        let Some(te) = line[ty..].iter().position(|&c| c == b')').map(|i| ty + i) else { return false };
        let mut piece: Vec<u8> = if o != 0 { " · ".as_bytes().to_vec() } else { Vec::new() };
        piece.extend_from_slice(&line[ty..te]);
        piece.push(b' ');
        piece.extend_from_slice(&line[fp..fe]);
        let room = cap - o;
        let n = piece.len().min(room - 1);
        out[o..o + n].copy_from_slice(&piece[..n]);
        out[o + n] = 0;
        if piece.len() >= room {
            return false;
        }
        o += piece.len();
    }
    o != 0
}

pub fn tls_decide(pin: Option<&[u8]>, pem: &[u8], verified: i32) -> c_int {
    match pin {
        // an accepted certificate outranks the system's list
        Some(p) => {
            if p != pem { RC_TLS_CHANGED } else { RC_TLS_PIN }
        }
        None => {
            if verified == 1 { RC_TLS_SYSTEM } else { RC_TLS_ASK }
        }
    }
}

pub fn name_ok(s: &[u8]) -> bool {
    let mut n = s.len();
    while n > 0 && matches!(s[n - 1], b'\n' | b'\r' | b' ') {
        n -= 1;
    }
    const YES: &[u8] = b" does match certificate";
    n > YES.len() && s[..n].ends_with(YES)
}

pub fn answer(shown: &[u8], print: &[u8], yes: bool) -> i32 {
    if shown.is_empty() || shown != print {
        0
    } else if yes {
        1
    } else {
        -1
    }
}

/// snprintf(out, n, "%s", s): cut to fit, always terminated (n > 0).
fn put_trunc(out: &mut [u8], s: &[u8]) {
    if out.is_empty() {
        return;
    }
    let n = s.len().min(out.len() - 1);
    out[..n].copy_from_slice(&s[..n]);
    out[n] = 0;
}

// ---- C ABI ----

#[no_mangle]
pub unsafe extern "C" fn rconnect_host_ok(s: *const c_char) -> c_int {
    host_ok(cstr(s)) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_user_ok(s: *const c_char) -> c_int {
    user_ok(cstr(s)) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_path_ok(s: *const c_char) -> c_int {
    path_ok(cstr(s)) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_config(
    o: *mut c_char,
    cap: usize,
    kind: *const c_char,
    host: *const c_char,
    port: c_int,
    user: *const c_char,
    ob: *const c_char,
    path: *const c_char,
    known: *const c_char,
) -> c_int {
    let Some(text) = config(cstr(kind), cstr(host), port, cstr(user), cstr(ob), cstr(path), cstr(known)) else { return -1 };
    put_trunc(buf(o.cast(), cap), &text);
    if !text.is_empty() && text.len() < cap { text.len() as c_int } else { -1 }
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_keyline_ok(l: *const c_char, tag: *const c_char) -> c_int {
    keyline_ok(cstr(l), cstr(tag)) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_host_tag(host: *const c_char, port: c_int, out: *mut c_char, n: usize) {
    put_trunc(buf(out.cast(), n), &host_tag(cstr(host), port));
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_known(file: *const c_char, tag: *const c_char, line: *const c_char) -> c_int {
    known(cstr(file), cstr(tag), cstr(line))
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_tls_parse(out: *const c_char, pem: *mut c_char, cap: usize) -> c_int {
    let Some((cert, ok)) = tls_parse(cstr(out)) else { return -1 };
    if cert.len() + 2 > cap {
        return -1;
    }
    let p = buf(pem.cast(), cap);
    p[..cert.len()].copy_from_slice(cert);
    p[cert.len()] = b'\n';
    p[cert.len() + 1] = 0;
    ok as c_int
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_prints(k: *const c_char, out: *mut c_char, cap: usize) -> c_int {
    let o = buf(out.cast(), cap);
    if o.is_empty() {
        return -1;
    }
    if prints(cstr(k), o) { 0 } else { -1 }
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_tls_decide(pin: *const c_char, pem: *const c_char, verified: c_int) -> c_int {
    tls_decide((!pin.is_null()).then(|| cstr(pin)), cstr(pem), verified)
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_name_ok(s: *const c_char) -> c_int {
    name_ok(cstr(s)) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn rconnect_answer(shown: *const c_char, print: *const c_char, yes: c_int) -> c_int {
    answer(cstr(shown), cstr(print), yes != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_and_config() {
        assert!(host_ok(b"files.example") && !host_ok(b"-x") && !host_ok(b"a b"));
        assert!(path_ok(b"") && path_ok(b"/srv/a") && !path_ok(b"/srv/..") && !path_ok(b"/a//b"));
        let c = config(b"ftps", b"h", 990, b"", b"ob", b"", b"").unwrap();
        assert!(c.ends_with(b"user = anonymous\nport = 990\npass = ob\ntls = true\n"));
        assert!(config(b"sftp", b"h", 0, b"u", b"o", b"", b"").is_none());
        let c = config(b"davs", b"h", 8443, b"u", b"o", b"/dav", b"").unwrap();
        assert!(std::str::from_utf8(&c).unwrap().contains("url = https://h:8443/dav\n"));
        assert!(config(b"webdav", b"h\n", 0, b"u", b"o", b"", b"").is_none());
    }

    #[test]
    fn answers() {
        let mut out = [0u8; 64];
        assert!(prints(b"256 SHA256:abc host (ED25519)\n3072 SHA256:def host (RSA)\n", &mut out));
        assert_eq!(&out[..out.iter().position(|&c| c == 0).unwrap()], "ED25519 SHA256:abc · RSA SHA256:def".as_bytes());
        assert!(!prints(b"garbage\n", &mut out));
        assert_eq!(known(b"a ssh-ed25519 K\nb x\n", b"a", b"a ssh-ed25519 K"), 1);
        assert_eq!(known(b"a ssh-rsa Z\n", b"a", b"a ssh-ed25519 K"), 0);
        assert_eq!(known(b"", b"a", b"a k"), -1);
        let o = b"x Verify return code: 0 (ok)\n-----BEGIN CERTIFICATE-----\nAA\n-----END CERTIFICATE-----\nVerify return code: 21 (bad)";
        assert_eq!(tls_parse(o).map(|(p, ok)| (p.len(), ok)), Some((27 + 4 + 25, false)));
        assert!(name_ok(b"Hostname h does match certificate\n") && !name_ok(b" does match certificate"));
        assert_eq!(host_tag(b"h", 2222), b"[h]:2222");
    }
}
