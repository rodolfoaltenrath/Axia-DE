//! What LordOS saved about the network and what the system says about DNS: the per-interface
//! configuration (`/etc/lordos/net/if/<name>.conf`), the public metadata of the VPNs and the known
//! Wi-Fi networks, `resolv.conf`, and a `.ovpn` with the files it names embedded.
use crate::Out;
use axia_errcode::lerr;
use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;

/// Where the files are (the tests point them at a fake tree).
#[derive(Clone, Debug)]
pub struct Paths {
    pub if_dir: String,
    pub vpn_dir: String,
    pub wifi_dir: String,
    pub resolv_conf: String,
    pub resolved_conf: String,
    pub sys_class_net: String,
}

impl Default for Paths {
    fn default() -> Paths {
        Paths {
            if_dir: "/etc/lordos/net/if".into(),
            vpn_dir: "/etc/lordos/net/vpn".into(),
            wifi_dir: "/etc/lordos/net/wifi".into(),
            resolv_conf: "/etc/resolv.conf".into(),
            resolved_conf: "/run/systemd/resolve/resolv.conf".into(),
            sys_class_net: "/sys/class/net".into(),
        }
    }
}

/// At most `max` bytes of a file (a leaf that is a link is not followed), cut at the first NUL as
/// a C string would be; `None` if it cannot be read.
pub fn read_file(path: &str, max: usize) -> Option<Vec<u8>> {
    let f = fs::OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(path).ok()?;
    let mut b = Vec::new();
    f.take(max as u64).read_to_end(&mut b).ok()?;
    if let Some(z) = b.iter().position(|&c| c == 0) {
        b.truncate(z);
    }
    Some(b)
}

/// Letters, digits, `-`, `_` and `.`, up to 15 bytes, and not "." or "..".
pub fn valid_ifname(s: &[u8]) -> bool {
    !s.is_empty() && s.len() < 16 && s != b"." && s != b".." && s.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}

/// The `idx`-th value of `key=value` in a text of lines (a value is cut to `n - 1` bytes).
pub fn kv_get(text: &[u8], key: &str, idx: usize, n: usize) -> Option<Vec<u8>> {
    let key = key.as_bytes();
    let mut seen = 0;
    for line in text.split(|&c| c == b'\n') {
        if line.len() > key.len() && line.starts_with(key) && line[key.len()] == b'=' {
            if seen == idx {
                let v = &line[key.len() + 1..];
                return Some(v[..v.len().min(n - 1)].to_vec());
            }
            seen += 1;
        }
    }
    None
}

/// What `sscanf("%<w0>s %<w1>s …")` reads: words of at most that many characters, each starting
/// where the last one stopped (a longer word continues in the next).
pub fn scan_words(line: &[u8], widths: &[usize]) -> Vec<Vec<u8>> {
    let is_space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let (mut i, mut out) = (0, Vec::new());
    for &w in widths {
        while i < line.len() && is_space(line[i]) {
            i += 1;
        }
        let start = i;
        while i < line.len() && !is_space(line[i]) && i - start < w {
            i += 1;
        }
        if i == start {
            break;
        }
        out.push(line[start..i].to_vec());
    }
    out
}

/// The configuration LordOS saved for an interface, as JSON: {method, addresses, gateway, dns}, or
/// `null` if it has none. `None` if the name is not a valid interface name.
pub fn if_config_json(p: &Paths, ifname: &[u8]) -> Option<Vec<u8>> {
    if !valid_ifname(ifname) {
        return None;
    }
    let path = format!("{}/{}.conf", p.if_dir, String::from_utf8_lossy(ifname));
    let mut o = Out::default();
    let Some(t) = read_file(&path, 8192) else {
        o.raw("null");
        return Some(o.0);
    };
    let get = |k: &str, i: usize| kv_get(&t, k, i, 256);
    o.raw("{\"method\":");
    o.string(&get("method", 0).unwrap_or_default());
    o.raw(",\"addresses\":[");
    for i in 0.. {
        let Some(v) = get("address", i) else { break };
        if i > 0 {
            o.raw(",");
        }
        o.string(&v);
    }
    o.raw("],\"gateway\":");
    o.string(&get("gateway", 0).unwrap_or_default());
    o.raw(",\"dns\":[");
    for i in 0.. {
        let Some(v) = get("dns", i) else { break };
        if i > 0 {
            o.raw(",");
        }
        o.string(&v);
    }
    o.raw("]}");
    Some(o.0)
}

