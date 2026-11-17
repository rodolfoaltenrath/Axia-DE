//! Kernel-side programs, built directly by rustc for bpfel-unknown-none.
#![no_std]
#![feature(core_intrinsics)]
#![allow(internal_features)]
#![allow(dead_code, non_upper_case_globals)]
mod shared;
use core::{ffi::c_void, mem::transmute, ptr};
use shared::*;
macro_rules! maps {($($n:ident),*)=>{$(#[no_mangle] #[link_section="maps"] pub static $n:[u8;4]=[0;4];)*}}
maps!(
    np_cfg, np_apps, np_union, np_lpm4, np_lpm6, np_seen, np_counts, np_events, np_snd, np_flows,
    np_bound, np_blk4, np_blk6, np_sysc
);
#[inline(always)]
unsafe fn lookup<T, K>(map: *const [u8; 4], k: &K) -> *mut T {
    let f: unsafe extern "C" fn(*const c_void, *const c_void) -> *mut T = transmute(1usize);
    f(map.cast(), (k as *const K).cast())
}
#[inline(always)]
unsafe fn update<K, V>(map: *const [u8; 4], k: &K, v: &V, flags: u64) {
    let f: unsafe extern "C" fn(*const c_void, *const c_void, *const c_void, u64) -> i64 =
        transmute(2usize);
    f(
        map.cast(),
        (k as *const K).cast(),
        (v as *const V).cast(),
        flags,
    );
}
#[inline(always)]
unsafe fn clock() -> u64 {
    let f: unsafe extern "C" fn() -> u64 = transmute(5usize);
    f()
}
#[inline(always)]
unsafe fn ancestor(level: u32) -> u64 {
    let f: unsafe extern "C" fn(i32) -> u64 = transmute(123usize);
    f(level as i32)
}
#[inline(always)]
unsafe fn emit(ev: &NpEvent) {
    let f: unsafe extern "C" fn(*const c_void, *const c_void, u64, u64) -> i64 =
        transmute(130usize);
    f(
        ptr::addr_of!(np_events).cast(),
        (ev as *const NpEvent).cast(),
        size_of::<NpEvent>() as u64,
        0,
    );
}
#[inline(always)]
unsafe fn add(p: *mut u64, n: u64) {
    core::intrinsics::atomic_xadd::<u64, u64, { core::intrinsics::AtomicOrdering::Relaxed }>(p, n);
}
#[inline(always)]
fn proto(p: u32) -> i32 {
    if p == 6 {
        1
    } else if p == 17 {
        2
    } else {
        3
    }
}
#[inline(always)]
unsafe fn app(cfg: &NpCfg) -> (u64, *mut NpApp) {
    let id = ancestor(cfg.level);
    if id != 0 {
        let a: *mut NpApp = lookup(ptr::addr_of!(np_apps), &id);
        if !a.is_null() {
            return (id, a);
        }
    }
    if cfg.user_level != 0 {
        let id = ancestor(cfg.user_level);
        if id != 0 {
            let a: *mut NpApp = lookup(ptr::addr_of!(np_union), &id);
            if !a.is_null() {
                return (id, a);
            }
        }
    }
    (0, ptr::null_mut())
}
#[inline(always)]
unsafe fn count(id: u64, deny: bool) {
    let mut c: *mut NpCount = lookup(ptr::addr_of!(np_counts), &id);
    if c.is_null() {
        update(ptr::addr_of!(np_counts), &id, &NpCount::default(), 1);
        c = lookup(ptr::addr_of!(np_counts), &id)
    }
    if !c.is_null() {
        if deny {
            add(ptr::addr_of_mut!((*c).denied), 1)
        } else {
            add(ptr::addr_of_mut!((*c).allowed), 1)
        }
    }
}
#[inline(always)]
fn key4(idx: u32, a: &[u8; 16]) -> NpLpm4Key {
    let mut k = NpLpm4Key {
        prefixlen: 64,
        data: [0; 8],
    };
    let idx = idx.to_be_bytes();
    let mut i = 0;
    while i < 4 {
        k.data[i] = idx[i];
        k.data[i + 4] = a[i];
        i += 1
    }
    k
}
#[inline(always)]
fn key6(idx: u32, a: &[u8; 16]) -> NpLpm6Key {
    let mut k = NpLpm6Key {
        prefixlen: 160,
        data: [0; 20],
    };
    let idx = idx.to_be_bytes();
    let mut i = 0;
    while i < 4 {
        k.data[i] = idx[i];
        i += 1
    }
    i = 0;
    while i < 16 {
        k.data[i + 4] = a[i];
        i += 1
    }
    k
}
#[inline(always)]
unsafe fn list(a: &NpApp, fam: i32, addr: &[u8; 16]) -> (*mut NpList, i32) {
    if fam == 4 {
        (
            if a.has4 != 0 {
                lookup(ptr::addr_of!(np_lpm4), &key4(a.idx, addr))
            } else {
                ptr::null_mut()
            },
            np_is_lan4(addr) as i32,
        )
    } else if fam == 6 {
        (
            if a.has6 != 0 {
                lookup(ptr::addr_of!(np_lpm6), &key6(a.idx, addr))
            } else {
                ptr::null_mut()
            },
            np_is_lan6(addr) as i32,
        )
    } else {
        (ptr::null_mut(), -1)
    }
}
#[inline(always)]
unsafe fn listed(a: &NpApp, fam: i32, addr: &[u8; 16]) -> bool {
    if a.blk == 0 {
        return false;
    }
    if fam == 4 {
        !lookup::<u8, _>(ptr::addr_of!(np_blk4), &key4(a.blk, addr)).is_null()
    } else if fam == 6 {
        !lookup::<u8, _>(ptr::addr_of!(np_blk6), &key6(a.blk, addr)).is_null()
    } else {
        false
    }
}
#[inline(always)]
unsafe fn handle(fam: i32, addr: [u8; 16], port: i32, pr: i32, dir: i32, always: bool) -> i32 {
    let cfg: *mut NpCfg = lookup(ptr::addr_of!(np_cfg), &mut 0u32);
    if cfg.is_null() || (*cfg).on == 0 {
        return 1;
    }
    let (id, a) = app(&*cfg);
    if a.is_null() {
        return 1;
    }
    let (l, lan) = list(&*a, fam, &addr);
    let r = np_decide(
        &*a,
        l.as_ref(),
        dir,
        pr,
        port,
        lan,
        listed(&*a, fam, &addr),
        np_is_loop(fam, &addr),
    );
    let deny = r.verdict == 1;
    let sk = NpSeenKey {
        cgid: id,
        addr,
        port: port as u16,
        fam: fam as u8,
        proto: pr as u8,
        dir: dir as u8,
        verdict: deny as u8,
        pad: [0; 2],
    };
    let now = clock();
    let last: *mut u64 = lookup(ptr::addr_of!(np_seen), &sk);
    let fresh = last.is_null() || now.wrapping_sub(*last) > NP_REPEAT_NS;
    if always || fresh {
        count(id, deny)
    }
    if fresh {
        update(ptr::addr_of!(np_seen), &sk, &now, 0);
        emit(&NpEvent {
            cgid: id,
            ts: now,
            addr,
            port: port as u16,
            fam: fam as u8,
            proto: pr as u8,
            dir: dir as u8,
            verdict: r.verdict as u8,
            reason: r.reason as u8,
            record: r.record as u8,
            rule_id: r.rule_id,
            flag: r.flag,
            ..NpEvent::default()
        })
    }
    (!deny) as i32
}
#[repr(C)]
pub struct SockAddr {
    user_family: u32,
    user_ip4: u32,
    user_ip6: [u32; 4],
    user_port: u32,
    family: u32,
    ty: u32,
    protocol: u32,
}
#[inline(always)]
fn addr(ctx: &SockAddr, v6: bool) -> (i32, [u8; 16]) {
    let mut a = [0; 16];
    if !v6 {
        a[..4].copy_from_slice(&ctx.user_ip4.to_le_bytes());
        return (4, a);
    }
    let mut i = 0;
    while i < 4 {
        a[i * 4..i * 4 + 4].copy_from_slice(&ctx.user_ip6[i].to_le_bytes());
        i += 1
    }
    if ctx.user_ip6[0] == 0 && ctx.user_ip6[1] == 0 && ctx.user_ip6[2] == 0xffff0000 {
        let mut b = [0; 16];
        b[..4].copy_from_slice(&a[12..]);
        (4, b)
    } else {
        (6, a)
    }
}
#[inline(always)]
unsafe fn connect(ctx: *mut SockAddr, v6: bool, send: bool) -> i32 {
    let c = &*ctx;
    let (f, a) = addr(c, v6);
    let p = if send { 2 } else { proto(c.protocol) };
    handle(
        f,
        a,
        (c.user_port as u16).swap_bytes() as i32,
        p,
        0,
        !send && p != 2,
    )
}
macro_rules! hook {
    ($name:ident,$sec:literal,$ty:ty,$body:expr) => {
        #[no_mangle]
        #[link_section=$sec]
        pub unsafe extern "C" fn $name(ctx: *mut $ty) -> i32 {
            ($body)(ctx)
        }
    };
}
hook!(np_connect4, "cgroup/connect4", SockAddr, |c| connect(
    c, false, false
));
hook!(np_connect6, "cgroup/connect6", SockAddr, |c| connect(
    c, true, false
));
hook!(np_sendmsg4, "cgroup/sendmsg4", SockAddr, |c| connect(
    c, false, true
));
hook!(np_sendmsg6, "cgroup/sendmsg6", SockAddr, |c| connect(
    c, true, true
));
#[inline(always)]
unsafe fn bound(port: i32, pr: i32) {
    let cfg: *mut NpCfg = lookup(ptr::addr_of!(np_cfg), &mut 0u32);
    if cfg.is_null() {
        return;
    }
    let id = ancestor((*cfg).level);
    if id == 0 || lookup::<NpCount, _>(ptr::addr_of!(np_counts), &id).is_null() {
        return;
    }
    let k = NpBoundKey {
        cgid: id,
        port: port as u16,
        proto: pr as u8,
        pad: [0; 5],
    };
    update(ptr::addr_of!(np_bound), &k, &mut 1u8, 0);
}
#[inline(always)]
unsafe fn bind(ctx: *mut SockAddr, v6: bool) -> i32 {
    let c = &*ctx;
    let p = (c.user_port as u16).swap_bytes() as i32;
    if p == 0 {
        return 1;
    }
    let (mut f, mut a) = addr(c, v6);
    if !np_is_loop(f, &a) {
        f = 0;
        a = [0; 16]
    }
    let pr = proto(c.protocol);
    let ok = handle(f, a, p, pr, 1, true);
    if ok != 0 {
        bound(p, pr)
    }
    ok
}
hook!(np_bind4, "cgroup/bind4", SockAddr, |c| bind(c, false));
hook!(np_bind6, "cgroup/bind6", SockAddr, |c| bind(c, true));
// Linux UAPI bpf_sock: offsets through dst_port and state.
#[repr(C)]
pub struct Sock {
    bound_dev_if: u32,
    family: u32,
    ty: u32,
    protocol: u32,
    mark: u32,
    priority: u32,
    src_ip4: u32,
    src_ip6: [u32; 4],
    src_port: u32,
    dst_port: u16,
    pad: u16,
    dst_ip4: u32,
    dst_ip6: [u32; 4],
    state: u32,
}
hook!(
    np_sock_create,
    "cgroup/sock_create",
    Sock,
    |c: *mut Sock| {
        if (*c).family != 2 && (*c).family != 10 {
            return 1;
        }
        let cfg: *mut NpCfg = lookup(ptr::addr_of!(np_cfg), &mut 0u32);
        if cfg.is_null() || (*cfg).on == 0 {
            return 1;
        }
        let id = ancestor((*cfg).level);
        let a: *mut NpApp = lookup(ptr::addr_of!(np_apps), &id);
        if id == 0 || a.is_null() || (*a).flags & NP_R_NO_NETWORK == 0 {
            return 1;
        }
        handle(0, [0; 16], 0, proto((*c).protocol), 0, true);
        0
    }
);
#[inline(always)]
unsafe fn report_dev(id: u64, dir: u8, major: u32, minor: u32, flag: u32) {
    let mut addr = [0; 16];
    addr[..4].copy_from_slice(&minor.to_le_bytes());
    let k = NpSeenKey {
        cgid: id,
        addr,
        port: major as u16,
        dir,
        verdict: 1,
        ..NpSeenKey::default()
    };
    let now = clock();
    let last: *mut u64 = lookup(ptr::addr_of!(np_seen), &k);
    if !last.is_null() && now.wrapping_sub(*last) <= NP_REPEAT_NS {
        return;
    }
    update(ptr::addr_of!(np_seen), &k, &now, 0);
    emit(&NpEvent {
        cgid: id,
        ts: now,
        addr,
        port: major as u16,
        proto: 3,
        dir,
        verdict: 1,
        reason: 0,
        record: 1,
        flag,
        ..NpEvent::default()
    })
}
#[repr(C)]
pub struct Device {
    access_type: u32,
    major: u32,
    minor: u32,
}
hook!(np_dev, "cgroup/dev", Device, |c: *mut Device| {
    let c = &*c;
    let cfg: *mut NpCfg = lookup(ptr::addr_of!(np_cfg), &mut 0u32);
    if cfg.is_null() || (*cfg).on == 0 {
        return 1;
    }
    let cfg = &*cfg;
    let mut need = np_dev_restriction(c.access_type & 0xffff, c.major, cfg.hidraw_major);
    let snd = need == 0 && c.access_type & 0xffff == 2 && c.major == 116;
    if need == 0 && !snd {
        return 1;
    }
    let (id, a) = app(cfg);
    if a.is_null() {
        return 1;
    }
    if snd {
        let k: *mut u32 = lookup(ptr::addr_of!(np_snd), &c.minor);
        let sid = ancestor(cfg.level);
        if if k.is_null() {
            sid != 0 && sid == id
        } else {
            *k == 2
        } {
            need = NP_R_NO_MIC
        } else {
            return 1;
        }
    }
    if (*a).flags & need == 0 {
        return 1;
    }
    report_dev(id, 2, c.major, c.minor, need);
    0
});
hook!(
    np_bt_socket,
    "cgroup/lsm_socket_create",
    u64,
    |c: *mut u64| {
        if *c != 31 || *c.add(3) != 0 {
            return 1;
        }
        let cfg: *mut NpCfg = lookup(ptr::addr_of!(np_cfg), &mut 0u32);
        if cfg.is_null() || (*cfg).on == 0 {
            return 1;
        }
        let (id, a) = app(&*cfg);
        if a.is_null() || (*a).flags & NP_R_NO_BLUETOOTH == 0 {
            return 1;
        }
        report_dev(id, 3, 0, 0, NP_R_NO_BLUETOOTH);
        0
    }
);
// Read only the UAPI offsets we need; sk is at offset 168 of __sk_buff.
#[inline(always)]
unsafe fn word(ctx: *mut u8, off: usize) -> u32 {
    ptr::read(ctx.add(off).cast())
}
#[inline(always)]
unsafe fn load(ctx: *mut u8, off: u32, out: *mut u8, len: u32) -> i64 {
    let f: unsafe extern "C" fn(*mut u8, u32, *mut u8, u32) -> i64 = transmute(26usize);
    f(ctx, off, out, len)
}
#[inline(always)]
unsafe fn skb_id(ctx: *mut u8, level: u32) -> u64 {
    let f: unsafe extern "C" fn(*mut u8, i32) -> u64 = transmute(83usize);
    f(ctx, level as i32)
}
#[inline(always)]
unsafe fn report_cut(
    ctx: *mut u8,
    id: u64,
    k: &NpFlowKey,
    la: [u8; 16],
    lp: i32,
    rp: i32,
    pr: i32,
    dir: i32,
    port: i32,
    r: &NpRes,
) {
    let cookie_fn: unsafe extern "C" fn(*mut u8) -> u64 = transmute(46usize);
    let cookie = cookie_fn(ctx);
    let mut sk = NpSeenKey {
        cgid: id,
        dir: 4,
        verdict: 1,
        ..NpSeenKey::default()
    };
    sk.addr[..8].copy_from_slice(&cookie.to_le_bytes());
    let now = clock();
    let last: *mut u64 = lookup(ptr::addr_of!(np_seen), &sk);
    if !last.is_null() && now.wrapping_sub(*last) <= NP_REPEAT_NS {
        return;
    }
    let first = last.is_null();
    update(ptr::addr_of!(np_seen), &sk, &now, 0);
    if first {
        count(id, true)
    }
    let mut ev = NpEvent {
        cgid: id,
        ts: now,
        addr: k.addr,
        laddr: la,
        port: port as u16,
        lport: lp as u16,
        rport: rp as u16,
        fam: k.fam,
        proto: pr as u8,
        dir: dir as u8,
        verdict: 1,
        reason: r.reason as u8,
        record: r.record as u8,
        rule_id: r.rule_id,
        flag: r.flag,
        cut: if first { 1 } else { 2 },
        cookie,
        ..NpEvent::default()
    };
    let mut s: *mut Sock = ptr::read(ctx.add(168).cast());
    if !s.is_null() {
        let f: unsafe extern "C" fn(*mut Sock) -> *mut Sock = transmute(95usize);
        s = f(s)
    }
    if !s.is_null() {
        ev.destroy = if pr == 1 {
            ((*s).state != 10) as u8
        } else {
            (pr == 2 && (*s).dst_port != 0) as u8
        }
    }
    emit(&ev)
}
#[inline(always)]
unsafe fn filter(ctx: *mut u8, input: bool) -> i32 {
    if word(ctx, 40) == 1 {
        return 1;
    }
    let cfg: *mut NpCfg = lookup(ptr::addr_of!(np_cfg), &mut 0u32);
    if cfg.is_null() {
        return 1;
    }
    let id = skb_id(ctx, (*cfg).level);
    let c: *mut NpCount = if id != 0 {
        lookup(ptr::addr_of!(np_counts), &id)
    } else {
        ptr::null_mut()
    };
    let len = word(ctx, 0) as u64;
    if c.is_null() {
        let f: unsafe extern "C" fn(*mut u8) -> u64 = transmute(79usize);
        let id = f(ctx);
        let c: *mut NpCount = lookup(ptr::addr_of!(np_sysc), &id);
        if !c.is_null() {
            if input {
                add(ptr::addr_of_mut!((*c).rx), len)
            } else {
                add(ptr::addr_of_mut!((*c).tx), len)
            }
        }
        return 1;
    }
    let mut h = [0u8; 40];
    if load(ctx, 0, h.as_mut_ptr(), 20) != 0 {
        return 1;
    }
    let mut k = NpFlowKey {
        cgid: id,
        ..NpFlowKey::default()
    };
    let mut la = [0; 16];
    let mut l4 = -1;
    let pr;
    if h[0] >> 4 == 6 {
        if load(ctx, 0, h.as_mut_ptr(), 40) != 0 {
            return 1;
        }
        k.addr
            .copy_from_slice(if input { &h[8..24] } else { &h[24..40] });
        la.copy_from_slice(if input { &h[24..40] } else { &h[8..24] });
        k.fam = 6;
        pr = proto(h[6] as u32);
        l4 = 40;
    } else if h[0] >> 4 == 4 {
        k.addr[..4].copy_from_slice(if input { &h[12..16] } else { &h[16..20] });
        la[..4].copy_from_slice(if input { &h[16..20] } else { &h[12..16] });
        k.fam = 4;
        pr = proto(h[9] as u32);
        if ((h[6] as u32) << 8 | h[7] as u32) & 0x1fff == 0 {
            l4 = (h[0] & 15) as i32 * 4
        }
    } else {
        return 1;
    }
    let (mut lp, mut rp, mut rst) = (0, 0, false);
    if l4 >= 0 && pr != 3 {
        let mut p = [0u8; 14];
        if load(ctx, l4 as u32, p.as_mut_ptr(), if pr == 1 { 14 } else { 4 }) == 0 {
            let sp = u16::from_be_bytes([p[0], p[1]]) as i32;
            let dp = u16::from_be_bytes([p[2], p[3]]) as i32;
            lp = if input { dp } else { sp };
            rp = if input { sp } else { dp };
            rst = pr == 1 && p[13] & 4 != 0
        }
    }
    let (mut dir, mut port) = (0, rp);
    if lp != 0 {
        let bk = NpBoundKey {
            cgid: id,
            port: lp as u16,
            proto: pr as u8,
            pad: [0; 5],
        };
        if !lookup::<u8, _>(ptr::addr_of!(np_bound), &bk).is_null() {
            dir = 1;
            port = lp
        }
    }
    let mut r = NpRes::default();
    let mut deny = false;
    let a: *mut NpApp = if (*cfg).on != 0 {
        lookup(ptr::addr_of!(np_apps), &id)
    } else {
        ptr::null_mut()
    };
    if !a.is_null() && (*a).can_deny != 0 {
        let (l, lan) = list(&*a, k.fam as i32, &k.addr);
        r = np_decide(
            &*a,
            l.as_ref(),
            dir,
            pr,
            port,
            lan,
            listed(&*a, k.fam as i32, &k.addr),
            np_is_loop(k.fam as i32, &k.addr),
        );
        deny = r.verdict == 1 && !rst
    }
    k.proto = pr as u8;
    k.dir = dir as u8;
    k.port = port as u16;
    let mut f: *mut NpFlow = lookup(ptr::addr_of!(np_flows), &k);
    if f.is_null() {
        update(ptr::addr_of!(np_flows), &k, &NpFlow::default(), 1);
        f = lookup(ptr::addr_of!(np_flows), &k)
    }
    if deny {
        add(ptr::addr_of_mut!((*c).blocked), len);
        if !f.is_null() {
            add(ptr::addr_of_mut!((*f).blocked), len)
        }
        report_cut(ctx, id, &k, la, lp, rp, pr, dir, port, &r);
        return 0;
    }
    if input {
        add(ptr::addr_of_mut!((*c).rx), len);
        if !f.is_null() {
            add(ptr::addr_of_mut!((*f).rx), len)
        }
    } else {
        add(ptr::addr_of_mut!((*c).tx), len);
        if !f.is_null() {
            add(ptr::addr_of_mut!((*f).tx), len)
        }
    }
    1
}
hook!(np_skb_in, "cgroup/skb_ingress", u8, |c| filter(c, true));
hook!(np_skb_out, "cgroup/skb_egress", u8, |c| filter(c, false));
