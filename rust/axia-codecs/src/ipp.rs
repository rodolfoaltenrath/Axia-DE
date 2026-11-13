//! Our own IPP/1.1 encoder and decoder (RFC 8010/8011), without libcups: enough to
//! talk to CUPS over its local socket and to read what it returns. What arrives is
//! untrusted: the message is validated whole in Rust, and only then turned into the
//! linked IppAttr/IppVal lists that cups.c walks (C ABI of server/src/print/ipp.h;
//! tests/ipp_test.c, with broken and mutated messages).
use std::ffi::{c_char, c_int, CStr};
use std::ptr;

const IPP_MAX_ATTRS: usize = 20000;
const IPP_MAX_VALUES: usize = 60000;
const IPP_MAX_DEPTH: u32 = 8;

const G_OPERATION: u8 = 0x01;
const G_END: u8 = 0x03;
const T_INTEGER: u8 = 0x21;
const T_BOOLEAN: u8 = 0x22;
const T_ENUM: u8 = 0x23;
const T_STRING: u8 = 0x30;
const T_RESOLUTION: u8 = 0x32;
const T_RANGE: u8 = 0x33;
const T_BEGCOL: u8 = 0x34;
const T_TEXTLANG: u8 = 0x35;
const T_NAMELANG: u8 = 0x36;
const T_ENDCOL: u8 = 0x37;
const T_TEXT: u8 = 0x41;
const T_NAME: u8 = 0x42;
const T_KEYWORD: u8 = 0x44;
const T_URI: u8 = 0x45;
const T_CHARSET: u8 = 0x47;
const T_LANGUAGE: u8 = 0x48;
const T_MEMBERNAME: u8 = 0x4a;

// ---- encoding ----

#[repr(C)]
pub struct IppBuf {
    p: *mut u8,
    n: usize,
    cap: usize,
    err: c_int,
}

impl IppBuf {
    unsafe fn put(&mut self, d: &[u8]) {
        if self.err != 0 {
            return;
        }
        if self.n + d.len() > self.cap {
            let mut cap = if self.cap != 0 { self.cap * 2 } else { 512 };
            while cap < self.n + d.len() {
                cap *= 2;
            }
            if cap > 64 << 20 {
                self.err = 1;
                return;
            }
            let p = libc::realloc(self.p.cast(), cap).cast::<u8>();
            if p.is_null() {
                self.err = 1;
                return;
            }
            self.p = p;
            self.cap = cap;
        }
        if !d.is_empty() {
            ptr::copy_nonoverlapping(d.as_ptr(), self.p.add(self.n), d.len());
        }
        self.n += d.len();
    }
    unsafe fn put16(&mut self, v: u16) {
        self.put(&v.to_be_bytes());
    }
    unsafe fn put32(&mut self, v: u32) {
        self.put(&v.to_be_bytes());
    }
    unsafe fn name(&mut self, tag: u8, name: &[u8]) {
        if name.len() > 0x7fff {
            self.err = 1;
            return;
        }
        self.put(&[tag]);
        self.put16(name.len() as u16);
        self.put(name);
    }
}

unsafe fn cstr<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() { b"" } else { CStr::from_ptr(p).to_bytes() }
}

