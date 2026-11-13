//! IMAP4rev1 response parser (RFC 3501) for axia-maild, pure and bounds-checked: the C ABI of
//! `server/src/mail/imapp.h`, which this replaced.
//!
//! The connection layer assembles one complete response: the line plus any literals, kept inline
//! exactly as on the wire (`{N}\r\n` followed by N bytes). `imap_parse` turns it into a small tree
//! of values (atoms, strings, NIL, lists) that live in one arena. Bracketed sections
//! (`BODY[HEADER.FIELDS (FROM)]<0>`, `[UIDVALIDITY 7]`) stay a single atom, as clients expect.
//! Lists nest at most `IMAP_MAX_NEST` deep (the header named the limit; the C parser never applied it).
use core::ffi::{c_char, c_int};
use core::{ptr, slice};

pub const IMAP_MAX_VALUES: usize = 200_000;
pub const IMAP_MAX_NEST: usize = 32;

pub const IV_ATOM: c_int = 0;
pub const IV_STR: c_int = 1;
pub const IV_NIL: c_int = 2;
pub const IV_LIST: c_int = 3;

#[repr(C)]
pub struct IVal {
    r#type: c_int,
    /// ATOM and STR: the bytes (they point into the response or the arena).
    s: *const c_char,
    n: usize,
    /// LIST: the first child; the next sibling.
    kid: *mut IVal,
    next: *mut IVal,
}

#[repr(C)]
pub struct IResp {
    /// "*", "+" or the command tag.
    tag: [c_char; 32],
    /// The tokens after the tag.
    first: *mut IVal,
    /// Owns the values and the unescaped strings.
    arena: *mut c_char,
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    vals: *mut IVal,
    nvals: usize,
    maxvals: usize,
    strs: *mut u8,
    nstr: usize,
}

