//! C string helpers with the exact semantics of the libc calls the parsers
//! were written against (C locale).

use core::ffi::c_char;
use std::ffi::CStr;

/// NUL-terminated input as bytes (NULL reads as "").
pub(crate) unsafe fn cs<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        b""
    } else {
        unsafe { CStr::from_ptr(p).to_bytes() }
    }
}

/// Length-delimited input (NULL or 0 reads as empty).
pub(crate) unsafe fn bytes<'a>(p: *const c_char, n: usize) -> &'a [u8] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(p.cast(), n) }
    }
}

/// Caller buffer of `cap` bytes (empty when NULL or 0).
pub(crate) unsafe fn out<'a>(p: *mut c_char, cap: usize) -> &'a mut [u8] {
    if p.is_null() || cap == 0 {
        &mut []
    } else {
        unsafe { std::slice::from_raw_parts_mut(p.cast(), cap) }
    }
}

/// The C string held in a fixed array (up to the first NUL).
pub(crate) fn z(a: &[u8]) -> &[u8] {
    match a.iter().position(|&c| c == 0) {
        Some(n) => &a[..n],
        None => a,
    }
}

/// Same, viewing a fixed `c_char` array as bytes.
pub(crate) fn zc(a: &[c_char]) -> &[u8] {
    // SAFETY: c_char and u8 have the same size and alignment.
    z(unsafe { std::slice::from_raw_parts(a.as_ptr().cast(), a.len()) })
}

pub(crate) fn zc_mut(a: &mut [c_char]) -> &mut [u8] {
    // SAFETY: c_char and u8 have the same size and alignment.
    unsafe { std::slice::from_raw_parts_mut(a.as_mut_ptr().cast(), a.len()) }
}

/// `snprintf(dst, sizeof dst, "%s", s)`.
pub(crate) fn put(dst: &mut [u8], s: &[u8]) {
    if dst.is_empty() {
        return;
    }
    let n = s.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&s[..n]);
    dst[n] = 0;
}

pub(crate) fn putc(dst: &mut [c_char], s: &[u8]) {
    put(zc_mut(dst), s)
}

/// `strlen` of a caller buffer (bounded by its size).
pub(crate) fn strlen(a: &[u8]) -> usize {
    z(a).len()
}

/// A `malloc`ed NUL-terminated copy; NULL when out of memory.
pub(crate) fn malloc_str(s: &[u8]) -> *mut c_char {
    unsafe {
        let p = libc::malloc(s.len() + 1).cast::<u8>();
        if !p.is_null() {
            std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
            *p.add(s.len()) = 0;
        }
        p.cast()
    }
}

pub(crate) fn set_len(olen: *mut usize, n: usize) {
    if !olen.is_null() {
        unsafe { *olen = n }
    }
}

pub(crate) fn lower(c: u8) -> u8 {
    c.to_ascii_lowercase()
}
pub(crate) fn upper(c: u8) -> u8 {
    c.to_ascii_uppercase()
}
pub(crate) fn lower_all(s: &[u8]) -> Vec<u8> {
    s.iter().map(|&c| lower(c)).collect()
}

/// `strcasecmp(a, b) == 0`.
pub(crate) fn eq_ic(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// `strncasecmp(s, p, strlen(p)) == 0`.
pub(crate) fn starts_ic(s: &[u8], p: &[u8]) -> bool {
    s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p)
}

/// `strstr`.
pub(crate) fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() {
        return Some(0);
    }
    h.windows(n.len()).position(|w| w == n)
}

/// `strcasestr`.
pub(crate) fn find_ic(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() {
        return Some(0);
    }
    h.windows(n.len()).position(|w| w.eq_ignore_ascii_case(n))
}

pub(crate) fn has_ic(h: &[u8], n: &[u8]) -> bool {
    find_ic(h, n).is_some()
}

pub(crate) fn chr(s: &[u8], c: u8) -> Option<usize> {
    s.iter().position(|&x| x == c)
}
pub(crate) fn rchr(s: &[u8], c: u8) -> Option<usize> {
    s.iter().rposition(|&x| x == c)
}

/// `strcspn`.
pub(crate) fn cspn(s: &[u8], set: &[u8]) -> usize {
    s.iter().position(|c| set.contains(c)).unwrap_or(s.len())
}

