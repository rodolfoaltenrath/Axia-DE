//! Per-app network rules: the rule store and the decider (docs/FIREWALL.md, "Rules").
//! It doesn't touch the kernel or the network. C ABI of server/src/fw/rules.h; the
//! JSON file (fw_to_json, fw_from_json, fw_save, fw_load) stays in rules_json.c on
//! top of this API.
//!
//! Decision order (fw_decide), from strongest to weakest:
//!   1. the app's restrictions (FW_R_*): always deny;
//!   2. explicit rules: the one with the most specific destination wins; with the same
//!      specificity, deny beats allow;
//!      2b. a subscribed blocklist (FwQuery.listed) denies, unless the winning rule
//!          names the destination (a domain, an address or a network);
//!   3. trusted app: allow without logging or asking;
//!   4. mode: silent = allow and log, ask = pending, off = allow, deny = deny and log
//!      (loopback stays allowed).
//!
//! Per app: a table of exact domains, one of wildcard suffixes (a name's labels are
//! walked from right to left) and one per (family, prefix, network) for addresses and
//! networks, with the prefix lengths in use, so a lookup is O(key length).
use crate::lanparse::cstr;
use std::collections::{BTreeMap, HashMap};
use std::ffi::{c_char, c_int, c_void};
use std::ptr;

const FW_APP_MAX: usize = 128;
const FW_NAME_MAX: usize = 254;
const FW_MAX_RULES: usize = 65536;
const FW_MAX_APPS: usize = 4096;

const FW_OUT: c_int = 0;
const FW_IN: c_int = 1;
const FW_PROTO_ANY: c_int = 0;
const FW_PROTO_TCP: c_int = 1;
const FW_PROTO_UDP: c_int = 2;
const FW_ALLOW: c_int = 0;
const FW_DENY: c_int = 1;
const FW_LIFE_FOREVER: c_int = 0;
const FW_LIFE_UNTIL_QUIT: c_int = 1;
const FW_LIFE_ONCE: c_int = 2;
const FW_MODE_SILENT: c_int = 0;
const FW_MODE_ASK: c_int = 1;
const FW_MODE_OFF: c_int = 2;
const FW_MODE_DENY: c_int = 3;
const FW_DST_ANY: c_int = 0;
const FW_DST_DOMAIN: c_int = 1;
const FW_DST_CIDR: c_int = 2;
const FW_DST_LAN: c_int = 3;
const FW_DST_INTERNET: c_int = 4;

const FW_R_NO_INTERNET: u32 = 1 << 0;
const FW_R_NO_LAN: u32 = 1 << 1;
const FW_R_NO_NETWORK: u32 = 1 << 2;
const FW_R_NO_INCOMING: u32 = 1 << 3;
const FW_R_ALL: u32 = (1 << 12) - 1;

const FW_V_ALLOW: c_int = 0;
const FW_V_DENY: c_int = 1;
const FW_V_ASK: c_int = 2;
const FW_BY_RESTRICTED: c_int = 0;
const FW_BY_RULE: c_int = 1;
const FW_BY_TRUSTED: c_int = 2;
const FW_BY_MODE: c_int = 3;
const FW_BY_LIST: c_int = 4;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FwRule {
    id: u32,
    app: [u8; FW_APP_MAX],
    dir: c_int,
    kind: c_int,
    wildcard: c_int,
    domain: [u8; FW_NAME_MAX],
    fam: c_int,
    addr: [u8; 16],
    prefix: c_int,
    port_lo: u16,
    port_hi: u16,
    proto: c_int,
    action: c_int,
    life: c_int,
    when: i64,
}

#[repr(C)]
pub struct FwQuery {
    app: *const c_char,
    dir: c_int,
    proto: c_int,
    fam: c_int,
    addr: [u8; 16],
    port: u16,
    hosts: *const *const c_char,
    nhosts: c_int,
    listed: c_int,
}

#[repr(C)]
pub struct FwDecision {
    verdict: c_int,
    reason: c_int,
    rule_id: u32,
    flag: u32,
    record: c_int,
}

fn zeroed_rule() -> FwRule {
    // SAFETY: plain integers and byte arrays.
    unsafe { std::mem::zeroed() }
}

fn cs(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

fn set(out: &mut [u8], s: &[u8]) {
    out[..s.len()].copy_from_slice(s);
    out[s.len()] = 0;
}

// ---- text and addresses ----

fn id_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"._-:@+~".contains(&c)
}

const COMPONENTS: [&[u8]; 6] = [b"mail", b"store", b"print", b"terminal", b"lists", b"lan"];

pub fn component_ok(name: &[u8]) -> bool {
    COMPONENTS.contains(&name)
}

/// Canonical id: "lordos", "desktop:<id>", "lpk:<id>" or an absolute executable path.
pub fn app_canon(inp: &[u8]) -> Option<Vec<u8>> {
    let len = inp.iter().position(|&c| c == 0).unwrap_or(inp.len()).min(FW_APP_MAX);
    if len == 0 || len >= FW_APP_MAX {
        return None;
    }
    let s = &inp[..len];
    // cgroup scopes (sec/cgroup.h): the .lpk one first, since it's the longest
    let scope = s.ends_with(b".scope");
    let pre: Option<(&[u8], &[u8])> = if scope && s.starts_with(b"app-lordos-lpk-") && len > 15 + 6 {
        Some((b"lpk:", &s[15..len - 6]))
    } else if scope && s.starts_with(b"app-lordos-") && len > 11 + 6 {
        Some((b"desktop:", &s[11..len - 6]))
    } else {
        None
    };
    let out = if let Some((p, mid)) = pre {
        if mid.iter().any(|&c| !id_char(c) || c == b':') {
            return None;
        }
        let mut v = p.to_vec();
        v.extend_from_slice(mid);
        if v.len() >= FW_APP_MAX {
            return None;
        }
        v
    } else if s[0] == b'/' {
        // absolute path of an executable (what the Monitor uses if there's no .desktop)
        for seg in s[1..].split(|&c| c == b'/') {
            if seg.is_empty() || seg == b"." || seg == b".." || !seg.iter().all(|&c| id_char(c)) {
                return None;
            }
        }
        s.to_vec()
    } else {
        if !s.iter().all(|&c| id_char(c)) {
            return None;
        }
        if let Some(c) = s.iter().position(|&c| c == b':') {
            if c == 0 || c == len - 1 {
                return None; // "desktop:" without an id, ":x"
            }
        }
        s.to_vec()
    };
    Some(out)
}