/// # Safety
/// `b` points to an IppBuf (its old contents are not freed).
#[no_mangle]
pub unsafe extern "C" fn ipp_init(b: *mut IppBuf, op: u16, id: u32) {
    let b = &mut *b;
    *b = IppBuf { p: ptr::null_mut(), n: 0, cap: 0, err: 0 };
    b.put(&[1, 1]); // version 1.1
    b.put16(op);
    b.put32(id);
}
/// # Safety
/// `b` was set up by ipp_init.
#[no_mangle]
pub unsafe extern "C" fn ipp_free(b: *mut IppBuf) {
    let b = &mut *b;
    libc::free(b.p.cast());
    *b = IppBuf { p: ptr::null_mut(), n: 0, cap: 0, err: 0 };
}
/// # Safety
/// `b` was set up by ipp_init.
#[no_mangle]
pub unsafe extern "C" fn ipp_group(b: *mut IppBuf, tag: u8) {
    (*b).put(&[tag]);
}
/// # Safety
/// `b` was set up by ipp_init; `name` and `value` are NULL or C strings.
#[no_mangle]
pub unsafe extern "C" fn ipp_add_str(b: *mut IppBuf, tag: u8, name: *const c_char, value: *const c_char) {
    let b = &mut *b;
    let v = cstr(value);
    if v.len() > 0x7fff {
        b.err = 1;
        return;
    }
    b.name(tag, cstr(name));
    b.put16(v.len() as u16);
    b.put(v);
}
/// # Safety
/// As ipp_add_str.
#[no_mangle]
pub unsafe extern "C" fn ipp_add_int(b: *mut IppBuf, tag: u8, name: *const c_char, v: i32) {
    let b = &mut *b;
    b.name(tag, cstr(name));
    b.put16(4);
    b.put32(v as u32);
}
/// # Safety
/// As ipp_add_str.
#[no_mangle]
pub unsafe extern "C" fn ipp_add_bool(b: *mut IppBuf, name: *const c_char, v: c_int) {
    let b = &mut *b;
    b.name(T_BOOLEAN, cstr(name));
    b.put16(1);
    b.put(&[(v != 0) as u8]);
}
/// # Safety
/// As ipp_add_str.
#[no_mangle]
pub unsafe extern "C" fn ipp_add_range(b: *mut IppBuf, name: *const c_char, lo: i32, hi: i32) {
    let b = &mut *b;
    b.name(T_RANGE, cstr(name));
    b.put16(8);
    b.put32(lo as u32);
    b.put32(hi as u32);
}
/// # Safety
/// `b` was set up by ipp_init.
#[no_mangle]
pub unsafe extern "C" fn ipp_end(b: *mut IppBuf) {
    (*b).put(&[G_END]);
}
/// # Safety
/// `b` was set up by ipp_init; the strings are NULL or C strings.
#[no_mangle]
pub unsafe extern "C" fn ipp_std(b: *mut IppBuf, printer_uri: *const c_char, user: *const c_char) {
    ipp_group(b, G_OPERATION);
    ipp_add_str(b, T_CHARSET, c"attributes-charset".as_ptr(), c"utf-8".as_ptr());
    ipp_add_str(b, T_LANGUAGE, c"attributes-natural-language".as_ptr(), c"en".as_ptr());
    if !printer_uri.is_null() {
        ipp_add_str(b, T_URI, c"printer-uri".as_ptr(), printer_uri);
    }
    if !user.is_null() {
        ipp_add_str(b, T_NAME, c"requesting-user-name".as_ptr(), user);
    }
}

// ---- decoding: first into owned values ----

struct Val {
    tag: u8,
    off: usize,
    len: u16,
    s: Option<Vec<u8>>,
    i: i32,
    i2: i32,
    col: Vec<Attr>,
}

struct Attr {
    group: u8,
    gseq: c_int,
    name: Vec<u8>,
    vals: Vec<Val>,
}

struct Rd<'a> {
    p: &'a [u8],
    i: usize,
    attrs: usize,
    vals: usize,
}

impl Rd<'_> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.p.get(self.i)?;
        self.i += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        if self.i + 2 > self.p.len() {
            return None;
        }
        let v = u16::from_be_bytes([self.p[self.i], self.p[self.i + 1]]);
        self.i += 2;
        Some(v)
    }
    /// A length and that many bytes after it.
    fn sized(&mut self) -> Option<(usize, u16)> {
        let l = self.u16()?;
        (self.i + l as usize <= self.p.len()).then_some((self.i, l))
    }
}

fn be32(p: &[u8]) -> i32 {
    i32::from_be_bytes([p[0], p[1], p[2], p[3]])
}

