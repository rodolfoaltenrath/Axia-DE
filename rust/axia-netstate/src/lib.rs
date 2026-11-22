//! The network state `lord.net` shows (docs/NETWORK.md), read without root: interfaces, addresses,
//! routes and counters from rtnetlink dumps, the DNS in use, the configurations LordOS saved, and
//! the notifications that tell JS something changed. Our own netlink, no libnl or libmnl.
//!
//! `axia-server` reaches it through the C ABI of `server/src/net/net.h` (`ffi`). The JSON is
//! built by hand, in the same order and with the same escapes as the C code it replaces, so what JS
//! parses does not change.
pub mod config;
mod ffi;
pub mod netlink;
pub mod state;

/// A JSON text under construction (bytes: names from the kernel and files are not always UTF-8).
#[derive(Default)]
pub(crate) struct Out(pub Vec<u8>);

impl Out {
    pub fn raw(&mut self, s: &str) {
        self.0.extend_from_slice(s.as_bytes());
    }
    /// `"text"` with `"` `\` and control characters escaped (other bytes as they are).
    pub fn string(&mut self, s: &[u8]) {
        self.0.push(b'"');
        for &c in s {
            match c {
                b'"' | b'\\' => {
                    self.0.push(b'\\');
                    self.0.push(c);
                }
                c if c < 0x20 => self.raw(&format!("\\u{c:04x}")),
                c => self.0.push(c),
            }
        }
        self.0.push(b'"');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_escaped_like_the_c_code_did() {
        let mut o = Out::default();
        o.string(b"a\"b\\c\n\x01\x7f\xc3\xa9");
        assert_eq!(o.0, b"\"a\\\"b\\\\c\\u000a\\u0001\x7f\xc3\xa9\"");
    }
}
