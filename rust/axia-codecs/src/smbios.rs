//! SMBIOS type 17 (Memory Device) reader for System Information. Offsets from the
//! SMBIOS specification (DSP0134): 0x0C size, 0x10 device locator, 0x11 bank locator,
//! 0x12 memory type, 0x15 speed (2.3+), 0x17 manufacturer (2.3+), 0x1C extended size
//! (2.7+), 0x20 configured speed (2.7+), 0x54/0x58 extended speeds (3.3+). The bytes
//! are untrusted: every offset and string index is checked. C ABI of
//! server/src/sysreport/smbios.h.
use crate::smart::put_str;
use std::ffi::{c_char, c_int, c_uint};

#[repr(C)]
pub struct SmMem {
    slot: [u8; 32],
    bank: [u8; 32],
    size_mb: c_uint,
    ty: [u8; 12],
    speed: c_uint,
    configured: c_uint,
    maker: [u8; 32],
}

fn u16le(p: &[u8]) -> u32 {
    p[0] as u32 | (p[1] as u32) << 8
}
fn u32le(p: &[u8]) -> u32 {
    u16le(p) | u16le(&p[2..]) << 16
}

const JUNK: [&[u8]; 10] = [
    b"Unknown",
    b"UNKNOWN",
    b"Not Specified",
    b"NO DIMM",
    b"None",
    b"Undefined",
    b"To Be Filled By O.E.M.",
    b"Manufacturer00",
    b"0000",
    b"00000000",
];

/// String number idx (1-based) of the set `strs` (which ends before its double NUL),
/// written into `out` like the C did: printable ASCII with '|' replaced, cut to fit,
/// then trimmed and placeholders emptied by moving the NUL (the copied bytes stay).
fn str_at(strs: &[u8], idx: u8, out: &mut [u8]) {
    out[0] = 0;
    if idx == 0 {
        return;
    }
    let mut s = 0;
    let mut i = 1;
    while s < strs.len() && strs[s] != 0 {
        let Some(e) = strs[s..].iter().position(|&c| c == 0).map(|k| s + k) else { return };
        if i == idx as usize {
            while s < e && strs[s] == b' ' {
                s += 1;
            }
            let mut o = 0;
            while s < e && o + 1 < out.len() {
                let c = strs[s];
                out[o] = if (0x20..0x7f).contains(&c) && c != b'|' { c } else { b'?' };
                o += 1;
                s += 1;
            }
            while o > 0 && out[o - 1] == b' ' {
                o -= 1;
            }
            out[o] = 0;
            if JUNK.contains(&&out[..o]) {
                out[0] = 0;
            }
            return;
        }
        s = e + 1;
        i += 1;
    }
}

fn mem_type(t: u8) -> &'static [u8] {
    match t {
        0x07 => b"RAM",
        0x0F => b"SDRAM",
        0x12 => b"DDR",
        0x13 => b"DDR2",
        0x18 => b"DDR3",
        0x1A => b"DDR4",
        0x1B => b"LPDDR",
        0x1C => b"LPDDR2",
        0x1D => b"LPDDR3",
        0x1E => b"LPDDR4",
        0x22 => b"DDR5",
        0x23 => b"LPDDR5",
        0x00..=0x02 => b"",
        _ => b"other",
    }
}

fn set(out: &mut [u8], s: &[u8]) {
    let k = s.len().min(out.len() - 1);
    out[..k].copy_from_slice(&s[..k]);
    out[k] = 0;
}

