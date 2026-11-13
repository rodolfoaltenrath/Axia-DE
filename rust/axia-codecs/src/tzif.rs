//! Time zones with our own code: TZif files (RFC 8536, versions 1 to 4) and POSIX TZ rules (the
//! TZif footer, for dates after the last transition). The C ABI of server/src/ui/tzif.h, which this
//! replaced: lord.time.zoneOffset and axia-maild read other cities' times without touching the
//! process time zone. Everything is validated: a short file or an index out of range is an error,
//! and nothing reads past its slice.
use std::ffi::{c_char, c_int, CStr};
use std::slice;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TzInfo {
    /// Seconds east of UTC (Buenos Aires: -10800).
    pub utoff: i32,
    pub isdst: c_int,
    /// "CEST", "-03"…
    pub abbr: [c_char; 16],
}
impl TzInfo {
    fn new(utoff: i32, isdst: bool, abbr: &[u8]) -> Self {
        let mut out = Self { utoff, isdst: isdst as c_int, abbr: [0; 16] };
        out.set_abbr(abbr);
        out
    }
    /// snprintf("%s", 16): at most 15 bytes and the NUL.
    fn set_abbr(&mut self, abbr: &[u8]) {
        let n = abbr.len().min(self.abbr.len() - 1);
        for (slot, b) in self.abbr.iter_mut().zip(abbr.iter().take(n)) {
            *slot = *b as c_char;
        }
        self.abbr[n] = 0;
    }
}

fn be32(p: &[u8]) -> u32 {
    u32::from(p[0]) << 24 | u32::from(p[1]) << 16 | u32::from(p[2]) << 8 | u32::from(p[3])
}
fn be64(p: &[u8]) -> i64 {
    (u64::from(be32(p)) << 32 | u64::from(be32(&p[4..]))) as i64
}

// ---- dates ----

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn year_of(t: i64) -> i64 {
    let z = (if t >= 0 { t } else { t - 86399 }) / 86400 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(m <= 2)
}

fn leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
fn mdays(y: i64, m: i64) -> i64 {
    const D: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if m == 2 && leap(y) {
        29
    } else {
        D[(m - 1) as usize]
    }
}

// ---- POSIX rules ----

/// 'J' n, 'D' n, 'M' m.w.d with the time of day it happens at.
#[derive(Clone, Copy)]
struct TzDate {
    kind: u8,
    a: i64,
    b: i64,
    c: i64,
    time: i32,
}

fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// A zone name, plain letters or `<…>`; at most `n - 1` of them are kept (what the C one did).
fn p_name(s: &[u8], mut i: usize, n: usize) -> Option<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    if at(s, i) == b'<' {
        i += 1;
        while at(s, i) != 0 && at(s, i) != b'>' {
            if out.len() + 1 < n {
                out.push(s[i]);
            }
            i += 1;
        }
        if at(s, i) != b'>' {
            return None;
        }
        i += 1;
    } else {
        while at(s, i).is_ascii_alphabetic() {
            if out.len() + 1 < n {
                out.push(s[i]);
            }
            i += 1;
        }
    }
    (!out.is_empty()).then_some((out, i))
}

/// `[+-]hh[:mm[:ss]]`; `max` is the allowed hours (24 for offsets, 167 for rule times).
fn p_time(s: &[u8], mut i: usize, max: i64) -> Option<(i32, usize)> {
    let mut sign = 1;
    if at(s, i) == b'+' {
        i += 1;
    } else if at(s, i) == b'-' {
        sign = -1;
        i += 1;
    }
    if !at(s, i).is_ascii_digit() {
        return None;
    }
    let (mut h, mut m, mut sec) = (0i64, 0i64, 0i64);
    while at(s, i).is_ascii_digit() {
        h = h * 10 + i64::from(s[i] - b'0');
        i += 1;
        if h > max {
            return None;
        }
    }
    if at(s, i) == b':' {
        i += 1;
        if !at(s, i).is_ascii_digit() {
            return None;
        }
        while at(s, i).is_ascii_digit() {
            m = m * 10 + i64::from(s[i] - b'0');
            i += 1;
            if m > 59 {
                return None;
            }
        }
        if at(s, i) == b':' {
            i += 1;
            if !at(s, i).is_ascii_digit() {
                return None;
            }
            while at(s, i).is_ascii_digit() {
                sec = sec * 10 + i64::from(s[i] - b'0');
                i += 1;
                if sec > 59 {
                    return None;
                }
            }
        }
    }
    Some(((sign * (h * 3600 + m * 60 + sec)) as i32, i))
}

