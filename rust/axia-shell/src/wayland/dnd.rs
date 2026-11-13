//! Owned, bounded URI payloads and readiness-driven streams. No worker threads.
use std::{
    ffi::{CStr, CString},
    os::fd::{AsRawFd, OwnedFd},
    path::{Component, Path},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
const LIMIT: usize = 1024 * 1024;
const STEP: usize = 64 * 1024;
static STREAMS: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Permit {
    fn new() -> Option<Self> {
        let mut n = STREAMS.load(Ordering::Relaxed);
        loop {
            if n >= 16 {
                return None;
            }
            match STREAMS.compare_exchange_weak(n, n + 1, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return Some(Self),
                Err(actual) => n = actual,
            }
        }
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        STREAMS.fetch_sub(1, Ordering::Relaxed);
    }
}
pub(super) struct Payload(Arc<[u8]>);
pub(super) struct Reader {
    fd: OwnedFd,
    data: Vec<u8>,
    result: Option<CString>,
    progress: Instant,
    _permit: Permit,
}
pub(super) struct Writer {
    fd: OwnedFd,
    data: Arc<[u8]>,
    offset: usize,
    progress: Instant,
    _permit: Permit,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Pending,
    Complete,
    Failed,
}
fn valid(path: &str) -> bool {
    !path.contains('\0')
        && Path::new(path).is_absolute()
        && !Path::new(path)
            .components()
            .any(|c| c == Component::ParentDir)
}
fn encode(paths: &[String]) -> Option<Vec<u8>> {
    if paths.is_empty() || paths.len() > 4096 {
        return None;
    }
    let mut data = Vec::new();
    for path in paths {
        if !valid(path) {
            return None;
        }
        data.extend_from_slice(b"file://");
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                data.push(byte);
            } else {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                data.extend_from_slice(&[
                    b'%',
                    HEX[(byte >> 4) as usize],
                    HEX[(byte & 15) as usize],
                ]);
            }
            if data.len() > LIMIT - 2 {
                return None;
            }
        }
        data.extend_from_slice(b"\r\n");
    }
    Some(data)
}
fn decode(data: &[u8]) -> Option<Vec<String>> {
    if data.len() > LIMIT {
        return None;
    }
    let text = std::str::from_utf8(data).ok()?;
    let mut paths = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
    {
        let uri = line.strip_prefix("file://")?;
        let raw = if uri.starts_with('/') {
            uri
        } else {
            uri.strip_prefix("localhost")
                .filter(|p| p.starts_with('/'))?
        };
        if raw.contains(['?', '#']) {
            return None;
        }
        let mut bytes = Vec::with_capacity(raw.len());
        let mut chars = raw.bytes();
        while let Some(byte) = chars.next() {
            bytes.push(if byte == b'%' {
                (chars.next()? as char).to_digit(16)? as u8 * 16
                    + (chars.next()? as char).to_digit(16)? as u8
            } else {
                byte
            });
        }
        let path = String::from_utf8(bytes).ok()?;
        if !valid(&path) || paths.len() >= 4096 {
            return None;
        }
        paths.push(path);
    }
    (!paths.is_empty()).then_some(paths)
}
fn nonblocking(fd: OwnedFd) -> Option<OwnedFd> {
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return None;
    }
    Some(fd)
}
impl Payload {
    pub fn new(json: &CStr) -> Option<Self> {
        let bytes = json.to_bytes();
        if bytes.len() > LIMIT {
            return None;
        }
        let paths: Vec<String> = serde_json::from_slice(bytes).ok()?;
        Some(Self(encode(&paths)?.into()))
    }
    pub fn json(&self) -> Option<CString> {
        CString::new(serde_json::to_vec(&decode(&self.0)?).ok()?).ok()
    }
}
impl Reader {
    pub fn new(fd: OwnedFd) -> Option<Self> {
        Some(Self {
            fd: nonblocking(fd)?,
            data: Vec::new(),
            result: None,
            progress: Instant::now(),
            _permit: Permit::new()?,
        })
    }
    pub fn poll(&mut self) -> Status {
        let mut buffer = [0u8; 8192];
        let mut work = 0;
        while work < STEP {
            let n = unsafe {
                libc::read(
                    self.fd.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if n > 0 {
                let n = n as usize;
                if self.data.len() + n > LIMIT {
                    return Status::Failed;
                }
                self.data.extend_from_slice(&buffer[..n]);
                self.progress = Instant::now();
                work += n;
            } else if n == 0 {
                let Some(paths) = decode(&self.data) else {
                    return Status::Failed;
                };
                self.result = serde_json::to_vec(&paths)
                    .ok()
                    .and_then(|v| CString::new(v).ok());
                return if self.result.is_some() {
                    Status::Complete
                } else {
                    Status::Failed
                };
            } else {
                match std::io::Error::last_os_error().raw_os_error() {
                    Some(libc::EINTR) => continue,
                    Some(libc::EAGAIN) => return Status::Pending,
                    _ => return Status::Failed,
                }
            }
        }
        Status::Pending
    }
    pub fn json(&self) -> Option<&CStr> {
        self.result.as_deref()
    }
    pub fn expired(&self) -> bool {
        self.progress.elapsed() >= Duration::from_secs(15)
    }
}
impl Writer {
    pub fn new(fd: OwnedFd, payload: &Payload) -> Option<Self> {
        Some(Self {
            fd: nonblocking(fd)?,
            data: payload.0.clone(),
            offset: 0,
            progress: Instant::now(),
            _permit: Permit::new()?,
        })
    }
    pub fn poll(&mut self) -> Status {
        let end = (self.offset + STEP).min(self.data.len());
        while self.offset < end {
            let n = unsafe {
                libc::write(
                    self.fd.as_raw_fd(),
                    self.data[self.offset..end].as_ptr().cast(),
                    end - self.offset,
                )
            };
            if n > 0 {
                self.offset += n as usize;
                self.progress = Instant::now();
            } else {
                match std::io::Error::last_os_error().raw_os_error() {
                    Some(libc::EINTR) => continue,
                    Some(libc::EAGAIN) => return Status::Pending,
                    _ => return Status::Failed,
                }
            }
        }
        if self.offset == self.data.len() {
            Status::Complete
        } else {
            Status::Pending
        }
    }
    pub fn expired(&self) -> bool {
        self.progress.elapsed() >= Duration::from_secs(15)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    #[test]
    fn uris_round_trip_spaces_unicode_and_reserved_characters_without_io() {
        let paths = vec!["/home/user/Um vídeo #1%.mp4".into(), "/tmp/a\nb".into()];
        let encoded = encode(&paths).unwrap();
        assert!(encoded.windows(3).any(|w| w == b"%23"));
        assert_eq!(decode(&encoded).unwrap(), paths);
        assert_eq!(
            decode(b"# comment\r\nfile://localhost/tmp/a%20b\r\n").unwrap(),
            vec!["/tmp/a b"]
        );
    }
    #[test]
    fn malformed_remote_and_oversized_payloads_are_rejected() {
        for data in [
            "file://server/tmp/a",
            "https://server/a",
            "file:///tmp/%00",
            "file:///tmp/%XX",
            "file:///tmp/a%",
            "file:///tmp/../etc/passwd",
            "file:///tmp/a#fragment",
            "file://localhostevil/tmp/a",
        ] {
            assert!(decode(data.as_bytes()).is_none(), "{data}");
        }
        assert!(decode(&vec![b'a'; LIMIT + 1]).is_none());
        assert!(encode(&vec!["/tmp/a".into(); 4097]).is_none());
    }
    #[test]
    fn pipes_transfer_asynchronously_without_changing_the_clipboard() {
        let (a, b) = UnixStream::pair().unwrap();
        let payload = Payload::new(c"[\"/tmp/file.mp4\"]").unwrap();
        let mut reader = Reader::new(a.into()).unwrap();
        let mut writer = Writer::new(b.into(), &payload).unwrap();
        assert_eq!(reader.poll(), Status::Pending);
        assert_eq!(writer.poll(), Status::Complete);
        drop(writer);
        assert_eq!(reader.poll(), Status::Complete);
        assert_eq!(reader.json().unwrap(), c"[\"/tmp/file.mp4\"]");
    }
    #[test]
    fn backpressure_keeps_each_callback_bounded_and_source_can_be_dropped() {
        let (a, b) = UnixStream::pair().unwrap();
        let size = 4096i32;
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    b.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&size as *const i32).cast(),
                    size_of::<i32>() as _,
                )
            },
            0
        );
        let paths = vec![format!("/tmp/{}", "a".repeat(160)); 2048];
        let payload = Payload(encode(&paths).unwrap().into());
        let mut reader = Reader::new(a.into()).unwrap();
        let mut writer = Writer::new(b.into(), &payload).unwrap();
        drop(payload);
        assert_eq!(writer.poll(), Status::Pending);
        let mut writer = Some(writer);
        let mut complete = false;
        for _ in 0..2000 {
            if let Some(w) = &mut writer {
                let before = w.offset;
                let status = w.poll();
                assert!(w.offset - before <= STEP);
                assert_ne!(status, Status::Failed);
                if status == Status::Complete {
                    writer = None;
                }
            }
            let before = reader.data.len();
            let status = reader.poll();
            assert!(reader.data.len() - before <= STEP);
            assert_ne!(status, Status::Failed);
            if status == Status::Complete {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(reader.json().unwrap().to_bytes()).unwrap(),
            paths
        );
    }
}
