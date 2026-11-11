//! iCalendar (RFC 5545), the content-line syntax shared with vCard, time
//! zones and recurrence; see server/src/mail/ical.h.

use crate::c::*;
use crate::jbuf::{self, JBuf};
use core::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::Mutex;

const IC_MAX_LINES: i64 = 200_000;
const IC_MAX_DEPTH: i32 = 8;
const IC_MAX_PROPS: c_int = 4000;
const IC_MAX_ATT: c_int = 64;

#[repr(C)]
pub struct IcProp {
    pub name: [c_char; 48],
    pub group: [c_char; 24],
    pub params: *mut c_char,
    pub value: *mut c_char,
}

#[repr(C)]
pub struct IcComp {
    pub name: [c_char; 24],
    pub props: *mut IcProp,
    pub nprops: c_int,
    pub capprops: c_int,
    pub kid: *mut IcComp,
    pub last: *mut IcComp,
    pub next: *mut IcComp,
    pub parent: *mut IcComp,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IcAttendee {
    pub email: [c_char; 256],
    pub name: [c_char; 160],
    pub partstat: [c_char; 24],
    pub role: [c_char; 24],
    pub rsvp: c_int,
}

#[repr(C)]
pub struct IcEvent {
    pub is_todo: c_int,
    pub uid: [c_char; 256],
    pub summary: [c_char; 512],
    pub location: [c_char; 512],
    pub description: *mut c_char,
    pub url: [c_char; 512],
    pub status: [c_char; 24],
    pub categories: [c_char; 256],
    pub start: i64,
    pub end: i64,
    pub allday: c_int,
    pub sdate: c_int,
    pub edate: c_int,
    pub tzid: [c_char; 64],
    pub rrule: [c_char; 512],
    pub exdate: [i64; 64],
    pub nexdate: c_int,
    pub recurrence_id: i64,
    pub alarms: [c_int; 8],
    pub nalarms: c_int,
    pub organizer: [c_char; 256],
    pub organizer_name: [c_char; 160],
    pub att: *mut IcAttendee,
    pub natt: c_int,
    pub sequence: c_int,
    pub dtstamp: i64,
    pub modified: i64,
    pub due: i64,
    pub due_allday: c_int,
    pub priority: c_int,
    pub completed: i64,
    pub flagged: c_int,
    pub parent_uid: [c_char; 256],
}

#[repr(C)]
pub struct IcBuild {
    pub is_todo: c_int,
    pub uid: *const c_char,
    pub summary: *const c_char,
    pub location: *const c_char,
    pub description: *const c_char,
    pub url: *const c_char,
    pub rrule: *const c_char,
    pub status: *const c_char,
    pub categories: *const c_char,
    pub start: i64,
    pub end: i64,
    pub allday: c_int,
    pub tzid: *const c_char,
    pub alarms: *const c_int,
    pub nalarms: c_int,
    pub due: i64,
    pub due_allday: c_int,
    pub priority: c_int,
    pub completed: i64,
    pub flagged: c_int,
    pub organizer: *const c_char,
    pub organizer_name: *const c_char,
    pub att: *const IcAttendee,
    pub natt: c_int,
    pub sequence: c_int,
    pub method: *const c_char,
    pub parent_uid: *const c_char,
    pub exdate: *const i64,
    pub nexdate: c_int,
}

#[repr(C)]
pub(crate) struct TzInfo {
    pub(crate) utoff: i32,
    pub(crate) isdst: c_int,
    pub(crate) abbr: [c_char; 16],
}

// The TZif reader stays in C (server/src/ui/tzif.c); unit tests link stubs.
#[cfg(not(test))]
unsafe extern "C" {
    fn tzif_lookup(buf: *const u8, n: usize, t: i64, out: *mut TzInfo) -> c_int;
    fn tz_zone_valid(zone: *const c_char) -> c_int;
}
#[cfg(test)]
use crate::tests::tz::{tz_zone_valid, tzif_lookup};

/// Bounded integer from untrusted text (strtol with range check; dflt otherwise).
fn bnum(s: &[u8], lo: i64, hi: i64, dflt: i32) -> (i32, usize) {
    let (v, used, over) = strtol(s);
    if used == 0 || over || v < lo || v > hi {
        (dflt, used)
    } else {
        (v as i32, used)
    }
}

// ---- content lines ----

fn comp_new(name: &[u8], parent: *mut IcComp) -> *mut IcComp {
    // SAFETY: IcComp is plain data; zero is its C initial state.
    let mut c: Box<IcComp> = Box::new(unsafe { core::mem::zeroed() });
    let up: Vec<u8> = name.iter().map(|&c| upper(c)).collect();
    putc(&mut c.name, &up);
    c.parent = parent;
    let c = Box::into_raw(c);
    if !parent.is_null() {
        unsafe {
            if (*parent).last.is_null() {
                (*parent).kid = c;
            } else {
                (*(*parent).last).next = c;
            }
            (*parent).last = c;
        }
    }
    c
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_free(root: *mut IcComp) {
    let mut n = root;
    while !n.is_null() {
        unsafe {
            if !(*n).kid.is_null() {
                let k = (*n).kid;
                (*n).kid = null_mut();
                n = k;
                continue;
            }
            let next = (*n).next;
            let parent = (*n).parent;
            let last = n == root;
            for i in 0..(*n).nprops.max(0) as usize {
                let p = &*(*n).props.add(i);
                libc::free(p.params.cast());
                libc::free(p.value.cast());
            }
            libc::free((*n).props.cast());
            drop(Box::from_raw(n));
            if last {
                break;
            }
            n = if next.is_null() { parent } else { next };
        }
    }
}

/// Adds a property; Err(()) when out of limits or memory.
fn add_prop(c: &mut IcComp, line: &[u8], total: &mut i64) -> Result<(), ()> {
    *total += 1;
    if *total > IC_MAX_PROPS as i64 * 50 {
        return Err(());
    }
    if c.nprops >= IC_MAX_PROPS {
        return Ok(());
    }
    let n = line.len();
    // name[;params]:value (':' inside a quoted parameter does not count)
    let i = line
        .iter()
        .position(|&x| x == b';' || x == b':')
        .unwrap_or(n);
    if i == n || i == 0 {
        return Ok(());
    }
    let name_end = i;
    let (mut params_start, mut colon) = (i, i);
    if line[i] == b';' {
        let mut q = false;
        colon = i;
        while colon < n {
            if line[colon] == b'"' {
                q = !q;
            } else if line[colon] == b':' && !q {
                break;
            }
            colon += 1;
        }
        if colon >= n {
            return Ok(());
        }
        params_start = i + 1;
    }
    if c.nprops == c.capprops {
        let nc = if c.capprops != 0 { c.capprops * 2 } else { 16 };
        let np = unsafe { libc::realloc(c.props.cast(), nc as usize * size_of::<IcProp>()) }
            .cast::<IcProp>();
        if np.is_null() {
            return Err(());
        }
        c.props = np;
        c.capprops = nc;
    }
    // SAFETY: plain data, zero is the C initial state.
    let mut p: IcProp = unsafe { core::mem::zeroed() };
    // group prefix ("item1.EMAIL")
    let mut nm = &line[..name_end];
    if let Some(dot) = chr(nm, b'.') {
        putc(&mut p.group, &nm[..dot]);
        nm = &nm[dot + 1..];
    }
    let nm: Vec<u8> = nm.iter().map(|&x| upper(x)).collect();
    putc(&mut p.name, &nm);
    let pl = if colon > params_start {
        colon - params_start
    } else {
        0
    };
    p.params = malloc_str(&line[params_start..params_start + pl]);
    p.value = malloc_str(&line[colon + 1..]);
    if p.params.is_null() || p.value.is_null() {
        unsafe {
            libc::free(p.params.cast());
            libc::free(p.value.cast());
        }
        return Err(());
    }
    unsafe { c.props.add(c.nprops as usize).write(p) };
    c.nprops += 1;
    Ok(())
}

fn parse(s: &[u8]) -> Result<*mut IcComp, &'static CStr> {
    let root = comp_new(b"", null_mut());
    let mut cur = root;
    let mut depth = 0;
    let (mut lines, mut total) = (0i64, 0i64);
    let mut logical: Vec<u8> = Vec::new();
    let len = s.len();
    let mut i = if len >= 3 && s[..3] == [0xef, 0xbb, 0xbf] {
        3
    } else {
        0
    };
    let fail = |m: &'static CStr| {
        unsafe { ic_free(root) };
        Err(m)
    };
    loop {
        // next physical line
        let have = i < len;
        let (a, mut b) = (i, i);
        if have {
            while b < len && s[b] != b'\n' {
                b += 1;
            }
            i = if b < len { b + 1 } else { len };
            if b > a && s[b - 1] == b'\r' {
                b -= 1;
            }
            lines += 1;
            if lines > IC_MAX_LINES {
                return fail(c"too many lines");
            }
        }
        let cont = have && b > a && (s[a] == b' ' || s[a] == b'\t');
        // vCard 2.1: quoted-printable soft line break ("=" at the end)
        let mut qp = false;
        if have && !cont && logical.last() == Some(&b'=') {
            let cl = chr(&logical, b':').unwrap_or(logical.len());
            qp = (0..cl).any(|k| k + 16 <= cl && eq_ic(&logical[k..k + 16], b"QUOTED-PRINTABLE"));
        }
        if cont || qp {
            let skip = cont as usize;
            if qp {
                logical.pop();
            } // drop the "="
            logical.extend_from_slice(&s[a + skip..b]);
            continue;
        }
        // the previous logical line is complete
        if !logical.is_empty() {
            let ll = logical.len();
            if ll >= 6 && eq_ic(&logical[..6], b"BEGIN:") {
                if depth >= IC_MAX_DEPTH {
                    return fail(c"too deep");
                }
                let mut n = ll - 6;
                while n > 0 && logical[6 + n - 1] == b' ' {
                    n -= 1;
                }
                let nm = &logical[6..6 + n.min(23)];
                cur = comp_new(z(nm), cur);
                depth += 1;
            } else if ll >= 4 && eq_ic(&logical[..4], b"END:") {
                let mut n = ll - 4;
                while n > 0 && logical[4 + n - 1] == b' ' {
                    n -= 1;
                }
                let nm = z(&logical[4..4 + n.min(23)]).to_vec();
                // close up to the matching component (tolerates missing END lines)
                let mut m = cur;
                while !m.is_null() && m != root && !eq_ic(zc(unsafe { &(*m).name }), &nm) {
                    m = unsafe { (*m).parent };
                }
                if !m.is_null() && m != root {
                    while cur != m {
                        cur = unsafe { (*cur).parent };
                        depth -= 1;
                    }
                    cur = unsafe { (*cur).parent };
                    depth -= 1;
                }
            } else if add_prop(unsafe { &mut *cur }, &logical, &mut total).is_err() {
                return fail(c"too many properties");
            }
        }
        logical.clear();
        if !have {
            break;
        }
        logical.extend_from_slice(&s[a..b]);
    }
    Ok(root)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_parse(
    text: *const c_char,
    len: usize,
    err: *mut *const c_char,
) -> *mut IcComp {
    match parse(unsafe { bytes(text, len) }) {
        Ok(r) => {
            if !err.is_null() {
                unsafe { *err = std::ptr::null() }
            }
            r
        }
        Err(m) => {
            if !err.is_null() {
                unsafe { *err = m.as_ptr() }
            }
            null_mut()
        }
    }
}

fn props<'a>(c: &'a IcComp) -> &'a [IcProp] {
    if c.props.is_null() || c.nprops <= 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(c.props, c.nprops as usize) }
    }
}

