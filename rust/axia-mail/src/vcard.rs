//! vCard 2.1, 3.0 and 4.0 (RFC 6350) for Contacts; see server/src/mail/vcard.h.
//! Reading goes through the shared content-line parser (ical.rs).

use crate::c::*;
use crate::ical::{
    self, IcComp, IcProp, param, prop, props_named, unescape, unescape_all, unescape_into, value,
};
use crate::jbuf::{self, JBuf};
use crate::mime::{b64_dec, b64_enc, qp_dec, to_utf8, utf8_clean_vec};
use core::ffi::{c_char, c_int, c_void};

const VCARD_MAX_PHOTO: usize = 4 << 20;

/// The C JSON value (server/src/sec/json.h).
#[repr(C)]
pub struct JVal {
    pub type_: c_int,
    pub num: f64,
    pub str_: *const c_char,
    pub len: usize,
    pub key: *const c_char,
    pub kid: *mut JVal,
    pub next: *mut JVal,
}

const J_STR: c_int = 3;
const J_ARR: c_int = 4;
const J_OBJ: c_int = 5;

/// json_get: the first member called `key`.
fn json_get<'a>(o: Option<&'a JVal>, key: &[u8]) -> Option<&'a JVal> {
    let o = o?;
    if o.type_ != J_OBJ {
        return None;
    }
    let mut k = o.kid;
    while !k.is_null() {
        let v = unsafe { &*k };
        if unsafe { cs(v.key) } == key {
            return Some(v);
        }
        k = v.next;
    }
    None
}

/// json_str, with NULL as "".
fn js<'a>(o: Option<&'a JVal>, key: &[u8]) -> &'a [u8] {
    match json_get(o, key) {
        Some(v) if v.type_ == J_STR => unsafe { cs(v.str_) },
        _ => b"",
    }
}

/// The elements of an array member (empty when absent or not an array).
fn items<'a>(o: Option<&'a JVal>, key: &[u8]) -> Vec<&'a JVal> {
    let mut v = Vec::new();
    if let Some(a) = json_get(o, key).filter(|a| a.type_ == J_ARR) {
        let mut k = a.kid;
        while !k.is_null() {
            let x = unsafe { &*k };
            v.push(x);
            k = x.next;
        }
    }
    v
}

/// Splits a structured value on unescaped ';' into `max` unescaped fields of
/// at most 255 bytes each.
fn split_fields(v: &[u8], max: usize) -> Vec<Vec<u8>> {
    let mut f: Vec<Vec<u8>> = vec![Vec::new(); max];
    let mut n = 0;
    let mut k = 0;
    while k < v.len() && n < max {
        let c = v[k];
        if c == b'\\' && k + 1 < v.len() {
            k += 1;
            let c = if v[k] == b'n' || v[k] == b'N' {
                b'\n'
            } else {
                v[k]
            };
            if f[n].len() + 1 < 256 {
                f[n].push(c);
            }
            k += 1;
            continue;
        }
        if c == b';' {
            n += 1;
            k += 1;
            continue;
        }
        if f[n].len() + 1 < 256 {
            f[n].push(c);
        }
        k += 1;
    }
    // fields are C strings in the original: they end at an embedded NUL
    f.iter_mut().for_each(|x| x.truncate(z(x).len()));
    f
}

/// vCard 2.1 values may be quoted-printable and in another charset.
fn prop_text(p: &IcProp) -> Vec<u8> {
    let (_, enc) = param(p, b"ENCODING", 32);
    let (_, cs) = param(p, b"CHARSET", 40);
    let v = value(p);
    if eq_ic(&enc, b"QUOTED-PRINTABLE") {
        let mut raw = Vec::with_capacity(v.len());
        qp_dec(v, false, &mut raw);
        return to_utf8(if cs.is_empty() { b"utf-8" } else { &cs }, &raw);
    }
    if cs.is_empty() {
        utf8_clean_vec(v)
    } else {
        to_utf8(&cs, v)
    }
}

/// First meaningful TYPE: home, work, cell/mobile, fax, iphone, main, other.
fn type_of(p: &IcProp) -> Vec<u8> {
    let (_, t) = param(p, b"TYPE", 128);
    for tok in t.split(|&c| c == b',').filter(|x| !x.is_empty()) {
        let s = tok.iter().position(|&c| c != b' ').unwrap_or(tok.len());
        let tok = lower_all(&tok[s..]);
        if matches!(tok.as_slice(), b"pref" | b"internet" | b"voice" | b"x400") {
            continue;
        }
        let tok = if tok == b"mobile" {
            b"cell".to_vec()
        } else {
            tok
        };
        return tok[..tok.len().min(31)].to_vec();
    }
    Vec::new()
}