impl P<'_> {
    fn newval(&mut self, t: c_int) -> *mut IVal {
        if self.nvals >= self.maxvals {
            return ptr::null_mut();
        }
        // SAFETY: nvals < maxvals and the arena holds maxvals values.
        unsafe {
            let v = self.vals.add(self.nvals);
            self.nvals += 1;
            v.write(IVal { r#type: t, s: ptr::null(), n: 0, kid: ptr::null_mut(), next: ptr::null_mut() });
            v
        }
    }
}

fn atom_char(c: u8) -> bool {
    c > 0x20 && c < 0x7f && !matches!(c, b'(' | b')' | b'{' | b'"' | b'%')
}

/// One value at `p.i`. Null at the end of the current list or on error (`*err` set).
fn value(p: &mut P, err: &mut bool, depth: usize) -> *mut IVal {
    let len = p.s.len();
    while p.i < len && p.s[p.i] == b' ' {
        p.i += 1;
    }
    if p.i >= len {
        return ptr::null_mut();
    }
    let c = p.s[p.i];
    if c == b'\r' || c == b'\n' {
        p.i = len;
        return ptr::null_mut();
    }
    if c == b')' {
        return ptr::null_mut();
    }
    let fail = |err: &mut bool| -> *mut IVal {
        *err = true;
        ptr::null_mut()
    };
    if c == b'"' {
        let v = p.newval(IV_STR);
        if v.is_null() {
            return fail(err);
        }
        p.i += 1;
        // SAFETY: the string area has len + 2 bytes and an unescaped string is never longer than
        // its quoted form, which is consumed from the same response.
        let o = unsafe { p.strs.add(p.nstr) };
        let mut n = 0usize;
        while p.i < len && p.s[p.i] != b'"' {
            let mut ch = p.s[p.i];
            if ch == b'\\' && p.i + 1 < len {
                p.i += 1;
                ch = p.s[p.i];
            } else if ch == b'\r' || ch == b'\n' {
                return fail(err);
            }
            unsafe { *o.add(n) = ch };
            n += 1;
            p.i += 1;
        }
        if p.i >= len {
            return fail(err);
        }
        p.i += 1;
        unsafe {
            *o.add(n) = 0;
            p.nstr += n + 1;
            (*v).s = o.cast();
            (*v).n = n;
        }
        return v;
    }
    if c == b'{' {
        let mut k = p.i + 1;
        let (mut n, d0) = (0u64, k);
        while k < len && p.s[k].is_ascii_digit() {
            n = n * 10 + u64::from(p.s[k] - b'0');
            if n > 1 << 31 {
                return fail(err);
            }
            k += 1;
        }
        if k == d0 {
            return fail(err);
        }
        if k < len && p.s[k] == b'+' {
            k += 1;
        }
        if k >= len || p.s[k] != b'}' {
            return fail(err);
        }
        k += 1;
        if k + 1 < len && p.s[k] == b'\r' && p.s[k + 1] == b'\n' {
            k += 2;
        } else if k < len && p.s[k] == b'\n' {
            k += 1;
        } else {
            return fail(err);
        }
        if n > (len - k) as u64 {
            return fail(err);
        }
        let v = p.newval(IV_STR);
        if v.is_null() {
            return fail(err);
        }
        unsafe {
            (*v).s = p.s.as_ptr().add(k).cast();
            (*v).n = n as usize;
        }
        p.i = k + n as usize;
        return v;
    }
    if c == b'(' {
        if depth >= IMAP_MAX_NEST {
            return fail(err);
        }
        let v = p.newval(IV_LIST);
        if v.is_null() {
            return fail(err);
        }
        p.i += 1;
        let mut last: *mut IVal = ptr::null_mut();
        loop {
            let k = value(p, err, depth + 1);
            if *err {
                return ptr::null_mut();
            }
            if k.is_null() {
                break;
            }
            unsafe {
                if last.is_null() {
                    (*v).kid = k;
                } else {
                    (*last).next = k;
                }
            }
            last = k;
        }
        if p.i >= len || p.s[p.i] != b')' {
            return fail(err);
        }
        p.i += 1;
        return v;
    }
    // an atom, possibly with [section] (spaces and parens allowed inside) and <partial>
    let a = p.i;
    let mut nest = 0;
    while p.i < len {
        let ch = p.s[p.i];
        if ch == b'[' {
            nest += 1;
            p.i += 1;
            continue;
        }
        if ch == b']' && nest > 0 {
            nest -= 1;
            p.i += 1;
            continue;
        }
        if nest > 0 {
            if ch == b'\r' || ch == b'\n' {
                break;
            }
            p.i += 1;
            continue;
        }
        if !atom_char(ch) && ch != b'%' {
            break;
        }
        p.i += 1;
    }
    if nest > 0 || p.i == a {
        return fail(err);
    }
    let v = p.newval(IV_ATOM);
    if v.is_null() {
        return fail(err);
    }
    unsafe {
        (*v).s = p.s.as_ptr().add(a).cast();
        (*v).n = p.i - a;
        if (*v).n == 3 && p.s[a..a + 3].eq_ignore_ascii_case(b"NIL") {
            (*v).r#type = IV_NIL;
        }
    }
    v
}

/// 0 ok, -1 malformed (unbalanced lists, a bad literal, too many values).
///
/// # Safety
/// `buf` covers `len` bytes, which must outlive `out`'s values; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imap_parse(buf: *const c_char, len: usize, out: *mut IResp) -> c_int {
    unsafe {
        out.write(IResp { tag: [0; 32], first: ptr::null_mut(), arena: ptr::null_mut() });
        let s: &[u8] = if len == 0 { &[] } else { slice::from_raw_parts(buf.cast(), len) };
        let maxvals = (len + 2).min(IMAP_MAX_VALUES);
        let vbytes = maxvals * core::mem::size_of::<IVal>();
        let arena = libc::malloc(vbytes + len + 2).cast::<u8>();
        if arena.is_null() {
            return -1;
        }
        (*out).arena = arena.cast();
        let mut p = P { s, i: 0, vals: arena.cast(), nvals: 0, maxvals, strs: arena.add(vbytes), nstr: 0 };
        // the tag
        let mut k = 0;
        while k < len && s[k] != b' ' && s[k] != b'\r' && s[k] != b'\n' && k + 1 < 32 {
            (*out).tag[k] = s[k] as c_char;
            k += 1;
        }
        (*out).tag[k] = 0;
        if k == 0 {
            imap_resp_free(out);
            return -1;
        }
        p.i = k;
        let mut last: *mut IVal = ptr::null_mut();
        let mut err = false;
        loop {
            let v = value(&mut p, &mut err, 0);
            if err {
                imap_resp_free(out);
                return -1;
            }
            if v.is_null() {
                if p.i < len && s[p.i] == b')' {
                    imap_resp_free(out); // a stray ')'
                    return -1;
                }
                break;
            }
            if last.is_null() {
                (*out).first = v;
            } else {
                (*last).next = v;
            }
            last = v;
        }
        0
    }
}

