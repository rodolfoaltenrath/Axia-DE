//! Which names belong to an IP, per app, learned from the DNS responses the system
//! resolver already received (docs/FIREWALL.md, "Names for an address"). The response
//! reader takes untrusted packets; the table is a bounded LRU (max entries app+IP+name
//! in total, DNSMAP_PER_IP names per IP): when it is full, what was used longest ago is
//! forgotten. C ABI of server/src/fw/dnsmap.h.
use crate::fwrules::{app_canon, is_mapped4, norm_domain, FW_NAME_MAX};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_longlong, c_void};

const DNS_MAX_ADDRS: usize = 64;
const DNS_MAX_CNAMES: usize = 16;
const DNSMAP_PER_IP: usize = 8;
const DNSMAP_TTL_MIN: u32 = 60;
const DNSMAP_TTL_MAX: u32 = 86400;
const MAX_HOPS: u32 = 64;

#[repr(C)]
pub struct DnsAddrRec {
    fam: c_int,
    addr: [u8; 16],
    ttl: u32,
    owner: [u8; FW_NAME_MAX],
}

#[repr(C)]
pub struct DnsCname {
    from: [u8; FW_NAME_MAX],
    to: [u8; FW_NAME_MAX],
    ttl: u32,
}

#[repr(C)]
pub struct DnsResponse {
    qname: [u8; FW_NAME_MAX],
    qtype: u16,
    rcode: c_int,
    naddr: c_int,
    ncname: c_int,
    addr: [DnsAddrRec; DNS_MAX_ADDRS],
    cname: [DnsCname; DNS_MAX_CNAMES],
}

fn set(out: &mut [u8], s: &[u8]) {
    out[..s.len()].copy_from_slice(s);
    out[s.len()] = 0;
}

fn cs(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// The name at `off`: (first byte after it in place, the normalized host name or None
/// when it isn't one; the packet is still valid then). Pointers only go backwards from
/// the span being read, at most 64 jumps; labels up to 63, names up to 255 bytes.
fn read_name(p: &[u8], off: usize) -> Option<(usize, Option<Vec<u8>>)> {
    let len = p.len();
    let (mut pos, mut seg, mut wire, mut hops) = (off, off, 0usize, 0u32);
    let mut next = None;
    let mut txt = Vec::new();
    let mut ok = true;
    loop {
        let c = *p.get(pos)? as usize;
        if c == 0 {
            next.get_or_insert(pos + 1);
            break;
        }
        if c & 0xc0 == 0xc0 {
            if pos + 1 >= len {
                return None;
            }
            let ptr = (c & 0x3f) << 8 | p[pos + 1] as usize;
            hops += 1;
            if ptr >= seg || hops > MAX_HOPS {
                return None;
            }
            next.get_or_insert(pos + 2);
            pos = ptr;
            seg = ptr;
            continue;
        }
        if c & 0xc0 != 0 || pos + 1 + c > len {
            return None; // 0x40 and 0x80: obsolete label types
        }
        wire += 1 + c;
        if wire > 255 {
            return None;
        }
        if !txt.is_empty() {
            txt.push(b'.');
        }
        for &ch in &p[pos + 1..pos + 1 + c] {
            let ch = ch.to_ascii_lowercase();
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-' || ch == b'_' {
                txt.push(ch);
            } else {
                ok = false;
                txt.push(b'?');
            }
        }
        pos += 1 + c;
    }
    let host = if ok && !txt.is_empty() && txt.len() <= FW_NAME_MAX { norm_domain(&txt) } else { None };
    Some((next.unwrap(), host))
}