fn p_num(s: &[u8], mut i: usize, lo: i64, hi: i64) -> Option<(i64, usize)> {
    if !at(s, i).is_ascii_digit() {
        return None;
    }
    let mut v = 0i64;
    while at(s, i).is_ascii_digit() {
        v = v * 10 + i64::from(s[i] - b'0');
        i += 1;
        if v > hi {
            return None;
        }
    }
    (v >= lo).then_some((v, i))
}

/// A rule date into `d` (which keeps its other fields when the kind does not set them).
fn p_date(s: &[u8], i: usize, d: &mut TzDate) -> Option<usize> {
    d.time = 7200;
    let mut at_i = if at(s, i) == b'J' {
        d.kind = b'J';
        p_num(s, i + 1, 1, 365).map(|(v, j)| {
            d.a = v;
            j
        })
    } else if at(s, i) == b'M' {
        d.kind = b'M';
        let mut j = p_num(s, i + 1, 1, 12).map(|(v, j)| {
            d.a = v;
            j
        });
        j = match j {
            Some(k) if at(s, k) == b'.' => p_num(s, k + 1, 1, 5).map(|(v, j)| {
                d.b = v;
                j
            }),
            _ => None,
        };
        match j {
            Some(k) if at(s, k) == b'.' => p_num(s, k + 1, 0, 6).map(|(v, j)| {
                d.c = v;
                j
            }),
            _ => None,
        }
    } else {
        d.kind = b'D';
        p_num(s, i, 0, 365).map(|(v, j)| {
            d.a = v;
            j
        })
    };
    if let Some(k) = at_i {
        if at(s, k) == b'/' {
            at_i = p_time(s, k + 1, 167).map(|(v, j)| {
                d.time = v;
                j
            });
        }
    }
    at_i
}

/// Seconds since 1970 of the start (local 00:00) of the day the rule names in year `y`.
fn rule_day(d: &TzDate, y: i64) -> i64 {
    let jan1 = days_from_civil(y, 1, 1);
    if d.kind == b'J' {
        return (jan1 + d.a - 1 + i64::from(leap(y) && d.a >= 60)) * 86400;
    }
    if d.kind == b'D' {
        return (jan1 + d.a) * 86400;
    }
    let first = days_from_civil(y, d.a, 1);
    let wd = ((first + 4) % 7 + 7) % 7; // 1970-01-01 was a Thursday
    let mut day = 1 + (d.c - wd + 7) % 7 + (d.b - 1) * 7;
    while day > mdays(y, d.a) {
        day -= 7;
    }
    (first + day - 1) * 86400
}

