//! Codecs shared by the compositor and sandboxed helpers. C owns returned
//! buffers and frees them with free(), or sm_free() in the vault build.
#[cfg(all(feature = "secure-memory", feature = "mail"))]
compile_error!("the mail parsers belong to axia-maild, never to the vault build");
#[cfg(all(feature = "secure-memory", feature = "fonts"))]
compile_error!("vault codecs require --no-default-features; never unify the protected allocator with compositor fonts");

// Links the mail parsers' exported C symbols into axia-maild's static library.
#[cfg(feature = "mail")]
extern crate axia_mail;

mod cbor;
mod lpk;
#[cfg(not(feature = "secure-memory"))]
mod journal;
// Thai word breaks (the shell's text layout) and the Files → Network / printer packet parsers.
#[cfg(not(feature = "secure-memory"))]
mod thbrk;
#[cfg(not(feature = "secure-memory"))]
mod lanparse;
#[cfg(not(feature = "secure-memory"))]
mod mdns;
#[cfg(not(feature = "secure-memory"))]
mod lanaddr;
#[cfg(not(feature = "secure-memory"))]
mod rconnect;
#[cfg(not(feature = "secure-memory"))]
mod ipp;
mod compression;
mod qr;
mod tzif;
mod xz;
mod vgen;
#[cfg(feature = "secure-memory")]
mod secure;
// The KDBX XML reader and the CSV reader keep their data in the vault's secrets arena.
#[cfg(feature = "secure-memory")]
mod csvp;
#[cfg(feature = "secure-memory")]
mod xmlmini;

use std::{ptr, slice};

/// C callers guarantee that non-null pointers cover the supplied length.
unsafe fn input<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    if data.is_null() || len > isize::MAX as usize {
        return None;
    }
    Some(slice::from_raw_parts(data, len))
}

unsafe fn export(data: &[u8]) -> *mut u8 {
    #[cfg(feature = "secure-memory")]
    let out = secure::sm_alloc(data.len().max(1)).cast::<u8>();
    #[cfg(not(feature = "secure-memory"))]
    let out = libc::malloc(data.len().max(1)).cast::<u8>();
    if !out.is_null() {
        ptr::copy_nonoverlapping(data.as_ptr(), out, data.len());
    }
    out
}

#[no_mangle]
pub unsafe extern "C" fn axia_codec_inflate(
    data: *const u8,
    len: usize,
    max: usize,
    outlen: *mut usize,
) -> *mut u8 {
    decompress(data, len, max, outlen, false)
}
#[no_mangle]
pub unsafe extern "C" fn axia_codec_gunzip(
    data: *const u8,
    len: usize,
    max: usize,
    outlen: *mut usize,
) -> *mut u8 {
    decompress(data, len, max, outlen, true)
}
unsafe fn decompress(
    data: *const u8,
    len: usize,
    max: usize,
    outlen: *mut usize,
    gzip: bool,
) -> *mut u8 {
    if outlen.is_null() {
        return ptr::null_mut();
    }
    *outlen = 0;
    let Some(data) = input(data, len) else {
        return ptr::null_mut();
    };
    let Some(decoded) = compression::decode(data, max, gzip) else {
        return ptr::null_mut();
    };
    let out = export(&decoded);
    if !out.is_null() {
        *outlen = decoded.len();
    }
    out
}

pub mod images;

#[no_mangle]
pub unsafe extern "C" fn axia_image_decode(
    data: *const u8,
    len: usize,
    w: *mut i32,
    h: *mut i32,
    channels: i32,
    side: u32,
    pixels: u64,
) -> *mut u8 {
    if w.is_null() || h.is_null() {
        return ptr::null_mut();
    }
    *w = 0;
    *h = 0;
    if channels != 3 && channels != 4 {
        return ptr::null_mut();
    }
    let Some(data) = input(data, len) else {
        return ptr::null_mut();
    };
    let Some(image) = images::decode(data, side, pixels) else {
        return ptr::null_mut();
    };
    let (width, height) = image.dimensions();
    let out = if channels == 4 {
        export(image.as_raw())
    } else {
        export(image::DynamicImage::ImageRgba8(image).into_rgb8().as_raw())
    };
    if !out.is_null() {
        *w = width as i32;
        *h = height as i32;
    }
    out
}

#[no_mangle]
pub unsafe extern "C" fn axia_image_load(
    path: *const libc::c_char,
    w: *mut i32,
    h: *mut i32,
) -> *mut u8 {
    use std::{
        io::Read,
        os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    };
    if path.is_null() || w.is_null() || h.is_null() {
        return ptr::null_mut();
    }
    *w = 0;
    *h = 0;
    let path = std::ffi::OsStr::from_bytes(std::ffi::CStr::from_ptr(path).to_bytes());
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    else {
        return ptr::null_mut();
    };
    let Ok(meta) = file.metadata() else {
        return ptr::null_mut();
    };
    const MAX: u64 = 64 * 1024 * 1024;
    if !meta.is_file() || meta.len() > MAX {
        return ptr::null_mut();
    }
    let mut data = Vec::new();
    if file.take(MAX + 1).read_to_end(&mut data).is_err() || data.len() as u64 > MAX {
        return ptr::null_mut();
    }
    axia_image_decode(data.as_ptr(), data.len(), w, h, 4, 16384, 40_000_000)
}

