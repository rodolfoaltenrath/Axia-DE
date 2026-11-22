//! The rtnetlink side on a real kernel. The machine's own `lo` is always there; the rest runs in a
//! private network namespace (a user namespace gives the test the right to build interfaces in it):
//! the test starts itself again inside one. Where namespaces are not allowed, those parts say so and
//! stop.
use axia_netstate::config::Paths;
use axia_netstate::state::{dispatch, interfaces_json, open_monitor, routes_json};
use serde_json::Value;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::Command;

// ---- raw rtnetlink, only for building the test's interfaces ----

struct Nl(axia_netstate::netlink::Socket);

fn attr(ty: u16, data: &[u8]) -> Vec<u8> {
    let mut v = ((4 + data.len()) as u16).to_ne_bytes().to_vec();
    v.extend_from_slice(&ty.to_ne_bytes());
    v.extend_from_slice(data);
    while v.len() % 4 != 0 {
        v.push(0);
    }
    v
}

fn nest(ty: u16, inner: Vec<u8>) -> Vec<u8> {
    attr(ty | 0x8000, &inner)
}

impl Nl {
    fn new() -> Nl {
        Nl(axia_netstate::netlink::Socket::open(0, 0).unwrap())
    }

    /// Sends a request with an ACK and returns the kernel's answer (0 = done).
    fn talk(&self, ty: u16, flags: u16, body: Vec<u8>) -> i32 {
        let len = 16 + body.len();
        let mut m = (len as u32).to_ne_bytes().to_vec();
        m.extend_from_slice(&ty.to_ne_bytes());
        m.extend_from_slice(&(1 | 4 | flags).to_ne_bytes()); // REQUEST | ACK
        m.extend_from_slice(&77u32.to_ne_bytes());
        m.extend_from_slice(&0u32.to_ne_bytes());
        m.extend_from_slice(&body);
        let fd = self.0 .0.as_raw_fd();
        unsafe {
            let mut sa: libc::sockaddr_nl = std::mem::zeroed();
            sa.nl_family = libc::AF_NETLINK as u16;
            assert_eq!(libc::sendto(fd, m.as_ptr().cast(), m.len(), 0, (&sa as *const libc::sockaddr_nl).cast(), size_of::<libc::sockaddr_nl>() as u32), m.len() as isize);
            let mut buf = vec![0u8; 8192];
            let r = libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0);
            assert!(r >= 20);
            i32::from_ne_bytes([buf[16], buf[17], buf[18], buf[19]])
        }
    }

    fn ifinfo(index: i32, flags: u32, change: u32) -> Vec<u8> {
        let mut v = vec![0u8, 0, 0, 0];
        v.extend_from_slice(&index.to_ne_bytes());
        v.extend_from_slice(&flags.to_ne_bytes());
        v.extend_from_slice(&change.to_ne_bytes());
        v
    }

    fn new_link(&self, name: &str, kind: &str, extra: Vec<u8>) {
        let mut b = Nl::ifinfo(0, 0, 0);
        b.extend(attr(3, format!("{name}\0").as_bytes()));
        b.extend(extra);
        b.extend(nest(18, attr(1, kind.as_bytes())));
        assert_eq!(self.talk(16, 0x400 | 0x200, b), 0, "create {name}"); // CREATE | EXCL
    }

    fn up(&self, name: &str) {
        let c = std::ffi::CString::new(name).unwrap();
        let ix = unsafe { libc::if_nametoindex(c.as_ptr()) } as i32;
        assert_eq!(self.talk(16, 0, Nl::ifinfo(ix, 1, 1)), 0, "up {name}");
    }

    fn addr(&self, name: &str, family: u8, a: &[u8], prefix: u8) {
        let c = std::ffi::CString::new(name).unwrap();
        let ix = unsafe { libc::if_nametoindex(c.as_ptr()) };
        let mut b = vec![family, prefix, 0, 0];
        b.extend_from_slice(&ix.to_ne_bytes());
        b.extend(attr(2, a)); // IFA_LOCAL
        b.extend(attr(1, a)); // IFA_ADDRESS
        assert_eq!(self.talk(20, 0x400 | 0x100, b), 0, "addr {name}");
    }

    fn route(&self, family: u8, dst: Option<(&[u8], u8)>, gw: Option<&[u8]>, dev: &str, metric: u32, table: u8) {
        let c = std::ffi::CString::new(dev).unwrap();
        let ix = unsafe { libc::if_nametoindex(c.as_ptr()) };
        // rtmsg: family, dst_len, src_len, tos, table, protocol (static), scope, type (unicast), flags
        let scope = if gw.is_some() { 0 } else { 253 };
        let mut b = vec![family, dst.map_or(0, |d| d.1), 0, 0, table, 4, scope, 1, 0, 0, 0, 0];
        if let Some((d, _)) = dst {
            b.extend(attr(1, d));
        }
        if let Some(g) = gw {
            b.extend(attr(5, g));
        }
        b.extend(attr(4, &ix.to_ne_bytes()));
        if metric != 0 {
            b.extend(attr(6, &metric.to_ne_bytes()));
        }
        assert_eq!(self.talk(24, 0x400 | 0x100, b), 0, "route {dev}");
    }
}

