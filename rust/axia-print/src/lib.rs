//! Printers and scanners for `lord.print` (docs/PRINTING.md): our own IPP client for CUPS over
//! its local socket, our own eSCL client for the network's scanners, and the mDNS discovery of
//! both. No libcups, no SANE, no HTTP library: everything that arrives from the network is
//! untrusted and read with bounds.
//!
//! `axia-server` reaches it through the C ABI of `server/src/print/print.h` (`ffi`): each request
//! runs in its own thread and its result is read with `print_poll()`, so the compositor never
//! waits.
pub mod browse;
pub mod cups;
pub mod discover;
pub mod escl;
mod ffi;
pub mod http;
pub mod ipp;
pub mod net;
pub mod tasks;

use serde_json::{json, Value};

/// A request that failed: `{"error": code, "detail": detail}` is what JS receives.
#[derive(Debug, Clone, PartialEq)]
pub struct Fail(pub Value);

impl Fail {
    pub fn new(code: &str, detail: &str) -> Fail {
        Fail(json!({ "error": code, "detail": detail }))
    }
    /// The error code ("" if the payload has none).
    pub fn code(&self) -> &str {
        self.0["error"].as_str().unwrap_or("")
    }
    pub fn detail(&self) -> &str {
        self.0["detail"].as_str().unwrap_or("")
    }
}

/// The result of a request: its JSON or the failure.
pub type Reply = Result<Value, Fail>;

/// A text from the network, as JSON text (bytes that are not UTF-8 are replaced).
pub(crate) fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// strtol(s, NULL, 10): blanks, a sign, digits; 0 when there are none; saturating.
pub(crate) fn strtol(s: &[u8]) -> i64 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && matches!(s[i], b'-' | b'+') {
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.saturating_mul(10).saturating_add((s[i] - b'0') as i64);
        i += 1;
    }
    if neg { -v } else { v }
}

/// The text under `key` of a JSON object, if there is one.
pub(crate) fn jstr<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

/// The number under `key` of a JSON object (`def` if it is missing or is not a number).
pub(crate) fn jnum(v: &Value, key: &str, def: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(def)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtol_reads_what_c_reads() {
        assert_eq!(strtol(b"  42abc"), 42);
        assert_eq!(strtol(b"-7"), -7);
        assert_eq!(strtol(b"+3"), 3);
        assert_eq!(strtol(b"x"), 0);
        assert_eq!(strtol(b""), 0);
        assert_eq!(strtol(b"99999999999999999999999"), i64::MAX);
    }

    #[test]
    fn json_helpers_ignore_other_types() {
        let v: Value = serde_json::from_str(r#"{"a":"x","n":3,"s":"4"}"#).unwrap();
        assert_eq!((jstr(&v, "a"), jstr(&v, "n"), jstr(&v, "zz")), (Some("x"), None, None));
        assert_eq!((jnum(&v, "n", 9.0), jnum(&v, "s", 9.0), jnum(&v, "zz", 9.0)), (3.0, 9.0, 9.0));
        assert_eq!(jstr(&Value::Null, "a"), None);
        let f = Fail::new("http", "500");
        assert_eq!((f.code(), f.detail()), ("http", "500"));
    }
}
