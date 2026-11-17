//! Wire layouts and decision policy shared verbatim by userspace and the Rust eBPF programs.
#![allow(non_camel_case_types, non_snake_case, dead_code)]
pub const NP_LIST_MAX: u32 = 16;
pub const NP_GEN_MAX: u32 = 16;
pub const NP_MAX_APPS: u32 = 8192;
pub const NP_MAX_LPM: u32 = 65536;
pub const NP_MAX_SEEN: u32 = 16384;
pub const NP_MAX_BOUND: u32 = 4096;
pub const NP_MAX_FLOWS: u32 = 8192;
pub const NP_MAX_BLK: u32 = 131072;
pub const NP_MAX_SYS: u32 = 16;
pub const NP_RINGBUF: u32 = 256 << 10;
pub const NP_REPEAT_NS: u64 = 5 * 1000000000;
pub const NP_R_NO_INTERNET: u32 = 1 << 0;
pub const NP_R_NO_LAN: u32 = 1 << 1;
pub const NP_R_NO_NETWORK: u32 = 1 << 2;
pub const NP_R_NO_INCOMING: u32 = 1 << 3;
pub const NP_R_NO_CAMERA: u32 = 1 << 4;
pub const NP_R_NO_MIC: u32 = 1 << 5;
pub const NP_R_NO_SCREEN: u32 = 1 << 6;
pub const NP_R_NO_USB: u32 = 1 << 7;
pub const NP_R_NO_BLUETOOTH: u32 = 1 << 8;
pub const NP_R_NET_MASK: u32 = NP_R_NO_INTERNET | NP_R_NO_LAN | NP_R_NO_NETWORK | NP_R_NO_INCOMING;
pub const NP_R_DEV_MASK: u32 =
    NP_R_NO_CAMERA | NP_R_NO_MIC | NP_R_NO_SCREEN | NP_R_NO_USB | NP_R_NO_BLUETOOTH;
