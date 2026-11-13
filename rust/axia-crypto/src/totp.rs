//! Rust port of `server/src/vault/totp.c` with the C ABI of `totp.h`: HOTP (RFC 4226), TOTP
//! (RFC 6238) with SHA-1/256/512, base32 (RFC 4648) and `otpauth://` URIs. The secret never
//! leaves axia-vault; every scratch buffer that held it is wiped.
use crate::vcrypto::{hmac_sha1, hmac_sha256, hmac_sha512};
use std::os::raw::{c_char, c_int, c_long};
use std::{ptr, slice};
use zeroize::Zeroize;

const TOTP_SHA256: c_int = 1;
const TOTP_SHA512: c_int = 2;

/// Layout of the C `Totp`.
#[repr(C)]
pub struct Totp {
    key: [u8; 128],
    keylen: usize,
    algo: c_int,
    digits: c_int,
    period: c_int,
    hotp: c_int,
    counter: u64,
    issuer: [u8; 128],
    account: [u8; 128],
}

impl Totp {
    fn fresh() -> Totp {
        Totp {
            key: [0; 128],
            keylen: 0,
            algo: 0,
            digits: 6,
            period: 30,
            hotp: 0,
            counter: 0,
            issuer: [0; 128],
            account: [0; 128],
        }
    }
}

/// The bytes of a NUL-terminated C string.
unsafe fn cstr<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        std::ffi::CStr::from_ptr(p).to_bytes()
    }
}
/// A buffer filled like a C string: it ends at the first NUL.
fn until_nul(v: &[u8]) -> &[u8] {
    &v[..v.iter().position(|&c| c == 0).unwrap_or(v.len())]
}

/// base32 → bytes (ignores spaces, dashes and '='; upper or lower case).
fn b32_decode(s: &[u8], out: &mut [u8]) -> Option<usize> {
    let (mut acc, mut bits, mut n) = (0u32, 0u32, 0usize);
    for &c in s {
        if matches!(c, b' ' | b'-' | b'=' | b'\t' | b'\n' | b'\r') {
            continue;
        }
        let c = c.to_ascii_uppercase();
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        acc = acc << 5 | v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            if n >= out.len() {
                return None;
            }
            out[n] = (acc >> bits) as u8;
            n += 1;
        }
    }
    Some(n)
}

/// Length decoded into `out` (`cap` bytes) or -1.
#[no_mangle]
pub unsafe extern "C" fn base32_decode(s: *const c_char, out: *mut u8, cap: usize) -> c_long {
    let o: &mut [u8] = if cap == 0 || out.is_null() { &mut [] } else { slice::from_raw_parts_mut(out, cap) };
    match b32_decode(cstr(s), o) {
        Some(n) => n as c_long,
        None => -1,
    }
}

/// Without '='; `out` receives at most `cap - 1` characters and a NUL. Returns the length.
#[no_mangle]
pub unsafe extern "C" fn base32_encode(input: *const u8, n: usize, out: *mut c_char, cap: usize) -> usize {
    const A: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let inp: &[u8] = if n == 0 || input.is_null() { &[] } else { slice::from_raw_parts(input, n) };
    let (mut acc, mut bits, mut o) = (0u32, 0u32, 0usize);
    let put = |c: u8, o: &mut usize| {
        if *o + 1 < cap && !out.is_null() {
            *out.add(*o) = c as c_char;
            *o += 1;
        }
    };
    for &b in inp {
        acc = acc << 8 | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            put(A[((acc >> bits) & 31) as usize], &mut o);
        }
    }
    if bits > 0 {
        put(A[((acc << (5 - bits)) & 31) as usize], &mut o);
    }
    if cap != 0 && !out.is_null() {
        *out.add(o) = 0;
    }
    o
}

/// Decodes `%XX` and `+` of a URI component into at most `cap - 1` bytes.
fn pct_decode(s: &[u8], cap: usize) -> Vec<u8> {
    let n = s.len();
    let mut dst = Vec::with_capacity(cap.min(n + 1));
    let mut i = 0;
    while i < n && dst.len() + 1 < cap {
        if s[i] == b'%' && i + 2 < n && s[i + 1].is_ascii_hexdigit() && s[i + 2].is_ascii_hexdigit() {
            let h = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
            dst.push(h(s[i + 1]) << 4 | h(s[i + 2]));
            i += 2;
        } else {
            dst.push(if s[i] == b'+' { b' ' } else { s[i] });
        }
        i += 1;
    }
    dst
}

