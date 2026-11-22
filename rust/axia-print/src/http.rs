//! A minimal HTTP/1.1 client for printing: IPP to CUPS over its local socket and eSCL to the
//! network's scanners. One connection per request (`Connection: close`), a deadline on every wait
//! (poll) and caps on the sizes; what the network answers is untrusted.
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::time::{Duration, Instant};

/// A request that could not complete (no connection, timeout, a reply that is not HTTP or is over the cap).
#[derive(Debug, PartialEq)]
pub struct HttpError;

#[derive(Debug, Default)]
pub struct Response {
    pub status: i32,
    pub body: Vec<u8>,
    pub location: String,
    pub www_auth: String,
    pub content_type: String,
    pub retry_after: i32,
}

pub struct Request<'a> {
    pub method: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    /// Complete header lines, each ending in "\r\n".
    pub headers: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
    /// A file whose first `len` bytes follow the body (read with pread: the descriptor can be reused).
    pub doc: Option<(BorrowedFd<'a>, usize)>,
    /// The cap of the reply's body.
    pub max: usize,
    pub timeout_ms: u64,
}

/// Is it a local network IPv4 address (private, link-local or this machine)?
pub fn lan_ipv4(ip: &str) -> bool {
    let Ok(a) = ip.parse::<std::net::Ipv4Addr>() else { return false };
    let a = u32::from(a);
    (a >> 24) == 10 || (a >> 20) == (172 << 4 | 1) || (a >> 16) == (192 << 8 | 168) || (a >> 16) == (169 << 8 | 254) || (a >> 24) == 127
}

/// A connected, blocking descriptor of a local (unix) stream socket.
pub fn unix(path: &str) -> io::Result<OwnedFd> {
    let s = std::os::unix::net::UnixStream::connect(path)?;
    // SAFETY: into_raw_fd gives up a descriptor that is only ours.
    Ok(unsafe { OwnedFd::from_raw_fd(s.into_raw_fd()) })
}

/// A connected, blocking TCP descriptor made by the printers' connector (see `net`).
pub fn tcp(ip: &str, port: i32, timeout_ms: i32) -> io::Result<OwnedFd> {
    let bad = || io::Error::from(io::ErrorKind::InvalidInput);
    if !(1..=65535).contains(&port) {
        return Err(bad());
    }
    let ip: std::net::Ipv4Addr = ip.parse().map_err(|_| bad())?;
    // SAFETY: a plain C struct; zero is valid.
    let mut a: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    a.sin_family = libc::AF_INET as libc::sa_family_t;
    a.sin_port = (port as u16).to_be();
    a.sin_addr.s_addr = u32::from(ip).to_be();
    let fd = crate::net::socket(libc::SOCK_STREAM, Some(&a))?;
    let mut p = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
    let mut err: libc::c_int = 0;
    let mut el = size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: valid pointers to a pollfd and an int.
    let ok = unsafe {
        libc::poll(&mut p, 1, timeout_ms) == 1
            && libc::getsockopt(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_ERROR, (&mut err as *mut libc::c_int).cast(), &mut el) == 0
            && err == 0
    };
    if !ok {
        return Err(io::Error::from(io::ErrorKind::TimedOut));
    }
    // SAFETY: fcntl on our descriptor.
    unsafe {
        let fl = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, fl & !libc::O_NONBLOCK);
    }
    Ok(fd)
}

fn wait_io(fd: RawFd, ev: i16, deadline: Instant) -> Result<(), HttpError> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now()).as_millis().min(i32::MAX as u128) as i32;
        if left <= 0 {
            return Err(HttpError);
        }
        let mut p = libc::pollfd { fd, events: ev, revents: 0 };
        // SAFETY: a valid pollfd.
        let r = unsafe { libc::poll(&mut p, 1, left) };
        if r > 0 {
            return Ok(());
        }
        if r < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(HttpError);
    }
}

fn send_all(fd: RawFd, mut d: &[u8], deadline: Instant) -> Result<(), HttpError> {
    while !d.is_empty() {
        wait_io(fd, libc::POLLOUT, deadline)?;
        // SAFETY: `d` is a valid buffer.
        let w = unsafe { libc::send(fd, d.as_ptr().cast(), d.len(), libc::MSG_NOSIGNAL) };
        if w < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if w <= 0 {
            return Err(HttpError);
        }
        d = &d[w as usize..];
    }
    Ok(())
}

