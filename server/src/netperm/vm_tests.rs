//! Executes the actual embedded eBPF instructions with in-memory kernel helpers.
//! No capabilities, network sockets, cgroups or host firewall state are touched.
use crate::{bpf, compile::NpNode, shared::*};
use std::collections::BTreeMap;
struct Region {
    base: u64,
    b: Vec<u8>,
    init: Vec<bool>,
}
struct Map {
    name: &'static str,
    k: usize,
    v: usize,
    entries: BTreeMap<Vec<u8>, u64>,
}
struct Vm {
    regions: Vec<Region>,
    maps: Vec<Map>,
    events: Vec<Vec<u8>>,
    now: u64,
    scope: u64,
    user: u64,
    socket_cg: u64,
    packet: Vec<u8>,
    cookie: u64,
}
fn bytes<T: Copy>(v: &T) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts((v as *const T).cast(), size_of::<T>()).to_vec() }
}
impl Vm {
    fn new() -> Self {
        let defs = [
            ("np_cfg", 4, 16),
            ("np_apps", 8, size_of::<NpApp>()),
            ("np_union", 8, size_of::<NpApp>()),
            ("np_lpm4", 12, size_of::<NpList>()),
            ("np_lpm6", 24, size_of::<NpList>()),
            ("np_seen", 32, 8),
            ("np_counts", 8, 40),
            ("np_events", 0, 0),
            ("np_snd", 4, 4),
            ("np_flows", 32, 24),
            ("np_bound", 16, 1),
            ("np_blk4", 12, 1),
            ("np_blk6", 24, 1),
            ("np_sysc", 8, 40),
        ];
        let mut v = Self {
            regions: vec![],
            maps: defs
                .into_iter()
                .map(|(name, k, v)| Map {
                    name,
                    k,
                    v,
                    entries: BTreeMap::new(),
                })
                .collect(),
            events: vec![],
            now: 10_000_000_000,
            scope: 42,
            user: 99,
            socket_cg: 42,
            packet: vec![],
            cookie: 123,
        };
        v.set(
            "np_cfg",
            &0u32,
            &NpCfg {
                level: 4,
                on: 1,
                user_level: 3,
                hidraw_major: 240,
            },
        );
        v
    }
    fn alloc(&mut self, b: Vec<u8>) -> u64 {
        let base = 0x10000 + self.regions.len() as u64 * 0x10000;
        let n = b.len();
        self.regions.push(Region {
            base,
            b,
            init: vec![true; n],
        });
        base
    }
    fn read(&self, p: u64, n: usize) -> Vec<u8> {
        let r = self
            .regions
            .iter()
            .find(|r| {
                p >= r.base
                    && p - r.base <= r.b.len() as u64
                    && n as u64 <= r.b.len() as u64 - (p - r.base)
            })
            .unwrap_or_else(|| panic!("out-of-bounds BPF read {p:x}, {n}"));
        let o = (p - r.base) as usize;
        assert!(
            r.init[o..o + n].iter().all(|&b| b),
            "uninitialized BPF stack read at {p:x}, len {n}"
        );
        r.b[o..o + n].to_vec()
    }
    fn write(&mut self, p: u64, b: &[u8]) {
        let r = self
            .regions
            .iter_mut()
            .find(|r| {
                p >= r.base
                    && p - r.base <= r.b.len() as u64
                    && b.len() as u64 <= r.b.len() as u64 - (p - r.base)
            })
            .unwrap_or_else(|| panic!("out-of-bounds BPF write {p:x}, {}", b.len()));
        let o = (p - r.base) as usize;
        r.b[o..o + b.len()].copy_from_slice(b);
        r.init[o..o + b.len()].fill(true);
    }
    fn map(&self, n: &str) -> usize {
        self.maps.iter().position(|m| m.name == n).unwrap()
    }
    fn set<K: Copy, V: Copy>(&mut self, n: &str, k: &K, v: &V) {
        let i = self.map(n);
        let k = bytes(k);
        let b = bytes(v);
        assert_eq!(k.len(), self.maps[i].k);
        assert_eq!(b.len(), self.maps[i].v);
        if let Some(&p) = self.maps[i].entries.get(&k) {
            self.write(p, &b)
        } else {
            let p = self.alloc(b);
            self.maps[i].entries.insert(k, p);
        }
    }
    fn get<K: Copy, V: Copy + Default>(&self, n: &str, k: &K) -> Option<V> {
        let m = &self.maps[self.map(n)];
        let p = *m.entries.get(&bytes(k))?;
        let b = self.read(p, size_of::<V>());
        Some(unsafe { std::ptr::read_unaligned(b.as_ptr().cast()) })
    }
    fn lookup(&self, i: usize, key: &[u8]) -> u64 {
        let m = &self.maps[i];
        if matches!(m.name, "np_lpm4" | "np_lpm6" | "np_blk4" | "np_blk6") {
            let prefix = u32::from_ne_bytes(key[..4].try_into().unwrap());
            m.entries
                .iter()
                .filter_map(|(k, &p)| {
                    let n = u32::from_ne_bytes(k[..4].try_into().unwrap());
                    if n > prefix || n as usize > (k.len() - 4) * 8 {
                        return None;
                    }
                    if (0..n as usize)
                        .all(|bit| (k[4 + bit / 8] ^ key[4 + bit / 8]) & (128 >> (bit % 8)) == 0)
                    {
                        Some((n, p))
                    } else {
                        None
                    }
                })
                .max_by_key(|x| x.0)
                .map_or(0, |x| x.1)
        } else {
            m.entries.get(key).copied().unwrap_or(0)
        }
    }
    fn helper(&mut self, id: i32, r: &[u64; 11]) -> u64 {
        match id {
            1 => {
                let i = r[1] as usize - 1;
                let k = self.read(r[2], self.maps[i].k);
                self.lookup(i, &k)
            }
            2 => {
                let i = r[1] as usize - 1;
                let k = self.read(r[2], self.maps[i].k);
                let v = self.read(r[3], self.maps[i].v);
                if r[4] == 1 && self.maps[i].entries.contains_key(&k) {
                    return (-17i64) as u64;
                }
                if let Some(&p) = self.maps[i].entries.get(&k) {
                    self.write(p, &v)
                } else {
                    let p = self.alloc(v);
                    self.maps[i].entries.insert(k, p);
                }
                0
            }
            5 => self.now,
            123 => {
                if r[1] == 4 {
                    self.scope
                } else if r[1] == 3 {
                    self.user
                } else {
                    0
                }
            }
            130 => {
                self.events.push(self.read(r[2], r[3] as usize));
                0
            }
            26 => {
                let o = r[2] as usize;
                let n = r[4] as usize;
                if o > self.packet.len() || n > self.packet.len() - o {
                    (-14i64) as u64
                } else {
                    let b = self.packet[o..o + n].to_vec();
                    self.write(r[3], &b);
                    0
                }
            }
            83 => {
                if r[2] == 4 {
                    self.socket_cg
                } else {
                    0
                }
            }
            79 => self.socket_cg,
            46 => self.cookie,
            95 => r[1],
            _ => panic!("unsupported helper {id}"),
        }
    }
    fn run(&mut self, section: &str, ctx: Vec<u8>) -> i32 {
        let p = bpf::PROGRAMS.iter().find(|p| p.section == section).unwrap();
        let mut ins = p.insns.to_vec();
        for &(pc, n) in p.relocs {
            ins[pc] = (ins[pc] & 0xffff_ffff) | ((self.map(n) as u64 + 1) << 32);
            ins[pc + 1] = 0;
        }
        let ctx = self.alloc(ctx);
        let stack = self.alloc(vec![0; 512]);
        self.regions.last_mut().unwrap().init.fill(false);
        let mut r = [0u64; 11];
        r[1] = ctx;
        r[10] = stack + 512;
        let mut pc = 0usize;
        for _ in 0..100_000 {
            let raw = ins[pc];
            let op = raw as u8;
            let d = (raw >> 8 & 15) as usize;
            let s = (raw >> 12 & 15) as usize;
            let off = (raw >> 16) as i16 as i64;
            let imm = (raw >> 32) as i32;
            let src = if op & 8 != 0 { r[s] } else { imm as i64 as u64 };
            pc += 1;
            match op & 7 {
                0 => {
                    assert_eq!(op, 0x18);
                    r[d] = (imm as u32 as u64) | ((ins[pc] >> 32) << 32);
                    pc += 1;
                }
                1 => {
                    let n = width(op);
                    let b = self.read(r[s].wrapping_add(off as u64), n);
                    let mut v = [0; 8];
                    v[..n].copy_from_slice(&b);
                    r[d] = u64::from_le_bytes(v);
                }
                2 => {
                    let n = width(op);
                    self.write(
                        r[d].wrapping_add(off as u64),
                        &(imm as i64 as u64).to_le_bytes()[..n],
                    );
                }
                3 => {
                    let n = width(op);
                    let addr = r[d].wrapping_add(off as u64);
                    if op & 0xe0 == 0xc0 {
                        let mut old = [0; 8];
                        old[..n].copy_from_slice(&self.read(addr, n));
                        let old = u64::from_le_bytes(old);
                        assert_eq!(imm & 0xf0, 0);
                        self.write(addr, &old.wrapping_add(r[s]).to_le_bytes()[..n]);
                        if imm & 1 != 0 {
                            r[s] = old
                        }
                    } else {
                        self.write(addr, &r[s].to_le_bytes()[..n])
                    }
                }
                4 | 7 => {
                    let small = op & 7 == 4;
                    let a = if small { r[d] as u32 as u64 } else { r[d] };
                    let b = if small { src as u32 as u64 } else { src };
                    let v = match op & 0xf0 {
                        0 => a.wrapping_add(b),
                        0x10 => a.wrapping_sub(b),
                        0x20 => a.wrapping_mul(b),
                        0x30 => {
                            if b == 0 {
                                0
                            } else {
                                a / b
                            }
                        }
                        0x40 => a | b,
                        0x50 => a & b,
                        0x60 => a.wrapping_shl(b as u32),
                        0x70 => a.wrapping_shr(b as u32),
                        0x80 => a.wrapping_neg(),
                        0x90 => {
                            if b == 0 {
                                a
                            } else {
                                a % b
                            }
                        }
                        0xa0 => a ^ b,
                        0xb0 => b,
                        0xc0 => {
                            if small {
                                ((a as i32) >> (b as u32)) as u32 as u64
                            } else {
                                ((a as i64) >> (b as u32)) as u64
                            }
                        }
                        0xd0 => {
                            assert_eq!(op & 8, 8);
                            match imm {
                                16 => (a as u16).swap_bytes() as u64,
                                32 => (a as u32).swap_bytes() as u64,
                                64 => r[d].swap_bytes(),
                                _ => panic!("bad endian width"),
                            }
                        }
                        _ => panic!("unsupported ALU {op:x}"),
                    };
                    r[d] = if small && !(op & 0xf0 == 0xd0 && imm == 64) {
                        v as u32 as u64
                    } else {
                        v
                    };
                }
                5 | 6 => {
                    if op == 0x85 {
                        r[0] = self.helper(imm, &r);
                        r[1..6].fill(0);
                        continue;
                    }
                    if op == 0x95 {
                        return r[0] as i32;
                    }
                    let (a, b, sa, sb) = if op & 7 == 6 {
                        (
                            r[d] as u32 as u64,
                            src as u32 as u64,
                            r[d] as i32 as i64,
                            src as i32 as i64,
                        )
                    } else {
                        (r[d], src, r[d] as i64, src as i64)
                    };
                    let yes = match op & 0xf0 {
                        0 => true,
                        0x10 => a == b,
                        0x20 => a > b,
                        0x30 => a >= b,
                        0x40 => a & b != 0,
                        0x50 => a != b,
                        0x60 => sa > sb,
                        0x70 => sa >= sb,
                        0xa0 => a < b,
                        0xb0 => a <= b,
                        0xc0 => sa < sb,
                        0xd0 => sa <= sb,
                        _ => panic!("unsupported jump {op:x}"),
                    };
                    if yes {
                        pc = (pc as i64 + off) as usize
                    }
                }
                _ => panic!("unsupported opcode {op:x}"),
            }
        }
        panic!("BPF instruction limit exceeded: {section}")
    }
}
fn width(op: u8) -> usize {
    match op & 0x18 {
        0 => 4,
        8 => 2,
        16 => 1,
        24 => 8,
        _ => unreachable!(),
    }
}
fn ctx4(ip: [u8; 4], port: u16, pr: u32) -> Vec<u8> {
    let mut c = vec![0; 40];
    c[4..8].copy_from_slice(&ip);
    c[24..28].copy_from_slice(&(port.to_be() as u32).to_le_bytes());
    c[36..40].copy_from_slice(&pr.to_le_bytes());
    c
}
#[test]
fn compiled_connect_and_sendmsg_match_userspace_decisions() {
    let mut state = 0x123456789u64;
    let mut rnd = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as u32
    };
    for _ in 0..600 {
        let mut vm = Vm::new();
        let mut a = NpApp {
            idx: 7,
            flags: rnd() & 15,
            trusted: (rnd() % 2) as u8,
            mode: [0, 2, 3][rnd() as usize % 3],
            ngen: 4,
            ..Default::default()
        };
        for e in &mut a.gen[..4] {
            *e = NpEnt {
                port_lo: 0,
                port_hi: if rnd() % 2 == 0 { 65535 } else { 443 },
                proto: (rnd() % 3) as u8,
                dir: (rnd() % 2) as u8,
                verdict: (rnd() % 2) as u8,
                kind: (rnd() % 3 + 1) as u8,
                rule_id: rnd(),
            };
        }
        let ip = if rnd() % 2 == 0 {
            [8, 8, 8, 8]
        } else {
            [192, 168, 1, 2]
        };
        let mut addr = [0; 16];
        addr[..4].copy_from_slice(&ip);
        vm.set("np_apps", &42u64, &a);
        let port = if rnd() % 2 == 0 { 443 } else { 8080 };
        let want = np_decide(&a, None, 0, 1, port, np_is_lan4(&addr) as i32, false, false);
        assert_eq!(
            vm.run("cgroup/connect4", ctx4(ip, port as u16, 6)),
            1 - want.verdict
        );
        let ev: NpEvent =
            unsafe { std::ptr::read_unaligned(vm.events.last().unwrap().as_ptr().cast()) };
        assert_eq!(
            (ev.verdict, ev.reason, ev.rule_id, ev.flag),
            (
                want.verdict as u8,
                want.reason as u8,
                want.rule_id,
                want.flag
            )
        );
        assert_eq!(ev.addr, addr);
        assert_eq!(ev.port, port as u16);
        let before: NpCount = vm.get("np_counts", &42u64).unwrap();
        vm.run("cgroup/sendmsg4", ctx4(ip, port as u16, 17));
        let first: NpCount = vm.get("np_counts", &42u64).unwrap();
        vm.run("cgroup/sendmsg4", ctx4(ip, port as u16, 17));
        let second: NpCount = vm.get("np_counts", &42u64).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.allowed + first.denied,
            before.allowed + before.denied + 1
        );
    }
}
#[test]
fn device_bluetooth_union_and_incoming_hooks() {
    let mut vm = Vm::new();
    vm.set(
        "np_apps",
        &42u64,
        &NpApp {
            flags: NP_R_DEV_MASK | NP_R_NO_INCOMING,
            ..Default::default()
        },
    );
    let mut dev = vec![0; 12];
    dev[..4].copy_from_slice(&2u32.to_le_bytes());
    dev[4..8].copy_from_slice(&81u32.to_le_bytes());
    assert_eq!(vm.run("cgroup/dev", dev.clone()), 0);
    dev[4..8].copy_from_slice(&116u32.to_le_bytes());
    dev[8..12].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(vm.run("cgroup/dev", dev.clone()), 0);
    vm.set("np_snd", &1u32, &1u32);
    assert_eq!(vm.run("cgroup/dev", dev.clone()), 1);
    vm.set("np_snd", &1u32, &2u32);
    assert_eq!(vm.run("cgroup/dev", dev), 0);
    let mut bt = vec![0; 32];
    bt[..8].copy_from_slice(&31u64.to_le_bytes());
    assert_eq!(vm.run("cgroup/lsm_socket_create", bt.clone()), 0);
    bt[24..32].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(vm.run("cgroup/lsm_socket_create", bt), 1);
    assert_eq!(vm.run("cgroup/bind4", ctx4([0; 4], 8080, 6)), 0);
    assert_eq!(vm.run("cgroup/bind4", ctx4([0; 4], 0, 6)), 1);
    vm.scope = 7;
    vm.set(
        "np_union",
        &99u64,
        &NpApp {
            flags: NP_R_NO_NETWORK,
            ..Default::default()
        },
    );
    assert_eq!(vm.run("cgroup/connect4", ctx4([8; 4], 443, 6)), 0);
}
#[test]
fn tries_blocklists_and_ipv4_mapped_ipv6() {
    let mut vm = Vm::new();
    let a = NpApp {
        idx: 7,
        has4: 1,
        blk: 9,
        mode: 3,
        ..Default::default()
    };
    vm.set("np_apps", &42u64, &a);
    let node = NpNode {
        fam: 4,
        prefix: 24,
        addr: [203, 0, 113, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        list: NpList {
            n: 1,
            e: {
                let mut e = [NpEnt::default(); 16];
                e[0] = NpEnt {
                    port_hi: 65535,
                    verdict: 0,
                    rule_id: 17,
                    ..Default::default()
                };
                e
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let k = crate::compile::key4(7, &node);
    vm.set("np_lpm4", &k, &node.list);
    let bk = crate::compile::key4(9, &node);
    vm.set("np_blk4", &bk, &1u8);
    assert_eq!(vm.run("cgroup/connect4", ctx4([203, 0, 113, 5], 443, 6)), 1);
    assert_eq!(vm.run("cgroup/connect4", ctx4([8, 8, 8, 8], 443, 6)), 0);
    let mut c = ctx4([0; 4], 443, 6);
    c[16..20].copy_from_slice(&[0, 0, 255, 255]);
    c[20..24].copy_from_slice(&[203, 0, 113, 5]);
    assert_eq!(vm.run("cgroup/connect6", c), 1);
}
#[test]
fn skb_counts_drops_and_reports_open_sockets() {
    let mut vm = Vm::new();
    vm.set("np_counts", &42u64, &NpCount::default());
    vm.set(
        "np_apps",
        &42u64,
        &NpApp {
            flags: NP_R_NO_NETWORK,
            can_deny: 1,
            ..Default::default()
        },
    );
    let mut ctx = vec![0; 176];
    ctx[..4].copy_from_slice(&40u32.to_le_bytes());
    ctx[40..44].copy_from_slice(&2u32.to_le_bytes());
    let mut pkt = vec![0; 40];
    pkt[0] = 0x45;
    pkt[9] = 6;
    pkt[12..16].copy_from_slice(&[192, 168, 1, 2]);
    pkt[16..20].copy_from_slice(&[8, 8, 8, 8]);
    pkt[20..22].copy_from_slice(&1234u16.to_be_bytes());
    pkt[22..24].copy_from_slice(&443u16.to_be_bytes());
    vm.packet = pkt;
    assert_eq!(vm.run("cgroup/skb_egress", ctx.clone()), 0);
    let c: NpCount = vm.get("np_counts", &42u64).unwrap();
    assert_eq!((c.blocked, c.denied, c.tx), (40, 1, 0));
    vm.run("cgroup/skb_egress", ctx.clone());
    let c: NpCount = vm.get("np_counts", &42u64).unwrap();
    assert_eq!((c.blocked, c.denied), (80, 1));
    vm.packet[33] = 4;
    assert_eq!(vm.run("cgroup/skb_egress", ctx.clone()), 1);
    let c: NpCount = vm.get("np_counts", &42u64).unwrap();
    assert_eq!(c.tx, 40);
    ctx[40..44].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(vm.run("cgroup/skb_egress", ctx), 1);
    let c2: NpCount = vm.get("np_counts", &42u64).unwrap();
    assert_eq!(c, c2);
}
#[test]
fn ipv6_send_bind_creation_and_ingress_use_their_uapi_contexts() {
    let mut vm = Vm::new();
    vm.set(
        "np_apps",
        &42u64,
        &NpApp {
            flags: NP_R_NO_NETWORK,
            ..Default::default()
        },
    );
    let mut c = ctx4([0; 4], 443, 6);
    c[8..24].copy_from_slice(
        &"2001:db8::5"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    assert_eq!(vm.run("cgroup/sendmsg6", c.clone()), 0);
    assert_eq!(vm.run("cgroup/bind6", c), 0);
    let mut sock = vec![0; 76];
    sock[4..8].copy_from_slice(&2u32.to_le_bytes());
    sock[12..16].copy_from_slice(&6u32.to_le_bytes());
    assert_eq!(vm.run("cgroup/sock_create", sock), 0);
    vm.set("np_apps", &42u64, &NpApp::default());
    vm.set("np_counts", &42u64, &NpCount::default());
    let mut ctx = vec![0; 176];
    ctx[..4].copy_from_slice(&60u32.to_le_bytes());
    ctx[40..44].copy_from_slice(&2u32.to_le_bytes());
    let mut pkt = vec![0; 60];
    pkt[0] = 0x60;
    pkt[6] = 17;
    pkt[8..24].copy_from_slice(
        &"2001:db8::5"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    pkt[24..40].copy_from_slice(
        &"2001:db8::6"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    pkt[40..42].copy_from_slice(&443u16.to_be_bytes());
    pkt[42..44].copy_from_slice(&1234u16.to_be_bytes());
    vm.packet = pkt;
    assert_eq!(vm.run("cgroup/skb_ingress", ctx.clone()), 1);
    let n: NpCount = vm.get("np_counts", &42u64).unwrap();
    assert_eq!(n.rx, 60);
    vm.socket_cg = 77;
    vm.set("np_sysc", &77u64, &NpCount::default());
    assert_eq!(vm.run("cgroup/skb_ingress", ctx), 1);
    let n: NpCount = vm.get("np_sysc", &77u64).unwrap();
    assert_eq!(n.rx, 60);
}
