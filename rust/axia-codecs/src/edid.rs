//! Display identity and modes for Settings → Displays: our own EDID 1.3/1.4 reader (the
//! 128-byte base block a monitor sends; untrusted, only validated bytes are read) and the
//! KMS mode helpers (exact refresh, picking a mode, the resolutions and rates a connector
//! offers). C ABI of server/src/hw/edid.h and server/src/hw/modes.h.
use std::ffi::{c_char, c_int};

#[repr(C)]
pub struct Edid {
    vendor: [u8; 4],
    product: u16,
    serial: u32,
    name: [u8; 14],
    serial_text: [u8; 14],
    year: c_int,
    week: c_int,
    width_cm: c_int,
    height_cm: c_int,
    pref_w: c_int,
    pref_h: c_int,
    version: c_int,
    revision: c_int,
    digital: c_int,
}

fn cs(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// Text of a display descriptor: up to 13 bytes, ends with 0x0A and is padded with
/// spaces. Printable ASCII only; anything else is dropped.
fn desc_text(d: &[u8], out: &mut [u8; 14]) {
    let mut v: Vec<u8> = Vec::new();
    for &c in &d[5..18] {
        if c == 0x0a || c == 0 {
            break;
        }
        if (0x20..0x7f).contains(&c) {
            v.push(c);
        }
    }
    while v.last() == Some(&b' ') {
        v.pop();
    }
    out[..v.len()].copy_from_slice(&v);
    out[v.len()] = 0;
}

/// Fills `e` (zeroed by the caller) from a valid base block.
pub fn parse(d: &[u8], e: &mut Edid) -> bool {
    const HDR: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
    if d.len() < 128 || d[..8] != HDR || d[..128].iter().fold(0u8, |s, &b| s.wrapping_add(b)) != 0 {
        return false;
    }
    let m = (d[8] as u32) << 8 | d[9] as u32;
    for i in 0..3 {
        let c = (m >> (10 - 5 * i)) & 31;
        e.vendor[i] = if (1..=26).contains(&c) { b'A' + c as u8 - 1 } else { b'?' };
    }
    e.product = u16::from_le_bytes([d[10], d[11]]);
    e.serial = u32::from_le_bytes([d[12], d[13], d[14], d[15]]);
    e.week = d[16] as c_int;
    e.year = if d[17] != 0 { 1990 + d[17] as c_int } else { 0 };
    e.version = d[18] as c_int;
    e.revision = d[19] as c_int;
    e.digital = (d[20] >> 7) as c_int;
    e.width_cm = d[21] as c_int;
    e.height_cm = d[22] as c_int;
    for k in 0..4 {
        let x = &d[54 + 18 * k..54 + 18 * (k + 1)];
        if x[0] != 0 || x[1] != 0 {
            // detailed timing
            if e.pref_w == 0 {
                e.pref_w = x[2] as c_int | ((x[4] & 0xf0) as c_int) << 4;
                e.pref_h = x[5] as c_int | ((x[7] & 0xf0) as c_int) << 4;
            }
            continue;
        }
        match x[3] {
            0xfc => desc_text(x, &mut e.name),
            0xff => desc_text(x, &mut e.serial_text),
            _ => {}
        }
    }
    true
}

/// Stable identity used as the settings key, only [A-Za-z0-9.-].
pub fn identity(e: &Edid) -> Vec<u8> {
    let vendor = String::from_utf8_lossy(cs(&e.vendor)).into_owned();
    let mut s = format!("{vendor}-{:04x}-", e.product).into_bytes();
    if !cs(&e.serial_text).is_empty() {
        s.extend_from_slice(cs(&e.serial_text));
    } else if e.serial != 0 {
        s.extend_from_slice(format!("{:08x}", e.serial).as_bytes());
    } else if !cs(&e.name).is_empty() {
        s.extend_from_slice(cs(&e.name));
    } else {
        s.push(b'0');
    }
    s
}

pub fn display_name(e: &Edid) -> Vec<u8> {
    if !cs(&e.name).is_empty() {
        return cs(&e.name).to_vec();
    }
    let mut s = cs(&e.vendor).to_vec();
    s.extend_from_slice(format!(" {:04X}", e.product).as_bytes());
    s
}

pub fn diagonal_tenths(e: &Edid) -> c_int {
    if e.width_cm <= 0 || e.height_cm <= 0 {
        return 0;
    }
    let (w, h) = (e.width_cm as f64, e.height_cm as f64);
    let inches = (w * w + h * h).sqrt() / 2.54;
    (inches * 10. + 0.5) as c_int
}

/// snprintf(out, n, "%s", s); `fix` runs over what was written.
unsafe fn put(out: *mut c_char, n: usize, s: &[u8], fix: bool) {
    if out.is_null() || n == 0 {
        return;
    }
    let o = std::slice::from_raw_parts_mut(out.cast::<u8>(), n);
    let k = s.len().min(n - 1);
    o[..k].copy_from_slice(&s[..k]);
    o[k] = 0;
    if fix {
        for c in &mut o[..k] {
            if !(c.is_ascii_alphanumeric() || *c == b'-' || *c == b'.') {
                *c = b'_';
            }
        }
    }
}

/// # Safety
/// `d` covers `len` bytes (or is NULL); `e` points to an Edid.
#[no_mangle]
pub unsafe extern "C" fn edid_parse(d: *const u8, len: usize, e: *mut Edid) -> c_int {
    std::ptr::write_bytes(e, 0, 1);
    if d.is_null() || len < 128 {
        return -1;
    }
    let e = &mut *e;
    if parse(std::slice::from_raw_parts(d, len), e) {
        0
    } else {
        std::ptr::write_bytes(e as *mut Edid, 0, 1);
        -1
    }
}
/// # Safety
/// `e` is valid; `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn edid_identity(e: *const Edid, out: *mut c_char, n: usize) {
    put(out, n, &identity(&*e), true);
}
/// # Safety
/// `e` is valid; `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn edid_display_name(e: *const Edid, out: *mut c_char, n: usize) {
    put(out, n, &display_name(&*e), false);
}
/// # Safety
/// `e` is valid.
#[no_mangle]
pub unsafe extern "C" fn edid_diagonal_tenths(e: *const Edid) -> c_int {
    diagonal_tenths(&*e)
}