/// # Safety
/// `r` was filled by `imap_parse` (or zeroed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imap_resp_free(r: *mut IResp) {
    unsafe {
        libc::free((*r).arena.cast());
        (*r).arena = ptr::null_mut();
        (*r).first = ptr::null_mut();
    }
}

fn bytes<'a>(v: &IVal) -> &'a [u8] {
    if v.s.is_null() || v.n == 0 {
        &[]
    } else {
        // SAFETY: a value points at n bytes of the response or of the arena.
        unsafe { slice::from_raw_parts(v.s.cast(), v.n) }
    }
}
fn is(v: *const IVal, atom: &[u8]) -> bool {
    // SAFETY: a null pointer or a value of a parsed response.
    unsafe { v.as_ref() }.is_some_and(|v| v.r#type == IV_ATOM && bytes(v).eq_ignore_ascii_case(atom))
}

/// A case-insensitive atom match.
///
/// # Safety
/// `v` is a value of a parsed response or null; `atom` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iv_is(v: *const IVal, atom: *const c_char) -> c_int {
    c_int::from(is(v, unsafe { core::ffi::CStr::from_ptr(atom) }.to_bytes()))
}

/// The number in an atom or a string, or `def`.
///
/// # Safety
/// `v` is a value of a parsed response or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iv_num(v: *const IVal, def: i64) -> i64 {
    let Some(v) = (unsafe { v.as_ref() }) else { return def };
    if (v.r#type != IV_ATOM && v.r#type != IV_STR) || v.n == 0 || v.n > 20 {
        return def;
    }
    let mut x: i64 = 0;
    for &c in bytes(v) {
        if !c.is_ascii_digit() {
            return def;
        }
        x = x.wrapping_mul(10).wrapping_add(i64::from(c - b'0'));
    }
    x
}

/// The n-th sibling.
///
/// # Safety
/// `first` is a value of a parsed response or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iv_at(mut first: *mut IVal, mut idx: c_int) -> *mut IVal {
    while !first.is_null() && idx > 0 {
        idx -= 1;
        first = unsafe { (*first).next };
    }
    first
}

/// # Safety
/// `first` is a value of a parsed response or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iv_count(mut first: *const IVal) -> c_int {
    let mut n = 0;
    while !first.is_null() {
        n += 1;
        first = unsafe { (*first).next };
    }
    n
}

/// In a FETCH list ("UID 5 FLAGS (\Seen) BODY[...] {12}..."), the value after `key`. The key match
/// is case-insensitive; a key with "[" (e.g. "BODY[HEADER") matches by prefix.
///
/// # Safety
/// `list` is a value of a parsed response or null; `key` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imap_fetch_item(list: *mut IVal, key: *const c_char) -> *mut IVal {
    let Some(l) = (unsafe { list.as_ref() }) else { return ptr::null_mut() };
    let key = unsafe { core::ffi::CStr::from_ptr(key) }.to_bytes();
    let mut k = if l.r#type == IV_LIST { l.kid } else { list };
    let prefix = key.contains(&b'[');
    unsafe {
        while !k.is_null() && !(*k).next.is_null() {
            if (*k).r#type != IV_ATOM {
                return ptr::null_mut();
            }
            let hit = if prefix {
                let name = bytes(&*k);
                name.len() >= key.len() && name[..key.len()].eq_ignore_ascii_case(key)
            } else {
                is(k, key)
            };
            if hit {
                return (*k).next;
            }
            k = (*(*k).next).next;
        }
    }
    ptr::null_mut()
}

