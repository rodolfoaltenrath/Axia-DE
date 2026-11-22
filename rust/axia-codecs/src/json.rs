//! A strict JSON reader (RFC 8259) for the files LordOS verifies and the saved settings: contenido.json and
//! firma.json of .lpk packages, the signed catalog, the permissions, the firewall's file. C ABI of
//! server/src/sec/json.h (`json_parse`, `json_free`, `json_get`, `json_str`, `json_num`, `json_bool`,
//! `json_quote`) and a Rust API (`parse`, `Json`) for the Rust callers.
//!
//! Rejects: text that isn't JSON, more than 32 levels, duplicate keys, `\u0000` and trailing garbage. Strings
//! are the raw bytes of the text with the escapes decoded (UTF-8 is not checked, as before).
use std::collections::HashSet;
use std::ffi::{c_char, c_int, c_void};
use std::ptr;

const MAX_DEPTH: u32 = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(Vec<u8>),
    Arr(Vec<Json>),
    Obj(Vec<(Vec<u8>, Json)>),
}

impl Json {
    /// The member `key` of an object.
    pub fn get(&self, key: &[u8]) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// Why a text was refused, and the byte where the reader stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub why: &'static str,
    pub at: usize,
}

impl JsonError {
    /// The message as the C code wrote it: "<why> (byte <n>)".
    pub fn message(&self) -> String {
        format!("{} (byte {})", self.why, self.at)
    }
}

struct P<'a> {
    t: &'a [u8],
    s: usize,
    depth: u32,
    why: Option<&'static str>,
}

type R<T> = Result<T, ()>;