/// Domain: ASCII lowercase, labels of 1 to 63 from [a-z0-9-_], 253 in total, an
/// optional trailing dot; the last label can't be only digits (a mistyped IP).
/// `inp` is at most FW_NAME_MAX + 1 bytes long (strnlen in the C).
pub fn norm_domain(inp: &[u8]) -> Option<Vec<u8>> {
    let mut len = inp.len().min(FW_NAME_MAX + 1);
    if len > 0 && inp[len - 1] == b'.' {
        len -= 1;
    }
    if len == 0 || len > 253 {
        return None;
    }
    let mut out = Vec::with_capacity(len);
    let (mut lab, mut last, mut digits) = (0, 0, true);
    for (i, &c) in inp[..len].iter().enumerate() {
        if c == b'.' {
            if lab == 0 {
                return None;
            }
            lab = 0;
            digits = true;
            last = i + 1;
            out.push(b'.');
            continue;
        }
        let c = c.to_ascii_lowercase();
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_') {
            return None;
        }
        if !c.is_ascii_digit() {
            digits = false;
        }
        lab += 1;
        if lab > 63 {
            return None;
        }
        out.push(c);
    }
    if lab == 0 || (digits && len - last > 0) {
        return None;
    }
    Some(out)
}

fn labels(d: &[u8]) -> i32 {
    1 + d.iter().filter(|&&c| c == b'.').count() as i32
}

fn is_mapped4(a: &[u8]) -> bool {
    a[..10].iter().all(|&b| b == 0) && a[10] == 0xff && a[11] == 0xff
}

pub fn addr_is_lan(fam: c_int, a: &[u8]) -> bool {
    if fam == 6 && is_mapped4(a) {
        return addr_is_lan(4, &a[12..]);
    }
    if fam == 4 {
        return a[0] == 0
            || a[0] == 10
            || a[0] == 127
            || (a[0] == 100 && a[1] & 0xc0 == 64)
            || (a[0] == 169 && a[1] == 254)
            || (a[0] == 172 && a[1] & 0xf0 == 16)
            || (a[0] == 192 && a[1] == 168)
            || a[0] & 0xf0 == 224
            || (a[0] == 255 && a[1] == 255 && a[2] == 255 && a[3] == 255);
    }
    if fam == 6 {
        if a[..15].iter().all(|&b| b == 0) && (a[15] == 0 || a[15] == 1) {
            return true; // :: and ::1
        }
        return a[0] & 0xfe == 0xfc || (a[0] == 0xfe && a[1] & 0xc0 == 0x80) || a[0] == 0xff;
    }
    false
}

pub fn addr_is_loopback(fam: c_int, a: &[u8]) -> bool {
    if fam == 6 && is_mapped4(a) {
        return a[12] == 127;
    }
    match fam {
        4 => a[0] == 127,
        6 => a[..15].iter().all(|&b| b == 0) && a[15] == 1,
        _ => false,
    }
}

fn bit_at(a: &[u8], i: usize) -> usize {
    (a[i >> 3] >> (7 - (i & 7)) & 1) as usize
}

fn host_bits_zero(a: &[u8], bytes: usize, prefix: usize) -> bool {
    (prefix..bytes * 8).all(|i| bit_at(a, i) == 0)
}

/// Network bits only (what the C's trie path is made of).
fn masked(a: &[u8; 16], fam: c_int, prefix: usize) -> [u8; 16] {
    let mut m = [0u8; 16];
    let bytes = if fam == 4 { 4 } else { 16 };
    for i in 0..prefix.min(bytes * 8) {
        if bit_at(a, i) == 1 {
            m[i >> 3] |= 0x80 >> (i & 7);
        }
    }
    m
}

fn parse_uint(s: &[u8], max: u32) -> Option<u32> {
    if s.is_empty() || s.len() > 5 || (s.len() > 1 && s[0] == b'0') || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = s.iter().fold(0u32, |v, &c| v * 10 + (c - b'0') as u32);
    (v <= max).then_some(v)
}

extern "C" {
    // glibc's own parsers, so addresses are read exactly as before
    fn inet_pton(af: c_int, src: *const c_char, dst: *mut c_void) -> c_int;
    fn inet_ntop(af: c_int, src: *const c_void, dst: *mut c_char, size: libc::socklen_t) -> *const c_char;
}

fn pton(fam: c_int, ip: &[u8]) -> Option<[u8; 16]> {
    let mut c = ip.to_vec();
    c.push(0);
    let mut a = [0u8; 16];
    let af = if fam == 6 { libc::AF_INET6 } else { libc::AF_INET };
    // SAFETY: c is NUL-terminated, a has room for an IPv6 address.
    (unsafe { inet_pton(af, c.as_ptr().cast(), a.as_mut_ptr().cast()) } == 1).then_some(a)
}

fn ntop(fam: c_int, a: &[u8; 16]) -> Option<Vec<u8>> {
    let mut buf = [0u8; 64];
    let af = if fam == 4 { libc::AF_INET } else { libc::AF_INET6 };
    // SAFETY: buf has 64 bytes.
    let p = unsafe { inet_ntop(af, a.as_ptr().cast(), buf.as_mut_ptr().cast(), buf.len() as libc::socklen_t) };
    (!p.is_null()).then(|| cs(&buf).to_vec())
}

