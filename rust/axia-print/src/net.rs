//! The sockets of the printers' own scope. In a LordOS session `axia-server` hands over a
//! function (`print_set_connector`, the `netconn_socket` of server/src/sec/netconn.h) that asks the
//! printers' connector process to make the socket, so the kernel, the firewall and Monitor see the
//! traffic as the app "lordos:print". With none set (development, the tests) the socket is made
//! here, as the connector does outside a session.
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicUsize, Ordering};

/// `int netconn_socket(NcComponent comp, int fam, int type, const struct sockaddr *to, socklen_t tolen)`
pub type Connector = unsafe extern "C" fn(libc::c_int, libc::c_int, libc::c_int, *const libc::sockaddr, libc::socklen_t) -> libc::c_int;

/// NC_PRINT in server/src/sec/netconn.h.
const NC_PRINT: libc::c_int = 1;

static CONNECTOR: AtomicUsize = AtomicUsize::new(0);

pub fn set_connector(f: Option<Connector>) {
    CONNECTOR.store(f.map_or(0, |f| f as usize), Ordering::Release);
}

/// A new non-blocking, close-on-exec IPv4 socket of `ty` (SOCK_STREAM / SOCK_DGRAM). With `to`
/// the connection is started too (EINPROGRESS: the caller polls for POLLOUT and reads SO_ERROR).
pub fn socket(ty: libc::c_int, to: Option<&libc::sockaddr_in>) -> io::Result<OwnedFd> {
    let (addr, len) = match to {
        Some(a) => (a as *const libc::sockaddr_in as *const libc::sockaddr, size_of::<libc::sockaddr_in>() as libc::socklen_t),
        None => (std::ptr::null(), 0),
    };
    let f = CONNECTOR.load(Ordering::Acquire);
    // SAFETY: `f` is 0 or a function pointer stored by set_connector; `addr` points at `to` or is NULL.
    let fd = unsafe {
        if f != 0 {
            let f: Connector = std::mem::transmute::<usize, Connector>(f);
            f(NC_PRINT, libc::AF_INET, ty, addr, len)
        } else {
            let fd = libc::socket(libc::AF_INET, ty | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC, 0);
            if fd >= 0 && !addr.is_null() && libc::connect(fd, addr, len) != 0 && *libc::__errno_location() != libc::EINPROGRESS {
                let e = io::Error::last_os_error();
                libc::close(fd);
                return Err(e);
            }
            fd
        }
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor that nobody else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

    #[test]
    fn a_socket_without_a_connector_is_non_blocking_and_close_on_exec() {
        let fd = socket(libc::SOCK_DGRAM, None).unwrap();
        let (fl, fdfl) = unsafe { (libc::fcntl(fd.as_raw_fd(), libc::F_GETFL), libc::fcntl(fd.as_raw_fd(), libc::F_GETFD)) };
        assert!(fl & libc::O_NONBLOCK != 0 && fdfl & libc::FD_CLOEXEC != 0);
    }
}