// ---- KMS modes (struct drm_mode_modeinfo, kernel uapi) ----

const DRM_MODE_FLAG_INTERLACE: u32 = 1 << 4;
const DRM_MODE_FLAG_DBLSCAN: u32 = 1 << 5;
const DRM_MODE_TYPE_PREFERRED: u32 = 1 << 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModeInfo {
    clock: u32,
    hdisplay: u16,
    hsync_start: u16,
    hsync_end: u16,
    htotal: u16,
    hskew: u16,
    vdisplay: u16,
    vsync_start: u16,
    vsync_end: u16,
    vtotal: u16,
    vscan: u16,
    vrefresh: u32,
    flags: u32,
    ty: u32,
    name: [u8; 32],
}

/// Refresh in millihertz (59940 = 59.94 Hz), from the pixel clock and totals. 0 if invalid.
pub fn refresh_mhz(m: &ModeInfo) -> c_int {
    if m.htotal == 0 || m.vtotal == 0 || m.clock == 0 {
        return 0;
    }
    let mut num = m.clock as i64 * 1_000_000; // kHz → mHz numerator
    let mut den = m.htotal as i64 * m.vtotal as i64;
    if m.flags & DRM_MODE_FLAG_INTERLACE != 0 {
        num *= 2;
    }
    if m.flags & DRM_MODE_FLAG_DBLSCAN != 0 {
        den *= 2;
    }
    if m.vscan > 1 {
        den *= m.vscan as i64;
    }
    ((num + den / 2) / den) as c_int
}