/// A 0 in the middle would cut the text: it's replaced.
fn dupn(p: &[u8]) -> Vec<u8> {
    p.iter().map(|&c| if c == 0 { b'?' } else { c }).collect()
}

fn is_text(t: u8) -> bool {
    (0x40..=0x5f).contains(&t)
}

/// Members of a collection, up to endCollection.
fn read_collection(r: &mut Rd, depth: u32) -> Option<Vec<Attr>> {
    if depth > IPP_MAX_DEPTH {
        return None;
    }
    let mut col: Vec<Attr> = Vec::new();
    loop {
        let tag = r.u8()?;
        let (_, nl) = r.sized()?;
        r.i += nl as usize; // in a collection the name is empty
        let (at, vl) = r.sized()?;
        if tag == T_ENDCOL {
            r.i += vl as usize;
            return Some(col);
        }
        if tag == T_MEMBERNAME {
            r.attrs += 1;
            if r.attrs > IPP_MAX_ATTRS {
                return None;
            }
            col.push(Attr { group: 0, gseq: 0, name: dupn(&r.p[at..at + vl as usize]), vals: Vec::new() });
            r.i += vl as usize;
            continue;
        }
        let cur = col.last_mut()?; // a value without a member name
        r.i -= 2; // read_value reads the length again
        cur.vals.push(read_value(r, tag, depth + 1)?);
    }
}

fn read_value(r: &mut Rd, tag: u8, depth: u32) -> Option<Val> {
    r.vals += 1;
    if r.vals > IPP_MAX_VALUES {
        return None;
    }
    let (off, vl) = r.sized()?;
    r.i += vl as usize;
    let d = &r.p[off..off + vl as usize];
    let mut v = Val { tag, off, len: vl, s: None, i: 0, i2: 0, col: Vec::new() };
    match tag {
        T_INTEGER | T_ENUM => {
            if vl != 4 {
                return None;
            }
            v.i = be32(d);
        }
        T_BOOLEAN => {
            if vl != 1 {
                return None;
            }
            v.i = (d[0] != 0) as i32;
        }
        T_RANGE | T_RESOLUTION => {
            if vl != if tag == T_RANGE { 8 } else { 9 } {
                return None;
            }
            v.i = be32(d);
            v.i2 = be32(&d[4..]);
        }
        T_BEGCOL => v.col = read_collection(r, depth)?,
        T_TEXTLANG | T_NAMELANG => {
            // language (length + text) and the text (length + text)
            if vl < 4 {
                return None;
            }
            let ll = (d[0] as usize) << 8 | d[1] as usize;
            if 2 + ll + 2 > vl as usize {
                return None;
            }
            let tl = (d[2 + ll] as usize) << 8 | d[3 + ll] as usize;
            if 4 + ll + tl != vl as usize {
                return None;
            }
            v.s = Some(dupn(&d[4 + ll..]));
        }
        _ => {
            if is_text(tag) || tag == T_STRING {
                v.s = Some(dupn(d));
            }
        }
    }
    Some(v)
}

/// The attributes and the bytes the message took.
fn parse(data: &[u8]) -> Option<(Vec<Attr>, usize)> {
    let mut r = Rd { p: data, i: 8, attrs: 0, vals: 0 };
    let mut attrs: Vec<Attr> = Vec::new();
    let (mut group, mut gseq, mut cur) = (0u8, 0, false);
    loop {
        let tag = r.u8()?;
        if tag == G_END {
            break;
        }
        if tag < 0x10 {
            // group tag
            if tag == 0 {
                return None;
            }
            group = tag;
            gseq += 1;
            cur = false;
            continue;
        }
        if group == 0 || tag == 0x7f {
            return None; // extended tags: not used
        }
        let (at, nl) = r.sized()?;
        if nl != 0 {
            r.attrs += 1;
            if r.attrs > IPP_MAX_ATTRS {
                return None;
            }
            attrs.push(Attr { group, gseq, name: dupn(&data[at..at + nl as usize]), vals: Vec::new() });
            cur = true;
            r.i += nl as usize;
        } else if !cur {
            return None; // extra value without an attribute
        }
        let v = read_value(&mut r, tag, 0)?;
        attrs.last_mut()?.vals.push(v);
    }
    Some((attrs, r.i))
}

