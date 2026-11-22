//! The C ABI of server/src/sysreport/sysreport.h.
use crate::{collect, fetch, out, periph};
use std::ffi::{c_char, c_int, CStr};

fn s<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    // SAFETY: a C string from the caller.
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
}

/// A malloc'd copy with its NUL (the caller frees it), or NULL.
fn ret(v: String) -> *mut c_char {
    // SAFETY: a malloc'd buffer of the right size, filled before it is returned.
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

fn report(root: *const c_char, lordos: *const c_char) -> collect::Report {
    let mut r = collect::collect(s(root), None, None);
    r.sw.lordos = crate::fsutil::fit(s(lordos), 48);
    r
}

/// The report of the tree at `root` ("" or NULL: this machine) as JSON (schema 1); `lordos` is LordOS's version.
///
/// # Safety
/// `root` and `lordos` are NULL or C strings.
#[no_mangle]
pub unsafe extern "C" fn sysreport_json(root: *const c_char, lordos: *const c_char) -> *mut c_char {
    ret(out::json(&report(root, lordos)))
}

/// The same report as a Markdown summary.
///
/// # Safety
/// As `sysreport_json`.
#[no_mangle]
pub unsafe extern "C" fn sysreport_markdown(root: *const c_char, lordos: *const c_char) -> *mut c_char {
    ret(out::markdown(&report(root, lordos)))
}

/// `axia-fetch`'s output: the mark and a summary. `color`: 0 plain, 1 256 colors, 2 true color.
///
/// # Safety
/// As `sysreport_json`.
#[no_mangle]
pub unsafe extern "C" fn sysreport_fetch(root: *const c_char, lordos: *const c_char, share: c_int, color: c_int) -> *mut c_char {
    let r = report(root, lordos);
    let live = fetch::live(s(root), share != 0);
    let c = match color {
        0 => fetch::Color::Plain,
        1 => fetch::Color::C256,
        _ => fetch::Color::TrueColor,
    };
    ret(fetch::render(&r, &live, c))
}

/// How to color stdout: `opt` is "always", "never", "auto" or NULL. 0 plain, 1 256 colors, 2 true color.
///
/// # Safety
/// `opt` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn sysreport_color_mode(opt: *const c_char, is_tty: c_int) -> c_int {
    let opt = if opt.is_null() { None } else { Some(s(opt)) };
    match fetch::color_mode(opt, is_tty != 0) {
        fetch::Color::Plain => 0,
        fetch::Color::C256 => 1,
        fetch::Color::TrueColor => 2,
    }
}

/// Whether the kernel command line holds the whole word `w` (or `w=…` when `w` ends in '=').
///
/// # Safety
/// Both are C strings.
#[no_mangle]
pub unsafe extern "C" fn sysreport_cmdline_has(cmdline: *const c_char, w: *const c_char) -> c_int {
    collect::cmdline_has(s(cmdline), s(w)) as c_int
}

/// `[{ name, kind, connection, battery }]` of the peripherals (malloc'd), for the Hardware tab only.
///
/// # Safety
/// `root` and `usb_ids` are NULL or C strings.
#[no_mangle]
pub unsafe extern "C" fn periph_json(root: *const c_char, usb_ids: *const c_char) -> *mut c_char {
    ret(periph::periph_json(s(root), if usb_ids.is_null() { None } else { Some(s(usb_ids)) }))
}
