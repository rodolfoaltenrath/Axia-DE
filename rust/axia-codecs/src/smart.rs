//! SMART parsers of the Disks app (pure functions, no I/O). C ABI of the SMART part
//! of server/src/disk/disk.h (tests/smart_test.c).
//!
//! ATA (ACS-3, "SMART READ DATA" and "READ THRESHOLDS", 512 bytes each): bytes 0-1
//! version, 2-361: 30 attributes of 12 bytes ([0] id, [1-2] flags (bit 0 = pre-fail),
//! [3] normalized value, [4] worst, [5-10] raw, 48 bits little endian); byte 511:
//! checksum (the 512 bytes sum to 0 modulo 256). Thresholds: [0] id, [1] threshold.
//! NVMe (1.4, log page 02h, 512 bytes): [0] critical warning, [1-2] temperature
//! (kelvin), [3] available spare %, [4] spare threshold, [5] % life used, [32-47] data
//! units read, [48-63] written (units of 512 000 bytes), [112-127] power cycles,
//! [128-143] power-on hours, [144-159] unsafe shutdowns, [160-175] media errors
//! (128-bit integers; the low 64 bits are used here).
use std::ffi::{c_char, c_int};
use std::fmt::Write as _;

const SMART_UNKNOWN: c_int = 0;
const SMART_OK: c_int = 1;
const SMART_WARN: c_int = 2;
const SMART_FAIL: c_int = 3;
const SMART_ATA: c_int = 1;
const SMART_NVME: c_int = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SmartAttr {
    id: u8,
    value: u8,
    worst: u8,
    thresh: u8,
    flags: u16,
    raw: u64,
    failing: c_int,
}

#[repr(C)]
pub struct SmartReport {
    kind: c_int,
    health: c_int,
    checksum_ok: c_int,
    status: c_int,
    temp_c: c_int,
    power_on_hours: i64,
    power_cycles: i64,
    reallocated: i64,
    pending: i64,
    uncorrectable: i64,
    nattr: c_int,
    attr: [SmartAttr; 30],
    crit_warning: c_int,
    spare: c_int,
    spare_thresh: c_int,
    used_pct: c_int,
    data_read: u64,
    data_written: u64,
    media_errors: u64,
    unsafe_shutdowns: u64,
}

fn le(p: &[u8], n: usize) -> u64 {
    p[..n].iter().rev().fold(0, |v, &b| v << 8 | b as u64)
}

/// 128 bits → 64 (saturates if the high half is not zero).
fn le128(p: &[u8]) -> u64 {
    if le(&p[8..], 8) != 0 { u64::MAX } else { le(p, 8) }
}

/// Field by field, so the struct's padding is never written.
fn set_attr(a: &mut SmartAttr, x: &SmartAttr) {
    a.id = x.id;
    a.value = x.value;
    a.worst = x.worst;
    a.thresh = x.thresh;
    a.flags = x.flags;
    a.raw = x.raw;
    a.failing = x.failing;
}

/// Like the C's memset + defaults: padding stays zero because the caller zeroed the
/// whole struct and only fields are assigned here.
fn clear(r: &mut SmartReport, kind: c_int) {
    r.kind = kind;
    r.health = SMART_UNKNOWN;
    r.checksum_ok = 0;
    r.status = -1;
    r.temp_c = -1;
    r.power_on_hours = -1;
    r.power_cycles = -1;
    r.reallocated = -1;
    r.pending = -1;
    r.uncorrectable = -1;
    r.nattr = 0;
    for a in r.attr.iter_mut() {
        set_attr(a, &SmartAttr::default());
    }
    r.crit_warning = 0;
    r.spare = -1;
    r.spare_thresh = -1;
    r.used_pct = -1;
    r.data_read = 0;
    r.data_written = 0;
    r.media_errors = 0;
    r.unsafe_shutdowns = 0;
}