fn parse_cidr(s: &[u8], r: &mut FwRule) -> bool {
    let slash = s.iter().position(|&c| c == b'/');
    let il = slash.unwrap_or(s.len());
    if il == 0 || il >= 64 {
        return false;
    }
    let ip = &s[..il];
    let (mut fam, a) = if ip.contains(&b':') { (6, pton(6, ip)) } else { (4, pton(4, ip)) };
    let Some(mut a) = a else { return false };
    let bits = if fam == 4 { 32 } else { 128 };
    let mut pfx = bits;
    if let Some(sl) = slash {
        match parse_uint(&s[sl + 1..], bits) {
            Some(p) => pfx = p,
            None => return false,
        }
    }
    if fam == 6 && is_mapped4(&a) && pfx >= 96 {
        // ::ffff:a.b.c.d/n is IPv4
        a.copy_within(12..16, 0);
        a[4..].fill(0);
        fam = 4;
        pfx -= 96;
    }
    if !host_bits_zero(&a, if fam == 4 { 4 } else { 16 }, pfx as usize) {
        return false;
    }
    r.kind = FW_DST_CIDR;
    r.fam = fam;
    r.addr = a;
    r.prefix = pfx as c_int;
    true
}

pub fn parse_dest(s: &[u8], r: &mut FwRule) -> bool {
    r.wildcard = 0;
    r.domain[0] = 0;
    r.fam = 0;
    r.prefix = 0;
    r.addr = [0; 16];
    match s {
        b"any" => {
            r.kind = FW_DST_ANY;
            return true;
        }
        b"lan" => {
            r.kind = FW_DST_LAN;
            return true;
        }
        b"internet" => {
            r.kind = FW_DST_INTERNET;
            return true;
        }
        _ => {}
    }
    if s.contains(&b':') || s.contains(&b'/') {
        return parse_cidr(s, r);
    }
    if s.first() == Some(&b'*') {
        if s.get(1) != Some(&b'.') {
            return false;
        }
        let Some(d) = norm_domain(&s[2..]) else { return false };
        set(&mut r.domain, &d);
        r.kind = FW_DST_DOMAIN;
        r.wildcard = 1;
        return true;
    }
    // "1.2.3.4" is an address; if it doesn't parse as one, norm_domain rejects it anyway
    if parse_cidr(s, r) {
        return true;
    }
    let Some(d) = norm_domain(s) else { return false };
    set(&mut r.domain, &d);
    r.kind = FW_DST_DOMAIN;
    true
}

pub fn format_dest(r: &FwRule) -> Option<Vec<u8>> {
    Some(match r.kind {
        FW_DST_ANY => b"any".to_vec(),
        FW_DST_LAN => b"lan".to_vec(),
        FW_DST_INTERNET => b"internet".to_vec(),
        FW_DST_DOMAIN => {
            let mut v = if r.wildcard != 0 { b"*.".to_vec() } else { Vec::new() };
            v.extend_from_slice(cs(&r.domain));
            v
        }
        FW_DST_CIDR => {
            let mut ip = ntop(r.fam, &r.addr)?;
            if r.prefix != if r.fam == 4 { 32 } else { 128 } {
                ip.extend_from_slice(format!("/{}", r.prefix).as_bytes());
            }
            ip
        }
        _ => return None,
    })
}

// ---- restrictions ----

const FLAG_NAMES: [&std::ffi::CStr; 12] = [
    c"no-internet",
    c"no-lan",
    c"no-network",
    c"no-incoming",
    c"no-camera",
    c"no-mic",
    c"no-screen",
    c"no-usb",
    c"no-bluetooth",
    c"no-files",
    c"no-notifications",
    c"no-clipboard",
];

// ---- the store ----

type Key = (u8, u8, [u8; 16]); // family (0 v4, 1 v6), prefix, network

#[derive(Default)]
struct App {
    id: Vec<u8>,
    trusted: bool,
    restricted: u32,
    nrules: usize,
    exact: HashMap<Vec<u8>, Vec<u32>>,
    wild: HashMap<Vec<u8>, Vec<u32>>,
    nets: HashMap<Key, Vec<u32>>,
    prefixes: [BTreeMap<u8, usize>; 2], // prefix length → how many networks use it
    any: Vec<u32>,
    lan: Vec<u32>,
    inet: Vec<u32>,
}

