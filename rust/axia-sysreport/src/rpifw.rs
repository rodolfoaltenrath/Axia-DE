//! What the Raspberry Pi firmware says about power and throttling, as the kernel shows it to an
//! unprivileged process (docs/BENCHMARK.md, "The Raspberry Pi firmware's throttling"). World-readable
//! sysfs only: no mailbox, no vcgencmd, no exec. Every reader takes a root folder ("" for the real
//! one) so tests can feed fake trees. Two sources, either, both or none:
//!  - the hwmon "rpi_volt", the only one the LordOS rpi-6.18.y kernel has: it asks GET_THROTTLED every
//!    2 s with the sticky bits cleared each time and exposes in0_lcrit_alarm, the "under-voltage has
//!    occurred" bit of the last answer: "under-voltage in the last 2 s window";
//!  - the firmware node's get_throttled of older Raspberry Pi kernels (the whole GET_THROTTLED word).
use std::ffi::{c_char, c_int, CStr};
use std::fs;
use std::io::Read;

/// GET_THROTTLED: bits 0-3 now (under-voltage, frequency capped, throttled, soft temperature limit),
/// bits 16-19 the same four "has occurred".
pub const NOW_MASK: i32 = 0x0000f;
pub const STICKY_MASK: i32 = 0xf0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct RpiFw {
    /// 1 if the rpi_volt hwmon exists (even if its alarm cannot be read).
    pub hwmon: c_int,
    /// Its in0_lcrit_alarm: 1 under-voltage in the last report, 0 none, -1 unknown.
    pub uv_alarm: c_int,
    /// get_throttled masked to the eight known bits, -1 no such file or not a strict number.
    pub throttled: c_int,
}

/// Is this device tree "compatible" list (NUL-separated strings) a BCM2712 board (Pi 5, Pi 500, CM5)?
pub fn is_bcm2712(compat: &[u8]) -> bool {
    compat.split(|&c| c == 0).any(|s| s == b"brcm,bcm2712")
}

/// get_throttled's text, strictly: 1-8 hex digits (an optional 0x), an optional newline, nothing
/// else; the value masked to the eight known bits, or -1.
pub fn parse_throttled(s: &[u8]) -> i32 {
    let s = s.strip_prefix(b"0x").or_else(|| s.strip_prefix(b"0X")).unwrap_or(s);
    let (digits, rest) = match s.iter().position(|&c| c == b'\n') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, &s[s.len()..]),
    };
    if digits.is_empty() || digits.len() > 8 || !rest.is_empty() {
        return -1;
    }
    let mut v: u64 = 0;
    for &c in digits {
        let Some(d) = (c as char).to_digit(16) else { return -1 };
        v = v << 4 | d as u64;
    }
    (v & (NOW_MASK | STICKY_MASK) as u64) as i32
}

/// A small sysfs file as is (trailing newline kept for the strict parser).
fn rd(root: &str, path: &str, n: usize) -> Option<Vec<u8>> {
    let f = fs::File::open(format!("{root}{path}")).ok()?;
    let mut b = Vec::new();
    f.take(n as u64 - 1).read_to_end(&mut b).ok()?;
    if let Some(z) = b.iter().position(|&c| c == 0) {
        b.truncate(z);
    }
    Some(b)
}

fn names(dir: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).map(|d| d.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// The rpi_volt hwmon's under-voltage alarm: exactly "0" or "1" (an optional newline), anything else
/// unknown.
fn volt(root: &str, f: &mut RpiFw) {
    for n in names(&format!("{root}/sys/class/hwmon")) {
        if !n.starts_with("hwmon") || n.len() > 10 {
            continue;
        }
        let name = rd(root, &format!("/sys/class/hwmon/{n}/name"), 32);
        if !matches!(name.as_deref(), Some(b"rpi_volt\n" | b"rpi_volt")) {
            continue;
        }
        f.hwmon = 1;
        if let Some(v) = rd(root, &format!("/sys/class/hwmon/{n}/in0_lcrit_alarm"), 8) {
            if matches!(v.as_slice(), [b'0' | b'1'] | [b'0' | b'1', b'\n']) {
                f.uv_alarm = (v[0] - b'0') as c_int;
            }
        }
        return;
    }
}

/// get_throttled (older Raspberry Pi kernels): directly under a platform node or in its
/// "<node>:firmware" child.
fn throttled(root: &str, f: &mut RpiFw) {
    for n in names(&format!("{root}/sys/devices/platform")) {
        if n.starts_with('.') || n.len() > 64 {
            continue;
        }
        for p in [format!("/sys/devices/platform/{n}/get_throttled"), format!("/sys/devices/platform/{n}/{n}:firmware/get_throttled")] {
            if let Some(b) = rd(root, &p, 32) {
                f.throttled = parse_throttled(&b);
            }
            if f.throttled >= 0 {
                return;
            }
        }
    }
}

/// Both sources; never fails (unknown fields stay -1, hwmon 0).
pub fn read(root: &str) -> RpiFw {
    let mut f = RpiFw { hwmon: 0, uv_alarm: -1, throttled: -1 };
    volt(root, &mut f);
    throttled(root, &mut f);
    f
}

/// # Safety
/// `compat` covers `n` bytes (or is NULL).
#[no_mangle]
pub unsafe extern "C" fn hw_is_bcm2712(compat: *const c_char, n: usize) -> c_int {
    (!compat.is_null() && is_bcm2712(std::slice::from_raw_parts(compat.cast(), n))) as c_int
}

/// # Safety
/// `s` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn rpifw_parse_throttled(s: *const c_char) -> c_int {
    if s.is_null() { -1 } else { parse_throttled(CStr::from_ptr(s).to_bytes()) }
}

/// # Safety
/// `root` is a C string and `f` points to an RpiFw.
#[no_mangle]
pub unsafe extern "C" fn rpifw_read(root: *const c_char, f: *mut RpiFw) {
    let root = if root.is_null() { "" } else { CStr::from_ptr(root).to_str().unwrap_or("") };
    *f = read(root);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_throttled_is_read_strictly() {
        assert_eq!(parse_throttled(b"0x50005\n"), 0x50005);
        assert_eq!(parse_throttled(b"50005"), 0x50005);
        assert_eq!(parse_throttled(b"0xffffffff"), NOW_MASK | STICKY_MASK);
        assert_eq!(parse_throttled(b"0"), 0);
        for bad in ["", "\n", "0x", "123456789", "0x5\n\n", "0x5\nx", "zz", "5 ", " 5", "0x5 \n"] {
            assert_eq!(parse_throttled(bad.as_bytes()), -1, "{bad:?}");
        }
    }

    #[test]
    fn the_bcm2712_boards() {
        assert!(is_bcm2712(b"raspberrypi,5-model-b\0brcm,bcm2712\0"));
        assert!(!is_bcm2712(b"raspberrypi,4-model-b\0brcm,bcm2711\0"));
        assert!(!is_bcm2712(b"brcm,bcm27120"));
        assert!(!is_bcm2712(b""));
    }
}
