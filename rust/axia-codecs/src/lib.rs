//! Codecs shared by the compositor and sandboxed helpers. C owns returned
//! buffers and frees them with free(), or sm_free() in the vault build.
mod compression;
#[cfg(feature = "secure-memory")]
mod secure;

use std::{ptr, slice};

/// C callers guarantee that non-null pointers cover the supplied length.
unsafe fn input<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    if data.is_null() || len > isize::MAX as usize { return None; }
    Some(slice::from_raw_parts(data, len))
}

unsafe fn export(data: &[u8]) -> *mut u8 {
    #[cfg(feature = "secure-memory")]
    let out = secure::sm_alloc(data.len().max(1)).cast::<u8>();
    #[cfg(not(feature = "secure-memory"))]
    let out = libc::malloc(data.len().max(1)).cast::<u8>();
    if !out.is_null() { ptr::copy_nonoverlapping(data.as_ptr(), out, data.len()); }
    out
}

#[no_mangle]
pub unsafe extern "C" fn axia_codec_inflate(data: *const u8, len: usize, max: usize, outlen: *mut usize) -> *mut u8 {
    decompress(data, len, max, outlen, false)
}
#[no_mangle]
pub unsafe extern "C" fn axia_codec_gunzip(data: *const u8, len: usize, max: usize, outlen: *mut usize) -> *mut u8 {
    decompress(data, len, max, outlen, true)
}
unsafe fn decompress(data: *const u8, len: usize, max: usize, outlen: *mut usize, gzip: bool) -> *mut u8 {
    if outlen.is_null() { return ptr::null_mut(); }
    *outlen = 0;
    let Some(data) = input(data, len) else { return ptr::null_mut(); };
    let Some(decoded) = compression::decode(data, max, gzip) else { return ptr::null_mut(); };
    let out = export(&decoded);
    if !out.is_null() { *outlen = decoded.len(); }
    out
}
