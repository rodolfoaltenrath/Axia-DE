//! Mail messages (RFC 5322, MIME 2045-2049, 2047, 2231) and character sets.
//! See server/src/mail/mime.h for the contract.

use crate::c::*;
use crate::charset_tables::{CS_TABLES, CsTable};
use core::ffi::{c_char, c_int};

pub const MIME_MAX_PARTS: usize = 256;
pub const MIME_MAX_DEPTH: c_int = 16;

#[repr(C)]
pub struct MimePart {
    pub parent: c_int,
    pub depth: c_int,
    pub type_: [c_char; 48],
    pub subtype: [c_char; 64],
    pub charset: [c_char; 40],
    pub boundary: [c_char; 128],
    pub encoding: [c_char; 24],
    pub disposition: [c_char; 16],
    pub filename: [c_char; 256],
    pub cid: [c_char; 160],
    pub method: [c_char; 24],
    pub hdr: *const c_char,
    pub hlen: usize,
    pub body: *const c_char,
    pub blen: usize,
}

#[repr(C)]
pub struct MimeMsg {
    pub parts: [MimePart; MIME_MAX_PARTS],
    pub nparts: c_int,
    pub truncated: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MimeAddr {
    pub name: [c_char; 160],
    pub email: [c_char; 256],
}

fn ws4(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

/// copy_trim: trims space/tab/CR/LF and copies at most cap-1 bytes.
fn copy_trim(out: &mut [u8], s: &[u8]) {
    if out.is_empty() {
        return;
    }
    let mut a = 0;
    let mut b = s.len();
    while a < b && ws4(s[a]) {
        a += 1;
    }
    while b > a && ws4(s[b - 1]) {
        b -= 1;
    }
    let n = (b - a).min(out.len() - 1);
    out[..n].copy_from_slice(&s[a..a + n]);
    out[n] = 0;
}

fn trimmed(s: &[u8], cap: usize) -> Vec<u8> {
    let mut t = vec![0u8; cap];
    copy_trim(&mut t, s);
    z(&t).to_vec()
}

fn lower_c(a: &mut [c_char]) {
    for c in a.iter_mut() {
        if *c == 0 {
            break;
        }
        *c = lower(*c as u8) as c_char;
    }
}
fn upper_c(a: &mut [c_char]) {
    for c in a.iter_mut() {
        if *c == 0 {
            break;
        }
        *c = upper(*c as u8) as c_char;
    }
}

/// malloc result for an owned buffer, setting *olen.
fn done(v: Vec<u8>, olen: *mut usize) -> *mut c_char {
    let p = malloc_str(&v);
    set_len(olen, if p.is_null() { 0 } else { v.len() });
    p
}

// ---- UTF-8 ----

pub(crate) fn utf8_clean_vec(s: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let k = utf8_len(&s[i..]);
        if k == 0 {
            o.extend_from_slice(b"\xef\xbf\xbd");
            i += 1;
            continue;
        }
        if k == 1 && s[i] == 0 {
            i += 1;
            continue;
        }
        o.extend_from_slice(&s[i..i + k]);
        i += k;
    }
    o
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn utf8_clean(
    input: *const c_char,
    len: usize,
    olen: *mut usize,
) -> *mut c_char {
    done(utf8_clean_vec(unsafe { bytes(input, len) }), olen)
}

// ---- character sets ----

fn cs_norm(cs: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &c in cs {
        if out.len() + 1 >= 64 {
            break;
        }
        if matches!(c, b'"' | b'\'' | b'-' | b'_' | b' ' | b'.') {
            continue;
        }
        if c == b'*' {
            break;
        }
        out.push(lower(c));
    }
    let fit = |mut v: Vec<u8>| {
        v.truncate(63);
        v
    };
    if out.starts_with(b"cp125") {
        fit([b"windows".as_slice(), &out[2..]].concat())
    } else if out.starts_with(b"xcp125") {
        fit([b"windows".as_slice(), &out[3..]].concat())
    } else if out == b"cp850" {
        b"ibm850".to_vec()
    } else if out == b"cp437" {
        b"ibm437".to_vec()
    } else if out == b"mac" || out == b"macroman" || out == b"xmacroman" {
        b"macintosh".to_vec()
    } else {
        out
    }
}

fn cs_table(norm: &[u8]) -> Option<&'static CsTable> {
    CS_TABLES
        .iter()
        .find(|t| cs_norm(t.name) == norm || (!t.alias.is_empty() && cs_norm(t.alias) == norm))
}

fn windows1252() -> Option<&'static [u16; 128]> {
    cs_table(b"windows1252").map(|t| &t.hi)
}

fn is_utf8_name(n: &[u8]) -> bool {
    n == b"utf8" || n == b"unicode11utf8" || n == b"xunicode20utf8"
}
fn is_ascii_name(n: &[u8]) -> bool {
    n.is_empty() || n == b"usascii" || n == b"ascii" || n == b"ansix341968" || n == b"us"
}
fn is_latin1_name(n: &[u8]) -> bool {
    n == b"iso88591" || n == b"latin1" || n == b"l1" || n == b"iso885911987" || n == b"isoir100"
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn charset_known(cs: *const c_char) -> c_int {
    let n = cs_norm(unsafe { crate::c::cs(cs) });
    (is_utf8_name(&n)
        || is_ascii_name(&n)
        || is_latin1_name(&n)
        || cs_table(&n).is_some()
        || n.starts_with(b"utf16")
        || n.starts_with(b"utf32")) as c_int
}

fn from_table(hi: Option<&[u16; 128]>, s: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len());
    for &c in s {
        if c == 0 {
            continue;
        }
        let cp = if c < 0x80 {
            c as u32
        } else {
            hi.map_or(c as u32, |h| h[(c - 0x80) as usize] as u32)
        };
        put_cp(&mut o, cp);
    }
    o
}

