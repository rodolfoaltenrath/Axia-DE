//! Settings' hardware readers (docs/SETTINGS.md), all read-only or closed to one preference and none
//! of them run with privileges:
//!
//!  - `power`: batteries and chargers from /sys/class/power_supply (Settings → Battery, the menu bar
//!    percentage and the low-battery notice);
//!  - `uevent`: kernel uevents (DRM connectors, power supplies, backlight) for the main loop's poll;
//!  - `hwscan`: the Store's "Suggested for your computer" facts (DMI, device tree, PCI, USB, virtio
//!    ports, the firmware a driver asked for and did not find);
//!  - `brframe`: the browsers' own "tabs in the title bar" preference in the user's profiles.
//!
//! `axia-server` reaches them through the C ABI of `server/src/hw/{power,uevent,hwscan,brframe}.h`
//! (`ffi`). Paths take a `root` ("" for the real system) so the tests feed fake trees.
pub mod brframe;
mod ffi;
pub mod hwscan;
pub mod power;
pub mod smart;
pub mod uevent;

/// `"text"` with `"` and `\` escaped and control characters as `\u00xx` (what `json_quote` does).
pub(crate) fn quote(s: &[u8], out: &mut Vec<u8>) {
    out.push(b'"');
    for &c in s {
        match c {
            b'"' | b'\\' => {
                out.push(b'\\');
                out.push(c);
            }
            c if c < 0x20 => out.extend_from_slice(format!("\\u{c:04x}").as_bytes()),
            c => out.push(c),
        }
    }
    out.push(b'"');
}

/// The bytes up to the first NUL (a C string in a fixed buffer).
pub(crate) fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// Copies `s` into a fixed C buffer as `snprintf("%s")` does: at most `buf.len() - 1` bytes and a NUL.
pub(crate) fn put_c(buf: &mut [u8], s: &[u8]) {
    let n = s.len().min(buf.len().saturating_sub(1));
    buf[..n].copy_from_slice(&s[..n]);
    if !buf.is_empty() {
        buf[n] = 0;
    }
    for b in buf.iter_mut().skip(n + 1) {
        *b = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_and_c_buffers() {
        let mut o = Vec::new();
        quote(b"a\"b\\c\n\xc3\xa9", &mut o);
        assert_eq!(o, b"\"a\\\"b\\\\c\\u000a\xc3\xa9\"");
        let mut b = [9u8; 6];
        put_c(&mut b, b"abcdefgh");
        assert_eq!(&b, b"abcde\0");
        put_c(&mut b, b"ab");
        assert_eq!(&b, b"ab\0\0\0\0");
        assert_eq!(cstr(b"ab\0cd"), b"ab");
    }
}
