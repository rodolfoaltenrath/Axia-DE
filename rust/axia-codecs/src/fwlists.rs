//! Subscribed blocklists (docs/FIREWALL.md, "Blocklists"): the raw parsers that run in
//! axia-dl on untrusted bytes, the set (exact names, names with every subdomain and
//! networks, each with the tag of its list) and the compiled file. C ABI of
//! server/src/fw/fwlists.h.
//!
//! The set keeps no names: a name is its 64-bit hash, seeded per process (so nobody can
//! craft a name that collides on purpose), sorted for binary search, with the tag of the
//! list it came from. Networks are kept whole.
use crate::fwrules::{addr_is_lan, norm_domain, ntop, parse_dest, zeroed_rule, FwRule, FW_DST_CIDR, FW_NAME_MAX};
use std::collections::HashSet;
use std::ffi::{c_char, c_int, c_long, c_void};
use std::hash::{BuildHasher, Hasher};
use std::sync::OnceLock;

const FWL_RAW_MAX: usize = 16 << 20;
const FWL_LINE_MAX: usize = 1024;
const FWL_LIST_DOMAINS: usize = 400_000;
const FWL_LIST_NETS: usize = 100_000;
const FWL_MAX_DOMAINS: usize = 600_000;
const FWL_MAX_NETS: usize = 150_000;
const FWL_FILE_MAX: usize = 32 << 20;
const FWL_HEADER: &[u8] = b"lordos-fwlists 1";
const FMT_HOSTS: c_int = 1;
const FMT_DOMAINS: c_int = 2;
const FMT_IPS: c_int = 3;

/// The per-process seeded 64-bit hash of a name.
fn hash(b: &[u8]) -> u64 {
    static SEED: OnceLock<std::collections::hash_map::RandomState> = OnceLock::new();
    let mut h = SEED.get_or_init(Default::default).build_hasher();
    h.write(b);
    h.finish()
}

#[derive(Clone, Copy)]
struct NameEnt {
    h: u64,
    seq: u32,
    tag: u16,
}

#[derive(Clone, Copy)]
struct NetEnt {
    k: [u8; 18], // family, prefix, network
    tag: u16,
    seq: u32,
}

pub struct FwLists {
    ex: Vec<NameEnt>,
    sub: Vec<NameEnt>,
    net: Vec<NetEnt>,
    seq: u32,
    sorted: bool,
    has4: [bool; 33],
    has6: [bool; 129],
}

impl Default for FwLists {
    fn default() -> Self {
        FwLists { ex: Vec::new(), sub: Vec::new(), net: Vec::new(), seq: 0, sorted: false, has4: [false; 33], has6: [false; 129] }
    }
}

fn net_key(fam: c_int, addr: &[u8], prefix: c_int) -> [u8; 18] {
    let mut k = [0u8; 18];
    k[0] = fam as u8;
    k[1] = prefix as u8;
    let bytes = if fam == 4 { 4 } else { 16 };
    for i in 0..bytes {
        let bits = prefix - i as c_int * 8;
        let m = if bits >= 8 { 0xff } else if bits <= 0 { 0 } else { 0xffu8 << (8 - bits) };
        k[2 + i] = addr[i] & m;
    }
    k
}

/// Sorts and drops repeated entries (the first one added keeps its tag). How many went.
fn uniq_names(v: &mut Vec<NameEnt>) -> usize {
    if v.len() < 2 {
        return 0;
    }
    v.sort_unstable_by_key(|e| (e.h, e.seq));
    let n = v.len();
    v.dedup_by_key(|e| e.h);
    n - v.len()
}

impl FwLists {
    fn finish(&mut self) -> usize {
        if self.sorted {
            return 0;
        }
        let mut gone = uniq_names(&mut self.ex) + uniq_names(&mut self.sub);
        if self.net.len() > 1 {
            self.net.sort_unstable_by_key(|e| (e.k, e.seq));
            let n = self.net.len();
            self.net.dedup_by_key(|e| e.k);
            gone += n - self.net.len();
        }
        self.sorted = true;
        gone
    }

