//! In-house XML reader for WebDAV (CalDAV and CardDAV); see server/src/mail/xml.h.
//! A single pass without recursion; no DOCTYPE, bounded depth, nodes and input.

use crate::c::*;
use core::ffi::{CStr, c_char};
use std::ptr::null_mut;

const MX_MAX_DEPTH: i32 = 64;
const MX_MAX_NODES: i64 = 400_000;
const MX_MAX_INPUT: usize = 32 << 20;
const MAX_BINDS: usize = 512;
const XML_NS: &[u8] = b"http://www.w3.org/XML/1998/namespace";

#[repr(C)]
pub struct MxNode {
    pub ns: *const c_char,
    pub name: *const c_char,
    pub text: *mut c_char,
    pub tlen: usize,
    pub kid: *mut MxNode,
    pub last: *mut MxNode,
    pub next: *mut MxNode,
    pub parent: *mut MxNode,
}

/// The allocation behind every MxNode handed to C (node first, so the
/// pointers are interchangeable).
#[repr(C)]
struct Elem {
    node: MxNode,
    qname: Vec<u8>,
    store: Vec<u8>, // ns \0 local \0
    text: Vec<u8>,  // NUL-terminated once the document is complete
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}
fn is_name(c: u8) -> bool {
    c > 0x7f || c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b':')
}

/// Copies s with entities decoded; None on a bad one.
fn decode(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    let mut k = 0;
    while k < s.len() {
        if s[k] != b'&' {
            out.push(s[k]);
            k += 1;
            continue;
        }
        let mut e = k + 1;
        while e < s.len() && e - k < 12 && s[e] != b';' {
            e += 1;
        }
        if e >= s.len() || s[e] != b';' {
            return None;
        }
        let ent = &s[k + 1..e];
        match ent {
            b"lt" => out.push(b'<'),
            b"gt" => out.push(b'>'),
            b"amp" => out.push(b'&'),
            b"quot" => out.push(b'"'),
            b"apos" => out.push(b'\''),
            _ if ent.len() >= 2 && ent[0] == b'#' => {
                let hex = ent[1] == b'x' || ent[1] == b'X';
                let d0 = if hex { 2 } else { 1 };
                if ent.len() <= d0 {
                    return None;
                }
                let mut cp: u32 = 0;
                for &c in &ent[d0..] {
                    let v = match c {
                        b'0'..=b'9' => (c - b'0') as u32,
                        b'a'..=b'f' if hex => (c - b'a' + 10) as u32,
                        b'A'..=b'F' if hex => (c - b'A' + 10) as u32,
                        _ => return None,
                    };
                    cp = cp * if hex { 16 } else { 10 } + v;
                    if cp > 0x10ffff {
                        return None;
                    }
                }
                if cp == 0 || (0xd800..=0xdfff).contains(&cp) {
                    return None;
                }
                put_cp(&mut out, cp);
            }
            _ => return None,
        }
        k = e + 1;
    }
    Some(out)
}

unsafe fn elem<'a>(n: *mut MxNode) -> &'a mut Elem {
    unsafe { &mut *n.cast::<Elem>() }
}

/// Next node in document order below `top` (None at the end).
unsafe fn walk_next(n: *mut MxNode, top: *mut MxNode) -> *mut MxNode {
    unsafe {
        if !(*n).kid.is_null() {
            return (*n).kid;
        }
        let mut x = n;
        while x != top && (*x).next.is_null() {
            x = (*x).parent;
        }
        if x == top { null_mut() } else { (*x).next }
    }
}

/// Points every node's `text` at its buffer (NULL when there is none).
unsafe fn finish_text(root: *mut MxNode) {
    let mut n = root;
    while !n.is_null() {
        let e = unsafe { elem(n) };
        if e.text.is_empty() {
            e.node.text = null_mut();
            e.node.tlen = 0;
        } else {
            e.node.tlen = e.text.len();
            e.text.push(0);
            e.node.text = e.text.as_mut_ptr().cast();
        }
        n = unsafe { walk_next(n, root) };
    }
}