/// Copies a string or atom value (NIL → "") into `out`; returns the length.
///
/// # Safety
/// `v` is a value of a parsed response or null; `out` covers `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iv_copy(v: *const IVal, out: *mut c_char, cap: usize) -> usize {
    if cap == 0 {
        return 0;
    }
    unsafe {
        *out = 0;
        let Some(v) = v.as_ref() else { return 0 };
        if v.r#type == IV_NIL || v.r#type == IV_LIST {
            return 0;
        }
        let n = v.n.min(cap - 1);
        ptr::copy_nonoverlapping(bytes(v).as_ptr(), out.cast::<u8>(), n);
        *out.add(n) = 0;
        n
    }
}

/// The response code number from an "[CODE n]" atom anywhere in the response (UIDVALIDITY…).
///
/// # Safety
/// `r` was filled by `imap_parse`; `code` is a NUL-terminated string; `out` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imap_code_num(r: *const IResp, code: *const c_char, out: *mut u64) -> c_int {
    unsafe {
        let code = core::ffi::CStr::from_ptr(code).to_bytes();
        let cl = code.len();
        let mut v = (*r).first.cast_const();
        while let Some(val) = v.as_ref() {
            v = val.next;
            let b = bytes(val);
            if val.r#type != IV_ATOM || b.len() < cl + 2 || b[0] != b'[' {
                continue;
            }
            if !b[1..1 + cl].eq_ignore_ascii_case(code) || (b[1 + cl] != b' ' && b[1 + cl] != b']') {
                continue;
            }
            let mut p = 1 + cl;
            while p < b.len() && b[p] == b' ' {
                p += 1;
            }
            let mut x: u64 = 0;
            let mut any = false;
            while p < b.len() && b[p].is_ascii_digit() {
                x = x.wrapping_mul(10).wrapping_add(u64::from(b[p] - b'0'));
                p += 1;
                any = true;
            }
            if !any {
                return 0;
            }
            *out = x;
            return 1;
        }
        0
    }
}

fn quote(s: &[u8], nonsync: bool) -> (Vec<u8>, bool) {
    let lit = s.iter().any(|&c| c == b'\r' || c == b'\n' || c >= 0x80 || c == 0);
    if lit {
        let mut o = format!("{{{}{}}}\r\n", s.len(), if nonsync { "+" } else { "" }).into_bytes();
        o.extend_from_slice(s);
        return (o, true);
    }
    let mut o = Vec::with_capacity(s.len() * 2 + 2);
    o.push(b'"');
    for &c in s {
        if c == b'"' || c == b'\\' {
            o.push(b'\\');
        }
        o.push(c);
    }
    o.push(b'"');
    (o, false)
}

/// An IMAP quoted string for a command, or a literal "{n}\r\n…" when needed. The caller frees it.
/// `nonsync` uses "{n+}" (LITERAL+); `*is_literal` says which one it was.
///
/// # Safety
/// `s` is a NUL-terminated string or null; `is_literal` is writable or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imap_quote(s: *const c_char, nonsync: c_int, is_literal: *mut c_int) -> *mut c_char {
    unsafe {
        let text: &[u8] = if s.is_null() { b"" } else { core::ffi::CStr::from_ptr(s).to_bytes() };
        let (o, lit) = quote(text, nonsync != 0);
        if !is_literal.is_null() {
            *is_literal = c_int::from(lit);
        }
        let out = libc::malloc(o.len() + 1).cast::<u8>();
        if out.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(o.as_ptr(), out, o.len());
        *out.add(o.len()) = 0;
        out.cast()
    }
}