    fn add_line(&mut self, line: &[u8], tag: u16) -> c_int {
        if tag == 0 {
            return -1;
        }
        match check_line(line) {
            Line::Bad => -1,
            Line::Net(r) => {
                if self.net.len() >= FWL_MAX_NETS * 2 {
                    return -2;
                }
                self.net.push(NetEnt { k: net_key(r.fam, &r.addr, r.prefix), tag, seq: self.seq });
                self.seq = self.seq.wrapping_add(1);
                if r.fam == 4 {
                    self.has4[r.prefix as usize] = true;
                } else {
                    self.has6[r.prefix as usize] = true;
                }
                self.sorted = false;
                0
            }
            Line::Name(sub, name) => {
                if self.ex.len() + self.sub.len() >= FWL_MAX_DOMAINS * 2 {
                    return -2;
                }
                let e = NameEnt { h: hash(&name), seq: self.seq, tag };
                self.seq = self.seq.wrapping_add(1);
                if sub { self.sub.push(e) } else { self.ex.push(e) }
                self.sorted = false;
                0
            }
        }
    }

    fn add_text(&mut self, text: &[u8], tag: u16) -> c_long {
        let mut added = 0;
        for line in lines(text) {
            if line.is_empty() {
                continue;
            }
            match self.add_line(line, tag) {
                -1 => return -1,
                -2 => break,
                _ => added += 1,
            }
        }
        self.finish();
        // the caps are on unique entries
        if self.ex.len() + self.sub.len() > FWL_MAX_DOMAINS {
            let keep = FWL_MAX_DOMAINS.saturating_sub(self.ex.len());
            self.sub.truncate(keep);
        }
        self.ex.truncate(FWL_MAX_DOMAINS);
        self.net.truncate(FWL_MAX_NETS);
        added
    }

    fn match_name(&mut self, inp: &[u8]) -> u16 {
        self.finish();
        if self.ex.is_empty() && self.sub.is_empty() {
            return 0;
        }
        if inp.len() > FW_NAME_MAX {
            return 0;
        }
        let Some(name) = norm_domain(inp) else { return 0 };
        let t = find_name(&self.ex, hash(&name));
        if t != 0 {
            return t;
        }
        let mut sfx: &[u8] = &name;
        loop {
            let t = find_name(&self.sub, hash(sfx));
            if t != 0 {
                return t;
            }
            match sfx.iter().position(|&c| c == b'.') {
                Some(i) => sfx = &sfx[i + 1..],
                None => return 0,
            }
        }
    }

    fn match_addr(&mut self, fam: c_int, addr: &[u8]) -> u16 {
        self.finish();
        if self.net.is_empty() {
            return 0;
        }
        let mut a = [0u8; 16];
        let mut fam = fam;
        if fam == 6 && crate::fwrules::is_mapped4(addr) {
            fam = 4;
            a[..4].copy_from_slice(&addr[12..16]);
        } else {
            let n = if fam == 4 { 4 } else { 16 };
            a[..n].copy_from_slice(&addr[..n]);
        }
        let top = if fam == 4 { 32 } else { 128 };
        for p in (0..=top).rev() {
            let present = if fam == 4 { self.has4[p] } else { self.has6[p] };
            if !present {
                continue;
            }
            let k = net_key(fam, &a, p as c_int);
            let i = self.net.partition_point(|e| e.k < k);
            if i < self.net.len() && self.net[i].k == k {
                return self.net[i].tag;
            }
        }
        0
    }
}

fn find_name(v: &[NameEnt], h: u64) -> u16 {
    let i = v.partition_point(|e| e.h < h);
    if i < v.len() && v[i].h == h { v[i].tag } else { 0 }
}

/// Lines split on '\n'; the piece after a final '\n' is not a line.
fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut v: Vec<&[u8]> = text.split(|&c| c == b'\n').collect();
    if text.ends_with(b"\n") || text.is_empty() {
        v.pop();
    }
    v.into_iter()
}

/// A name with at least one dot, normalized.
fn good_name(s: &[u8]) -> Option<Vec<u8>> {
    if s.is_empty() || s.len() > FW_NAME_MAX || s.contains(&0) {
        return None;
    }
    norm_domain(s).filter(|n| n.contains(&b'.'))
}

fn parse_net(s: &[u8]) -> Option<FwRule> {
    if s.is_empty() || s.len() >= 80 {
        return None;
    }
    let s = &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]; // the C read it as a C string
    if !s.iter().any(|&c| c == b':' || c == b'/' || c == b'.') {
        return None;
    }
    let mut r = zeroed_rule();
    (parse_dest(s, &mut r) && r.kind == FW_DST_CIDR).then_some(r)
}