fn interlaced(m: &ModeInfo) -> bool {
    m.flags & DRM_MODE_FLAG_INTERLACE != 0
}

fn area(m: &ModeInfo) -> i64 {
    m.hdisplay as i64 * m.vdisplay as i64
}

pub fn preferred(modes: &[ModeInfo]) -> c_int {
    if modes.is_empty() {
        return -1;
    }
    if let Some(i) = modes.iter().position(|m| m.ty & DRM_MODE_TYPE_PREFERRED != 0) {
        return i as c_int;
    }
    let mut best = 0;
    for i in 1..modes.len() {
        if area(&modes[i]) > area(&modes[best]) {
            best = i;
        }
    }
    best as c_int
}

pub fn pick(modes: &[ModeInfo], mut w: c_int, mut h: c_int, refresh: c_int) -> c_int {
    if w <= 0 || h <= 0 {
        let p = preferred(modes);
        if p < 0 {
            return -1;
        }
        w = modes[p as usize].hdisplay as c_int;
        h = modes[p as usize].vdisplay as c_int;
        if refresh <= 0 {
            return p;
        }
    }
    let (mut best, mut best_score) = (-1, 0i64);
    for (i, m) in modes.iter().enumerate() {
        if m.hdisplay as c_int != w || m.vdisplay as c_int != h {
            continue;
        }
        let r = refresh_mhz(m) as i64;
        // lower is better: interlace penalty, then distance from the wanted rate (or highest)
        let mut score = if interlaced(m) { 100_000_000 } else { 0 };
        score += if refresh > 0 { (r - refresh as i64).abs() } else { 10_000_000 - r };
        if m.ty & DRM_MODE_TYPE_PREFERRED != 0 {
            score -= 1; // tie → the preferred one
        }
        if best < 0 || score < best_score {
            best = i as c_int;
            best_score = score;
        }
    }
    best
}

pub fn resolutions(modes: &[ModeInfo], max: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for (i, m) in modes.iter().enumerate() {
        if interlaced(m) {
            continue;
        }
        if let Some(j) = out.iter().position(|&o| modes[o].hdisplay == m.hdisplay && modes[o].vdisplay == m.vdisplay) {
            if refresh_mhz(m) > refresh_mhz(&modes[out[j]]) {
                out[j] = i;
            }
            continue;
        }
        if out.len() < max {
            out.push(i);
        }
    }
    // largest area first, stable like the C's insertion sort
    out.sort_by(|&a, &b| area(&modes[b]).cmp(&area(&modes[a])));
    out
}

pub fn rates(modes: &[ModeInfo], w: c_int, h: c_int, max: usize) -> Vec<c_int> {
    let mut out: Vec<c_int> = Vec::new();
    for m in modes {
        if m.hdisplay as c_int != w || m.vdisplay as c_int != h || interlaced(m) {
            continue;
        }
        let r = refresh_mhz(m);
        if !out.iter().any(|&o| (o - r).abs() <= 5) && out.len() < max {
            out.push(r); // the same rate within 0.005 Hz counts once
        }
    }
    out.sort_by(|a, b| b.cmp(a));
    out
}

unsafe fn modes<'a>(m: *const ModeInfo, n: c_int) -> &'a [ModeInfo] {
    if m.is_null() || n <= 0 { &[] } else { std::slice::from_raw_parts(m, n as usize) }
}

