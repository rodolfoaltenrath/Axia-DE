//! Kernel uevents (NETLINK_KOBJECT_UEVENT) for Settings: DRM connector changes (hotplugged displays),
//! power_supply (battery, charger) and backlight. One socket in the main loop's poll: nothing is
//! polled. Receiving kernel uevents needs no privileges.
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicI32, Ordering};

pub const UEV_DRM: i32 = 1;
pub const UEV_POWER: i32 = 2;
pub const UEV_BACKLIGHT: i32 = 4;

static FD: AtomicI32 = AtomicI32::new(-1);

/// A socket bound to the kernel's uevent group, non-blocking.
pub fn open_socket() -> Option<OwnedFd> {
    // SAFETY: plain socket calls; the descriptor is ours from here on.
    unsafe {
        let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, libc::NETLINK_KOBJECT_UEVENT);
        if fd < 0 {
            return None;
        }
        let fd = OwnedFd::from_raw_fd(fd);
        let mut a: libc::sockaddr_nl = std::mem::zeroed();
        a.nl_family = libc::AF_NETLINK as libc::sa_family_t;
        a.nl_groups = 1; // group 1: the kernel's
        if libc::bind(fd.as_raw_fd(), (&a as *const libc::sockaddr_nl).cast(), size_of::<libc::sockaddr_nl>() as libc::socklen_t) != 0 {
            return None;
        }
        Some(fd)
    }
}

/// Opens the process-wide socket the main loop polls. `false` if it cannot.
pub fn open() -> bool {
    if FD.load(Ordering::SeqCst) >= 0 {
        return true;
    }
    match open_socket() {
        Some(fd) => {
            FD.store(fd.into_raw_fd(), Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// For poll; -1 if closed.
pub fn fd() -> RawFd {
    FD.load(Ordering::SeqCst)
}

/// Parses one uevent datagram ("ACTION@devpath\0KEY=VAL\0…"): its `UEV_*` subsystem, or 0.
pub fn parse(buf: &[u8]) -> i32 {
    // udev's own (libudev) messages have no '@' header
    if buf.is_empty() || !buf.contains(&b'@') {
        return 0;
    }
    let mut off = 0;
    while off < buf.len() {
        let rest = &buf[off..];
        let n = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
        if n > 10 && rest.starts_with(b"SUBSYSTEM=") {
            return match &rest[10..n] {
                b"drm" => UEV_DRM,
                b"power_supply" => UEV_POWER,
                b"backlight" => UEV_BACKLIGHT,
                _ => 0,
            };
        }
        off += n + 1;
    }
    0
}

/// Reads every pending uevent on `fd`; a mask of the `UEV_*` subsystems that changed. Only the kernel's own
/// messages count.
pub fn dispatch_fd(fd: RawFd) -> i32 {
    let mut mask = 0;
    let mut buf = vec![0u8; 8192];
    loop {
        // SAFETY: a zeroed msghdr with one iovec over `buf`.
        let (r, from_pid) = unsafe {
            let mut from: libc::sockaddr_nl = std::mem::zeroed();
            let mut iov = libc::iovec { iov_base: buf.as_mut_ptr().cast(), iov_len: buf.len() - 1 };
            let mut m: libc::msghdr = std::mem::zeroed();
            m.msg_name = (&mut from as *mut libc::sockaddr_nl).cast();
            m.msg_namelen = size_of::<libc::sockaddr_nl>() as libc::socklen_t;
            m.msg_iov = &mut iov;
            m.msg_iovlen = 1;
            (libc::recvmsg(fd, &mut m, 0), from.nl_pid)
        };
        if r < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if from_pid != 0 {
            continue; // only the kernel
        }
        mask |= parse(&buf[..r as usize]);
    }
    mask
}

/// `dispatch_fd` on the process-wide socket.
pub fn dispatch() -> i32 {
    let fd = fd();
    if fd < 0 { 0 } else { dispatch_fd(fd) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(parts: &[&str]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.bytes().chain(std::iter::once(0))).collect()
    }

    #[test]
    fn a_datagram_names_its_subsystem() {
        assert_eq!(parse(&msg(&["change@/devices/pci0000:00/drm/card0", "ACTION=change", "SUBSYSTEM=drm", "HOTPLUG=1"])), UEV_DRM);
        assert_eq!(parse(&msg(&["change@/devices/x/BAT0", "SUBSYSTEM=power_supply"])), UEV_POWER);
        assert_eq!(parse(&msg(&["change@/devices/x/intel_backlight", "SUBSYSTEM=backlight"])), UEV_BACKLIGHT);
        assert_eq!(parse(&msg(&["add@/devices/x/usb1", "SUBSYSTEM=usb"])), 0);
        assert_eq!(parse(&msg(&["libudev\0x", "SUBSYSTEM=drm"])), 0, "udev's own messages have no @");
        assert_eq!(parse(&msg(&["change@/x", "ACTION=change"])), 0, "no subsystem");
        assert_eq!(parse(b""), 0);
        assert_eq!(parse(&msg(&["change@/x", "SUBSYSTEM=drm2"])), 0);
        assert_eq!(parse(b"change@/x\0SUBSYSTEM=dr"), 0, "cut in the middle");
    }

    #[test]
    fn the_socket_opens_and_has_nothing_to_say_yet() {
        let Some(fd) = open_socket() else {
            eprintln!("no uevent socket here; skipped");
            return;
        };
        // the machine may be busy: what matters is that reading never blocks and never panics
        let _ = dispatch_fd(fd.as_raw_fd());
    }
}
