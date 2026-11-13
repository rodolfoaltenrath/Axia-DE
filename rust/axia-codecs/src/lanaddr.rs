//! Which addresses Files → Network may touch, and the arguments of the share-*
//! actions: pure functions, the same on both sides of the admin helper. C ABI of
//! server/src/lan/lanaddr.h (tests/lan_test.c).
use crate::lanparse::{buf, clean_text, copy_cstr, cstr};
use std::ffi::{c_char, c_int};

/// a (host order) in 10/8, 172.16/12, 192.168/16 or 169.254/16.
pub fn private(a: u32) -> bool {
    a >> 24 == 10 || a >> 20 == 0xac1 || a >> 16 == 0xc0a8 || a >> 16 == 0xa9fe
}

/// Its subnet, or the /24 around the address when the subnet is larger.
pub fn sweep_range(addr: u32, mask: u32) -> Option<(u32, u32)> {
    let mask = mask.max(0xffff_ff00);
    if !mask < 3 {
        return None;
    }
    let net = addr & mask;
    Some((net + 1, (net | !mask) - 1))
}

/// inet_pton(AF_INET): four decimal octets, no leading zeros, nothing else.
fn ipv4(s: &[u8]) -> Option<u32> {
    let mut parts = 0;
    let mut v = 0u32;
    for part in s.split(|&c| c == b'.') {
        parts += 1;
        if parts > 4 || part.is_empty() || part.len() > 3 || !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        if part.len() > 1 && part[0] == b'0' {
            return None;
        }
        let n = part.iter().fold(0u32, |n, &c| n * 10 + (c - b'0') as u32);
        if n > 255 {
            return None;
        }
        v = v << 8 | n;
    }
    (parts == 4).then_some(v)
}

/// Nothing below 0x20 (or 0x7f), nothing past the cap.
fn field_ok(s: &[u8], max: usize) -> bool {
    s.len() <= max && s.iter().all(|&c| c >= 0x20 && c != 0x7f)
}

fn share_name_ok(s: &[u8]) -> bool {
    !s.is_empty() && field_ok(s, 80) && s != b"." && s != b".." && !s.iter().any(|c| b"/\\:*?\"<>|".contains(c))
}

fn export_path_ok(s: &[u8]) -> bool {
    if s.first() != Some(&b'/') || !field_ok(s, 200) || s.windows(2).any(|w| w == b"//") {
        return false;
    }
    if s.iter().any(|c| b"\\:\"'`$;|&<>".contains(c)) {
        return false;
    }
    // no "/.." as a whole component
    !(0..s.len()).any(|i| s[i..].starts_with(b"/..") && (i + 3 == s.len() || s[i + 3] == b'/'))
}

fn user_ok(s: &[u8]) -> bool {
    field_ok(s, 60) && s.iter().all(|&c| c.is_ascii_alphanumeric() || c >= 0x80 || b"._- ".contains(&c))
}

#[repr(C)]
pub struct ShareReq {
    nfs: c_int,
    addr: [u8; 16],
    share: [u8; 256],
    user: [u8; 64],
    domain: [u8; 64],
    password: *const c_char,
}

/// Parses and validates `buf` in place (it stays NUL-separated).
///
/// # Safety
/// `buf` is a writable C string and `r` a valid ShareReq.
#[no_mangle]
pub unsafe extern "C" fn share_req_parse(b: *mut c_char, r: *mut ShareReq) -> c_int {
    if b.is_null() || r.is_null() {
        return -1;
    }
    let len = cstr(b).len();
    let s = std::slice::from_raw_parts_mut(b.cast::<u8>(), len);
    // kind, addr, share, user, domain, password
    let mut starts = vec![0usize];
    for i in 0..len {
        if starts.len() == 6 {
            break;
        }
        if s[i] == b'\n' {
            s[i] = 0;
            starts.push(i + 1);
        }
    }
    if starts.len() != 6 {
        return -1;
    }
    let f: Vec<&[u8]> = starts
        .iter()
        .map(|&st| {
            let rest = &s[st..];
            &rest[..rest.iter().position(|&c| c == 0).unwrap_or(rest.len())]
        })
        .collect();
    std::ptr::write_bytes(r, 0, 1);
    let r = &mut *r;
    match f[0] {
        b"nfs" => r.nfs = 1,
        b"smb" => {}
        _ => return -1,
    }
    if !ipv4(f[1]).is_some_and(private) {
        return -1;
    }
    copy_cstr(&mut r.addr, f[1]);
    if if r.nfs != 0 { !export_path_ok(f[2]) } else { !share_name_ok(f[2]) } {
        return -1;
    }
    copy_cstr(&mut r.share, f[2]);
    if !user_ok(f[3]) || !user_ok(f[4]) {
        return -1;
    }
    if r.nfs != 0 && (!f[3].is_empty() || !f[4].is_empty() || !f[5].is_empty()) {
        return -1; // NFS takes no credentials
    }
    if !field_ok(f[5], 256) {
        return -1;
    }
    copy_cstr(&mut r.user, f[3]);
    copy_cstr(&mut r.domain, f[4]);
    r.password = b.add(starts[5]);
    0
}