/// Reads a response into `out` (zeroed). false: malformed, truncated, or not a standard
/// response with a single question.
fn parse(p: &[u8], out: &mut DnsResponse) -> bool {
    let len = p.len();
    if !(12..=65535).contains(&len) || p[2] & 0x80 == 0 || (p[2] >> 3) & 0x0f != 0 {
        return false; // QR=1, OPCODE=QUERY
    }
    out.rcode = (p[3] & 0x0f) as c_int;
    let be16 = |i: usize| u16::from_be_bytes([p[i], p[i + 1]]);
    let (qd, an) = (be16(4), be16(6));
    if qd != 1 {
        return false;
    }
    let Some((mut pos, qhost)) = read_name(p, 12) else { return false };
    if let Some(q) = &qhost {
        set(&mut out.qname, q);
    }
    if pos + 4 > len {
        return false;
    }
    out.qtype = be16(pos);
    let qclass = be16(pos + 2);
    pos += 4;
    let usable = qhost.is_some() && qclass == 1 && out.rcode == 0;
    for _ in 0..an {
        let Some((next, owner)) = read_name(p, pos) else { return false };
        pos = next;
        if pos + 10 > len {
            return false;
        }
        let (ty, cls, rdlen) = (be16(pos), be16(pos + 2), be16(pos + 8) as usize);
        let mut ttl = u32::from_be_bytes([p[pos + 4], p[pos + 5], p[pos + 6], p[pos + 7]]);
        if ttl & 0x8000_0000 != 0 {
            ttl = 0; // RFC 2181: TTL with the high bit = 0
        }
        pos += 10;
        if pos + rdlen > len {
            return false;
        }
        let rd = pos;
        pos += rdlen;
        if cls != 1 {
            continue;
        }
        if ty == 1 || ty == 28 {
            if rdlen != if ty == 1 { 4 } else { 16 } {
                return false;
            }
            let Some(owner) = owner.filter(|_| usable) else { continue };
            if out.naddr as usize >= DNS_MAX_ADDRS {
                continue;
            }
            let a = &mut out.addr[out.naddr as usize];
            out.naddr += 1;
            a.fam = if ty == 1 { 4 } else { 6 };
            a.addr[..rdlen].copy_from_slice(&p[rd..rd + rdlen]);
            a.ttl = ttl;
            set(&mut a.owner, &owner);
        } else if ty == 5 {
            // the name has to fill rdata exactly (its pointers may go further back)
            match read_name(&p[..pos], rd) {
                Some((tnext, to)) if tnext == pos => {
                    let (Some(owner), Some(to)) = (owner.filter(|_| usable), to) else { continue };
                    if out.ncname as usize >= DNS_MAX_CNAMES {
                        continue;
                    }
                    let c = &mut out.cname[out.ncname as usize];
                    out.ncname += 1;
                    set(&mut c.from, &owner);
                    set(&mut c.to, &to);
                    c.ttl = ttl;
                }
                _ => return false,
            }
        }
    }
    true
}

fn new_response() -> Box<DnsResponse> {
    // SAFETY: integers and byte arrays only.
    unsafe { Box::new_zeroed().assume_init() }
}

// ---- the table ----

struct Entry {
    prev: Option<usize>, // global LRU: head = most recent
    next: Option<usize>,
    group: Vec<u8>,
    expires: i64,
    name: Vec<u8>,
}

pub struct DnsMap {
    slab: Vec<Option<Entry>>,
    free: Vec<usize>,
    groups: HashMap<Vec<u8>, Vec<usize>>, // key → its entries, most recent first
    head: Option<usize>,
    tail: Option<usize>,
    n: usize,
    max: usize,
}

/// Key app\0fam addr (IPv4 mapped into IPv6 counts as IPv4).
fn make_key(app: &[u8], fam: c_int, addr: &[u8]) -> Option<Vec<u8>> {
    let c = app_canon(app)?;
    let (fam, a) = if fam == 6 && is_mapped4(addr) { (4, &addr[12..16]) } else { (fam, addr) };
    let n = match fam {
        4 => 4,
        6 => 16,
        _ => return None,
    };
    let mut k = c;
    k.push(0);
    k.push(fam as u8);
    k.extend_from_slice(&a[..n]);
    Some(k)
}

impl DnsMap {
    fn new(max: usize) -> Self {
        let max = if max == 0 { 4096 } else { max.min(1 << 20) };
        DnsMap { slab: Vec::new(), free: Vec::new(), groups: HashMap::new(), head: None, tail: None, n: 0, max }
    }
    fn e(&self, i: usize) -> &Entry {
        self.slab[i].as_ref().unwrap()
    }
    fn em(&mut self, i: usize) -> &mut Entry {
        self.slab[i].as_mut().unwrap()
    }
    fn unlink(&mut self, i: usize) {
        let (prev, next) = (self.e(i).prev, self.e(i).next);
        match prev {
            Some(p) => self.em(p).next = next,
            None => self.head = next,
        }
        match next {
            Some(n) => self.em(n).prev = prev,
            None => self.tail = prev,
        }
        let e = self.em(i);
        e.prev = None;
        e.next = None;
    }
    fn front(&mut self, i: usize) {
        let head = self.head;
        let e = self.em(i);
        e.prev = None;
        e.next = head;
        match head {
            Some(h) => self.em(h).prev = Some(i),
            None => self.tail = Some(i),
        }
        self.head = Some(i);
    }
    /// Removes an entry from everything; an emptied group goes too if `drop_group`.
    fn del(&mut self, i: usize, drop_group: bool) {
        let key = self.e(i).group.clone();
        if let Some(g) = self.groups.get_mut(&key) {
            g.retain(|&x| x != i);
        }
        self.unlink(i);
        self.slab[i] = None;
        self.free.push(i);
        self.n -= 1;
        if drop_group && self.groups.get(&key).is_some_and(Vec::is_empty) {
            self.groups.remove(&key);
        }
    }
    /// The entry becomes the most recent of its group and of the whole table.
    fn touch(&mut self, i: usize) {
        let key = self.e(i).group.clone();
        if let Some(g) = self.groups.get_mut(&key) {
            if g.first() != Some(&i) {
                g.retain(|&x| x != i);
                g.insert(0, i);
            }
        }
        self.unlink(i);
        self.front(i);
    }