// ---- modified UTF-7 ----

const B64M: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

fn put_utf8(o: &mut [u8], cp: u32) -> usize {
    let mut t = [0u8; 4];
    let k = if cp < 0x80 {
        t[0] = cp as u8;
        1
    } else if cp < 0x800 {
        t[0] = 0xc0 | (cp >> 6) as u8;
        t[1] = 0x80 | (cp & 0x3f) as u8;
        2
    } else if cp < 0x10000 {
        t[0] = 0xe0 | (cp >> 12) as u8;
        t[1] = 0x80 | ((cp >> 6) & 0x3f) as u8;
        t[2] = 0x80 | (cp & 0x3f) as u8;
        3
    } else {
        t[0] = 0xf0 | (cp >> 18) as u8;
        t[1] = 0x80 | ((cp >> 12) & 0x3f) as u8;
        t[2] = 0x80 | ((cp >> 6) & 0x3f) as u8;
        t[3] = 0x80 | (cp & 0x3f) as u8;
        4
    };
    if k > o.len() {
        return 0;
    }
    o[..k].copy_from_slice(&t[..k]);
    k
}

/// Decodes into `out` (`cap` bytes with the NUL).
fn utf7_decode(inp: &[u8], out: &mut [u8]) -> usize {
    let cap = out.len();
    let at = |i: usize| inp.get(i).copied().unwrap_or(0);
    let mut o = 0;
    let mut i = 0;
    while at(i) != 0 && o + 1 < cap {
        if at(i) != b'&' {
            out[o] = at(i);
            o += 1;
            i += 1;
            continue;
        }
        i += 1;
        if at(i) == b'-' {
            out[o] = b'&';
            o += 1;
            i += 1;
            continue;
        }
        let (mut acc, mut bits, mut hi) = (0u32, 0u32, 0u32);
        while at(i) != 0 && at(i) != b'-' {
            let Some(d) = B64M.iter().position(|c| *c == at(i)) else { break };
            acc = (acc << 6) | d as u32;
            bits += 6;
            if bits >= 16 {
                bits -= 16;
                let u = (acc >> bits) & 0xffff;
                if (0xd800..=0xdbff).contains(&u) {
                    hi = u;
                } else {
                    let mut cp = u;
                    if (0xdc00..=0xdfff).contains(&u) && hi != 0 {
                        cp = 0x10000 + ((hi - 0xd800) << 10) + (u - 0xdc00);
                    } else if (0xdc00..=0xdfff).contains(&u) {
                        cp = 0xfffd;
                    }
                    hi = 0;
                    o += put_utf8(&mut out[o..cap - 1], cp);
                }
            }
            i += 1;
        }
        if at(i) == b'-' {
            i += 1;
        }
    }
    out[o] = 0;
    o
}

