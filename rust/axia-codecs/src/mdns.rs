//! DNS-SD over mDNS (RFC 6762/6763), the packet side: reading replies into the
//! cache, building queries, turning the cache into services and reading TXT keys.
//! Replies are untrusted. C ABI of server/src/print/mdns.h; the socket loop
//! (mdns_browse) stays in server/src/print/mdns.c.
use std::ffi::{c_char, c_int, CStr};
use std::slice;

const MDNS_MAX: usize = 256;
const T_A: u32 = 1;
const T_PTR: u32 = 12;
const T_TXT: u32 = 16;
const T_SRV: u32 = 33;

#[repr(C)]
pub struct MdnsSvc {
    ty: [u8; 24],
    name: [u8; 128],
    host: [u8; 128],
    addr: [u8; 16],
    port: c_int,
    txt: [u8; 1536],
}

#[repr(C)]
struct Ptr {
    name: [u8; 256],
    target: [u8; 256],
    first: [u8; 128],
}
#[repr(C)]
struct Srv {
    name: [u8; 256],
    host: [u8; 256],
    port: c_int,
}
#[repr(C)]
struct Txt {
    name: [u8; 256],
    txt: [u8; 1536],
}
#[repr(C)]
struct A {
    host: [u8; 256],
    addr: [u8; 16],
}

#[repr(C)]
pub struct MdnsCache {
    ptr: [Ptr; MDNS_MAX],
    srv: [Srv; MDNS_MAX],
    txt: [Txt; MDNS_MAX],
    a: [A; MDNS_MAX],
    nptr: c_int,
    nsrv: c_int,
    ntxt: c_int,
    na: c_int,
}