// ---- the C side ----

#[repr(C)]
pub struct IppVal {
    tag: u8,
    data: *const u8,
    len: u16,
    s: *mut c_char,
    i: i32,
    i2: i32,
    col: *mut IppAttr,
    next: *mut IppVal,
}

#[repr(C)]
pub struct IppAttr {
    group: u8,
    gseq: c_int,
    name: *mut c_char,
    vals: *mut IppVal,
    nvals: c_int,
    next: *mut IppAttr,
}

#[repr(C)]
pub struct IppMsg {
    major: u8,
    minor: u8,
    code: u16,
    request_id: u32,
    attrs: *mut IppAttr,
    used: usize,
}

unsafe fn c_text(s: &[u8]) -> *mut c_char {
    let p = libc::malloc(s.len() + 1).cast::<u8>();
    if !p.is_null() {
        ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        *p.add(s.len()) = 0;
    }
    p.cast()
}

unsafe fn c_attrs(list: &[Attr], base: *const u8) -> *mut IppAttr {
    let mut head: *mut IppAttr = ptr::null_mut();
    let mut tail: *mut *mut IppAttr = &mut head;
    for a in list {
        let c = libc::calloc(1, std::mem::size_of::<IppAttr>()).cast::<IppAttr>();
        if c.is_null() {
            libc::abort();
        }
        (*c).group = a.group;
        (*c).gseq = a.gseq;
        (*c).name = c_text(&a.name);
        (*c).nvals = a.vals.len() as c_int;
        let mut vt: *mut *mut IppVal = &mut (*c).vals;
        for v in &a.vals {
            let n = libc::calloc(1, std::mem::size_of::<IppVal>()).cast::<IppVal>();
            if n.is_null() {
                libc::abort();
            }
            (*n).tag = v.tag;
            (*n).data = base.add(v.off);
            (*n).len = v.len;
            (*n).s = v.s.as_deref().map_or(ptr::null_mut(), |s| c_text(s));
            (*n).i = v.i;
            (*n).i2 = v.i2;
            (*n).col = c_attrs(&v.col, base);
            *vt = n;
            vt = &mut (*n).next;
        }
        *tail = c;
        tail = &mut (*c).next;
    }
    head
}

unsafe fn vals_free(mut v: *mut IppVal) {
    while !v.is_null() {
        let n = (*v).next;
        libc::free((*v).s.cast());
        attrs_free((*v).col);
        libc::free(v.cast());
        v = n;
    }
}

unsafe fn attrs_free(mut a: *mut IppAttr) {
    while !a.is_null() {
        let n = (*a).next;
        libc::free((*a).name.cast());
        vals_free((*a).vals);
        libc::free(a.cast());
        a = n;
    }
}

/// 0 on success; -1 malformed message or larger than the caps.
///
/// # Safety
/// `data` covers `len` bytes (or is NULL) and `m` points to an IppMsg.
#[no_mangle]
pub unsafe extern "C" fn ipp_parse(data: *const u8, len: usize, m: *mut IppMsg) -> c_int {
    let m = &mut *m;
    *m = IppMsg { major: 0, minor: 0, code: 0, request_id: 0, attrs: ptr::null_mut(), used: 0 };
    if data.is_null() || len < 9 {
        return -1;
    }
    let d = std::slice::from_raw_parts(data, len);
    m.major = d[0];
    m.minor = d[1];
    m.code = u16::from_be_bytes([d[2], d[3]]);
    m.request_id = be32(&d[4..]) as u32;
    if !(1..=2).contains(&m.major) {
        return -1;
    }
    match parse(d) {
        Some((attrs, used)) => {
            m.attrs = c_attrs(&attrs, data);
            m.used = used;
            0
        }
        None => -1,
    }
}