/// Local time at `t` by a POSIX rule ("CET-1CEST,M3.5.0,M10.5.0/3").
fn posix(rule: &[u8], t: i64) -> Option<TzInfo> {
    let (std, i) = p_name(rule, 0, 16)?;
    let (stdoff, i) = p_time(rule, i, 24)?;
    let stdoff = -stdoff; // POSIX counts westwards
    if at(rule, i) == 0 {
        return Some(TzInfo::new(stdoff, false, &std));
    }
    let (dst, mut i) = p_name(rule, i, 16)?;
    let mut dstoff = stdoff + 3600;
    if at(rule, i) != 0 && at(rule, i) != b',' {
        let (v, j) = p_time(rule, i, 24)?;
        dstoff = -v;
        i = j;
    }
    let mut a = TzDate { kind: b'M', a: 3, b: 2, c: 0, time: 7200 }; // default: the US rule
    let mut b = TzDate { kind: b'M', a: 11, b: 1, c: 0, time: 7200 };
    if at(rule, i) == b',' {
        i = p_date(rule, i + 1, &mut a)?;
        if at(rule, i) != b',' {
            return None;
        }
        i = p_date(rule, i + 1, &mut b)?;
    }
    if at(rule, i) != 0 {
        return None;
    }
    let (stdoff64, dstoff64) = (i64::from(stdoff), i64::from(dstoff));
    let y = year_of(t + stdoff64);
    let mut inside = false;
    let mut yy = y - 1;
    while yy <= y + 1 && !inside {
        // the start is given in standard time and the end in daylight time
        let start = rule_day(&a, yy) + i64::from(a.time) - stdoff64;
        let end = rule_day(&b, yy) + i64::from(b.time) - dstoff64;
        if start < end {
            inside = t >= start && t < end;
        } else {
            // southern hemisphere: summer spans the new year; check this year's span
            let next_end = rule_day(&b, yy + 1) + i64::from(b.time) - dstoff64;
            inside = t >= start && t < next_end;
        }
        yy += 1;
    }
    Some(if inside { TzInfo::new(dstoff, true, &dst) } else { TzInfo::new(stdoff, false, &std) })
}

// ---- TZif ----

#[derive(Clone, Copy, Default)]
struct Counts {
    isut: u32,
    isstd: u32,
    leap: u32,
    time: u32,
    kind: u32,
    chars: u32,
}

fn counts(p: &[u8]) -> Option<Counts> {
    if p.len() < 44 || &p[..4] != b"TZif" {
        return None;
    }
    let c = Counts { isut: be32(&p[20..]), isstd: be32(&p[24..]), leap: be32(&p[28..]), time: be32(&p[32..]), kind: be32(&p[36..]), chars: be32(&p[40..]) };
    if c.kind == 0 || c.kind > 256 || c.chars == 0 || c.chars > 4096 || c.time > 1 << 20 || c.leap > 1 << 16 {
        return None;
    }
    if (c.isut != 0 && c.isut != c.kind) || (c.isstd != 0 && c.isstd != c.kind) {
        return None;
    }
    Some(c)
}

fn block_len(c: &Counts, v2: bool) -> usize {
    let ts = if v2 { 8 } else { 4 };
    c.time as usize * ts + c.time as usize + c.kind as usize * 6 + c.chars as usize + c.leap as usize * (ts + 4) + c.isstd as usize + c.isut as usize
}

fn fill(types: &[u8], chars: &[u8], idx: usize) -> TzInfo {
    let tt = &types[idx * 6..];
    let di = usize::from(tt[5]);
    let name: Vec<u8> = if di < chars.len() { chars[di..].iter().copied().take_while(|b| *b != 0).collect() } else { Vec::new() };
    TzInfo::new(be32(tt) as i32, tt[4] != 0, &name)
}