/// The C string in a fixed buffer (up to its NUL).
fn cs(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// snprintf(out, sizeof out, "%s", s).
fn set(out: &mut [u8], s: &[u8]) {
    let n = s.len().min(out.len() - 1);
    out[..n].copy_from_slice(&s[..n]);
    out[n] = 0;
}

fn caseeq(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn printable(c: u8) -> u8 {
    if c < 0x20 { b' ' } else { c }
}

/// DNS name with compression → "a.b.c" (without the final dot), at most `n - 1`
/// bytes; `first` = the first label as it is, cut to `fn_ - 1`. Returns the
/// position after the name (in the original place).
fn read_name(p: &[u8], off: usize, n: usize, first: Option<(&mut Vec<u8>, usize)>) -> Option<(usize, Vec<u8>)> {
    let len = p.len();
    let mut out = Vec::new();
    let (mut pos, mut jumps, mut end, mut labels) = (off, 0, None, 0);
    let mut first = first;
    if let Some((f, _)) = first.as_mut() {
        f.clear();
    }
    loop {
        if pos >= len {
            return None;
        }
        let l = p[pos] as usize;
        if l == 0 {
            end.get_or_insert(pos + 1);
            break;
        }
        if l & 0xc0 == 0xc0 {
            jumps += 1;
            if pos + 1 >= len || jumps > 64 {
                return None;
            }
            let to = (l & 0x3f) << 8 | p[pos + 1] as usize;
            end.get_or_insert(pos + 2);
            if to >= len {
                return None;
            }
            pos = to;
            continue;
        }
        if l & 0xc0 != 0 {
            return None;
        }
        labels += 1;
        if pos + 1 + l > len || labels > 64 {
            return None;
        }
        let label = &p[pos + 1..pos + 1 + l];
        if labels == 1 {
            if let Some((f, fn_)) = first.as_mut() {
                let k = l.min(*fn_ - 1);
                f.extend(label[..k].iter().map(|&c| printable(c)));
            }
        }
        if out.len() + l + 2 > n {
            return None;
        }
        if !out.is_empty() {
            out.push(b'.');
        }
        out.extend(label.iter().map(|&c| printable(c)));
        pos += 1 + l;
    }
    Some((end.unwrap(), out))
}

/// Adds the records of a reply packet. Records read before an invalid one stay.
pub fn parse(p: &[u8], c: &mut MdnsCache) -> i32 {
    let len = p.len();
    if len < 12 {
        return -1;
    }
    if p[2] & 0x80 == 0 {
        return 0; // a question, not an answer
    }
    let be = |i: usize| (p[i] as usize) << 8 | p[i + 1] as usize;
    let (qd, total) = (be(4), be(6) + be(8) + be(10));
    let mut off = 12;
    for _ in 0..qd {
        match read_name(p, off, 256, None) {
            Some((e, _)) if e + 4 <= len => off = e + 4,
            _ => return -1,
        }
    }
    if total > 400 {
        return -1;
    }
    for _ in 0..total {
        let (e, name) = match read_name(p, off, 256, None) {
            Some((e, name)) if e + 10 <= len => (e, name),
            _ => return -1,
        };
        off = e;
        let ty = be(off) as u32;
        let rdlen = be(off + 8);
        off += 10;
        if off + rdlen > len {
            return -1;
        }
        let rd = off;
        off += rdlen;
        if ty == T_PTR && (c.nptr as usize) < MDNS_MAX {
            let mut first = Vec::new();
            let Some((_, tmp)) = read_name(p, rd, 256, Some((&mut first, 128))) else { continue };
            let n = c.nptr as usize;
            if c.ptr[..n].iter().any(|r| caseeq(cs(&r.target), &tmp)) {
                continue;
            }
            let r = &mut c.ptr[n];
            set(&mut r.name, &name);
            set(&mut r.target, &tmp);
            set(&mut r.first, &first);
            c.nptr += 1;
        } else if ty == T_SRV && rdlen >= 7 && (c.nsrv as usize) < MDNS_MAX {
            let port = be(rd + 4) as c_int;
            let Some((_, tmp)) = read_name(p, rd + 6, 256, None) else { continue };
            let r = &mut c.srv[c.nsrv as usize];
            set(&mut r.name, &name);
            set(&mut r.host, &tmp);
            r.port = port;
            c.nsrv += 1;
        } else if ty == T_TXT && (c.ntxt as usize) < MDNS_MAX {
            let r = &mut c.txt[c.ntxt as usize];
            let (mut o, mut q) = (0, rd);
            while q < rd + rdlen {
                let l = p[q] as usize;
                q += 1;
                if q + l > rd + rdlen {
                    break;
                }
                if o + l + 2 < r.txt.len() {
                    for &ch in &p[q..q + l] {
                        r.txt[o] = printable(ch);
                        o += 1;
                    }
                    r.txt[o] = b'\n';
                    o += 1;
                }
                q += l;
            }
            r.txt[o] = 0;
            set(&mut r.name, &name);
            c.ntxt += 1;
        } else if ty == T_A && rdlen == 4 && (c.na as usize) < MDNS_MAX {
            let r = &mut c.a[c.na as usize];
            set(&mut r.host, &name);
            let a = format!("{}.{}.{}.{}", p[rd], p[rd + 1], p[rd + 2], p[rd + 3]);
            set(&mut r.addr, a.as_bytes());
            c.na += 1;
        }
    }
    0
}

/// One query with one question per (name, type). Its length, 0 if it doesn't fit.
pub fn query(q: &mut [u8], questions: &[(&[u8], i32)], id: u16) -> usize {
    let cap = q.len();
    if cap < 12 {
        return 0;
    }
    q[..12].fill(0);
    q[0] = (id >> 8) as u8;
    q[1] = id as u8;
    q[5] = questions.len() as u8;
    let mut o = 12;
    for &(name, ty) in questions {
        // labels: none empty, none longer than 63
        let mut s = name;
        while !s.is_empty() {
            let dot = s.iter().position(|&c| c == b'.');
            let l = dot.unwrap_or(s.len());
            if l == 0 || l > 63 || o + l + 2 > cap {
                return 0;
            }
            q[o] = l as u8;
            q[o + 1..o + 1 + l].copy_from_slice(&s[..l]);
            o += 1 + l;
            s = &s[l + dot.map_or(0, |_| 1)..];
        }
        if o + 1 > cap {
            return 0;
        }
        q[o] = 0;
        o += 1;
        if o + 4 > cap {
            return 0;
        }
        q[o..o + 4].copy_from_slice(&[0, ty as u8, 0, 1]); // IN
        o += 4;
    }
    o
}

/// The services of the requested types, written into `out`; how many have a port.
pub fn collect(c: &MdnsCache, types: &[&[u8]], out: &mut [MdnsSvc]) -> usize {
    let mut n = 0;
    for p in &c.ptr[..(c.nptr.max(0) as usize).min(MDNS_MAX)] {
        if n >= out.len() {
            break;
        }
        let mut t = None;
        for &ty in types {
            let mut full = ty.to_vec();
            full.extend_from_slice(b".local");
            full.truncate(63);
            if caseeq(cs(&p.name), &full) {
                t = Some(ty);
            }
        }
        let Some(t) = t else { continue };
        let s = &mut out[n];
        // memset(s, 0, sizeof *s)
        s.ty = [0; 24];
        s.name = [0; 128];
        s.host = [0; 128];
        s.addr = [0; 16];
        s.port = 0;
        s.txt = [0; 1536];
        set(&mut s.ty, t);
        set(&mut s.name, cs(&p.first));
        let target = cs(&p.target);
        for r in &c.srv[..(c.nsrv.max(0) as usize).min(MDNS_MAX)] {
            if caseeq(cs(&r.name), target) {
                set(&mut s.host, cs(&r.host));
                s.port = r.port;
            }
        }
        for r in &c.txt[..(c.ntxt.max(0) as usize).min(MDNS_MAX)] {
            if caseeq(cs(&r.name), target) {
                set(&mut s.txt, cs(&r.txt));
            }
        }
        let host = cs(&s.host);
        if !host.is_empty() {
            if let Some(a) = c.a[..(c.na.max(0) as usize).min(MDNS_MAX)].iter().find(|a| caseeq(cs(&a.host), host)) {
                let addr = cs(&a.addr).to_vec();
                set(&mut s.addr, &addr);
            }
        }
        if s.port != 0 {
            n += 1;
        }
    }
    n
}

/// The value of a TXT key (empty if it's not there).
pub fn txt<'a>(txt: &'a [u8], key: &[u8]) -> &'a [u8] {
    let kl = key.len();
    for line in cs(txt).split(|&c| c == b'\n') {
        if line.len() > kl && line[kl] == b'=' && line[..kl].eq_ignore_ascii_case(key) {
            return &line[kl + 1..];
        }
    }
    &[]
}

