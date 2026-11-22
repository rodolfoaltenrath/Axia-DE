//! Interfaces, addresses, routes and counters (rtnetlink dumps) as the JSON of `lord.net`, and the
//! notification socket that says when any of them changed.
use crate::config::{if_config_json, Paths};
use crate::netlink::*;
use crate::Out;
use std::collections::HashMap;
use std::ffi::CStr;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd, RawFd};

const IFF_UP: u32 = 1;
const IFF_LOWER_UP: u32 = 0x10000;
const ARPHRD_ETHER: u16 = 1;
const ARPHRD_NONE: u16 = 0xfffe;
const ARPHRD_LOOPBACK: u16 = 772;
const IFLA_ADDRESS: usize = 1;
const IFLA_IFNAME: usize = 3;
const IFLA_MTU: usize = 4;
const IFLA_MASTER: usize = 10;
const IFLA_OPERSTATE: usize = 16;
const IFLA_LINKINFO: usize = 18;
const IFLA_STATS64: usize = 23;
const IFLA_MAX: usize = 63;
const IFLA_INFO_KIND: usize = 1;
const IFA_ADDRESS: usize = 1;
const IFA_LOCAL: usize = 2;
const IFA_CACHEINFO: usize = 6;
const IFA_FLAGS: usize = 8;
const IFA_MAX: usize = 11;
const IFA_F_PERMANENT: u32 = 0x80;
const RTA_DST: usize = 1;
const RTA_OIF: usize = 4;
const RTA_GATEWAY: usize = 5;
const RTA_PRIORITY: usize = 6;
const RTA_TABLE: usize = 15;
const RTA_MAX: usize = 30;
const RT_TABLE_LOCAL: u32 = 255;
const RTN_UNICAST: u8 = 1;
const AF_INET: u8 = 2;
const AF_INET6: u8 = 10;

#[derive(Default, Clone)]
struct Link {
    index: i32,
    flags: u32,
    arphrd: u16,
    mtu: i32,
    master: i32,
    name: Vec<u8>,
    kind: Vec<u8>,
    mac: String,
    oper: &'static str,
    /// rx and tx: bytes, packets, errors.
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
    rx_errors: u64,
    tx_errors: u64,
}

fn oper_name(o: u8) -> &'static str {
    ["unknown", "notpresent", "down", "lowerlayerdown", "testing", "dormant", "up"].get(o as usize).copied().unwrap_or("unknown")
}

/// The text up to its NUL (a kernel string).
fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

fn fmt_ip(family: u8, a: &[u8]) -> String {
    match family {
        AF_INET if a.len() >= 4 => Ipv4Addr::new(a[0], a[1], a[2], a[3]).to_string(),
        AF_INET6 if a.len() >= 16 => {
            let mut o = [0u8; 16];
            o.copy_from_slice(&a[..16]);
            Ipv6Addr::from(o).to_string()
        }
        _ => "?".into(),
    }
}

fn ifindex_name(i: u32) -> Vec<u8> {
    let mut b = [0 as libc::c_char; libc::IF_NAMESIZE];
    // SAFETY: `b` has the room if_indextoname needs.
    let p = unsafe { libc::if_indextoname(i, b.as_mut_ptr()) };
    if p.is_null() {
        return Vec::new();
    }
    // SAFETY: a NUL-terminated name was written into `b`.
    unsafe { CStr::from_ptr(b.as_ptr()) }.to_bytes().to_vec()
}

