//! The C ABI of server/src/hw/{power,uevent,hwscan,brframe}.h.
use crate::brframe::{self, BrfResult, Edit};
use crate::power::{self, PowerState};
use crate::{hwscan, put_c, uevent};
use std::ffi::{c_char, c_int, CStr};

unsafe fn s<'a>(p: *const c_char) -> &'a str {
    if p.is_null() { "" } else { CStr::from_ptr(p).to_str().unwrap_or("") }
}

fn malloc_bytes(v: &[u8]) -> *mut c_char {
    // SAFETY: a malloc'd buffer of the right size with its NUL.
    unsafe {
        let p = libc::malloc(v.len() + 1).cast::<u8>();
        if p.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(v.as_ptr(), p, v.len());
        *p.add(v.len()) = 0;
        p.cast()
    }
}

// ---- power ----

/// Reads every supply under `root` ("/sys/class/power_supply"). Returns `ps->n`.
///
/// # Safety
/// `root` is a C string and `ps` points to a PowerState.
#[no_mangle]
pub unsafe extern "C" fn power_read(root: *const c_char, ps: *mut PowerState) -> c_int {
    *ps = power::read(s(root));
    (*ps).n
}

/// JSON for lord.power.state() in `out` (`n` bytes): the bytes written, never more than `n - 1`.
///
/// # Safety
/// `ps` is a PowerState and `out` has room for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn power_json(ps: *const PowerState, out: *mut c_char, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let j = power::json(&*ps);
    let k = j.len().min(n - 1);
    std::ptr::copy_nonoverlapping(j.as_ptr(), out.cast::<u8>(), k);
    *out.add(k) = 0;
    k
}

// ---- uevent ----

#[no_mangle]
pub extern "C" fn uevent_open() -> c_int {
    if uevent::open() { 0 } else { -1 }
}

#[no_mangle]
pub extern "C" fn uevent_fd() -> c_int {
    uevent::fd()
}

#[no_mangle]
pub extern "C" fn uevent_dispatch() -> c_int {
    uevent::dispatch()
}

/// # Safety
/// `buf` covers `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn uevent_parse(buf: *const c_char, len: c_int) -> c_int {
    if buf.is_null() || len <= 0 {
        return 0;
    }
    uevent::parse(std::slice::from_raw_parts(buf.cast(), len as usize))
}

// ---- hwscan ----

#[repr(C)]
pub struct HwFwMiss {
    pub file: [u8; hwscan::FW_FILE],
    pub driver: [u8; hwscan::FW_DRIVER],
}

/// The facts as JSON (malloc'd), or NULL on out-of-memory.
///
/// # Safety
/// `root` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn hwscan_json(root: *const c_char) -> *mut c_char {
    malloc_bytes(&hwscan::json(s(root)))
}

unsafe fn fill(out: *mut HwFwMiss, max: c_int, found: &[hwscan::FwMiss]) -> c_int {
    let mut k = 0;
    for m in found.iter().take(max.max(0) as usize) {
        put_c(&mut (*out.add(k)).file, m.file.as_bytes());
        put_c(&mut (*out.add(k)).driver, m.driver.as_bytes());
        k += 1;
    }
    k as c_int
}

/// # Safety
/// `text` covers `len` bytes and `out` has room for `max` entries.
#[no_mangle]
pub unsafe extern "C" fn hwscan_fw_parse(text: *const c_char, len: usize, out: *mut HwFwMiss, max: c_int) -> c_int {
    if text.is_null() || max <= 0 {
        return 0;
    }
    fill(out, max, &hwscan::fw_parse(std::slice::from_raw_parts(text.cast(), len), max as usize))
}

/// # Safety
/// `name` is a C string.
#[no_mangle]
pub unsafe extern "C" fn hwscan_fw_name_valid(name: *const c_char) -> c_int {
    if name.is_null() { 0 } else { hwscan::fw_name_valid(CStr::from_ptr(name).to_bytes()) as c_int }
}

/// # Safety
/// `root` is NULL or a C string; `name` is a C string.
#[no_mangle]
pub unsafe extern "C" fn hwscan_fw_present(root: *const c_char, name: *const c_char) -> c_int {
    hwscan::fw_present(s(root), s(name)) as c_int
}

