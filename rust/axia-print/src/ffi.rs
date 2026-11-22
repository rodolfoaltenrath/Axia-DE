//! The C ABI of server/src/print/print.h, for js_print.c.
use crate::{cups, escl, http, net, tasks};
use std::ffi::{c_char, c_int, CStr};
use std::os::fd::{FromRawFd, OwnedFd};

unsafe fn bytes<'a>(s: *const c_char) -> Option<&'a [u8]> {
    if s.is_null() { None } else { Some(CStr::from_ptr(s).to_bytes()) }
}

/// Starts a request in its own thread: kind | json (len bytes) | the PDF to print (`docfd`, which
/// is closed by the task, whatever happens). The id (> 0) or -1.
///
/// # Safety
/// `kind` is a C string; `json` is NULL or covers `len` bytes; `docfd` is -1 or a descriptor that
/// the caller gives up.
#[no_mangle]
pub unsafe extern "C" fn print_task(kind: *const c_char, json: *const c_char, len: usize, docfd: c_int, doclen: usize) -> c_int {
    let doc = (docfd >= 0).then(|| tasks::DocFd { fd: OwnedFd::from_raw_fd(docfd), len: doclen });
    let Some(kind) = bytes(kind).and_then(|k| std::str::from_utf8(k).ok()) else { return -1 };
    let json = (!json.is_null() && len > 0).then(|| std::slice::from_raw_parts(json.cast::<u8>(), len).to_vec());
    tasks::task(kind, json, doc).unwrap_or(-1)
}

/// `[{id, kind, ok, data}]` (malloc: the caller frees it) or NULL.
#[no_mangle]
pub extern "C" fn print_poll() -> *mut c_char {
    let Some(s) = tasks::poll() else { return std::ptr::null_mut() };
    // SAFETY: malloc'd copy with its NUL.
    unsafe {
        let p = libc::malloc(s.len() + 1).cast::<u8>();
        if p.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        *p.add(s.len()) = 0;
        p.cast()
    }
}

#[no_mangle]
pub extern "C" fn print_pending() -> c_int {
    tasks::pending()
}

/// Is CUPS's socket there?
#[no_mangle]
pub extern "C" fn print_has_cups() -> c_int {
    std::path::Path::new(&cups::socket_path()).exists() as c_int
}

/// Makes the printers' sockets with `netconn_socket` (sec/netconn.h) instead of directly.
///
/// # Safety
/// `f` is NULL or a function with the signature of `netconn_socket`, valid for the whole run.
#[no_mangle]
pub unsafe extern "C" fn print_set_connector(f: Option<net::Connector>) {
    net::set_connector(f);
}

/// # Safety
/// `s` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn cups_valid_name(s: *const c_char) -> c_int {
    cups::valid_name(bytes(s).unwrap_or(b"")) as c_int
}

/// # Safety
/// `s` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn cups_valid_uri(s: *const c_char) -> c_int {
    cups::valid_uri(bytes(s).unwrap_or(b"")) as c_int
}

/// # Safety
/// `s` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn cups_valid_text(s: *const c_char, max: usize) -> c_int {
    cups::valid_text(bytes(s), max) as c_int
}

/// # Safety
/// `ip` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn phttp_lan_ipv4(ip: *const c_char) -> c_int {
    bytes(ip).and_then(|b| std::str::from_utf8(b).ok()).is_some_and(http::lan_ipv4) as c_int
}

/// # Safety
/// `rs` is NULL or a C string.
#[no_mangle]
pub unsafe extern "C" fn escl_valid_rs(rs: *const c_char) -> c_int {
    bytes(rs).and_then(|b| std::str::from_utf8(b).ok()).is_some_and(escl::valid_rs) as c_int
}
