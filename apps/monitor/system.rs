//! Host facts for the dashboard: name, distribution, model, filesystems, local
//! addresses and battery. Collected on the worker thread every few seconds.
use super::{entries, read};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CString,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    os::unix::ffi::OsStrExt,
    path::Path,
};
pub fn collect(root: &Path) -> Value {
    json!({
        "host": read(&root.join("proc/sys/kernel/hostname"), 256).trim(),
        "os": os_name(root),
        "model": model(root),
        "filesystems": filesystems(root),
        "network": network(root),
        "battery": battery(root),
    })
}
pub fn os_name(root: &Path) -> String {
    for file in ["etc/os-release", "usr/lib/os-release"] {
        let text = read(&root.join(file), 16384);
        let field = |key: &str| {
            text.lines().find_map(|l| {
                l.strip_prefix(key)
                    .and_then(|v| v.strip_prefix('='))
                    .map(|v| v.trim().trim_matches('"').to_owned())
            })
        };
        if let Some(name) = field("PRETTY_NAME").or_else(|| field("NAME")) {
            if !name.is_empty() {
                return name;
            }
        }
    }
    String::new()
}
pub fn model(root: &Path) -> String {
    // Placeholders some firmware leaves in the DMI table.
    const UNSET: [&str; 4] = [
        "to be filled by o.e.m.",
        "system product name",
        "default string",
        "none",
    ];
    let dmi = |f: &str| {
        read(&root.join("sys/class/dmi/id").join(f), 256)
            .trim()
            .to_owned()
    };
    let usable = |s: &String| !s.is_empty() && !UNSET.contains(&s.to_lowercase().as_str());
    let product = dmi("product_name");
    if usable(&product) {
        let family = dmi("product_version");
        // Lenovo keeps the marketing name in product_version.
        if usable(&family) && family.chars().any(char::is_alphabetic) && family.len() > 3 {
            return family;
        }
        return product;
    }
    let board = dmi("board_name");
    if usable(&board) {
        return board;
    }
    read(&root.join("sys/firmware/devicetree/base/model"), 256)
        .trim_end_matches('\0')
        .trim()
        .to_owned()
}
/// `/proc/mounts` escapes spaces and tabs as octal sequences.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            if let Ok(n) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(n);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
fn usage(path: &Path) -> Option<(u64, u64, u64)> {
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let unit = s.f_frsize as u64;
    let total = (s.f_blocks as u64).saturating_mul(unit);
    let free = (s.f_bfree as u64).saturating_mul(unit);
    let avail = (s.f_bavail as u64).saturating_mul(unit);
    (total > 0).then_some((total, total.saturating_sub(free), avail))
}
/// Mounted block-device filesystems, the root first, one entry per device.
pub fn filesystems(root: &Path) -> Value {
    let mounts = read(&root.join("proc/mounts"), 1 << 20);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for line in mounts.lines() {
        let f: Vec<_> = line.split_whitespace().collect();
        let [dev, mount, fs, ..] = f[..] else {
            continue;
        };
        let mount = unescape(mount);
        if !dev.starts_with("/dev/")
            || matches!(fs, "squashfs" | "iso9660" | "udf" | "erofs")
            || mount.starts_with("/boot")
            || mount.starts_with("/snap")
            || mount.starts_with("/var/lib/")
            || mount.starts_with("/sysroot")
            || !seen.insert(dev.to_owned())
        {
            continue;
        }
        let Some((total, used, avail)) = usage(&root.join(mount.trim_start_matches('/'))) else {
            continue;
        };
        out.push(
            json!({"mount":mount,"device":dev,"fs":fs,"total":total,"used":used,"avail":avail}),
        );
        if out.len() >= 8 {
            break;
        }
    }
    out.sort_by_key(|v| v["mount"] != "/");
    json!(out)
}
fn route(root: &Path) -> Option<(String, Ipv4Addr)> {
    read(&root.join("proc/net/route"), 65536)
        .lines()
        .skip(1)
        .find_map(|l| {
            let f: Vec<_> = l.split_whitespace().collect();
            if f.len() < 3 || f[1] != "00000000" {
                return None;
            }
            // The kernel prints the network-order word as a native integer.
            let gw = u32::from_str_radix(f[2], 16).ok()?;
            Some((f[0].to_owned(), Ipv4Addr::from(gw.to_ne_bytes())))
        })
}
fn neighbour_mac(root: &Path, ip: Ipv4Addr) -> Option<String> {
    let ip = ip.to_string();
    read(&root.join("proc/net/arp"), 65536)
        .lines()
        .skip(1)
        .find_map(|l| {
            let f: Vec<_> = l.split_whitespace().collect();
            (f.len() > 3 && f[0] == ip && f[3] != "00:00:00:00:00:00").then(|| f[3].to_owned())
        })
}
/// Interface addresses from the kernel; only meaningful on the real root.
fn addresses(root: &Path) -> BTreeMap<String, Vec<IpAddr>> {
    let mut out: BTreeMap<String, Vec<IpAddr>> = BTreeMap::new();
    if root != Path::new("/") {
        return out;
    }
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            return out;
        }
        let mut cur = list;
        while let Some(ifa) = cur.as_ref() {
            cur = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || ifa.ifa_name.is_null() {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(ifa.ifa_name)
                .to_string_lossy()
                .into_owned();
            let ip = match (*ifa.ifa_addr).sa_family as i32 {
                libc::AF_INET => {
                    let a = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    IpAddr::V4(Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr)))
                }
                libc::AF_INET6 => {
                    let a = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                    IpAddr::V6(Ipv6Addr::from(a.sin6_addr.s6_addr))
                }
                _ => continue,
            };
            let entry = out.entry(name).or_default();
            if entry.len() < 8 {
                entry.push(ip);
            }
        }
        libc::freeifaddrs(list);
    }
    for list in out.values_mut() {
        // IPv4 first, then global IPv6 before link-local.
        list.sort_by_key(|ip| match ip {
            IpAddr::V4(_) => 0,
            IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80 => 2,
            IpAddr::V6(_) => 1,
        });
    }
    out
}
pub fn network(root: &Path) -> Value {
    let default = route(root);
    let mut addrs = addresses(root);
    let mut ifs = Vec::new();
    for path in entries(&root.join("sys/class/net")) {
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        let primary = default.as_ref().is_some_and(|d| d.0 == name);
        let virtual_if = root.join("sys/devices/virtual/net").join(&name).exists();
        if name == "lo" || (virtual_if && !primary) {
            continue;
        }
        let wireless = path.join("wireless").exists() || path.join("phy80211").exists();
        let up = read(&path.join("operstate"), 32).trim() == "up";
        let list: Vec<_> = addrs
            .remove(&name)
            .unwrap_or_default()
            .into_iter()
            .map(|ip| ip.to_string())
            .collect();
        ifs.push(json!({"name":name,"mac":read(&path.join("address"),64).trim(),"wireless":wireless,"up":up,"primary":primary,"addrs":list}));
        if ifs.len() >= 16 {
            break;
        }
    }
    // Primary first, then connected ones.
    ifs.sort_by_key(|i| (i["primary"] != true, i["up"] != true));
    let gateway = default.as_ref().map(|d| d.1);
    json!({
        "gateway": gateway.map(|g| g.to_string()),
        "gatewayMac": gateway.and_then(|g| neighbour_mac(root, g)),
        "ifs": ifs,
    })
}
fn micro(path: &Path, key: &str) -> Option<f64> {
    read(&path.join(key), 64)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.)
}
/// The first system battery (peripheral batteries report scope "Device").
pub fn battery(root: &Path) -> Value {
    for path in entries(&root.join("sys/class/power_supply")) {
        if read(&path.join("type"), 64).trim() != "Battery"
            || read(&path.join("scope"), 64).trim() == "Device"
        {
            continue;
        }
        // charge_* is in µAh; energy_* in µWh. Report mAh or Wh as given.
        let (full, design, now, unit) = if path.join("charge_full").exists() {
            (
                micro(&path, "charge_full"),
                micro(&path, "charge_full_design"),
                micro(&path, "charge_now"),
                "mAh",
            )
        } else {
            (
                micro(&path, "energy_full"),
                micro(&path, "energy_full_design"),
                micro(&path, "energy_now"),
                "Wh",
            )
        };
        let scale = if unit == "mAh" { 1e-3 } else { 1e-6 };
        let capacity = micro(&path, "capacity").or_else(|| match (now, full) {
            (Some(n), Some(f)) if f > 0. => Some((n / f * 100.).round()),
            _ => None,
        });
        return json!({
            "name": path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            "capacity": capacity.map(|c| c.min(100.)),
            "status": read(&path.join("status"), 64).trim(),
            "cycles": micro(&path, "cycle_count").filter(|c| *c > 0.),
            "full": full.map(|v| v * scale),
            "design": design.map(|v| v * scale),
            "unit": unit,
        });
    }
    Value::Null
}
