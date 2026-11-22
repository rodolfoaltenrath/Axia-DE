//! The C ABI of server/src/backup/backup.h, for `main.c` and `js_backup.c`.
//! Everything that returns text returns it in `malloc` memory (the caller frees it) or NULL; the
//! reason of a failure goes to `err` as a code line (server/src/errcode.h).
use crate::service;
use std::ffi::{c_char, c_int, CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

unsafe fn bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        CStr::from_ptr(p).to_bytes()
    }
}

unsafe fn text(p: *const c_char) -> String {
    String::from_utf8_lossy(bytes(p)).into_owned()
}

fn c_copy(data: &[u8]) -> *mut c_char {
    unsafe {
        let p = libc::malloc(data.len() + 1) as *mut u8;
        if p.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        *p.add(data.len()) = 0;
        p.cast()
    }
}

unsafe fn put_err(err: *mut c_char, n: usize, msg: &[u8]) {
    if err.is_null() || n == 0 {
        return;
    }
    let k = msg.len().min(n - 1);
    std::ptr::copy_nonoverlapping(msg.as_ptr(), err.cast::<u8>(), k);
    *err.add(k) = 0;
}

unsafe fn result(r: Result<(), Vec<u8>>, err: *mut c_char, n: usize) -> c_int {
    match r {
        Ok(()) => 0,
        Err(e) => {
            put_err(err, n, &e);
            -1
        }
    }
}

fn opt(data: Option<Vec<u8>>) -> *mut c_char {
    data.map_or(std::ptr::null_mut(), |d| c_copy(&d))
}

#[no_mangle]
pub unsafe extern "C" fn backup_start(home: *const c_char, user: *const c_char) -> c_int {
    if home.is_null() || user.is_null() {
        return -1;
    }
    if service::start(Path::new(OsStr::from_bytes(bytes(home))), &text(user)) {
        0
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn backup_fd() -> c_int {
    service::event_fd()
}

#[no_mangle]
pub unsafe extern "C" fn backup_dispatch(f: Option<unsafe extern "C" fn(*const c_char)>) {
    let Some(f) = f else { return };
    service::dispatch(|json| {
        let p = c_copy(json);
        if !p.is_null() {
            f(p);
            libc::free(p.cast());
        }
    });
}

#[no_mangle]
pub extern "C" fn backup_status_json() -> *mut c_char {
    opt(service::status_json())
}

#[no_mangle]
pub extern "C" fn backup_config_json() -> *mut c_char {
    opt(service::config_json())
}

#[no_mangle]
pub unsafe extern "C" fn backup_configure(json: *const c_char, err: *mut c_char, n: usize) -> c_int {
    result(service::configure(bytes(json)), err, n)
}

#[no_mangle]
pub unsafe extern "C" fn backup_run_now(err: *mut c_char, n: usize) -> c_int {
    result(service::run_now(), err, n)
}

#[no_mangle]
pub extern "C" fn backup_pause(on: c_int) {
    service::pause(on != 0);
}

#[no_mangle]
pub extern "C" fn backup_cancel() {
    service::cancel();
}

#[no_mangle]
pub extern "C" fn backup_list_json() -> *mut c_char {
    opt(service::list_json())
}

#[no_mangle]
pub unsafe extern "C" fn backup_ls_json(id: *const c_char, rel: *const c_char, err: *mut c_char, n: usize) -> *mut c_char {
    match service::ls_json(&text(id), &text(rel)) {
        Ok(json) => c_copy(&json),
        Err(e) => {
            put_err(err, n, &e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn backup_restore(id: *const c_char, rel: *const c_char, replace: c_int, suffix: *const c_char, to: *const c_char, err: *mut c_char, n: usize) -> c_int {
    let to = Path::new(OsStr::from_bytes(bytes(to)));
    result(service::restore(&text(id), &text(rel), replace != 0, bytes(suffix), to), err, n)
}

#[no_mangle]
pub unsafe extern "C" fn backup_delete(id: *const c_char, err: *mut c_char, n: usize) -> c_int {
    result(service::delete(&text(id)), err, n)
}

#[no_mangle]
pub unsafe extern "C" fn backup_unlock(pass: *const c_char, create: c_int, err: *mut c_char, n: usize) -> c_int {
    result(service::unlock(bytes(pass), create != 0), err, n)
}

#[no_mangle]
pub extern "C" fn backup_lock() {
    service::lock();
}

#[no_mangle]
pub unsafe extern "C" fn backup_snapshot_path(id: *const c_char, out: *mut c_char, n: usize) -> c_int {
    let Some(real) = service::snapshot_path(&text(id)) else { return -1 };
    let b = real.as_os_str().as_bytes();
    if out.is_null() || b.len() >= n {
        return -1;
    }
    put_err(out, n, b);
    0
}
