//! The HTTP client against a fake server: the shapes of a reply it has to read (Content-Length,
//! chunked, until the connection closes, 100 Continue) and the ones it has to refuse.
mod support;

use axia_print::http::{self, HttpError, Request};
use std::time::{Duration, Instant};
use support::{serve_tcp, Rep};

fn get(port: u16, path: &str, max: usize, timeout_ms: u64) -> Result<http::Response, HttpError> {
    let fd = http::tcp("127.0.0.1", port as i32, 2000).expect("connects");
    http::request(&fd, &Request { method: "GET", host: "127.0.0.1", path, headers: Some("X-Test: 1\r\n"), content_type: None, body: &[], doc: None, max, timeout_ms })
}

fn page(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 200) as u8 + 32).collect()
}

#[test]
fn it_reads_every_way_a_body_can_come() {
    let body = page(5000);
    let b = body.clone();
    let port = serve_tcp(move |r| {
        assert_eq!((r.method.as_str(), r.header("Host"), r.header("X-Test"), r.header("Connection")), ("GET", Some("127.0.0.1"), Some("1"), Some("close")));
        assert!(r.header("Content-Length").is_none(), "a GET has no body");
        match r.path.as_str() {
            "/len" => Rep::ok(b.clone()).with("Content-Type", "image/png").with("Location", "/elsewhere").with("Retry-After", "7").with("WWW-Authenticate", "Basic"),
            "/chunked" => Rep { chunked: true, ..Rep::ok(b.clone()) },
            "/close" => Rep { until_close: true, ..Rep::ok(b.clone()) },
            "/continue" => Rep { continue_first: true, ..Rep::ok(b.clone()) },
            "/empty204" => Rep::status(204),
            "/404" => Rep { status: 404, body: b"nope".to_vec(), ..Rep::default() },
            _ => unreachable!(),
        }
    });
    for path in ["/len", "/chunked", "/close", "/continue"] {
        let r = get(port, path, 1 << 20, 3000).unwrap_or_else(|_| panic!("{path}"));
        assert_eq!((r.status, r.body == body), (200, true), "{path}");
    }
    let r = get(port, "/len", 1 << 20, 3000).unwrap();
    assert_eq!((r.content_type.as_str(), r.location.as_str(), r.retry_after, r.www_auth.as_str()), ("image/png", "/elsewhere", 7, "Basic"));
    let r = get(port, "/empty204", 100, 3000).unwrap();
    assert_eq!((r.status, r.body.len()), (204, 0));
    let r = get(port, "/404", 100, 3000).unwrap();
    assert_eq!((r.status, r.body.as_slice()), (404, &b"nope"[..]));
}

#[test]
fn a_reply_over_the_cap_or_that_is_not_http_is_refused() {
    let port = serve_tcp(|r| match r.path.as_str() {
        "/big" => Rep::ok(page(5000)),
        "/bigchunks" => Rep { chunked: true, ..Rep::ok(page(5000)) },
        "/bigclose" => Rep { until_close: true, ..Rep::ok(page(5000)) },
        "/hangup" => Rep { hang_up: true, ..Rep::default() },
        _ => unreachable!(),
    });
    for p in ["/big", "/bigchunks", "/bigclose"] {
        assert_eq!(get(port, p, 4999, 3000).err(), Some(HttpError), "{p}");
        assert!(get(port, p, 5000, 3000).is_ok(), "{p} at the cap");
    }
    assert!(get(port, "/hangup", 100, 3000).is_err());
    // something that is not HTTP at all
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for s in l.incoming().flatten() {
            let mut s = s;
            let mut b = [0u8; 1024];
            let _ = s.read(&mut b);
            let _ = s.write_all(b"SSH-2.0-OpenSSH\r\n\r\n");
        }
    });
    assert!(get(port, "/", 100, 3000).is_err());
}

#[test]
fn a_server_that_never_answers_ends_at_the_deadline() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut keep = Vec::new();
        for s in l.incoming().flatten() {
            keep.push(s); // accepted and left in silence
        }
    });
    let t = Instant::now();
    assert!(get(port, "/", 100, 400).is_err());
    assert!(t.elapsed() >= Duration::from_millis(350) && t.elapsed() < Duration::from_millis(3000), "{:?}", t.elapsed());
}

#[test]
fn a_connection_to_nothing_fails_quickly() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    assert!(http::tcp("127.0.0.1", port as i32, 2000).is_err());
    assert!(http::unix("/nonexistent/sock").is_err());
}

#[test]
fn a_post_sends_its_length_and_its_body() {
    let port = serve_tcp(|r| {
        assert_eq!((r.method.as_str(), r.header("Content-Length"), r.header("Content-Type")), ("POST", Some("5"), Some("text/xml")));
        assert_eq!(r.body, b"hello");
        Rep::ok(r.body.iter().rev().copied().collect())
    });
    let fd = http::tcp("127.0.0.1", port as i32, 2000).unwrap();
    let r = http::request(&fd, &Request { method: "POST", host: "x", path: "/p", headers: None, content_type: Some("text/xml"), body: b"hello", doc: None, max: 100, timeout_ms: 3000 }).unwrap();
    assert_eq!(r.body, b"olleh");
}
