//! Our own IPP/1.1 encoder and decoder (RFC 8010/8011), without libcups: enough to talk to CUPS
//! over its local socket and to read what it returns. What arrives is untrusted: the message is
//! validated whole (every length, the nesting, the number of attributes and values) before any of
//! it is used.

pub const G_OPERATION: u8 = 0x01;
pub const G_JOB: u8 = 0x02;
pub const G_END: u8 = 0x03;
pub const G_PRINTER: u8 = 0x04;

pub const T_INTEGER: u8 = 0x21;
pub const T_BOOLEAN: u8 = 0x22;
pub const T_ENUM: u8 = 0x23;
pub const T_STRING: u8 = 0x30;
pub const T_RESOLUTION: u8 = 0x32;
pub const T_RANGE: u8 = 0x33;
pub const T_BEGCOL: u8 = 0x34;
pub const T_TEXTLANG: u8 = 0x35;
pub const T_NAMELANG: u8 = 0x36;
pub const T_ENDCOL: u8 = 0x37;
pub const T_TEXT: u8 = 0x41;
pub const T_NAME: u8 = 0x42;
pub const T_KEYWORD: u8 = 0x44;
pub const T_URI: u8 = 0x45;
pub const T_CHARSET: u8 = 0x47;
pub const T_LANGUAGE: u8 = 0x48;
pub const T_MIMETYPE: u8 = 0x49;
pub const T_MEMBERNAME: u8 = 0x4a;

pub const OP_PRINT_JOB: u16 = 0x0002;
pub const OP_CANCEL_JOB: u16 = 0x0008;
pub const OP_GET_JOBS: u16 = 0x000a;
pub const OP_CUPS_GET_DEFAULT: u16 = 0x4001;
pub const OP_CUPS_GET_PRINTERS: u16 = 0x4002;
pub const OP_CUPS_ADD_MODIFY_PRINTER: u16 = 0x4003;
pub const OP_CUPS_DELETE_PRINTER: u16 = 0x4004;
pub const OP_CUPS_SET_DEFAULT: u16 = 0x400a;

const MAX_ATTRS: usize = 20000;
const MAX_VALUES: usize = 60000;
const MAX_DEPTH: u32 = 8;
const MAX_MESSAGE: usize = 64 << 20;

// ---- encoding ----

/// A request (or a reply, for the tests) being built. A value that does not fit in its 16-bit
/// length marks the message as failed: `finish()` then gives nothing.
pub struct Builder {
    buf: Vec<u8>,
    err: bool,
}

impl Builder {
    /// Version 1.1, with the operation (or status) and the request id.
    pub fn new(op: u16, request_id: u32) -> Builder {
        let mut b = Builder { buf: Vec::with_capacity(512), err: false };
        b.put(&[1, 1]);
        b.put(&op.to_be_bytes());
        b.put(&request_id.to_be_bytes());
        b
    }

    fn put(&mut self, d: &[u8]) {
        if self.err {
            return;
        }
        if self.buf.len() + d.len() > MAX_MESSAGE {
            self.err = true;
            return;
        }
        self.buf.extend_from_slice(d);
    }

    /// The tag, the name (empty for the following values of the same attribute) and no value yet.
    fn name(&mut self, tag: u8, name: Option<&str>) {
        let n = name.unwrap_or("").as_bytes();
        if n.len() > 0x7fff {
            self.err = true;
            return;
        }
        self.put(&[tag]);
        self.put(&(n.len() as u16).to_be_bytes());
        self.put(n);
    }

    pub fn group(&mut self, tag: u8) -> &mut Self {
        self.put(&[tag]);
        self
    }

    /// The first value carries the name; the following ones of the same attribute, `None`.
    pub fn add_str(&mut self, tag: u8, name: Option<&str>, value: &str) -> &mut Self {
        if value.len() > 0x7fff {
            self.err = true;
            return self;
        }
        self.name(tag, name);
        self.put(&(value.len() as u16).to_be_bytes());
        self.put(value.as_bytes());
        self
    }

    /// An INTEGER or an ENUM.
    pub fn add_int(&mut self, tag: u8, name: Option<&str>, v: i32) -> &mut Self {
        self.name(tag, name);
        self.put(&4u16.to_be_bytes());
        self.put(&v.to_be_bytes());
        self
    }

    pub fn add_bool(&mut self, name: &str, v: bool) -> &mut Self {
        self.name(T_BOOLEAN, Some(name));
        self.put(&1u16.to_be_bytes());
        self.put(&[v as u8]);
        self
    }