/// Encodes into `out` (`cap` bytes with the NUL).
fn utf7_encode(s: &[u8], out: &mut [u8]) -> usize {
    let cap = out.len();
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut o = 0;
    let mut i = 0;
    while at(i) != 0 && o + 1 < cap {
        let c = at(i);
        if (0x20..0x7f).contains(&c) {
            if c == b'&' {
                if o + 2 >= cap {
                    break;
                }
                out[o] = b'&';
                out[o + 1] = b'-';
                o += 2;
            } else {
                out[o] = c;
                o += 1;
            }
            i += 1;
            continue;
        }
        if o + 1 >= cap {
            break;
        }
        out[o] = b'&';
        o += 1;
        let (mut acc, mut bits) = (0u32, 0u32);
        while at(i) != 0 && (at(i) < 0x20 || at(i) >= 0x7f) {
            let mut cp = u32::from(at(i));
            let mut k = 1;
            if cp >= 0xf0 && at(i + 1) != 0 && at(i + 2) != 0 && at(i + 3) != 0 {
                cp = ((cp & 7) << 18) | (u32::from(at(i + 1) & 0x3f) << 12) | (u32::from(at(i + 2) & 0x3f) << 6) | u32::from(at(i + 3) & 0x3f);
                k = 4;
            } else if cp >= 0xe0 && at(i + 1) != 0 && at(i + 2) != 0 {
                cp = ((cp & 15) << 12) | (u32::from(at(i + 1) & 0x3f) << 6) | u32::from(at(i + 2) & 0x3f);
                k = 3;
            } else if cp >= 0xc0 && at(i + 1) != 0 {
                cp = ((cp & 31) << 6) | u32::from(at(i + 1) & 0x3f);
                k = 2;
            }
            i += k;
            let mut units = [0u32; 2];
            let mut nu = 0;
            if cp >= 0x10000 {
                cp -= 0x10000;
                units[0] = 0xd800 + (cp >> 10);
                units[1] = 0xdc00 + (cp & 0x3ff);
                nu = 2;
            } else {
                units[0] = cp;
                nu = nu.max(1);
            }
            for &u in &units[..nu] {
                acc = (acc << 16) | u;
                bits += 16;
                while bits >= 6 {
                    bits -= 6;
                    if o + 2 < cap {
                        out[o] = B64M[((acc >> bits) & 63) as usize];
                        o += 1;
                    }
                }
            }
        }
        if bits > 0 && o + 2 < cap {
            out[o] = B64M[((acc << (6 - bits)) & 63) as usize];
            o += 1;
        }
        if o + 1 < cap {
            out[o] = b'-';
            o += 1;
        }
    }
    out[o] = 0;
    o
}

/// Modified UTF-7 (RFC 3501 §5.1.3) mailbox names into UTF-8.
///
/// # Safety
/// `inp` is a NUL-terminated string; `out` covers `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imap_utf7_decode(inp: *const c_char, out: *mut c_char, cap: usize) {
    if cap == 0 {
        return;
    }
    unsafe { utf7_decode(core::ffi::CStr::from_ptr(inp).to_bytes(), slice::from_raw_parts_mut(out.cast(), cap)) };
}

/// UTF-8 into a modified UTF-7 mailbox name.
///
/// # Safety
/// `inp` is a NUL-terminated string; `out` covers `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imap_utf7_encode(inp: *const c_char, out: *mut c_char, cap: usize) {
    if cap == 0 {
        return;
    }
    unsafe { utf7_encode(core::ffi::CStr::from_ptr(inp).to_bytes(), slice::from_raw_parts_mut(out.cast(), cap)) };
}

/// A compact UID set ("1:5,7,9:12") from an ascending array; returns the bytes written.
///
/// # Safety
/// `u` covers `n` values and `out` covers `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imap_uidset(u: *const u32, n: usize, out: *mut c_char, cap: usize) -> usize {
    if cap == 0 {
        return 0;
    }
    let u: &[u32] = if n == 0 { &[] } else { unsafe { slice::from_raw_parts(u, n) } };
    let out = unsafe { slice::from_raw_parts_mut(out.cast::<u8>(), cap) };
    out[0] = 0;
    let mut o = 0;
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && u[j + 1] == u[j].wrapping_add(1) {
            j += 1;
        }
        let sep = if o > 0 { "," } else { "" };
        let t = if j > i { format!("{sep}{}:{}", u[i], u[j]) } else { format!("{sep}{}", u[i]) };
        if o + t.len() + 1 > cap {
            break;
        }
        out[o..o + t.len()].copy_from_slice(t.as_bytes());
        o += t.len();
        out[o] = 0;
        i = j + 1;
    }
    o
}