/// Buffered reading with a deadline.
struct Rd {
    fd: RawFd,
    buf: Box<[u8; 16384]>,
    a: usize,
    b: usize,
    deadline: Instant,
}

impl Rd {
    fn fill(&mut self) -> Result<(), HttpError> {
        if self.a < self.b {
            return Ok(());
        }
        wait_io(self.fd, libc::POLLIN, self.deadline)?;
        let n = loop {
            // SAFETY: the buffer is ours and big enough.
            let n = unsafe { libc::recv(self.fd, self.buf.as_mut_ptr().cast(), self.buf.len(), 0) };
            if n < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break n;
        };
        if n <= 0 {
            return Err(HttpError);
        }
        self.a = 0;
        self.b = n as usize;
        Ok(())
    }

    /// A line without its end (CRLF or LF), at most `n - 1` bytes long.
    fn line(&mut self, n: usize) -> Result<Vec<u8>, HttpError> {
        let mut out = Vec::new();
        loop {
            self.fill()?;
            let c = self.buf[self.a];
            self.a += 1;
            if c == b'\n' {
                break;
            }
            if out.len() + 1 < n {
                out.push(c);
            } else {
                return Err(HttpError); // line too long
            }
        }
        if out.last() == Some(&b'\r') {
            out.pop();
        }
        Ok(out)
    }

    /// Exactly `n` bytes, appended to `out`.
    fn bytes(&mut self, mut n: usize, out: &mut Vec<u8>, max: usize) -> Result<(), HttpError> {
        while n > 0 {
            self.fill()?;
            let k = (self.b - self.a).min(n);
            if out.len() + k > max {
                return Err(HttpError);
            }
            out.extend_from_slice(&self.buf[self.a..self.a + k]);
            self.a += k;
            n -= k;
        }
        Ok(())
    }
}