/// Networks a blocklist may never carry: the LAN and anything broader than /8 (IPv4) or /16 (IPv6).
fn net_usable(r: &FwRule) -> bool {
    r.prefix >= if r.fam == 4 { 8 } else { 16 } && !addr_is_lan(r.fam, &r.addr)
}

enum Line {
    Bad,
    Name(bool, Vec<u8>),
    Net(FwRule),
}

/// What a normalized line is: "d name", "s name" or "n cidr".
fn check_line(line: &[u8]) -> Line {
    if line.len() < 3 || line[1] != b' ' {
        return Line::Bad;
    }
    let v = &line[2..];
    match line[0] {
        b'd' | b's' => match good_name(v) {
            Some(n) if n == v => Line::Name(line[0] == b's', n), // already normalized
            _ => Line::Bad,
        },
        b'n' => match parse_net(v) {
            Some(r) if net_usable(&r) => Line::Net(r),
            _ => Line::Bad,
        },
        _ => Line::Bad,
    }
}

// ---- raw lists ----

#[repr(C)]
#[derive(Default)]
pub struct FwlStats {
    lines: usize,
    domains: usize,
    nets: usize,
    skipped: usize,
    bad: usize,
    dups: usize,
    truncated: c_int,
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | 0x0b | 0x0c)
}

/// Names a hosts file maps to itself, not blocklist entries.
fn hosts_noise(name: &[u8]) -> bool {
    const V: [&[u8]; 12] = [
        b"localhost",
        b"localhost.localdomain",
        b"local",
        b"broadcasthost",
        b"ip6-localhost",
        b"ip6-loopback",
        b"ip6-localnet",
        b"ip6-mcastprefix",
        b"ip6-allnodes",
        b"ip6-allrouters",
        b"ip6-allhosts",
        b"0.0.0.0",
    ];
    V.contains(&name)
}

/// The sink-hole addresses a hosts-format blocklist points names to.
fn hosts_target(s: &[u8]) -> bool {
    matches!(s, b"0.0.0.0" | b"127.0.0.1" | b"::" | b"::1")
}

/// What the parser writes: the normalized lines, each once.
struct Raw {
    out: Vec<u8>,
    seen: HashSet<Vec<u8>>,
}

impl Raw {
    fn line(&mut self, line: &[u8], count: &mut usize, st: &mut FwlStats) {
        if !self.seen.insert(line.to_vec()) {
            st.dups += 1;
            return;
        }
        self.out.extend_from_slice(line);
        self.out.push(b'\n');
        *count += 1;
    }

    /// One name from a list: written (d/s) or counted as why not. false: the list is full.
    fn name(&mut self, mut s: &[u8], mut sub: bool, st: &mut FwlStats) -> bool {
        if s.len() > 2 && s[0] == b'*' && s[1] == b'.' {
            s = &s[2..];
            sub = true;
        }
        if s.len() > 3 && s[0] == b'|' && s[1] == b'|' && s[s.len() - 1] == b'^' {
            s = &s[2..s.len() - 1];
            sub = true;
        }
        if s.is_empty() || s.len() > FW_NAME_MAX {
            st.bad += 1;
            return true;
        }
        let tmp = s.to_ascii_lowercase();
        if hosts_noise(&tmp) {
            st.skipped += 1;
            return true;
        }
        let Some(name) = norm_domain(&tmp) else {
            st.bad += 1;
            return true;
        };
        if !name.contains(&b'.') || hosts_noise(&name) {
            st.skipped += 1;
            return true;
        }
        if st.domains >= FWL_LIST_DOMAINS {
            return false;
        }
        let mut line = vec![if sub { b's' } else { b'd' }, b' '];
        line.extend_from_slice(&name);
        let mut n = st.domains;
        self.line(&line, &mut n, st);
        st.domains = n;
        true
    }

    fn net(&mut self, s: &[u8], st: &mut FwlStats) -> bool {
        let Some(r) = parse_net(s) else {
            st.bad += 1;
            return true;
        };
        if !net_usable(&r) {
            st.skipped += 1;
            return true;
        }
        if st.nets >= FWL_LIST_NETS {
            return false;
        }
        let ip = ntop(r.fam, &r.addr).unwrap_or_default();
        let mut line = b"n ".to_vec();
        line.extend_from_slice(&ip);
        if r.prefix != if r.fam == 4 { 32 } else { 128 } {
            line.extend_from_slice(format!("/{}", r.prefix).as_bytes());
        }
        let mut n = st.nets;
        self.line(&line, &mut n, st);
        st.nets = n;
        true
    }
}