pub(crate) fn prop<'a>(c: &'a IcComp, name: &[u8]) -> Option<&'a IcProp> {
    props(c).iter().find(|p| zc(&p.name) == name)
}

/// Every property called `name`, in order.
pub(crate) fn props_named<'a>(
    c: &'a IcComp,
    name: &'a [u8],
) -> impl Iterator<Item = &'a IcProp> + 'a {
    props(c).iter().filter(move |p| zc(&p.name) == name)
}

pub(crate) fn value(p: &IcProp) -> &[u8] {
    unsafe { cs(p.value) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_prop(c: *const IcComp, name: *const c_char) -> *mut IcProp {
    match unsafe { c.as_ref() } {
        Some(c) => {
            prop(c, unsafe { cs(name) }).map_or(null_mut(), |p| (p as *const IcProp).cast_mut())
        }
        None => null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_prop_next(
    c: *const IcComp,
    after: *const IcProp,
    name: *const c_char,
) -> *mut IcProp {
    let Some(c) = (unsafe { c.as_ref() }) else {
        return null_mut();
    };
    let ps = props(c);
    let start = if after.is_null() {
        0
    } else {
        (unsafe { after.offset_from(c.props) } + 1).max(0) as usize
    };
    let name = unsafe { cs(name) };
    ps.iter()
        .skip(start)
        .find(|p| zc(&p.name) == name)
        .map_or(null_mut(), |p| (p as *const IcProp).cast_mut())
}

/// ic_param into a caller buffer, with the C semantics (TYPE accumulates, bare
/// vCard 2.1 parameters count as TYPE).
fn param_into(params: &[u8], name: &[u8], out: &mut [u8]) -> bool {
    let cap = out.len();
    if cap != 0 {
        out[0] = 0;
    }
    let is_type = eq_ic(name, b"TYPE");
    let at = |k: usize| params.get(k).copied().unwrap_or(0);
    let mut s = 0;
    while at(s) != 0 {
        let a = s;
        while at(s) != 0 && at(s) != b'=' && at(s) != b';' {
            s += 1;
        }
        let kl = s - a;
        let matched = eq_ic(&params[a..s], name);
        if at(s) != b'=' {
            if is_type && kl != 0 && cap != 0 {
                let cur = strlen(out);
                if cur + kl + 2 < cap {
                    let mut c = cur;
                    if c != 0 {
                        out[c] = b',';
                        c += 1;
                    }
                    out[c..c + kl].copy_from_slice(&params[a..s]);
                    out[c + kl] = 0;
                }
            }
            if at(s) == b';' {
                s += 1;
            }
            continue;
        }
        s += 1;
        let mut o = if matched && cap != 0 { strlen(out) } else { 0 };
        if matched && o != 0 && o + 1 < cap {
            out[o] = b',';
            o += 1;
        }
        let mut q = false;
        while at(s) != 0 && (q || at(s) != b';') {
            if at(s) == b'"' {
                q = !q;
                s += 1;
                continue;
            }
            if matched && cap != 0 && o + 1 < cap {
                out[o] = at(s);
                o += 1;
            }
            s += 1;
        }
        if matched && cap != 0 {
            out[o] = 0;
            if !is_type {
                return true;
            }
        }
        if at(s) == b';' {
            s += 1;
        }
    }
    cap != 0 && out[0] != 0
}

pub(crate) fn param(p: &IcProp, name: &[u8], cap: usize) -> (bool, Vec<u8>) {
    let mut b = vec![0u8; cap];
    let r = !p.params.is_null() && param_into(unsafe { cs(p.params) }, name, &mut b);
    (r, z(&b).to_vec())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_param(
    p: *const IcProp,
    name: *const c_char,
    out: *mut c_char,
    cap: usize,
) -> c_int {
    let o = unsafe { crate::c::out(out, cap) };
    if !o.is_empty() {
        o[0] = 0;
    }
    let Some(p) = (unsafe { p.as_ref() }) else {
        return 0;
    };
    if p.params.is_null() {
        return 0;
    }
    param_into(unsafe { cs(p.params) }, unsafe { cs(name) }, o) as c_int
}

pub(crate) fn unescape_into(input: &[u8], out: &mut [u8]) {
    let cap = out.len();
    if cap == 0 {
        return;
    }
    let mut o = 0;
    let mut k = 0;
    while k < input.len() && o + 1 < cap {
        if input[k] == b'\\' && k + 1 < input.len() {
            k += 1;
            out[o] = if input[k] == b'n' || input[k] == b'N' {
                b'\n'
            } else {
                input[k]
            };
        } else {
            out[o] = input[k];
        }
        o += 1;
        k += 1;
    }
    out[o] = 0;
    // Do not cut a UTF-8 character: only when the text was truncated, drop an
    // incomplete last sequence. (The C version stripped trailing continuation
    // bytes whenever the buffer was exactly full, so ic_unescape_dup turned a
    // value ending in "é" into a lone lead byte.)
    if k < input.len() {
        let mut j = o;
        while j > 0 && out[j - 1] & 0xc0 == 0x80 {
            j -= 1;
        }
        if j > 0 && out[j - 1] >= 0xc0 {
            let lead = out[j - 1];
            let need = if lead >= 0xf0 {
                4
            } else if lead >= 0xe0 {
                3
            } else {
                2
            };
            if o - (j - 1) < need {
                o = j - 1;
                out[o] = 0;
            }
        }
    }
}

pub(crate) fn unescape(input: &[u8], cap: usize) -> Vec<u8> {
    let mut b = vec![0u8; cap];
    unescape_into(input, &mut b);
    z(&b).to_vec()
}

pub(crate) fn unescape_all(input: &[u8]) -> Vec<u8> {
    unescape(input, input.len() + 1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_unescape(input: *const c_char, out: *mut c_char, cap: usize) {
    unescape_into(unsafe { cs(input) }, unsafe { crate::c::out(out, cap) });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_unescape_dup(input: *const c_char) -> *mut c_char {
    malloc_str(&unescape_all(unsafe { cs(input) }))
}

pub(crate) fn escape(input: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(input.len() * 2);
    for &c in input {
        if c == b'\r' {
            continue;
        }
        if c == b'\n' {
            o.extend_from_slice(b"\\n");
            continue;
        }
        if c == b'\\' || c == b',' || c == b';' {
            o.push(b'\\');
        }
        o.push(c);
    }
    o
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_escape(input: *const c_char) -> *mut c_char {
    malloc_str(&escape(unsafe { cs(input) }))
}

/// Folds a content line at 75 octets (UTF-8 safe) and appends it with CRLF.
pub(crate) fn line(b: &mut JBuf, line: &[u8]) {
    let line = z(line);
    let n = line.len();
    let mut col = 0;
    let mut i = 0;
    while i < n {
        let c = line[i];
        let mut k = if c >= 0xf0 {
            4
        } else if c >= 0xe0 {
            3
        } else if c >= 0xc0 {
            2
        } else {
            1
        };
        if i + k > n {
            k = n - i;
        }
        if col + k > 75 {
            b.raw(b"\r\n ");
            col = 1;
        }
        b.raw(&line[i..i + k]);
        col += k;
        i += k;
    }
    b.raw(b"\r\n");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_line(b: *mut JBuf, l: *const c_char) {
    if let Some(b) = unsafe { jbuf::from_ptr(b) } {
        line(b, unsafe { cs(l) })
    }
}

// ---- time ----

#[unsafe(no_mangle)]
pub extern "C" fn ic_days_from_civil(y: i64, m: c_int, d: c_int) -> i64 {
    let (m, d) = (m as i64, d as i64);
    let y = y - (m <= 2) as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub(crate) fn civil_from_days(z: i64) -> (i32, i32, i32) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let yy = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dd = (doy - (153 * mp + 2) / 5 + 1) as i32;
    let mm = (if mp < 10 { mp + 3 } else { mp - 9 }) as i32;
    ((yy + (mm <= 2) as i64) as i32, mm, dd)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_civil_from_days(z: i64, y: *mut c_int, m: *mut c_int, d: *mut c_int) {
    let (yy, mm, dd) = civil_from_days(z);
    unsafe {
        *y = yy;
        *m = mm;
        *d = dd;
    }
}

struct Zone {
    name: Vec<u8>,
    buf: Vec<u8>,
    bad: bool,
}

struct ZoneState {
    local: [u8; 64],
    cache: Vec<Zone>, // at most 24, slot 0 evicted when full
}

static ZONES: Mutex<ZoneState> = Mutex::new(ZoneState { local: *b"UTC\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0", cache: Vec::new() });

fn zones() -> std::sync::MutexGuard<'static, ZoneState> {
    ZONES.lock().unwrap_or_else(|e| e.into_inner())
}

fn zone_valid(z: &[u8]) -> bool {
    let mut c = z.to_vec();
    c.push(0);
    unsafe { tz_zone_valid(c.as_ptr().cast()) != 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_set_local_zone(z: *const c_char) {
    if z.is_null() {
        return;
    }
    let z = unsafe { cs(z) };
    if zone_valid(z) {
        put(&mut zones().local, z);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ic_local_zone() -> *const c_char {
    // The array lives in a static, so the pointer stays valid (as in C, a later
    // ic_set_local_zone changes the text it points to).
    zones().local.as_ptr().cast()
}

const WINZONES: [(&[u8], &[u8]); 43] = [
    (b"Pacific Standard Time", b"America/Los_Angeles"),
    (b"Mountain Standard Time", b"America/Denver"),
    (b"Central Standard Time", b"America/Chicago"),
    (b"Eastern Standard Time", b"America/New_York"),
    (b"GMT Standard Time", b"Europe/London"),
    (b"Greenwich Standard Time", b"Atlantic/Reykjavik"),
    (b"W. Europe Standard Time", b"Europe/Berlin"),
    (b"Romance Standard Time", b"Europe/Paris"),
    (b"Central Europe Standard Time", b"Europe/Budapest"),
    (b"Central European Standard Time", b"Europe/Warsaw"),
    (b"E. Europe Standard Time", b"Europe/Chisinau"),
    (b"FLE Standard Time", b"Europe/Kiev"),
    (b"GTB Standard Time", b"Europe/Bucharest"),
    (b"Russian Standard Time", b"Europe/Moscow"),
    (
        b"Argentina Standard Time",
        b"America/Argentina/Buenos_Aires",
    ),
    (b"E. South America Standard Time", b"America/Sao_Paulo"),
    (b"SA Pacific Standard Time", b"America/Bogota"),
    (b"Pacific SA Standard Time", b"America/Santiago"),
    (b"Central America Standard Time", b"America/Guatemala"),
    (b"Central Standard Time (Mexico)", b"America/Mexico_City"),
    (b"Mexico Standard Time", b"America/Mexico_City"),
    (b"Venezuela Standard Time", b"America/Caracas"),
    (b"Montevideo Standard Time", b"America/Montevideo"),
    (b"SA Western Standard Time", b"America/La_Paz"),
    (b"Paraguay Standard Time", b"America/Asuncion"),
    (b"Tokyo Standard Time", b"Asia/Tokyo"),
    (b"China Standard Time", b"Asia/Shanghai"),
    (b"India Standard Time", b"Asia/Kolkata"),
    (b"AUS Eastern Standard Time", b"Australia/Sydney"),
    (b"Arabian Standard Time", b"Asia/Dubai"),
    (b"Israel Standard Time", b"Asia/Jerusalem"),
    (b"Korea Standard Time", b"Asia/Seoul"),
    (b"Singapore Standard Time", b"Asia/Singapore"),
    (b"UTC", b"UTC"),
    (b"Coordinated Universal Time", b"UTC"),
    (b"Atlantic Standard Time", b"America/Halifax"),
    (b"Hawaiian Standard Time", b"Pacific/Honolulu"),
    (b"Alaskan Standard Time", b"America/Anchorage"),
    (b"US Mountain Standard Time", b"America/Phoenix"),
    (b"SE Asia Standard Time", b"Asia/Bangkok"),
    (b"New Zealand Standard Time", b"Pacific/Auckland"),
    (b"South Africa Standard Time", b"Africa/Johannesburg"),
    (b"Turkey Standard Time", b"Europe/Istanbul"),
];

/// Loads (or finds in the cache) a zone's TZif bytes; index into the cache, None if unusable.
fn zone_load(st: &mut ZoneState, zone: &[u8]) -> Option<usize> {
    let zone = if zone.is_empty() {
        z(&st.local).to_vec()
    } else {
        zone.to_vec()
    };
    let mut zn = zone[..zone.len().min(63)].to_vec();
    if let Some((_, iana)) = WINZONES.iter().find(|(w, _)| eq_ic(&zn, w)) {
        zn = iana.to_vec();
    }
    // "/mozilla.org/20050126_1/Europe/Madrid", "/freeassociation.sourceforge.net/Tzfile/Europe/Madrid"
    if zn.first() == Some(&b'/') {
        if let Some(last) = rchr(&zn, b'/') {
            match rchr(&zn[..last], b'/') {
                Some(prev) => zn = zn[prev + 1..].to_vec(),
                None => zn = zn[last + 1..].to_vec(),
            }
        }
    }
    if eq_ic(&zn, b"Z") || eq_ic(&zn, b"GMT") || eq_ic(&zn, b"Etc/UTC") {
        zn = b"UTC".to_vec();
    }
    if let Some(k) = st
        .cache
        .iter()
        .position(|c| !c.name.is_empty() && c.name == zn)
    {
        return if st.cache[k].bad { None } else { Some(k) };
    }
    let slot = if st.cache.len() < 24 {
        st.cache.push(Zone {
            name: Vec::new(),
            buf: Vec::new(),
            bad: true,
        });
        st.cache.len() - 1
    } else {
        0
    };
    st.cache[slot] = Zone {
        name: zn.clone(),
        buf: Vec::new(),
        bad: true,
    };
    if !zone_valid(&zn) {
        return None;
    }
    let mut path = b"/usr/share/zoneinfo/".to_vec();
    path.extend_from_slice(&zn);
    path.truncate(159);
    let path = <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(&path);
    let Ok(f) = std::fs::File::open(path) else {
        return None;
    };
    let Ok(md) = f.metadata() else { return None };
    if !md.is_file() || md.len() == 0 || md.len() > 1 << 20 {
        return None;
    }
    let mut buf = Vec::with_capacity(md.len() as usize);
    use std::io::Read;
    if (&f).take(md.len()).read_to_end(&mut buf).is_err() || buf.len() as u64 != md.len() {
        return None;
    }
    st.cache[slot].buf = buf;
    st.cache[slot].bad = false;
    Some(slot)
}

pub(crate) fn zone_offset(zone: &[u8], t: i64) -> i32 {
    let mut st = zones();
    let k = match zone_load(&mut st, zone) {
        Some(k) => Some(k),
        None => {
            let local = z(&st.local).to_vec();
            zone_load(&mut st, &local)
        }
    };
    let Some(k) = k else { return 0 };
    let buf = &st.cache[k].buf;
    // SAFETY: TzInfo is plain data.
    let mut ti: TzInfo = unsafe { core::mem::zeroed() };
    if unsafe { tzif_lookup(buf.as_ptr(), buf.len(), t, &mut ti) } != 0 {
        return 0;
    }
    ti.utoff
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_zone_offset(zone: *const c_char, t: i64) -> i32 {
    zone_offset(unsafe { cs(zone) }, t)
}

pub(crate) fn wall_to_utc(wall: i64, zone: &[u8]) -> i64 {
    let o1 = zone_offset(zone, wall);
    let mut t = wall - o1 as i64;
    let o2 = zone_offset(zone, t);
    if o2 != o1 {
        t = wall - o2 as i64;
    }
    t
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_wall_to_utc(wall: i64, zone: *const c_char) -> i64 {
    wall_to_utc(wall, unsafe { cs(zone) })
}

fn digits(s: &[u8], n: usize) -> Option<i32> {
    if s.len() < n {
        return None;
    }
    s[..n].iter().try_fold(0i32, |v, &c| {
        is_digit(c).then(|| v * 10 + (c - b'0') as i32)
    })
}

/// "20251018", "20251018T103000", "20251018T103000Z", vCard "1990-05-17".
/// Writes t/allday/utc in the same order as the C version (callers observe it).
fn parse_dt(v: &[u8], t: &mut i64, allday: &mut i32, utc: &mut bool) -> bool {
    let s = v.iter().position(|&c| c != b' ').unwrap_or(v.len());
    let v = &v[s..];
    let n = v.len();
    if n >= 10 && v[4] == b'-' && v[7] == b'-' {
        let (Some(y), Some(mo), Some(d)) = (digits(v, 4), digits(&v[5..], 2), digits(&v[8..], 2))
        else {
            return false;
        };
        *allday = 1;
        *utc = false;
        *t = ic_days_from_civil(y as i64, mo, d) * 86400;
        return (1..=12).contains(&mo) && (1..=31).contains(&d);
    }
    if n < 8 {
        return false;
    }
    let (Some(y), Some(mo), Some(d)) = (digits(v, 4), digits(&v[4..], 2), digits(&v[6..], 2))
    else {
        return false;
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return false;
    }
    *allday = 1;
    *utc = false;
    let (mut h, mut mi, mut sec) = (0, 0, 0);
    if n >= 15 && (v[8] == b'T' || v[8] == b't') {
        let (Some(a), Some(b), Some(c)) =
            (digits(&v[9..], 2), digits(&v[11..], 2), digits(&v[13..], 2))
        else {
            return false;
        };
        if a > 23 || b > 59 || c > 60 {
            return false;
        }
        (h, mi, sec) = (a, b, c);
        *allday = 0;
        *utc = n >= 16 && (v[15] == b'Z' || v[15] == b'z');
    }
    *t =
        ic_days_from_civil(y as i64, mo, d) * 86400 + h as i64 * 3600 + mi as i64 * 60 + sec as i64;
    true
}

/// ic_time: Some(t) on success; allday and tzid are written like the C version.
pub(crate) fn time(p: Option<&IcProp>, allday: &mut i32, tzid: Option<&mut [u8]>) -> Option<i64> {
    let mut tzid = tzid;
    if let Some(tz) = tzid.as_deref_mut() {
        if !tz.is_empty() {
            tz[0] = 0;
        }
    }
    let p = p?;
    if p.value.is_null() {
        return None;
    }
    let (_, vt) = param(p, b"VALUE", 32);
    let mut utc = false;
    let mut w = 0i64;
    if !parse_dt(value(p), &mut w, allday, &mut utc) {
        return None;
    }
    if eq_ic(&vt, b"DATE") {
        *allday = 1;
    }
    if *allday != 0 {
        return Some(wall_to_utc(w - w % 86400, b""));
    }
    if utc {
        return Some(w);
    }
    let (_, z) = param(p, b"TZID", 64);
    if let Some(tz) = tzid {
        put(tz, &z);
    }
    Some(wall_to_utc(w, &z))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ic_time(
    p: *const IcProp,
    t: *mut i64,
    allday: *mut c_int,
    tzid: *mut c_char,
    tzcap: usize,
) -> c_int {
    let tz = unsafe { crate::c::out(tzid, tzcap) };
    let mut ad = unsafe { *allday };
    let r = time(unsafe { p.as_ref() }, &mut ad, Some(tz));
    unsafe { *allday = ad };
    match r {
        Some(v) => {
            unsafe { *t = v };
            1
        }
        None => 0,
    }
}

/// ISO 8601 duration ("-PT15M", "P1DT2H", "P2W") in seconds.
fn parse_dur(v: &[u8]) -> Option<i64> {
    let s = v.iter().position(|&c| c != b' ').unwrap_or(v.len());
    let v = &v[s..];
    let at = |k: usize| v.get(k).copied().unwrap_or(0);
    let mut i = 0;
    let mut neg = false;
    if at(0) == b'-' {
        neg = true;
        i += 1;
    } else if at(0) == b'+' {
        i += 1;
    }
    if at(i) != b'P' && at(i) != b'p' {
        return None;
    }
    i += 1;
    let mut tot: i64 = 0;
    let (mut time, mut any) = (false, false);
    while at(i) != 0 {
        if at(i) == b'T' || at(i) == b't' {
            time = true;
            i += 1;
            continue;
        }
        if !is_digit(at(i)) {
            return None;
        }
        let mut x: i64 = 0;
        while is_digit(at(i)) {
            x = x * 10 + (at(i) - b'0') as i64;
            if x > 100_000_000 {
                return None;
            }
            i += 1;
        }
        let u = upper(at(i));
        if u == 0 {
            return None;
        }
        i += 1;
        any = true;
        if tot > 100_000_000_000 {
            return None;
        }
        tot += match u {
            b'W' => x * 604800,
            b'D' => x * 86400,
            b'H' if time => x * 3600,
            b'M' if time => x * 60,
            b'S' if time => x,
            _ => return None,
        };
    }
    if !any || tot > 100_000_000_000 {
        return None;
    }
    Some(if neg { -tot } else { tot })
}

// ---- events ----

fn text_prop(c: &IcComp, name: &[u8], out: &mut [c_char]) {
    let o = zc_mut(out);
    if !o.is_empty() {
        o[0] = 0;
    }
    if let Some(p) = prop(c, name) {
        unescape_into(value(p), o);
    }
}

fn mailto(v: &[u8], out: &mut [c_char]) {
    let s = v.iter().position(|&c| c != b' ').unwrap_or(v.len());
    let mut v = &v[s..];
    if starts_ic(v, b"mailto:") {
        v = &v[7..];
    }
    putc(out, &lower_all(v));
}

fn clamp_min(m: i64) -> c_int {
    m.clamp(-5_000_000, 5_000_000) as c_int
}

fn fill_event(c: &IcComp, e: &mut IcEvent) {
    // SAFETY: plain data; zero is the C initial state.
    *e = unsafe { core::mem::zeroed() };
    e.is_todo = (zc(&c.name) == b"VTODO") as c_int;
    text_prop(c, b"UID", &mut e.uid);
    text_prop(c, b"SUMMARY", &mut e.summary);
    text_prop(c, b"LOCATION", &mut e.location);
    text_prop(c, b"URL", &mut e.url);
    text_prop(c, b"STATUS", &mut e.status);
    for x in e.status.iter_mut() {
        if *x == 0 {
            break;
        }
        *x = upper(*x as u8) as c_char;
    }
    text_prop(c, b"CATEGORIES", &mut e.categories);
    e.description = malloc_str(&unescape_all(prop(c, b"DESCRIPTION").map_or(b"", value)));
    if let Some(rr) = prop(c, b"RRULE") {
        putc(&mut e.rrule, value(rr));
    }
    if let Some(seq) = prop(c, b"SEQUENCE") {
        e.sequence = bnum(value(seq), 0, 1_000_000_000, 0).0;
    }
    let mut ad = 0;
    let mut tz = [0u8; 64];
    if let Some(t) = time(prop(c, b"DTSTAMP"), &mut ad, Some(&mut tz)) {
        e.dtstamp = t;
    }
    if let Some(t) = time(prop(c, b"LAST-MODIFIED"), &mut ad, Some(&mut tz)) {
        e.modified = t;
    }
    let ds = prop(c, b"DTSTART");
    if let Some(t) = time(ds, &mut e.allday, Some(zc_mut(&mut e.tzid))) {
        e.start = t;
        if e.allday != 0 {
            let (mut w, mut a2, mut u) = (0, 0, false);
            if parse_dt(value(ds.expect("DTSTART")), &mut w, &mut a2, &mut u) {
                e.sdate = (w / 86400) as c_int;
            }
        }
    }
    let de = prop(
        c,
        if e.is_todo != 0 {
            b"DUE".as_slice()
        } else {
            b"DTEND".as_slice()
        },
    );
    if e.is_todo != 0 {
        if let Some(t) = time(de, &mut e.due_allday, Some(&mut tz)) {
            e.due = t;
        }
        if ds.is_none() {
            e.start = e.due;
            e.allday = e.due_allday;
        }
    }
    let end = if e.is_todo == 0 && de.is_some() {
        time(de, &mut ad, Some(&mut tz))
    } else {
        None
    };
    if let Some(t) = end {
        e.end = t;
        if e.allday != 0 {
            let (mut w, mut a2, mut u) = (0, 0, false);
            if parse_dt(value(de.expect("DTEND")), &mut w, &mut a2, &mut u) {
                e.edate = (w / 86400) as c_int;
            }
        }
    } else if e.is_todo == 0 {
        let dur = prop(c, b"DURATION")
            .and_then(|d| parse_dur(value(d)))
            .filter(|&d| d >= 0);
        e.end = match dur {
            Some(d) => e.start + d,
            None => e.start + if e.allday != 0 { 86400 } else { 0 },
        };
        if e.allday != 0 {
            e.edate = e.sdate + ((e.end - e.start + 86399) / 86400) as c_int;
        }
    }
    if e.allday != 0 && e.edate <= e.sdate {
        e.edate = e.sdate + 1;
    }
    if e.end < e.start {
        e.end = e.start;
    }
    if let Some(rid) = prop(c, b"RECURRENCE-ID") {
        if let Some(t) = time(Some(rid), &mut ad, Some(&mut tz)) {
            e.recurrence_id = t;
        }
    }
    for x in props_named(c, b"EXDATE") {
        // comma separated, with the same parameters
        let v = value(x);
        let mut k = 0;
        while k < v.len() && e.nexdate < 64 {
            let n = cspn(&v[k..], b",");
            if n < 40 {
                let mut one = v[k..k + n].to_vec();
                one.push(0);
                let tmp = IcProp {
                    name: x.name,
                    group: x.group,
                    params: x.params,
                    value: one.as_mut_ptr().cast(),
                };
                if let Some(t) = time(Some(&tmp), &mut ad, Some(&mut tz)) {
                    e.exdate[e.nexdate as usize] = t;
                    e.nexdate += 1;
                }
            }
            k += n;
            if v.get(k) == Some(&b',') {
                k += 1;
            }
        }
    }
    if let Some(org) = prop(c, b"ORGANIZER") {
        mailto(value(org), &mut e.organizer);
        let (ok, cn) = param(org, b"CN", 160);
        if ok {
            putc(&mut e.organizer_name, &cn);
        }
    }
    for a in props_named(c, b"ATTENDEE") {
        if e.natt >= IC_MAX_ATT {
            break;
        }
        let na = unsafe {
            libc::realloc(
                e.att.cast(),
                (e.natt as usize + 1) * size_of::<IcAttendee>(),
            )
        }
        .cast::<IcAttendee>();
        if na.is_null() {
            break;
        }
        e.att = na;
        // SAFETY: plain data, slot within the new allocation.
        let x = unsafe {
            na.add(e.natt as usize).write(core::mem::zeroed());
            &mut *na.add(e.natt as usize)
        };
        e.natt += 1;
        mailto(value(a), &mut x.email);
        putc(&mut x.name, &param(a, b"CN", 160).1);
        let (ok, ps) = param(a, b"PARTSTAT", 24);
        putc(
            &mut x.partstat,
            if ok { ps.as_slice() } else { b"NEEDS-ACTION" },
        );
        for c in x.partstat.iter_mut() {
            if *c == 0 {
                break;
            }
            *c = upper(*c as u8) as c_char;
        }
        putc(&mut x.role, &param(a, b"ROLE", 24).1);
        let (ok, rs) = param(a, b"RSVP", 16);
        x.rsvp = (ok && eq_ic(&rs, b"TRUE")) as c_int;
    }
    if let Some(pr) = prop(c, b"PRIORITY") {
        e.priority = bnum(value(pr), 0, 9, 0).0;
    }
    if let Some(co) = prop(c, b"COMPLETED") {
        if let Some(t) = time(Some(co), &mut ad, Some(&mut tz)) {
            e.completed = t;
        }
    }
    if e.completed == 0 && zc(&e.status) == b"COMPLETED" {
        e.completed = if e.modified != 0 { e.modified } else { 1 };
    }
    if let Some(fl) = prop(c, b"X-LORDOS-FLAGGED") {
        e.flagged = (bnum(value(fl), 0, 1, 0).0 != 0) as c_int;
    }
    if let Some(rel) = prop(c, b"RELATED-TO") {
        unescape_into(value(rel), zc_mut(&mut e.parent_uid));
    }
    // alarms
    let mut k = c.kid;
    while !k.is_null() && e.nalarms < 8 {
        let kc = unsafe { &*k };
        k = kc.next;
        if zc(&kc.name) != b"VALARM" {
            continue;
        }
        let Some(tr) = prop(kc, b"TRIGGER") else {
            continue;
        };
        let (_, vt) = param(tr, b"VALUE", 24);
        if eq_ic(&vt, b"DATE-TIME") {
            if let Some(at) = time(Some(tr), &mut ad, Some(&mut tz)) {
                let r = if e.is_todo != 0 && e.due != 0 {
                    e.due
                } else {
                    e.start
                };
                e.alarms[e.nalarms as usize] = clamp_min((r - at) / 60);
                e.nalarms += 1;
            }
        } else if let Some(dur) = parse_dur(value(tr)) {
            let (_, rel2) = param(tr, b"RELATED", 16);
            let mut before = -dur;
            if eq_ic(&rel2, b"END") && e.is_todo == 0 {
                before -= e.end - e.start;
            }
            e.alarms[e.nalarms as usize] = clamp_min(before / 60);
            e.nalarms += 1;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ical_events(
    text: *const c_char,
    len: usize,
    out: *mut *mut IcEvent,
    method: *mut c_char,
    mcap: usize,
) -> c_int {
    unsafe { *out = null_mut() };
    let mo = unsafe { crate::c::out(method, mcap) };
    if !mo.is_empty() {
        mo[0] = 0;
    }
    let Ok(root) = parse(unsafe { bytes(text, len) }) else {
        return -1;
    };
    let (mut n, mut cap) = (0usize, 0usize);
    let mut ev: *mut IcEvent = null_mut();
    let mut cal = unsafe { (*root).kid };
    while !cal.is_null() {
        let calc = unsafe { &*cal };
        cal = calc.next;
        if zc(&calc.name) != b"VCALENDAR" {
            continue;
        }
        if let Some(m) = prop(calc, b"METHOD") {
            if !mo.is_empty() && mo[0] == 0 {
                put(mo, value(m));
                for c in mo.iter_mut() {
                    if *c == 0 {
                        break;
                    }
                    *c = upper(*c);
                }
            }
        }
        let mut c = calc.kid;
        while !c.is_null() {
            let cc = unsafe { &*c };
            c = cc.next;
            let nm = zc(&cc.name);
            if nm != b"VEVENT" && nm != b"VTODO" {
                continue;
            }
            if n == cap {
                let nc = if cap != 0 { cap * 2 } else { 4 };
                let ne = unsafe { libc::realloc(ev.cast(), nc * size_of::<IcEvent>()) }
                    .cast::<IcEvent>();
                if ne.is_null() {
                    break;
                }
                ev = ne;
                cap = nc;
            }
            // SAFETY: slot n is within the allocation; fill_event initializes it.
            unsafe {
                ev.add(n).write(core::mem::zeroed());
                fill_event(cc, &mut *ev.add(n));
            }
            n += 1;
            if n >= 20000 {
                break;
            }
        }
    }
    unsafe {
        ic_free(root);
        *out = ev;
    }
    n as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ical_events_free(ev: *mut IcEvent, n: c_int) {
    for i in 0..n.max(0) as usize {
        unsafe {
            let e = &*ev.add(i);
            libc::free(e.description.cast());
            libc::free(e.att.cast());
        }
    }
    unsafe { libc::free(ev.cast()) };
}

// ---- recurrence ----

#[derive(Default)]
struct Rule {
    freq: u8, // 'D','W','M','Y'
    interval: i32,
    count: i32,
    until: i64,
    byday: Vec<(i32, i32)>, // weekday 0=MO..6=SU, ordinal (0 = every); at most 16
    bymonthday: Vec<i32>,   // at most 16
    bymonth: Vec<i32>,      // at most 12
}

fn wd_index(s: &[u8]) -> Option<i32> {
    const W: [&[u8]; 7] = [b"MO", b"TU", b"WE", b"TH", b"FR", b"SA", b"SU"];
    W.iter().position(|w| starts_ic(s, w)).map(|i| i as i32)
}

/// strtok semantics: non-empty pieces between delimiters.
fn tokens<'a>(s: &'a [u8], d: u8) -> impl Iterator<Item = &'a [u8]> + 'a {
    s.split(move |&c| c == d).filter(|t| !t.is_empty())
}

fn parse_rule(r: &[u8]) -> Option<Rule> {
    let mut o = Rule {
        interval: 1,
        ..Rule::default()
    };
    let r = &r[..r.len().min(511)];
    for part in tokens(r, b';') {
        let Some(eq) = chr(part, b'=') else { continue };
        let (k, v) = (&part[..eq], &part[eq + 1..]);
        if eq_ic(k, b"FREQ") {
            o.freq = if eq_ic(v, b"DAILY") {
                b'D'
            } else if eq_ic(v, b"WEEKLY") {
                b'W'
            } else if eq_ic(v, b"MONTHLY") {
                b'M'
            } else if eq_ic(v, b"YEARLY") {
                b'Y'
            } else {
                0
            };
        } else if eq_ic(k, b"INTERVAL") {
            o.interval = bnum(v, 1, 1000, 1).0;
        } else if eq_ic(k, b"COUNT") {
            o.count = bnum(v, 0, 100000, 0).0;
        } else if eq_ic(k, b"UNTIL") {
            let (mut t, mut ad, mut utc) = (0i64, 0, false);
            if parse_dt(v, &mut t, &mut ad, &mut utc) {
                o.until = if ad != 0 {
                    wall_to_utc(t + 86399, b"")
                } else if utc {
                    t
                } else {
                    wall_to_utc(t, b"")
                };
            }
        } else if eq_ic(k, b"BYDAY") {
            for d in tokens(v, b',') {
                if o.byday.len() >= 16 {
                    break;
                }
                let mut d = d;
                let mut ord = 0;
                if d[0] == b'+' || d[0] == b'-' || is_digit(d[0]) {
                    let (x, used) = bnum(d, -53, 53, 99); // RFC 5545: ±1..53
                    if x == 99 || x == 0 {
                        continue;
                    }
                    ord = x;
                    d = &d[used..];
                }
                let Some(w) = wd_index(d) else { continue };
                o.byday.push((w, ord));
            }
        } else if eq_ic(k, b"BYMONTHDAY") {
            for d in tokens(v, b',') {
                if o.bymonthday.len() >= 16 {
                    break;
                }
                let x = bnum(d, -31, 31, 0).0;
                if x != 0 {
                    o.bymonthday.push(x);
                }
            }
        } else if eq_ic(k, b"BYMONTH") {
            for d in tokens(v, b',') {
                if o.bymonth.len() >= 12 {
                    break;
                }
                let x = bnum(d, 1, 12, 0).0;
                if x != 0 {
                    o.bymonth.push(x);
                }
            }
        }
    }
    (o.freq != 0).then_some(o)
}

fn weekday_of(days: i64) -> i32 {
    (days + 3).rem_euclid(7) as i32
} // 1970-01-01 = Thursday → 0=MO

fn month_len(y: i32, m: i32) -> i32 {
    const ML: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) {
        return 0;
    } // only reachable for dates before year 0
    if m == 2 && ((y % 4 == 0 && y % 100 != 0) || y % 400 == 0) {
        29
    } else {
        ML[(m - 1) as usize]
    }
}

/// Days of a month that match the rule (ascending).
fn month_days(r: &Rule, y: i32, m: i32, dflt_day: i32, out: &mut Vec<i64>) {
    let ml = month_len(y, m);
    let first = ic_days_from_civil(y as i64, m, 1);
    let mut pick = [false; 32];
    if !r.bymonthday.is_empty() {
        for &x in &r.bymonthday {
            let d = if x > 0 { x } else { ml + x + 1 };
            if (1..=ml).contains(&d) {
                pick[d as usize] = true;
            }
        }
    } else if !r.byday.is_empty() {
        for &(w, ord) in &r.byday {
            let fw = weekday_of(first);
            let d1 = 1 + (w - fw + 7) % 7; // first such weekday
            if !(-53..=53).contains(&ord) {
                continue;
            }
            if ord == 0 {
                let mut d = d1;
                while d >= 1 && d <= ml {
                    pick[d as usize] = true;
                    d += 7;
                }
            } else if ord > 0 {
                let d = d1 + (ord - 1) * 7;
                if (1..=ml).contains(&d) {
                    pick[d as usize] = true;
                }
            } else {
                let mut last = d1;
                while last + 7 <= ml {
                    last += 7;
                }
                let d = last + (ord + 1) * 7;
                if (1..=ml).contains(&d) {
                    pick[d as usize] = true;
                }
            }
        }
    } else if (1..=ml).contains(&dflt_day) {
        pick[dflt_day as usize] = true;
    }
    for d in 1..=ml {
        if pick[d as usize] {
            out.push(first + d as i64 - 1);
        }
    }
}

fn expand(ev: &IcEvent, from: i64, to: i64, max: i32, emit: &mut dyn FnMut(i64)) -> i32 {
    let dur = ev.end - ev.start;
    let mut nout = 0;
    let tzid = zc(&ev.tzid);
    let zone: &[u8] = if ev.allday != 0 { b"" } else { tzid };
    let exdates = &ev.exdate[..ev.nexdate.clamp(0, 64) as usize];
    let mut put = |t: i64, nout: &mut i32| {
        let ex = exdates
            .iter()
            .any(|&x| x == t || (ev.allday != 0 && (x - t) / 86400 == 0 && (x - t).abs() < 86400));
        if !ex && t < to && t + if dur > 0 { dur } else { 1 } > from {
            emit(t);
            *nout += 1;
        }
    };
    let rrule = zc(&ev.rrule);
    let rule = if rrule.is_empty() || ev.recurrence_id != 0 {
        None
    } else {
        parse_rule(rrule)
    };
    let Some(r) = rule else {
        put(ev.start, &mut nout);
        return nout;
    };
    let wall0 = ev.start + zone_offset(zone, ev.start) as i64;
    let mut day0 = if wall0 >= 0 {
        wall0 / 86400
    } else {
        (wall0 - 86399) / 86400
    };
    let mut tod = wall0 - day0 * 86400;
    if ev.allday != 0 {
        day0 = ev.sdate as i64;
        tod = 0;
    }
    let (y0, m0, d0) = civil_from_days(day0);
    let mut seen = 0;
    let bymonth = |m: i32| r.bymonth.is_empty() || r.bymonth.contains(&m);
    let mut cand: Vec<i64> = Vec::with_capacity(400);
    let interval = r.interval as i64;
    let mut period: i64 = 0;
    while period < 50000 && nout < max {
        cand.clear();
        match r.freq {
            b'D' => {
                let d = day0 + period * interval;
                let (_, m, dd) = civil_from_days(d);
                if bymonth(m)
                    && (r.byday.is_empty() || r.byday.iter().any(|&(w, _)| w == weekday_of(d)))
                    && (r.bymonthday.is_empty() || r.bymonthday.contains(&dd))
                {
                    cand.push(d);
                }
            }
            b'W' => {
                let wk = day0 - weekday_of(day0) as i64 + period * 7 * interval;
                for w in 0..7 {
                    let d = wk + w as i64;
                    let on = if r.byday.is_empty() {
                        w == weekday_of(day0)
                    } else {
                        r.byday.iter().any(|&(x, _)| x == w)
                    };
                    if on {
                        let (_, m, _) = civil_from_days(d);
                        if bymonth(m) {
                            cand.push(d);
                        }
                    }
                }
            }
            b'M' => {
                let mi = y0 as i64 * 12 + (m0 - 1) as i64 + period * interval;
                let (y, m) = ((mi / 12) as i32, (mi % 12) as i32 + 1);
                if bymonth(m) {
                    month_days(&r, y, m, d0, &mut cand);
                }
            }
            _ => {
                let y = y0 as i64 + period * interval;
                if y > 9999 {
                    break;
                }
                let y = y as i32;
                if !r.bymonth.is_empty() {
                    for &bm in &r.bymonth {
                        if cand.len() + 31 > 400 {
                            break;
                        }
                        month_days(&r, y, bm, d0, &mut cand);
                    }
                } else {
                    month_days(&r, y, m0, d0, &mut cand);
                }
            }
        }
        // candidates are in ascending order per period (yearly months may not be: sort)
        cand.sort();
        let mut stop = false;
        for &cd in &cand {
            if cd < day0 {
                continue;
            }
            let mut start = if ev.allday != 0 {
                wall_to_utc(cd * 86400, b"")
            } else {
                wall_to_utc(cd * 86400 + tod, zone)
            };
            if cd == day0 {
                start = ev.start;
            }
            if r.until != 0 && start > r.until {
                stop = true;
                break;
            }
            if r.count != 0 && seen >= r.count {
                stop = true;
                break;
            }
            seen += 1;
            if start >= to {
                stop = true;
                break;
            }
            put(start, &mut nout);
            if nout >= max {
                stop = true;
                break;
            }
        }
        if stop {
            break;
        }
        period += 1;
    }
    nout
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ical_expand(
    ev: *const IcEvent,
    from: i64,
    to: i64,
    max: c_int,
    f: Option<unsafe extern "C" fn(*mut c_void, i64)>,
    u: *mut c_void,
) -> c_int {
    let ev = unsafe { &*ev };
    expand(ev, from, to, max, &mut |t| {
        if let Some(f) = f {
            unsafe { f(u, t) }
        }
    })
}

// ---- JSON ----

fn date_str(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn event_json(b: &mut JBuf, e: &IcEvent, occ: i64) {
    let start = if occ != 0 { occ } else { e.start };
    let end = start + (e.end - e.start);
    b.raw(b"{\"uid\":");
    b.str(zc(&e.uid));
    b.raw(b",\"title\":");
    b.str(zc(&e.summary));
    if e.location[0] != 0 {
        b.raw(b",\"loc\":");
        b.str(zc(&e.location));
    }
    let desc = unsafe { cs(e.description) };
    if !desc.is_empty() {
        b.raw(b",\"notes\":");
        b.str(desc);
    }
    if e.url[0] != 0 {
        b.raw(b",\"url\":");
        b.str(zc(&e.url));
    }
    if e.status[0] != 0 {
        b.raw(b",\"status\":");
        b.str(zc(&e.status));
    }
    if e.categories[0] != 0 {
        b.raw(b",\"cats\":");
        b.str(zc(&e.categories));
    }
    b.fmt(format_args!(
        ",\"start\":{},\"end\":{},\"allDay\":{}",
        start.wrapping_mul(1000),
        end.wrapping_mul(1000),
        if e.allday != 0 { "true" } else { "false" }
    ));
    if e.allday != 0 {
        let (mut sd, mut ed) = (e.sdate as i64, e.edate as i64);
        if occ != 0 {
            let w = occ + zone_offset(b"", occ) as i64;
            let nd = (if w >= 0 {
                w / 86400
            } else {
                (w - 86399) / 86400
            }) as i32 as i64;
            ed = nd + (ed - sd);
            sd = nd;
        }
        b.raw(b",\"sdate\":");
        b.str(date_str(sd as i32 as i64).as_bytes());
        b.raw(b",\"edate\":");
        b.str(date_str(ed as i32 as i64).as_bytes());
    }
    if e.tzid[0] != 0 {
        b.raw(b",\"tz\":");
        b.str(zc(&e.tzid));
    }
    if e.rrule[0] != 0 {
        b.raw(b",\"rrule\":");
        b.str(zc(&e.rrule));
    }
    if occ != 0 {
        b.fmt(format_args!(",\"occ\":{}", occ.wrapping_mul(1000)));
    }
    if e.recurrence_id != 0 {
        b.fmt(format_args!(
            ",\"rid\":{}",
            e.recurrence_id.wrapping_mul(1000)
        ));
    }
    b.raw(b",\"alarms\":[");
    for i in 0..e.nalarms.clamp(0, 8) as usize {
        b.fmt(format_args!(
            "{}{}",
            if i != 0 { "," } else { "" },
            e.alarms[i]
        ));
    }
    b.raw(b"]");
    if e.organizer[0] != 0 {
        b.raw(b",\"org\":{\"email\":");
        b.str(zc(&e.organizer));
        b.raw(b",\"name\":");
        b.str(zc(&e.organizer_name));
        b.raw(b"}");
    }
    if e.natt > 0 && !e.att.is_null() {
        b.raw(b",\"att\":[");
        for i in 0..e.natt as usize {
            let a = unsafe { &*e.att.add(i) };
            if i != 0 {
                b.raw(b",");
            }
            b.raw(b"{\"email\":");
            b.str(zc(&a.email));
            b.raw(b",\"name\":");
            b.str(zc(&a.name));
            b.raw(b",\"ps\":");
            b.str(zc(&a.partstat));
            if a.role[0] != 0 {
                b.raw(b",\"role\":");
                b.str(zc(&a.role));
            }
            b.raw(b"}");
        }
        b.raw(b"]");
    }
    b.fmt(format_args!(",\"seq\":{}", e.sequence));
    if e.is_todo != 0 {
        b.raw(b",\"todo\":true");
        if e.due != 0 {
            b.fmt(format_args!(
                ",\"due\":{},\"dueAllDay\":{}",
                e.due.wrapping_mul(1000),
                if e.due_allday != 0 { "true" } else { "false" }
            ));
            if e.due_allday != 0 {
                let w = e.due + zone_offset(b"", e.due) as i64;
                b.raw(b",\"dueDate\":");
                b.str(date_str((w / 86400) as i32 as i64).as_bytes());
            }
        } else {
            b.raw(b",\"due\":null");
        }
        b.fmt(format_args!(
            ",\"prio\":{},\"done\":{},\"flag\":{}",
            e.priority,
            if e.completed != 0 { "true" } else { "false" },
            if e.flagged != 0 { "true" } else { "false" }
        ));
        if e.completed > 1 {
            b.fmt(format_args!(
                ",\"doneAt\":{}",
                e.completed.wrapping_mul(1000)
            ));
        }
        if e.parent_uid[0] != 0 {
            b.raw(b",\"parent\":");
            b.str(zc(&e.parent_uid));
        }
    }
    b.raw(b"}");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ical_event_json(b: *mut JBuf, ev: *const IcEvent, occ: i64) {
    if let Some(b) = unsafe { jbuf::from_ptr(b) } {
        event_json(b, unsafe { &*ev }, occ)
    }
}

// ---- building ----

fn split_days(t: i64) -> (i32, i32, i32, i64) {
    let days = if t >= 0 {
        t / 86400
    } else {
        (t - 86399) / 86400
    };
    let (y, m, d) = civil_from_days(days);
    (y, m, d, t - days * 86400)
}

fn fmt_utc(t: i64) -> String {
    let (y, m, d, s) = split_days(t);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        y,
        m,
        d,
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

fn fmt_wall(t: i64, zone: &[u8]) -> String {
    let (y, m, d, s) = split_days(t + zone_offset(zone, t) as i64);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}",
        y,
        m,
        d,
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

fn fmt_date(t: i64) -> String {
    let (y, m, d, _) = split_days(t + zone_offset(b"", t) as i64);
    format!("{:04}{:02}{:02}", y, m, d)
}

fn kv(b: &mut JBuf, k: &[u8], v: &[u8]) {
    if v.is_empty() {
        return;
    }
    let mut l = k.to_vec();
    l.push(b':');
    l.extend_from_slice(&escape(v));
    line(b, &l);
}

fn safe_rrule(r: &[u8]) -> bool {
    r.iter().all(|&c| is_alnum(c) || b"=;,:+-".contains(&c))
}

/// Parameter values: no quotes, no control characters.
fn safe_param(s: &[u8], cap: usize) -> Vec<u8> {
    s.iter()
        .copied()
        .filter(|&c| c >= 0x20 && c != b'"')
        .take(cap.saturating_sub(1))
        .collect()
}

/// A POSIX TZ name: "<-03>" or letters.
fn posix_name(r: &[u8], p: &mut usize) -> Vec<u8> {
    let mut n = Vec::new();
    if r.get(*p) == Some(&b'<') {
        *p += 1;
        while *p < r.len() && r[*p] != b'>' && n.len() < 15 {
            n.push(r[*p]);
            *p += 1;
        }
        if r.get(*p) == Some(&b'>') {
            *p += 1;
        }
    } else {
        while *p < r.len() && is_alpha(r[*p]) && n.len() < 15 {
            n.push(r[*p]);
            *p += 1;
        }
    }
    n
}

/// `sscanf(s, ",M%d.%d.%d") == 3`.
fn scan_m(s: &[u8]) -> Option<(i32, i32, i32)> {
    let s = s.strip_prefix(b",M")?;
    let (a, k1) = scan_d(s)?;
    let s = s[k1..].strip_prefix(b".")?;
    let (b, k2) = scan_d(s)?;
    let s = s[k2..].strip_prefix(b".")?;
    let (c, _) = scan_d(s)?;
    Some((a, b, c))
}

/// VTIMEZONE from the zone's TZif footer (POSIX rule "CET-1CEST,M3.5.0,M10.5.0/3").
fn vtimezone(b: &mut JBuf, zone: &[u8]) -> bool {
    let rule = {
        let mut st = zones();
        let Some(k) = zone_load(&mut st, zone) else {
            return false;
        };
        let c = &st.cache[k].buf;
        if c.len() < 2 {
            return false;
        }
        // footer: "\n<rule>\n" at the end
        let e = c.len() - 1;
        if c[e] != b'\n' {
            return false;
        }
        let mut s = e;
        while s > 0 && c[s - 1] != b'\n' {
            s -= 1;
        }
        let rl = e - s;
        if rl == 0 || rl >= 80 {
            return false;
        }
        z(&c[s..e]).to_vec()
    };
    let r = rule.as_slice();
    let at = |k: usize| r.get(k).copied().unwrap_or(0);
    let mut p = 0;
    let stdn = posix_name(r, &mut p);
    let mut sign = 1;
    if at(p) == b'-' {
        sign = -1;
        p += 1;
    } else if at(p) == b'+' {
        p += 1;
    }
    if !is_digit(at(p)) {
        return false;
    }
    let (hh, used, _) = strtol(&r[p..]);
    p += used;
    let mut mm = 0i64;
    if at(p) == b':' {
        p += 1;
        let (m, used, _) = strtol(&r[p..]);
        mm = m;
        p += used;
    }
    let stdoff = (-(sign as i64) * (hh as i32 as i64 * 3600 + mm as i32 as i64 * 60)) as i32; // POSIX sign is inverted
    let mut dstoff = stdoff.wrapping_add(3600);
    let mut dst: Option<((i32, i32, i32), (i32, i32, i32))> = None;
    let mut dstn = Vec::new();
    if at(p) != 0 {
        dstn = posix_name(r, &mut p);
        if at(p) != 0 && at(p) != b',' {
            let mut sg = 1;
            if at(p) == b'-' {
                sg = -1;
                p += 1;
            } else if at(p) == b'+' {
                p += 1;
            }
            let (h2, used, _) = strtol(&r[p..]);
            p += used;
            let mut m2 = 0i64;
            if at(p) == b':' {
                p += 1;
                let (m, used, _) = strtol(&r[p..]);
                m2 = m;
                p += used;
            }
            dstoff = (-(sg as i64) * (h2 as i32 as i64 * 3600 + m2 as i32 as i64 * 60)) as i32;
        }
        if let Some(s1) = scan_m(&r[p..]) {
            let q = (p + 1..r.len()).find(|&k| r[k] == b',');
            if let Some(s2) = q.and_then(|q| scan_m(&r[q..])) {
                dst = Some((s1, s2));
            }
        }
        if dst.is_none() {
            return false;
        }
    }
    let zq = safe_param(zone, 80);
    b.raw(b"BEGIN:VTIMEZONE\r\n");
    let mut l = b"TZID:".to_vec();
    l.extend_from_slice(&zq);
    l.truncate(159);
    line(b, &l);
    const WD: [&str; 7] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];
    let off = |o: i32| {
        format!(
            "{}{:02}{:02}",
            if o < 0 { '-' } else { '+' },
            o.unsigned_abs() / 3600,
            o.unsigned_abs() / 60 % 60
        )
    };
    // names are copied as raw bytes, like printf("%s")
    let tzname = |b: &mut JBuf, n: &[u8]| {
        if !n.is_empty() {
            b.raw(b"TZNAME:");
            b.raw(n);
            b.raw(b"\r\n");
        }
    };
    match dst {
        None => {
            b.raw(b"BEGIN:STANDARD\r\nDTSTART:19700101T000000\r\n");
            b.fmt(format_args!(
                "TZOFFSETFROM:{}\r\nTZOFFSETTO:{}\r\n",
                off(stdoff),
                off(stdoff)
            ));
            tzname(b, &stdn);
            b.raw(b"END:STANDARD\r\n");
        }
        Some(((sm, sw, sd), (em, ew, ed))) => {
            let sord = if sw == 5 { -1 } else { sw };
            let eord = if ew == 5 { -1 } else { ew };
            b.raw(b"BEGIN:DAYLIGHT\r\nDTSTART:19700301T020000\r\n");
            b.fmt(format_args!(
                "TZOFFSETFROM:{}\r\nTZOFFSETTO:{}\r\n",
                off(stdoff),
                off(dstoff)
            ));
            b.fmt(format_args!(
                "RRULE:FREQ=YEARLY;BYMONTH={};BYDAY={}{}\r\n",
                sm,
                sord,
                WD[sd.rem_euclid(7) as usize]
            ));
            tzname(b, &dstn);
            b.raw(b"END:DAYLIGHT\r\nBEGIN:STANDARD\r\nDTSTART:19701001T020000\r\n");
            b.fmt(format_args!(
                "TZOFFSETFROM:{}\r\nTZOFFSETTO:{}\r\n",
                off(dstoff),
                off(stdoff)
            ));
            b.fmt(format_args!(
                "RRULE:FREQ=YEARLY;BYMONTH={};BYDAY={}{}\r\n",
                em,
                eord,
                WD[ed.rem_euclid(7) as usize]
            ));
            tzname(b, &stdn);
            b.raw(b"END:STANDARD\r\n");
        }
    }
    b.raw(b"END:VTIMEZONE\r\n");
    true
}

fn time_line(b: &mut JBuf, name: &[u8], t: i64, allday: bool, tz: &[u8]) {
    let mut l = name.to_vec();
    if allday {
        l.extend_from_slice(b";VALUE=DATE:");
        l.extend_from_slice(fmt_date(t).as_bytes());
    } else if !tz.is_empty() {
        l.extend_from_slice(b";TZID=");
        l.extend_from_slice(&safe_param(tz, 80));
        l.push(b':');
        l.extend_from_slice(fmt_wall(t, tz).as_bytes());
    } else {
        l.push(b':');
        l.extend_from_slice(fmt_utc(t).as_bytes());
    }
    l.truncate(199);
    line(b, &l);
}

fn person_line(
    b: &mut JBuf,
    prop: &[u8],
    email: &[u8],
    name: &[u8],
    ps: &[u8],
    role: &[u8],
    rsvp: bool,
) {
    let cn = safe_param(name, 200);
    let e = safe_param(email, 300);
    let mut l = prop.to_vec();
    if !cn.is_empty() {
        l.extend_from_slice(b";CN=\"");
        l.extend_from_slice(&cn);
        l.push(b'"');
    }
    if !role.is_empty() {
        l.extend_from_slice(b";ROLE=");
        l.extend_from_slice(role);
    }
    if !ps.is_empty() {
        l.extend_from_slice(b";PARTSTAT=");
        l.extend_from_slice(ps);
    }
    if rsvp {
        l.extend_from_slice(b";RSVP=TRUE");
    }
    l.extend_from_slice(b":mailto:");
    l.extend_from_slice(&e);
    l.truncate(699);
    line(b, &l);
}

fn now() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

fn build(x: &IcBuild) -> JBuf {
    let s = |p: *const c_char| unsafe { cs(p) };
    let mut b = JBuf::new();
    b.raw(b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Axia DE//Calendar 1.0//EN\r\nCALSCALE:GREGORIAN\r\n");
    let method = s(x.method);
    if !method.is_empty() {
        b.raw(b"METHOD:");
        b.put(method);
        b.raw(b"\r\n");
    }
    let mut tz: &[u8] = if !s(x.tzid).is_empty() && zone_valid(s(x.tzid)) {
        s(x.tzid)
    } else {
        b""
    };
    if !tz.is_empty() && x.allday == 0 && (x.is_todo == 0 || x.start != 0) && !vtimezone(&mut b, tz)
    {
        tz = b"";
    }
    b.raw(if x.is_todo != 0 {
        b"BEGIN:VTODO\r\n"
    } else {
        b"BEGIN:VEVENT\r\n"
    });
    kv(&mut b, b"UID", s(x.uid));
    let nw = fmt_utc(now());
    b.fmt(format_args!("DTSTAMP:{nw}\r\nLAST-MODIFIED:{nw}\r\n"));
    b.fmt(format_args!("SEQUENCE:{}\r\n", x.sequence));
    let allday = x.allday != 0;
    if x.is_todo == 0 || x.start != 0 {
        time_line(&mut b, b"DTSTART", x.start, allday, tz);
    }
    if x.is_todo == 0 {
        let end = if x.end > x.start {
            x.end
        } else {
            x.start + if allday { 86400 } else { 3600 }
        };
        time_line(&mut b, b"DTEND", end, allday, tz);
    }
    kv(&mut b, b"SUMMARY", s(x.summary));
    kv(&mut b, b"LOCATION", s(x.location));
    kv(&mut b, b"DESCRIPTION", s(x.description));
    if !s(x.url).is_empty() {
        let mut l = b"URL:".to_vec();
        l.extend_from_slice(&safe_param(s(x.url), 520));
        l.truncate(599);
        line(&mut b, &l);
    }
    // a to-do's STATUS follows "completed" (below): one STATUS only (RFC 5545)
    if x.is_todo == 0 && !s(x.status).is_empty() {
        b.raw(b"STATUS:");
        b.put(&safe_param(s(x.status), 32));
        b.raw(b"\r\n");
    }
    kv(&mut b, b"CATEGORIES", s(x.categories));
    let rr = s(x.rrule);
    if !rr.is_empty() && safe_rrule(rr) {
        let mut l = b"RRULE:".to_vec();
        l.extend_from_slice(rr);
        l.truncate(599);
        line(&mut b, &l);
    }
    for i in 0..x.nexdate.max(0) as usize {
        time_line(&mut b, b"EXDATE", unsafe { *x.exdate.add(i) }, allday, tz);
    }
    if !s(x.organizer).is_empty() {
        person_line(
            &mut b,
            b"ORGANIZER",
            s(x.organizer),
            s(x.organizer_name),
            b"",
            b"",
            false,
        );
    }
    for i in 0..x.natt.max(0) as usize {
        let a = unsafe { &*x.att.add(i) };
        let ps = if a.partstat[0] != 0 {
            zc(&a.partstat)
        } else {
            b"NEEDS-ACTION"
        };
        let role = if a.role[0] != 0 {
            zc(&a.role)
        } else {
            b"REQ-PARTICIPANT"
        };
        person_line(
            &mut b,
            b"ATTENDEE",
            zc(&a.email),
            zc(&a.name),
            ps,
            role,
            a.rsvp != 0,
        );
    }
    if x.is_todo != 0 {
        if x.due != 0 {
            time_line(&mut b, b"DUE", x.due, x.due_allday != 0, b"");
        }
        if x.priority > 0 && x.priority <= 9 {
            b.fmt(format_args!("PRIORITY:{}\r\n", x.priority));
        }
        if x.completed != 0 {
            let c = fmt_utc(if x.completed > 1 { x.completed } else { now() });
            b.fmt(format_args!(
                "STATUS:COMPLETED\r\nCOMPLETED:{c}\r\nPERCENT-COMPLETE:100\r\n"
            ));
        } else {
            b.raw(b"STATUS:NEEDS-ACTION\r\n");
        }
        if x.flagged != 0 {
            b.raw(b"X-LORDOS-FLAGGED:1\r\n");
        }
        kv(&mut b, b"RELATED-TO", s(x.parent_uid));
    }
    let summary = s(x.summary);
    for i in 0..x.nalarms.clamp(0, 8) as usize {
        let m = unsafe { *x.alarms.add(i) };
        b.raw(b"BEGIN:VALARM\r\nACTION:DISPLAY\r\n");
        kv(
            &mut b,
            b"DESCRIPTION",
            if summary.is_empty() {
                b"Reminder"
            } else {
                summary
            },
        );
        if m >= 0 {
            b.fmt(format_args!("TRIGGER:-PT{m}M\r\n"));
        } else {
            b.fmt(format_args!("TRIGGER:PT{}M\r\n", -(m as i64)));
        }
        b.raw(b"END:VALARM\r\n");
    }
    b.raw(if x.is_todo != 0 {
        b"END:VTODO\r\n"
    } else {
        b"END:VEVENT\r\n"
    });
    b.raw(b"END:VCALENDAR\r\n");
    b
}

fn hand_over(mut b: JBuf, olen: *mut usize) -> *mut c_char {
    if b.oom != 0 {
        b.free();
        return null_mut();
    }
    set_len(olen, b.n);
    b.p
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ical_build(x: *const IcBuild, olen: *mut usize) -> *mut c_char {
    hand_over(build(unsafe { &*x }), olen)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ical_reply(
    ev: *const IcEvent,
    attendee: *const c_char,
    name: *const c_char,
    partstat: *const c_char,
    olen: *mut usize,
) -> *mut c_char {
    let ev = unsafe { &*ev };
    let (attendee, partstat) = unsafe { (cs(attendee), cs(partstat)) };
    // SAFETY: plain data.
    let mut me: IcAttendee = unsafe { core::mem::zeroed() };
    putc(&mut me.email, attendee);
    if !name.is_null() {
        putc(&mut me.name, unsafe { cs(name) });
    }
    putc(&mut me.partstat, partstat);
    let no_name = name.is_null() || unsafe { *name } == 0;
    for i in 0..ev.natt.max(0) as usize {
        let a = unsafe { &*ev.att.add(i) };
        if eq_ic(zc(&a.email), attendee) {
            if no_name {
                me.name = a.name;
            }
            me.role = a.role;
        }
    }
    let x = IcBuild {
        is_todo: 0,
        uid: ev.uid.as_ptr(),
        summary: ev.summary.as_ptr(),
        location: ev.location.as_ptr(),
        description: std::ptr::null(),
        url: std::ptr::null(),
        rrule: ev.rrule.as_ptr(),
        status: std::ptr::null(),
        categories: std::ptr::null(),
        start: ev.start,
        end: ev.end,
        allday: ev.allday,
        tzid: std::ptr::null(),
        alarms: std::ptr::null(),
        nalarms: 0,
        due: 0,
        due_allday: 0,
        priority: 0,
        completed: 0,
        flagged: 0,
        organizer: ev.organizer.as_ptr(),
        organizer_name: ev.organizer_name.as_ptr(),
        att: &me,
        natt: 1,
        sequence: ev.sequence,
        method: c"REPLY".as_ptr(),
        parent_uid: std::ptr::null(),
        exdate: std::ptr::null(),
        nexdate: 0,
    };
    hand_over(build(&x), olen)
}
