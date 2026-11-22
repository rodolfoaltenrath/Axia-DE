//! A fake HTTP server on a unix socket (CUPS) or on 127.0.0.1 (a scanner), for the tests of the
//! clients: it reads one request per connection, calls the handler and writes the reply.
#![allow(dead_code)]
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Req {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

#[derive(Default)]
pub struct Rep {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Send the body as chunks (with a trailer) instead of a Content-Length.
    pub chunked: bool,
    /// Send "100 Continue" first.
    pub continue_first: bool,
    /// Send no Content-Length and close when done.
    pub until_close: bool,
    /// Close the connection without answering.
    pub hang_up: bool,
}

impl Rep {
    pub fn ok(body: Vec<u8>) -> Rep {
        Rep { status: 200, body, ..Rep::default() }
    }
    pub fn status(status: u16) -> Rep {
        Rep { status, ..Rep::default() }
    }
    pub fn with(mut self, k: &str, v: &str) -> Rep {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }
}

pub fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axia-print-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn read_request(s: &mut (impl Read + Write)) -> Option<Req> {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    let end = loop {
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let n = s.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next()?;
    let mut parts = first.split(' ');
    let (method, path) = (parts.next()?.to_string(), parts.next()?.to_string());
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))).collect();
    let clen: usize = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let mut body = data[end..].to_vec();
    while body.len() < clen {
        let n = s.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&buf[..n]);
    }
    Some(Req { method, path, headers, body })
}

fn answer(s: &mut (impl Read + Write), r: Rep) {
    if r.hang_up {
        return;
    }
    if r.continue_first {
        let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    let mut head = format!("HTTP/1.1 {} X\r\nConnection: close\r\n", r.status);
    for (k, v) in &r.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if r.chunked {
        head.push_str("Transfer-Encoding: chunked\r\n\r\n");
        let _ = s.write_all(head.as_bytes());
        for c in r.body.chunks(1000) {
            let _ = s.write_all(format!("{:x};ext=1\r\n", c.len()).as_bytes());
            let _ = s.write_all(c);
            let _ = s.write_all(b"\r\n");
        }
        let _ = s.write_all(b"0\r\nX-Trailer: 1\r\n\r\n");
    } else if r.until_close {
        head.push_str("\r\n");
        let _ = s.write_all(head.as_bytes());
        let _ = s.write_all(&r.body);
    } else {
        head.push_str(&format!("Content-Length: {}\r\n\r\n", r.body.len()));
        let _ = s.write_all(head.as_bytes());
        let _ = s.write_all(&r.body);
    }
}

/// CUPS's socket: every connection is one request.
pub fn serve_unix(path: &Path, handler: impl Fn(&Req) -> Rep + Send + Sync + 'static) {
    let l = UnixListener::bind(path).unwrap();
    let h = Arc::new(handler);
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let h = h.clone();
            std::thread::spawn(move || {
                let mut s = s;
                if let Some(rq) = read_request(&mut s) {
                    answer(&mut s, h(&rq));
                }
            });
        }
    });
}

/// A scanner on 127.0.0.1: the port it listens on.
pub fn serve_tcp(handler: impl Fn(&Req) -> Rep + Send + Sync + 'static) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let h = Arc::new(handler);
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let h = h.clone();
            std::thread::spawn(move || {
                let mut s = s;
                if let Some(rq) = read_request(&mut s) {
                    answer(&mut s, h(&rq));
                }
            });
        }
    });
    port
}
