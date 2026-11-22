//! The C ABI of server/src/net/net.h (the part axia-server uses): the JSON for `lord.net` and the
//! notification socket the main loop polls.
use crate::config::{self, Paths};
use crate::state;
use std::ffi::{c_char, c_int, CStr};
use std::os::fd::IntoRawFd;
use std::sync::atomic::{AtomicI32, Ordering};

static MON_FD: AtomicI32 = AtomicI32::new(-1);

/// A malloc'd copy with its NUL (the caller frees it), or NULL.
fn ret(v: Option<Vec<u8>>) -> *mut c_char {
    let Some(v) = v else { return std::ptr::null_mut() };
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

/// Opens the rtnetlink notification socket. 0, or -1.
#[no_mangle]
pub extern "C" fn net_open() -> c_int {
    match state::open_monitor() {
        Ok(s) => {
            MON_FD.store(s.0.into_raw_fd(), Ordering::SeqCst);
            0
        }
        Err(_) => {
            MON_FD.store(-1, Ordering::SeqCst);
            -1
        }
    }
}

/// For poll; -1 if there is none.
#[no_mangle]
pub extern "C" fn net_fd() -> c_int {
    MON_FD.load(Ordering::SeqCst)
}

/// There were notifications: consumes them and calls `f` once for each kind that changed.
///
/// # Safety
/// `f` is a valid function of one C string.
#[no_mangle]
pub unsafe extern "C" fn net_dispatch(f: extern "C" fn(*const c_char)) {
    let fd = net_fd();
    if fd < 0 {
        return;
    }
    state::dispatch(fd, |what| {
        let c = std::ffi::CString::new(what).unwrap();
        f(c.as_ptr());
    });
}

#[no_mangle]
pub extern "C" fn net_interfaces_json() -> *mut c_char {
    ret(state::interfaces_json(&Paths::default()))
}

#[no_mangle]
pub extern "C" fn net_routes_json() -> *mut c_char {
    ret(state::routes_json())
}

#[no_mangle]
pub extern "C" fn net_dns_json() -> *mut c_char {
    ret(Some(config::dns_json(&Paths::default())))
}

#[no_mangle]
pub extern "C" fn net_vpn_list_json() -> *mut c_char {
    ret(Some(config::dir_json_array(&Paths::default().vpn_dir)))
}

#[no_mangle]
pub extern "C" fn net_wifi_known_json() -> *mut c_char {
    ret(Some(config::dir_json_array(&Paths::default().wifi_dir)))
}

/// # Safety
/// `ifname` is a C string.
#[no_mangle]
pub unsafe extern "C" fn net_if_config_json(ifname: *const c_char) -> *mut c_char {
    if ifname.is_null() {
        return std::ptr::null_mut();
    }
    ret(config::if_config_json(&Paths::default(), CStr::from_ptr(ifname).to_bytes()))
}

/// The `.ovpn` with the files it names embedded, or NULL with the error code line in `err`.
///
/// # Safety
/// `path` is a C string and `err` has room for `errn` bytes (or is NULL).
#[no_mangle]
pub unsafe extern "C" fn net_ovpn_inline(path: *const c_char, err: *mut c_char, errn: usize) -> *mut c_char {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    match config::ovpn_inline(CStr::from_ptr(path).to_bytes(), errn) {
        Ok(text) => ret(Some(text)),
        Err(line) => {
            if !err.is_null() && errn > 0 {
                let n = line.len().min(errn - 1);
                std::ptr::copy_nonoverlapping(line.as_ptr(), err.cast::<u8>(), n);
                *err.add(n) = 0;
            }
            std::ptr::null_mut()
        }
    }
}