pub fn ata(data: &[u8; 512], thresh: Option<&[u8; 512]>, status: c_int, r: &mut SmartReport) {
    clear(r, SMART_ATA);
    r.status = status;
    r.checksum_ok = (data.iter().fold(0u8, |s, &b| s.wrapping_add(b)) == 0) as c_int;
    let mut failing = false;
    for i in 0..30 {
        let a = &data[2 + i * 12..];
        if a[0] == 0 {
            continue;
        }
        let mut x = SmartAttr { id: a[0], flags: le(&a[1..], 2) as u16, value: a[3], worst: a[4], raw: le(&a[5..], 6), ..Default::default() };
        if let Some(t) = thresh {
            if let Some(k) = (0..30).find(|k| t[2 + k * 12] == x.id) {
                x.thresh = t[2 + k * 12 + 1];
            }
        }
        // value 0 or 0xFE/0xFF = no data (some disks); threshold 0 never fails
        x.failing = (x.thresh != 0 && x.value != 0 && x.value <= x.thresh && x.value < 0xfe) as c_int;
        if x.failing != 0 && x.flags & 1 != 0 {
            failing = true;
        }
        let low = (x.raw & 0xffff_ffff) as i64;
        match x.id {
            5 => r.reallocated = low,
            9 => r.power_on_hours = low,
            12 => r.power_cycles = low,
            194 => r.temp_c = (x.raw & 0xff) as c_int,
            190 if r.temp_c < 0 => r.temp_c = (x.raw & 0xff) as c_int,
            197 => r.pending = low,
            198 => r.uncorrectable = low,
            _ => {}
        }
        set_attr(&mut r.attr[r.nattr as usize], &x);
        r.nattr += 1;
    }
    r.health = if status == 0 || failing {
        SMART_FAIL
    } else if r.reallocated > 0 || r.pending > 0 || r.uncorrectable > 0 {
        SMART_WARN
    } else if status == 1 || r.nattr != 0 {
        if r.checksum_ok != 0 { SMART_OK } else { SMART_WARN }
    } else {
        SMART_UNKNOWN
    };
}

pub fn nvme(log: &[u8; 512], r: &mut SmartReport) {
    clear(r, SMART_NVME);
    r.checksum_ok = 1;
    r.crit_warning = log[0] as c_int;
    let k = le(&log[1..], 2) as c_int;
    r.temp_c = if k != 0 { k - 273 } else { -1 };
    r.spare = log[3] as c_int;
    r.spare_thresh = log[4] as c_int;
    r.used_pct = log[5] as c_int;
    let bytes = |u: u64| u.checked_mul(512_000).unwrap_or(u64::MAX);
    r.data_read = bytes(le128(&log[32..]));
    r.data_written = bytes(le128(&log[48..]));
    r.power_cycles = le128(&log[112..]).min(i64::MAX as u64) as i64;
    r.power_on_hours = le128(&log[128..]).min(i64::MAX as u64) as i64;
    r.unsafe_shutdowns = le128(&log[144..]);
    r.media_errors = le128(&log[160..]);
    // warning bits: 0 low spare, 1 temperature, 2 degraded reliability, 3 read-only, 4 volatile backup failed
    r.health = if r.crit_warning & 0x0d != 0 {
        SMART_FAIL
    } else if r.crit_warning != 0 || r.used_pct >= 100 || r.spare < r.spare_thresh || r.media_errors != 0 {
        SMART_WARN
    } else {
        SMART_OK
    };
}

pub fn summary_json(r: &SmartReport) -> String {
    const H: [&str; 4] = ["unknown", "ok", "warning", "failing"];
    let mut s = format!(
        "{{\"kind\":\"{}\",\"health\":\"{}\",\"checksum\":{},\"status\":{},\"temp\":{},\"hours\":{},\"cycles\":{}",
        if r.kind == SMART_NVME { "nvme" } else { "ata" },
        H[(r.health & 3) as usize],
        r.checksum_ok != 0,
        r.status,
        r.temp_c,
        r.power_on_hours,
        r.power_cycles
    );
    if r.kind == SMART_ATA {
        let _ = write!(
            s,
            ",\"reallocated\":{},\"pending\":{},\"uncorrectable\":{},\"attributes\":{}}}",
            r.reallocated, r.pending, r.uncorrectable, r.nattr
        );
    } else {
        let _ = write!(
            s,
            ",\"critical\":{},\"spare\":{},\"spareThreshold\":{},\"used\":{},\"read\":{},\"written\":{},\"mediaErrors\":{},\"unsafeShutdowns\":{}}}",
            r.crit_warning, r.spare, r.spare_thresh, r.used_pct, r.data_read, r.data_written, r.media_errors, r.unsafe_shutdowns
        );
    }
    s
}