/// C `atoi`: leading blanks, sign and digits; the long is truncated to int.
fn atoi(s: &[u8]) -> c_int {
    let s = s.iter().copied().skip_while(|c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')).collect::<Vec<_>>();
    let (neg, digits) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, &s[..]),
    };
    let mut v: i64 = 0;
    for &c in digits.iter().take_while(|c| c.is_ascii_digit()) {
        v = v.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    (if neg { -v } else { v }) as c_int
}

/// C `strtoull(s, NULL, 10)`: overflow gives `ULLONG_MAX`, a minus sign negates.
fn strtoull10(s: &[u8]) -> u64 {
    let s = s.iter().copied().skip_while(|c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')).collect::<Vec<_>>();
    let (neg, digits) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, &s[..]),
    };
    let mut v: u64 = 0;
    let mut over = false;
    for &c in digits.iter().take_while(|c| c.is_ascii_digit()) {
        match v.checked_mul(10).and_then(|x| x.checked_add((c - b'0') as u64)) {
            Some(x) => v = x,
            None => over = true,
        }
    }
    if over {
        u64::MAX
    } else if neg {
        v.wrapping_neg()
    } else {
        v
    }
}

/// `snprintf(dst, 128, "%.127s", src)` into a zeroed field.
fn put_str(dst: &mut [u8; 128], src: &[u8]) {
    dst.fill(0);
    let src = until_nul(src);
    let k = src.len().min(127);
    dst[..k].copy_from_slice(&src[..k]);
}

fn starts_ci(s: &[u8], p: &[u8]) -> bool {
    s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p)
}

fn parse(text: &[u8], t: &mut Totp) -> c_int {
    let skip = text.iter().take_while(|c| matches!(c, b' ' | b'\t' | b'\n')).count();
    let text = &text[skip..];
    if !starts_ci(text, b"otpauth://") {
        return match b32_decode(text, &mut t.key) {
            Some(n) if n > 0 => {
                t.keylen = n;
                0
            }
            _ => -1,
        };
    }
    let mut p = &text[10..];
    if starts_ci(p, b"totp/") {
        p = &p[5..];
    } else if starts_ci(p, b"hotp/") {
        p = &p[5..];
        t.hotp = 1;
    } else {
        return -1;
    }
    let mut q = p.iter().position(|&c| c == b'?');
    let llen = q.unwrap_or(p.len());
    let mut label = pct_decode(&p[..llen], 256);
    {
        let l = until_nul(&label);
        if let Some(colon) = l.iter().position(|&c| c == b':') {
            put_str(&mut t.issuer, &l[..colon]);
            let acc = &l[colon + 1..];
            let sp = acc.iter().take_while(|&&c| c == b' ').count();
            put_str(&mut t.account, &acc[sp..]);
        } else {
            put_str(&mut t.account, l);
        }
    }
    label.zeroize();
    let mut have_secret = false;
    while let Some(i) = q {
        let rest = &p[i + 1..];
        let amp = rest.iter().position(|&c| c == b'&');
        let seg = &rest[..amp.unwrap_or(rest.len())];
        q = amp.map(|a| i + 1 + a);
        let Some(kl) = seg.iter().position(|&c| c == b'=') else { continue };
        let mut vbuf = pct_decode(&seg[kl + 1..], 512);
        let val = until_nul(&vbuf);
        let name = &seg[..kl];
        let mut bad = false;
        if name.eq_ignore_ascii_case(b"secret") {
            match b32_decode(val, &mut t.key) {
                Some(k) if k > 0 => {
                    t.keylen = k;
                    have_secret = true;
                }
                _ => bad = true,
            }
        } else if name.eq_ignore_ascii_case(b"issuer") {
            put_str(&mut t.issuer, val);
        } else if name.eq_ignore_ascii_case(b"algorithm") {
            if val.eq_ignore_ascii_case(b"SHA256") {
                t.algo = TOTP_SHA256;
            } else if val.eq_ignore_ascii_case(b"SHA512") {
                t.algo = TOTP_SHA512;
            } else if !val.eq_ignore_ascii_case(b"SHA1") {
                bad = true;
            }
        } else if name.eq_ignore_ascii_case(b"digits") {
            let d = atoi(val);
            if (6..=10).contains(&d) {
                t.digits = d;
            } else {
                bad = true;
            }
        } else if name.eq_ignore_ascii_case(b"period") {
            let pr = atoi(val);
            if (1..=3600).contains(&pr) {
                t.period = pr;
            } else {
                bad = true;
            }
        } else if name.eq_ignore_ascii_case(b"counter") {
            t.counter = strtoull10(val);
        }
        vbuf.zeroize();
        if bad {
            return -1;
        }
    }
    if have_secret {
        0
    } else {
        -1
    }
}

