//! Minimal non-validating XML reader for the KeePass (KDBX) XML and key files: builds a tree in
//! the secrets arena (smem). The C ABI of server/src/vault/xmlmini.h, which this replaced.
//! Elements, attributes, text with the predefined and numeric entities (`&#N;` `&#xN;`), CDATA,
//! comments and processing instructions. It never expands declared entities: a `<!DOCTYPE` is an
//! error (no "billion laughs", no external entities). Maximum depth 256.
use std::ffi::{c_char, c_int, c_void, CStr};
use std::{ptr, slice};

unsafe extern "C" {
    fn sm_alloc(n: usize) -> *mut c_void;
    fn sm_realloc(p: *mut c_void, n: usize) -> *mut c_void;
    fn sm_free(p: *mut c_void);
    fn sm_strndup(s: *const c_char, n: usize) -> *mut c_char;
}

const MAX_DEPTH: i32 = 256;
const MAX_NODES: i64 = 4 * 1024 * 1024;

#[repr(C)]
pub struct XAttr {
    name: *mut c_char,
    value: *mut c_char,
}
#[repr(C)]
pub struct XNode {
    name: *mut c_char,
    /// Direct text (without the children's), no entities.
    text: *mut c_char,
    tlen: usize,
    attrs: *mut XAttr,
    nattrs: c_int,
    kids: *mut *mut XNode,
    nkids: c_int,
    cap: c_int,
    parent: *mut XNode,
}

struct X<'a> {
    s: &'a [u8],
    p: usize,
    err: *const c_char,
    nodes: i64,
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}
fn is_name(c: u8) -> bool {
    !is_ws(c) && !matches!(c, b'>' | b'/' | b'=' | b'<' | b'"' | b'\'') && c != 0
}

fn put_utf8(d: &mut [u8], cp: u32) -> usize {
    if cp < 0x80 {
        d[0] = cp as u8;
        1
    } else if cp < 0x800 {
        d[0] = 0xc0 | (cp >> 6) as u8;
        d[1] = 0x80 | (cp & 63) as u8;
        2
    } else if cp < 0x10000 {
        d[0] = 0xe0 | (cp >> 12) as u8;
        d[1] = 0x80 | ((cp >> 6) & 63) as u8;
        d[2] = 0x80 | (cp & 63) as u8;
        3
    } else {
        d[0] = 0xf0 | (cp >> 18) as u8;
        d[1] = 0x80 | ((cp >> 12) & 63) as u8;
        d[2] = 0x80 | ((cp >> 6) & 63) as u8;
        d[3] = 0x80 | (cp & 63) as u8;
        4
    }
}

/// Appends `s[a..b]` to `*buf` (decoding entities if `decode`). The result is never longer than the
/// input (an entity takes at least as much space as its UTF-8). True when it failed.
unsafe fn append(buf: &mut *mut c_char, n: &mut usize, s: &[u8], a: usize, b: usize, decode: bool) -> bool {
    let d = sm_realloc((*buf).cast(), *n + (b - a) + 1).cast::<u8>();
    if d.is_null() {
        return true;
    }
    *buf = d.cast();
    let out = slice::from_raw_parts_mut(d, *n + (b - a) + 1);
    let mut o = *n;
    let mut i = a;
    while i < b {
        if !decode || s[i] != b'&' {
            out[o] = s[i];
            o += 1;
            i += 1;
            continue;
        }
        let mut e = i + 1;
        while e < b && e - i < 12 && s[e] != b';' {
            e += 1;
        }
        if e >= b || s[e] != b';' {
            return true;
        }
        let ent = &s[i + 1..e];
        match ent {
            b"lt" => {
                out[o] = b'<';
                o += 1;
            }
            b"gt" => {
                out[o] = b'>';
                o += 1;
            }
            b"amp" => {
                out[o] = b'&';
                o += 1;
            }
            b"quot" => {
                out[o] = b'"';
                o += 1;
            }
            b"apos" => {
                out[o] = b'\'';
                o += 1;
            }
            _ if ent.len() >= 2 && ent[0] == b'#' => {
                let hex = ent[1] == b'x' || ent[1] == b'X';
                let k0 = if hex { 2 } else { 1 };
                if k0 >= ent.len() {
                    return true;
                }
                let mut cp: u32 = 0;
                for &c in &ent[k0..] {
                    let v = match c {
                        b'0'..=b'9' => u32::from(c - b'0'),
                        b'a'..=b'f' if hex => u32::from(c - b'a') + 10,
                        b'A'..=b'F' if hex => u32::from(c - b'A') + 10,
                        _ => return true,
                    };
                    cp = cp * if hex { 16 } else { 10 } + v;
                    if cp > 0x10ffff {
                        return true;
                    }
                }
                if cp == 0 || (0xd800..0xe000).contains(&cp) {
                    cp = 0xfffd;
                }
                o += put_utf8(&mut out[o..], cp);
            }
            _ => return true,
        }
        i = e + 1;
    }
    out[o] = 0;
    *n = o;
    false
}