/// C `isspace` in the C locale.
pub(crate) fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}
pub(crate) fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}
pub(crate) fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}
pub(crate) fn is_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}
pub(crate) fn is_xdigit(c: u8) -> bool {
    c.is_ascii_hexdigit()
}

/// `strtol(s, &end, 10)`: the value, the number of bytes consumed (0 when no
/// digits, like `end == s`) and whether it overflowed (`ERANGE`, saturated).
pub(crate) fn strtol(s: &[u8]) -> (i64, usize, bool) {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let d0 = i;
    let mut v: i128 = 0;
    let mut over = false;
    while i < s.len() && is_digit(s[i]) {
        if !over {
            v = v * 10 + (s[i] - b'0') as i128;
            if v > i64::MAX as i128 + 1 {
                over = true;
            }
        }
        i += 1;
    }
    if i == d0 {
        return (0, 0, false);
    }
    let v = if neg { -v } else { v };
    if over || v > i64::MAX as i128 || v < i64::MIN as i128 {
        return (if neg { i64::MIN } else { i64::MAX }, i, true);
    }
    (v as i64, i, false)
}

/// `atol`.
pub(crate) fn atol(s: &[u8]) -> i64 {
    strtol(s).0
}

/// `sscanf(s, "%d")` for one conversion: the int (glibc stores the
/// saturated long truncated to int) and the bytes consumed; None on failure.
pub(crate) fn scan_d(s: &[u8]) -> Option<(i32, usize)> {
    let (v, n, _) = strtol(s);
    if n == 0 { None } else { Some((v as i32, n)) }
}

/// `strtod(s, &end)` through libc, so the accepted syntax (hex floats,
/// "inf", "nan", exponents) is exactly the C one. Returns (value, consumed).
pub(crate) fn strtod(s: &[u8]) -> (f64, usize) {
    let mut tmp = Vec::with_capacity(s.len() + 1);
    tmp.extend_from_slice(z(s));
    tmp.push(0);
    let mut end: *mut c_char = std::ptr::null_mut();
    let v = unsafe { libc::strtod(tmp.as_ptr().cast(), &mut end) };
    let used = if end.is_null() {
        0
    } else {
        end as usize - tmp.as_ptr() as usize
    };
    (v, used)
}

/// UTF-8 encoding without validation (callers pass valid code points).
pub(crate) fn put_cp(o: &mut Vec<u8>, cp: u32) {
    if cp < 0x80 {
        o.push(cp as u8);
    } else if cp < 0x800 {
        o.extend_from_slice(&[(0xc0 | (cp >> 6)) as u8, (0x80 | (cp & 0x3f)) as u8]);
    } else if cp < 0x10000 {
        o.extend_from_slice(&[
            (0xe0 | (cp >> 12)) as u8,
            (0x80 | ((cp >> 6) & 0x3f)) as u8,
            (0x80 | (cp & 0x3f)) as u8,
        ]);
    } else {
        o.extend_from_slice(&[
            (0xf0 | (cp >> 18)) as u8,
            (0x80 | ((cp >> 12) & 0x3f)) as u8,
            (0x80 | ((cp >> 6) & 0x3f)) as u8,
            (0x80 | (cp & 0x3f)) as u8,
        ]);
    }
}

/// Length of the valid UTF-8 sequence at the start of s (0 if invalid).
pub(crate) fn utf8_len(s: &[u8]) -> usize {
    let Some(&c) = s.first() else { return 0 };
    if c < 0x80 {
        return 1;
    }
    let (k, mut cp) = match c {
        0xc2..=0xdf => (2, (c & 0x1f) as u32),
        0xe0..=0xef => (3, (c & 0x0f) as u32),
        0xf0..=0xf4 => (4, (c & 0x07) as u32),
        _ => return 0,
    };
    if k > s.len() {
        return 0;
    }
    for &d in &s[1..k] {
        if d & 0xc0 != 0x80 {
            return 0;
        }
        cp = (cp << 6) | (d & 0x3f) as u32;
    }
    if (k == 3 && cp < 0x800)
        || (k == 4 && !(0x10000..=0x10ffff).contains(&cp))
        || (0xd800..=0xdfff).contains(&cp)
    {
        return 0;
    }
    k
}

pub(crate) fn utf8_valid(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        let k = utf8_len(&s[i..]);
        if k == 0 {
            return false;
        }
        i += k;
    }
    true
}
