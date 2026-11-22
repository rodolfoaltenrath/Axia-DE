//! A small netlink client: a raw socket, dump requests, attribute tables and the notification
//! socket. Replies are untrusted-sized data and are walked with bounds, like `NLMSG_OK` and
//! `RTA_OK` do.
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicU32, Ordering};

pub const NETLINK_ROUTE: i32 = 0;

pub const RTM_NEWLINK: u16 = 16;
pub const RTM_DELLINK: u16 = 17;
pub const RTM_GETLINK: u16 = 18;
pub const RTM_NEWADDR: u16 = 20;
pub const RTM_DELADDR: u16 = 21;
pub const RTM_GETADDR: u16 = 22;
pub const RTM_NEWROUTE: u16 = 24;
pub const RTM_DELROUTE: u16 = 25;
pub const RTM_GETROUTE: u16 = 26;

const NLM_F_REQUEST: u16 = 1;
const NLM_F_MULTI: u16 = 2;
const NLM_F_DUMP: u16 = 0x300;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLMSG_HDRLEN: usize = 16;

pub const RTMGRP_LINK: u32 = 1;
pub const RTMGRP_IPV4_IFADDR: u32 = 0x10;
pub const RTMGRP_IPV4_ROUTE: u32 = 0x40;
pub const RTMGRP_IPV6_IFADDR: u32 = 0x100;
pub const RTMGRP_IPV6_ROUTE: u32 = 0x400;

static SEQ: AtomicU32 = AtomicU32::new(0);

/// A netlink socket (close-on-exec), bound to `groups` (0: requests only).
pub struct Socket(pub OwnedFd);

impl Socket {
    pub fn open(proto: i32, groups: u32) -> io::Result<Socket> {
        // SAFETY: plain socket calls; the descriptor is ours from here on.
        unsafe {
            let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_RAW | libc::SOCK_CLOEXEC, proto);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let fd = OwnedFd::from_raw_fd(fd);
            let mut sa: libc::sockaddr_nl = std::mem::zeroed();
            sa.nl_family = libc::AF_NETLINK as libc::sa_family_t;
            sa.nl_groups = groups;
            if libc::bind(fd.as_raw_fd(), (&sa as *const libc::sockaddr_nl).cast(), size_of::<libc::sockaddr_nl>() as libc::socklen_t) != 0 {
                return Err(io::Error::last_os_error());
            }
            let one: libc::c_int = 1;
            libc::setsockopt(fd.as_raw_fd(), libc::SOL_NETLINK, libc::NETLINK_EXT_ACK, (&one as *const libc::c_int).cast(), size_of::<libc::c_int>() as libc::socklen_t);
            Ok(Socket(fd))
        }
    }

    /// Asks for everything of one kind (`ty` is an `RTM_GET*`, `body` the family header: its size
    /// and the family byte) and calls `f` for each message of the answer. `Err` carries the
    /// kernel's (negative) error, or minus the errno of a failed send or receive.
    pub fn dump(&self, ty: u16, body: &[u8], mut f: impl FnMut(&Msg)) -> Result<(), i32> {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        let len = NLMSG_HDRLEN + body.len();
        let mut req = Vec::with_capacity(len);
        req.extend_from_slice(&(len as u32).to_ne_bytes());
        req.extend_from_slice(&ty.to_ne_bytes());
        req.extend_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
        req.extend_from_slice(&seq.to_ne_bytes());
        req.extend_from_slice(&0u32.to_ne_bytes());
        req.extend_from_slice(body);
        // SAFETY: a zeroed sockaddr_nl is the kernel's address; `req` is a valid buffer.
        let sent = unsafe {
            let mut sa: libc::sockaddr_nl = std::mem::zeroed();
            sa.nl_family = libc::AF_NETLINK as libc::sa_family_t;
            libc::sendto(self.0.as_raw_fd(), req.as_ptr().cast(), req.len(), 0, (&sa as *const libc::sockaddr_nl).cast(), size_of::<libc::sockaddr_nl>() as libc::socklen_t)
        };
        if sent != req.len() as isize {
            return Err(-io::Error::last_os_error().raw_os_error().unwrap_or(libc::EIO));
        }
        let mut buf = vec![0u8; 32768];
        loop {
            // SAFETY: `buf` is ours.
            let r = unsafe { libc::recv(self.0.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0) };
            if r < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(-e.raw_os_error().unwrap_or(libc::EIO));
            }
            for m in Messages(&buf[..r as usize]) {
                if m.seq != seq {
                    continue;
                }
                if m.ty == NLMSG_ERROR {
                    // 0 is the ACK
                    let e = m.payload.get(..4).map_or(0, |b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]));
                    return if e == 0 { Ok(()) } else { Err(e) };
                }
                if m.ty == NLMSG_DONE {
                    return Ok(());
                }
                f(&m);
                if m.flags & NLM_F_MULTI == 0 {
                    return Ok(());
                }
            }
        }
    }
}

/// One message of a datagram: its header fields and what follows the header.
pub struct Msg<'a> {
    pub ty: u16,
    pub flags: u16,
    pub seq: u32,
    pub payload: &'a [u8],
}

/// The messages in a buffer, as `NLMSG_OK` and `NLMSG_NEXT` walk them (a length that is too short
/// or runs past the end stops the walk).
pub struct Messages<'a>(pub &'a [u8]);