fn dump_links(s: &Socket) -> Result<Vec<Link>, i32> {
    let mut links = Vec::new();
    let mut body = [0u8; 16]; // ifinfomsg, family AF_UNSPEC
    body[0] = 0;
    s.dump(RTM_GETLINK, &body, |m| {
        if m.ty != RTM_NEWLINK || m.payload.len() < 16 {
            return;
        }
        let p = m.payload;
        let tb = attrs(p, 16, IFLA_MAX);
        let Some(name) = tb[IFLA_IFNAME] else { return };
        let mut k = Link {
            arphrd: u16::from_ne_bytes([p[2], p[3]]),
            index: i32::from_ne_bytes([p[4], p[5], p[6], p[7]]),
            flags: u32::from_ne_bytes([p[8], p[9], p[10], p[11]]),
            name: cstr(name)[..cstr(name).len().min(15)].to_vec(),
            oper: tb[IFLA_OPERSTATE].and_then(|b| b.first()).map_or("unknown", |&o| oper_name(o)),
            ..Link::default()
        };
        k.mtu = u32_of(tb[IFLA_MTU]).unwrap_or(0) as i32;
        k.master = u32_of(tb[IFLA_MASTER]).unwrap_or(0) as i32;
        if let Some(a) = tb[IFLA_ADDRESS].filter(|a| a.len() == 6) {
            k.mac = format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", a[0], a[1], a[2], a[3], a[4], a[5]);
        }
        if let Some(st) = tb[IFLA_STATS64].filter(|st| st.len() >= 48) {
            let g = |i: usize| u64::from_ne_bytes(st[i * 8..i * 8 + 8].try_into().unwrap());
            (k.rx_packets, k.tx_packets, k.rx_bytes, k.tx_bytes, k.rx_errors, k.tx_errors) = (g(0), g(1), g(2), g(3), g(4), g(5));
        }
        if let Some(li) = tb[IFLA_LINKINFO] {
            if let Some(kind) = nested(li, 2)[IFLA_INFO_KIND] {
                k.kind = cstr(kind)[..cstr(kind).len().min(23)].to_vec();
            }
        }
        links.push(k);
    })?;
    Ok(links)
}

/// The addresses of the machine by interface index, in the kernel's order, as JSON objects.
fn dump_addrs(s: &Socket) -> HashMap<u32, Vec<String>> {
    let mut by: HashMap<u32, Vec<String>> = HashMap::new();
    let body = [0u8; 8]; // ifaddrmsg, AF_UNSPEC
    let _ = s.dump(RTM_GETADDR, &body, |m| {
        if m.ty != RTM_NEWADDR || m.payload.len() < 8 {
            return;
        }
        let p = m.payload;
        let (family, prefix, ifa_flags, scope) = (p[0], p[1], p[2], p[3]);
        let index = u32::from_ne_bytes([p[4], p[5], p[6], p[7]]);
        let tb = attrs(p, 8, IFA_MAX);
        let Some(a) = tb[IFA_LOCAL].or(tb[IFA_ADDRESS]) else { return };
        let flags = u32_of(tb[IFA_FLAGS]).unwrap_or(ifa_flags as u32);
        let scope = match scope {
            0 => "global",
            253 => "link",
            254 => "host",
            _ => "site",
        };
        // struct ifa_cacheinfo { prefered, valid, cstamp, tstamp }
        let valid: i64 = tb[IFA_CACHEINFO].filter(|c| c.len() >= 8).map_or(-1, |c| {
            let v = u32::from_ne_bytes([c[4], c[5], c[6], c[7]]);
            if v == 0xFFFF_FFFF { -1 } else { v as i64 }
        });
        let mut o = Out::default();
        o.raw(&format!(
            "{{\"family\":{},\"address\":\"{}\",\"prefix\":{},\"scope\":\"{}\",\"dynamic\":{},\"validLifetime\":{}}}",
            if family == AF_INET { 4 } else { 6 },
            fmt_ip(family, a),
            prefix,
            scope,
            flags & IFA_F_PERMANENT == 0,
            valid
        ));
        by.entry(index).or_default().push(String::from_utf8(o.0).unwrap());
    });
    by
}

fn link_type(p: &Paths, k: &Link) -> String {
    let exists = |what: &str| {
        let path = format!("{}/{}/{what}", p.sys_class_net, String::from_utf8_lossy(&k.name));
        std::ffi::CString::new(path).is_ok_and(|c| {
            // SAFETY: a valid C string.
            unsafe { libc::access(c.as_ptr(), libc::F_OK) == 0 }
        })
    };
    if k.arphrd == ARPHRD_LOOPBACK {
        return "loopback".into();
    }
    match k.kind.as_slice() {
        b"wireguard" => return "wireguard".into(),
        b"tun" => return "tun".into(),
        b"bridge" => return "bridge".into(),
        [] => {}
        kind => return String::from_utf8_lossy(kind).into_owned(), // veth, vlan, bond, dummy …
    }
    if exists("wireless") || exists("phy80211") {
        return "wifi".into();
    }
    match k.arphrd {
        ARPHRD_ETHER => "ethernet",
        ARPHRD_NONE => "tun",
        _ => "other",
    }
    .into()
}