fn from_utf16(s: &[u8], be: bool, wide: bool) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len());
    let step = if wide { 4 } else { 2 };
    let mut i = 0;
    while i + step <= s.len() {
        let u = &s[i..];
        let at0 = i == 0;
        let mut cp;
        if wide {
            cp = if be {
                u32::from_be_bytes([u[0], u[1], u[2], u[3]])
            } else {
                u32::from_le_bytes([u[0], u[1], u[2], u[3]])
            };
        } else {
            cp = if be {
                u16::from_be_bytes([u[0], u[1]])
            } else {
                u16::from_le_bytes([u[0], u[1]])
            } as u32;
            if (0xd800..=0xdbff).contains(&cp) && i + 4 <= s.len() {
                let lo = if be {
                    u16::from_be_bytes([u[2], u[3]])
                } else {
                    u16::from_le_bytes([u[2], u[3]])
                } as u32;
                if (0xdc00..=0xdfff).contains(&lo) {
                    cp = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00);
                    i += 2;
                } else {
                    cp = 0xfffd;
                }
            } else if (0xd800..=0xdfff).contains(&cp) {
                cp = 0xfffd;
            }
        }
        i += step;
        if cp == 0xfeff && at0 {
            continue;
        }
        if cp == 0 {
            continue;
        }
        if cp > 0x10ffff || (0xd800..=0xdfff).contains(&cp) {
            cp = 0xfffd;
        }
        put_cp(&mut o, cp);
    }
    o
}

pub(crate) fn to_utf8(cs: &[u8], s: &[u8]) -> Vec<u8> {
    let n = cs_norm(cs);
    if is_utf8_name(&n) {
        return utf8_clean_vec(s);
    }
    if is_ascii_name(&n) {
        // "us-ascii" with 8-bit bytes: almost always mislabeled UTF-8 or windows-1252
        if utf8_valid(s) {
            return utf8_clean_vec(s);
        }
        return from_table(windows1252(), s);
    }
    if is_latin1_name(&n) {
        return from_table(None, s);
    }
    if let Some(t) = cs_table(&n) {
        return from_table(Some(&t.hi), s);
    }
    if n.starts_with(b"utf16") || n.starts_with(b"utf32") {
        let wide = n.get(3) == Some(&b'3');
        let suf = &n[5..];
        let be = if suf == b"le" {
            false
        } else if suf == b"be" {
            true
        } else {
            !(s.len() >= 2 && s[0] == 0xff && s[1] == 0xfe)
        };
        return from_utf16(s, be, wide);
    }
    utf8_clean_vec(s)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn charset_to_utf8(
    cs: *const c_char,
    input: *const c_char,
    len: usize,
    olen: *mut usize,
) -> *mut c_char {
    done(
        to_utf8(unsafe { crate::c::cs(cs) }, unsafe { bytes(input, len) }),
        olen,
    )
}

// ---- base64 and quoted-printable ----

fn b64v(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' | b'-' => 62,
        b'/' | b'_' => 63,
        _ => return None,
    } as u32)
}

pub(crate) fn b64_dec(s: &[u8], out: &mut Vec<u8>) {
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in s {
        if c == b'=' {
            break;
        }
        let Some(v) = b64v(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1u32 << bits) - 1;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn b64_decode(input: *const c_char, len: usize, out: *mut u8) -> usize {
    let mut v = Vec::new();
    b64_dec(unsafe { bytes(input, len) }, &mut v);
    if !v.is_empty() {
        unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len()) }
    }
    v.len()
}

pub(crate) fn b64_enc(s: &[u8], wrap: bool) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = Vec::with_capacity(s.len() * 4 / 3 + 8);
    let mut col = 0;
    let mut i = 0;
    while i < s.len() {
        let mut v = (s[i] as u32) << 16;
        if i + 1 < s.len() {
            v |= (s[i + 1] as u32) << 8;
        }
        if i + 2 < s.len() {
            v |= s[i + 2] as u32;
        }
        o.push(T[((v >> 18) & 63) as usize]);
        o.push(T[((v >> 12) & 63) as usize]);
        o.push(if i + 1 < s.len() {
            T[((v >> 6) & 63) as usize]
        } else {
            b'='
        });
        o.push(if i + 2 < s.len() {
            T[(v & 63) as usize]
        } else {
            b'='
        });
        col += 4;
        if wrap && col >= 76 {
            o.extend_from_slice(b"\r\n");
            col = 0;
        }
        i += 3;
    }
    if wrap && col != 0 {
        o.extend_from_slice(b"\r\n");
    }
    o
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn b64_encode(
    input: *const u8,
    len: usize,
    wrap: c_int,
    olen: *mut usize,
) -> *mut c_char {
    done(
        b64_enc(unsafe { bytes(input.cast(), len) }, wrap != 0),
        olen,
    )
}