    fn add(&mut self, app: &[u8], name: &[u8], fam: c_int, addr: &[u8], ttl: u32, now: i64) -> bool {
        let Some(key) = make_key(app, fam, addr) else { return false };
        if name.len() > FW_NAME_MAX {
            return false;
        }
        let Some(nm) = norm_domain(name) else { return false };
        let exp = now + ttl.clamp(DNSMAP_TTL_MIN, DNSMAP_TTL_MAX) as i64;
        if let Some(g) = self.groups.get(&key) {
            if let Some(&i) = g.iter().find(|&&i| self.e(i).name == nm) {
                self.em(i).expires = exp; // the newest answer wins
                self.touch(i);
                return true;
            }
        }
        // table full: forget what was used longest ago (it may belong to this same group)
        while self.n >= self.max {
            let Some(t) = self.tail else { break };
            self.del(t, true);
        }
        if let Some(g) = self.groups.get(&key) {
            if g.len() >= DNSMAP_PER_IP {
                let old = *g.last().unwrap(); // the group's tail is the least used
                self.del(old, false);
            }
        }
        let e = Entry { prev: None, next: None, group: key.clone(), expires: exp, name: nm };
        let i = match self.free.pop() {
            Some(i) => {
                self.slab[i] = Some(e);
                i
            }
            None => {
                self.slab.push(Some(e));
                self.slab.len() - 1
            }
        };
        self.groups.entry(key).or_default().insert(0, i);
        self.front(i);
        self.n += 1;
        true
    }

    fn learn(&mut self, app: &[u8], pkt: &[u8], now: i64) -> c_int {
        let mut r = new_response();
        if !parse(pkt, &mut r) {
            return -1;
        }
        let mut added = 0;
        let qname = cs(&r.qname).to_vec();
        if qname.is_empty() || r.rcode != 0 {
            return 0;
        }
        // the chain from the question: qname → CNAME → … (no repeats, at most DNS_MAX_CNAMES)
        let mut chain = vec![qname];
        let mut cttl = u32::MAX;
        for _ in 0..DNS_MAX_CNAMES {
            let last = chain.last().unwrap().clone();
            let Some(found) = r.cname[..r.ncname as usize].iter().find(|c| cs(&c.from) == last.as_slice()) else { break };
            let to = cs(&found.to).to_vec();
            if chain.contains(&to) {
                break; // CNAME loop
            }
            cttl = cttl.min(found.ttl);
            chain.push(to);
        }
        for a in &r.addr[..r.naddr as usize] {
            if !chain.iter().any(|c| c.as_slice() == cs(&a.owner)) {
                continue; // stray records: they don't come from the question
            }
            let ttl = a.ttl.min(cttl);
            for name in &chain {
                if self.add(app, name, a.fam, &a.addr, ttl, now) {
                    added += 1;
                }
            }
        }
        added
    }

    /// Current names of (app, IP), from most recent to oldest.
    fn lookup(&mut self, app: &[u8], fam: c_int, addr: &[u8], now: i64) -> Vec<Vec<u8>> {
        let Some(key) = make_key(app, fam, addr) else { return Vec::new() };
        let Some(g) = self.groups.get(&key).cloned() else { return Vec::new() };
        for i in g {
            if self.e(i).expires <= now {
                self.del(i, false);
            }
        }
        let g = self.groups.get(&key).cloned().unwrap_or_default();
        if g.is_empty() {
            self.groups.remove(&key);
            return Vec::new();
        }
        let v: Vec<usize> = g.into_iter().take(DNSMAP_PER_IP).collect();
        let names = v.iter().map(|&i| self.e(i).name.clone()).collect();
        // they were used: to the front of the LRU, keeping their order
        for &i in v.iter().rev() {
            self.unlink(i);
            self.front(i);
        }
        names
    }

