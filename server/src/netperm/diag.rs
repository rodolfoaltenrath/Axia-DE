//! SOCK_DIAG only; never opens an Internet socket. Socket cookies prevent tuple-reuse races.
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::atomic::{AtomicU32, Ordering},
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Socket {
    pub fam: i32,
    pub proto: i32,
    pub state: u8,
    pub src: [u8; 16],
    pub dst: [u8; 16],
    pub sport: u16,
    pub dport: u16,
    pub ifindex: u32,
    pub cookie: [u32; 2],
    pub cgid: u64,
}
static SEQ: AtomicU32 = AtomicU32::new(1);
fn u32at(b: &[u8], o: usize) -> u32 {
    u32::from_ne_bytes(b[o..o + 4].try_into().unwrap())
}
fn u16at(b: &[u8], o: usize) -> u16 {
    u16::from_ne_bytes(b[o..o + 2].try_into().unwrap())
}
pub fn messages(b: &[u8]) -> io::Result<Vec<&[u8]>> {
    let mut out = vec![];
    let mut off = 0;
    while off + 16 <= b.len() {
        let n = u32at(b, off) as usize;
        if n < 16 || n > b.len() - off {
            return Err(io::Error::from_raw_os_error(libc::EPROTO));
        }
        out.push(&b[off..off + n]);
        off += (n + 3) & !3;
    }
    Ok(out)
}
pub fn decode(m: &[u8], proto: i32) -> Option<Socket> {
    if m.len() < 88 {
        return None;
    }
    let b = &m[16..];
    let fam = b[0] as i32;
    if fam != libc::AF_INET && fam != libc::AF_INET6 {
        return None;
    }
    let mut s = Socket {
        fam,
        proto,
        state: b[1],
        sport: u16::from_be_bytes([b[4], b[5]]),
        dport: u16::from_be_bytes([b[6], b[7]]),
        ifindex: u32at(b, 40),
        cookie: [u32at(b, 44), u32at(b, 48)],
        ..Default::default()
    };
    let n = if fam == 2 { 4 } else { 16 };
    s.src[..n].copy_from_slice(&b[8..8 + n]);
    s.dst[..n].copy_from_slice(&b[24..24 + n]);
    let mut off = 88;
    while off + 4 <= m.len() {
        let n = u16at(m, off) as usize;
        let ty = u16at(m, off + 2) & 0x3fff;
        if n < 4 || n > m.len() - off {
            break;
        }
        if ty == 21 && n >= 12 {
            s.cgid = u64::from_ne_bytes(m[off + 4..off + 12].try_into().ok()?)
        }
        off += (n + 3) & !3;
    }
    Some(s)
}
pub struct Diag(OwnedFd);
impl Diag {
    pub fn open() -> io::Result<Self> {
        unsafe {
            let fd = libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                libc::NETLINK_SOCK_DIAG,
            );
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let fd = OwnedFd::from_raw_fd(fd);
            let tv = libc::timeval {
                tv_sec: 0,
                tv_usec: 500000,
            };
            if libc::setsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                (&tv as *const libc::timeval).cast(),
                size_of_val(&tv) as u32,
            ) < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(fd))
        }
    }
    fn request(
        &self,
        ty: u16,
        flags: u16,
        fam: i32,
        proto: i32,
        states: u32,
        s: Option<&Socket>,
    ) -> io::Result<u32> {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let mut q = [0u8; 72];
        q[..4].copy_from_slice(&72u32.to_ne_bytes());
        q[4..6].copy_from_slice(&ty.to_ne_bytes());
        q[6..8].copy_from_slice(&(flags | 1).to_ne_bytes());
        q[8..12].copy_from_slice(&seq.to_ne_bytes());
        q[16] = fam as u8;
        q[17] = proto as u8;
        q[20..24].copy_from_slice(&states.to_ne_bytes());
        if let Some(s) = s {
            q[24..26].copy_from_slice(&s.sport.to_be_bytes());
            q[26..28].copy_from_slice(&s.dport.to_be_bytes());
            q[28..44].copy_from_slice(&s.src);
            q[44..60].copy_from_slice(&s.dst);
            q[60..64].copy_from_slice(&s.ifindex.to_ne_bytes());
            q[64..68].copy_from_slice(&s.cookie[0].to_ne_bytes());
            q[68..72].copy_from_slice(&s.cookie[1].to_ne_bytes());
        }
        unsafe {
            let mut sa = std::mem::zeroed::<libc::sockaddr_nl>();
            sa.nl_family = libc::AF_NETLINK as u16;
            let n = libc::sendto(
                self.0.as_raw_fd(),
                q.as_ptr().cast(),
                q.len(),
                0,
                (&sa as *const libc::sockaddr_nl).cast(),
                size_of_val(&sa) as u32,
            );
            if n != q.len() as isize {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(seq)
    }
    fn receive(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let n =
                unsafe { libc::recv(self.0.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0) };
            if n == 0 {
                return Err(io::Error::from_raw_os_error(libc::EIO));
            }
            if n > 0 {
                return Ok(n as usize);
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
    pub fn each(&self, fam: i32, proto: i32, states: u32) -> io::Result<Vec<Socket>> {
        let seq = self.request(20, 0x300, fam, proto, states, None)?;
        let mut out = vec![];
        let mut buf = vec![0; 32768];
        loop {
            let n = self.receive(&mut buf)?;
            for m in messages(&buf[..n])? {
                if u32at(m, 8) != seq {
                    continue;
                }
                match u16at(m, 4) {
                    3 => {
                        if u16at(m, 6) & 0x10 != 0 {
                            return Err(io::Error::from_raw_os_error(libc::EINTR));
                        }
                        return Ok(out);
                    }
                    2 => {
                        let e = if m.len() >= 20 {
                            u32at(m, 16) as i32
                        } else {
                            -libc::EIO
                        };
                        return Err(io::Error::from_raw_os_error(if e == 0 {
                            libc::EIO
                        } else {
                            -e
                        }));
                    }
                    20 => {
                        if let Some(s) = decode(m, proto) {
                            out.push(s)
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    pub fn destroy(&self, s: &Socket) -> io::Result<()> {
        let seq = self.request(21, 4, s.fam, s.proto, u32::MAX, Some(s))?;
        let mut buf = [0; 1024];
        loop {
            let n = self.receive(&mut buf)?;
            for m in messages(&buf[..n])? {
                if u32at(m, 8) != seq || u16at(m, 4) != 2 {
                    continue;
                }
                if m.len() < 20 {
                    return Err(io::Error::from_raw_os_error(libc::EIO));
                }
                let e = u32at(m, 16) as i32;
                return if e == 0 {
                    Ok(())
                } else {
                    Err(io::Error::from_raw_os_error(-e))
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_a_real_local_listener_without_privileges_or_attachment() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let diag = Diag::open().unwrap();
        let sockets = diag
            .each(libc::AF_INET, libc::IPPROTO_TCP, 1 << 10)
            .unwrap();
        let s = sockets
            .iter()
            .find(|s| s.sport == port && s.src[..4] == [127, 0, 0, 1])
            .expect("our listener in sock_diag dump");
        assert_eq!(s.state, 10);
        assert_eq!(s.dport, 0);
        assert_ne!(s.cookie, [u32::MAX; 2]);
    }
}