/// Every interface with its counters, addresses and saved configuration. `None` if the kernel
/// would not list the links.
pub fn interfaces_json(p: &Paths) -> Option<Vec<u8>> {
    let sock = Socket::open(NETLINK_ROUTE, 0).ok()?;
    let links = dump_links(&sock).ok()?;
    let addrs = Socket::open(NETLINK_ROUTE, 0).ok().map(|s| dump_addrs(&s)).unwrap_or_default();
    let mut o = Out::default();
    o.raw("[");
    for (i, k) in links.iter().enumerate() {
        let master = if k.master != 0 { ifindex_name(k.master as u32) } else { Vec::new() };
        if i > 0 {
            o.raw(",");
        }
        o.raw("{\"name\":");
        o.string(&k.name);
        o.raw(&format!(",\"index\":{},\"type\":\"{}\",\"kind\":", k.index, link_type(p, k)));
        o.string(&k.kind);
        o.raw(&format!(
            ",\"up\":{},\"carrier\":{},\"operstate\":\"{}\",\"mac\":\"{}\",\"mtu\":{},\"master\":",
            k.flags & IFF_UP != 0,
            k.flags & IFF_LOWER_UP != 0,
            k.oper,
            k.mac,
            k.mtu
        ));
        o.string(&master);
        // the host distribution manages every interface
        o.raw(&format!(
            ",\"managed\":\"system\",\"rxBytes\":{},\"txBytes\":{},\"rxPackets\":{},\"txPackets\":{},\"rxErrors\":{},\"txErrors\":{},\"addresses\":[",
            k.rx_bytes, k.tx_bytes, k.rx_packets, k.tx_packets, k.rx_errors, k.tx_errors
        ));
        if let Some(list) = u32::try_from(k.index).ok().and_then(|ix| addrs.get(&ix)) {
            o.raw(&list.join(","));
        }
        o.raw("],\"config\":");
        match if_config_json(p, &k.name) {
            Some(c) => o.0.extend_from_slice(&c),
            None => o.raw("null"),
        }
        o.raw("}");
    }
    o.raw("]");
    Some(o.0)
}

/// The unicast routes (the local table and IPv6 link-local prefixes are left out). `None` if no
/// netlink socket could be opened.
pub fn routes_json() -> Option<Vec<u8>> {
    let sock = Socket::open(NETLINK_ROUTE, 0).ok()?;
    let mut o = Out::default();
    o.raw("[");
    let mut first = true;
    for family in [AF_INET, AF_INET6] {
        let mut body = [0u8; 12]; // rtmsg
        body[0] = family;
        let _ = sock.dump(RTM_GETROUTE, &body, |m| {
            if m.ty != RTM_NEWROUTE || m.payload.len() < 12 {
                return;
            }
            let p = m.payload;
            let (fam, dst_len, table8, proto, rtype) = (p[0], p[1], p[4], p[5], p[7]);
            let tb = attrs(p, 12, RTA_MAX);
            let table = u32_of(tb[RTA_TABLE]).unwrap_or(table8 as u32);
            if rtype != RTN_UNICAST || table == RT_TABLE_LOCAL {
                return;
            }
            if fam == AF_INET6 && dst_len == 64 && tb[RTA_DST].is_some_and(|d| d.first() == Some(&0xfe)) {
                return;
            }
            let dst = match tb[RTA_DST] {
                Some(d) if dst_len != 0 => format!("{}/{}", fmt_ip(fam, d), dst_len),
                _ => "default".to_string(),
            };
            let gw = tb[RTA_GATEWAY].map(|g| fmt_ip(fam, g)).unwrap_or_default();
            let dev = u32_of(tb[RTA_OIF]).map(ifindex_name).unwrap_or_default();
            let metric = u32_of(tb[RTA_PRIORITY]).unwrap_or(0);
            let proto = match proto {
                2 => "kernel".to_string(),
                3 => "boot".to_string(),
                4 => "static".to_string(),
                16 => "dhcp".to_string(),
                9 => "ra".to_string(),
                n => n.to_string(),
            };
            if !first {
                o.raw(",");
            }
            first = false;
            o.raw(&format!("{{\"family\":{},\"dst\":\"{}\",\"gateway\":\"{}\",\"dev\":\"", if fam == AF_INET { 4 } else { 6 }, dst, gw));
            o.0.extend_from_slice(&dev);
            o.raw(&format!("\",\"metric\":{metric},\"table\":{table},\"protocol\":\"{proto}\"}}"));
        });
    }
    o.raw("]");
    Some(o.0)
}