    fn lru(&self) -> Vec<usize> {
        let mut v = Vec::with_capacity(self.n);
        let mut c = self.head;
        while let Some(i) = c {
            v.push(i);
            c = self.e(i).next;
        }
        v
    }

    fn expire(&mut self, now: i64) {
        for i in self.lru() {
            if self.slab[i].is_some() && self.e(i).expires <= now {
                self.del(i, true);
            }
        }
    }

    fn forget_app(&mut self, app: &[u8]) {
        let Some(mut c) = app_canon(app) else { return };
        c.push(0);
        for i in self.lru() {
            if self.slab[i].as_ref().is_some_and(|e| e.group.len() > c.len() && e.group.starts_with(&c)) {
                self.del(i, true);
            }
        }
    }
}

// ---- C ABI ----

unsafe fn cstr<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() { b"" } else { std::ffi::CStr::from_ptr(p).to_bytes() }
}
unsafe fn addr16<'a>(fam: c_int, a: *const u8) -> &'a [u8] {
    // the C read 16 bytes for an IPv6 address and 4 for IPv4 (after the mapped check)
    std::slice::from_raw_parts(a, if fam == 6 { 16 } else { 4 })
}

/// # Safety
/// `pkt` covers `len` bytes; `out` is a DnsResponse.
#[no_mangle]
pub unsafe extern "C" fn dns_parse_response(pkt: *const u8, len: usize, out: *mut DnsResponse) -> c_int {
    if pkt.is_null() || out.is_null() {
        return -1;
    }
    std::ptr::write_bytes(out, 0, 1);
    if parse(std::slice::from_raw_parts(pkt, len), &mut *out) { 0 } else { -1 }
}
#[no_mangle]
pub extern "C" fn dnsmap_new(max: usize) -> *mut DnsMap {
    Box::into_raw(Box::new(DnsMap::new(max)))
}
/// # Safety
/// `m` NULL or from dnsmap_new.
#[no_mangle]
pub unsafe extern "C" fn dnsmap_free(m: *mut DnsMap) {
    if !m.is_null() {
        drop(Box::from_raw(m));
    }
}
/// # Safety
/// `m` NULL or a map.
#[no_mangle]
pub unsafe extern "C" fn dnsmap_count(m: *const DnsMap) -> usize {
    m.as_ref().map_or(0, |m| m.n)
}
/// # Safety
/// `m` NULL or a map; strings NULL or C strings; `addr` NULL or the address.
#[no_mangle]
pub unsafe extern "C" fn dnsmap_add(
    m: *mut DnsMap,
    app: *const c_char,
    name: *const c_char,
    fam: c_int,
    addr: *const u8,
    ttl: u32,
    now: c_longlong,
) -> c_int {
    let Some(m) = m.as_mut() else { return -1 };
    if app.is_null() || name.is_null() || addr.is_null() || (fam != 4 && fam != 6) {
        return -1;
    }
    let name = std::slice::from_raw_parts(name.cast::<u8>(), libc::strnlen(name, FW_NAME_MAX + 1));
    if m.add(cstr(app), name, fam, addr16(fam, addr), ttl, now) { 0 } else { -1 }
}
/// # Safety
/// `m` NULL or a map; `app` a C string; `pkt` covers `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn dnsmap_learn(m: *mut DnsMap, app: *const c_char, pkt: *const u8, len: usize, now: c_longlong) -> c_int {
    let Some(m) = m.as_mut() else { return -1 };
    if pkt.is_null() {
        return -1;
    }
    m.learn(cstr(app), std::slice::from_raw_parts(pkt, len), now)
}
/// # Safety
/// `m` NULL or a map; `out` has room for `max` names.
#[no_mangle]
pub unsafe extern "C" fn dnsmap_lookup(
    m: *mut DnsMap,
    app: *const c_char,
    fam: c_int,
    addr: *const u8,
    now: c_longlong,
    out: *mut [u8; FW_NAME_MAX],
    max: c_int,
) -> c_int {
    let Some(m) = m.as_mut() else { return 0 };
    if app.is_null() || addr.is_null() || (fam != 4 && fam != 6) {
        return 0;
    }
    let names = m.lookup(cstr(app), fam, addr16(fam, addr), now);
    let mut w = 0;
    for n in names.iter().take(max.max(0) as usize) {
        set(&mut *out.add(w), n);
        w += 1;
    }
    w as c_int
}
/// # Safety
/// `m` NULL or a map.
#[no_mangle]
pub unsafe extern "C" fn dnsmap_expire(m: *mut DnsMap, now: c_longlong) {
    if let Some(m) = m.as_mut() {
        m.expire(now);
    }
}
/// # Safety
/// `m` NULL or a map; `app` NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn dnsmap_forget_app(m: *mut DnsMap, app: *const c_char) {
    if let Some(m) = m.as_mut() {
        if !app.is_null() {
            m.forget_app(cstr(app));
        }
    }
}
/// Walks an app's addresses with their current names (the LRU doesn't change).
///
/// # Safety
/// `m` NULL or a map; `cb` a valid callback.
#[no_mangle]
pub unsafe extern "C" fn dnsmap_each(
    m: *mut DnsMap,
    app: *const c_char,
    now: c_longlong,
    cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const u8, *const [u8; FW_NAME_MAX], c_int) -> c_int>,
    u: *mut c_void,
) -> c_int {
    let (Some(m), Some(cb)) = (m.as_ref(), cb) else { return 0 };
    if app.is_null() {
        return 0;
    }
    let Some(mut c) = app_canon(cstr(app)) else { return 0 };
    c.push(0);
    let cl = c.len();
    let mut groups = 0;
    for (key, g) in &m.groups {
        if key.len() <= cl + 1 || !key.starts_with(&c) {
            continue;
        }
        let fam = key[cl] as c_int;
        if (fam != 4 && fam != 6) || key.len() != cl + 1 + if fam == 4 { 4 } else { 16 } {
            continue;
        }
        let names: Vec<[u8; FW_NAME_MAX]> = g
            .iter()
            .take(DNSMAP_PER_IP)
            .filter(|&&i| m.e(i).expires > now)
            .map(|&i| {
                let mut b = [0u8; FW_NAME_MAX];
                set(&mut b, &m.e(i).name);
                b
            })
            .collect();
        if names.is_empty() {
            continue;
        }
        groups += 1;
        if cb(u, fam, key[cl + 1..].as_ptr(), names.as_ptr(), names.len() as c_int) != 0 {
            break;
        }
    }
    groups
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

    fn response() -> Vec<u8> {
        let mut p = vec![0, 1, 0x81, 0x80, 0, 1, 0, 3, 0, 0, 0, 0];
        name(&mut p, "www.Example.com");
        p.extend_from_slice(&[0, 1, 0, 1]);
        // CNAME www.example.com → cdn.example.net
        p.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 30]);
        let mut rd = Vec::new();
        name(&mut rd, "cdn.example.net");
        p.extend_from_slice(&(rd.len() as u16).to_be_bytes());
        p.extend_from_slice(&rd);
        let cdn = 12 + 17 + 4 + 12;
        // two A records for cdn.example.net
        for last in [7u8, 8] {
            p.extend_from_slice(&[0xc0, cdn as u8, 0, 1, 0, 1, 0, 0, 1, 0, 0, 4, 192, 0, 2, last]);
        }
        p
    }

    #[test]
    fn learns_the_chain_and_looks_it_up() {
        let mut r = new_response();
        let p = response();
        assert!(parse(&p, &mut r));
        assert_eq!((cs(&r.qname), r.naddr, r.ncname), (&b"www.example.com"[..], 2, 1));
        let mut m = DnsMap::new(0);
        assert_eq!(m.learn(b"desktop:firefox", &p, 100), 4);
        let names = m.lookup(b"desktop:firefox", 4, &[192, 0, 2, 7], 101);
        assert_eq!(names, vec![b"cdn.example.net".to_vec(), b"www.example.com".to_vec()]);
        assert!(m.lookup(b"desktop:other", 4, &[192, 0, 2, 7], 101).is_empty());
        assert!(m.lookup(b"desktop:firefox", 4, &[192, 0, 2, 7], 100 + 60).is_empty(), "the TTL is at least 60 s");
        m.forget_app(b"desktop:firefox");
        assert_eq!(m.n, 0, "forgetting the app drops all of its addresses");
        for cut in 0..p.len() {
            assert!(!parse(&p[..cut], &mut new_response()) || cut == p.len());
        }
    }

    #[test]
    fn the_table_is_bounded() {
        let mut m = DnsMap::new(5);
        for i in 0..20u8 {
            assert!(m.add(b"lordos", format!("n{i}.example").as_bytes(), 4, &[10, 0, 0, i % 3], 600, 0));
        }
        assert_eq!(m.n, 5);
        assert!(m.groups.values().all(|g| g.len() <= DNSMAP_PER_IP));
    }
}