impl<'a> Iterator for Messages<'a> {
    type Item = Msg<'a>;
    fn next(&mut self) -> Option<Msg<'a>> {
        let b = self.0;
        if b.len() < NLMSG_HDRLEN {
            return None;
        }
        let len = u32::from_ne_bytes([b[0], b[1], b[2], b[3]]) as usize;
        if len < NLMSG_HDRLEN || len > b.len() {
            return None;
        }
        let m = Msg {
            ty: u16::from_ne_bytes([b[4], b[5]]),
            flags: u16::from_ne_bytes([b[6], b[7]]),
            seq: u32::from_ne_bytes([b[8], b[9], b[10], b[11]]),
            payload: &b[NLMSG_HDRLEN..len],
        };
        self.0 = &b[(len + 3 & !3).min(b.len())..];
        Some(m)
    }
}

/// The attributes after a family header, by type up to `max` (the last one of a type wins).
/// `bytes` is where the header ends: a payload shorter than `hdr` has none.
pub fn attrs(payload: &[u8], hdr: usize, max: usize) -> Vec<Option<&[u8]>> {
    let mut tb: Vec<Option<&[u8]>> = vec![None; max + 1];
    let mut rest = payload.get(hdr..).unwrap_or(&[]);
    while rest.len() >= 4 {
        let len = u16::from_ne_bytes([rest[0], rest[1]]) as usize;
        if len < 4 || len > rest.len() {
            break;
        }
        let ty = (u16::from_ne_bytes([rest[2], rest[3]]) & !(0x8000 | 0x4000)) as usize; // NESTED, NET_BYTEORDER
        if ty <= max {
            tb[ty] = Some(&rest[4..len]);
        }
        rest = &rest[(len + 3 & !3).min(rest.len())..];
    }
    tb
}

/// The attributes nested inside one attribute (`IFLA_LINKINFO`).
pub fn nested(data: &[u8], max: usize) -> Vec<Option<&[u8]>> {
    attrs(data, 0, max)
}

pub fn u32_of(b: Option<&[u8]>) -> Option<u32> {
    let b = b?.get(..4)?;
    Some(u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(ty: u16, data: &[u8]) -> Vec<u8> {
        let mut v = ((4 + data.len()) as u16).to_ne_bytes().to_vec();
        v.extend_from_slice(&ty.to_ne_bytes());
        v.extend_from_slice(data);
        while v.len() % 4 != 0 {
            v.push(0);
        }
        v
    }

    fn msg(ty: u16, seq: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = ((NLMSG_HDRLEN + payload.len()) as u32).to_ne_bytes().to_vec();
        v.extend_from_slice(&ty.to_ne_bytes());
        v.extend_from_slice(&0u16.to_ne_bytes());
        v.extend_from_slice(&seq.to_ne_bytes());
        v.extend_from_slice(&0u32.to_ne_bytes());
        v.extend_from_slice(payload);
        while v.len() % 4 != 0 {
            v.push(0);
        }
        v
    }

    #[test]
    fn messages_are_walked_with_bounds() {
        let mut b = msg(16, 1, b"abcde");
        b.extend(msg(17, 2, b"xy"));
        let all: Vec<_> = Messages(&b).map(|m| (m.ty, m.seq, m.payload.to_vec())).collect();
        assert_eq!(all, [(16, 1, b"abcde".to_vec()), (17, 2, b"xy".to_vec())]);
        // a length that runs past the end, one that is too short, and a cut header
        let mut bad = msg(16, 1, b"abcd");
        bad[0] = 200;
        assert_eq!(Messages(&bad).count(), 0);
        let mut short = msg(16, 1, b"abcd");
        short[0] = 8;
        assert_eq!(Messages(&short).count(), 0);
        assert_eq!(Messages(&b[..15]).count(), 0);
        assert_eq!(Messages(&b[..b.len() - 3]).count(), 1, "the second one is cut");
    }

    #[test]
    fn attributes_are_found_by_type_and_the_last_one_wins() {
        let mut p = vec![0u8; 8]; // a family header
        p.extend(attr(1, b"aaaa"));
        p.extend(attr(3, b"eth0\0"));
        p.extend(attr(1, b"bbbbbb"));
        p.extend(attr(0x8000 | 18, b"nested"));
        p.extend(attr(99, b"ignored"));
        let tb = attrs(&p, 8, 23);
        assert_eq!((tb[1], tb[3], tb[18], tb[2]), (Some(&b"bbbbbb"[..]), Some(&b"eth0\0"[..]), Some(&b"nested"[..]), None));
        assert_eq!(u32_of(Some(&[1, 0, 0, 0, 9])), Some(u32::from_ne_bytes([1, 0, 0, 0])));
        assert_eq!(u32_of(Some(&[1, 0, 0])), None);
        // a length that is too short or runs past the end stops the walk
        let mut bad = attr(1, b"aaaa");
        bad[0] = 2;
        assert!(attrs(&bad, 0, 5)[1].is_none());
        let mut over = attr(1, b"aaaa");
        over[0] = 40;
        assert!(attrs(&over, 0, 5)[1].is_none());
        assert!(attrs(&[1, 2, 3], 0, 5).iter().all(Option::is_none));
        assert!(attrs(&p, 400, 5).iter().all(Option::is_none), "a header longer than the message has no attributes");
    }
}
