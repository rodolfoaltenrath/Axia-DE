//! Night Light math for Settings → Displays: color temperature → RGB gains, the
//! gamma ramp, sunrise/sunset and the schedule blend. No location services: the
//! coordinates come from the chosen time zone (zone1970.tab), never from the
//! network. C ABI of server/src/hw/night.h. The operations follow the C in the same
//! order (no fused multiply-add), so results are the same bits.
use crate::lanparse::cstr;
use std::ffi::{c_char, c_int};

const NIGHT_K_MAX: i32 = 6500;
const PI: f64 = 3.141_592_653_589_793_238_46;

/// Tanner Helland's fit of the blackbody curve (sRGB 0..255); only its ratios are used.
fn helland(kelvin: f64) -> (f64, f64, f64) {
    let t = kelvin / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698727446 * (t - 60.0).powf(-0.1332047592) };
    let g = if t <= 66.0 { 99.4708025861 * t.ln() - 161.1195681661 } else { 288.1221695283 * (t - 60.0).powf(-0.0755148492) };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.5177312231 * (t - 10.0).ln() - 305.0447927307
    };
    (r, g, b)
}

fn clamp01(v: f64) -> f64 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

pub fn temp_rgb(kelvin: i32) -> (f64, f64, f64) {
    let kelvin = kelvin.clamp(1000, 10000);
    if kelvin == NIGHT_K_MAX {
        return (1.0, 1.0, 1.0);
    }
    let (r0, g0, b0) = helland(NIGHT_K_MAX as f64);
    let (r1, g1, b1) = helland(kelvin as f64);
    (clamp01(r1 / r0), clamp01(g1 / g0), clamp01(b1 / b0))
}

fn day_of_year(y: i32, m: i32, d: i32) -> i32 {
    const CUM: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    CUM[(m - 1).rem_euclid(12) as usize] + d + (leap && m > 2) as i32
}

/// C's fmod (truncated remainder, the sign of x): Rust's % on f64 is the same.
fn fmod360(v: f64) -> f64 {
    let v = v % 360.0;
    if v < 0.0 { v + 360.0 } else { v }
}
fn rad(d: f64) -> f64 {
    d * PI / 180.0
}
fn deg(r: f64) -> f64 {
    r * 180.0 / PI
}

/// "Almanac for Computers" (US Naval Observatory, 1990) sunrise algorithm, official
/// zenith 90°50', in minutes after midnight UTC.
fn sun_event(n: i32, lat: f64, lon: f64, rising: bool) -> Option<f64> {
    let lng_hour = lon / 15.0;
    let t = n as f64 + ((if rising { 6.0 } else { 18.0 }) - lng_hour) / 24.0;
    let m = 0.9856 * t - 3.289;
    let l = fmod360(m + 1.916 * rad(m).sin() + 0.020 * rad(2.0 * m).sin() + 282.634);
    let mut ra = fmod360(deg((0.91764 * rad(l).tan()).atan()));
    ra += (l / 90.0).floor() * 90.0 - (ra / 90.0).floor() * 90.0;
    ra /= 15.0;
    let sin_dec = 0.39782 * rad(l).sin();
    let cos_dec = sin_dec.asin().cos();
    let cos_h = (rad(90.833).cos() - sin_dec * rad(lat).sin()) / (cos_dec * rad(lat).cos());
    if cos_h > 1.0 || cos_h < -1.0 {
        return None;
    }
    let mut h = if rising { 360.0 - deg(cos_h.acos()) } else { deg(cos_h.acos()) };
    h /= 15.0;
    let local = h + ra - 0.06571 * t - 6.622;
    let mut ut = (local - lng_hour) % 24.0;
    if ut < 0.0 {
        ut += 24.0;
    }
    Some(ut * 60.0)
}

pub fn sun_times(y: i32, m: i32, d: i32, lat: f64, lon: f64) -> Option<(f64, f64)> {
    let n = day_of_year(y, m, d);
    Some((sun_event(n, lat, lon, true)?, sun_event(n, lat, lon, false)?))
}