/// Parses a raw list: the normalized lines, or the error code.
pub fn parse_raw(raw: &[u8], fmt: c_int, sub: bool, st: &mut FwlStats) -> Result<Vec<u8>, &'static str> {
    *st = FwlStats::default();
    if ![FMT_HOSTS, FMT_DOMAINS, FMT_IPS].contains(&fmt) {
        return Err("format");
    }
    if raw.len() > FWL_RAW_MAX {
        return Err("big");
    }
    if raw.contains(&0) {
        return Err("nul");
    }
    let mut o = Raw { out: Vec::new(), seen: HashSet::new() };
    let mut i = if raw.starts_with(b"\xef\xbb\xbf") { 3 } else { 0 }; // a UTF-8 byte order mark
    let len = raw.len();
    while i < len {
        let e = raw[i..].iter().position(|&c| c == b'\n').map_or(len, |k| i + k);
        if e - i > FWL_LINE_MAX {
            return Err("line");
        }
        let mut s = &raw[i..e];
        i = e + 1;
        // comments: whole-line (#, !, ;, //) and, after a space, inline (#)
        while let Some((&c, rest)) = s.split_first() {
            if !is_space(c) {
                break;
            }
            s = rest;
        }
        if s.is_empty() || matches!(s[0], b'#' | b'!' | b';') || s.starts_with(b"//") {
            continue;
        }
        if let Some(k) = (1..s.len()).find(|&k| s[k] == b'#' && is_space(s[k - 1])) {
            s = &s[..k];
        }
        if fmt == FMT_IPS {
            if let Some(k) = s.iter().position(|&c| c == b';') {
                s = &s[..k];
            }
        }
        while let Some((&c, rest)) = s.split_last() {
            if !is_space(c) {
                break;
            }
            s = rest;
        }
        if s.is_empty() {
            continue;
        }
        st.lines += 1;
        // tokens
        let toks: Vec<&[u8]> = s.split(|&c| is_space(c)).filter(|t| !t.is_empty()).collect();
        let control = s.iter().any(|&c| c < 0x20 && !is_space(c));
        if toks.len() > 16 || toks.is_empty() || control {
            st.bad += 1;
            continue;
        }
        let more = match fmt {
            FMT_HOSTS => {
                if toks.len() < 2 || !hosts_target(toks[0]) {
                    st.bad += 1;
                    continue;
                }
                toks[1..].iter().all(|t| o.name(t, sub, st))
            }
            FMT_DOMAINS => {
                if toks.len() != 1 {
                    st.bad += 1;
                    continue;
                }
                o.name(toks[0], sub, st)
            }
            _ => o.net(toks[0], st), // other columns (counts, dates) are ignored
        };
        if !more {
            st.truncated = 1;
            break;
        }
    }
    if st.bad > 20 && st.bad * 10 > st.lines {
        return Err("format");
    }
    if st.domains == 0 && st.nets == 0 {
        return Err("empty");
    }
    Ok(o.out)
}

// ---- C ABI ----

unsafe fn bytes<'a>(p: *const c_char, len: usize) -> &'a [u8] {
    if p.is_null() || len == 0 { &[] } else { std::slice::from_raw_parts(p.cast(), len) }
}

unsafe fn put_err(err: *mut c_char, errlen: usize, msg: &str) {
    if !err.is_null() && errlen > 0 {
        let n = msg.len().min(errlen - 1);
        std::ptr::copy_nonoverlapping(msg.as_ptr(), err.cast::<u8>(), n);
        *err.add(n) = 0;
    }
}

/// The lists are read from const pointers, but a lookup sorts what was added since the
/// last sort, like the C did.
unsafe fn lists<'a>(l: *const FwLists) -> Option<&'a mut FwLists> {
    (l as *mut FwLists).as_mut()
}