impl P<'_> {
    fn fail<T>(&mut self, why: &'static str) -> R<T> {
        if self.why.is_none() {
            self.why = Some(why);
        }
        Err(())
    }

    fn peek(&self) -> Option<u8> {
        self.t.get(self.s).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.s += 1;
        }
    }

    fn hex4(&self, at: usize) -> Option<u32> {
        let mut v = 0u32;
        for i in 0..4 {
            let c = *self.t.get(at + i)?;
            v = v << 4 | (c as char).to_digit(16)?;
        }
        Some(v)
    }

    /// A quoted string (`s` is at the opening quote).
    fn string(&mut self) -> R<Vec<u8>> {
        self.s += 1;
        let (s, end) = (self.s, self.t.len());
        let mut raw = 0;
        while s + raw < end && self.t[s + raw] != b'"' {
            if self.t[s + raw] == b'\\' {
                raw += 1;
            }
            raw += 1;
        }
        if s + raw >= end {
            return self.fail("unterminated string");
        }
        let mut o = Vec::with_capacity(raw);
        while self.peek() != Some(b'"') {
            let Some(c) = self.peek() else { return self.fail("unterminated string") };
            if c < 0x20 {
                return self.fail("control character in a string");
            }
            if c != b'\\' {
                o.push(c);
                self.s += 1;
                continue;
            }
            self.s += 1;
            match self.peek() {
                Some(b'"') => o.push(b'"'),
                Some(b'\\') => o.push(b'\\'),
                Some(b'/') => o.push(b'/'),
                Some(b'b') => o.push(8),
                Some(b'f') => o.push(12),
                Some(b'n') => o.push(b'\n'),
                Some(b'r') => o.push(b'\r'),
                Some(b't') => o.push(b'\t'),
                Some(b'u') => {
                    let Some(mut cp) = (if end - self.s < 5 { None } else { self.hex4(self.s + 1) }) else {
                        return self.fail("invalid \\u");
                    };
                    self.s += 4;
                    if (0xd800..0xdc00).contains(&cp) {
                        let lo = if end - self.s < 7 || self.t[self.s + 1] != b'\\' || self.t[self.s + 2] != b'u' { None } else { self.hex4(self.s + 3) };
                        match lo {
                            Some(lo) if (0xdc00..=0xdfff).contains(&lo) => {
                                cp = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00);
                                self.s += 6;
                            }
                            _ => return self.fail("invalid surrogate pair"),
                        }
                    } else if (0xdc00..0xe000).contains(&cp) {
                        return self.fail("invalid surrogate pair");
                    }
                    if cp == 0 {
                        return self.fail("\\u0000 is not accepted");
                    }
                    // cp is a scalar value here: surrogates were refused above
                    let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                    let mut b = [0u8; 4];
                    o.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                }
                _ => return self.fail("invalid escape"),
            }
            self.s += 1;
        }
        self.s += 1;
        Ok(o)
    }

    fn number(&mut self) -> R<f64> {
        let t = self.t;
        let mut s = self.s;
        let digit = |s: usize| t.get(s).is_some_and(u8::is_ascii_digit);
        if t.get(s) == Some(&b'-') {
            s += 1;
        }
        if s >= t.len() {
            return self.fail("invalid number");
        }
        if t[s] == b'0' {
            s += 1;
        } else if (b'1'..=b'9').contains(&t[s]) {
            while digit(s) {
                s += 1;
            }
        } else {
            return self.fail("invalid number");
        }
        if t.get(s) == Some(&b'.') {
            s += 1;
            if !digit(s) {
                return self.fail("invalid number");
            }
            while digit(s) {
                s += 1;
            }
        }
        if matches!(t.get(s), Some(b'e' | b'E')) {
            s += 1;
            if matches!(t.get(s), Some(b'+' | b'-')) {
                s += 1;
            }
            if !digit(s) {
                return self.fail("invalid number");
            }
            while digit(s) {
                s += 1;
            }
        }
        let n = s - self.s;
        if n >= 64 {
            return self.fail("number too long");
        }
        // the text is ASCII digits, signs, '.' and 'e': always valid for f64
        let v = std::str::from_utf8(&t[self.s..s]).ok().and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0);
        self.s = s;
        Ok(v)
    }

    fn lit(&mut self, w: &[u8]) -> R<()> {
        if self.t.len() - self.s < w.len() || &self.t[self.s..self.s + w.len()] != w {
            return self.fail("invalid value");
        }
        self.s += w.len();
        Ok(())
    }

    fn container(&mut self, obj: bool) -> R<Json> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("nested too deeply");
        }
        self.s += 1;
        self.ws();
        let close = if obj { b'}' } else { b']' };
        let (mut arr, mut members, mut seen) = (Vec::new(), Vec::new(), HashSet::new());
        if self.peek() == Some(close) {
            self.s += 1;
            self.depth -= 1;
            return Ok(if obj { Json::Obj(members) } else { Json::Arr(arr) });
        }
        loop {
            let mut key = Vec::new();
            if obj {
                self.ws();
                if self.peek() != Some(b'"') {
                    return self.fail("expected a key");
                }
                key = self.string()?;
                if !seen.insert(key.clone()) {
                    return self.fail("repeated key");
                }
                self.ws();
                if self.peek() != Some(b':') {
                    return self.fail("expected ':'");
                }
                self.s += 1;
            }
            let v = self.value()?;
            if obj {
                members.push((key, v));
            } else {
                arr.push(v);
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.s += 1,
                Some(c) if c == close => {
                    self.s += 1;
                    break;
                }
                _ => return self.fail(if obj { "expected ',' or '}'" } else { "expected ',' or ']'" }),
            }
        }
        self.depth -= 1;
        Ok(if obj { Json::Obj(members) } else { Json::Arr(arr) })
    }

    fn value(&mut self) -> R<Json> {
        self.ws();
        let Some(c) = self.peek() else { return self.fail("missing value") };
        match c {
            b'{' | b'[' => self.container(c == b'{'),
            b'"' => self.string().map(Json::Str),
            b't' => self.lit(b"true").map(|_| Json::Bool(true)),
            b'f' => self.lit(b"false").map(|_| Json::Bool(false)),
            b'n' => self.lit(b"null").map(|_| Json::Null),
            _ => self.number().map(Json::Num),
        }
    }
}

/// Reads a whole text.
pub fn parse(text: &[u8]) -> Result<Json, JsonError> {
    let mut p = P { t: text, s: 0, depth: 0, why: None };
    let mut v = p.value();
    if v.is_ok() {
        p.ws();
        if p.s != text.len() {
            v = p.fail("trailing garbage");
        }
    }
    v.map_err(|_| JsonError { why: p.why.unwrap_or("invalid JSON"), at: p.s })
}