fn cs(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

pub fn parse(raw: &[u8], m: &mut SmMem) -> bool {
    let len = raw.len();
    if len < 0x15 || raw[0] != 17 {
        return false;
    }
    let fl = raw[1] as usize; // the formatted area's length
    if fl < 0x15 || fl > len {
        return false;
    }
    // the string set must end with a double NUL inside the buffer
    let Some(end) = (fl..len.saturating_sub(1)).find(|&p| raw[p] == 0 && raw[p + 1] == 0) else { return false };
    let strs = &raw[fl..end + 1];
    let size = u16le(&raw[0x0C..]);
    m.size_mb = if size == 0xFFFF {
        0 // unknown: treated as empty
    } else if size == 0x7FFF && fl >= 0x20 {
        u32le(&raw[0x1C..]) & 0x7FFF_FFFF
    } else if size & 0x8000 != 0 {
        (size & 0x7FFF) / 1024 // in KB
    } else {
        size
    };
    str_at(strs, raw[0x10], &mut m.slot);
    str_at(strs, raw[0x11], &mut m.bank);
    set(&mut m.ty, mem_type(raw[0x12]));
    if fl >= 0x17 {
        m.speed = u16le(&raw[0x15..]);
        if m.speed == 0xFFFF && fl >= 0x58 {
            m.speed = u32le(&raw[0x54..]);
        }
    }
    if fl >= 0x18 {
        str_at(strs, raw[0x17], &mut m.maker);
    }
    if fl >= 0x22 {
        m.configured = u16le(&raw[0x20..]);
        if m.configured == 0xFFFF && fl >= 0x5C {
            m.configured = u32le(&raw[0x58..]);
        }
    }
    if m.speed > 100_000 {
        m.speed = 0;
    }
    if m.configured > 100_000 {
        m.configured = 0;
    }
    true
}

/// # Safety
/// `raw` covers `len` bytes (or is NULL) and `m` points to an SmMem.
#[no_mangle]
pub unsafe extern "C" fn smbios_memory_parse(raw: *const u8, len: usize, m: *mut SmMem) -> c_int {
    std::ptr::write_bytes(m, 0, 1);
    if raw.is_null() {
        return -1;
    }
    if parse(std::slice::from_raw_parts(raw, len), &mut *m) { 0 } else { -1 }
}

/// "mem <size_mb>|<type>|<speed>|<configured>|<slot>|<bank>|<maker>".
///
/// # Safety
/// `m` is a valid SmMem and `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn smbios_memory_line(m: *const SmMem, out: *mut c_char, n: usize) {
    let m = &*m;
    let mut line = format!("mem {}|", m.size_mb).into_bytes();
    line.extend_from_slice(cs(&m.ty));
    line.extend_from_slice(format!("|{}|{}|", m.speed, m.configured).as_bytes());
    for (i, f) in [&m.slot[..], &m.bank[..], &m.maker[..]].into_iter().enumerate() {
        if i > 0 {
            line.push(b'|');
        }
        line.extend_from_slice(cs(f));
    }
    put_str(out, n, &line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ddr4_module() {
        let mut raw = vec![0u8; 0x28];
        raw[0] = 17;
        raw[1] = 0x28;
        raw[0x0C] = 0x00;
        raw[0x0D] = 0x20; // 8192 MB
        raw[0x10] = 1;
        raw[0x11] = 2;
        raw[0x12] = 0x1A;
        raw[0x15] = 0x80;
        raw[0x16] = 0x0C; // 3200
        raw[0x17] = 3;
        raw[0x20] = 0x60;
        raw[0x21] = 0x09; // 2400
        raw.extend_from_slice(b"  DIMM|A1 \0BANK 0\0Not Specified\0\0");
        let mut m: SmMem = unsafe { std::mem::zeroed() };
        assert!(parse(&raw, &mut m));
        assert_eq!((m.size_mb, cs(&m.ty), m.speed, m.configured), (8192, &b"DDR4"[..], 3200, 2400));
        assert_eq!((cs(&m.slot), cs(&m.bank), cs(&m.maker)), (&b"DIMM?A1"[..], &b"BANK 0"[..], &b""[..]));
        let n = raw.len();
        assert!(!parse(&raw[..n - 1], &mut unsafe { std::mem::zeroed() }));
    }
}