fn list_item(b: &mut JBuf, first: &mut bool, ty: &[u8], value: &[u8]) {
    if value.is_empty() {
        return;
    }
    b.raw(if *first {
        b"{\"type\":"
    } else {
        b",{\"type\":"
    });
    b.str(ty);
    b.raw(b",\"value\":");
    b.str(value);
    b.raw(b"}");
    *first = false;
}

fn strip_uuid(u: &[u8]) -> &[u8] {
    if starts_ic(u, b"urn:uuid:") {
        &u[9..]
    } else {
        u
    }
}

fn one_card(c: &IcComp, b: &mut JBuf) {
    b.raw(b"{\"uid\":");
    let uid = prop(c, b"UID").map_or_else(Vec::new, |p| unescape(value(p), 256));
    b.str(strip_uuid(&uid));
    let fn_ = prop(c, b"FN").map(|p| unescape_all(z(&prop_text(p))));
    b.raw(b",\"fn\":");
    b.str(fn_.as_deref().unwrap_or(b""));
    b.raw(b",\"n\":{");
    if let Some(p) = prop(c, b"N") {
        let f = split_fields(&prop_text(p), 5);
        for (i, k) in ["family", "given", "middle", "prefix", "suffix"]
            .iter()
            .enumerate()
        {
            b.fmt(format_args!("{}\"{}\":", if i != 0 { "," } else { "" }, k));
            b.str(&f[i]);
        }
    }
    b.raw(b"}");
    if let Some(p) = prop(c, b"NICKNAME") {
        b.raw(b",\"nick\":");
        b.str(&unescape(value(p), 256));
    }
    if let Some(p) = prop(c, b"ORG") {
        let f = split_fields(&prop_text(p), 2);
        b.raw(b",\"org\":");
        b.str(&f[0]);
        if !f[1].is_empty() {
            b.raw(b",\"dept\":");
            b.str(&f[1]);
        }
    }
    if let Some(p) = prop(c, b"TITLE") {
        b.raw(b",\"title\":");
        b.str(&unescape(value(p), 256));
    }
    for (pn, key) in [
        (&b"EMAIL"[..], "emails"),
        (b"TEL", "phones"),
        (b"URL", "urls"),
        (b"IMPP", "im"),
    ] {
        b.fmt(format_args!(",\"{}\":[", key));
        let mut first = true;
        for p in props_named(c, pn) {
            let ty = type_of(p);
            let mut v = unescape(z(&prop_text(p)), 512);
            if pn == b"TEL" && starts_ic(&v, b"tel:") {
                v.drain(..4);
            }
            list_item(b, &mut first, &ty, &v);
        }
        b.raw(b"]");
    }
    b.raw(b",\"addrs\":[");
    let mut first = true;
    for p in props_named(c, b"ADR") {
        let ty = type_of(p);
        let f = split_fields(&prop_text(p), 7);
        b.raw(if first { b"{\"type\":" } else { b",{\"type\":" });
        first = false;
        b.str(&ty);
        // street = extended + street
        let mut street = f[1].clone();
        if !f[1].is_empty() && !f[2].is_empty() {
            street.push(b'\n');
        }
        street.extend_from_slice(&f[2]);
        b.raw(b",\"street\":");
        b.str(&street);
        b.raw(b",\"city\":");
        b.str(&f[3]);
        b.raw(b",\"region\":");
        b.str(&f[4]);
        b.raw(b",\"code\":");
        b.str(&f[5]);
        b.raw(b",\"country\":");
        b.str(&f[6]);
        b.raw(b"}");
    }
    b.raw(b"]");
    if let Some(p) = prop(c, b"BDAY") {
        let v = value(p);
        if v.starts_with(b"--") {
            // vCard 4: --0517 (no year)
            if v.len() >= 6 {
                let mut out = b"--".to_vec();
                out.extend_from_slice(&v[2..4]);
                out.push(b'-');
                out.extend_from_slice(&v[4..v.len().min(6)]);
                b.raw(b",\"bday\":");
                b.str(&out);
            }
        } else {
            let mut ad = 0;
            let mut tz = [0u8; 8];
            if let Some(t) = ical::time(Some(p), &mut ad, Some(&mut tz)) {
                let w = t + ical::zone_offset(b"", t) as i64;
                let (y, m, d) = ical::civil_from_days(if w >= 0 {
                    w / 86400
                } else {
                    (w - 86399) / 86400
                });
                let mut out = format!("{:04}-{:02}-{:02}", y, m, d).into_bytes();
                out.truncate(15);
                b.raw(b",\"bday\":");
                b.str(&out);
            }
        }
    }
    if let Some(p) = prop(c, b"NOTE") {
        b.raw(b",\"notes\":");
        b.str(&unescape_all(z(&prop_text(p))));
    }
    if let Some(p) = prop(c, b"CATEGORIES") {
        b.raw(b",\"cats\":[");
        let v = value(p);
        let t = &v[..v.len().min(511)];
        let mut f = true;
        for tok in t.split(|&c| c == b',').filter(|x| !x.is_empty()) {
            let u = unescape(tok, 256);
            if u.is_empty() {
                continue;
            }
            if !f {
                b.raw(b",");
            }
            b.str(&u);
            f = false;
        }
        b.raw(b"]");
    }
    b.fmt(format_args!(
        ",\"photo\":{}",
        if prop(c, b"PHOTO").is_some() {
            "true"
        } else {
            "false"
        }
    ));
    let kind = prop(c, b"KIND").or_else(|| prop(c, b"X-ADDRESSBOOKSERVER-KIND"));
    let group = kind.is_some_and(|k| starts_ic(value(k), b"group"));
    b.fmt(format_args!(
        ",\"kind\":\"{}\"",
        if group { "group" } else { "individual" }
    ));
    if group {
        b.raw(b",\"members\":[");
        let mut f = true;
        for pn in [&b"MEMBER"[..], b"X-ADDRESSBOOKSERVER-MEMBER"] {
            for p in props_named(c, pn) {
                let u = unescape(value(p), 256);
                let x = strip_uuid(&u);
                if x.is_empty() {
                    continue;
                }
                if !f {
                    b.raw(b",");
                }
                b.str(x);
                f = false;
            }
        }
        b.raw(b"]");
    }
    if let Some(p) = prop(c, b"REV") {
        b.raw(b",\"rev\":");
        b.str(value(p));
    }
    b.raw(b"}");
}