/// The kernel log ring (malloc'd, NUL-terminated), or NULL with `*denied` = 1 when the kernel refuses.
///
/// # Safety
/// `root` is NULL or a C string; `len` and `denied` point to ints.
#[no_mangle]
pub unsafe extern "C" fn hwscan_read_kmsg(root: *const c_char, len: *mut usize, denied: *mut c_int) -> *mut c_char {
    *len = 0;
    *denied = 0;
    match hwscan::read_kmsg(s(root)) {
        Ok(b) => {
            *len = b.len();
            malloc_bytes(&b)
        }
        Err(d) => {
            *denied = d as c_int;
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `root` is NULL or a C string, `text` covers `len` bytes and `out` has room for `max` entries.
#[no_mangle]
pub unsafe extern "C" fn hwscan_fw_missing(root: *const c_char, text: *const c_char, len: usize, out: *mut HwFwMiss, max: c_int) -> c_int {
    if text.is_null() || max <= 0 {
        return 0;
    }
    fill(out, max, &hwscan::fw_missing(s(root), std::slice::from_raw_parts(text.cast(), len), max as usize))
}

// ---- brframe ----

/// # Safety
/// `input` covers `n` bytes; `out` and `outn` point to a pointer and a size.
#[no_mangle]
pub unsafe extern "C" fn brf_chrome_edit(input: *const c_char, n: usize, on: c_int, out: *mut *mut c_char, outn: *mut usize) -> c_int {
    *out = std::ptr::null_mut();
    *outn = 0;
    if input.is_null() {
        return -1;
    }
    match brframe::chrome_edit(std::slice::from_raw_parts(input.cast(), n), on != 0) {
        Edit::Changed(b) => {
            *outn = b.len();
            *out = malloc_bytes(&b);
            if (*out).is_null() { -1 } else { 0 }
        }
        Edit::Same => 1,
        Edit::Bad => -1,
    }
}

/// # Safety
/// As `brf_chrome_edit`; `own` points to an int.
#[no_mangle]
pub unsafe extern "C" fn brf_userjs_edit(input: *const c_char, n: usize, value: c_int, out: *mut *mut c_char, outn: *mut usize, own: *mut c_int) -> c_int {
    *out = std::ptr::null_mut();
    *outn = 0;
    *own = 0;
    if n != 0 && input.is_null() {
        return -1;
    }
    let data: &[u8] = if n == 0 { &[] } else { std::slice::from_raw_parts(input.cast(), n) };
    let (e, o) = brframe::userjs_edit(data, value);
    *own = o as c_int;
    match e {
        Edit::Changed(b) => {
            *outn = b.len();
            *out = malloc_bytes(&b);
            if (*out).is_null() { -1 } else { 0 }
        }
        Edit::Same => 1,
        Edit::Bad => -1,
    }
}

/// # Safety
/// `input` covers `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn brf_prefs_value(input: *const c_char, n: usize) -> c_int {
    if input.is_null() { -1 } else { brframe::prefs_value(std::slice::from_raw_parts(input.cast(), n)) }
}

/// # Safety
/// `input` covers `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn brf_userjs_ours(input: *const c_char, n: usize) -> c_int {
    if input.is_null() { -1 } else { brframe::userjs_ours(std::slice::from_raw_parts(input.cast(), n)) }
}

/// # Safety
/// `cfg` is a C string and `r` points to a BrfResult.
#[no_mangle]
pub unsafe extern "C" fn brf_chromium_apply(cfg: *const c_char, on: c_int, r: *mut BrfResult) {
    *r = brframe::chromium_apply(s(cfg), on != 0);
}

/// # Safety
/// `cfg` and `home` are C strings and `r` points to a BrfResult.
#[no_mangle]
pub unsafe extern "C" fn brf_firefox_apply(cfg: *const c_char, home: *const c_char, on: c_int, r: *mut BrfResult) {
    *r = brframe::firefox_apply(s(cfg), s(home), on != 0);
}

/// # Safety
/// `cfg` and `home` are C strings.
#[no_mangle]
pub unsafe extern "C" fn brf_firefox_tidy(cfg: *const c_char, home: *const c_char) -> c_int {
    brframe::firefox_tidy(s(cfg), s(home))
}

/// # Safety
/// `cfg` is a C string.
#[no_mangle]
pub unsafe extern "C" fn brf_chromium_running(cfg: *const c_char) -> c_int {
    brframe::chromium_running(s(cfg)) as c_int
}

/// # Safety
/// `cfg` is a C string; `on` and `off` point to ints.
#[no_mangle]
pub unsafe extern "C" fn brf_chromium_state(cfg: *const c_char, on: *mut c_int, off: *mut c_int) {
    let (a, b) = brframe::chromium_state(s(cfg));
    *on = a;
    *off = b;
}

/// # Safety
/// Both point to a BrfResult.
#[no_mangle]
pub unsafe extern "C" fn brf_result_json(chromium: *const BrfResult, firefox: *const BrfResult) -> *mut c_char {
    malloc_bytes(brframe::result_json(&*chromium, &*firefox).as_bytes())
}