/// # Safety
/// `m` is a valid mode.
#[no_mangle]
pub unsafe extern "C" fn mode_refresh_mhz(m: *const ModeInfo) -> c_int {
    refresh_mhz(&*m)
}
/// # Safety
/// `m` has `n` modes.
#[no_mangle]
pub unsafe extern "C" fn mode_preferred(m: *const ModeInfo, n: c_int) -> c_int {
    preferred(modes(m, n))
}
/// # Safety
/// `m` has `n` modes.
#[no_mangle]
pub unsafe extern "C" fn mode_pick(m: *const ModeInfo, n: c_int, w: c_int, h: c_int, refresh: c_int) -> c_int {
    pick(modes(m, n), w, h, refresh)
}
/// # Safety
/// `m` has `n` modes; `out` has room for `max` ints.
#[no_mangle]
pub unsafe extern "C" fn mode_resolutions(m: *const ModeInfo, n: c_int, out: *mut c_int, max: c_int) -> c_int {
    let v = resolutions(modes(m, n), max.max(0) as usize);
    for (k, &i) in v.iter().enumerate() {
        *out.add(k) = i as c_int;
    }
    v.len() as c_int
}
/// # Safety
/// `m` has `n` modes; `out` has room for `max` ints.
#[no_mangle]
pub unsafe extern "C" fn mode_rates(m: *const ModeInfo, n: c_int, w: c_int, h: c_int, out: *mut c_int, max: c_int) -> c_int {
    let v = rates(modes(m, n), w, h, max.max(0) as usize);
    for (k, &r) in v.iter().enumerate() {
        *out.add(k) = r;
    }
    v.len() as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> [u8; 128] {
        let mut d = [0u8; 128];
        d[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        d[8] = 0x10; // "DEL": D=4 E=5 L=12
        d[9] = 0xac;
        d[10] = 0xc4;
        d[11] = 0xa0;
        d[17] = 30;
        d[18] = 1;
        d[19] = 4;
        d[21] = 60;
        d[22] = 34;
        d[54] = 1; // a detailed timing, 3840×2160
        d[56] = 0x00;
        d[58] = 0xf0;
        d[59] = 0x70;
        d[61] = 0x80;
        d[72 + 3] = 0xfc;
        d[72 + 5..72 + 18].copy_from_slice(b"DELL U2720Q\n ");
        let s = d.iter().fold(0u8, |s, &b| s.wrapping_add(b));
        d[127] = 0u8.wrapping_sub(s);
        d
    }

    #[test]
    fn a_monitor() {
        let mut e: Edid = unsafe { std::mem::zeroed() };
        assert!(parse(&block(), &mut e));
        assert_eq!((cs(&e.vendor), e.product, e.year, e.pref_w, e.pref_h), (&b"DEL"[..], 0xa0c4, 2020, 3840, 2160));
        assert_eq!(identity(&e), b"DEL-a0c4-DELL U2720Q");
        assert_eq!(display_name(&e), b"DELL U2720Q");
        assert_eq!(diagonal_tenths(&e), 272);
        let mut bad = block();
        bad[100] ^= 1;
        assert!(!parse(&bad, &mut e));
    }

    #[test]
    fn modes_are_picked_like_kms() {
        let mk = |w: u16, h: u16, clock: u32, ht: u16, vt: u16, flags: u32, ty: u32| ModeInfo {
            clock,
            hdisplay: w,
            hsync_start: 0,
            hsync_end: 0,
            htotal: ht,
            hskew: 0,
            vdisplay: h,
            vsync_start: 0,
            vsync_end: 0,
            vtotal: vt,
            vscan: 0,
            vrefresh: 0,
            flags,
            ty,
            name: [0; 32],
        };
        let m = [
            mk(1920, 1080, 148500, 2200, 1125, 0, DRM_MODE_TYPE_PREFERRED),
            mk(1920, 1080, 148352, 2200, 1125, 0, 0),
            mk(1920, 1080, 74250, 2200, 1125, DRM_MODE_FLAG_INTERLACE, 0),
            mk(2560, 1440, 241500, 2720, 1481, 0, 0),
        ];
        assert_eq!(refresh_mhz(&m[0]), 60000);
        assert_eq!(refresh_mhz(&m[1]), 59940);
        assert_eq!(pick(&m, 0, 0, 0), 0);
        assert_eq!(pick(&m, 1920, 1080, 59940), 1);
        assert_eq!(resolutions(&m, 8), vec![3, 0]);
        assert_eq!(rates(&m, 1920, 1080, 8), vec![60000, 59940]);
    }
}