fn lookup(buf: &[u8], t: i64) -> Option<TzInfo> {
    let mut c = counts(buf)?;
    let v2 = buf[4] >= b'2';
    let mut p = &buf[44..];
    let mut len = block_len(&c, false);
    if len > p.len() {
        return None;
    }
    if v2 {
        // skip the 32-bit block
        p = &p[len..];
        c = counts(p)?;
        p = &p[44..];
        len = block_len(&c, true);
        if len > p.len() {
            return None;
        }
    }
    let ts = if v2 { 8 } else { 4 };
    let ntime = c.time as usize;
    let times = &p[..ntime * ts];
    let idx = &p[ntime * ts..ntime * ts + ntime];
    let types = &p[ntime * ts + ntime..];
    let chars = &types[c.kind as usize * 6..c.kind as usize * 6 + c.chars as usize];
    if idx.iter().any(|i| u32::from(*i) >= c.kind) {
        return None;
    }
    let mut footer: &[u8] = &[];
    if v2 && p.len() > len + 1 && p[len] == b'\n' {
        let f = &p[len + 1..];
        if let Some(e) = f.iter().position(|b| *b == b'\n') {
            if e < 128 {
                footer = &f[..e];
            }
        }
    }
    let tt = |i: usize| -> i64 {
        if v2 {
            be64(&times[i * 8..])
        } else {
            i64::from(be32(&times[i * 4..]) as i32)
        }
    };
    if ntime == 0 || t < tt(0) {
        if ntime == 0 && !footer.is_empty() {
            if let Some(info) = posix(footer, t) {
                return Some(info);
            }
        }
        return Some(fill(types, chars, 0));
    }
    if t >= tt(ntime - 1) && !footer.is_empty() {
        if let Some(info) = posix(footer, t) {
            return Some(info);
        }
    }
    let (mut lo, mut hi) = (0usize, ntime - 1); // the last transition <= t
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if tt(mid) <= t {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    Some(fill(types, chars, usize::from(idx[lo])))
}

fn zone_valid(zone: &[u8]) -> bool {
    let n = zone.len();
    if n == 0 || n > 64 || zone[0] == b'/' || zone.windows(2).any(|w| w == b".." || w == b"//") {
        return false;
    }
    zone.iter().all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(b))
}

fn zone_offset(zone: &[u8], t: i64) -> Option<TzInfo> {
    use std::io::Read;
    use std::os::unix::ffi::OsStrExt;
    if !zone_valid(zone) {
        return None;
    }
    let mut path = b"/usr/share/zoneinfo/".to_vec();
    path.extend_from_slice(zone);
    // tzdata uses links: follow them, but never outside zoneinfo
    let real = std::fs::canonicalize(std::ffi::OsStr::from_bytes(&path)).ok()?;
    if !real.as_os_str().as_bytes().starts_with(b"/usr/share/zoneinfo/") {
        return None;
    }
    let mut file = std::fs::File::open(&real).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() < 44 || meta.len() > 1 << 20 {
        return None;
    }
    let mut buf = Vec::with_capacity(meta.len() as usize);
    file.by_ref().take(meta.len()).read_to_end(&mut buf).ok()?;
    if buf.len() as u64 != meta.len() {
        return None;
    }
    lookup(&buf, t)
}

/// Local time at instant `t` (Unix seconds) by an already loaded TZif file. 0 on success.
///
/// # Safety
/// `buf` covers `n` bytes (or is null) and `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn tzif_lookup(buf: *const u8, n: usize, t: i64, out: *mut TzInfo) -> c_int {
    if buf.is_null() || out.is_null() || n > isize::MAX as usize {
        return -1;
    }
    match lookup(slice::from_raw_parts(buf, n), t) {
        Some(info) => {
            *out = info;
            0
        }
        None => -1,
    }
}

/// The same from a POSIX rule: the one in the TZif footer.
///
/// # Safety
/// `rule` is a NUL-terminated string (or null) and `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn tz_posix(rule: *const c_char, t: i64, out: *mut TzInfo) -> c_int {
    if rule.is_null() || out.is_null() {
        return -1;
    }
    match posix(CStr::from_ptr(rule).to_bytes(), t) {
        Some(info) => {
            *out = info;
            0
        }
        None => -1,
    }
}

/// A valid zone name: "Europe/Madrid", "UTC" (no "..", no leading "/", `[A-Za-z0-9/_+-]`).
///
/// # Safety
/// `zone` is a NUL-terminated string or null.
#[no_mangle]
pub unsafe extern "C" fn tz_zone_valid(zone: *const c_char) -> c_int {
    c_int::from(!zone.is_null() && zone_valid(CStr::from_ptr(zone).to_bytes()))
}

