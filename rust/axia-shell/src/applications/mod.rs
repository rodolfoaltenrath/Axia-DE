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
/// An installed desktop app as other Rust modules see it (the Axia Store).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesktopFacts {
    /// The desktop id; the shell names the app `desktop:<id>`.
    pub id: String,
    /// `Categories=` as written (`;`-separated).
    pub categories: String,
    /// A PNG path, or empty.
    pub icon: String,
}
/// The current catalog of installed desktop apps (UI thread). Only the three
/// strings are copied out, not the catalog's fixed-size records.
pub fn desktop_facts() -> Vec<DesktopFacts> {
    let text = |s: &[c_char]| {
        // SAFETY: the catalog's fixed buffers are NUL-terminated.
        unsafe { CStr::from_ptr(s.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    };
    desktop::with_list(&desktop::language(), |apps| {
        apps.iter()
            .map(|a| DesktopFacts {
                id: text(&a.id),
                categories: text(&a.categories),
                icon: text(&a.icon),
            })
            .collect()
    })
}
/// Changes only when the desktop catalog was re-read with a different list.
pub fn desktop_generation() -> u64 {
    desktop::generation(&desktop::language())
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