/// Reads an otpauth://totp/… (or hotp) URI or a bare base32 secret. 0 on success.
#[no_mangle]
pub unsafe extern "C" fn totp_parse(text: *const c_char, t: *mut Totp) -> c_int {
    if t.is_null() {
        return -1;
    }
    ptr::write(t, Totp::fresh());
    parse(cstr(text), &mut *t)
}

#[no_mangle]
pub unsafe extern "C" fn hotp_code(t: *const Totp, counter: u64) -> u32 {
    if t.is_null() {
        return 0;
    }
    let t = &*t;
    let msg = counter.to_be_bytes();
    let mut mac = [0u8; 64];
    let klen = t.keylen.min(128);
    let (k, m) = (t.key.as_ptr(), msg.as_ptr());
    let ml = match t.algo {
        TOTP_SHA256 => {
            hmac_sha256(k, klen, m, 8, mac.as_mut_ptr());
            32
        }
        TOTP_SHA512 => {
            hmac_sha512(k, klen, m, 8, mac.as_mut_ptr());
            64
        }
        _ => {
            hmac_sha1(k, klen, m, 8, mac.as_mut_ptr());
            20
        }
    };
    let off = (mac[ml - 1] & 15) as usize;
    let bin = ((mac[off] & 0x7f) as u32) << 24
        | (mac[off + 1] as u32) << 16
        | (mac[off + 2] as u32) << 8
        | mac[off + 3] as u32;
    mac.zeroize();
    let mut modulo = 1u64;
    for _ in 0..t.digits.clamp(0, 10) {
        modulo *= 10;
    }
    (bin as u64 % modulo) as u32
}

/// Code for unix time `now`; `*remaining` = seconds left in the period.
#[no_mangle]
pub unsafe extern "C" fn totp_code(t: *const Totp, now: u64, remaining: *mut c_int) -> u32 {
    if t.is_null() {
        return 0;
    }
    let period = if (*t).period > 0 { (*t).period } else { 30 };
    if !remaining.is_null() {
        *remaining = period - (now % period as u64) as c_int;
    }
    hotp_code(t, now / period as u64)
}

/// Code with leading zeros: `out` of at least 11 bytes.
#[no_mangle]
pub unsafe extern "C" fn totp_format(t: *const Totp, code: u32, out: *mut c_char) {
    if t.is_null() || out.is_null() {
        return;
    }
    let w = (*t).digits.clamp(0, 10) as usize;
    let s = format!("{code:0w$}");
    let b = s.as_bytes();
    let k = b.len().min(10);
    ptr::copy_nonoverlapping(b.as_ptr(), out as *mut u8, k);
    *out.add(k) = 0;
}