/// Runs `body` in a private network namespace: the test binary starts itself again after
/// unshare(NEWUSER|NEWNET). `false` if the machine does not allow that (the test then stops).
fn in_namespace(test: &str) -> bool {
    if std::env::var_os("AXIA_NETSTATE_IN_NS").is_some() {
        return true;
    }
    let me = std::env::current_exe().unwrap();
    let mut c = Command::new(me);
    c.args(["--exact", test, "--nocapture", "--test-threads=1"]).env("AXIA_NETSTATE_IN_NS", "1");
    // root inside the namespace (mapped to this user), or exec would drop the capabilities
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let (uid_map, gid_map) = (format!("0 {uid} 1\n"), format!("0 {gid} 1\n"));
    fn put(path: &[u8], data: &[u8]) -> std::io::Result<()> {
        // SAFETY: open and write are async-signal-safe; `path` ends in a NUL.
        unsafe {
            let fd = libc::open(path.as_ptr().cast(), libc::O_WRONLY);
            if fd < 0 || libc::write(fd, data.as_ptr().cast(), data.len()) != data.len() as isize {
                return Err(std::io::Error::last_os_error());
            }
            libc::close(fd);
        }
        Ok(())
    }
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        c.pre_exec(move || {
            if libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            put(b"/proc/self/setgroups\0", b"deny")?;
            put(b"/proc/self/uid_map\0", uid_map.as_bytes())?;
            put(b"/proc/self/gid_map\0", gid_map.as_bytes())
        });
    }
    match c.output() {
        Err(e) => {
            eprintln!("{test}: no private network namespace here ({e}); skipped");
            false
        }
        Ok(o) => {
            assert!(o.status.success(), "{test} failed in its namespace:\n{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            false
        }
    }
}

fn parse(v: Vec<u8>) -> Value {
    serde_json::from_slice(&v).unwrap_or_else(|e| panic!("{}: {e}", String::from_utf8_lossy(&v)))
}

fn by_name<'a>(v: &'a Value, name: &str) -> &'a Value {
    v.as_array().unwrap().iter().find(|i| i["name"] == name).unwrap_or_else(|| panic!("no {name} in {v}"))
}

