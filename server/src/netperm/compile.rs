//! Compile the existing rule store into bounded, precedence-sorted kernel maps.
use crate::{dnsmap::*, fwrules::*, shared::*};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr},
    ptr,
};
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NpNode {
    pub fam: i32,
    pub prefix: i32,
    pub addr: [u8; 16],
    pub host: i32,
    pub list: NpList,
}
#[repr(C)]
pub struct NpCompiled {
    pub app: NpApp,
    pub nodes: *mut NpNode,
    pub nnodes: i32,
    pub cap: i32,
    pub truncated: i32,
    pub has_domains: i32,
}
pub struct Compiled {
    pub app: NpApp,
    pub nodes: Vec<NpNode>,
    pub truncated: usize,
    pub has_domains: bool,
}
fn score(r: &FwRule) -> i32 {
    match r.kind {
        1 => {
            (if r.wildcard != 0 { 2000 } else { 3000 })
                + 1
                + cs(&r.domain).iter().filter(|&&x| x == b'.').count() as i32
        }
        2 => 1000 + r.prefix,
        3 | 4 => 500,
        _ => 0,
    }
}
fn fill(mut rules: Vec<&FwRule>, out: &mut [NpEnt]) -> (u32, usize) {
    rules.sort_by_key(|r| {
        (
            std::cmp::Reverse(score(r)),
            r.port_hi - r.port_lo,
            std::cmp::Reverse(r.proto != 0),
            std::cmp::Reverse(r.action == 1),
            std::cmp::Reverse(r.id),
        )
    });
    let n = rules.len().min(out.len());
    for (e, r) in out.iter_mut().zip(&rules) {
        *e = NpEnt {
            port_lo: r.port_lo,
            port_hi: r.port_hi,
            proto: r.proto as u8,
            dir: r.dir as u8,
            verdict: r.action as u8,
            kind: match r.kind {
                3 => 2,
                4 => 3,
                0 => 1,
                _ => 0,
            },
            rule_id: r.id,
        };
    }
    (n as u32, rules.len() - n)
}
fn contains(r: &FwRule, fam: i32, addr: &[u8; 16], prefix: i32) -> bool {
    r.kind == 2
        && r.fam == fam
        && r.prefix >= 0
        && r.prefix <= prefix
        && (0..r.prefix as usize).all(|i| (r.addr[i / 8] ^ addr[i / 8]) & (128 >> (i % 8)) == 0)
}
fn domain(r: &FwRule, name: &[u8]) -> bool {
    let d = cs(&r.domain);
    r.kind == 1
        && (name == d
            || (r.wildcard != 0
                && name.len() > d.len()
                && name.ends_with(d)
                && name[name.len() - d.len() - 1] == b'.'))
}
unsafe fn rules(s: *mut FwStore, app: *const c_char) -> Vec<FwRule> {
    let n = fw_rule_list(s, app, ptr::null_mut(), 0).max(0) as usize;
    let mut v = vec![zeroed_rule(); n];
    let got = fw_rule_list(s, app, v.as_mut_ptr(), n as i32).max(0) as usize;
    v.truncate(got.min(n));
    v.retain(|r| r.life != 2);
    v
}
fn build(rules: &[FwRule], mut node: NpNode, names: &[[u8; 254]]) -> (NpNode, usize) {
    let v = rules
        .iter()
        .filter(|r| {
            contains(r, node.fam, &node.addr, node.prefix) || names.iter().any(|n| domain(r, cs(n)))
        })
        .collect();
    let (n, tr) = fill(v, &mut node.list.e);
    node.list.n = n;
    (node, tr)
}
pub unsafe fn compile(
    s: *mut FwStore,
    app: *const c_char,
    idx: u32,
    names: *mut DnsMap,
    names_app: *const c_char,
    now: i64,
) -> Compiled {
    let rules = rules(s, app);
    let flags = fw_restricted(s, app)
        & (NP_R_NET_MASK
            | if CStr::from_ptr(app).to_bytes() == b"lordos" {
                0
            } else {
                NP_R_DEV_MASK
            });
    let mut c = Compiled {
        app: NpApp {
            idx,
            flags,
            trusted: (fw_is_trusted(s, app) != 0) as u8,
            mode: match fw_mode(s) {
                2 => 2,
                3 => 3,
                _ => 0,
            },
            ..Default::default()
        },
        nodes: vec![],
        truncated: 0,
        has_domains: rules.iter().any(|r| r.kind == 1),
    };
    let (n, tr) = fill(
        rules
            .iter()
            .filter(|r| matches!(r.kind, 0 | 3 | 4))
            .collect(),
        &mut c.app.gen,
    );
    c.app.ngen = n;
    c.truncated += tr;
    let mut seen = HashMap::new();
    for r in &rules {
        if r.kind != 2 {
            continue;
        }
        let key = (r.fam, r.prefix, r.addr);
        if seen.contains_key(&key) {
            continue;
        }
        let (node, tr) = build(
            &rules,
            NpNode {
                fam: r.fam,
                prefix: r.prefix,
                addr: r.addr,
                ..Default::default()
            },
            &[],
        );
        seen.insert(key, c.nodes.len());
        c.nodes.push(node);
        c.truncated += tr;
    }
    struct Ctx<'a> {
        rules: &'a [FwRule],
        c: &'a mut Compiled,
        seen: &'a mut HashMap<(i32, i32, [u8; 16]), usize>,
    }
    unsafe extern "C" fn host(
        u: *mut c_void,
        fam: i32,
        addr: *const u8,
        names: *const [u8; 254],
        n: i32,
    ) -> i32 {
        let h = &mut *u.cast::<Ctx>();
        let names = std::slice::from_raw_parts(names, n.max(0) as usize);
        if !h
            .rules
            .iter()
            .any(|r| names.iter().any(|n| domain(r, cs(n))))
        {
            return 0;
        }
        let mut node = NpNode {
            fam,
            prefix: if fam == 4 { 32 } else { 128 },
            host: 1,
            ..Default::default()
        };
        ptr::copy_nonoverlapping(addr, node.addr.as_mut_ptr(), if fam == 4 { 4 } else { 16 });
        let (node, tr) = build(h.rules, node, names);
        h.c.truncated += tr;
        let key = (fam, node.prefix, node.addr);
        if let Some(&i) = h.seen.get(&key) {
            h.c.nodes[i] = node
        } else {
            h.seen.insert(key, h.c.nodes.len());
            h.c.nodes.push(node)
        }
        0
    }
    if c.has_domains && !names.is_null() && !names_app.is_null() {
        let mut h = Ctx {
            rules: &rules,
            c: &mut c,
            seen: &mut seen,
        };
        dnsmap_each(
            names,
            names_app,
            now,
            Some(host),
            (&mut h as *mut Ctx).cast(),
        );
    }
    c.app.can_deny = (flags & NP_R_NET_MASK != 0
        || rules.iter().any(|r| r.kind == 1 && r.action == 1)
        || c.app.mode == 3
        || np_ents_deny(&c.app.gen[..c.app.ngen as usize])) as u32;
    for n in &c.nodes {
        if n.fam == 4 {
            c.app.has4 = 1
        } else {
            c.app.has6 = 1
        }
        if np_ents_deny(&n.list.e[..n.list.n as usize]) {
            c.app.can_deny = 1
        }
    }
    c
}
#[no_mangle]
pub unsafe extern "C" fn np_compile_app(
    s: *mut FwStore,
    app: *const c_char,
    idx: u32,
    names: *mut DnsMap,
    names_app: *const c_char,
    now: i64,
    out: *mut NpCompiled,
) -> i32 {
    if out.is_null() || app.is_null() {
        return -1;
    }
    ptr::write_bytes(out, 0, 1);
    let c = compile(s, app, idx, names, names_app, now);
    let n = c.nodes.len();
    let nodes = if n == 0 {
        ptr::null_mut()
    } else {
        libc::malloc(n * size_of::<NpNode>()).cast::<NpNode>()
    };
    if n > 0 && nodes.is_null() {
        return -1;
    }
    if n > 0 {
        ptr::copy_nonoverlapping(c.nodes.as_ptr(), nodes, n)
    }
    out.write(NpCompiled {
        app: c.app,
        nodes,
        nnodes: n as i32,
        cap: n as i32,
        truncated: c.truncated as i32,
        has_domains: c.has_domains as i32,
    });
    0
}
#[no_mangle]
pub unsafe extern "C" fn np_compile_host(
    s: *mut FwStore,
    app: *const c_char,
    fam: i32,
    addr: *const u8,
    names: *const [u8; 254],
    n: i32,
    out: *mut NpNode,
    tr: *mut i32,
) -> i32 {
    if app.is_null()
        || addr.is_null()
        || out.is_null()
        || !matches!(fam, 4 | 6)
        || n < 0
        || (n > 0 && names.is_null())
    {
        return -1;
    }
    let rules = rules(s, app);
    let names = if n == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(names, n as usize)
    };
    if !rules.iter().any(|r| names.iter().any(|n| domain(r, cs(n)))) {
        return 0;
    }
    let mut node = NpNode {
        fam,
        prefix: if fam == 4 { 32 } else { 128 },
        host: 1,
        ..Default::default()
    };
    ptr::copy_nonoverlapping(addr, node.addr.as_mut_ptr(), if fam == 4 { 4 } else { 16 });
    let (node, t) = build(&rules, node, names);
    out.write(node);
    if !tr.is_null() {
        *tr += t as i32
    }
    1
}
#[no_mangle]
pub unsafe extern "C" fn np_compiled_free(c: *mut NpCompiled) {
    if let Some(c) = c.as_mut() {
        libc::free(c.nodes.cast());
        c.nodes = ptr::null_mut();
        c.nnodes = 0;
        c.cap = 0;
    }
}
#[no_mangle]
pub unsafe extern "C" fn np_cidr_contains(
    r: *const FwRule,
    fam: i32,
    addr: *const u8,
    prefix: i32,
) -> i32 {
    if r.is_null()
        || addr.is_null()
        || !matches!(fam, 4 | 6)
        || prefix < 0
        || prefix > if fam == 4 { 32 } else { 128 }
    {
        return 0;
    }
    let mut a = [0; 16];
    ptr::copy_nonoverlapping(addr, a.as_mut_ptr(), if fam == 4 { 4 } else { 16 });
    contains(&*r, fam, &a, prefix) as i32
}
pub fn key4(idx: u32, n: &NpNode) -> NpLpm4Key {
    let mut k = NpLpm4Key {
        prefixlen: 32 + n.prefix as u32,
        ..Default::default()
    };
    k.data[..4].copy_from_slice(&idx.to_be_bytes());
    k.data[4..].copy_from_slice(&n.addr[..4]);
    k
}
pub fn key6(idx: u32, n: &NpNode) -> NpLpm6Key {
    let mut k = NpLpm6Key {
        prefixlen: 32 + n.prefix as u32,
        ..Default::default()
    };
    k.data[..4].copy_from_slice(&idx.to_be_bytes());
    k.data[4..].copy_from_slice(&n.addr);
    k
}
#[no_mangle]
pub unsafe extern "C" fn np_key4(idx: u32, n: *const NpNode, k: *mut NpLpm4Key) {
    if !n.is_null() && !k.is_null() {
        k.write(key4(idx, &*n))
    }
}
#[no_mangle]
pub unsafe extern "C" fn np_key6(idx: u32, n: *const NpNode, k: *mut NpLpm6Key) {
    if !n.is_null() && !k.is_null() {
        k.write(key6(idx, &*n))
    }
}