pub const NP_MAJOR_FB: u32 = 29;
pub const NP_MAJOR_SND: u32 = 116;
pub const NP_SND_OTHER: u32 = 1;
pub const NP_SND_CAPTURE: u32 = 2;
pub const NP_MAJOR_V4L: u32 = 81;
pub const NP_MAJOR_TTYACM: u32 = 166;
pub const NP_MAJOR_TTYUSB: u32 = 188;
pub const NP_MAJOR_USBDEV: u32 = 189;
pub const NP_DEV_CHAR: u32 = 2;
pub const NP_P_ANY: u32 = 0;
pub const NP_P_TCP: u32 = 1;
pub const NP_P_UDP: u32 = 2;
pub const NP_P_OTHER: u32 = 3;
pub const NP_D_OUT: u32 = 0;
pub const NP_D_IN: u32 = 1;
pub const NP_D_DEV: u32 = 2;
pub const NP_D_BT: u32 = 3;
pub const NP_D_CUT: u32 = 4;
pub const NP_V_ALLOW: u32 = 0;
pub const NP_V_DENY: u32 = 1;
pub const NP_K_RULE: u32 = 0;
pub const NP_K_ANY: u32 = 1;
pub const NP_K_LAN: u32 = 2;
pub const NP_K_INET: u32 = 3;
pub const NP_M_SILENT: u32 = 0;
pub const NP_M_OFF: u32 = 2;
pub const NP_M_DENY: u32 = 3;
pub const NP_BY_RESTRICTED: u32 = 0;
pub const NP_BY_RULE: u32 = 1;
pub const NP_BY_TRUSTED: u32 = 2;
pub const NP_BY_MODE: u32 = 3;
pub const NP_BY_LIST: u32 = 4;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpEnt {
    pub port_lo: u16,
    pub port_hi: u16,
    pub proto: u8,
    pub dir: u8,
    pub verdict: u8,
    pub kind: u8,
    pub rule_id: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpList {
    pub n: u32,
    pub pad: u32,
    pub e: [NpEnt; NP_LIST_MAX as usize],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpApp {
    pub idx: u32,
    pub flags: u32,
    pub trusted: u8,
    pub mode: u8,
    pub has4: u8,
    pub has6: u8,
    pub ngen: u32,
    pub can_deny: u32,
    pub blk: u32,
    pub pad0: u32,
    pub gen: [NpEnt; NP_GEN_MAX as usize],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpLpm4Key {
    pub prefixlen: u32,
    pub data: [u8; 8 as usize],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpLpm6Key {
    pub prefixlen: u32,
    pub data: [u8; 20 as usize],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpCfg {
    pub level: u32,
    pub on: u32,
    pub user_level: u32,
    pub hidraw_major: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpSeenKey {
    pub cgid: u64,
    pub addr: [u8; 16 as usize],
    pub port: u16,
    pub fam: u8,
    pub proto: u8,
    pub dir: u8,
    pub verdict: u8,
    pub pad: [u8; 2 as usize],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpCount {
    pub allowed: u64,
    pub denied: u64,
    pub rx: u64,
    pub tx: u64,
    pub blocked: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpFlowKey {
    pub cgid: u64,
    pub addr: [u8; 16 as usize],
    pub fam: u8,
    pub proto: u8,
    pub dir: u8,
    pub pad: u8,
    pub port: u16,
    pub pad2: [u8; 2 as usize],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpFlow {
    pub rx: u64,
    pub tx: u64,
    pub blocked: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpBoundKey {
    pub cgid: u64,
    pub port: u16,
    pub proto: u8,
    pub pad: [u8; 5 as usize],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpEvent {
    pub cgid: u64,
    pub ts: u64,
    pub addr: [u8; 16 as usize],
    pub port: u16,
    pub fam: u8,
    pub proto: u8,
    pub dir: u8,
    pub verdict: u8,
    pub reason: u8,
    pub record: u8,
    pub rule_id: u32,
    pub flag: u32,
    pub laddr: [u8; 16 as usize],
    pub lport: u16,
    pub rport: u16,
    pub cut: u8,
    pub destroy: u8,
    pub pad2: [u8; 2 as usize],
    pub cookie: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NpRes {
    pub verdict: i32,
    pub reason: i32,
    pub record: i32,
    pub rule_id: u32,
    pub flag: u32,
}
#[inline(always)]
pub fn np_dev_restriction(ty: u32, major: u32, hidraw: u32) -> u32 {
    if ty != NP_DEV_CHAR {
        0
    } else if major == NP_MAJOR_V4L {
        NP_R_NO_CAMERA
    } else if major == NP_MAJOR_FB {
        NP_R_NO_SCREEN
    } else if major == NP_MAJOR_USBDEV
        || major == NP_MAJOR_TTYACM
        || major == NP_MAJOR_TTYUSB
        || (hidraw != 0 && major == hidraw)
    {
        NP_R_NO_USB
    } else {
        0
    }
}
#[inline(always)]
pub fn np_is_lan4(a: &[u8; 16]) -> bool {
    let (b, c) = (a[0], a[1]);
    b == 0
        || b == 10
        || b == 127
        || (b == 100 && c & 0xc0 == 64)
        || (b == 169 && c == 254)
        || (b == 172 && c & 0xf0 == 16)
        || (b == 192 && c == 168)
        || b & 0xf0 == 224
        || (b == 255 && c == 255 && a[2] == 255 && a[3] == 255)
}
#[inline(always)]
pub fn np_is_lan6(a: &[u8; 16]) -> bool {
    let mut any = 0;
    let mut i = 0;
    while i < 15 {
        any |= a[i];
        i += 1;
    }
    (any == 0 && a[15] <= 1)
        || a[0] & 0xfe == 0xfc
        || (a[0] == 0xfe && a[1] & 0xc0 == 0x80)
        || a[0] == 0xff
}
#[inline(always)]
pub fn np_is_loop(fam: i32, a: &[u8; 16]) -> bool {
    if fam == 4 {
        return a[0] == 127;
    }
    if fam != 6 {
        return false;
    }
    let mut any = 0;
    let mut i = 0;
    while i < 15 {
        any |= a[i];
        i += 1;
    }
    any | (a[15] ^ 1) == 0
}
#[inline(always)]
pub fn np_ent_match(e: &NpEnt, dir: i32, proto: i32, port: i32) -> bool {
    e.dir as i32 == dir
        && (e.proto == 0 || e.proto as i32 == proto)
        && port >= e.port_lo as i32
        && port <= e.port_hi as i32
}
#[inline(always)]
pub fn np_decide(
    a: &NpApp,
    l: Option<&NpList>,
    dir: i32,
    proto: i32,
    port: i32,
    lan: i32,
    listed: bool,
    lo: bool,
) -> NpRes {
    let mut r = NpRes {
        verdict: 1,
        reason: 0,
        record: 1,
        rule_id: 0,
        flag: 0,
    };
    let f = a.flags;
    if f & NP_R_NO_NETWORK != 0 {
        r.flag = NP_R_NO_NETWORK;
        return r;
    }
    if dir == 1 && f & NP_R_NO_INCOMING != 0 {
        r.flag = NP_R_NO_INCOMING;
        return r;
    }
    if f & NP_R_NO_INTERNET != 0 && lan != 1 {
        r.flag = NP_R_NO_INTERNET;
        return r;
    }
    if f & NP_R_NO_LAN != 0 && lan == 1 {
        r.flag = NP_R_NO_LAN;
        return r;
    }
    r.record = 0;
    if let Some(l) = l {
        let mut i = 0;
        while i < 16 {
            if i as u32 >= l.n {
                break;
            }
            let e = &l.e[i];
            if np_ent_match(e, dir, proto, port) {
                r.verdict = e.verdict as i32;
                r.reason = 1;
                r.rule_id = e.rule_id;
                return r;
            }
            i += 1;
        }
    }
    if listed {
        r.reason = 4;
        r.record = 1;
        return r;
    }
    let mut i = 0;
    while i < 16 {
        if i as u32 >= a.ngen {
            break;
        }
        let e = &a.gen[i];
        if !(e.kind == 2 && lan != 1 || e.kind == 3 && lan != 0)
            && np_ent_match(e, dir, proto, port)
        {
            r.verdict = e.verdict as i32;
            r.reason = 1;
            r.rule_id = e.rule_id;
            return r;
        }
        i += 1;
    }
    r.verdict = 0;
    if a.trusted != 0 {
        r.reason = 2;
        return r;
    }
    r.reason = 3;
    r.record = (a.mode == 0 || a.mode == 3) as i32;
    if a.mode == 3 && !lo {
        r.verdict = 1
    }
    r
}
pub fn np_ents_deny(e: &[NpEnt]) -> bool {
    e.iter().any(|e| e.verdict == 1)
}