/// # Safety
/// `raw` covers `len` bytes; `out`, `outlen` are valid; `st` NULL or valid; `err` NULL or `errlen` bytes.
#[no_mangle]
pub unsafe extern "C" fn fwl_parse_raw(
    raw: *const c_char,
    len: usize,
    fmt: c_int,
    sub: c_int,
    out: *mut *mut c_char,
    outlen: *mut usize,
    st: *mut FwlStats,
    err: *mut c_char,
    errlen: usize,
) -> c_int {
    let mut local = FwlStats::default();
    let st = st.as_mut().unwrap_or(&mut local);
    *out = std::ptr::null_mut();
    *outlen = 0;
    match parse_raw(bytes(raw, len), fmt, sub != 0, st) {
        Ok(v) => {
            let p = libc::malloc(v.len() + 1).cast::<u8>();
            if p.is_null() {
                put_err(err, errlen, "oom");
                return -1;
            }
            std::ptr::copy_nonoverlapping(v.as_ptr(), p, v.len());
            *p.add(v.len()) = 0;
            *out = p.cast();
            *outlen = v.len();
            0
        }
        Err(code) => {
            put_err(err, errlen, code);
            -1
        }
    }
}
/// # Safety
/// `n` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fwl_format_from_name(n: *const c_char) -> c_int {
    if n.is_null() {
        return 0;
    }
    match std::ffi::CStr::from_ptr(n).to_bytes() {
        b"hosts" => FMT_HOSTS,
        b"domains" => FMT_DOMAINS,
        b"ips" => FMT_IPS,
        _ => 0,
    }
}
#[no_mangle]
pub extern "C" fn fwl_new() -> *mut FwLists {
    Box::into_raw(Box::default())
}
/// # Safety
/// `l` is NULL or from fwl_new / fwl_load_text.
#[no_mangle]
pub unsafe extern "C" fn fwl_free(l: *mut FwLists) {
    if !l.is_null() {
        drop(Box::from_raw(l));
    }
}
/// # Safety
/// `l` NULL or a set; `line` covers `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fwl_add_line(l: *mut FwLists, line: *const c_char, len: usize, tag: u16) -> c_int {
    match l.as_mut() {
        Some(l) if !line.is_null() => l.add_line(bytes(line, len), tag),
        _ => -1,
    }
}
/// # Safety
/// `line` NULL or covers `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fwl_check_line(line: *const c_char, len: usize) -> c_int {
    if line.is_null() || matches!(check_line(bytes(line, len)), Line::Bad) { -1 } else { 0 }
}
/// # Safety
/// `l` NULL or a set.
#[no_mangle]
pub unsafe extern "C" fn fwl_finish(l: *mut FwLists) -> usize {
    l.as_mut().map_or(0, |l| l.finish())
}
/// # Safety
/// `l` is a set; `text` covers `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fwl_add_text(l: *mut FwLists, text: *const c_char, len: usize, tag: u16) -> c_long {
    let Some(l) = l.as_mut() else { return -1 };
    l.add_text(bytes(text, len), tag)
}
/// # Safety
/// `text` covers `len` bytes; `err` NULL or `errlen` bytes.
#[no_mangle]
pub unsafe extern "C" fn fwl_load_text(text: *const c_char, len: usize, err: *mut c_char, errlen: usize) -> *mut FwLists {
    if text.is_null() || len > FWL_FILE_MAX {
        put_err(err, errlen, "not a list file");
        return std::ptr::null_mut();
    }
    let t = bytes(text, len);
    if t.contains(&0) {
        put_err(err, errlen, "not a list file");
        return std::ptr::null_mut();
    }
    let hl = FWL_HEADER.len();
    if len < hl || &t[..hl] != FWL_HEADER || (len > hl && t[hl] != b'\n') {
        put_err(err, errlen, "no header");
        return std::ptr::null_mut();
    }
    let mut l = FwLists::default();
    let mut lineno = 1usize;
    let mut i = hl + 1;
    while i < len {
        let e = t[i..].iter().position(|&c| c == b'\n').map_or(len, |k| i + k);
        lineno += 1;
        let r = if e > i { l.add_line(&t[i..e], 1) } else { 0 };
        if r == -1 || r == -2 {
            let msg = if r == -2 { format!("too many entries (line {lineno})") } else { format!("invalid line {lineno}") };
            put_err(err, errlen, &msg);
            return std::ptr::null_mut();
        }
        i = e + 1;
    }
    l.finish();
    if l.ex.len() + l.sub.len() > FWL_MAX_DOMAINS || l.net.len() > FWL_MAX_NETS {
        put_err(err, errlen, "too many entries");
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(l))
}
/// # Safety
/// `l` NULL or a set.
#[no_mangle]
pub unsafe extern "C" fn fwl_domains(l: *const FwLists) -> usize {
    lists(l).map_or(0, |l| {
        l.finish();
        l.ex.len() + l.sub.len()
    })
}
/// # Safety
/// `l` NULL or a set.
#[no_mangle]
pub unsafe extern "C" fn fwl_nets(l: *const FwLists) -> usize {
    lists(l).map_or(0, |l| {
        l.finish();
        l.net.len()
    })
}
/// # Safety
/// `l` NULL or a set; `name` NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fwl_match_name(l: *const FwLists, name: *const c_char) -> u16 {
    match lists(l) {
        Some(l) if !name.is_null() => {
            let n = libc::strnlen(name, FW_NAME_MAX + 1);
            l.match_name(std::slice::from_raw_parts(name.cast(), n))
        }
        _ => 0,
    }
}
/// # Safety
/// `l` NULL or a set; `addr` NULL or 4 (fam 4) / 16 (fam 6) bytes.
#[no_mangle]
pub unsafe extern "C" fn fwl_match_addr(l: *const FwLists, fam: c_int, addr: *const u8) -> u16 {
    match lists(l) {
        Some(l) if !addr.is_null() && (fam == 4 || fam == 6) => {
            l.match_addr(fam, std::slice::from_raw_parts(addr, if fam == 4 { 4 } else { 16 }))
        }
        _ => 0,
    }
}
/// # Safety
/// `l` NULL or a set; `cb` a valid callback.
#[no_mangle]
pub unsafe extern "C" fn fwl_net_each(
    l: *const FwLists,
    cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const u8, c_int) -> c_int>,
    u: *mut c_void,
) {
    let (Some(l), Some(cb)) = (lists(l), cb) else { return };
    l.finish();
    for e in &l.net {
        let mut a = [0u8; 16];
        let n = if e.k[0] == 4 { 4 } else { 16 };
        a[..n].copy_from_slice(&e.k[2..2 + n]);
        if cb(u, e.k[0] as c_int, a.as_ptr(), e.k[1] as c_int) != 0 {
            return;
        }
    }
}
/// # Safety
/// `l` NULL or a set; `names` has `n` C strings (or NULLs).
#[no_mangle]
pub unsafe extern "C" fn fwl_block_names(l: *const FwLists, names: *const *const c_char, n: c_int) -> u16 {
    let mut first = 0;
    for i in 0..n.max(0) as usize {
        let t = fwl_match_name(l, *names.add(i));
        if t == 0 {
            return 0; // an unlisted name shares the address
        }
        if first == 0 {
            first = t;
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_hosts_and_the_set() {
        let mut st = FwlStats::default();
        let raw = b"\xef\xbb\xbf# list\n0.0.0.0 ads.example.com tracker.example.net # inline\n127.0.0.1 localhost\n0.0.0.0 ADS.example.com\n1.2.3.4 x.example\n";
        let out = parse_raw(raw, FMT_HOSTS, false, &mut st).unwrap();
        assert_eq!(out, b"d ads.example.com\nd tracker.example.net\n");
        assert_eq!((st.lines, st.domains, st.skipped, st.bad, st.dups), (4, 2, 1, 1, 1));
        let mut l = FwLists::default();
        assert_eq!(l.add_text(&out, 3), 2);
        assert_eq!(l.add_line(b"s example.org", 4), 0);
        assert_eq!(l.add_line(b"n 203.0.113.0/24", 5), 0);
        assert_eq!(l.add_line(b"n 10.0.0.0/8", 5), -1, "LAN networks are refused");
        assert_eq!(l.match_name(b"ads.example.com"), 3);
        assert_eq!(l.match_name(b"cdn.www.example.org"), 4);
        assert_eq!(l.match_name(b"example.com"), 0);
        assert_eq!(l.match_addr(4, &[203, 0, 113, 9]), 5);
        assert_eq!(parse_raw(b"bad\n", FMT_DOMAINS, false, &mut st), Err("empty"));
        assert_eq!(parse_raw(b"a\0", FMT_DOMAINS, false, &mut st), Err("nul"));
    }
}