/// Opens the notification socket (links, addresses and routes of both families), non-blocking.
pub fn open_monitor() -> std::io::Result<Socket> {
    let s = Socket::open(NETLINK_ROUTE, RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR | RTMGRP_IPV4_ROUTE | RTMGRP_IPV6_ROUTE)?;
    // SAFETY: fcntl on our descriptor.
    unsafe {
        let fl = libc::fcntl(s.0.as_raw_fd(), libc::F_GETFL);
        libc::fcntl(s.0.as_raw_fd(), libc::F_SETFL, fl | libc::O_NONBLOCK);
    }
    Ok(s)
}

/// Reads every pending notification and calls `f` once for each of "link", "addr" and "route"
/// that changed (all of them if notifications were lost).
pub fn dispatch(fd: RawFd, mut f: impl FnMut(&str)) {
    let mut buf = vec![0u8; 16384];
    let (mut link, mut addr, mut route) = (false, false, false);
    loop {
        // SAFETY: `buf` is ours.
        let r = unsafe { libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0) };
        if r < 0 {
            let e = std::io::Error::last_os_error();
            match e.raw_os_error() {
                Some(libc::EINTR) => continue,
                Some(libc::ENOBUFS) => {
                    (link, addr, route) = (true, true, true); // notifications were lost
                    continue;
                }
                _ => break,
            }
        }
        if r == 0 {
            break;
        }
        for m in Messages(&buf[..r as usize]) {
            match m.ty {
                RTM_NEWLINK | RTM_DELLINK => link = true,
                RTM_NEWADDR | RTM_DELADDR => addr = true,
                RTM_NEWROUTE | RTM_DELROUTE => route = true,
                _ => {}
            }
        }
    }
    if link {
        f("link");
    }
    if addr {
        f("addr");
    }
    if route {
        f("route");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_type_of_an_interface() {
        let dir = std::env::temp_dir().join(format!("axia-netstate-type-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("wlan0/wireless")).unwrap();
        std::fs::create_dir_all(dir.join("wlan1/phy80211")).unwrap();
        std::fs::create_dir_all(dir.join("eth0")).unwrap();
        let p = Paths { sys_class_net: dir.to_str().unwrap().into(), ..Paths::default() };
        let t = |name: &str, arphrd: u16, kind: &str| link_type(&p, &Link { name: name.into(), arphrd, kind: kind.into(), ..Link::default() });
        assert_eq!(t("lo", ARPHRD_LOOPBACK, "dummy"), "loopback");
        assert_eq!(t("wg0", 65534, "wireguard"), "wireguard");
        for (name, arphrd, kind, want) in [
            ("tun0", 65534, "tun", "tun"),
            ("br0", 1, "bridge", "bridge"),
            ("veth1", 1, "veth", "veth"),
            ("bond0", 1, "bond", "bond"),
            ("wlan0", ARPHRD_ETHER, "", "wifi"),
            ("wlan1", ARPHRD_ETHER, "", "wifi"),
            ("eth0", ARPHRD_ETHER, "", "ethernet"),
            ("ppp0", ARPHRD_NONE, "", "tun"),
            ("sit0", 776, "", "other"),
        ] {
            assert_eq!(t(name, arphrd, kind), want, "{name}");
        }
        assert_eq!(oper_name(6), "up");
        assert_eq!(oper_name(2), "down");
        assert_eq!(oper_name(77), "unknown");
        assert_eq!(fmt_ip(AF_INET, &[10, 1, 2, 3]), "10.1.2.3");
        assert_eq!(fmt_ip(AF_INET6, &[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]), "fe80::1");
        assert_eq!(fmt_ip(AF_INET, &[1, 2]), "?");
    }
}
