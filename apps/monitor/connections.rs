//! Linux socket tables and owning processes, collected off the UI thread.
use super::{entries, geo::Database, read};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
};
fn endpoint(s: &str) -> Option<(IpAddr, u16)> {
    let (addr, port) = s.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    if addr.len() == 8 {
        let n = u32::from_str_radix(addr, 16).ok()?;
        Some((Ipv4Addr::from(n.to_le_bytes()).into(), port))
    } else if addr.len() == 32 {
        let mut bytes = [0; 16];
        for (i, c) in addr.as_bytes().chunks_exact(8).enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(
                &u32::from_str_radix(std::str::from_utf8(c).ok()?, 16)
                    .ok()?
                    .to_le_bytes(),
            );
        }
        Some((Ipv6Addr::from(bytes).into(), port))
    } else {
        None
    }
}
pub fn scope(addr: IpAddr) -> &'static str {
    match addr {
        IpAddr::V4(a) => {
            if a.is_loopback() || a.is_unspecified() {
                "local"
            } else if a.is_private() || a.is_link_local() {
                "lan"
            } else {
                "internet"
            }
        }
        IpAddr::V6(a) => {
            if a.is_loopback() || a.is_unspecified() {
                "local"
            } else if a.segments()[0] & 0xfe00 == 0xfc00 || a.segments()[0] & 0xffc0 == 0xfe80 {
                "lan"
            } else {
                "internet"
            }
        }
    }
}
pub fn table(text: &str, proto: &str) -> Vec<Value> {
    text.lines().skip(1).take(20000).filter_map(|line|{
        let v:Vec<_>=line.split_whitespace().collect();let(src,sport)=endpoint(v.get(1)?)?;let(dst,dport)=endpoint(v.get(2)?)?;
        let state=u8::from_str_radix(v.get(3)?,16).ok()?;let uid=v.get(7)?.parse::<u32>().ok()?;let inode=v.get(9)?.parse::<u64>().ok()?;
        if inode==0{return None}
        Some(json!({"id":format!("{proto}:{inode}"),"inode":inode,"proto":proto,"local":src.to_string(),"sport":sport,"remote":dst.to_string(),"port":dport,"uid":uid,"state":state,"listening":state==10||(proto=="udp"&&dport==0),"incoming":false,"scope":scope(dst),"rx":null,"tx":null,"pid":null,"app":"","cc":null}))
    }).collect()
}
pub fn app_of_cgroup(text: &str, uid: u32) -> String {
    let prefix = format!("0::/system.slice/lordos.service/lordos-session-{uid}/");
    text.lines()
        .find_map(|line| {
            let path = line.strip_prefix(&prefix)?;
            let scope = path.split('/').next()?;
            let id = scope.strip_prefix("app-lordos-")?.strip_suffix(".scope")?;
            let (kind, id) = id
                .strip_prefix("lpk-")
                .map(|id| ("lpk", id))
                .unwrap_or(("desktop", id));
            if id.is_empty()
                || id.len() > 150
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b))
            {
                return None;
            }
            Some(format!("{kind}:{id}"))
        })
        .unwrap_or_default()
}
pub fn collect(root: &Path, geo: Option<&Database>) -> Value {
    let mut list = Vec::new();
    for (name, proto) in [
        ("tcp", "tcp"),
        ("tcp6", "tcp"),
        ("udp", "udp"),
        ("udp6", "udp"),
        ("icmp", "icmp"),
        ("icmp6", "icmp6"),
        ("raw", "raw"),
        ("raw6", "raw6"),
    ] {
        list.extend(table(
            &read(&root.join("proc/net").join(name), 4 << 20),
            proto,
        ));
    }
    let traffic = if root == Path::new("/") {
        tcp_counters()
    } else {
        HashMap::new()
    };
    let listeners: std::collections::HashSet<_> = list
        .iter()
        .filter(|s| s["listening"] == true)
        .filter_map(|s| s["sport"].as_u64())
        .collect();
    let mut owners = HashMap::new();
    let uid = unsafe { libc::getuid() };
    for dir in entries(&root.join("proc")).into_iter().take(20000) {
        let Some(pid) = dir
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        use std::os::unix::fs::MetadataExt;
        if std::fs::metadata(&dir)
            .map(|m| m.uid() != uid)
            .unwrap_or(true)
        {
            continue;
        }
        let name = read(&dir.join("comm"), 256).trim().to_owned();
        let exe = std::fs::read_link(dir.join("exe"))
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let app = if pid == std::process::id() {
            "lordos".into()
        } else {
            app_of_cgroup(&read(&dir.join("cgroup"), 8192), uid)
        };
        for fd in entries(&dir.join("fd")).into_iter().take(8192) {
            if let Ok(link) = std::fs::read_link(fd) {
                if let Some(inode) = link
                    .to_str()
                    .and_then(|s| s.strip_prefix("socket:["))
                    .and_then(|s| s.strip_suffix(']'))
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    owners
                        .entry(inode)
                        .or_insert((pid, name.clone(), exe.clone(), app.clone()));
                }
            }
        }
    }
    let mut others = 0;
    for item in &mut list {
        if item["uid"].as_u64() != Some(uid as u64) {
            others += 1;
            continue;
        }
        if let Some((pid, name, exe, app)) = item["inode"].as_u64().and_then(|i| owners.get(&i)) {
            item["pid"] = json!(pid);
            item["name"] = json!(name);
            item["exe"] = json!(exe);
            item["app"] = json!(app);
        }
        if let Some((rx, tx)) = item["inode"].as_u64().and_then(|i| traffic.get(&i)) {
            item["rx"] = json!(rx);
            item["tx"] = json!(tx);
        }
        item["incoming"] = json!(
            item["listening"] != true
                && item["sport"]
                    .as_u64()
                    .is_some_and(|p| listeners.contains(&p))
        );
        if let Some(db) = geo {
            if let Some(cc) = item["remote"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .and_then(|addr| db.country(addr))
            {
                item["cc"] = json!(cc)
            }
        }
    }
    list.retain(|s| s["uid"].as_u64() == Some(uid as u64));
    json!({"list":list,"others":others,"diag":true})
}