impl App {
    fn holds_nothing(&self) -> bool {
        self.nrules == 0 && !self.trusted && self.restricted == 0
    }
    /// The list of r's destination; create: makes whatever is missing.
    fn list(&mut self, r: &FwRule, create: bool) -> Option<&mut Vec<u32>> {
        match r.kind {
            FW_DST_ANY => Some(&mut self.any),
            FW_DST_LAN => Some(&mut self.lan),
            FW_DST_INTERNET => Some(&mut self.inet),
            FW_DST_DOMAIN => {
                let m = if r.wildcard != 0 { &mut self.wild } else { &mut self.exact };
                let d = cs(&r.domain).to_vec();
                if create { Some(m.entry(d).or_default()) } else { m.get_mut(&d) }
            }
            FW_DST_CIDR => {
                let k = key(r);
                if create && !self.nets.contains_key(&k) {
                    *self.prefixes[k.0 as usize].entry(k.1).or_default() += 1;
                    self.nets.insert(k, Vec::new());
                }
                self.nets.get_mut(&k)
            }
            _ => None,
        }
    }
    /// Deletes r's destination list if it was left empty.
    fn prune(&mut self, r: &FwRule) {
        match r.kind {
            FW_DST_DOMAIN => {
                let m = if r.wildcard != 0 { &mut self.wild } else { &mut self.exact };
                let d = cs(&r.domain);
                if m.get(d).is_some_and(Vec::is_empty) {
                    m.remove(d);
                }
            }
            FW_DST_CIDR => {
                let k = key(r);
                if self.nets.get(&k).is_some_and(Vec::is_empty) {
                    self.nets.remove(&k);
                    let p = &mut self.prefixes[k.0 as usize];
                    if let Some(n) = p.get_mut(&k.1) {
                        *n -= 1;
                        if *n == 0 {
                            p.remove(&k.1);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn key(r: &FwRule) -> Key {
    ((r.fam == 6) as u8, r.prefix as u8, masked(&r.addr, r.fam, r.prefix as usize))
}

pub struct FwStore {
    apps: HashMap<Vec<u8>, App>,
    ids: HashMap<u32, Box<FwRule>>,
    next_id: u32,
    mode: c_int,
}

impl FwStore {
    fn new() -> Self {
        FwStore { apps: HashMap::new(), ids: HashMap::new(), next_id: 0, mode: FW_MODE_SILENT }
    }

    /// Releases the App if it no longer holds anything.
    fn prune_app(&mut self, id: &[u8]) {
        if self.apps.get(id).is_some_and(App::holds_nothing) {
            self.apps.remove(id);
        }
    }

    fn add(&mut self, r: FwRule) -> Option<u32> {
        let app = cs(&r.app).to_vec();
        if !self.apps.contains_key(&app) {
            if self.apps.len() >= FW_MAX_APPS {
                return None;
            }
            self.apps.insert(app.clone(), App { id: app.clone(), ..Default::default() });
        }
        // an existing rule with the same destination, direction, ports and protocol is replaced
        let a = self.apps.get_mut(&app)?;
        let same = a.list(&r, false).and_then(|l| {
            l.iter().copied().find(|id| {
                let x = &self.ids[id];
                x.dir == r.dir && x.port_lo == r.port_lo && x.port_hi == r.port_hi && x.proto == r.proto
            })
        });
        if let Some(id) = same {
            let slot = self.ids.get_mut(&id)?;
            **slot = FwRule { id, ..r };
            return Some(id);
        }
        if self.ids.len() >= FW_MAX_RULES {
            self.prune_app(&app);
            return None;
        }
        loop {
            self.next_id = self.next_id.wrapping_add(1);
            if self.next_id == 0 {
                self.next_id = 1;
            }
            if !self.ids.contains_key(&self.next_id) {
                break;
            }
        }
        let id = self.next_id;
        let a = self.apps.get_mut(&app)?;
        a.list(&r, true)?.insert(0, id);
        a.nrules += 1;
        self.ids.insert(id, Box::new(FwRule { id, ..r }));
        Some(id)
    }

    fn remove(&mut self, id: u32) -> bool {
        let Some(r) = self.ids.remove(&id) else { return false };
        let app = cs(&r.app).to_vec();
        if let Some(a) = self.apps.get_mut(&app) {
            if let Some(l) = a.list(&r, false) {
                l.retain(|&x| x != id);
            }
            a.prune(&r);
            a.nrules -= 1;
        }
        self.prune_app(&app);
        true
    }

    /// The ids of the rules (of one app, or all) in order.
    fn sorted(&self, only: Option<&[u8]>) -> Vec<u32> {
        let mut v: Vec<u32> = self.ids.values().filter(|r| only.is_none_or(|a| cs(&r.app) == a)).map(|r| r.id).collect();
        v.sort_unstable();
        v
    }

    fn remove_where(&mut self, app: &[u8], only_until_quit: bool) {
        for id in self.sorted(Some(app)) {
            if !only_until_quit || self.ids[&id].life == FW_LIFE_UNTIL_QUIT {
                self.remove(id);
            }
        }
    }

    fn decide(&mut self, q: &FwQuery, app: &[u8], hosts: &[Option<&[u8]>]) -> FwDecision {
        let mut d = FwDecision { verdict: FW_V_DENY, reason: FW_BY_RESTRICTED, rule_id: 0, flag: FW_R_NO_NETWORK, record: 0 };
        let mut fam = q.fam;
        let mut addr = [0u8; 16];
        if fam == 4 {
            addr[..4].copy_from_slice(&q.addr[..4]);
        } else if fam == 6 {
            addr = q.addr;
            if is_mapped4(&addr) {
                addr.copy_within(12..16, 0);
                addr[4..].fill(0);
                fam = 4;
            }
        } else {
            fam = 0;
        }
        let lan = if fam != 0 { addr_is_lan(fam, &addr) as i32 } else { -1 }; // -1: unknown address
        let a = self.apps.get(app);

        // 1. restrictions
        let rf = a.map_or(0, |a| a.restricted);
        let hit = if rf & FW_R_NO_NETWORK != 0 {
            FW_R_NO_NETWORK
        } else if q.dir == FW_IN && rf & FW_R_NO_INCOMING != 0 {
            FW_R_NO_INCOMING
        } else if rf & FW_R_NO_INTERNET != 0 && lan != 1 {
            FW_R_NO_INTERNET // unknown: internet
        } else if rf & FW_R_NO_LAN != 0 && lan == 1 {
            FW_R_NO_LAN
        } else {
            0
        };
        if hit != 0 {
            d.flag = hit;
            d.record = 1;
            return d;
        }
        d.flag = 0;

        // 2. explicit rules
        let (mut decided, mut once) = (false, None);
        if let Some(a) = a.filter(|a| a.nrules != 0) {
            let mut p = Pick { q, best: None, dst: 0, port: 0, proto: 0 };
            for h in hosts.iter().flatten() {
                let Some(name) = (h.len() <= FW_NAME_MAX).then(|| norm_domain(h)).flatten() else { continue };
                let mut nl = labels(&name);
                if !a.exact.is_empty() {
                    p.consider(&self.ids, a.exact.get(&name), SC_EXACT + nl);
                }
                if !a.wild.is_empty() {
                    // labels from right to left: each suffix of the name, from the longest to the TLD
                    let mut sfx: &[u8] = &name;
                    loop {
                        p.consider(&self.ids, a.wild.get(sfx), SC_WILD + nl);
                        nl -= 1;
                        match sfx.iter().position(|&c| c == b'.') {
                            Some(i) => sfx = &sfx[i + 1..],
                            None => break,
                        }
                    }
                }
            }
            if fam != 0 {
                let f = (fam == 6) as u8;
                for (&pfx, _) in &a.prefixes[f as usize] {
                    let k = (f, pfx, masked(&addr, fam, pfx as usize));
                    p.consider(&self.ids, a.nets.get(&k), SC_CIDR + pfx as i32);
                }
            }
            if lan == 1 {
                p.consider(&self.ids, Some(&a.lan), SC_NET);
            } else if lan == 0 {
                p.consider(&self.ids, Some(&a.inet), SC_NET);
            }
            p.consider(&self.ids, Some(&a.any), SC_ANY);
            if let Some(best) = p.best.filter(|_| p.dst >= SC_CIDR || q.listed == 0) {
                let r = &self.ids[&best];
                d.verdict = if r.action == FW_DENY { FW_V_DENY } else { FW_V_ALLOW };
                d.reason = FW_BY_RULE;
                d.rule_id = best;
                once = (r.life == FW_LIFE_ONCE).then_some(best);
                decided = true;
            }
        }
        if decided {
            if let Some(id) = once {
                self.remove(id);
            }
            return d;
        }

        // 2b. a subscribed blocklist: only a rule that names the destination (above) beats it
        if q.listed != 0 {
            d.verdict = FW_V_DENY;
            d.reason = FW_BY_LIST;
            d.record = 1;
            return d;
        }
        // 3. trusted
        if self.apps.get(app).is_some_and(|a| a.trusted) {
            d.verdict = FW_V_ALLOW;
            d.reason = FW_BY_TRUSTED;
            return d;
        }
        // 4. mode
        d.reason = FW_BY_MODE;
        match self.mode {
            FW_MODE_SILENT => {
                d.verdict = FW_V_ALLOW;
                d.record = 1;
            }
            FW_MODE_ASK => d.verdict = FW_V_ASK,
            FW_MODE_OFF => d.verdict = FW_V_ALLOW,
            FW_MODE_DENY => {
                d.verdict = if fam != 0 && addr_is_loopback(fam, &addr) { FW_V_ALLOW } else { FW_V_DENY };
                d.record = 1;
            }
            _ => {}
        }
        d
    }
}

/// Destination score: exact domain > wildcard (more labels, more specific) > network
/// (longest prefix) > LAN/internet > any.
const SC_ANY: i32 = 0;
const SC_NET: i32 = 500;
const SC_CIDR: i32 = 1000;
const SC_WILD: i32 = 2000;
const SC_EXACT: i32 = 3000;

struct Pick<'a> {
    q: &'a FwQuery,
    best: Option<u32>,
    dst: i32,
    port: i32,
    proto: i32,
}

impl Pick<'_> {
    /// Destination, then smaller port range, concrete protocol, deny before allow and,
    /// last, the newest decision.
    fn consider(&mut self, ids: &HashMap<u32, Box<FwRule>>, list: Option<&Vec<u32>>, dst: i32) {
        for id in list.into_iter().flatten() {
            let r = &ids[id];
            if r.dir != self.q.dir || (r.proto != FW_PROTO_ANY && r.proto != self.q.proto) {
                continue;
            }
            if self.q.port < r.port_lo || self.q.port > r.port_hi {
                continue;
            }
            let port = 65535 - (r.port_hi as i32 - r.port_lo as i32);
            let proto = (r.proto != FW_PROTO_ANY) as i32;
            if let Some(b) = self.best {
                let b = &ids[&b];
                if dst != self.dst {
                    if dst < self.dst {
                        continue;
                    }
                } else if port != self.port {
                    if port < self.port {
                        continue;
                    }
                } else if proto != self.proto {
                    if proto < self.proto {
                        continue;
                    }
                } else if r.action != b.action {
                    if r.action != FW_DENY {
                        continue;
                    }
                } else if r.id < b.id {
                    continue;
                }
            }
            self.best = Some(r.id);
            self.dst = dst;
            self.port = port;
            self.proto = proto;
        }
    }
}

/// Validates and normalizes a rule.
fn validate(inp: &FwRule) -> Option<FwRule> {
    let mut out = zeroed_rule();
    set(&mut out.app, &app_canon(&inp.app)?);
    if inp.dir != FW_OUT && inp.dir != FW_IN {
        return None;
    }
    if ![FW_PROTO_ANY, FW_PROTO_TCP, FW_PROTO_UDP].contains(&inp.proto) || ![FW_ALLOW, FW_DENY].contains(&inp.action) {
        return None;
    }
    if ![FW_LIFE_FOREVER, FW_LIFE_UNTIL_QUIT, FW_LIFE_ONCE].contains(&inp.life) || inp.port_lo > inp.port_hi {
        return None;
    }
    out.dir = inp.dir;
    out.proto = inp.proto;
    out.action = inp.action;
    out.life = inp.life;
    out.port_lo = inp.port_lo;
    out.port_hi = inp.port_hi;
    if inp.port_lo == 0 && inp.port_hi == 0 {
        out.port_hi = 65535; // no ports = any
    }
    out.when = inp.when.max(0);
    out.kind = inp.kind;
    match inp.kind {
        FW_DST_ANY | FW_DST_LAN | FW_DST_INTERNET => Some(out),
        FW_DST_DOMAIN => {
            let d = cs(&inp.domain);
            if d.len() >= FW_NAME_MAX {
                return None;
            }
            set(&mut out.domain, &norm_domain(d)?);
            out.wildcard = (inp.wildcard != 0) as c_int;
            Some(out)
        }
        FW_DST_CIDR => {
            if inp.fam != 4 && inp.fam != 6 {
                return None;
            }
            if inp.prefix < 0 || inp.prefix > if inp.fam == 4 { 32 } else { 128 } {
                return None;
            }
            let bytes = if inp.fam == 4 { 4 } else { 16 };
            if !host_bits_zero(&inp.addr, bytes, inp.prefix as usize) || (inp.fam == 4 && inp.addr[4..].iter().any(|&b| b != 0)) {
                return None;
            }
            out.fam = inp.fam;
            out.prefix = inp.prefix;
            out.addr = inp.addr;
            Some(out)
        }
        _ => None,
    }
}

// ---- C ABI ----

unsafe fn store<'a>(s: *const FwStore) -> Option<&'a FwStore> {
    s.as_ref()
}
unsafe fn store_mut<'a>(s: *mut FwStore) -> Option<&'a mut FwStore> {
    s.as_mut()
}
unsafe fn canon(app: *const c_char) -> Option<Vec<u8>> {
    if app.is_null() {
        return None;
    }
    app_canon(cstr(app))
}
/// strnlen(s, max) bytes of a C string, without reading past them.
unsafe fn cstr_n<'a>(p: *const c_char, max: usize) -> &'a [u8] {
    let n = libc::strnlen(p, max);
    std::slice::from_raw_parts(p.cast(), n)
}

#[no_mangle]
pub extern "C" fn fw_store_new() -> *mut FwStore {
    Box::into_raw(Box::new(FwStore::new()))
}
/// # Safety
/// `s` is NULL or from fw_store_new.
#[no_mangle]
pub unsafe extern "C" fn fw_store_free(s: *mut FwStore) {
    if !s.is_null() {
        drop(Box::from_raw(s));
    }
}
/// Swaps the contents of two stores (fw_from_json loads into a new one, then swaps).
///
/// # Safety
/// Both are valid stores.
#[no_mangle]
pub unsafe extern "C" fn fw_store_swap(a: *mut FwStore, b: *mut FwStore) {
    if let (Some(a), Some(b)) = (a.as_mut(), b.as_mut()) {
        std::mem::swap(a, b);
    }
}

/// # Safety
/// `inp` is NULL or a C string; `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn fw_app_canon(inp: *const c_char, out: *mut c_char, n: usize) -> c_int {
    if inp.is_null() || out.is_null() || n == 0 {
        return -1;
    }
    match app_canon(cstr_n(inp, FW_APP_MAX)) {
        Some(c) if c.len() < n => {
            set(std::slice::from_raw_parts_mut(out.cast(), n), &c);
            0
        }
        _ => -1,
    }
}
/// # Safety
/// `name` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fw_component_ok(name: *const c_char) -> c_int {
    (!name.is_null() && component_ok(cstr(name))) as c_int
}
/// # Safety
/// `scope` is NULL or a C string; `app` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn fw_component_scope(scope: *const c_char, app: *mut c_char, n: usize) -> c_int {
    let s = if scope.is_null() { &b""[..] } else { cstr(scope) };
    let l = s.len();
    if l <= 17 || !s.starts_with(b"lordos-net-") || !s.ends_with(b".scope") {
        return -1;
    }
    let comp = &s[11..l - 6];
    if comp.len() >= 32 || !component_ok(comp) {
        return -1;
    }
    let mut v = b"lordos:".to_vec();
    v.extend_from_slice(comp);
    if app.is_null() || n == 0 {
        return -1;
    }
    let o = std::slice::from_raw_parts_mut(app.cast::<u8>(), n);
    let k = v.len().min(n - 1);
    o[..k].copy_from_slice(&v[..k]);
    o[k] = 0;
    if v.len() < n { 0 } else { -1 }
}
/// # Safety
/// `s` is NULL or a C string; `r` NULL or a valid rule.
#[no_mangle]
pub unsafe extern "C" fn fw_parse_dest(s: *const c_char, r: *mut FwRule) -> c_int {
    if s.is_null() || r.is_null() {
        return -1;
    }
    if parse_dest(cstr(s), &mut *r) { 0 } else { -1 }
}
/// # Safety
/// `inp` is NULL or a C string; `out` has room for FW_NAME_MAX bytes.
#[no_mangle]
pub unsafe extern "C" fn fw_norm_domain(inp: *const c_char, out: *mut c_char) -> c_int {
    if inp.is_null() || out.is_null() {
        return -1;
    }
    let s = cstr_n(inp, FW_NAME_MAX + 1);
    if s.len() > FW_NAME_MAX {
        return -1;
    }
    match norm_domain(s) {
        Some(d) => {
            set(std::slice::from_raw_parts_mut(out.cast(), FW_NAME_MAX), &d);
            0
        }
        None => -1,
    }
}
/// # Safety
/// `r` is a valid rule; `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn fw_format_dest(r: *const FwRule, out: *mut c_char, n: usize) -> c_int {
    let Some(v) = format_dest(&*r) else { return -1 };
    if !out.is_null() && n > 0 {
        let o = std::slice::from_raw_parts_mut(out.cast::<u8>(), n);
        let k = v.len().min(n - 1);
        o[..k].copy_from_slice(&v[..k]);
        o[k] = 0;
    }
    if v.len() < n { 0 } else { -1 }
}
/// # Safety
/// `a` is NULL or 16 bytes.
#[no_mangle]
pub unsafe extern "C" fn fw_addr_is_lan(fam: c_int, a: *const u8) -> c_int {
    (!a.is_null() && addr_is_lan(fam, std::slice::from_raw_parts(a, 16))) as c_int
}
/// # Safety
/// `a` is NULL or 16 bytes.
#[no_mangle]
pub unsafe extern "C" fn fw_addr_is_loopback(fam: c_int, a: *const u8) -> c_int {
    (!a.is_null() && addr_is_loopback(fam, std::slice::from_raw_parts(a, 16))) as c_int
}

/// # Safety
/// `s` is NULL or a store, `r` NULL or a rule, `id_out` NULL or valid.
#[no_mangle]
pub unsafe extern "C" fn fw_rule_add(s: *mut FwStore, r: *const FwRule, id_out: *mut u32) -> c_int {
    let (Some(s), Some(r)) = (store_mut(s), r.as_ref()) else { return -1 };
    let Some(v) = validate(r) else { return -1 };
    match s.add(v) {
        Some(id) => {
            if !id_out.is_null() {
                *id_out = id;
            }
            0
        }
        None => -1,
    }
}
/// # Safety
/// `s` is NULL or a store.
#[no_mangle]
pub unsafe extern "C" fn fw_rule_remove(s: *mut FwStore, id: u32) -> c_int {
    match store_mut(s) {
        Some(s) => {
            if s.remove(id) { 0 } else { -1 }
        }
        None => -1,
    }
}
/// # Safety
/// `s` is NULL or a store. The pointer stays valid until the rule changes or goes.
#[no_mangle]
pub unsafe extern "C" fn fw_rule_get(s: *const FwStore, id: u32) -> *const FwRule {
    store(s).and_then(|s| s.ids.get(&id)).map_or(ptr::null(), |r| &**r as *const FwRule)
}
/// # Safety
/// `s` is NULL or a store; `app` NULL or a C string; `out` has room for `max` rules.
#[no_mangle]
pub unsafe extern "C" fn fw_rule_list(s: *const FwStore, app: *const c_char, out: *mut FwRule, max: c_int) -> c_int {
    let Some(s) = store(s) else { return 0 };
    let only = if app.is_null() {
        None
    } else {
        match canon(app) {
            Some(c) if s.apps.contains_key(&c) => Some(c),
            _ => return 0,
        }
    };
    let v = s.sorted(only.as_deref());
    for (i, id) in v.iter().enumerate().take(max.max(0) as usize) {
        *out.add(i) = *s.ids[id];
    }
    v.len() as c_int
}
/// # Safety
/// `s` is NULL or a store.
#[no_mangle]
pub unsafe extern "C" fn fw_rule_count(s: *const FwStore) -> usize {
    store(s).map_or(0, |s| s.ids.len())
}
/// # Safety
/// `s` is NULL or a store; `app` NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fw_app_quit(s: *mut FwStore, app: *const c_char) {
    let (Some(s), Some(c)) = (store_mut(s), canon(app)) else { return };
    if s.apps.contains_key(&c) {
        s.remove_where(&c, true);
    }
}
/// # Safety
/// `s` is NULL or a store; `app` NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fw_app_forget(s: *mut FwStore, app: *const c_char) {
    let (Some(s), Some(c)) = (store_mut(s), canon(app)) else { return };
    let Some(a) = s.apps.get_mut(&c) else { return };
    a.trusted = false;
    a.restricted = 0;
    if a.nrules != 0 {
        s.remove_where(&c, false); // the last rule releases the App
    } else {
        s.prune_app(&c);
    }
}
/// # Safety
/// `s` is NULL or a store; `app` NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fw_set_trusted(s: *mut FwStore, app: *const c_char, on: c_int) -> c_int {
    let (Some(s), Some(c)) = (store_mut(s), canon(app)) else { return -1 };
    if !s.apps.contains_key(&c) {
        if on == 0 {
            return 0;
        }
        if s.apps.len() >= FW_MAX_APPS {
            return -1;
        }
        s.apps.insert(c.clone(), App { id: c.clone(), ..Default::default() });
    }
    let a = s.apps.get_mut(&c).unwrap();
    a.trusted = on != 0;
    if on != 0 {
        a.restricted = 0;
    }
    s.prune_app(&c);
    0
}
/// # Safety
/// `s` is NULL or a store; `app` NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fw_is_trusted(s: *const FwStore, app: *const c_char) -> c_int {
    match (store(s), canon(app)) {
        (Some(s), Some(c)) => s.apps.get(&c).is_some_and(|a| a.trusted) as c_int,
        _ => 0,
    }
}
/// # Safety
/// `s` is NULL or a store; `app` NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fw_set_restricted(s: *mut FwStore, app: *const c_char, flags: u32) -> c_int {
    let (Some(s), Some(c)) = (store_mut(s), canon(app)) else { return -1 };
    if flags & !FW_R_ALL != 0 {
        return -1;
    }
    if !s.apps.contains_key(&c) {
        if flags == 0 {
            return 0;
        }
        if s.apps.len() >= FW_MAX_APPS {
            return -1;
        }
        s.apps.insert(c.clone(), App { id: c.clone(), ..Default::default() });
    }
    let a = s.apps.get_mut(&c).unwrap();
    a.restricted = flags;
    if flags != 0 {
        a.trusted = false;
    }
    s.prune_app(&c);
    0
}
/// # Safety
/// `s` is NULL or a store; `app` NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fw_restricted(s: *const FwStore, app: *const c_char) -> u32 {
    match (store(s), canon(app)) {
        (Some(s), Some(c)) => s.apps.get(&c).map_or(0, |a| a.restricted),
        _ => 0,
    }
}
/// # Safety
/// As fw_restricted.
#[no_mangle]
pub unsafe extern "C" fn fw_blocks(s: *const FwStore, app: *const c_char, flag: u32) -> c_int {
    (fw_restricted(s, app) & flag != 0) as c_int
}
/// # Safety
/// `s` is NULL or a store; `cb` is called with each app's id (valid during the call).
#[no_mangle]
pub unsafe extern "C" fn fw_app_each(
    s: *const FwStore,
    cb: Option<unsafe extern "C" fn(*mut c_void, *const c_char, c_int, u32) -> c_int>,
    u: *mut c_void,
) {
    let (Some(s), Some(cb)) = (store(s), cb) else { return };
    for a in s.apps.values() {
        let mut id = a.id.clone();
        id.push(0);
        if cb(u, id.as_ptr().cast(), a.trusted as c_int, a.restricted) != 0 {
            return;
        }
    }
}
/// # Safety
/// `s` is NULL or a store.
#[no_mangle]
pub unsafe extern "C" fn fw_set_mode(s: *mut FwStore, m: c_int) {
    if let Some(s) = store_mut(s) {
        if (FW_MODE_SILENT..=FW_MODE_DENY).contains(&m) {
            s.mode = m;
        }
    }
}
/// # Safety
/// `s` is NULL or a store.
#[no_mangle]
pub unsafe extern "C" fn fw_mode(s: *const FwStore) -> c_int {
    store(s).map_or(FW_MODE_SILENT, |s| s.mode)
}
/// # Safety
/// `s` is NULL or a store; `q` NULL or a query whose strings are C strings.
#[no_mangle]
pub unsafe extern "C" fn fw_decide(s: *mut FwStore, q: *const FwQuery) -> FwDecision {
    let no = FwDecision { verdict: FW_V_DENY, reason: FW_BY_RESTRICTED, rule_id: 0, flag: FW_R_NO_NETWORK, record: 0 };
    let (Some(s), Some(q)) = (store_mut(s), q.as_ref()) else { return no };
    let Some(app) = canon(q.app) else { return no }; // no valid app: no
    let mut hosts = Vec::new();
    if !q.hosts.is_null() {
        for h in 0..q.nhosts.max(0) as usize {
            let p = *q.hosts.add(h);
            hosts.push((!p.is_null()).then(|| cstr_n(p, FW_NAME_MAX + 1)));
        }
    }
    s.decide(q, &app, &hosts)
}
#[no_mangle]
pub extern "C" fn fw_flag_name(flag: u32) -> *const c_char {
    (0..FLAG_NAMES.len()).find(|&i| flag == 1 << i).map_or(ptr::null(), |i| FLAG_NAMES[i].as_ptr())
}
/// # Safety
/// `name` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn fw_flag_from_name(name: *const c_char) -> u32 {
    if name.is_null() {
        return 0;
    }
    let n = cstr(name);
    FLAG_NAMES.iter().position(|f| f.to_bytes() == n).map_or(0, |i| 1 << i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(app: &str, dst: &str, action: c_int) -> FwRule {
        let mut r = zeroed_rule();
        set(&mut r.app, app.as_bytes());
        assert!(parse_dest(dst.as_bytes(), &mut r), "{dst}");
        r.action = action;
        r
    }

    fn query(app: &std::ffi::CStr, addr: [u8; 4], hosts: &[*const c_char]) -> FwQuery {
        let mut a = [0u8; 16];
        a[..4].copy_from_slice(&addr);
        FwQuery {
            app: app.as_ptr(),
            dir: FW_OUT,
            proto: FW_PROTO_TCP,
            fam: 4,
            addr: a,
            port: 443,
            hosts: hosts.as_ptr(),
            nhosts: hosts.len() as c_int,
            listed: 0,
        }
    }

    #[test]
    fn canon_and_domains() {
        assert_eq!(app_canon(b"app-lordos-lpk-org.x.scope").unwrap(), b"lpk:org.x");
        assert_eq!(app_canon(b"app-lordos-firefox.scope").unwrap(), b"desktop:firefox");
        assert!(app_canon(b"/usr/bin/../x").is_none() && app_canon(b"desktop:").is_none());
        assert_eq!(norm_domain(b"WWW.Example.COM.").unwrap(), b"www.example.com");
        assert!(norm_domain(b"1.2.3.4").is_none() && norm_domain(b"a..b").is_none());
        let mut r = zeroed_rule();
        assert!(parse_dest(b"::ffff:10.0.0.0/104", &mut r) && r.fam == 4 && r.prefix == 8);
        assert_eq!(format_dest(&r).unwrap(), b"10.0.0.0/8");
        assert!(!parse_dest(b"10.0.0.1/8", &mut r));
    }

    #[test]
    fn deciding() {
        unsafe {
            let s = fw_store_new();
            let mut id = 0;
            assert_eq!(fw_rule_add(s, &rule("desktop:f", "*.example.com", FW_ALLOW), &mut id), 0);
            assert_eq!(fw_rule_add(s, &rule("desktop:f", "ads.example.com", FW_DENY), ptr::null_mut()), 0);
            assert_eq!(fw_rule_add(s, &rule("desktop:f", "10.0.0.0/8", FW_DENY), ptr::null_mut()), 0);
            // same destination and match: replaced, same id
            let mut again = 0;
            assert_eq!(fw_rule_add(s, &rule("desktop:f", "*.example.com", FW_ALLOW), &mut again), 0);
            assert_eq!((again, fw_rule_count(s)), (id, 3));
            let app = c"desktop:f";
            let www = c"www.example.com";
            let ads = c"ads.example.com";
            let d = fw_decide(s, &query(app, [93, 184, 216, 34], &[www.as_ptr()]));
            assert_eq!((d.verdict, d.reason, d.rule_id), (FW_V_ALLOW, FW_BY_RULE, id));
            let d = fw_decide(s, &query(app, [93, 184, 216, 34], &[ads.as_ptr()]));
            assert_eq!(d.verdict, FW_V_DENY);
            let d = fw_decide(s, &query(app, [10, 1, 2, 3], &[]));
            assert_eq!((d.verdict, d.reason), (FW_V_DENY, FW_BY_RULE));
            assert_eq!(fw_set_restricted(s, app.as_ptr(), FW_R_NO_NETWORK), 0);
            let d = fw_decide(s, &query(app, [10, 1, 2, 3], &[]));
            assert_eq!((d.reason, d.flag), (FW_BY_RESTRICTED, FW_R_NO_NETWORK));
            fw_app_forget(s, app.as_ptr());
            assert_eq!(fw_rule_count(s), 0);
            assert!((*s).apps.is_empty());
            fw_store_free(s);
        }
    }
}