#[no_mangle]
pub unsafe extern "C" fn totp_wipe(t: *mut Totp) {
    if !t.is_null() {
        slice::from_raw_parts_mut(t as *mut u8, std::mem::size_of::<Totp>()).zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn parse_str(s: &str) -> (c_int, Totp) {
        let c = CString::new(s).unwrap();
        let mut t = std::mem::MaybeUninit::<Totp>::uninit();
        let rc = unsafe { totp_parse(c.as_ptr(), t.as_mut_ptr()) };
        (rc, unsafe { t.assume_init() })
    }
    fn s(b: &[u8]) -> &str {
        std::str::from_utf8(until_nul(b)).unwrap()
    }
    fn code(t: &Totp, now: u64) -> String {
        let mut o = [0 as c_char; 12];
        unsafe { totp_format(t, totp_code(t, now, ptr::null_mut()), o.as_mut_ptr()) };
        unsafe { std::ffi::CStr::from_ptr(o.as_ptr()) }.to_str().unwrap().to_string()
    }

    fn with_key(key: &[u8], algo: c_int) -> Totp {
        let mut t = Totp::fresh();
        t.key[..key.len()].copy_from_slice(key);
        t.keylen = key.len();
        t.digits = 8;
        t.algo = algo;
        t
    }

    #[test]
    fn layout_matches_c() {
        assert_eq!(std::mem::size_of::<Totp>(), 416);
        assert_eq!(std::mem::offset_of!(Totp, counter), 152);
        assert_eq!(std::mem::offset_of!(Totp, account), 288);
    }

    #[test]
    fn rfc6238() {
        let a = with_key(b"12345678901234567890", 0);
        let b = with_key(b"12345678901234567890123456789012", 1);
        let c = with_key(b"1234567890123456789012345678901234567890123456789012345678901234", 2);
        for (t, s1, s256, s512) in [
            (59u64, "94287082", "46119246", "90693936"),
            (1111111109, "07081804", "68084774", "25091201"),
            (1111111111, "14050471", "67062674", "99943326"),
            (1234567890, "89005924", "91819424", "93441116"),
            (2000000000, "69279037", "90698825", "38618901"),
            (20000000000, "65353130", "77737706", "47863826"),
        ] {
            assert_eq!(code(&a, t), s1);
            assert_eq!(code(&b, t), s256);
            assert_eq!(code(&c, t), s512);
        }
        let mut rem = 0;
        unsafe { totp_code(&a, 59, &mut rem) };
        assert_eq!(rem, 1);
    }

    #[test]
    fn rfc4226() {
        let mut a = with_key(b"12345678901234567890", 0);
        a.digits = 6;
        let want = ["755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583", "399871", "520489"];
        for (i, w) in want.iter().enumerate() {
            let mut o = [0 as c_char; 12];
            unsafe { totp_format(&a, hotp_code(&a, i as u64), o.as_mut_ptr()) };
            assert_eq!(unsafe { std::ffi::CStr::from_ptr(o.as_ptr()) }.to_str().unwrap(), *w);
        }
    }

    #[test]
    fn parse_uris() {
        let (rc, t) = parse_str("otpauth://totp/ACME%20Co:john.doe@email.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=ACME%20Co&algorithm=SHA1&digits=6&period=30");
        assert_eq!((rc, s(&t.issuer), s(&t.account), t.keylen, t.digits), (0, "ACME Co", "john.doe@email.com", 20, 6));
        let (rc, t) = parse_str("JBSW Y3DP EHPK 3PXP");
        assert_eq!((rc, t.keylen, &t.key[..10]), (0, 10, &b"Hello!\xde\xad\xbe\xef"[..]));
        assert_ne!(parse_str("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&digits=11").0, 0);
        assert_ne!(parse_str("otpauth://totp/x?issuer=sin-secreto").0, 0);
        assert_ne!(parse_str("no es base32 !!").0, 0);
        assert_ne!(parse_str("otpauth://steam/x?secret=JBSWY3DPEHPK3PXP").0, 0);
        assert_ne!(parse_str("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&algorithm=MD5").0, 0);
        let (rc, t) = parse_str("  OTPAUTH://HOTP/Lbl?Secret=jbswy3dpehpk3pxp&Algorithm=sha512&PERIOD=60&counter=18446744073709551616&digits=8");
        assert_eq!((rc, t.hotp, t.algo, t.period, t.counter, t.digits, s(&t.account)), (0, 1, 2, 60, u64::MAX, 8, "Lbl"));
        let (_, t) = parse_str("otpauth://totp/Iss:%20%20 me?secret=JBSWY3DPEHPK3PXP&counter=-1");
        assert_eq!((s(&t.issuer), s(&t.account), t.counter), ("Iss", "me", u64::MAX));
        // the label stops at an encoded NUL like a C string would
        let (_, t) = parse_str("otpauth://totp/ab%00cd?secret=JBSWY3DPEHPK3PXP");
        assert_eq!(s(&t.account), "ab");
        // a 129-byte secret does not fit
        let big = "A".repeat(210);
        assert_ne!(parse_str(&format!("otpauth://totp/x?secret={big}")).0, 0);
    }

    #[test]
    fn base32() {
        let mut enc = [0 as c_char; 64];
        let n = unsafe { base32_encode(b"Hello!\xde\xad\xbe\xef".as_ptr(), 10, enc.as_mut_ptr(), 64) };
        assert_eq!(n, 16);
        assert_eq!(unsafe { std::ffi::CStr::from_ptr(enc.as_ptr()) }.to_str().unwrap(), "JBSWY3DPEHPK3PXP");
        // truncated by the capacity, always terminated
        let n = unsafe { base32_encode(b"Hello!".as_ptr(), 6, enc.as_mut_ptr(), 5) };
        assert_eq!(n, 4);
        assert_eq!(unsafe { std::ffi::CStr::from_ptr(enc.as_ptr()) }.to_str().unwrap(), "JBSW");
        let mut out = [0u8; 4];
        let c = CString::new("JBSWY3DPEHPK3PXP").unwrap();
        assert_eq!(unsafe { base32_decode(c.as_ptr(), out.as_mut_ptr(), 4) }, -1);
        assert_eq!(unsafe { base32_decode(c"jbswy3dp".as_ptr(), out.as_mut_ptr(), 4) }, -1);
        let mut out = [0u8; 8];
        assert_eq!(unsafe { base32_decode(c"jb-sw y3dp====".as_ptr(), out.as_mut_ptr(), 8) }, 5);
    }

    #[test]
    fn wipe() {
        let (_, mut t) = parse_str("JBSWY3DPEHPK3PXP");
        unsafe { totp_wipe(&mut t) };
        assert!(t.key.iter().all(|&b| b == 0) && t.keylen == 0 && t.digits == 0);
    }
}