unsafe fn node_new(x: &mut X, parent: *mut XNode) -> *mut XNode {
    x.nodes += 1;
    if x.nodes > MAX_NODES {
        x.err = c"too-big".as_ptr();
        return ptr::null_mut();
    }
    let n = sm_alloc(std::mem::size_of::<XNode>()).cast::<XNode>();
    if n.is_null() {
        x.err = c"memory".as_ptr();
        return ptr::null_mut();
    }
    (*n).parent = parent;
    if !parent.is_null() {
        let p = &mut *parent;
        if p.nkids == p.cap {
            let nc = if p.cap != 0 { p.cap * 2 } else { 4 };
            let k = sm_realloc(p.kids.cast(), std::mem::size_of::<*mut XNode>() * nc as usize).cast::<*mut XNode>();
            if k.is_null() {
                sm_free(n.cast());
                x.err = c"memory".as_ptr();
                return ptr::null_mut();
            }
            p.kids = k;
            p.cap = nc;
        }
        *p.kids.add(p.nkids as usize) = n;
        p.nkids += 1;
    }
    n
}

fn starts(x: &X, w: &[u8]) -> bool {
    x.s.len() >= x.p + w.len() && &x.s[x.p..x.p + w.len()] == w
}
fn skip_to(x: &mut X, w: &[u8]) -> bool {
    while x.p + w.len() <= x.s.len() {
        if &x.s[x.p..x.p + w.len()] == w {
            x.p += w.len();
            return false;
        }
        x.p += 1;
    }
    true
}

/// `<name attributes>` or `</name>`; x.p is on the `<`. True when it failed.
unsafe fn parse_tag(x: &mut X, cur: &mut *mut XNode, root: &mut *mut XNode, depth: &mut i32) -> bool {
    let s = x.s;
    let len = s.len();
    x.p += 1;
    if x.p < len && s[x.p] == b'/' {
        x.p += 1;
        let a = x.p;
        while x.p < len && is_name(s[x.p]) {
            x.p += 1;
        }
        let nl = x.p - a;
        while x.p < len && is_ws(s[x.p]) {
            x.p += 1;
        }
        if x.p >= len || s[x.p] != b'>' {
            return true;
        }
        x.p += 1;
        if cur.is_null() || CStr::from_ptr((**cur).name).to_bytes() != &s[a..a + nl] {
            return true;
        }
        *cur = (**cur).parent;
        *depth -= 1;
        return false;
    }
    if cur.is_null() && !root.is_null() {
        return true; // a second root element
    }
    let a = x.p;
    while x.p < len && is_name(s[x.p]) {
        x.p += 1;
    }
    if x.p == a {
        return true;
    }
    let n = node_new(x, *cur);
    if n.is_null() {
        return true;
    }
    if root.is_null() {
        *root = n;
    }
    (*n).name = sm_strndup(s[a..].as_ptr().cast(), x.p - a);
    if (*n).name.is_null() {
        x.err = c"memory".as_ptr();
        return true;
    }
    loop {
        while x.p < len && is_ws(s[x.p]) {
            x.p += 1;
        }
        if x.p >= len {
            return true;
        }
        if s[x.p] == b'>' {
            x.p += 1;
            break;
        }
        if s[x.p] == b'/' {
            if x.p + 1 >= len || s[x.p + 1] != b'>' {
                return true;
            }
            x.p += 2;
            return false; // empty: not entered
        }
        let na = x.p;
        while x.p < len && is_name(s[x.p]) {
            x.p += 1;
        }
        if x.p == na {
            return true;
        }
        let nb = x.p;
        while x.p < len && is_ws(s[x.p]) {
            x.p += 1;
        }
        if x.p >= len || s[x.p] != b'=' {
            return true;
        }
        x.p += 1;
        while x.p < len && is_ws(s[x.p]) {
            x.p += 1;
        }
        if x.p >= len || (s[x.p] != b'"' && s[x.p] != b'\'') {
            return true;
        }
        let q = s[x.p];
        x.p += 1;
        let va = x.p;
        while x.p < len && s[x.p] != q {
            if s[x.p] == b'<' {
                return true;
            }
            x.p += 1;
        }
        if x.p >= len {
            return true;
        }
        let vb = x.p;
        x.p += 1;
        if (*n).nattrs >= 64 {
            return true;
        }
        let at = sm_realloc((*n).attrs.cast(), std::mem::size_of::<XAttr>() * ((*n).nattrs as usize + 1)).cast::<XAttr>();
        if at.is_null() {
            x.err = c"memory".as_ptr();
            return true;
        }
        (*n).attrs = at;
        let t = at.add((*n).nattrs as usize);
        (*t).name = sm_strndup(s[na..].as_ptr().cast(), nb - na);
        (*t).value = ptr::null_mut();
        let mut vl = 0usize;
        (*n).nattrs += 1;
        if (*t).name.is_null() || append(&mut (*t).value, &mut vl, s, va, vb, true) {
            return true;
        }
    }
    *depth += 1;
    if *depth > MAX_DEPTH {
        x.err = c"too-deep".as_ptr();
        return true;
    }
    *cur = n;
    false
}