unsafe fn free_tree(root: *mut MxNode) {
    let mut n = root;
    while !n.is_null() {
        unsafe {
            if !(*n).kid.is_null() {
                let k = (*n).kid;
                (*n).kid = null_mut();
                n = k;
                continue;
            }
            let next = (*n).next;
            let parent = (*n).parent;
            let last = n == root;
            drop(Box::from_raw(n.cast::<Elem>()));
            if last {
                break;
            }
            n = if next.is_null() { parent } else { next };
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mx_free(root: *mut MxNode) {
    if !root.is_null() {
        unsafe { free_tree(root) }
    }
}

struct Bind {
    prefix: Vec<u8>,
    uri: Vec<u8>,
    depth: i32,
}

fn pop_binds(b: &mut Vec<Bind>, depth: i32) {
    while b.last().is_some_and(|x| x.depth >= depth) {
        b.pop();
    }
}

fn resolve<'a>(binds: &'a [Bind], prefix: &[u8]) -> &'a [u8] {
    if prefix == b"xml" {
        return XML_NS;
    }
    binds
        .iter()
        .rev()
        .find(|b| b.prefix == prefix)
        .map_or(b"", |b| &b.uri)
}

fn find(s: &[u8], from: usize, lit: &[u8]) -> Option<usize> {
    if from > s.len() || s.len() - from < lit.len() {
        return None;
    }
    s[from..]
        .windows(lit.len())
        .position(|w| w == lit)
        .map(|k| k + from)
}

fn parse(s: &[u8], root: &mut *mut MxNode) -> Result<(), &'static CStr> {
    let len = s.len();
    if len > MX_MAX_INPUT {
        return Err(c"input");
    }
    let mut binds: Vec<Bind> = Vec::new();
    let mut cur: *mut MxNode = null_mut();
    let mut depth = 0i32;
    let mut nodes = 0i64;
    let mut i = if len >= 3 && s[..3] == [0xef, 0xbb, 0xbf] {
        3
    } else {
        0
    };
    let starts = |i: usize, lit: &[u8]| s.len() - i >= lit.len() && &s[i..i + lit.len()] == lit;
    while i < len {
        if s[i] != b'<' {
            let a = i;
            while i < len && s[i] != b'<' {
                i += 1;
            }
            if cur.is_null() {
                if s[a..i].iter().any(|&c| !is_ws(c)) {
                    return Err(c"text outside element");
                }
                continue;
            }
            let t = decode(&s[a..i]).ok_or(c"entity")?;
            unsafe { elem(cur) }.text.extend_from_slice(&t);
            continue;
        }
        if starts(i, b"<?") {
            let e = find(s, i + 2, b"?>").ok_or(c"unclosed instruction")?;
            i = e + 2;
            continue;
        }
        if starts(i, b"<!--") {
            let e = find(s, i + 4, b"-->").ok_or(c"unclosed comment")?;
            i = e + 3;
            continue;
        }
        if starts(i, b"<![CDATA[") {
            if cur.is_null() {
                return Err(c"CDATA outside element");
            }
            let e = find(s, i + 9, b"]]>").ok_or(c"unclosed CDATA")?;
            unsafe { elem(cur) }.text.extend_from_slice(&s[i + 9..e]);
            i = e + 3;
            continue;
        }
        if starts(i, b"<!") {
            return Err(c"DOCTYPE not allowed");
        }
        if starts(i, b"</") {
            i += 2;
            let a = i;
            while i < len && is_name(s[i]) {
                i += 1;
            }
            let nl = i - a;
            while i < len && is_ws(s[i]) {
                i += 1;
            }
            if i >= len || s[i] != b'>' {
                return Err(c"malformed closing tag");
            }
            i += 1;
            if cur.is_null() {
                return Err(c"closing tag without opening");
            }
            if unsafe { elem(cur) }.qname != s[a..a + nl] {
                return Err(c"mismatched closing tag");
            }
            pop_binds(&mut binds, depth);
            depth -= 1;
            cur = unsafe { (*cur).parent };
            continue;
        }
        // opening tag
        i += 1;
        let qa = i;
        while i < len && is_name(s[i]) {
            i += 1;
        }
        let ql = i - qa;
        if ql == 0 {
            return Err(c"empty name");
        }
        if cur.is_null() && !root.is_null() {
            return Err(c"two roots");
        }
        if depth >= MX_MAX_DEPTH {
            return Err(c"too deep");
        }
        nodes += 1;
        if nodes > MX_MAX_NODES {
            return Err(c"too many elements");
        }
        let mut selfclose = false;
        loop {
            while i < len && is_ws(s[i]) {
                i += 1;
            }
            if i >= len {
                return Err(c"unclosed tag");
            }
            if s[i] == b'>' {
                i += 1;
                break;
            }
            if s[i] == b'/' {
                if i + 1 >= len || s[i + 1] != b'>' {
                    return Err(c"stray /");
                }
                i += 2;
                selfclose = true;
                break;
            }
            let an = i;
            while i < len && is_name(s[i]) {
                i += 1;
            }
            let anl = i - an;
            if anl == 0 {
                return Err(c"attribute");
            }
            while i < len && is_ws(s[i]) {
                i += 1;
            }
            if i >= len || s[i] != b'=' {
                return Err(c"attribute without =");
            }
            i += 1;
            while i < len && is_ws(s[i]) {
                i += 1;
            }
            if i >= len || (s[i] != b'"' && s[i] != b'\'') {
                return Err(c"unquoted value");
            }
            let qc = s[i];
            i += 1;
            let va = i;
            while i < len && s[i] != qc {
                if s[i] == b'<' {
                    return Err(c"< in an attribute");
                }
                i += 1;
            }
            if i >= len {
                return Err(c"unclosed value");
            }
            let vb = i;
            i += 1;
            let aname = &s[an..an + anl];
            let is_def = aname == b"xmlns";
            let is_pre = anl > 6 && aname.starts_with(b"xmlns:");
            if is_def || is_pre {
                if binds.len() >= MAX_BINDS {
                    return Err(c"too many namespaces");
                }
                let uri = decode(&s[va..vb]).ok_or(c"entity")?;
                // the URI is a C string in the original: it ends at an embedded NUL
                let uri = z(&uri).to_vec();
                let prefix = if is_def {
                    Vec::new()
                } else {
                    aname[6..].to_vec()
                };
                binds.push(Bind {
                    prefix,
                    uri,
                    depth: depth + 1,
                });
            } else {
                // other attributes are validated (entities) and discarded: DAV does not use them
                decode(&s[va..vb]).ok_or(c"entity")?;
            }
        }
        // resolved name
        let q = &s[qa..qa + ql];
        let colon = chr(q, b':');
        let prefix = colon.map_or(&q[..0], |c| &q[..c]);
        let local = colon.map_or(q, |c| &q[c + 1..]);
        if local.is_empty() {
            return Err(c"empty local name");
        }
        let ns = z(resolve(&binds, prefix));
        let mut store = Vec::with_capacity(ns.len() + local.len() + 2);
        store.extend_from_slice(ns);
        store.push(0);
        store.extend_from_slice(local);
        store.push(0);
        let mut e = Box::new(Elem {
            node: MxNode {
                ns: std::ptr::null(),
                name: std::ptr::null(),
                text: null_mut(),
                tlen: 0,
                kid: null_mut(),
                last: null_mut(),
                next: null_mut(),
                parent: cur,
            },
            qname: q.to_vec(),
            store,
            text: Vec::new(),
        });
        e.node.ns = e.store.as_ptr().cast();
        e.node.name = unsafe { e.store.as_ptr().add(ns.len() + 1) }.cast();
        let n = Box::into_raw(e).cast::<MxNode>();
        unsafe {
            if !cur.is_null() {
                if (*cur).last.is_null() {
                    (*cur).kid = n;
                } else {
                    (*(*cur).last).next = n;
                }
                (*cur).last = n;
            } else {
                *root = n;
            }
        }
        if selfclose {
            pop_binds(&mut binds, depth + 1);
        } else {
            cur = n;
            depth += 1;
        }
    }
    if !cur.is_null() || root.is_null() {
        return Err(c"incomplete document");
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mx_parse(
    s: *const c_char,
    len: usize,
    err: *mut *const c_char,
) -> *mut MxNode {
    let mut root: *mut MxNode = null_mut();
    let r = if s.is_null() {
        Err(c"input")
    } else {
        parse(unsafe { bytes(s, len) }, &mut root)
    };
    match r {
        Ok(()) => {
            unsafe { finish_text(root) };
            if !err.is_null() {
                unsafe { *err = std::ptr::null() }
            }
            root
        }
        Err(m) => {
            if !root.is_null() {
                unsafe { free_tree(root) }
            }
            if !err.is_null() {
                unsafe { *err = m.as_ptr() }
            }
            null_mut()
        }
    }
}

unsafe fn matches(n: *const MxNode, ns: *const c_char, name: &[u8]) -> bool {
    unsafe { (ns.is_null() || cs((*n).ns) == cs(ns)) && cs((*n).name) == name }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mx_child(
    n: *const MxNode,
    ns: *const c_char,
    name: *const c_char,
) -> *mut MxNode {
    let name = unsafe { cs(name) };
    let mut k = if n.is_null() {
        null_mut()
    } else {
        unsafe { (*n).kid }
    };
    while !k.is_null() {
        if unsafe { matches(k, ns, name) } {
            return k;
        }
        k = unsafe { (*k).next };
    }
    null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mx_next(
    n: *const MxNode,
    ns: *const c_char,
    name: *const c_char,
) -> *mut MxNode {
    let name = unsafe { cs(name) };
    let mut k = if n.is_null() {
        null_mut()
    } else {
        unsafe { (*n).next }
    };
    while !k.is_null() {
        if unsafe { matches(k, ns, name) } {
            return k;
        }
        k = unsafe { (*k).next };
    }
    null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mx_find(
    top: *const MxNode,
    ns: *const c_char,
    name: *const c_char,
) -> *mut MxNode {
    if top.is_null() {
        return null_mut();
    }
    let name = unsafe { cs(name) };
    let top = top.cast_mut();
    let mut n = unsafe { (*top).kid };
    while !n.is_null() {
        if unsafe { matches(n, ns, name) } {
            return n;
        }
        n = unsafe { walk_next(n, top) };
    }
    null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mx_text(n: *const MxNode) -> *const c_char {
    if n.is_null() || unsafe { (*n).text.is_null() } {
        c"".as_ptr()
    } else {
        unsafe { (*n).text }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mx_child_text(
    n: *const MxNode,
    ns: *const c_char,
    name: *const c_char,
    out: *mut c_char,
    cap: usize,
) {
    let o = unsafe { crate::c::out(out, cap) };
    if o.is_empty() {
        return;
    }
    o[0] = 0;
    let k = unsafe { mx_child(n, ns, name) };
    if k.is_null() || unsafe { (*k).text.is_null() } {
        return;
    }
    let t = unsafe { bytes((*k).text, (*k).tlen) };
    let mut a = 0;
    let mut b = t.len();
    while a < b && is_ws(t[a]) {
        a += 1;
    }
    while b > a && is_ws(t[b - 1]) {
        b -= 1;
    }
    let l = (b - a).min(cap - 1);
    o[..l].copy_from_slice(&t[a..a + l]);
    o[l] = 0;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mx_escape(out: *mut c_char, cap: usize, s: *const c_char) -> usize {
    let o = unsafe { crate::c::out(out, cap) };
    if o.is_empty() {
        return 0;
    }
    let mut n = 0;
    for &c in unsafe { cs(s) } {
        let one = [c];
        let r: &[u8] = match c {
            b'&' => b"&amp;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            b'"' => b"&quot;",
            _ => &one,
        };
        if n + r.len() >= cap {
            break;
        }
        o[n..n + r.len()].copy_from_slice(r);
        n += r.len();
    }
    o[n] = 0;
    n
}