#[test]
fn the_loopback_of_this_machine_is_listed() {
    let j = parse(interfaces_json(&Paths::default()).expect("the kernel lists the links"));
    let lo = by_name(&j, "lo");
    assert_eq!((lo["type"].as_str(), lo["kind"].as_str(), lo["up"].as_bool(), lo["operstate"].as_str()), (Some("loopback"), Some(""), Some(true), Some("unknown")));
    assert_eq!((lo["managed"].as_str(), lo["master"].as_str(), lo["mac"].as_str()), (Some("system"), Some(""), Some("00:00:00:00:00:00")));
    let mtu: i64 = std::fs::read_to_string("/sys/class/net/lo/mtu").unwrap().trim().parse().unwrap();
    assert_eq!(lo["mtu"].as_i64(), Some(mtu));
    let a = &lo["addresses"].as_array().unwrap().iter().find(|a| a["family"] == 4).expect("127.0.0.1")["address"];
    assert_eq!(a, "127.0.0.1");
    let a4 = lo["addresses"].as_array().unwrap().iter().find(|a| a["family"] == 4).unwrap();
    assert_eq!((a4["prefix"].as_i64(), a4["scope"].as_str(), a4["dynamic"].as_bool(), a4["validLifetime"].as_i64()), (Some(8), Some("host"), Some(false), Some(-1)));
    // every interface of /sys/class/net is in the list, in the order of their indexes
    let mut names: Vec<String> = std::fs::read_dir("/sys/class/net").unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
    names.sort();
    let mut listed: Vec<String> = j.as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap().to_string()).collect();
    listed.sort();
    assert_eq!(listed, names);
    let idx: Vec<i64> = j.as_array().unwrap().iter().map(|i| i["index"].as_i64().unwrap()).collect();
    assert!(idx.windows(2).all(|w| w[0] < w[1]), "{idx:?}");
    // routes: an array of the documented shape
    for r in parse(routes_json().unwrap()).as_array().unwrap() {
        assert!(matches!(r["family"].as_i64(), Some(4 | 6)) && r["dst"].is_string() && r["gateway"].is_string() && r["dev"].is_string());
        assert!(r["metric"].is_u64() && r["table"].is_u64() && r["protocol"].is_string());
        assert_ne!(r["table"], 255, "the local table is left out");
    }
}