pub fn attr_json(a: &SmartAttr) -> String {
    format!(
        "{{\"id\":{},\"value\":{},\"worst\":{},\"thresh\":{},\"raw\":{},\"prefail\":{},\"failing\":{}}}",
        a.id,
        a.value,
        a.worst,
        a.thresh,
        a.raw,
        a.flags & 1 != 0,
        a.failing != 0
    )
}

/// snprintf semantics: writes what fits (with its NUL); 0 if all of it fit, -1 otherwise.
pub(crate) unsafe fn put_str(out: *mut c_char, n: usize, s: impl AsRef<[u8]>) -> c_int {
    let s = s.as_ref();
    if !out.is_null() && n > 0 {
        let k = s.len().min(n - 1);
        std::ptr::copy_nonoverlapping(s.as_ptr(), out.cast::<u8>(), k);
        *out.add(k) = 0;
    }
    if s.len() < n { 0 } else { -1 }
}

/// # Safety
/// `data` covers 512 bytes, `thresh` is NULL or 512 bytes, `r` is a valid SmartReport.
#[no_mangle]
pub unsafe extern "C" fn smart_ata_parse(data: *const u8, thresh: *const u8, status: c_int, r: *mut SmartReport) -> c_int {
    std::ptr::write_bytes(r, 0, 1);
    let r = &mut *r;
    ata(&*(data as *const [u8; 512]), (thresh as *const [u8; 512]).as_ref(), status, r);
    if r.nattr != 0 || status >= 0 { 0 } else { -1 }
}

/// # Safety
/// `log` covers 512 bytes, `r` is a valid SmartReport.
#[no_mangle]
pub unsafe extern "C" fn smart_nvme_parse(log: *const u8, r: *mut SmartReport) -> c_int {
    std::ptr::write_bytes(r, 0, 1);
    nvme(&*(log as *const [u8; 512]), &mut *r);
    0
}

/// # Safety
/// `r` is a valid report; `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn smart_summary_json(r: *const SmartReport, out: *mut c_char, n: usize) -> c_int {
    put_str(out, n, &summary_json(&*r))
}

/// # Safety
/// `a` is a valid attribute; `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn smart_attr_json(a: *const SmartAttr, out: *mut c_char, n: usize) -> c_int {
    put_str(out, n, &attr_json(&*a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ata_and_nvme() {
        let mut d = [0u8; 512];
        // attribute 5 (pre-fail), value 100, 3 reallocated; 194 at 41 °C
        d[2..2 + 12].copy_from_slice(&[5, 1, 0, 100, 100, 3, 0, 0, 0, 0, 0, 0]);
        d[14..14 + 12].copy_from_slice(&[194, 0, 0, 59, 40, 41, 0, 0, 0, 0, 0, 0]);
        let sum = d.iter().fold(0u8, |s, &b| s.wrapping_add(b));
        d[511] = 0u8.wrapping_sub(sum);
        let mut t = [0u8; 512];
        t[2] = 5;
        t[3] = 36;
        let mut r: Box<SmartReport> = unsafe { Box::new_zeroed().assume_init() };
        ata(&d, Some(&t), 1, &mut r);
        assert_eq!((r.nattr, r.reallocated, r.temp_c, r.health, r.checksum_ok), (2, 3, 41, SMART_WARN, 1));
        assert_eq!(r.attr[0].thresh, 36);
        assert!(summary_json(&r).ends_with("\"attributes\":2}"));
        let mut l = [0u8; 512];
        l[1] = 0x3c;
        l[2] = 0x01; // 316 K
        l[3] = 100;
        l[4] = 10;
        l[32] = 2;
        l[48 + 8] = 1; // high half: saturates
        nvme(&l, &mut r);
        assert_eq!((r.temp_c, r.data_read, r.data_written, r.health), (43, 1_024_000, u64::MAX, SMART_OK));
    }
}
