//! Pure helpers of the smart-* actions (C ABI of server/src/disk/disk.h, the former
//! smartctl.c; tests/smartctl_test.c): which device names smartctl may be pointed at,
//! and the compaction of its JSON so it travels in few LOG lines. No I/O.
use libc::{c_char, c_int};

fn digits(n: &[u8], i: &mut usize) -> usize {
    let a = *i;
    while *i < n.len() && n[*i].is_ascii_digit() {
        *i += 1;
    }
    *i - a
}

/// sd<letters> / hd<letters> (SATA, SAS, USB disks behind a bridge) and nvme<N>n<M> (an NVMe
/// namespace). Nothing else: no partitions (sda1, nvme0n1p1), loop, dm, md, zram, mmcblk (cards
/// have no SMART), sr (optical), and no path characters at all.
fn valid_disk(n: &[u8]) -> bool {
    let l = n.len();
    if !(3..=15).contains(&l) {
        return false;
    }
    if (n[0] == b's' || n[0] == b'h') && n[1] == b'd' {
        return l <= 5 && n[2..].iter().all(|c| c.is_ascii_lowercase());
    }
    if !n.starts_with(b"nvme") {
        return false;
    }
    let mut i = 4;
    let d = digits(n, &mut i);
    if d == 0 || d > 3 || i >= l || n[i] != b'n' {
        return false;
    }
    if d > 1 && n[4] == b'0' {
        return false; // no leading zeros
    }
    i += 1;
    let start = i;
    let d = digits(n, &mut i);
    if d == 0 || d > 3 || i != l {
        return false;
    }
    // namespaces start at 1, without leading zeros
    n[start] != b'0'
}

#[no_mangle]
pub unsafe extern "C" fn smartctl_valid_disk(name: *const c_char) -> c_int {
    if name.is_null() {
        return 0;
    }
    valid_disk(std::ffi::CStr::from_ptr(name).to_bytes()) as c_int
}

/// Removes the whitespace outside JSON strings, in place. Returns the new length. Raw control
/// characters (which valid JSON never has outside strings, and smartctl escapes inside them)
/// become spaces inside strings and are dropped outside.
fn compact(s: &mut [u8]) -> usize {
    let n = s.len();
    let (mut o, mut inside, mut esc) = (0, false, false);
    for i in 0..n {
        let mut c = s[i];
        if c == 0 {
            break;
        }
        if inside {
            if c < 0x20 {
                c = b' ';
            }
            s[o] = c;
            o += 1;
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                inside = false;
            }
            continue;
        }
        if c < 0x21 && (c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c < 0x20) {
            continue;
        }
        if c == b'"' {
            inside = true;
        }
        s[o] = c;
        o += 1;
    }
    if o < n {
        s[o] = 0;
    }
    o
}

#[no_mangle]
pub unsafe extern "C" fn smartctl_compact(s: *mut c_char, n: usize) -> usize {
    if s.is_null() || n == 0 {
        return 0;
    }
    compact(std::slice::from_raw_parts_mut(s.cast::<u8>(), n))
}

/// Cut point for a LOG line of at most max bytes from s[0..n): never inside a UTF-8 sequence.
fn chunk(s: &[u8], max: usize) -> usize {
    if s.len() <= max {
        return s.len();
    }
    let mut k = max;
    while k > 1 && s[k] & 0xc0 == 0x80 {
        k -= 1;
    }
    k
}

#[no_mangle]
pub unsafe extern "C" fn smartctl_chunk(s: *const c_char, n: usize, max: usize) -> usize {
    if n <= max {
        return n;
    }
    chunk(std::slice::from_raw_parts(s.cast::<u8>(), n), max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_names() {
        for g in ["sda", "sdb", "sdz", "sdaa", "sdzzz", "hda", "nvme0n1", "nvme1n1", "nvme12n3", "nvme0n128"] {
            assert!(valid_disk(g.as_bytes()), "accepts {g}");
        }
        for b in [
            "", "sd", "sda1", "sdA", "sdaaaa", "nvme0", "nvme0n0", "nvme0n1p1", "nvme00n1", "nvme0n01", "nvmen1", "nvme1234n1",
            "nvme0n1234", "loop0", "dm-0", "md0", "zram0", "mmcblk0", "mmcblk0p1", "sr0", "vda", "xvda", "../sda", "sda/", "/dev/sda",
            "sda ", " sda", "sd-a", "sda\n", "nvme0n1\n", "hd1", "sdá",
        ] {
            assert!(!valid_disk(b.as_bytes()), "refuses {b:?}");
        }
    }

    #[test]
    fn compaction_keeps_strings() {
        let mut j = b"{\n  \"model_name\": \"QEMU HARDDISK\",\n  \"x\": [\n    1,\n    2\n  ],\n\
                      \x20 \"s\": \"a \\\"b\\\" \\\\ c\"\t}\n"
            .to_vec();
        let n = compact(&mut j);
        assert_eq!(&j[..n], b"{\"model_name\":\"QEMU HARDDISK\",\"x\":[1,2],\"s\":\"a \\\"b\\\" \\\\ c\"}");
        let mut raw = b"{\"a\":\"x\ty\"}".to_vec(); // a raw tab inside a string becomes a space
        let n = compact(&mut raw);
        assert_eq!(&raw[..n], b"{\"a\":\"x y\"}");
        let mut nul = b"{ \"a\" : 1 }\0 ignored".to_vec(); // stops at a NUL
        let n = compact(&mut nul);
        assert_eq!(&nul[..n], b"{\"a\":1}");
    }

    #[test]
    fn chunks_never_split_utf8() {
        let u = "ab\u{e9}cd".as_bytes(); // "abécd"
        assert_eq!(chunk(u, 3), 2);
        assert_eq!(chunk(u, 4), 4);
        assert_eq!(chunk(u, 900), 6);
    }
}