/// atoi: blanks, a sign, digits.
fn atoi(s: &[u8]) -> i32 {
    crate::strtol(s).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

/// A header value kept up to `n - 1` bytes.
fn keep(v: &[u8], n: usize) -> String {
    crate::lossy(&v[..v.len().min(n - 1)])
}

/// Sends the request and reads the complete reply. `Err` if the connection failed or the reply
/// is not HTTP, is cut, or is over the cap.
pub fn request(fd: &OwnedFd, rq: &Request) -> Result<Response, HttpError> {
    let fd = fd.as_raw_fd();
    let deadline = Instant::now() + Duration::from_millis(rq.timeout_ms);
    let doclen = rq.doc.as_ref().map_or(0, |d| d.1);
    let total = rq.body.len() + doclen;
    let mut head = format!("{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Axia/0.8\r\nConnection: close\r\n", rq.method, rq.path, rq.host);
    if let Some(c) = rq.content_type {
        head.push_str(&format!("Content-Type: {c}\r\n"));
    }
    if !rq.body.is_empty() || rq.doc.is_some() || rq.method == "POST" || rq.method == "PUT" {
        head.push_str(&format!("Content-Length: {total}\r\n"));
    }
    if let Some(h) = rq.headers {
        head.push_str(h);
    }
    head.push_str("\r\n");
    if head.len() >= 2048 {
        return Err(HttpError);
    }
    send_all(fd, head.as_bytes(), deadline)?;
    send_all(fd, rq.body, deadline)?;
    if let Some((doc, len)) = &rq.doc {
        let mut buf = vec![0u8; 1 << 15];
        let mut off = 0usize;
        while off < *len {
            // SAFETY: the buffer is ours.
            let k = unsafe { libc::pread(doc.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len().min(*len - off), off as libc::off_t) };
            if k <= 0 {
                return Err(HttpError);
            }
            send_all(fd, &buf[..k as usize], deadline + Duration::from_secs(600))?; // big jobs take long
            off += k as usize;
        }
    }
    let mut rd = Rd { fd, buf: Box::new([0; 16384]), a: 0, b: 0, deadline: deadline + if rq.doc.is_some() { Duration::from_secs(600) } else { Duration::ZERO } };
    let mut r = Response::default();
    // 100 Continue and other 1xx replies are skipped
    loop {
        let line = rd.line(1024)?;
        if !line.starts_with(b"HTTP/1.") || line.len() < 12 {
            return Err(HttpError);
        }
        r.status = atoi(&line[9..]);
        if !(100..200).contains(&r.status) {
            break;
        }
        while matches!(rd.line(1024), Ok(l) if !l.is_empty()) {}
    }
    let (mut clen, mut chunked, mut nheaders) = (-1i64, false, 0);
    loop {
        let line = rd.line(1024)?;
        if line.is_empty() {
            break;
        }
        nheaders += 1;
        if nheaders > 100 {
            return Err(HttpError);
        }
        let Some(colon) = line.iter().position(|&c| c == b':') else { continue };
        let name = &line[..colon];
        let mut v = &line[colon + 1..];
        while let [b' ' | b'\t', rest @ ..] = v {
            v = rest;
        }
        if name.eq_ignore_ascii_case(b"Content-Length") {
            clen = crate::strtol(v);
        } else if name.eq_ignore_ascii_case(b"Transfer-Encoding") && contains_ci(v, b"chunked") {
            chunked = true;
        } else if name.eq_ignore_ascii_case(b"Location") {
            r.location = keep(v, 512);
        } else if name.eq_ignore_ascii_case(b"WWW-Authenticate") {
            r.www_auth = keep(v, 256);
        } else if name.eq_ignore_ascii_case(b"Content-Type") {
            r.content_type = keep(v, 128);
        } else if name.eq_ignore_ascii_case(b"Retry-After") {
            r.retry_after = atoi(v);
        }
    }
    let max = rq.max;
    if chunked {
        loop {
            let line = rd.line(1024)?;
            let n = hex_prefix(&line).ok_or(HttpError)?;
            if n > max as u64 {
                return Err(HttpError);
            }
            if n == 0 {
                while matches!(rd.line(1024), Ok(l) if !l.is_empty()) {}
                break;
            }
            rd.bytes(n as usize, &mut r.body, max)?;
            rd.line(1024)?; // the CRLF after the chunk
        }
    } else if clen >= 0 {
        if clen as u64 > max as u64 {
            return Err(HttpError);
        }
        rd.bytes(clen as usize, &mut r.body, max)?;
    } else if r.status != 204 && r.status != 304 {
        // until it closes
        while rd.fill().is_ok() {
            let k = rd.b - rd.a;
            if r.body.len() + k > max {
                return Err(HttpError);
            }
            r.body.extend_from_slice(&rd.buf[rd.a..rd.b]);
            rd.a = rd.b;
        }
    }
    Ok(r)
}

/// The hexadecimal number a chunk-size line starts with (an extension after it is ignored).
fn hex_prefix(line: &[u8]) -> Option<u64> {
    let mut i = 0;
    while i < line.len() && matches!(line[i], b' ' | b'\t') {
        i += 1;
    }
    let start = i;
    let mut v: u64 = 0;
    while i < line.len() && line[i].is_ascii_hexdigit() {
        v = v.checked_mul(16)?.checked_add((line[i] as char).to_digit(16)? as u64)?;
        i += 1;
    }
    (i > start).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_local_network_is_lan() {
        for ip in ["10.0.0.1", "172.16.0.1", "172.31.255.255", "192.168.1.20", "169.254.3.4", "127.0.0.1"] {
            assert!(lan_ipv4(ip), "{ip}");
        }
        for ip in ["8.8.8.8", "172.32.0.1", "172.15.0.1", "192.169.0.1", "::1", "", "10.0.0", "300.1.1.1", "192.168.1.1 "] {
            assert!(!lan_ipv4(ip), "{ip}");
        }
    }

    #[test]
    fn chunk_sizes_are_hexadecimal() {
        assert_eq!(hex_prefix(b"1f"), Some(31));
        assert_eq!(hex_prefix(b"0"), Some(0));
        assert_eq!(hex_prefix(b"A;ext=1"), Some(10));
        assert_eq!(hex_prefix(b"zz"), None);
        assert_eq!(hex_prefix(b""), None);
        assert_eq!(hex_prefix(b"ffffffffffffffffff"), None);
    }

    #[test]
    fn tcp_refuses_bad_addresses_and_ports() {
        assert!(tcp("not an ip", 631, 100).is_err());
        assert!(tcp("127.0.0.1", 0, 100).is_err());
        assert!(tcp("127.0.0.1", 70000, 100).is_err());
    }
}