/// # Safety
/// `m` was filled by ipp_parse.
#[no_mangle]
pub unsafe extern "C" fn ipp_msg_free(m: *mut IppMsg) {
    attrs_free((*m).attrs);
    (*m).attrs = ptr::null_mut();
}

unsafe fn name_is(a: *const IppAttr, name: &[u8]) -> bool {
    !(*a).name.is_null() && CStr::from_ptr((*a).name).to_bytes() == name
}

/// The first attribute with that name in the group (0 = any group), after `from`.
///
/// # Safety
/// `m` was filled by ipp_parse; `from` is NULL or one of its attributes.
#[no_mangle]
pub unsafe extern "C" fn ipp_find(m: *const IppMsg, from: *const IppAttr, group: u8, name: *const c_char) -> *const IppAttr {
    let name = cstr(name);
    let mut a: *const IppAttr = if from.is_null() { (*m).attrs } else { (*from).next };
    while !a.is_null() {
        if (group == 0 || (*a).group == group) && name_is(a, name) {
            return a;
        }
        a = (*a).next;
    }
    ptr::null()
}

/// Forward within the same group as start (the attributes are in order).
///
/// # Safety
/// `start` is NULL or an attribute of a parsed message.
#[no_mangle]
pub unsafe extern "C" fn ipp_find_in(start: *const IppAttr, name: *const c_char) -> *const IppAttr {
    if start.is_null() {
        return ptr::null();
    }
    let (name, gseq) = (cstr(name), (*start).gseq);
    let mut a = start;
    while !a.is_null() && (*a).gseq == gseq {
        if name_is(a, name) {
            return a;
        }
        a = (*a).next;
    }
    ptr::null()
}

/// # Safety
/// `a` is NULL or an attribute of a parsed message.
#[no_mangle]
pub unsafe extern "C" fn ipp_str(a: *const IppAttr) -> *const c_char {
    if a.is_null() || (*a).vals.is_null() { ptr::null() } else { (*(*a).vals).s }
}

/// # Safety
/// `a` is NULL or an attribute of a parsed message.
#[no_mangle]
pub unsafe extern "C" fn ipp_int(a: *const IppAttr, def: i32) -> i32 {
    if a.is_null() || (*a).vals.is_null() {
        return def;
    }
    let v = &*(*a).vals;
    if matches!(v.tag, T_INTEGER | T_ENUM | T_BOOLEAN) { v.i } else { def }
}

#[no_mangle]
pub extern "C" fn ipp_tag_name(tag: u8) -> *const c_char {
    match tag {
        T_INTEGER => c"integer",
        T_BOOLEAN => c"boolean",
        T_ENUM => c"enum",
        T_TEXT => c"text",
        T_NAME => c"name",
        T_KEYWORD => c"keyword",
        T_URI => c"uri",
        T_RANGE => c"rangeOfInteger",
        T_BEGCOL => c"collection",
        _ => c"other",
    }
    .as_ptr()
}