unsafe fn cstr<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() { b"" } else { CStr::from_ptr(p).to_bytes() }
}

/// # Safety
/// `pkt` covers `len` bytes and `c` is a valid cache.
#[no_mangle]
pub unsafe extern "C" fn mdns_parse(pkt: *const u8, len: usize, c: *mut MdnsCache) -> c_int {
    let Some(c) = c.as_mut() else { return -1 };
    if pkt.is_null() {
        return -1;
    }
    parse(slice::from_raw_parts(pkt, len), c)
}

/// # Safety
/// `names` and `types` have `n` entries; `q` has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn mdns_query(
    q: *mut u8,
    cap: usize,
    names: *const *const c_char,
    types: *const c_int,
    n: c_int,
    id: u16,
) -> usize {
    if q.is_null() || cap < 12 {
        return 0;
    }
    let n = n.max(0) as usize;
    let qs: Vec<(&[u8], i32)> = (0..n).map(|i| (cstr(*names.add(i)), *types.add(i))).collect();
    query(slice::from_raw_parts_mut(q, cap), &qs, id)
}

/// # Safety
/// `types` has `ntypes` C strings and `out` room for `max` services.
#[no_mangle]
pub unsafe extern "C" fn mdns_collect(
    c: *const MdnsCache,
    types: *const *const c_char,
    ntypes: c_int,
    out: *mut MdnsSvc,
    max: c_int,
) -> c_int {
    let Some(c) = c.as_ref() else { return 0 };
    if out.is_null() || max <= 0 {
        return 0;
    }
    let ts: Vec<&[u8]> = (0..ntypes.max(0) as usize).map(|i| cstr(*types.add(i))).collect();
    collect(c, &ts, slice::from_raw_parts_mut(out, max as usize)) as c_int
}