// inet_diag TCP_INFO attributes carry lifetime byte counts. All datagram and
// attribute lengths are checked before inspecting offsets from linux/tcp.h.
pub fn diag_messages(bytes: &[u8], counts: &mut HashMap<u64, (u64, u64)>) -> bool {
    let mut at = 0;
    while at + 16 <= bytes.len() {
        let len = u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        if len < 16 || at + len > bytes.len() {
            break;
        }
        let kind = u16::from_ne_bytes(bytes[at + 4..at + 6].try_into().unwrap());
        if kind == 2 || kind == 3 {
            return true;
        }
        let data = &bytes[at + 16..at + len];
        if kind == 20 && data.len() >= 72 {
            let inode = u32::from_ne_bytes(data[68..72].try_into().unwrap()) as u64;
            let mut offset = 72;
            while offset + 4 <= data.len() {
                let n = u16::from_ne_bytes(data[offset..offset + 2].try_into().unwrap()) as usize;
                let kind = u16::from_ne_bytes(data[offset + 2..offset + 4].try_into().unwrap());
                if n < 4 || offset + n > data.len() {
                    break;
                }
                let info = &data[offset + 4..offset + n];
                if kind == 2 && info.len() >= 136 {
                    counts.insert(
                        inode,
                        (
                            u64::from_ne_bytes(info[128..136].try_into().unwrap()),
                            u64::from_ne_bytes(info[120..128].try_into().unwrap()),
                        ),
                    );
                }
                offset += (n + 3) & !3;
            }
        }
        at += (len + 3) & !3;
    }
    false
}
fn tcp_counters() -> HashMap<u64, (u64, u64)> {
    let mut counts = HashMap::new();
    for family in [libc::AF_INET, libc::AF_INET6] {
        unsafe {
            let fd = libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_DGRAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                libc::NETLINK_INET_DIAG,
            );
            if fd < 0 {
                continue;
            }
            let mut req = [0u8; 72];
            req[..4].copy_from_slice(&72u32.to_ne_bytes());
            req[4..6].copy_from_slice(&20u16.to_ne_bytes());
            req[6..8].copy_from_slice(&0x301u16.to_ne_bytes());
            req[8..12].copy_from_slice(&1u32.to_ne_bytes());
            req[16] = family as u8;
            req[17] = libc::IPPROTO_TCP as u8;
            req[18] = 2;
            req[20..24].copy_from_slice(&u32::MAX.to_ne_bytes());
            let mut addr: libc::sockaddr_nl = std::mem::zeroed();
            addr.nl_family = libc::AF_NETLINK as u16;
            if libc::sendto(
                fd,
                req.as_ptr().cast(),
                req.len(),
                0,
                (&addr as *const libc::sockaddr_nl).cast(),
                std::mem::size_of_val(&addr) as u32,
            ) >= 0
            {
                let mut data = vec![0u8; 65536];
                for _ in 0..128 {
                    let mut poll = libc::pollfd {
                        fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    if libc::poll(&mut poll, 1, 50) <= 0 {
                        break;
                    }
                    let n = libc::recv(fd, data.as_mut_ptr().cast(), data.len(), 0);
                    if n <= 0 || diag_messages(&data[..n as usize], &mut counts) {
                        break;
                    }
                    if counts.len() > 20000 {
                        break;
                    }
                }
            }
            libc::close(fd);
        }
    }
    counts
}
