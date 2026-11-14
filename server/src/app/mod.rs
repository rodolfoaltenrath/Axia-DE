//! Isolated application process: package selection, launch, sandbox, Wayland and bytecode.
//! The public ABI remains app.h/probe.h/apphost_bc.h. Only QuickJS values and C
//! renderer structures cross the small bridge in ui/js_app_host.c.
// Keep the shared package codec ABI linked even in unit-test binaries.
extern crate axia_codecs;
mod bytecode;
mod client;
mod probe;
mod spawn;
#[cfg(test)]
mod tests;
mod wire;
use libc::{c_char, c_int, c_void};
use std::{
    ffi::{CStr, CString, OsStr},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};
unsafe fn path(p: *const c_char) -> PathBuf {
    if p.is_null() {
        PathBuf::new()
    } else {
        PathBuf::from(OsStr::from_bytes(CStr::from_ptr(p).to_bytes()))
    }
}
fn cpath(p: &Path) -> CString {
    CString::new(p.as_os_str().as_bytes()).expect("filesystem path has no NUL")
}
unsafe fn text<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        CStr::from_ptr(p).to_bytes()
    }
}
fn errno() -> i32 {
    unsafe { *libc::__errno_location() }
}
fn now() -> f64 {
    let mut t: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as f64 * 1000. + t.tv_nsec as f64 / 1e6
}
#[repr(C)]
struct SandboxConfig {
    id: *const c_char,
    pkg_dir: *const c_char,
    data_dir: *const c_char,
    runtime_dir: *const c_char,
    ro_extra: *const *const c_char,
    dev_files: *const *const c_char,
    network: c_int,
}
#[repr(C)]
struct SandboxHelperCfg {
    name: *const c_char,
    ro_paths: *const *const c_char,
    extra_syscalls: *const c_int,
    allow_ioctl_classes: c_int,
    mem_max_mb: libc::c_long,
    ro_dir_fds: *const c_int,
    rw_dir_fds: *const c_int,
    dev_files: *const *const c_char,
}
extern "C" {
    fn lpk_valid_id(id: *const c_char) -> c_int;
    fn lpk_root() -> *const c_char;
    fn js_sys_assets() -> *const c_char;
    fn js_sys_set_assets(p: *const c_char);
    fn codesig_init(p: *const c_char) -> c_int;
    fn codesig_state() -> c_int;
    fn codesig_load(
        p: *const c_char,
        max: usize,
        len: *mut usize,
        why: *mut c_char,
        whylen: usize,
    ) -> *mut c_char;
    fn sha256(p: *const c_void, len: usize, out: *mut u8);
    fn sandbox_apply(cfg: *const SandboxConfig) -> c_int;
    fn sandbox_apply_helper(cfg: *const SandboxHelperCfg) -> c_int;
    fn brokersrv_new_window(id: *const c_char) -> c_int;
    fn brokersrv_add(fd: c_int, id: *const c_char, pid: c_int) -> c_int;
    fn brokersrv_set_system(fd: c_int);
    fn brokersrv_set_vault(fd: c_int);
    fn vc_attach_app() -> c_int;
}
