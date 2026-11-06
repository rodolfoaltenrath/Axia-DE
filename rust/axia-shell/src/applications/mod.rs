//! Application discovery, MIME preferences and launch policy behind the stable C ABI.
//! C callers use the current UI-thread ABI. MIME text and preference-file buffers
//! stay owned by Rust; callers receive results through their own output buffers.
mod defaults;
mod desktop;
mod launch;
mod mime;
#[cfg(test)]
mod tests;

use std::ffi::{c_char, CStr, CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

// SAFETY: callers supply live, NUL-terminated strings and writable output buffers
// of the declared capacity. No caller-owned pointer is retained across calls.
unsafe fn bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        b""
    } else {
        CStr::from_ptr(p).to_bytes()
    }
}
unsafe fn path<'a>(p: *const c_char) -> &'a Path {
    Path::new(OsStr::from_bytes(bytes(p)))
}
fn cstring(s: &[u8]) -> CString {
    CString::new(s).unwrap_or_default()
}
unsafe fn output(p: *mut c_char, n: usize, value: &[u8]) -> i32 {
    if p.is_null() || value.len() >= n {
        return -1;
    }
    std::ptr::copy_nonoverlapping(value.as_ptr(), p.cast(), value.len());
    *p.add(value.len()) = 0;
    0
}
fn field<const N: usize>(value: &[u8]) -> [c_char; N] {
    let mut out = [0; N];
    for (d, s) in out.iter_mut().take(N.saturating_sub(1)).zip(value) {
        *d = *s as c_char;
    }
    out
}
fn value<const N: usize>(s: &[c_char; N]) -> &[u8] {
    let n = s.iter().position(|b| *b == 0).unwrap_or(N);
    // c_char and u8 have identical layout; bytes need not be UTF-8.
    unsafe { std::slice::from_raw_parts(s.as_ptr().cast(), n) }
}
fn env(name: &str) -> Option<Vec<u8>> {
    std::env::var_os(name).map(|s| s.as_bytes().to_vec())
}
fn list_has(list: &[u8], item: &[u8]) -> bool {
    list.split(|b| *b == b';').any(|s| s == item)
}