/// # Safety
/// `s` is a valid service, `key` a C string and `out` has room for `n` (≥ 1) bytes.
#[no_mangle]
pub unsafe extern "C" fn mdns_txt(s: *const MdnsSvc, key: *const c_char, out: *mut c_char, n: usize) -> *const c_char {
    if out.is_null() || n == 0 {
        return out;
    }
    let o = slice::from_raw_parts_mut(out.cast::<u8>(), n);
    match s.as_ref() {
        Some(s) => set(o, txt(&s.txt, cstr(key))),
        None => o[0] = 0,
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(out: &mut Vec<u8>, s: &str) {
        for l in s.split('.') {
            out.push(l.len() as u8);
            out.extend_from_slice(l.as_bytes());
        }
        out.push(0);
    }

    fn rr(out: &mut Vec<u8>, ty: u16, rd: &[u8]) {
        out.extend_from_slice(&ty.to_be_bytes());
        out.extend_from_slice(&[0, 1, 0, 0, 0, 120]);
        out.extend_from_slice(&(rd.len() as u16).to_be_bytes());
        out.extend_from_slice(rd);
    }

    fn cache() -> Box<MdnsCache> {
        // SAFETY: all fields are integers and byte arrays, zero is valid.
        unsafe { Box::new_zeroed().assume_init() }
    }

    #[test]
    fn reply_becomes_a_service() {
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 4, 0, 0, 0, 0];
        name(&mut p, "_ipp._tcp.local");
        let mut rd = Vec::new();
        name(&mut rd, "Office.Printer._ipp._tcp.local");
        rr(&mut p, 12, &rd);
        name(&mut p, "Office.Printer._ipp._tcp.local");
        let mut rd = vec![0, 0, 0, 0, 0x02, 0x77];
        name(&mut rd, "printer.local");
        rr(&mut p, 33, &rd);
        name(&mut p, "Office.Printer._ipp._tcp.local");
        rr(&mut p, 16, b"\x06rp=ipp\x07ty=Ofi\x01");
        name(&mut p, "printer.local");
        rr(&mut p, 1, &[192, 168, 1, 9]);
        let mut c = cache();
        assert_eq!(parse(&p, &mut c), 0);
        assert_eq!((c.nptr, c.nsrv, c.ntxt, c.na), (1, 1, 1, 1));
        assert_eq!(cs(&c.ptr[0].first), b"Office");
        let mut out: Vec<MdnsSvc> = (0..2).map(|_| unsafe { std::mem::zeroed() }).collect();
        assert_eq!(collect(&c, &[b"_ipp._tcp"], &mut out), 1);
        assert_eq!((cs(&out[0].host), cs(&out[0].addr), out[0].port), (&b"printer.local"[..], &b"192.168.1.9"[..], 631));
        assert_eq!(txt(&out[0].txt, b"RP"), b"ipp");
        assert_eq!(txt(&out[0].txt, b"x"), b"");
        // every truncation is rejected or partial, never a panic
        for cut in 0..p.len() {
            let mut c = cache();
            parse(&p[..cut], &mut c);
        }
    }

    #[test]
    fn compression_loops_and_queries() {
        let mut p = vec![0, 0, 0x84, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0xc0, 12];
        p.extend_from_slice(&[0, 12, 0, 1]);
        assert_eq!(parse(&p, &mut cache()), -1);
        let mut q = [0u8; 64];
        let n = query(&mut q, &[(b"_ipp._tcp.local", 12)], 0x1234);
        assert_eq!(n, 12 + 17 + 4);
        assert_eq!(&q[..6], &[0x12, 0x34, 0, 0, 0, 1]);
        assert_eq!(query(&mut q, &[(b"a..b", 12)], 0), 0);
        assert_eq!(query(&mut q[..20], &[(b"_ipp._tcp.local", 12)], 0), 0);
    }
}
