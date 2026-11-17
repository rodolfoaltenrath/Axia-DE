//! Receive-only DNS learning: resolved's root monitor, or configured resolver replies.
use crate::fwrules::norm_domain;
use std::{
    ffi::{c_char, c_void},
    io::{Read, Write},
    net::IpAddr,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::{fs::FileTypeExt, net::UnixStream},
};
pub type Callback = unsafe extern "C" fn(*mut c_void, *const u8, usize);
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NpAddr {
    pub fam: i32,
    pub addr: [u8; 16],
}
fn base64(s: &[u8]) -> Option<Vec<u8>> {
    if s.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::new();
    for (i, b) in s.chunks_exact(4).enumerate() {
        let mut x = 0u32;
        let mut pad = 0;
        for (k, &c) in b.iter().enumerate() {
            let v = match c {
                b'A'..=b'Z' => (c - b'A') as u32,
                b'a'..=b'z' => (c - b'a' + 26) as u32,
                b'0'..=b'9' => (c - b'0' + 52) as u32,
                b'+' => 62,
                b'/' => 63,
                b'=' if (i + 1) * 4 == s.len() && k >= 2 => {
                    pad += 1;
                    0
                }
                _ => return None,
            };
            if pad > 0 && c != b'=' {
                return None;
            }
            x = x << 6 | v;
        }
        out.extend_from_slice(&x.to_be_bytes()[1..4 - pad]);
    }
    Some(out)
}
pub fn varlink(json: &[u8]) -> Result<Vec<Vec<u8>>, ()> {
    let root: serde_json::Value = crate::policy::strict_json(json).map_err(|_| ())?;
    let p = &root["parameters"];
    if p["state"] != "success" {
        return Ok(vec![]);
    }
    let (Some(qs), Some(ans)) = (p["question"].as_array(), p["answer"].as_array()) else {
        return Ok(vec![]);
    };
    let mut done = Vec::new();
    let mut packets = Vec::new();
    for q in qs {
        if done.len() == 4 {
            break;
        }
        let Some(name) = q["name"].as_str().and_then(|s| norm_domain(s.as_bytes())) else {
            continue;
        };
        if done.contains(&name) {
            continue;
        }
        done.push(name.clone());
        let mut pkt = vec![0; 12];
        pkt[2] = 0x81;
        pkt[3] = 0x80;
        pkt[5] = 1;
        for label in name.split(|&x| x == b'.') {
            pkt.push(label.len() as u8);
            pkt.extend_from_slice(label)
        }
        pkt.extend_from_slice(&[0, 0, 1, 0, 1]);
        let mut n = 0u16;
        for a in ans {
            if n == 512 {
                break;
            }
            let Some(raw) = a["raw"]
                .as_str()
                .filter(|s| s.len() <= 4096)
                .and_then(|s| base64(s.as_bytes()))
            else {
                continue;
            };
            if raw.len() < 11 || pkt.len() + raw.len() > 65535 {
                continue;
            }
            pkt.extend_from_slice(&raw);
            n += 1;
        }
        if n != 0 {
            pkt[6..8].copy_from_slice(&n.to_be_bytes());
            packets.push(pkt)
        }
    }
    Ok(packets)
}
pub fn packet<'a>(p: &'a [u8], servers: &[NpAddr]) -> Option<&'a [u8]> {
    if p.len() < 20 {
        return None;
    }
    let (fam, src, off, end) = match p[0] >> 4 {
        4 => {
            let h = (p[0] & 15) as usize * 4;
            let t = u16::from_be_bytes([p[2], p[3]]) as usize;
            if h < 20
                || t < h
                || t > p.len()
                || p[9] != 17
                || u16::from_be_bytes([p[6], p[7]]) & 0x3fff != 0
            {
                return None;
            }
            (4, &p[12..16], h, t)
        }
        6 => {
            if p.len() < 40 || p[6] != 17 {
                return None;
            }
            let t = 40 + u16::from_be_bytes([p[4], p[5]]) as usize;
            if t > p.len() {
                return None;
            }
            (6, &p[8..24], 40, t)
        }
        _ => return None,
    };
    if end - off < 8 || p[off..off + 2] != [0, 53] {
        return None;
    }
    let n = u16::from_be_bytes([p[off + 4], p[off + 5]]) as usize;
    if n < 8
        || n > end - off
        || !servers
            .iter()
            .any(|s| s.fam == fam && &s.addr[..src.len()] == src)
    {
        return None;
    }
    Some(&p[off + 8..off + n])
}
#[no_mangle]
pub unsafe extern "C" fn npdns_from_varlink(
    json: *const c_char,
    len: usize,
    cb: Option<Callback>,
    u: *mut c_void,
) -> i32 {
    if json.is_null() || len > 1 << 20 {
        return -1;
    }
    let Some(cb) = cb else { return -1 };
    match varlink(std::slice::from_raw_parts(json.cast(), len)) {
        Ok(v) => {
            for p in &v {
                cb(u, p.as_ptr(), p.len())
            }
            v.len() as i32
        }
        Err(_) => -1,
    }
}
#[no_mangle]
pub unsafe extern "C" fn npdns_from_packet(
    p: *const u8,
    len: usize,
    servers: *const NpAddr,
    n: i32,
    dns: *mut *const u8,
    dl: *mut usize,
) -> i32 {
    if p.is_null()
        || len > 65536
        || n < 0
        || (n > 0 && servers.is_null())
        || dns.is_null()
        || dl.is_null()
    {
        return -1;
    }
    let srv = if n == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(servers, n as usize)
    };
    if let Some(d) = packet(std::slice::from_raw_parts(p, len), srv) {
        dns.write(d.as_ptr());
        dl.write(d.len());
        0
    } else {
        -1
    }
}
const MONITOR: &str = "/run/systemd/resolve/io.systemd.Resolve.Monitor";
const MAX: usize = 1 << 20;
pub struct Monitor {
    stream: Option<UnixStream>,
    raw: Option<OwnedFd>,
    buf: Vec<u8>,
    servers: Vec<NpAddr>,
    last_try: i64,
    last_conf: i64,
}
impl Default for Monitor {
    fn default() -> Self {
        Self {
            stream: None,
            raw: None,
            buf: vec![],
            servers: vec![],
            last_try: -100,
            last_conf: -100,
        }
    }
}
impl Monitor {
    pub fn fd(&self) -> i32 {
        self.stream
            .as_ref()
            .map(AsRawFd::as_raw_fd)
            .or_else(|| self.raw.as_ref().map(AsRawFd::as_raw_fd))
            .unwrap_or(-1)
    }
    pub fn source(&self) -> &'static str {
        if self.stream.is_some() {
            "resolved"
        } else if self.raw.is_some() {
            "packet"
        } else {
            "none"
        }
    }
    pub fn tick(&mut self, now: i64) {
        let resolved = std::fs::metadata(MONITOR).is_ok_and(|m| m.file_type().is_socket());
        if resolved {
            if self.stream.is_none() && now - self.last_try >= 5 {
                self.last_try = now;
                if let Ok(mut s) = UnixStream::connect(MONITOR) {
                    let _ = s.set_write_timeout(Some(std::time::Duration::from_millis(500)));
                    if s.write_all(b"{\"method\":\"io.systemd.Resolve.Monitor.SubscribeQueryResults\",\"more\":true}\0").is_ok() && s.set_nonblocking(true).is_ok(){self.stream=Some(s);self.buf.clear();}
                }
            }
            if self.stream.is_some() {
                self.raw = None
            }
        } else {
            self.stream = None;
            if self.raw.is_none() && now - self.last_try >= 5 {
                self.last_try = now;
                self.raw = open_packet();
                self.last_conf = -100
            }
        }
        if self.raw.is_some() && now - self.last_conf >= 30 {
            self.last_conf = now;
            self.servers = std::fs::read_to_string("/etc/resolv.conf")
                .unwrap_or_default()
                .lines()
                .filter_map(|l| {
                    let mut w = l.split_whitespace();
                    if w.next() != Some("nameserver") {
                        return None;
                    }
                    let s = w.next()?.split('%').next()?;
                    let ip: IpAddr = s.parse().ok()?;
                    let mut a = NpAddr::default();
                    match ip {
                        IpAddr::V4(v) => {
                            a.fam = 4;
                            a.addr[..4].copy_from_slice(&v.octets())
                        }
                        IpAddr::V6(v) => {
                            a.fam = 6;
                            a.addr = v.octets()
                        }
                    }
                    Some(a)
                })
                .take(8)
                .collect()
        }
    }
    pub fn ready(&mut self) -> Vec<Vec<u8>> {
        let mut out = vec![];
        if let Some(s) = &mut self.stream {
            let mut buf = [0; 8192];
            loop {
                match s.read(&mut buf) {
                    Ok(0) => {
                        self.stream = None;
                        self.buf.clear();
                        break;
                    }
                    Ok(n) => {
                        if self.buf.len() + n > MAX {
                            self.stream = None;
                            self.buf.clear();
                            break;
                        }
                        self.buf.extend_from_slice(&buf[..n]);
                        while let Some(i) = self.buf.iter().position(|&x| x == 0) {
                            if let Ok(v) = varlink(&self.buf[..i]) {
                                out.extend(v)
                            }
                            self.buf.drain(..=i);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        self.stream = None;
                        self.buf.clear();
                        break;
                    }
                }
            }
        } else if let Some(fd) = &self.raw {
            let mut buf = vec![0; 65536];
            for _ in 0..256 {
                unsafe {
                    let mut sa: libc::sockaddr_ll = std::mem::zeroed();
                    let mut len = size_of_val(&sa) as u32;
                    let n = libc::recvfrom(
                        fd.as_raw_fd(),
                        buf.as_mut_ptr().cast(),
                        buf.len(),
                        0,
                        (&mut sa as *mut libc::sockaddr_ll).cast(),
                        &mut len,
                    );
                    if n <= 0 {
                        break;
                    }
                    if sa.sll_pkttype == libc::PACKET_OUTGOING {
                        continue;
                    }
                    if let Some(p) = packet(&buf[..n as usize], &self.servers) {
                        out.push(p.to_vec())
                    }
                }
            }
        }
        out
    }
}
fn open_packet() -> Option<OwnedFd> {
    unsafe {
        let fd = libc::socket(
            libc::AF_PACKET,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            (libc::ETH_P_ALL as u16).to_be() as i32,
        );
        if fd < 0 {
            return None;
        }
        let fd = OwnedFd::from_raw_fd(fd);
        let ops = [
            (0x30, 0, 0, 0),
            (0x74, 0, 0, 4),
            (0x15, 0, 7, 4),
            (0x30, 0, 0, 9),
            (0x15, 0, 11, 17),
            (0x28, 0, 0, 6),
            (0x45, 9, 0, 0x3fff),
            (0xb1, 0, 0, 0),
            (0x48, 0, 0, 0),
            (0x15, 5, 6, 53),
            (0x15, 0, 5, 6),
            (0x30, 0, 0, 6),
            (0x15, 0, 3, 17),
            (0x28, 0, 0, 40),
            (0x15, 0, 1, 53),
            (0x06, 0, 0, 0xffff),
            (0x06, 0, 0, 0),
        ];
        let mut code: Vec<libc::sock_filter> = ops
            .iter()
            .map(|&(code, jt, jf, k)| libc::sock_filter { code, jt, jf, k })
            .collect();
        let f = libc::sock_fprog {
            len: code.len() as u16,
            filter: code.as_mut_ptr(),
        };
        if libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_ATTACH_FILTER,
            (&f as *const libc::sock_fprog).cast(),
            size_of_val(&f) as u32,
        ) < 0
        {
            None
        } else {
            Some(fd)
        }
    }
}