/// JSON text of `s` between quotes, escaping `"`, `\` and control characters.
pub fn quote(s: &[u8], out: &mut Vec<u8>) {
    out.push(b'"');
    for &c in s {
        match c {
            b'"' | b'\\' => out.extend_from_slice(&[b'\\', c]),
            0..=0x1f => out.extend_from_slice(format!("\\u{c:04x}").as_bytes()),
            _ => out.push(c),
        }
    }
    out.push(b'"');
}

// ---- C ABI ----

#[repr(C)]
pub struct JVal {
    typ: c_int,
    num: f64,
    str: *const c_char,
    len: usize,
    key: *const c_char,
    kid: *mut JVal,
    next: *mut JVal,
}

#[repr(C)]
pub struct JDoc {
    root: *mut JVal,
    mem: *mut c_void,
}

const J_NULL: c_int = 0;
const J_BOOL: c_int = 1;
const J_NUM: c_int = 2;
const J_STR: c_int = 3;
const J_ARR: c_int = 4;
const J_OBJ: c_int = 5;

/// How many values and how many bytes of text the tree needs.
fn measure(v: &Json, nodes: &mut usize, bytes: &mut usize) {
    *nodes += 1;
    match v {
        Json::Str(s) => *bytes += s.len() + 1,
        Json::Arr(a) => a.iter().for_each(|x| measure(x, nodes, bytes)),
        Json::Obj(m) => m.iter().for_each(|(k, x)| {
            *bytes += k.len() + 1;
            measure(x, nodes, bytes);
        }),
        _ => {}
    }
}

struct Arena {
    vals: *mut JVal,
    used: usize,
    text: *mut u8,
    tused: usize,
}

impl Arena {
    unsafe fn text(&mut self, s: &[u8]) -> *const c_char {
        let p = self.text.add(self.tused);
        ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        *p.add(s.len()) = 0;
        self.tused += s.len() + 1;
        p.cast()
    }

    /// Writes `v` (with `key` if it is an object member) and returns it.
    unsafe fn put(&mut self, v: &Json, key: Option<&[u8]>) -> *mut JVal {
        let slot = self.vals.add(self.used);
        self.used += 1;
        let key = key.map_or(ptr::null(), |k| self.text(k));
        let mut j = JVal { typ: J_NULL, num: 0.0, str: ptr::null(), len: 0, key, kid: ptr::null_mut(), next: ptr::null_mut() };
        match v {
            Json::Null => {}
            Json::Bool(b) => {
                j.typ = J_BOOL;
                j.num = *b as u8 as f64;
            }
            Json::Num(n) => {
                j.typ = J_NUM;
                j.num = *n;
            }
            Json::Str(s) => {
                j.typ = J_STR;
                j.str = self.text(s);
                j.len = s.len();
            }
            Json::Arr(a) => {
                j.typ = J_ARR;
                j.len = a.len();
                let mut tail: *mut *mut JVal = &mut j.kid;
                let mut kids = Vec::with_capacity(a.len());
                for x in a {
                    kids.push(self.put(x, None));
                }
                for k in kids {
                    *tail = k;
                    tail = &mut (*k).next;
                }
            }
            Json::Obj(m) => {
                j.typ = J_OBJ;
                j.len = m.len();
                let mut tail: *mut *mut JVal = &mut j.kid;
                let mut kids = Vec::with_capacity(m.len());
                for (k, x) in m {
                    kids.push(self.put(x, Some(k)));
                }
                for k in kids {
                    *tail = k;
                    tail = &mut (*k).next;
                }
            }
        }
        slot.write(j);
        slot
    }
}

/// Copies `msg` into `err` (room for `n` bytes) as snprintf would.
unsafe fn put_err(err: *mut c_char, n: usize, msg: &str) {
    if err.is_null() || n == 0 {
        return;
    }
    let b = msg.as_bytes();
    let k = b.len().min(n - 1);
    ptr::copy_nonoverlapping(b.as_ptr(), err.cast(), k);
    *err.add(k) = 0;
}