#[no_mangle]
pub extern "C" fn ipp_status_name(s: u16) -> *const c_char {
    match s {
        0x0000 => c"successful-ok",
        0x0001 => c"successful-ok-ignored-or-substituted-attributes",
        0x0002 => c"successful-ok-conflicting-attributes",
        0x0400 => c"client-error-bad-request",
        0x0401 => c"client-error-forbidden",
        0x0402 => c"client-error-not-authenticated",
        0x0403 => c"client-error-not-authorized",
        0x0404 => c"client-error-not-possible",
        0x0406 => c"client-error-not-found",
        0x040a => c"client-error-document-format-not-supported",
        0x040b => c"client-error-attributes-or-values-not-supported",
        0x0500 => c"server-error-internal-error",
        0x0501 => c"server-error-operation-not-supported",
        0x0502 => c"server-error-service-unavailable",
        0x0507 => c"server-error-busy",
        s if s < 0x100 => c"successful-ok",
        s if s < 0x500 => c"client-error",
        _ => c"server-error",
    }
    .as_ptr()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_a_collection() {
        unsafe {
            let mut b: IppBuf = std::mem::zeroed();
            ipp_init(&mut b, 0x000b, 7);
            ipp_std(&mut b, c"ipp://localhost/printers/p".as_ptr(), c"ana".as_ptr());
            ipp_add_int(&mut b, T_ENUM, c"printer-state".as_ptr(), 3);
            ipp_add_range(&mut b, c"page-ranges".as_ptr(), 1, 5);
            let mut msg = std::slice::from_raw_parts(b.p, b.n).to_vec();
            // media-col = { media-size = { x-dimension = 21000 } }
            let mut put = |tag: u8, name: &[u8], val: &[u8]| {
                msg.push(tag);
                msg.extend_from_slice(&(name.len() as u16).to_be_bytes());
                msg.extend_from_slice(name);
                msg.extend_from_slice(&(val.len() as u16).to_be_bytes());
                msg.extend_from_slice(val);
            };
            put(T_BEGCOL, b"media-col", b"");
            put(T_MEMBERNAME, b"", b"media-size");
            put(T_BEGCOL, b"", b"");
            put(T_MEMBERNAME, b"", b"x-dimension");
            put(T_INTEGER, b"", &21000i32.to_be_bytes());
            put(T_ENDCOL, b"", b"");
            put(T_ENDCOL, b"", b"");
            put(T_TEXTLANG, b"printer-info", b"\x00\x02en\x00\x03a\x00b");
            msg.push(G_END);
            msg.extend_from_slice(b"%PDF");
            ipp_free(&mut b);
            let mut m: IppMsg = std::mem::zeroed();
            assert_eq!(ipp_parse(msg.as_ptr(), msg.len(), &mut m), 0);
            assert_eq!((m.code, m.request_id, m.used), (0x000b, 7, msg.len() - 4));
            let a = ipp_find(&m, ptr::null(), 0, c"printer-state".as_ptr());
            assert_eq!(ipp_int(a, -1), 3);
            let a = ipp_find(&m, ptr::null(), 1, c"requesting-user-name".as_ptr());
            assert_eq!(CStr::from_ptr(ipp_str(a)).to_bytes(), b"ana");
            let col = ipp_find(&m, ptr::null(), 0, c"media-col".as_ptr());
            let size = (*(*col).vals).col;
            assert_eq!(CStr::from_ptr((*size).name).to_bytes(), b"media-size");
            let x = (*(*size).vals).col;
            assert_eq!(ipp_int(x, 0), 21000);
            let info = ipp_find(&m, ptr::null(), 0, c"printer-info".as_ptr());
            assert_eq!(CStr::from_ptr(ipp_str(info)).to_bytes(), b"a?b");
            ipp_msg_free(&mut m);
            for cut in 0..msg.len() - 4 {
                let mut m: IppMsg = std::mem::zeroed();
                assert_eq!(ipp_parse(msg.as_ptr(), cut, &mut m), -1, "cut {cut}");
                assert!(m.attrs.is_null());
            }
        }
    }

    #[test]
    fn deep_collections_are_refused() {
        let mut msg = vec![1, 1, 0, 0, 0, 0, 0, 1, 1];
        msg.extend_from_slice(&[T_BEGCOL, 0, 1, b'c', 0, 0]);
        for _ in 0..10 {
            msg.extend_from_slice(&[T_MEMBERNAME, 0, 0, 0, 1, b'm', T_BEGCOL, 0, 0, 0, 0]);
        }
        assert!(parse(&msg).is_none());
    }
}