unsafe fn parse(x: &mut X) -> *mut XNode {
    let s = x.s;
    let len = s.len();
    let (mut root, mut cur): (*mut XNode, *mut XNode) = (ptr::null_mut(), ptr::null_mut());
    let mut depth = 0;
    if len >= 3 && &s[..3] == b"\xef\xbb\xbf" {
        x.p = 3;
    }
    while x.p < len {
        if s[x.p] == b'<' {
            if starts(x, b"<?") {
                if skip_to(x, b"?>") {
                    break;
                }
                continue;
            }
            if starts(x, b"<!--") {
                if skip_to(x, b"-->") {
                    break;
                }
                continue;
            }
            if starts(x, b"<![CDATA[") {
                if cur.is_null() {
                    break;
                }
                let a = x.p + 9;
                x.p = a;
                if skip_to(x, b"]]>") || append(&mut (*cur).text, &mut (*cur).tlen, s, a, x.p - 3, false) {
                    break;
                }
                continue;
            }
            if starts(x, b"<!") {
                x.err = c"doctype".as_ptr(); // DOCTYPE/ENTITY: never
                break;
            }
            if parse_tag(x, &mut cur, &mut root, &mut depth) {
                break;
            }
            if !root.is_null() && cur.is_null() && depth == 0 {
                // root closed: only whitespace, comments or PIs may follow
                while x.p < len {
                    if is_ws(s[x.p]) {
                        x.p += 1;
                        continue;
                    }
                    if starts(x, b"<!--") {
                        if skip_to(x, b"-->") {
                            xml_free(root);
                            return ptr::null_mut();
                        }
                        continue;
                    }
                    if starts(x, b"<?") {
                        if skip_to(x, b"?>") {
                            xml_free(root);
                            return ptr::null_mut();
                        }
                        continue;
                    }
                    xml_free(root);
                    return ptr::null_mut();
                }
                x.err = ptr::null();
                return root;
            }
            continue;
        }
        let a = x.p;
        while x.p < len && s[x.p] != b'<' {
            x.p += 1;
        }
        if cur.is_null() {
            if s[a..x.p].iter().any(|c| !is_ws(*c)) {
                break;
            }
            continue;
        }
        if append(&mut (*cur).text, &mut (*cur).tlen, s, a, x.p, true) {
            break;
        }
    }
    xml_free(root);
    ptr::null_mut()
}

/// The document root (the topmost element) or NULL; `err` (if not NULL) receives a code.
///
/// # Safety
/// `s` covers `len` bytes; `err` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn xml_parse(s: *const c_char, len: usize, err: *mut *const c_char) -> *mut XNode {
    let bytes: &[u8] = if len == 0 || s.is_null() { &[] } else { slice::from_raw_parts(s.cast(), len) };
    let mut x = X { s: bytes, p: 0, err: ptr::null(), nodes: 0 };
    let root = parse(&mut x);
    if !err.is_null() {
        *err = if root.is_null() && x.err.is_null() { c"corrupt".as_ptr() } else { x.err };
    }
    root
}

/// # Safety
/// `n` is a node from xml_parse (or null) that is not used again.
#[no_mangle]
pub unsafe extern "C" fn xml_free(n: *mut XNode) {
    if n.is_null() {
        return;
    }
    let node = &mut *n;
    // recursive: depth is already bounded (MAX_DEPTH)
    for i in 0..node.nkids {
        xml_free(*node.kids.add(i as usize));
    }
    for i in 0..node.nattrs {
        let a = &*node.attrs.add(i as usize);
        sm_free(a.name.cast());
        sm_free(a.value.cast());
    }
    sm_free(node.attrs.cast());
    sm_free(node.kids.cast());
    sm_free(node.name.cast());
    sm_free(node.text.cast());
    sm_free(n.cast());
}