/// Reads /usr/share/zoneinfo/<zone> (a regular file up to 1 MB) and looks up `t`. 0 on success.
///
/// # Safety
/// `zone` is a NUL-terminated string or null and `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn tz_zone_offset(zone: *const c_char, t: i64, out: *mut TzInfo) -> c_int {
    if zone.is_null() || out.is_null() {
        return -1;
    }
    match zone_offset(CStr::from_ptr(zone).to_bytes(), t) {
        Some(info) => {
            *out = info;
            0
        }
        None => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(i: &TzInfo) -> String {
        let n = i.abbr.iter().position(|c| *c == 0).unwrap();
        i.abbr[..n].iter().map(|c| *c as u8 as char).collect()
    }
    // 2024-01-15 12:00 UTC, 2024-07-15 12:00 UTC
    const WINTER: i64 = 1_705_320_000;
    const SUMMER: i64 = 1_721_044_800;

    #[test]
    fn posix_rules_give_standard_and_daylight_time() {
        let madrid = posix(b"CET-1CEST,M3.5.0,M10.5.0/3", WINTER).unwrap();
        assert_eq!((madrid.utoff, madrid.isdst, name(&madrid).as_str()), (3600, 0, "CET"));
        let madrid = posix(b"CET-1CEST,M3.5.0,M10.5.0/3", SUMMER).unwrap();
        assert_eq!((madrid.utoff, madrid.isdst, name(&madrid).as_str()), (7200, 1, "CEST"));
        let ba = posix(b"<-03>3", SUMMER).unwrap();
        assert_eq!((ba.utoff, ba.isdst, name(&ba).as_str()), (-10800, 0, "-03"));
        let sydney = posix(b"AEST-10AEDT,M10.1.0,M4.1.0/3", WINTER).unwrap();
        assert_eq!((sydney.utoff, sydney.isdst), (39600, 1), "southern summer spans the new year");
        assert_eq!(posix(b"AEST-10AEDT,M10.1.0,M4.1.0/3", SUMMER).unwrap().isdst, 0);
    }
    #[test]
    fn broken_rules_are_refused() {
        for rule in ["", "CET", "CET-25", "CET-1CEST,M13.5.0,M10.5.0", "CET-1CEST,M3.5.0", "CET-1CEST,M3.5.0,M10.5.0,x", "<CET-1", "123"] {
            assert!(posix(rule.as_bytes(), WINTER).is_none(), "{rule:?}");
        }
    }
    #[test]
    fn zone_names_cannot_leave_zoneinfo() {
        assert!(zone_valid(b"Europe/Madrid") && zone_valid(b"UTC") && zone_valid(b"Etc/GMT+3"));
        for bad in ["", "/etc/passwd", "../x", "a//b", "a b", "a\\b", &"x".repeat(65)] {
            assert!(!zone_valid(bad.as_bytes()), "{bad:?}");
        }
    }
    #[test]
    fn a_tzif_file_is_read_with_its_footer_and_short_files_are_refused() {
        // version 2: no 32-bit transitions, a 64-bit block with one transition and a footer
        let mut f = Vec::new();
        let header = |time: u32, kind: u32, chars: u32, version: u8| {
            let mut h = b"TZif".to_vec();
            h.push(version);
            h.extend([0u8; 15]);
            for v in [0, 0, 0, time, kind, chars] {
                h.extend(v.to_be_bytes());
            }
            h
        };
        f.extend(header(0, 1, 2, b'2'));
        f.extend([0, 0, 0, 0, 0, 0]);
        f.extend(b"X\0");
        f.extend(header(1, 2, 8, b'2'));
        f.extend(0i64.to_be_bytes());
        f.push(1);
        f.extend(3600i32.to_be_bytes());
        f.extend([0, 0]);
        f.extend(0i32.to_be_bytes());
        f.extend([0, 4]);
        f.extend(b"STD\0DST\0");
        f.extend(b"\nCET-1CEST,M3.5.0,M10.5.0/3\n");
        let before = lookup(&f, -5).unwrap();
        assert_eq!((before.utoff, name(&before).as_str()), (3600, "STD"), "before the first transition: the first type");
        let summer = lookup(&f, SUMMER).unwrap();
        assert_eq!((summer.utoff, summer.isdst, name(&summer).as_str()), (7200, 1, "CEST"), "after the last: the footer's rule");
        assert!(lookup(&f[..30], 0).is_none() && lookup(b"nope", 0).is_none());
    }
}