pub type WriteCallback = unsafe extern "C" fn(*mut libc::c_void, *const u8, usize) -> i32;
#[no_mangle]
pub unsafe extern "C" fn axia_image_encode(
    callback: Option<WriteCallback>,
    ctx: *mut libc::c_void,
    data: *const u8,
    w: i32,
    h: i32,
    channels: i32,
    quality: i32,
) -> i32 {
    if w <= 0 || h <= 0 || !matches!(channels, 3 | 4) || (w as u64) * (h as u64) > 120_000_000 {
        return 0;
    }
    let Some(callback) = callback else {
        return 0;
    };
    let Some(len) = (w as usize)
        .checked_mul(h as usize)
        .and_then(|v| v.checked_mul(channels as usize))
    else {
        return 0;
    };
    let Some(data) = input(data, len) else {
        return 0;
    };
    let quality = if quality == 0 {
        None
    } else {
        Some(quality.clamp(1, 100) as u8)
    };
    let Some(encoded) = images::encode(data, w as u32, h as u32, channels as u8, quality) else {
        return 0;
    };
    callback(ctx, encoded.as_ptr(), encoded.len())
}
#[no_mangle]
pub unsafe extern "C" fn axia_image_write_png(
    path: *const libc::c_char,
    w: i32,
    h: i32,
    data: *const u8,
) -> i32 {
    use std::os::unix::ffi::OsStrExt;
    if path.is_null() || w <= 0 || h <= 0 || (w as u64) * (h as u64) > 120_000_000 {
        return 0;
    }
    let Some(len) = (w as usize)
        .checked_mul(h as usize)
        .and_then(|v| v.checked_mul(4))
    else {
        return 0;
    };
    let Some(data) = input(data, len) else {
        return 0;
    };
    let Some(png) = images::encode(data, w as u32, h as u32, 4, None) else {
        return 0;
    };
    let path = std::ffi::OsStr::from_bytes(std::ffi::CStr::from_ptr(path).to_bytes());
    std::fs::write(path, png).is_ok() as i32
}
#[cfg(feature = "secure-memory")]
#[no_mangle]
pub unsafe extern "C" fn axia_vault_decode_argb(
    data: *const u8,
    len: usize,
    w: *mut i32,
    h: *mut i32,
) -> *mut u32 {
    if w.is_null() || h.is_null() {
        return ptr::null_mut();
    }
    *w = 0;
    *h = 0;
    let Some(data) = input(data, len) else {
        return ptr::null_mut();
    };
    // The vault accepts only its original PNG/JPEG input formats.
    if !matches!(
        image::guess_format(data),
        Ok(image::ImageFormat::Png | image::ImageFormat::Jpeg)
    ) {
        return ptr::null_mut();
    }
    let Some(decoded) = images::decode(data, 8192, 8192 * 8192) else {
        return ptr::null_mut();
    };
    let words: Vec<u32> = decoded
        .pixels()
        .map(|p| u32::from_be_bytes([p[3], p[0], p[1], p[2]]))
        .collect();
    let out = export(slice::from_raw_parts(
        words.as_ptr().cast::<u8>(),
        words.len() * 4,
    ))
    .cast::<u32>();
    if !out.is_null() {
        *w = decoded.width() as i32;
        *h = decoded.height() as i32;
    }
    out
}
#[cfg(feature = "fonts")]
mod fonts;
#[cfg(not(feature = "secure-memory"))]
mod mail;

#[cfg(test)]
mod ffi_tests {
    use super::*;
    #[test]
    fn decoder_clears_output_sizes_on_failure() {
        unsafe {
            let (mut w, mut h) = (123, 456);
            assert!(
                axia_image_decode(ptr::null(), 10, &mut w, &mut h, 4, 8192, 40_000_000).is_null()
            );
            assert_eq!((w, h), (0, 0));
            let mut len = 123;
            assert!(axia_codec_gunzip(b"invalid".as_ptr(), 7, 100, &mut len).is_null());
            assert_eq!(len, 0);
        }
    }
    #[test]
    fn encoder_propagates_sink_failure() {
        unsafe {
            unsafe extern "C" fn fail(_: *mut libc::c_void, _: *const u8, _: usize) -> i32 {
                0
            }
            assert_eq!(
                axia_image_encode(
                    Some(fail),
                    ptr::null_mut(),
                    [1, 2, 3, 255].as_ptr(),
                    1,
                    1,
                    4,
                    0
                ),
                0
            );
        }
    }
}

// One static archive per executable keeps the vault allocator isolated.
pub use axia_crypto as crypto;