/// The first child with that name.
///
/// # Safety
/// `n` is a node from xml_parse or null; `name` is a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xml_child(n: *const XNode, name: *const c_char) -> *mut XNode {
    if n.is_null() {
        return ptr::null_mut();
    }
    let want = CStr::from_ptr(name).to_bytes();
    for i in 0..(*n).nkids {
        let kid = *(*n).kids.add(i as usize);
        if CStr::from_ptr((*kid).name).to_bytes() == want {
            return kid;
        }
    }
    ptr::null_mut()
}

/// The attribute's value ("" if it has none), NULL if absent.
///
/// # Safety
/// `n` is a node from xml_parse or null; `name` is a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xml_attr(n: *const XNode, name: *const c_char) -> *const c_char {
    if n.is_null() {
        return ptr::null();
    }
    let want = CStr::from_ptr(name).to_bytes();
    for i in 0..(*n).nattrs {
        let a = &*(*n).attrs.add(i as usize);
        if CStr::from_ptr(a.name).to_bytes() == want {
            return if a.value.is_null() { c"".as_ptr() } else { a.value };
        }
    }
    ptr::null()
}

/// The node's text, "" if none.
///
/// # Safety
/// `n` is a node from xml_parse or null.
#[no_mangle]
pub unsafe extern "C" fn xml_text(n: *const XNode) -> *const c_char {
    if n.is_null() || (*n).text.is_null() {
        c"".as_ptr()
    } else {
        (*n).text
    }
}

/// Replaces the text (the node takes ownership of the new one; in smem).
///
/// # Safety
/// `n` is a node from xml_parse; `text` is smem memory or null.
#[no_mangle]
pub unsafe extern "C" fn xml_set_text(n: *mut XNode, text: *mut c_char, len: usize) {
    sm_free((*n).text.cast());
    (*n).text = text;
    (*n).tlen = len;
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn text_of(n: *const XNode) -> String {
        CStr::from_ptr(xml_text(n)).to_string_lossy().into_owned()
    }
    unsafe fn parse_str(s: &str) -> (*mut XNode, String) {
        let mut err: *const c_char = ptr::null();
        let root = xml_parse(s.as_ptr().cast(), s.len(), &mut err);
        (root, if err.is_null() { String::new() } else { CStr::from_ptr(err).to_string_lossy().into_owned() })
    }

    #[test]
    fn reads_elements_attributes_entities_cdata_and_comments() {
        unsafe {
            let (root, err) = parse_str("\u{feff}<?xml version=\"1.0\"?><!-- c --><a k=\"v&amp;w\" e=''><b>x &lt;&#65;&#x42;&gt;</b><c><![CDATA[<raw>]]></c><d/></a> \n<!-- tail -->");
            assert!(!root.is_null() && err.is_empty(), "{err}");
            assert_eq!(CStr::from_ptr(xml_attr(root, c"k".as_ptr())).to_str().unwrap(), "v&w");
            assert_eq!(CStr::from_ptr(xml_attr(root, c"e".as_ptr())).to_str().unwrap(), "");
            assert!(xml_attr(root, c"none".as_ptr()).is_null());
            assert_eq!(text_of(xml_child(root, c"b".as_ptr())), "x <AB>");
            assert_eq!(text_of(xml_child(root, c"c".as_ptr())), "<raw>");
            assert!(!xml_child(root, c"d".as_ptr()).is_null() && xml_child(root, c"zz".as_ptr()).is_null());
            assert_eq!(text_of(root), "");
            xml_free(root);
        }
    }
    #[test]
    fn refuses_what_could_hurt() {
        unsafe {
            for (doc, why) in [
                ("<!DOCTYPE a [<!ENTITY x 'y'>]><a>&x;</a>", "doctype"),
                ("<a>&unknown;</a>", "corrupt"),
                ("<a>&#xZZ;</a>", "corrupt"),
                ("<a>&#x110000;</a>", "corrupt"),
                ("<a><b></a></b>", "corrupt"),
                ("<a></a><b></b>", "corrupt"),
                ("<a", "corrupt"),
                ("<a k=v></a>", "corrupt"),
                ("<a k=\"<\"></a>", "corrupt"),
                ("text<a/>", "corrupt"),
                ("<a/>trailing", "corrupt"),
                ("", "corrupt"),
            ] {
                let (root, err) = parse_str(doc);
                assert!(root.is_null(), "{doc}");
                assert_eq!(err, why, "{doc}");
            }
            let deep = "<a>".repeat(300);
            let (root, err) = parse_str(&deep);
            assert!(root.is_null() && err == "too-deep");
        }
    }
    #[test]
    fn an_invalid_character_reference_becomes_the_replacement_character() {
        unsafe {
            let (root, _) = parse_str("<a>&#0;&#xD800;</a>");
            assert_eq!(text_of(root), "\u{fffd}\u{fffd}");
            xml_free(root);
        }
    }
}
