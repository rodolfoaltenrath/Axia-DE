//! Per-app network permissions. No C implementation or C build dependency.
#![allow(clippy::missing_safety_doc)]
pub mod bpf;
pub mod compile;
pub mod diag;
pub mod dns;
pub mod policy;
pub mod service;
pub mod shared;
// Reuse the already migrated firewall engine, DNS cache and list parser without
// pulling the compositor, image codecs or fonts into the privileged service.
#[path = "../../../rust/axia-codecs/src/dnsmap.rs"]
pub mod dnsmap;
#[path = "../../../rust/axia-codecs/src/fwlists.rs"]
pub mod fwlists;
#[path = "../../../rust/axia-codecs/src/json.rs"]
pub mod json;
#[path = "../../../rust/axia-codecs/src/fwrules.rs"]
pub mod fwrules;
mod lanparse {
    pub(crate) unsafe fn cstr<'a>(p: *const libc::c_char) -> &'a [u8] {
        if p.is_null() {
            &[]
        } else {
            std::ffi::CStr::from_ptr(p).to_bytes()
        }
    }
}
// C regression fixtures call the very same Rust policy used by the kernel.
#[export_name = "np_dev_restriction"]
pub extern "C" fn c_dev(ty: u32, major: u32, hid: u32) -> u32 {
    shared::np_dev_restriction(ty, major, hid)
}
#[export_name = "np_is_lan4"]
pub unsafe extern "C" fn c_lan4(a: *const u8) -> i32 {
    if a.is_null() {
        return 0;
    }
    let mut b = [0; 16];
    std::ptr::copy_nonoverlapping(a, b.as_mut_ptr(), 4);
    shared::np_is_lan4(&b) as i32
}
#[export_name = "np_is_lan6"]
pub unsafe extern "C" fn c_lan6(a: *const u8) -> i32 {
    if a.is_null() {
        return 0;
    }
    shared::np_is_lan6(&*a.cast()) as i32
}
#[export_name = "np_is_loop"]
pub unsafe extern "C" fn c_loop(f: i32, a: *const u8) -> i32 {
    if a.is_null() || !matches!(f, 4 | 6) {
        return 0;
    }
    let mut b = [0; 16];
    std::ptr::copy_nonoverlapping(a, b.as_mut_ptr(), if f == 4 { 4 } else { 16 });
    shared::np_is_loop(f, &b) as i32
}
#[export_name = "np_ent_match"]
pub unsafe extern "C" fn c_match(e: *const shared::NpEnt, d: i32, p: i32, port: i32) -> i32 {
    e.as_ref()
        .is_some_and(|e| shared::np_ent_match(e, d, p, port)) as i32
}
#[export_name = "np_decide"]
pub unsafe extern "C" fn c_decide(
    a: *const shared::NpApp,
    l: *const shared::NpList,
    d: i32,
    p: i32,
    port: i32,
    lan: i32,
    listed: i32,
    lo: i32,
    r: *mut shared::NpRes,
) {
    if let (Some(a), Some(r)) = (a.as_ref(), r.as_mut()) {
        *r = shared::np_decide(a, l.as_ref(), d, p, port, lan, listed != 0, lo != 0)
    }
}
#[export_name = "np_ents_deny"]
pub unsafe extern "C" fn c_deny(e: *const shared::NpEnt, n: u32) -> i32 {
    if n == 0 || e.is_null() || n > 16 {
        return 0;
    }
    shared::np_ents_deny(std::slice::from_raw_parts(e, n as usize)) as i32
}
#[cfg(test)]
mod tests;

#[cfg(all(test, feature="bpf"))]
mod vm_tests;