    pub fn add_range(&mut self, name: Option<&str>, lo: i32, hi: i32) -> &mut Self {
        self.name(T_RANGE, name);
        self.put(&8u16.to_be_bytes());
        self.put(&lo.to_be_bytes());
        self.put(&hi.to_be_bytes());
        self
    }

    /// The end-of-attributes tag.
    pub fn end(&mut self) -> &mut Self {
        self.put(&[G_END]);
        self
    }

    /// The common header: attributes-charset, attributes-natural-language, printer-uri (if any)
    /// and requesting-user-name (if any).
    pub fn std(&mut self, printer_uri: Option<&str>, user: Option<&str>) -> &mut Self {
        self.group(G_OPERATION);
        self.add_str(T_CHARSET, Some("attributes-charset"), "utf-8");
        self.add_str(T_LANGUAGE, Some("attributes-natural-language"), "en");
        if let Some(u) = printer_uri {
            self.add_str(T_URI, Some("printer-uri"), u);
        }
        if let Some(u) = user {
            self.add_str(T_NAME, Some("requesting-user-name"), u);
        }
        self
    }

    /// The message, or `None` if something did not fit.
    pub fn finish(self) -> Option<Vec<u8>> {
        (!self.err).then_some(self.buf)
    }

    /// The bytes so far, for tests that continue the message by hand.
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }
}

// ---- decoding ----

#[derive(Debug)]
pub struct Val {
    pub tag: u8,
    /// Texts and strings (a 0 in the middle is replaced by `?`).
    pub s: Option<Vec<u8>>,
    /// INTEGER, ENUM and BOOLEAN; the start of a RANGE; the width of a RESOLUTION.
    pub i: i32,
    /// The end of a RANGE; the height of a RESOLUTION.
    pub i2: i32,
    /// The members of a collection.
    pub col: Vec<Attr>,
}

#[derive(Debug)]
pub struct Attr {
    pub group: u8,
    /// The number of its group in the message (each printer in a list has its own).
    pub gseq: i32,
    pub name: Vec<u8>,
    pub vals: Vec<Val>,
}

impl Attr {
    pub fn is(&self, name: &str) -> bool {
        self.name == name.as_bytes()
    }
    /// The first value as text.
    pub fn str(&self) -> Option<&[u8]> {
        self.vals.first()?.s.as_deref()
    }
    /// The first value as an integer, `def` if it has none or is another type.
    pub fn int(&self, def: i32) -> i32 {
        match self.vals.first() {
            Some(v) if matches!(v.tag, T_INTEGER | T_ENUM | T_BOOLEAN) => v.i,
            _ => def,
        }
    }
}

#[derive(Debug)]
pub struct Msg {
    pub major: u8,
    pub minor: u8,
    /// The status, in replies.
    pub code: u16,
    pub request_id: u32,
    pub attrs: Vec<Attr>,
    /// Bytes the message took (what follows is document data).
    pub used: usize,
}