/// `strtoul(s, &e, 10)`: the value and where it ended (`start` when there are no digits).
fn strtoul10(s: &[u8], start: usize) -> (u64, usize) {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut i = start;
    while matches!(at(i), b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let neg = at(i) == b'-';
    if at(i) == b'+' || neg {
        i += 1;
    }
    if !at(i).is_ascii_digit() {
        return (0, start);
    }
    let mut v: u64 = 0;
    let mut over = false;
    while at(i).is_ascii_digit() {
        match v.checked_mul(10).and_then(|x| x.checked_add(u64::from(at(i) - b'0'))) {
            Some(x) => v = x,
            None => over = true,
        }
        i += 1;
    }
    let v = if over { u64::MAX } else if neg { v.wrapping_neg() } else { v };
    (v, i)
}

fn parse_uidset(s: &[u8], max: usize) -> Vec<u32> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut out = Vec::new();
    let mut i = 0;
    while at(i) != 0 && out.len() < max {
        let (mut a, e) = strtoul10(s, i);
        if e == i {
            while at(i) != 0 && at(i) != b',' {
                i += 1;
            }
            if at(i) != 0 {
                i += 1;
            }
            continue;
        }
        let mut b = a;
        i = e;
        if at(i) == b':' {
            i += 1;
            let (v, e) = strtoul10(s, i);
            if e == i {
                b = a;
                while at(i) != 0 && at(i) != b',' {
                    i += 1;
                }
            } else {
                b = v;
                i = e;
            }
        }
        if b < a {
            core::mem::swap(&mut a, &mut b);
        }
        let mut x = a;
        while x <= b && out.len() < max {
            out.push(x as u32);
            if x == u64::MAX {
                break;
            }
            x += 1;
        }
        if at(i) == b',' {
            i += 1;
        }
    }
    out
}