pub fn amount(now: i32, from: i32, to: i32, fade: i32) -> f64 {
    let len = (to.wrapping_sub(from) % 1440 + 1440) % 1440;
    if len == 0 {
        return 0.0;
    }
    let d = (now.wrapping_sub(from) % 1440 + 1440) % 1440;
    if d >= len {
        return 0.0;
    }
    if fade <= 0 {
        return 1.0;
    }
    let (a, b) = (d as f64 / fade as f64, (len - d) as f64 / fade as f64);
    clamp01(if a < b { a } else { b })
}

/// ±DDMM[SS] (lat) or ±DDDMM[SS] (lon): deg_digits = 2 or 3.
fn coord(s: &[u8], deg_digits: usize) -> Option<f64> {
    let len = s.len();
    if len != 1 + deg_digits + 2 && len != 1 + deg_digits + 4 {
        return None;
    }
    if s[0] != b'+' && s[0] != b'-' {
        return None;
    }
    if !s[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = |i: usize| (s[i] - b'0') as i32;
    let dg = (1..=deg_digits).fold(0, |n, i| n * 10 + v(i));
    let mn = v(deg_digits + 1) * 10 + v(deg_digits + 2);
    let sc = if len > 1 + deg_digits + 2 { v(deg_digits + 3) * 10 + v(deg_digits + 4) } else { 0 };
    if mn > 59 || sc > 59 {
        return None;
    }
    let r = dg as f64 + mn as f64 / 60.0 + sc as f64 / 3600.0;
    Some(if s[0] == b'-' { -r } else { r })
}

/// ISO 6709 coordinates as in zone1970.tab ("+4230+00131" or "+404251-0740023").
pub fn parse_coords(s: &[u8]) -> Option<(f64, f64)> {
    let split = (1..s.len()).find(|&i| s[i] == b'+' || s[i] == b'-')?;
    let lat = coord(&s[..split], 2)?;
    let lon = coord(&s[split..], 3)?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some((lat, lon))
}

/// Looks up a zone in a zone1970.tab text: codes \t coordinates \t TZ [\t comments].
pub fn zone_coords(tab: &[u8], zone: &[u8]) -> Option<(f64, f64)> {
    if zone.is_empty() {
        return None;
    }
    let mut lines: Vec<&[u8]> = tab.split(|&c| c == b'\n').collect();
    if tab.ends_with(b"\n") {
        lines.pop();
    }
    for line in lines {
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let mut f = line.splitn(4, |&c| c == b'\t');
        let (Some(_), Some(c), Some(z)) = (f.next(), f.next(), f.next()) else { continue };
        if z == zone {
            if c.len() >= 24 {
                return None;
            }
            return parse_coords(c);
        }
    }
    None
}

// ---- C ABI ----

/// # Safety
/// The outs are valid pointers.
#[no_mangle]
pub unsafe extern "C" fn night_temp_rgb(kelvin: c_int, r: *mut f64, g: *mut f64, b: *mut f64) {
    let (x, y, z) = temp_rgb(kelvin);
    *r = x;
    *g = y;
    *b = z;
}

/// # Safety
/// red, green and blue have room for `n` entries.
#[no_mangle]
pub unsafe extern "C" fn night_ramp(n: c_int, r: f64, g: f64, b: f64, bright: f64, red: *mut u16, green: *mut u16, blue: *mut u16) {
    let bright = clamp01(bright);
    for i in 0..n.max(0) as usize {
        let v = if n > 1 { i as f64 / (n - 1) as f64 } else { 1.0 };
        let q = |c: f64| (clamp01(v * c * bright) * 65535.0 + 0.5) as u16;
        *red.add(i) = q(r);
        *green.add(i) = q(g);
        *blue.add(i) = q(b);
    }
}

/// # Safety
/// The outs are valid pointers.
#[no_mangle]
pub unsafe extern "C" fn night_sun_times(y: c_int, m: c_int, d: c_int, lat: f64, lon: f64, rise: *mut f64, set: *mut f64) -> c_int {
    // the C wrote sunrise before it knew whether sunset existed
    let n = day_of_year(y, m, d);
    let Some(r) = sun_event(n, lat, lon, true) else { return -1 };
    *rise = r;
    let Some(s) = sun_event(n, lat, lon, false) else { return -1 };
    *set = s;
    0
}

#[no_mangle]
pub extern "C" fn night_amount(now: c_int, from: c_int, to: c_int, fade: c_int) -> f64 {
    amount(now, from, to, fade)
}

/// # Safety
/// `s` is a C string; the outs are valid pointers.
#[no_mangle]
pub unsafe extern "C" fn night_parse_coords(s: *const c_char, lat: *mut f64, lon: *mut f64) -> c_int {
    coords_out(parse_coords_partial(cstr(s)), lat, lon)
}

/// # Safety
/// `tab` and `zone` are C strings; the outs are valid pointers.
#[no_mangle]
pub unsafe extern "C" fn night_zone_coords(tab: *const c_char, zone: *const c_char, lat: *mut f64, lon: *mut f64) -> c_int {
    let (tab, zone) = (cstr(tab), cstr(zone));
    if zone.is_empty() {
        return -1;
    }
    let mut lines: Vec<&[u8]> = tab.split(|&c| c == b'\n').collect();
    if tab.ends_with(b"\n") {
        lines.pop();
    }
    for line in lines {
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let mut f = line.splitn(4, |&c| c == b'\t');
        let (Some(_), Some(c), Some(z)) = (f.next(), f.next(), f.next()) else { continue };
        if z == zone {
            if c.len() >= 24 {
                return -1;
            }
            return coords_out(parse_coords_partial(c), lat, lon);
        }
    }
    -1
}

/// What the C left in lat/lon: the latitude is written as soon as it parses, the
/// longitude too, before the range check.
fn parse_coords_partial(s: &[u8]) -> (Option<f64>, Option<f64>, bool) {
    let Some(split) = (1..s.len()).find(|&i| s[i] == b'+' || s[i] == b'-') else { return (None, None, false) };
    let Some(lat) = coord(&s[..split], 2) else { return (None, None, false) };
    let Some(lon) = coord(&s[split..], 3) else { return (Some(lat), None, false) };
    let ok = (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon);
    (Some(lat), Some(lon), ok)
}

unsafe fn coords_out((lat, lon, ok): (Option<f64>, Option<f64>, bool), plat: *mut f64, plon: *mut f64) -> c_int {
    if let Some(v) = lat {
        *plat = v;
    }
    if let Some(v) = lon {
        *plon = v;
    }
    if ok { 0 } else { -1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math() {
        assert_eq!(temp_rgb(6500), (1.0, 1.0, 1.0));
        let (r, g, b) = temp_rgb(3400);
        assert!(r == 1.0 && g < 1.0 && b < g);
        assert_eq!(amount(23 * 60, 22 * 60, 6 * 60, 60), 1.0);
        assert_eq!(amount(12 * 60, 22 * 60, 6 * 60, 60), 0.0);
        assert_eq!(amount(22 * 60 + 30, 22 * 60, 6 * 60, 60), 0.5);
        let (lat, lon) = parse_coords(b"-3436-05827").unwrap();
        assert!((lat + 34.6).abs() < 1e-9 && (lon + 58.45).abs() < 1e-9);
        assert!(parse_coords(b"+404251-0740023").is_some());
        assert!(parse_coords(b"+9930+00000").is_none());
        let tab = b"#c\nAR\t-3436-05827\tAmerica/Argentina/Buenos_Aires\tmost\n";
        assert!(zone_coords(tab, b"America/Argentina/Buenos_Aires").is_some());
        let (rise, set) = sun_times(2026, 6, 21, -34.6, -58.45).unwrap();
        assert!(rise > 600.0 && rise < 700.0 && set > 1200.0 && set < 1300.0, "{rise} {set}");
        assert!(sun_times(2026, 6, 21, 80.0, 0.0).is_none());
    }
}