impl Msg {
    /// The first attribute with that name in the group (0 = any group), starting at `from`.
    pub fn find_from(&self, from: usize, group: u8, name: &str) -> Option<usize> {
        (from..self.attrs.len()).find(|&i| {
            let a = &self.attrs[i];
            (group == 0 || a.group == group) && a.is(name)
        })
    }
    pub fn find(&self, group: u8, name: &str) -> Option<&Attr> {
        self.find_from(0, group, name).map(|i| &self.attrs[i])
    }
    /// An attribute within the group numbered `gseq` (the group of one printer or one job).
    pub fn find_in(&self, gseq: i32, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|a| a.gseq == gseq && a.is(name))
    }
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
        let b = self.p.get(self.i..self.i + 2)?;
        self.i += 2;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }
    /// A length and that many bytes after it: where they start and how many.
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
    if depth > MAX_DEPTH {
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
            if r.attrs > MAX_ATTRS {
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
    if r.vals > MAX_VALUES {
        return None;
    }
    let (off, vl) = r.sized()?;
    r.i += vl as usize;
    let d = &r.p[off..off + vl as usize];
    let mut v = Val { tag, s: None, i: 0, i2: 0, col: Vec::new() };
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

/// Reads a message. `None` if it is malformed or larger than the caps.
pub fn parse(data: &[u8]) -> Option<Msg> {
    if data.len() < 9 {
        return None;
    }
    let (major, minor) = (data[0], data[1]);
    if !(1..=2).contains(&major) {
        return None;
    }
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
            if r.attrs > MAX_ATTRS {
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
    Some(Msg {
        major,
        minor,
        code: u16::from_be_bytes([data[2], data[3]]),
        request_id: be32(&data[4..]) as u32,
        attrs,
        used: r.i,
    })
}

/// The name of a status code (the error code JS sees when CUPS refuses something).
pub fn status_name(s: u16) -> &'static str {
    match s {
        0x0000 => "successful-ok",
        0x0001 => "successful-ok-ignored-or-substituted-attributes",
        0x0002 => "successful-ok-conflicting-attributes",
        0x0400 => "client-error-bad-request",
        0x0401 => "client-error-forbidden",
        0x0402 => "client-error-not-authenticated",
        0x0403 => "client-error-not-authorized",
        0x0404 => "client-error-not-possible",
        0x0406 => "client-error-not-found",
        0x040a => "client-error-document-format-not-supported",
        0x040b => "client-error-attributes-or-values-not-supported",
        0x0500 => "server-error-internal-error",
        0x0501 => "server-error-operation-not-supported",
        0x0502 => "server-error-service-unavailable",
        0x0507 => "server-error-busy",
        s if s < 0x100 => "successful-ok",
        s if s < 0x500 => "client-error",
        _ => "server-error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *seed >> 33
    }

    #[test]
    fn a_request_round_trips_and_leaves_the_document_alone() {
        let mut b = Builder::new(OP_PRINT_JOB, 42);
        b.std(Some("ipp://localhost/printers/PDF"), Some("lord"));
        b.add_str(T_NAME, Some("job-name"), "Informe ñ.pdf");
        b.add_str(T_MIMETYPE, Some("document-format"), "application/pdf");
        b.group(G_JOB);
        b.add_int(T_INTEGER, Some("copies"), 3);
        b.add_range(Some("page-ranges"), 1, 3);
        b.add_range(None, 5, 5);
        b.add_str(T_KEYWORD, Some("sides"), "two-sided-long-edge");
        b.add_bool("x-bool", true);
        b.add_int(T_ENUM, Some("orientation-requested"), 4);
        b.end();
        let mut buf = b.finish().expect("encodes");
        let n = buf.len();
        buf.extend_from_slice(b"%PDF"); // what follows the message is the document data
        let m = parse(&buf).expect("decodes");
        assert_eq!((m.major, m.minor, m.code, m.request_id, m.used), (1, 1, OP_PRINT_JOB, 42, n));
        assert_eq!(m.find(G_OPERATION, "attributes-charset").and_then(Attr::str), Some(&b"utf-8"[..]));
        assert_eq!(m.find(0, "job-name").and_then(Attr::str), Some("Informe ñ.pdf".as_bytes()));
        assert_eq!(m.find(G_JOB, "copies").map(|a| a.int(0)), Some(3));
        let pr = m.find(G_JOB, "page-ranges").unwrap();
        assert_eq!((pr.vals.len(), pr.vals[0].i, pr.vals[0].i2, pr.vals[1].i), (2, 1, 3, 5));
        assert_eq!(m.find(G_JOB, "x-bool").map(|a| a.int(0)), Some(1));
        assert_eq!(m.find(G_JOB, "orientation-requested").map(|a| a.int(0)), Some(4));
        assert!(m.find(G_OPERATION, "copies").is_none(), "the group counts");
    }

    /// A reply like CUPS-Get-Printers': two printers, a collection and a textWithLanguage.
    fn response() -> Vec<u8> {
        let mut b = Builder::new(0, 7);
        b.std(None, None);
        b.add_str(T_TEXT, Some("status-message"), "successful-ok");
        b.group(G_PRINTER);
        b.add_str(T_NAME, Some("printer-name"), "PDF");
        b.add_int(T_ENUM, Some("printer-state"), 3);
        b.add_str(T_KEYWORD, Some("media-supported"), "iso_a4_210x297mm");
        b.add_str(T_KEYWORD, None, "na_letter_8.5x11in");
        b.group(G_PRINTER);
        b.add_str(T_NAME, Some("printer-name"), "Oficina");
        b.add_int(T_ENUM, Some("printer-state"), 5);
        // a collection by hand, before the end: media-col = { size: "a4" }
        let mut v = b.bytes().to_vec();
        v.extend_from_slice(&[T_BEGCOL, 0, 9]);
        v.extend_from_slice(b"media-col");
        v.extend_from_slice(&[0, 0, T_MEMBERNAME, 0, 0, 0, 4]);
        v.extend_from_slice(b"size");
        v.extend_from_slice(&[T_KEYWORD, 0, 0, 0, 2, b'a', b'4', T_ENDCOL, 0, 0, 0, 0]);
        v.extend_from_slice(&[T_TEXTLANG, 0, 4]);
        v.extend_from_slice(b"info");
        v.extend_from_slice(&[0, 12, 0, 2, b'e', b's', 0, 6]);
        v.extend_from_slice(b"hola!!");
        v.push(G_END);
        v
    }

    #[test]
    fn a_reply_with_two_printers_a_collection_and_a_language_text() {
        let buf = response();
        let m = parse(&buf).expect("the reply");
        let a = m.find_from(0, G_PRINTER, "printer-name").unwrap();
        let b = m.find_from(a + 1, G_PRINTER, "printer-name").unwrap();
        let (a, b) = (&m.attrs[a], &m.attrs[b]);
        assert_eq!((a.str(), b.str()), (Some(&b"PDF"[..]), Some(&b"Oficina"[..])));
        assert_ne!(a.gseq, b.gseq, "separate groups");
        assert_eq!((m.find_in(a.gseq, "printer-state").map(|x| x.int(0)), m.find_in(b.gseq, "printer-state").map(|x| x.int(0))), (Some(3), Some(5)));
        let ms = m.find_in(a.gseq, "media-supported").unwrap();
        assert_eq!((ms.vals.len(), ms.vals[1].s.as_deref()), (2, Some(&b"na_letter_8.5x11in"[..])));
        assert!(m.find_in(b.gseq, "media-supported").is_none(), "does not look into another printer's group");
        let mc = m.find(0, "media-col").unwrap();
        assert_eq!(mc.vals[0].tag, T_BEGCOL);
        assert_eq!((mc.vals[0].col[0].name.as_slice(), mc.vals[0].col[0].vals[0].s.as_deref()), (&b"size"[..], Some(&b"a4"[..])));
        assert_eq!(m.find(0, "info").and_then(Attr::str), Some(&b"hola!!"[..]));
    }

    #[test]
    fn a_message_cut_anywhere_or_changed_at_random_never_breaks_anything() {
        let buf = response();
        for k in 0..buf.len() {
            assert!(parse(&buf[..k]).is_none(), "a cut at {k} was accepted");
        }
        let (mut seed, mut accepted) = (1234u64, 0);
        for _ in 0..20000 {
            let mut m = buf.clone();
            for _ in 0..1 + lcg(&mut seed) % 4 {
                let at = lcg(&mut seed) as usize % m.len();
                m[at] = lcg(&mut seed) as u8;
            }
            accepted += parse(&m).is_some() as u32; // accepted or not, but without a panic
        }
        assert!(accepted > 0, "some changes land on a value and stay valid");
    }

    #[test]
    fn impossible_lengths_and_shapes_are_refused() {
        let h = [1, 1, 0, 0, 0, 0, 0, 1];
        let with = |rest: &[u8]| [&h[..], rest].concat();
        assert!(parse(&with(&[1, T_NAME, 0xff, 0xff, b'a'])).is_none(), "a name longer than the message");
        assert!(parse(&with(&[1, T_NAME, 0, 0, 0, 1, b'x', 3])).is_none(), "an extra value without an attribute");
        assert!(parse(&with(&[1, T_INTEGER, 0, 1, b'n', 0, 2, 0, 1, 3])).is_none(), "a 2-byte integer");
        assert!(parse(&with(&[T_NAME, 0, 1, b'n', 0, 1, b'x', 3])).is_none(), "a value before any group");
        assert!(parse(&[3, 1, 0, 0, 0, 0, 0, 1, 3]).is_none(), "version 3");
        assert!(parse(&with(&[3])).is_some(), "an empty message is fine");
        // collections nested too deep
        let mut deep = with(&[1, T_BEGCOL, 0, 1, b'c', 0, 0]);
        for _ in 0..40 {
            deep.extend_from_slice(&[T_MEMBERNAME, 0, 0, 0, 1, b'm', T_BEGCOL, 0, 0, 0, 0]);
        }
        deep.push(3);
        assert!(parse(&deep).is_none(), "collections nested too deep");
    }

    #[test]
    fn values_that_do_not_fit_make_the_request_fail() {
        let mut b = Builder::new(OP_GET_JOBS, 1);
        b.add_str(T_NAME, Some("x"), &"a".repeat(0x8000));
        assert!(b.finish().is_none());
        let mut b = Builder::new(OP_GET_JOBS, 1);
        b.add_str(T_NAME, Some(&"n".repeat(0x8000)), "v");
        assert!(b.finish().is_none());
        assert_eq!(status_name(0x0406), "client-error-not-found");
        assert_eq!(status_name(0x0099), "successful-ok");
        assert_eq!(status_name(0x0450), "client-error");
        assert_eq!(status_name(0x0601), "server-error");
    }
}