/// Splits "1:5,7" into the UIDs (ascending, up to `max`; "*" is ignored). Returns the count.
///
/// # Safety
/// `s` is a NUL-terminated string; `out` covers `max` values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imap_parse_uidset(s: *const c_char, out: *mut u32, max: usize) -> usize {
    let list = parse_uidset(unsafe { core::ffi::CStr::from_ptr(s) }.to_bytes(), max);
    for (i, v) in list.iter().enumerate() {
        unsafe { *out.add(i) = *v };
    }
    list.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Option<(IResp, Vec<u8>)> {
        let buf = text.as_bytes().to_vec();
        let mut r = IResp { tag: [0; 32], first: ptr::null_mut(), arena: ptr::null_mut() };
        let rc = unsafe { imap_parse(buf.as_ptr().cast(), buf.len(), &mut r) };
        (rc == 0).then_some((r, buf))
    }
    fn tag(r: &IResp) -> String {
        r.tag.iter().take_while(|c| **c != 0).map(|c| *c as u8 as char).collect()
    }
    fn text(v: *const IVal) -> String {
        let mut out = [0 as c_char; 64];
        let n = unsafe { iv_copy(v, out.as_mut_ptr(), out.len()) };
        out[..n].iter().map(|c| *c as u8 as char).collect()
    }

    #[test]
    fn a_fetch_response_with_a_literal_a_list_and_nil() {
        let (mut r, _buf) = parse("* 12 FETCH (UID 5 FLAGS (\\Seen \\Flagged) ENVELOPE (NIL \"a \\\"b\\\"\") BODY[HEADER] {5}\r\nhello)\r\n").unwrap();
        unsafe {
            assert_eq!(tag(&r), "*");
            assert_eq!(iv_count(r.first), 3);
            assert_eq!(iv_num(r.first, -1), 12);
            let list = iv_at(r.first, 2);
            assert_eq!((*list).r#type, IV_LIST);
            let uid = imap_fetch_item(list, c"uid".as_ptr());
            assert_eq!(iv_num(uid, -1), 5);
            let flags = imap_fetch_item(list, c"FLAGS".as_ptr());
            assert_eq!(iv_count((*flags).kid), 2);
            let env = imap_fetch_item(list, c"ENVELOPE".as_ptr());
            assert_eq!((*(*env).kid).r#type, IV_NIL);
            assert_eq!(text((*(*env).kid).next), "a \"b\"");
            assert_eq!(text(imap_fetch_item(list, c"BODY[".as_ptr())), "hello", "a key with [ matches by prefix");
            assert!(imap_fetch_item(list, c"MISSING".as_ptr()).is_null());
            imap_resp_free(&mut r);
            assert!(r.arena.is_null());
        }
    }
    #[test]
    fn response_codes_carry_their_numbers() {
        let (mut r, _b) = parse("* OK [UIDVALIDITY 3857529045] UIDs valid\r\n").unwrap();
        let mut n = 0u64;
        unsafe {
            assert_eq!(imap_code_num(&r, c"uidvalidity".as_ptr(), &mut n), 1);
            assert_eq!(n, 3_857_529_045);
            assert_eq!(imap_code_num(&r, c"UIDNEXT".as_ptr(), &mut n), 0);
            imap_resp_free(&mut r);
        }
    }
    #[test]
    fn malformed_responses_are_refused() {
        for bad in ["* (1 2\r\n", "* 1 2)\r\n", "* {99}\r\nshort", "* {x}\r\n", "* \"open", "* BODY[open\r\n", "", " * 1"] {
            assert!(parse(bad).is_none(), "{bad:?}");
        }
        let deep = format!("* {}{}\r\n", "(".repeat(40), ")".repeat(40));
        assert!(parse(&deep).is_none(), "deeper than IMAP_MAX_NEST");
        let ok = format!("* {}{}\r\n", "(".repeat(30), ")".repeat(30));
        assert!(parse(&ok).is_some());
    }
    #[test]
    fn quoting_uses_a_literal_only_when_it_has_to() {
        unsafe {
            let mut lit = 0;
            let q = imap_quote(c"a \"b\" \\c".as_ptr(), 0, &mut lit);
            assert_eq!((core::ffi::CStr::from_ptr(q).to_bytes(), lit), (&b"\"a \\\"b\\\" \\\\c\""[..], 0));
            libc::free(q.cast());
            let q = imap_quote(c"caf\xc3\xa9".as_ptr(), 1, &mut lit);
            assert_eq!((core::ffi::CStr::from_ptr(q).to_bytes(), lit), (&b"{5+}\r\ncaf\xc3\xa9"[..], 1));
            libc::free(q.cast());
        }
    }
    #[test]
    fn mailbox_names_round_trip_through_modified_utf7() {
        let mut enc = [0u8; 64];
        let mut dec = [0u8; 64];
        for name in ["INBOX", "Entwürfe", "日本語", "A&B", "😀 smile", "Sent/Débiles"] {
            let c = std::ffi::CString::new(name).unwrap();
            unsafe {
                imap_utf7_encode(c.as_ptr(), enc.as_mut_ptr().cast(), enc.len());
                imap_utf7_decode(enc.as_ptr().cast(), dec.as_mut_ptr().cast(), dec.len());
            }
            let n = dec.iter().position(|b| *b == 0).unwrap();
            assert_eq!(std::str::from_utf8(&dec[..n]).unwrap(), name);
        }
        unsafe { imap_utf7_encode(c"Entw\xc3\xbcrfe".as_ptr(), enc.as_mut_ptr().cast(), enc.len()) };
        assert_eq!(&enc[..10], b"Entw&APw-r");
    }
    #[test]
    fn uid_sets_compact_and_expand() {
        let mut out = [0 as c_char; 64];
        let n = unsafe { imap_uidset([1u32, 2, 3, 4, 5, 7, 9, 10, 11, 12].as_ptr(), 10, out.as_mut_ptr(), out.len()) };
        assert_eq!(unsafe { core::ffi::CStr::from_ptr(out.as_ptr()) }.to_str().unwrap(), "1:5,7,9:12");
        assert_eq!(n, 10);
        assert_eq!(parse_uidset(b"1:3,7,9:8,*,x,5:", 100), [1, 2, 3, 7, 8, 9, 5]);
        assert_eq!(parse_uidset(b"1:100000", 4), [1, 2, 3, 4], "up to max");
    }
}