/// "<share> on <addr>", without / or control characters and never starting with a dot.
pub fn leaf(nfs: bool, share: &[u8], addr: &[u8]) -> Vec<u8> {
    let mut s = share;
    if nfs {
        s = match share.iter().rposition(|&c| c == b'/') {
            Some(i) if i + 1 < share.len() => &share[i + 1..],
            _ => b"export",
        };
    }
    let mut name: Vec<u8> = s.iter().take(127).map(|&c| if c < 0x20 || c == b'/' || c == 0x7f { b'_' } else { c }).collect();
    if name.first() == Some(&b'.') {
        name[0] = b'_';
    }
    name.extend_from_slice(b" on ");
    name.extend_from_slice(addr);
    name
}

/// # Safety
/// `r` is a valid ShareReq and `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn share_leaf(r: *const ShareReq, out: *mut c_char, n: usize) {
    let Some(r) = r.as_ref() else { return };
    let o = buf(out.cast(), n);
    if o.is_empty() {
        return;
    }
    let share = &r.share[..r.share.iter().position(|&c| c == 0).unwrap_or(r.share.len())];
    let addr = &r.addr[..r.addr.iter().position(|&c| c == 0).unwrap_or(r.addr.len())];
    copy_cstr(o, &leaf(r.nfs != 0, share, addr));
}

/// /media/<user>/<one folder>.
pub fn path_ok(path: &[u8], user: &[u8]) -> bool {
    let mut base = b"/media/".to_vec();
    base.extend_from_slice(user);
    base.push(b'/');
    if base.len() >= 96 || !path.starts_with(&base) {
        return false;
    }
    let leaf = &path[base.len()..];
    !leaf.is_empty() && leaf.len() <= 200 && !leaf.contains(&b'/') && leaf[0] != b'.' && leaf.iter().all(|&c| c >= 0x20)
}

#[no_mangle]
pub extern "C" fn lan_private(a: u32) -> c_int {
    private(a) as c_int
}

/// # Safety
/// `first` and `last` are valid pointers.
#[no_mangle]
pub unsafe extern "C" fn lan_sweep_range(addr: u32, mask: u32, first: *mut u32, last: *mut u32) -> c_int {
    match sweep_range(addr, mask) {
        Some((a, b)) => {
            *first = a;
            *last = b;
            (b - a + 1) as c_int
        }
        None => 0,
    }
}

/// # Safety
/// `inp` is NULL or a C string; `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn lan_clean_text(inp: *const c_char, out: *mut c_char, n: usize) -> c_int {
    let o = buf(out.cast(), n);
    if o.is_empty() {
        return 0;
    }
    let t = clean_text(cstr(inp), n);
    copy_cstr(o, &t);
    !t.is_empty() as c_int
}

/// # Safety
/// Both are C strings.
#[no_mangle]
pub unsafe extern "C" fn share_path_ok(path: *const c_char, user: *const c_char) -> c_int {
    if path_ok(cstr(path), cstr(user)) { 0 } else { -1 }
}

/// # Safety
/// `t` is a C string.
#[no_mangle]
pub unsafe extern "C" fn share_fstype(t: *const c_char) -> c_int {
    matches!(cstr(t), b"cifs" | b"smb3" | b"nfs" | b"nfs4") as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert!(private(0x0a00_0001) && private(0xc0a8_0101) && private(0xac1f_0001) && !private(0x0808_0808));
        assert_eq!(sweep_range(0xc0a8_0105, 0xffff_0000), Some((0xc0a8_0101, 0xc0a8_01fe)));
        assert_eq!(sweep_range(1, 0xffff_fffe), None);
        assert_eq!(ipv4(b"192.168.1.2"), Some(0xc0a8_0102));
        for bad in [&b"01.2.3.4"[..], b"1.2.3", b"1.2.3.4.5", b"256.1.1.1", b"1..2.3", b" 1.2.3.4", b""] {
            assert_eq!(ipv4(bad), None);
        }
    }

    #[test]
    fn requests() {
        unsafe {
            let mut r: ShareReq = std::mem::zeroed();
            let mut b = b"smb\n192.168.1.4\nMusic\nana\n\nse cret\0".to_vec();
            assert_eq!(share_req_parse(b.as_mut_ptr().cast(), &mut r), 0);
            assert_eq!(std::ffi::CStr::from_ptr(r.password).to_bytes(), b"se cret");
            assert_eq!(leaf(false, b".Music", b"192.168.1.4"), b"_Music on 192.168.1.4");
            assert_eq!(leaf(true, b"/srv/", b"10.0.0.1"), b"export on 10.0.0.1");
            let mut b = b"nfs\n10.0.0.2\n/srv/../etc\n\n\n\0".to_vec();
            assert_eq!(share_req_parse(b.as_mut_ptr().cast(), &mut r), -1);
            let mut b = b"smb\n8.8.8.8\nx\n\n\n\0".to_vec();
            assert_eq!(share_req_parse(b.as_mut_ptr().cast(), &mut r), -1);
        }
        assert!(export_path_ok(b"/srv/..x") && !export_path_ok(b"/srv/..") && !export_path_ok(b"/a//b"));
        assert!(path_ok(b"/media/ana/Music on 1.2.3.4", b"ana") && !path_ok(b"/media/ana/.x", b"ana"));
    }
}