fn resolved_active(p: &Paths) -> bool {
    let Ok(l) = fs::read_link(&p.resolv_conf) else { return false };
    let l = l.to_string_lossy();
    // the link's text is read up to 63 bytes
    let l: String = l.chars().take(63).collect();
    l.contains("systemd/resolve") && access_readable(&p.resolved_conf)
}

fn access_readable(path: &str) -> bool {
    std::ffi::CString::new(path).is_ok_and(|c| {
        // SAFETY: a valid C string.
        unsafe { libc::access(c.as_ptr(), libc::R_OK) == 0 }
    })
}

/// The DNS in use: {source, servers, search}.
pub fn dns_json(p: &Paths) -> Vec<u8> {
    let resolved = resolved_active(p);
    let path = if resolved { &p.resolved_conf } else { &p.resolv_conf };
    let mut o = Out::default();
    o.raw(&format!("{{\"source\":\"{}\",\"servers\":[", if resolved { "systemd-resolved" } else { "resolv.conf" }));
    let mut search: Vec<u8> = Vec::new();
    let mut n = 0;
    if let Ok(text) = fs::read(path) {
        // fgets with a 512-byte buffer: a longer line comes in pieces
        for line in lines_of(&text, 512) {
            let words = scan_words(&line, &[15, 255]);
            if words.len() != 2 {
                continue;
            }
            let w = &words[0];
            if w == b"nameserver" {
                if n > 0 {
                    o.raw(",");
                }
                n += 1;
                o.string(&words[1]);
            }
            if w == b"search" || w == b"domain" {
                let rest = &line[line.iter().position(|&c| !c.is_ascii_whitespace()).unwrap_or(0) + w.len()..];
                let rest = rest.split(|&c| c == b'\n').next().unwrap_or(b"");
                search = rest[..rest.len().min(511)].to_vec();
            }
        }
    }
    o.raw("],\"search\":[");
    let mut first = true;
    for tok in search.split(|&c| c == b' ' || c == b'\t').filter(|t| !t.is_empty()) {
        if !first {
            o.raw(",");
        }
        first = false;
        o.string(tok);
    }
    o.raw("]}");
    o.0
}

/// The lines of a text as `fgets(buf, n)` returns them: each with its newline, cut at `n - 1`.
fn lines_of(text: &[u8], n: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let upto = rest.iter().position(|&c| c == b'\n').map_or(rest.len(), |i| i + 1).min(n - 1);
        out.push(rest[..upto].to_vec());
        rest = &rest[upto..];
    }
    out
}

/// The (public) `.json` files of a folder, in name order, as an array: only those whose text starts
/// with `{`, and only their first line.
pub fn dir_json_array(dir: &str) -> Vec<u8> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    names.sort();
    let mut o = Out::default();
    o.raw("[");
    let mut n = 0;
    for name in names {
        if name.starts_with('.') || name.len() < 6 || !name.ends_with(".json") {
            continue;
        }
        let Some(t) = read_file(&format!("{dir}/{name}"), 8192) else { continue };
        if t.first() != Some(&b'{') {
            continue;
        }
        if n > 0 {
            o.raw(",");
        }
        n += 1;
        o.0.extend_from_slice(t.split(|&c| c == b'\n').next().unwrap_or(b""));
    }
    o.raw("]");
    o.0
}

const INLINE_DIRS: [&str; 8] = ["ca", "cert", "key", "tls-auth", "tls-crypt", "tls-crypt-v2", "extra-certs", "crl-verify"];