fn hexv(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn qp_dec(s: &[u8], header: bool, out: &mut Vec<u8>) {
    let len = s.len();
    let mut i = 0;
    while i < len {
        let c = s[i];
        if header && c == b'_' {
            out.push(b' ');
            i += 1;
            continue;
        }
        if c != b'=' {
            out.push(c);
            i += 1;
            continue;
        }
        if i + 1 < len && s[i + 1] == b'\n' {
            i += 2;
            continue;
        }
        if i + 2 < len && s[i + 1] == b'\r' && s[i + 2] == b'\n' {
            i += 3;
            continue;
        }
        // "=" with whitespace before the line break (transport padding)
        let mut k = i + 1;
        while k < len && (s[k] == b' ' || s[k] == b'\t') {
            k += 1;
        }
        if k < len && k > i + 1 && (s[k] == b'\n' || s[k] == b'\r') {
            i = if s[k] == b'\r' && k + 1 < len && s[k + 1] == b'\n' {
                k + 1
            } else {
                k
            };
            i += 1;
            continue;
        }
        if i + 2 < len {
            if let (Some(h), Some(l)) = (hexv(s[i + 1]), hexv(s[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b'=');
        i += 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn qp_decode(
    input: *const c_char,
    len: usize,
    out: *mut c_char,
    header: c_int,
) -> usize {
    let mut v = Vec::new();
    qp_dec(unsafe { bytes(input, len) }, header != 0, &mut v);
    if !v.is_empty() {
        unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out.cast::<u8>(), v.len()) }
    }
    v.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn qp_encode(
    input: *const c_char,
    len: usize,
    olen: *mut usize,
) -> *mut c_char {
    const HX: &[u8; 16] = b"0123456789ABCDEF";
    let s = unsafe { bytes(input, len) };
    let mut o = Vec::with_capacity(s.len() + 16);
    let mut col = 0usize;
    for i in 0..s.len() {
        let c = s[i];
        if c == b'\r' && i + 1 < s.len() && s[i + 1] == b'\n' {
            continue;
        }
        if c == b'\n' {
            o.extend_from_slice(b"\r\n");
            col = 0;
            continue;
        }
        let at_eol = i + 1 >= s.len() || s[i + 1] == b'\n' || s[i + 1] == b'\r';
        let mut enc: &[u8] = &[c];
        let esc = [b'=', HX[(c >> 4) as usize], HX[(c & 15) as usize]];
        if !(((33..=126).contains(&c) && c != b'=') || ((c == b' ' || c == b'\t') && !at_eol)) {
            enc = &esc;
        }
        // "." at the start of a line is also encoded (some servers mangle it)
        if col == 0 && c == b'.' {
            enc = b"=2E";
        }
        if col + enc.len() > 75 {
            o.extend_from_slice(b"=\r\n");
            col = 0;
        }
        o.extend_from_slice(enc);
        col += enc.len();
    }
    done(o, olen)
}

// ---- headers ----

pub(crate) fn split(m: &[u8]) -> (usize, usize) {
    // (hlen, body offset); the body runs to the end
    let len = m.len();
    if len == 0 {
        return (0, 0);
    }
    if m[0] == b'\n' {
        return (0, 1);
    }
    if len >= 2 && m[0] == b'\r' && m[1] == b'\n' {
        return (0, 2);
    }
    for i in 0..len - 1 {
        if m[i] != b'\n' {
            continue;
        }
        if m[i + 1] == b'\n' {
            return (i + 1, i + 2);
        }
        if m[i + 1] == b'\r' && i + 2 < len && m[i + 2] == b'\n' {
            return (i + 1, i + 3);
        }
    }
    (len, len)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_split(
    m: *const c_char,
    len: usize,
    hdr: *mut *const c_char,
    hlen: *mut usize,
    body: *mut *const c_char,
    blen: *mut usize,
) {
    let (hl, b) = split(unsafe { bytes(m, len) });
    unsafe {
        *hdr = m;
        *hlen = hl;
        *body = if m.is_null() { m } else { m.add(b) };
        *blen = len - b.min(len);
    }
}

/// The unfolded raw value of the first `name:` header line, or None.
fn header_raw(h: &[u8], name: &[u8]) -> Option<Vec<u8>> {
    let n = h.len();
    let nl = name.len();
    let mut i = 0;
    while i < n {
        let mut e = i;
        while e < n && h[e] != b'\n' {
            e += 1;
        }
        if e - i > nl
            && h[i] != b' '
            && h[i] != b'\t'
            && h[i + nl] == b':'
            && eq_ic(&h[i..i + nl], name)
        {
            let mut v = i + nl + 1;
            let mut out = Vec::new();
            loop {
                let mut le = e;
                if le > v && h[le - 1] == b'\r' {
                    le -= 1;
                }
                if v < le {
                    out.extend_from_slice(&h[v..le]);
                }
                if e + 1 < n && (h[e + 1] == b' ' || h[e + 1] == b'\t') {
                    v = e + 1;
                    e = v;
                    while e < n && h[e] != b'\n' {
                        e += 1;
                    }
                    continue;
                }
                break;
            }
            return Some(out);
        }
        i = e + 1;
    }
    None
}

/// mime_header into a fixed buffer: raw bytes (embedded NULs included), trimmed like the C code.
fn header_into(h: &[u8], name: &[u8], out: &mut [u8]) -> bool {
    let cap = out.len();
    let Some(v) = header_raw(h, name) else {
        if cap != 0 {
            out[0] = 0;
        }
        return false;
    };
    if cap == 0 {
        return true;
    }
    let mut o = v.len().min(cap - 1);
    out[..o].copy_from_slice(&v[..o]);
    out[o] = 0;
    let mut s = 0;
    while out[s] == b' ' || out[s] == b'\t' {
        s += 1;
    }
    if s != 0 {
        out.copy_within(s..o + 1, 0);
        o -= s;
    }
    while o > 0 && (out[o - 1] == b' ' || out[o - 1] == b'\t') {
        o -= 1;
        out[o] = 0;
    }
    true
}

/// mime_header with a cap, as an owned C-string value.
pub(crate) fn header(h: &[u8], name: &[u8], cap: usize) -> Option<Vec<u8>> {
    let mut b = vec![0u8; cap];
    if header_into(h, name, &mut b) {
        Some(z(&b).to_vec())
    } else {
        None
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_header(
    hdr: *const c_char,
    hlen: usize,
    name: *const c_char,
    out: *mut c_char,
    cap: usize,
) -> c_int {
    let (h, name, out) = unsafe { (bytes(hdr, hlen), cs(name), crate::c::out(out, cap)) };
    header_into(h, name, out) as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_header_dup(
    hdr: *const c_char,
    hlen: usize,
    name: *const c_char,
) -> *mut c_char {
    let Some(v) = header_raw(unsafe { bytes(hdr, hlen) }, unsafe { cs(name) }) else {
        return std::ptr::null_mut();
    };
    let v = z(&v);
    let s = v
        .iter()
        .position(|&c| c != b' ' && c != b'\t')
        .unwrap_or(v.len());
    malloc_str(&v[s..])
}

/// Raw header text (without encoded words) to UTF-8.
fn raw_header_utf8(b: &mut Vec<u8>, s: &[u8]) {
    if utf8_valid(s) {
        b.extend_from_slice(s);
    } else {
        b.extend_from_slice(&from_table(windows1252(), s));
    }
}

/// mime_decode_words into a buffer of `cap` bytes; returns the C string.
pub(crate) fn decode_words(input: &[u8], cap: usize) -> Vec<u8> {
    let mut out = vec![0u8; cap];
    decode_words_into(input, &mut out);
    z(&out).to_vec()
}

fn decode_words_into(inp: &[u8], out: &mut [u8]) {
    let cap = out.len();
    if cap == 0 {
        return;
    }
    let ws = |c: u8| ws4(c);
    let n = inp.len();
    let mut b: Vec<u8> = Vec::with_capacity(n);
    let (mut i, mut lit) = (0usize, 0usize);
    let mut last_was_word = false;
    while i < n {
        if inp[i] == b'=' && i + 1 < n && inp[i + 1] == b'?' {
            let mut q1 = i + 2;
            while q1 < n && inp[q1] != b'?' && q1 - i < 70 {
                q1 += 1;
            }
            if q1 + 3 < n && inp[q1] == b'?' && inp[q1 + 2] == b'?' {
                let enc = lower(inp[q1 + 1]);
                let t = q1 + 3;
                let mut te = t;
                while te + 1 < n && !(inp[te] == b'?' && inp[te + 1] == b'=') {
                    te += 1;
                }
                if (enc == b'b' || enc == b'q') && te + 1 < n {
                    // whitespace between two encoded words is ignored
                    let only_ws = inp[lit..i].iter().all(|&c| ws(c));
                    if !(last_was_word && only_ws) {
                        raw_header_utf8(&mut b, &inp[lit..i]);
                    }
                    let cs = &inp[i + 2..q1];
                    let cs = &cs[..cs.len().min(63)];
                    let mut raw = Vec::new();
                    if enc == b'b' {
                        b64_dec(&inp[t..te], &mut raw)
                    } else {
                        qp_dec(&inp[t..te], true, &mut raw)
                    }
                    b.extend_from_slice(&to_utf8(cs, &raw));
                    i = te + 2;
                    lit = i;
                    last_was_word = true;
                    continue;
                }
            }
        }
        i += 1;
        if !ws(inp[i - 1]) {
            last_was_word = false;
        }
    }
    if lit < n {
        raw_header_utf8(&mut b, &inp[lit..n]);
    }
    let mut o = 0;
    for &c in z(&b) {
        if o + 1 >= cap {
            break;
        }
        if c < 0x20 || c == 0x7f {
            if o != 0 && out[o - 1] != b' ' {
                out[o] = b' ';
                o += 1;
            }
            continue;
        }
        out[o] = c;
        o += 1;
    }
    out[o] = 0;
    // trimmed (spaces only)
    let s = out[..o].iter().position(|&c| c != b' ').unwrap_or(o);
    if s != 0 {
        out.copy_within(s..o + 1, 0);
        o -= s;
    }
    while o > 0 && out[o - 1] == b' ' {
        o -= 1;
        out[o] = 0;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_decode_words(input: *const c_char, out: *mut c_char, cap: usize) {
    decode_words_into(unsafe { cs(input) }, unsafe { crate::c::out(out, cap) });
}

/// RFC 2231: %XX
fn pct_decode(b: &mut Vec<u8>, s: &[u8]) {
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' && i + 2 < s.len() {
            if let (Some(h), Some(l)) = (hexv(s[i + 1]), hexv(s[i + 2])) {
                b.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        b.push(s[i]);
        i += 1;
    }
}

fn param_into(value: &[u8], name: &[u8], out: &mut [u8]) -> bool {
    if out.is_empty() {
        return false;
    }
    out[0] = 0;
    let p = value;
    let nl = name.len();
    let at = |k: usize| p.get(k).copied().unwrap_or(0);
    let mut k = 0;
    // skip the main value
    let mut inq = false;
    while at(k) != 0 && (inq || at(k) != b';') {
        if at(k) == b'"' {
            inq = !inq;
        } else if at(k) == b'\\' && inq && at(k + 1) != 0 {
            k += 1;
        }
        k += 1;
    }
    let mut segs: Vec<(usize, bool, Vec<u8>)> = Vec::new();
    let mut plain: Option<Vec<u8>> = None;
    while at(k) == b';' {
        k += 1;
        while ws4(at(k)) {
            k += 1;
        }
        let ka = k;
        while at(k) != 0 && at(k) != b'=' && at(k) != b';' {
            k += 1;
        }
        let mut ke = k;
        while ke > ka && (p[ke - 1] == b' ' || p[ke - 1] == b'\t') {
            ke -= 1;
        }
        if at(k) != b'=' {
            continue;
        }
        k += 1;
        while at(k) == b' ' || at(k) == b'\t' {
            k += 1;
        }
        let mut val = Vec::new();
        if at(k) == b'"' {
            k += 1;
            while at(k) != 0 && at(k) != b'"' {
                if at(k) == b'\\' && at(k + 1) != 0 {
                    k += 1;
                }
                val.push(at(k));
                k += 1;
            }
            if at(k) == b'"' {
                k += 1;
            }
            while at(k) != 0 && at(k) != b';' {
                k += 1;
            }
        } else {
            let va = k;
            while at(k) != 0 && at(k) != b';' {
                k += 1;
            }
            let mut ve = k;
            while ve > va && ws4(p[ve - 1]) {
                ve -= 1;
            }
            val.extend_from_slice(&p[va..ve]);
        }
        let kl = ke - ka;
        let key = &p[ka..ke];
        if kl == nl && eq_ic(key, name) {
            val.truncate(1023);
            plain = Some(val);
        } else if kl > nl && eq_ic(&key[..nl], name) && key[nl] == b'*' {
            let r = &key[nl + 1..];
            let (mut idx, mut ext) = (0usize, false);
            if r.is_empty() {
                ext = true;
            } else {
                let mut j = 0;
                while j < r.len() && is_digit(r[j]) {
                    idx = idx * 10 + (r[j] - b'0') as usize;
                    j += 1;
                    if idx > 1000 {
                        break;
                    }
                }
                if j < r.len() && r[j] == b'*' {
                    ext = true;
                }
            }
            if segs.len() < 32 && idx < 32 {
                segs.push((idx, ext, val));
            }
        }
    }
    if !segs.is_empty() {
        let mut raw = Vec::new();
        let mut cs: Vec<u8> = b"us-ascii".to_vec();
        for want in 0..32 {
            let Some(seg) = segs.iter().find(|s| s.0 == want) else {
                break;
            };
            let mut v: &[u8] = &seg.2;
            if seg.1 {
                if want == 0 {
                    if let Some(q1) = chr(v, b'\'') {
                        if let Some(q2) = chr(&v[q1 + 1..], b'\'') {
                            if q1 != 0 && q1 < 64 {
                                cs = v[..q1].to_vec();
                            }
                            v = &v[q1 + 1 + q2 + 1..];
                        }
                    }
                }
                pct_decode(&mut raw, v);
            } else {
                raw.extend_from_slice(v);
            }
        }
        let mut u = to_utf8(&cs, &raw);
        u.truncate(1023);
        decode_words_into(z(&u), out);
        return true;
    }
    if let Some(v) = plain {
        decode_words_into(&v, out);
        return true;
    }
    false
}

pub(crate) fn param(value: &[u8], name: &[u8], cap: usize) -> Option<Vec<u8>> {
    let mut b = vec![0u8; cap];
    if param_into(value, name, &mut b) {
        Some(z(&b).to_vec())
    } else {
        None
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_param(
    value: *const c_char,
    name: *const c_char,
    out: *mut c_char,
    cap: usize,
) -> c_int {
    let o = unsafe { crate::c::out(out, cap) };
    if o.is_empty() {
        return 0;
    }
    o[0] = 0;
    if value.is_null() {
        return 0;
    }
    param_into(unsafe { cs(value) }, unsafe { cs(name) }, o) as c_int
}

// ---- parts ----

fn parse_ctype(h: &[u8], p: &mut MimePart, parent: Option<(&[u8], &[u8])>) {
    putc(&mut p.type_, b"text");
    putc(&mut p.subtype, b"plain");
    if let Some((t, s)) = parent {
        if t == b"multipart" && s == b"digest" {
            putc(&mut p.type_, b"message");
            putc(&mut p.subtype, b"rfc822");
        }
    }
    if let Some(ct) = header(h, b"Content-Type", 1024) {
        let k = cspn(&ct, b";");
        let main = lower_all(&trimmed(&ct[..k], 160));
        if let Some(sl) = chr(&main, b'/') {
            if sl != 0 && sl + 1 < main.len() {
                copy_trim(zc_mut(&mut p.type_), &main[..sl]);
                copy_trim(zc_mut(&mut p.subtype), &main[sl + 1..]);
            }
        }
        if let Some(v) = param(&ct, b"charset", 256) {
            putc(&mut p.charset, &v);
            lower_c(&mut p.charset);
        }
        if let Some(v) = param(&ct, b"boundary", 256) {
            putc(&mut p.boundary, &v);
        }
        if let Some(v) = param(&ct, b"name", 256) {
            putc(&mut p.filename, &v);
        }
        if let Some(v) = param(&ct, b"method", 256) {
            putc(&mut p.method, &v);
            upper_c(&mut p.method);
        }
    }
    if let Some(v) = header(h, b"Content-Transfer-Encoding", 256) {
        copy_trim(zc_mut(&mut p.encoding), &v);
        lower_c(&mut p.encoding);
    }
    if let Some(cd) = header(h, b"Content-Disposition", 1024) {
        let k = cspn(&cd, b";");
        copy_trim(zc_mut(&mut p.disposition), &cd[..k]);
        lower_c(&mut p.disposition);
        if let Some(v) = param(&cd, b"filename", 256) {
            if !v.is_empty() {
                putc(&mut p.filename, &v);
            }
        }
    }
    if let Some(v) = header(h, b"Content-ID", 256) {
        let a = v
            .iter()
            .position(|&c| c != b' ' && c != b'<')
            .unwrap_or(v.len());
        let mut l = v.len();
        while l > a && (v[l - 1] == b'>' || v[l - 1] == b' ') {
            l -= 1;
        }
        copy_trim(zc_mut(&mut p.cid), &v[a..l]);
    }
    // file names: decoded first (RFC 2231/2047 above), then reduced to a safe basename
    let f = zc(&p.filename).to_vec();
    if f.is_empty() {
        putc(&mut p.filename, b"");
    } else {
        putc(&mut p.filename, &safe_filename(&f, 256));
    }
}

fn find_boundary(b: &[u8], from: usize, bnd: &[u8]) -> Option<usize> {
    let bl = bnd.len();
    let mut i = from;
    while i + 2 + bl <= b.len() {
        if (i == 0 || b[i - 1] == b'\n')
            && b[i] == b'-'
            && b[i + 1] == b'-'
            && &b[i + 2..i + 2 + bl] == bnd
        {
            return Some(i);
        }
        i += chr(&b[i..], b'\n')? + 1;
    }
    None
}

fn parse_part(m: &mut MimeMsg, data: &[u8], parent: c_int, depth: c_int) {
    if m.nparts as usize >= MIME_MAX_PARTS {
        m.truncated = 1;
        return;
    }
    let idx = m.nparts as usize;
    m.nparts += 1;
    let (hl, bo) = split(data);
    let par = if parent >= 0 {
        let pp = &m.parts[parent as usize];
        Some((zc(&pp.type_).to_vec(), zc(&pp.subtype).to_vec()))
    } else {
        None
    };
    {
        let p = &mut m.parts[idx];
        // SAFETY: MimePart is plain data; all-zero is its C initial state.
        *p = unsafe { core::mem::zeroed() };
        p.parent = parent;
        p.depth = depth;
        p.hdr = data.as_ptr().cast();
        p.hlen = hl;
        p.body = data[bo..].as_ptr().cast();
        p.blen = data.len() - bo;
        parse_ctype(
            &data[..hl],
            p,
            par.as_ref().map(|(a, b)| (a.as_slice(), b.as_slice())),
        );
    }
    let body = &data[bo..];
    let p = &m.parts[idx];
    let (ty, sub, enc, disp) = (
        zc(&p.type_),
        zc(&p.subtype),
        zc(&p.encoding),
        zc(&p.disposition),
    );
    if ty == b"multipart" && p.boundary[0] != 0 && depth + 1 < MIME_MAX_DEPTH {
        let bnd = zc(&p.boundary).to_vec();
        let bl = bnd.len();
        let blen = body.len();
        let mut pos = find_boundary(body, 0, &bnd);
        while let Some(at) = pos {
            let after = at + 2 + bl;
            if after + 2 <= blen && body[after] == b'-' && body[after + 1] == b'-' {
                break;
            } // closing
            let Some(nl) = chr(&body[after..], b'\n') else {
                break;
            };
            let start = after + nl + 1;
            let next = find_boundary(body, start, &bnd);
            let mut end = next.unwrap_or(blen);
            // the line break before the delimiter belongs to the delimiter
            if end > start && body[end - 1] == b'\n' {
                end -= 1;
            }
            if end > start && body[end - 1] == b'\r' {
                end -= 1;
            }
            if end < start {
                end = start;
            }
            parse_part(m, &body[start..end], idx as c_int, depth + 1);
            if next.is_none() || m.nparts as usize >= MIME_MAX_PARTS {
                break;
            }
            pos = next;
        }
    } else if ty == b"message"
        && sub == b"rfc822"
        && disp != b"attachment"
        && (enc.is_empty() || enc == b"7bit" || enc == b"8bit" || enc == b"binary")
        && depth + 1 < MIME_MAX_DEPTH
    {
        parse_part(m, body, idx as c_int, depth + 1);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_parse(msg: *const c_char, len: usize, out: *mut MimeMsg) {
    let m = unsafe { &mut *out };
    m.nparts = 0;
    m.truncated = 0;
    // The parts point into the caller's buffer, including an empty one.
    let data: &[u8] = if msg.is_null() {
        b""
    } else {
        unsafe { std::slice::from_raw_parts(msg.cast::<u8>(), len) }
    };
    parse_part(m, data, -1, 0);
}

pub(crate) fn decode_body(p: &MimePart) -> Vec<u8> {
    let body = unsafe { bytes(p.body, p.blen) };
    let enc = zc(&p.encoding);
    let mut out = Vec::with_capacity(body.len());
    if enc == b"base64" {
        b64_dec(body, &mut out);
    } else if enc == b"quoted-printable" {
        qp_dec(body, false, &mut out);
    } else {
        out.extend_from_slice(body);
    }
    out
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_decode_body(p: *const MimePart, olen: *mut usize) -> *mut c_char {
    let v = decode_body(unsafe { &*p });
    // the C version allocates blen + 4 and NUL-terminates; a copy of the decoded bytes is equivalent
    done(v, olen)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_part_text(p: *const MimePart, olen: *mut usize) -> *mut c_char {
    let p = unsafe { &*p };
    let raw = decode_body(p);
    let cs = zc(&p.charset);
    done(
        to_utf8(if cs.is_empty() { b"us-ascii" } else { cs }, &raw),
        olen,
    )
}

// ---- addresses ----

fn addrs_raw(value: &[u8], out: &mut [MimeAddr]) -> usize {
    let at = |k: usize| value.get(k).copied().unwrap_or(0);
    let mut n = 0;
    let mut s = 0;
    while at(s) != 0 && n < out.len() {
        // one element up to the top-level comma
        let (mut name, mut email, mut comment, mut bare) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut inq, mut angle, mut par, mut had_angle) = (false, false, 0i32, false);
        while at(s) != 0 {
            let c = at(s);
            if inq {
                if c == b'\\' && at(s + 1) != 0 {
                    s += 1;
                    name.push(at(s));
                    s += 1;
                    continue;
                }
                if c == b'"' {
                    inq = false;
                    s += 1;
                    continue;
                }
                name.push(c);
                s += 1;
                continue;
            }
            if par != 0 {
                if c == b'\\' && at(s + 1) != 0 {
                    s += 1;
                    comment.push(at(s));
                    s += 1;
                    continue;
                }
                if c == b'(' {
                    par += 1;
                } else if c == b')' {
                    par -= 1;
                    if par == 0 {
                        s += 1;
                        continue;
                    }
                }
                comment.push(c);
                s += 1;
                continue;
            }
            if angle {
                if c == b'>' {
                    angle = false;
                } else {
                    email.push(c);
                }
                s += 1;
                continue;
            }
            match c {
                b'"' => inq = true,
                b'(' => par = 1,
                b'<' => {
                    angle = true;
                    had_angle = true;
                }
                b',' | b';' => {
                    s += 1;
                    break;
                }
                b':' => {
                    name.clear();
                    bare.clear();
                } // group name: discarded
                _ => {
                    bare.push(c);
                    name.push(c);
                }
            }
            s += 1;
        }
        // SAFETY: plain data, zero is the C initial state.
        let mut a: MimeAddr = unsafe { core::mem::zeroed() };
        if had_angle {
            copy_trim(zc_mut(&mut a.email), &email);
            let mut tmp = trimmed(&name, 512);
            if tmp.is_empty() {
                tmp = trimmed(&comment, 512);
            }
            decode_words_into(&tmp, zc_mut(&mut a.name));
        } else {
            copy_trim(zc_mut(&mut a.email), &bare);
            // unquoted "a@b" with spaces: keep the word containing @
            let e = zc(&a.email).to_vec();
            if chr(&e, b' ').is_some() {
                if let Some(atp) = chr(&e, b'@') {
                    let mut st = atp;
                    while st > 0 && e[st - 1] != b' ' {
                        st -= 1;
                    }
                    let mut en = atp;
                    while en < e.len() && e[en] != b' ' {
                        en += 1;
                    }
                    putc(&mut a.email, &e[st..en]);
                }
            }
            decode_words_into(&trimmed(&comment, 512), zc_mut(&mut a.name));
        }
        // only address-shaped addresses
        let em = zc_mut(&mut a.email);
        for c in em.iter_mut() {
            if *c == 0 {
                break;
            }
            if *c < 0x21 || *c == b'<' || *c == b'>' || *c == b'"' {
                *c = 0;
                break;
            }
        }
        let e = zc(&a.email);
        if !e.is_empty() && chr(e, b'@').is_some() {
            out[n] = a;
            n += 1;
        }
        while matches!(at(s), b' ' | b'\t' | b',' | b'\r' | b'\n') {
            s += 1;
        }
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_addrs(value: *const c_char, out: *mut MimeAddr, max: c_int) -> c_int {
    if value.is_null() {
        return 0;
    }
    let v = unsafe { cs(value) };
    let out = if out.is_null() || max <= 0 {
        &mut [][..]
    } else {
        unsafe { std::slice::from_raw_parts_mut(out, max as usize) }
    };
    let mut n = addrs_raw(v, out);
    // broken mailers encode the whole "Name <addr>" as one encoded word: decode, then parse
    if n == 0 && find(v, b"=?").is_some() {
        n = addrs_raw(&decode_words(v, 2048), out);
    }
    n as c_int
}

// ---- dates ----

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - (m <= 2) as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn date(v: &[u8]) -> i64 {
    const MON: [&[u8]; 12] = [
        b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep", b"oct", b"nov",
        b"dec",
    ];
    const ZONES: [(&[u8], i64); 12] = [
        (b"ut", 0),
        (b"utc", 0),
        (b"gmt", 0),
        (b"z", 0),
        (b"est", -5),
        (b"edt", -4),
        (b"cst", -6),
        (b"cdt", -5),
        (b"mst", -7),
        (b"mdt", -6),
        (b"pst", -8),
        (b"pdt", -7),
    ];
    let at = |k: usize| v.get(k).copied().unwrap_or(0);
    let (mut day, mut month, mut year) = (-1i32, -1i32, -1i32);
    let (mut hh, mut mm, mut ss, mut have_time) = (0i32, 0i32, 0i32, false);
    let mut zone: i64 = 0;
    let mut s = 0;
    while at(s) != 0 {
        while matches!(at(s), b' ' | b'\t' | b',' | b'\r' | b'\n') {
            s += 1;
        }
        if at(s) == 0 {
            break;
        }
        if at(s) == b'(' {
            let mut d = 0;
            while at(s) != 0 {
                if at(s) == b'(' {
                    d += 1;
                } else if at(s) == b')' {
                    d -= 1;
                    if d == 0 {
                        s += 1;
                        break;
                    }
                }
                s += 1;
            }
            continue;
        }
        let a = s;
        while at(s) != 0 && !matches!(at(s), b' ' | b'\t' | b',' | b'(') {
            s += 1;
        }
        let tok = &v[a..s];
        let n = tok.len();
        if n >= 32 {
            continue;
        }
        if chr(tok, b':').is_some() && !have_time {
            // sscanf("%d:%d:%d") >= 2; like sscanf, each conversion is stored as it succeeds
            let mut got = 0;
            if let Some((h, k)) = scan_d(tok) {
                hh = h;
                got = 1;
                if tok.get(k) == Some(&b':') {
                    if let Some((m2, k2)) = scan_d(&tok[k + 1..]) {
                        mm = m2;
                        got = 2;
                        let r = &tok[k + 1 + k2..];
                        if r.first() == Some(&b':') {
                            if let Some((s2, _)) = scan_d(&r[1..]) {
                                ss = s2;
                            }
                        }
                    }
                }
            }
            if got >= 2 {
                have_time = true;
            }
            continue;
        }
        if (tok[0] == b'+' || tok[0] == b'-') && n == 5 && is_digit(tok[1]) {
            let zz = atol(&tok[1..]);
            let mut zv = (zz / 100) * 3600 + (zz % 100) * 60;
            if tok[0] == b'-' {
                zv = -zv;
            }
            zone = zv;
            continue;
        }
        if is_digit(tok[0]) {
            let x = atol(tok);
            if day < 0 && (1..=31).contains(&x) && n <= 2 {
                day = x as i32;
            } else if year < 0 {
                year = if n <= 2 {
                    if x < 50 { 2000 + x } else { 1900 + x }
                } else {
                    x
                } as i32;
            }
            continue;
        }
        let low = lower_all(tok);
        if let Some(k) = MON.iter().position(|m| low.starts_with(m)) {
            month = k as i32 + 1;
            continue;
        }
        for (zn, h) in ZONES {
            if low == zn {
                zone = h * 3600;
            }
        }
    }
    if day < 1 || month < 1 || !(1970..=9999).contains(&year) || hh > 23 || mm > 59 || ss > 60 {
        return 0;
    }
    let days = days_from_civil(year as i64, month as i64, day as i64);
    let t = days * 86400 + hh as i64 * 3600 + mm as i64 * 60 + ss as i64 - zone;
    if t > 0 { t } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_date(v: *const c_char) -> libc::time_t {
    if v.is_null() {
        return 0;
    }
    date(unsafe { cs(v) }) as libc::time_t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_format_date(t: libc::time_t, out: *mut c_char, cap: usize) {
    const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MO: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // SAFETY: localtime_r fills a caller-owned tm.
    let mut tm: libc::tm = unsafe { core::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    let mut off = tm.tm_gmtoff;
    let sign = if off < 0 { '-' } else { '+' };
    if off < 0 {
        off = -off;
    }
    let s = format!(
        "{}, {} {} {} {:02}:{:02}:{:02} {}{:02}{:02}",
        WD[tm.tm_wday.rem_euclid(7) as usize],
        tm.tm_mday,
        MO[tm.tm_mon.rem_euclid(12) as usize],
        tm.tm_year + 1900,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        sign,
        off / 3600,
        (off % 3600) / 60
    );
    put(unsafe { crate::c::out(out, cap) }, s.as_bytes());
}

// ---- building headers ----

fn needs_encoding(s: &[u8]) -> bool {
    s.iter().any(|&c| c >= 0x7f || (c < 0x20 && c != b'\t'))
}

fn encode_header(u: &[u8]) -> Vec<u8> {
    if !needs_encoding(u) && find(u, b"=?").is_none() {
        return u.to_vec();
    }
    let mut b = Vec::new();
    let n = u.len();
    let mut i = 0;
    let mut first = true;
    while i < n {
        let mut take = 0;
        while i + take < n && take < 42 {
            let k = utf8_len(&u[i + take..]).max(1);
            if take + k > 42 {
                break;
            }
            take += k;
        }
        if !first {
            b.extend_from_slice(b"\r\n ");
        }
        b.extend_from_slice(b"=?UTF-8?B?");
        b.extend_from_slice(&b64_enc(&u[i..i + take], false));
        b.extend_from_slice(b"?=");
        first = false;
        i += take;
    }
    b
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_encode_header(u: *const c_char) -> *mut c_char {
    malloc_str(&encode_header(unsafe { cs(u) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_format_addr(
    name: *const c_char,
    email: *const c_char,
) -> *mut c_char {
    let (name, email) = unsafe { (cs(name), cs(email)) };
    let mut b = Vec::new();
    if !name.is_empty() {
        if needs_encoding(name) {
            b.extend_from_slice(&encode_header(name));
        } else if name.iter().any(|c| b"()<>[]:;@\\,.\"".contains(c)) {
            b.push(b'"');
            for &c in name {
                if c == b'"' || c == b'\\' {
                    b.push(b'\\');
                }
                b.push(c);
            }
            b.push(b'"');
        } else {
            b.extend_from_slice(name);
        }
        b.extend_from_slice(b" <");
        b.extend_from_slice(email);
        b.push(b'>');
    } else {
        b.extend_from_slice(email);
    }
    malloc_str(&b)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_base_subject(subj: *const c_char, out: *mut c_char, cap: usize) {
    const PRE: [&[u8]; 15] = [
        b"re", b"fw", b"fwd", b"rv", b"aw", b"wg", b"sv", b"vs", b"tr", b"ref", b"res", b"enc",
        b"antw", b"odp", b"r",
    ];
    let subj = unsafe { cs(subj) };
    let at = |k: usize| subj.get(k).copied().unwrap_or(0);
    let mut s = 0;
    for _ in 0..20 {
        while at(s) == b' ' || at(s) == b'\t' {
            s += 1;
        }
        let mut hit = false;
        for p in PRE {
            if !starts_ic(&subj[s..], p) {
                continue;
            }
            let mut t = s + p.len();
            if at(t) == b'[' || at(t) == b'(' {
                let close = if at(t) == b'[' { b']' } else { b')' };
                t += 1;
                while is_digit(at(t)) {
                    t += 1;
                }
                if at(t) != close {
                    continue;
                }
                t += 1;
            }
            while at(t) == b' ' {
                t += 1;
            }
            if at(t) == b':' || at(t) == 0xef {
                if at(t) == b':' {
                    t += 1;
                } else if at(t + 1) == 0xbc && at(t + 2) == 0x9a {
                    t += 3;
                } else {
                    continue;
                }
                s = t;
                hit = true;
                break;
            }
        }
        if !hit {
            break;
        }
    }
    copy_trim(unsafe { crate::c::out(out, cap) }, &subj[s..]);
}

/// A safe basename for an attacker-chosen file name; never empty.
pub(crate) fn safe_filename(input: &[u8], cap: usize) -> Vec<u8> {
    let mut out = vec![0u8; cap];
    safe_filename_into(input, &mut out);
    z(&out).to_vec()
}

fn safe_filename_into(input: &[u8], out: &mut [u8]) {
    let cap = out.len();
    if cap == 0 {
        return;
    }
    let mut s = input
        .iter()
        .rposition(|&c| c == b'/' || c == b'\\')
        .map_or(0, |k| k + 1);
    let limit = (cap - 1).min(200);
    let mut o = 0;
    while s < input.len() && matches!(input[s], b'.' | b' ' | b'\t') {
        s += 1;
    }
    while s < input.len() && o < limit {
        let c = input[s];
        let k = utf8_len(&input[s..]);
        if k == 0 {
            if o + 1 > limit {
                break;
            }
            out[o] = b'_';
            o += 1;
            s += 1;
            continue;
        }
        let q = &input[s..];
        let cp = match k {
            2 => ((c as u32 & 0x1f) << 6) | (q[1] as u32 & 0x3f),
            3 => ((c as u32 & 0x0f) << 12) | ((q[1] as u32 & 0x3f) << 6) | (q[2] as u32 & 0x3f),
            4 => {
                ((c as u32 & 0x07) << 18)
                    | ((q[1] as u32 & 0x3f) << 12)
                    | ((q[2] as u32 & 0x3f) << 6)
                    | (q[3] as u32 & 0x3f)
            }
            _ => c as u32,
        };
        let bad = cp < 0x20
            || cp == 0x7f
            || (0x80..0xa0).contains(&cp)
            || (cp < 0x80 && b"<>:\"|?*".contains(&(cp as u8)))
            || (0x202a..=0x202e).contains(&cp)
            || (0x2066..=0x2069).contains(&cp)
            || cp == 0x200e
            || cp == 0x200f
            || cp == 0x061c
            || cp == 0xfeff;
        if bad {
            out[o] = b'_';
            o += 1;
            s += k;
            continue;
        }
        if o + k > limit {
            break;
        }
        out[o..o + k].copy_from_slice(&input[s..s + k]);
        o += k;
        s += k;
    }
    while o > 0 && (out[o - 1] == b' ' || out[o - 1] == b'.') {
        o -= 1;
    }
    out[o] = 0;
    if o == 0 || out[..o].iter().all(|c| b"._ ".contains(c)) {
        put(out, b"attachment");
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mime_safe_filename(input: *const c_char, out: *mut c_char, cap: usize) {
    safe_filename_into(unsafe { cs(input) }, unsafe { crate::c::out(out, cap) });
}