/// 0 on success. On failure `err` (optional) gets the reason and the position.
///
/// # Safety
/// `text` is valid for `len` bytes (or NULL with `len` 0), `doc` is writable, `err` NULL or has `errlen` bytes.
#[no_mangle]
pub unsafe extern "C" fn json_parse(text: *const c_char, len: usize, doc: *mut JDoc, err: *mut c_char, errlen: usize) -> c_int {
    let Some(doc) = doc.as_mut() else { return -1 };
    doc.root = ptr::null_mut();
    doc.mem = ptr::null_mut();
    let t: &[u8] = if text.is_null() { &[] } else { std::slice::from_raw_parts(text.cast(), len) };
    let v = match parse(t) {
        Ok(v) => v,
        Err(e) => {
            put_err(err, errlen, &e.message());
            return -1;
        }
    };
    let (mut nodes, mut bytes) = (0, 0);
    measure(&v, &mut nodes, &mut bytes);
    let vals_size = nodes * size_of::<JVal>();
    let mem = libc::malloc(vals_size + bytes + 1);
    if mem.is_null() {
        put_err(err, errlen, "out of memory (byte 0)");
        return -1;
    }
    let mut a = Arena { vals: mem.cast(), used: 0, text: mem.cast::<u8>().add(vals_size), tused: 0 };
    doc.root = a.put(&v, None);
    doc.mem = mem;
    0
}

/// # Safety
/// `doc` is NULL or was filled by json_parse.
#[no_mangle]
pub unsafe extern "C" fn json_free(doc: *mut JDoc) {
    if let Some(d) = doc.as_mut() {
        libc::free(d.mem);
        d.mem = ptr::null_mut();
        d.root = ptr::null_mut();
    }
}

unsafe fn member(o: *const JVal, key: *const c_char) -> *const JVal {
    if o.is_null() || key.is_null() || (*o).typ != J_OBJ {
        return ptr::null();
    }
    let mut k = (*o).kid;
    while !k.is_null() {
        if libc::strcmp((*k).key, key) == 0 {
            return k;
        }
        k = (*k).next;
    }
    ptr::null()
}

/// # Safety
/// `o` is NULL or a value of a parsed document, `key` a C string.
#[no_mangle]
pub unsafe extern "C" fn json_get(o: *const JVal, key: *const c_char) -> *const JVal {
    member(o, key)
}

/// # Safety
/// As json_get.
#[no_mangle]
pub unsafe extern "C" fn json_str(o: *const JVal, key: *const c_char) -> *const c_char {
    let v = member(o, key);
    if !v.is_null() && (*v).typ == J_STR { (*v).str } else { ptr::null() }
}

/// # Safety
/// As json_get.
#[no_mangle]
pub unsafe extern "C" fn json_num(o: *const JVal, key: *const c_char, def: f64) -> f64 {
    let v = member(o, key);
    if !v.is_null() && (*v).typ == J_NUM { (*v).num } else { def }
}

/// # Safety
/// As json_get.
#[no_mangle]
pub unsafe extern "C" fn json_bool(o: *const JVal, key: *const c_char, def: c_int) -> c_int {
    let v = member(o, key);
    if !v.is_null() && (*v).typ == J_BOOL { (*v).num as c_int } else { def }
}

/// JSON text with quotes and escapes of `s`, written into `out` (room for `n` bytes, always ends with NUL).
/// Returns the bytes written; what doesn't fit is left out, as before.
///
/// # Safety
/// `out` has room for `n` bytes, `s` is a C string.
#[no_mangle]
pub unsafe extern "C" fn json_quote(out: *mut c_char, n: usize, s: *const c_char) -> usize {
    if n < 3 {
        if n > 0 {
            *out = 0;
        }
        return 0;
    }
    let out = std::slice::from_raw_parts_mut(out.cast::<u8>(), n);
    let mut o = 0;
    out[o] = b'"';
    o += 1;
    let mut p = s.cast::<u8>();
    while *p != 0 && o + 8 < n {
        let c = *p;
        if c == b'"' || c == b'\\' {
            out[o] = b'\\';
            out[o + 1] = c;
            o += 2;
        } else if c < 0x20 {
            let t = format!("\\u{c:04x}");
            out[o..o + 6].copy_from_slice(t.as_bytes());
            o += 6;
        } else {
            out[o] = c;
            o += 1;
        }
        p = p.add(1);
    }
    out[o] = b'"';
    o += 1;
    out[o] = 0;
    o
}