/// Reads a user's `.ovpn` and embeds the files it names (ca, cert, key, tls-auth …) that are
/// beside it. `Err` is the error code line (`ovpn.missingFile`, `sys.readFailed`), `cap` bytes at most.
pub fn ovpn_inline(path: &[u8], cap: usize) -> Result<Vec<u8>, Vec<u8>> {
    let spath = String::from_utf8_lossy(path).into_owned();
    let Some(t) = read_file(&spath, 256 * 1024) else { return Err(lerr(cap, "sys.readFailed", &[("path", path)])) };
    let dir: Vec<u8> = match path.iter().rposition(|&c| c == b'/') {
        Some(i) => path[..i].to_vec(),
        None => b".".to_vec(),
    };
    let mut out: Vec<u8> = Vec::new();
    let mut rest: &[u8] = &t;
    while !rest.is_empty() {
        let (l, next) = match rest.iter().position(|&c| c == b'\n') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        rest = next;
        let words = scan_words(l, &[31, 399, 15]);
        let mut done = false;
        if words.len() >= 2 {
            let (w, a1) = (&words[0], &words[1]);
            if let Some(name) = INLINE_DIRS.iter().find(|d| d.as_bytes() == w.as_slice()).filter(|_| a1 != b"[inline]") {
                let mut p = if a1.first() == Some(&b'/') { a1.clone() } else { [dir.as_slice(), b"/", a1].concat() };
                p.truncate(1023);
                let Some(f) = read_file(&String::from_utf8_lossy(&p), 64 * 1024) else {
                    return Err(lerr(cap, "ovpn.missingFile", &[("file", a1)]));
                };
                out.extend_from_slice(format!("<{name}>\n").as_bytes());
                out.extend_from_slice(&f);
                if f.last() != Some(&b'\n') {
                    out.push(b'\n');
                }
                out.extend_from_slice(format!("</{name}>\n").as_bytes());
                if *name == "tls-auth" && words.len() == 3 {
                    out.extend_from_slice(b"key-direction ");
                    out.extend_from_slice(&words[2]);
                    out.push(b'\n');
                }
                done = true;
            }
            if !done && w == b"auth-user-pass" {
                out.extend_from_slice(b"auth-user-pass\n");
                done = true;
            }
        }
        if !done {
            out.extend_from_slice(l);
            out.push(b'\n');
        }
    }
    if out.is_empty() {
        return Err(lerr(cap, "sys.outOfMemory", &[]));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_names() {
        assert!(valid_ifname(b"eth0") && valid_ifname(b"wlp2s0") && valid_ifname(b"wg-home_1.2") && valid_ifname(&[b'a'; 15]));
        for bad in [&b""[..], b".", b"..", b"a b", b"a/b", &[b'a'; 16], b"\xc3\xa9"] {
            assert!(!valid_ifname(bad), "{bad:?}");
        }
    }

    #[test]
    fn values_by_key_and_position() {
        let t = b"method=static\naddress=10.0.0.2/24\naddress=fd00::2/64\ngateway=10.0.0.1\nmethodx=no\naddress=\nnoequals\n";
        assert_eq!(kv_get(t, "method", 0, 256).as_deref(), Some(&b"static"[..]));
        assert_eq!(kv_get(t, "address", 1, 256).as_deref(), Some(&b"fd00::2/64"[..]));
        assert_eq!(kv_get(t, "address", 2, 256).as_deref(), Some(&b""[..]), "an empty value is still a value");
        assert_eq!(kv_get(t, "address", 3, 256), None);
        assert_eq!(kv_get(t, "gateway", 0, 4).as_deref(), Some(&b"10."[..]), "cut to the buffer");
        assert_eq!(kv_get(t, "dns", 0, 256), None);
    }

    #[test]
    fn words_are_read_like_sscanf() {
        assert_eq!(scan_words(b"  ca   ca.crt  x\n", &[31, 399, 15]), [b"ca".to_vec(), b"ca.crt".to_vec(), b"x".to_vec()]);
        assert_eq!(scan_words(b"ab", &[31, 399, 15]), [b"ab".to_vec()]);
        assert_eq!(scan_words(b"abcdef gh", &[3, 5]), [b"abc".to_vec(), b"def".to_vec()], "a long word continues in the next");
        assert!(scan_words(b"   \n", &[3]).is_empty());
    }

    #[test]
    fn lines_come_in_pieces_like_fgets() {
        assert_eq!(lines_of(b"ab\ncd", 512), [b"ab\n".to_vec(), b"cd".to_vec()]);
        assert_eq!(lines_of(b"abcdefg\nh\n", 4), [b"abc".to_vec(), b"def".to_vec(), b"g\n".to_vec(), b"h\n".to_vec()]);
        assert!(lines_of(b"", 4).is_empty());
    }
}