/// The top-level VCARD components of a parsed text.
fn cards(root: &IcComp) -> Vec<&IcComp> {
    let mut v = Vec::new();
    let mut c = root.kid;
    while !c.is_null() {
        let cc = unsafe { &*c };
        if zc(&cc.name) == b"VCARD" {
            v.push(cc);
        }
        c = cc.next;
    }
    v
}

/// Runs f over the parsed text, then frees it; None when it does not parse.
fn with_parsed<R>(text: *const c_char, len: usize, f: impl FnOnce(&IcComp) -> R) -> Option<R> {
    let root = unsafe { ical::ic_parse(text, len, std::ptr::null_mut()) };
    if root.is_null() {
        return None;
    }
    let r = f(unsafe { &*root });
    unsafe { ical::ic_free(root) };
    Some(r)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vcard_json(text: *const c_char, len: usize, b: *mut JBuf) -> c_int {
    let Some(b) = (unsafe { jbuf::from_ptr(b) }) else {
        return -1;
    };
    with_parsed(text, len, |root| {
        let mut n = 0;
        for c in cards(root) {
            if n != 0 {
                b.raw(b",");
            }
            one_card(c, b);
            n += 1;
            if n >= 50000 {
                break;
            }
        }
        if n != 0 { n } else { -1 }
    })
    .unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vcard_photo(text: *const c_char, len: usize, olen: *mut usize) -> *mut u8 {
    set_len(olen, 0);
    with_parsed(text, len, |root| {
        // only the first VCARD counts
        let c = *cards(root).first()?;
        let p = prop(c, b"PHOTO")?;
        let mut v = value(p);
        let (_, enc) = param(p, b"ENCODING", 16);
        if starts_ic(v, b"data:") {
            let comma = chr(v, b',')?;
            find(v, b";base64")?;
            v = &v[comma + 1..];
        } else if !eq_ic(&enc, b"b") && !eq_ic(&enc, b"base64") {
            return None; // a URI: not fetched
        }
        if v.len() > VCARD_MAX_PHOTO * 4 / 3 + 16 {
            return None;
        }
        let mut out = Vec::with_capacity(v.len() * 3 / 4 + 4);
        b64_dec(v, &mut out);
        let p = unsafe { libc::malloc(v.len() * 3 / 4 + 4) }.cast::<u8>();
        if p.is_null() {
            return None;
        }
        unsafe { std::ptr::copy_nonoverlapping(out.as_ptr(), p, out.len()) };
        set_len(olen, out.len());
        Some(p)
    })
    .flatten()
    .unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vcard_uid(text: *const c_char, len: usize, out: *mut c_char, cap: usize) {
    let o = unsafe { crate::c::out(out, cap) };
    if !o.is_empty() {
        o[0] = 0;
    }
    with_parsed(text, len, |root| {
        let Some(c) = cards(root).first().copied() else {
            return;
        };
        if let Some(p) = prop(c, b"UID") {
            unescape_into(value(p), o);
            let l = strlen(o);
            if l >= 9 && starts_ic(&o[..l], b"urn:uuid:") {
                o.copy_within(9..l + 1, 0);
            }
        }
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vcard_emails(
    text: *const c_char,
    len: usize,
    f: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
    u: *mut c_void,
) {
    with_parsed(text, len, |root| {
        for c in cards(root) {
            let mut name =
                prop(c, b"FN").map_or_else(Vec::new, |p| unescape(z(&prop_text(p)), 256));
            name.push(0);
            for p in props_named(c, b"EMAIL") {
                let mut e = unescape(value(p), 256);
                if e.is_empty() {
                    continue;
                }
                e.push(0);
                if let Some(f) = f {
                    unsafe { f(u, name.as_ptr().cast(), e.as_ptr().cast()) }
                }
            }
        }
    });
}

// ---- writing ----

fn field(l: &mut JBuf, s: &[u8]) {
    l.raw(&ical::escape(s));
}

/// Appends the line built in l (unless it ran out of memory) and frees it.
fn emit(b: &mut JBuf, mut l: JBuf) {
    if l.oom == 0 {
        ical::line(b, l.as_bytes());
    }
    l.free();
}

fn typed_line(b: &mut JBuf, prop: &[u8], item: &JVal, extra_type: Option<&[u8]>) {
    let ty = js(Some(item), b"type");
    let value = js(Some(item), b"value");
    if value.is_empty() {
        return;
    }
    let mut l = JBuf::new();
    l.raw(prop);
    let t: Vec<u8> = ty
        .iter()
        .copied()
        .filter(|&c| is_alnum(c) || c == b'-')
        .map(upper)
        .take(39)
        .collect();
    if !t.is_empty() || extra_type.is_some() {
        l.raw(b";TYPE=");
        if let Some(x) = extra_type {
            l.raw(x);
            if !t.is_empty() {
                l.raw(b",");
            }
        }
        l.raw(&t);
    }
    l.raw(b":");
    field(&mut l, value);
    emit(b, l);
}

fn build(c: Option<&JVal>, photo: &[u8], ptype: &[u8]) -> JBuf {
    let mut b = JBuf::new();
    b.raw(b"BEGIN:VCARD\r\nVERSION:3.0\r\nPRODID:-//Axia DE//Contacts 1.0//EN\r\n");
    let line = |b: &mut JBuf, f: &dyn Fn(&mut JBuf)| {
        let mut l = JBuf::new();
        f(&mut l);
        emit(b, l);
    };
    line(&mut b, &|l| {
        l.raw(b"UID:");
        field(l, js(c, b"uid"));
    });
    let n = json_get(c, b"n");
    let mut fn_ = js(c, b"fn").to_vec();
    if fn_.is_empty() {
        let (given, family) = (js(n, b"given"), js(n, b"family"));
        fn_ = given.to_vec();
        if !given.is_empty() && !family.is_empty() {
            fn_.push(b' ');
        }
        fn_.extend_from_slice(family);
        fn_.truncate(599);
        if fn_.is_empty() {
            fn_ = js(c, b"org").to_vec();
            fn_.truncate(599);
        }
    }
    line(&mut b, &|l| {
        l.raw(b"FN:");
        field(l, &fn_);
    });
    line(&mut b, &|l| {
        l.raw(b"N:");
        for (i, k) in [&b"family"[..], b"given", b"middle", b"prefix", b"suffix"]
            .iter()
            .enumerate()
        {
            if i != 0 {
                l.raw(b";");
            }
            field(l, js(n, k));
        }
    });
    if !js(c, b"nick").is_empty() {
        line(&mut b, &|l| {
            l.raw(b"NICKNAME:");
            field(l, js(c, b"nick"));
        });
    }
    if !js(c, b"org").is_empty() || !js(c, b"dept").is_empty() {
        line(&mut b, &|l| {
            l.raw(b"ORG:");
            field(l, js(c, b"org"));
            if !js(c, b"dept").is_empty() {
                l.raw(b";");
                field(l, js(c, b"dept"));
            }
        });
    }
    if !js(c, b"title").is_empty() {
        line(&mut b, &|l| {
            l.raw(b"TITLE:");
            field(l, js(c, b"title"));
        });
    }
    for it in items(c, b"emails") {
        typed_line(&mut b, b"EMAIL", it, Some(b"INTERNET"));
    }
    for it in items(c, b"phones") {
        typed_line(&mut b, b"TEL", it, None);
    }
    for it in items(c, b"urls") {
        typed_line(&mut b, b"URL", it, None);
    }
    for it in items(c, b"im") {
        typed_line(&mut b, b"IMPP", it, None);
    }
    for it in items(c, b"addrs") {
        let it = Some(it);
        let t: Vec<u8> = js(it, b"type")
            .iter()
            .copied()
            .filter(|&x| is_alnum(x))
            .map(upper)
            .take(39)
            .collect();
        line(&mut b, &|l| {
            l.raw(b"ADR");
            if !t.is_empty() {
                l.raw(b";TYPE=");
                l.raw(&t);
            }
            l.raw(b":;;");
            for (i, k) in [&b"street"[..], b"city", b"region", b"code", b"country"]
                .iter()
                .enumerate()
            {
                if i != 0 {
                    l.raw(b";");
                }
                field(l, js(it, k));
            }
        });
    }
    let bd = js(c, b"bday");
    if bd.len() == 10 && bd[4] == b'-' && bd[7] == b'-' && is_digit(bd[0]) {
        b.raw(b"BDAY;VALUE=date:");
        b.put(bd);
        b.raw(b"\r\n");
    }
    if !js(c, b"notes").is_empty() {
        line(&mut b, &|l| {
            l.raw(b"NOTE:");
            field(l, js(c, b"notes"));
        });
    }
    let cats = items(c, b"cats");
    if !cats.is_empty() {
        line(&mut b, &|l| {
            l.raw(b"CATEGORIES:");
            let mut f = true;
            for it in cats.iter().filter(|x| x.type_ == J_STR) {
                if !f {
                    l.raw(b",");
                }
                field(l, unsafe { cs(it.str_) });
                f = false;
            }
        });
    }
    if js(c, b"kind") == b"group" {
        b.raw(b"X-ADDRESSBOOKSERVER-KIND:group\r\n");
        for it in items(c, b"members")
            .into_iter()
            .filter(|x| x.type_ == J_STR)
        {
            line(&mut b, &|l| {
                l.raw(b"X-ADDRESSBOOKSERVER-MEMBER:urn:uuid:");
                field(l, unsafe { cs(it.str_) });
            });
        }
    }
    if !photo.is_empty() && photo.len() <= VCARD_MAX_PHOTO {
        let e = b64_enc(photo, false);
        let t: &[u8] = if eq_ic(ptype, b"png") {
            b"PNG"
        } else {
            b"JPEG"
        };
        let mut pl = JBuf::new();
        pl.raw(b"PHOTO;ENCODING=b;TYPE=");
        pl.raw(t);
        pl.raw(b":");
        pl.raw(&e);
        emit(&mut b, pl);
    }
    // SAFETY: gmtime_r fills a caller-owned tm.
    let mut tm: libc::tm = unsafe { core::mem::zeroed() };
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    unsafe { libc::gmtime_r(&now, &mut tm) };
    b.fmt(format_args!(
        "REV:{:04}{:02}{:02}T{:02}{:02}{:02}Z\r\n",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    ));
    b.raw(b"END:VCARD\r\n");
    b
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn vcard_build(
    c: *const JVal,
    photo: *const u8,
    plen: usize,
    ptype: *const c_char,
    olen: *mut usize,
) -> *mut c_char {
    let photo = if photo.is_null() {
        &[][..]
    } else {
        unsafe { bytes(photo.cast(), plen) }
    };
    // a NULL contact reads as an empty object, like json_get(NULL, …) in C
    let mut b = build(unsafe { c.as_ref() }, photo, unsafe { cs(ptype) });
    if b.oom != 0 {
        b.free();
        return std::ptr::null_mut();
    }
    set_len(olen, b.n);
    b.p
}