#[test]
fn interfaces_addresses_and_routes_in_a_private_namespace() {
    if !in_namespace("interfaces_addresses_and_routes_in_a_private_namespace") {
        return;
    }
    let nl = Nl::new();
    // lo is down in a fresh namespace: bring it up with its address, then build the rest
    nl.up("lo");
    nl.addr("lo", 2, &[127, 0, 0, 1], 8);
    nl.new_link("br0", "bridge", vec![]);
    nl.new_link("dummy0", "dummy", attr(4, &1400u32.to_ne_bytes()));
    nl.new_link("dummy1", "dummy", vec![]);
    // dummy0 joins the bridge
    {
        let c = std::ffi::CString::new("br0").unwrap();
        let br = unsafe { libc::if_nametoindex(c.as_ptr()) };
        let c = std::ffi::CString::new("dummy0").unwrap();
        let ix = unsafe { libc::if_nametoindex(c.as_ptr()) } as i32;
        assert_eq!(nl.talk(16, 0, { let mut b = Nl::ifinfo(ix, 0, 0); b.extend(attr(10, &br.to_ne_bytes())); b }), 0);
    }
    for n in ["br0", "dummy0", "dummy1"] {
        nl.up(n);
    }
    nl.addr("dummy1", 2, &[192, 0, 2, 5], 24);
    nl.addr("dummy1", 10, &[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5], 64);
    nl.route(2, Some((&[198, 51, 100, 0], 24)), Some(&[192, 0, 2, 1]), "dummy1", 50, 254);
    nl.route(2, None, Some(&[192, 0, 2, 1]), "dummy1", 0, 254);
    nl.route(2, Some((&[203, 0, 113, 0], 24)), None, "dummy1", 0, 100); // another table

    let dir = std::env::temp_dir().join(format!("axia-netstate-ns-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("if")).unwrap();
    std::fs::write(dir.join("if/dummy1.conf"), "method=static\naddress=192.0.2.5/24\n").unwrap();
    std::fs::create_dir_all(dir.join("sys/dummy1")).unwrap();
    let p = Paths { if_dir: dir.join("if").to_str().unwrap().into(), sys_class_net: dir.join("sys").to_str().unwrap().into(), ..Paths::default() };
    let j = parse(interfaces_json(&p).unwrap());
    let names: Vec<&str> = j.as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["lo", "br0", "dummy0", "dummy1"], "in the order of their indexes");
    let lo = by_name(&j, "lo");
    assert_eq!((lo["type"].as_str(), lo["up"].as_bool(), lo["addresses"][0]["address"].as_str()), (Some("loopback"), Some(true), Some("127.0.0.1")));
    let br = by_name(&j, "br0");
    assert_eq!((br["type"].as_str(), br["kind"].as_str()), (Some("bridge"), Some("bridge")));
    let d0 = by_name(&j, "dummy0");
    assert_eq!((d0["type"].as_str(), d0["kind"].as_str(), d0["mtu"].as_i64(), d0["master"].as_str()), (Some("dummy"), Some("dummy"), Some(1400), Some("br0")));
    let d1 = by_name(&j, "dummy1");
    assert_eq!((d1["up"].as_bool(), d1["mtu"].as_i64(), d1["master"].as_str(), d1["managed"].as_str()), (Some(true), Some(1500), Some(""), Some("system")));
    assert_eq!(d1["mac"].as_str().map(str::len), Some(17));
    assert_eq!(d1["config"], serde_json::json!({ "method": "static", "addresses": ["192.0.2.5/24"], "gateway": "", "dns": [] }));
    assert!(by_name(&j, "br0")["config"].is_null());
    let a = d1["addresses"].as_array().unwrap();
    let v4 = a.iter().find(|x| x["family"] == 4).unwrap();
    assert_eq!(*v4, serde_json::json!({ "family": 4, "address": "192.0.2.5", "prefix": 24, "scope": "global", "dynamic": false, "validLifetime": -1 }));
    let v6 = a.iter().find(|x| x["family"] == 6 && x["address"] == "2001:db8::5").expect("the global IPv6 address");
    assert_eq!((v6["prefix"].as_i64(), v6["scope"].as_str()), (Some(64), Some("global")));
    assert!(d1["rxBytes"].is_u64() && d1["txPackets"].is_u64() && d1["rxErrors"].is_u64());
    // an interface that has a wireless folder in sysfs is Wi-Fi
    let r = parse(routes_json().unwrap());
    let r = r.as_array().unwrap();
    let find = |dst: &str| r.iter().find(|x| x["dst"] == dst).unwrap_or_else(|| panic!("no {dst} in {r:?}"));
    assert_eq!(*find("198.51.100.0/24"), serde_json::json!({ "family": 4, "dst": "198.51.100.0/24", "gateway": "192.0.2.1", "dev": "dummy1", "metric": 50, "table": 254, "protocol": "static" }));
    assert_eq!((find("default")["gateway"].as_str(), find("default")["metric"].as_u64()), (Some("192.0.2.1"), Some(0)));
    assert_eq!(*find("203.0.113.0/24"), serde_json::json!({ "family": 4, "dst": "203.0.113.0/24", "gateway": "", "dev": "dummy1", "metric": 0, "table": 100, "protocol": "static" }));
    assert!(r.iter().any(|x| x["dst"] == "192.0.2.0/24" && x["protocol"] == "kernel"), "the connected route the kernel made");
    assert!(r.iter().all(|x| !x["dst"].as_str().unwrap().starts_with("fe80")), "link-local prefixes are left out");
    assert!(r.iter().all(|x| x["table"] != 255), "the local table is left out");
    // Wi-Fi: only what sysfs says, for an interface with no kind
    std::fs::create_dir_all(dir.join("sys/lo/wireless")).unwrap();
    let j = parse(interfaces_json(&p).unwrap());
    assert_eq!(by_name(&j, "lo")["type"], "loopback", "loopback is decided first");
}

#[test]
fn notifications_say_what_changed() {
    if !in_namespace("notifications_say_what_changed") {
        return;
    }
    let nl = Nl::new();
    let mon = open_monitor().unwrap();
    let fd = mon.0.as_raw_fd();
    let seen = |what: &mut Vec<String>| dispatch(fd, |w| what.push(w.to_string()));
    let mut w = Vec::new();
    seen(&mut w);
    assert!(w.is_empty(), "nothing yet");
    nl.new_link("dummy9", "dummy", vec![]);
    seen(&mut w);
    assert_eq!(w, ["link"]);
    w.clear();
    nl.up("dummy9");
    nl.addr("dummy9", 2, &[192, 0, 2, 9], 24);
    nl.route(2, Some((&[198, 51, 100, 0], 24)), Some(&[192, 0, 2, 1]), "dummy9", 7, 254);
    seen(&mut w);
    assert_eq!(w, ["link", "addr", "route"], "each kind once, in that order");
    w.clear();
    seen(&mut w);
    assert!(w.is_empty(), "consumed");
}
