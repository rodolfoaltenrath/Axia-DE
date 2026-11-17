//! Optional system-codec ABIs for formats absent from the vendored Rust image
//! crate. All ownership, bounds, loading and conversion are Rust; these do not
//! use axia-media or ship a C/C++ adapter. libheif performs HEIF transformations.
//! ABI: strukturag/libheif v1.19.8 heif.h; libtiff TIFFReadRGBAImageOriented.
use super::io::{self, Entry};
use std::{
    ffi::{c_char, c_void, CString},
    io::Read,
    os::fd::AsRawFd,
    path::Path,
    ptr,
};
struct Library(*mut c_void);
impl Library {
    unsafe fn open(name: &std::ffi::CStr) -> Result<Self, String> {
        let h = libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if h.is_null() {
            Err("photos.codecUnavailable".into())
        } else {
            Ok(Self(h))
        }
    }
    unsafe fn symbol<T: Copy>(&self, name: &std::ffi::CStr) -> Result<T, String> {
        let p = libc::dlsym(self.0, name.as_ptr());
        if p.is_null() {
            Err("photos.codecUnavailable".into())
        } else {
            Ok(std::mem::transmute_copy(&p))
        }
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.0);
        }
    }
}
struct Object {
    p: *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
}
impl Drop for Object {
    fn drop(&mut self) {
        if !self.p.is_null() {
            unsafe { (self.free)(self.p) }
        }
    }
}
#[repr(C)]
struct Error {
    code: i32,
    subcode: i32,
    message: *const c_char,
}
fn dimensions(w: i32, h: i32) -> Result<(u32, u32), String> {
    if w <= 0 || h <= 0 || w > 32768 || h > 32768 || w as u64 * h as u64 > io::MAX_PIXELS {
        Err("photos.tooLarge".into())
    } else {
        Ok((w as u32, h as u32))
    }
}
pub fn decode(entry: &Entry, home: &Path) -> Result<image::RgbaImage, String> {
    let ext = entry
        .path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    match ext.as_str() {
        "heic" | "heif" | "avif" => heif(entry, home),
        "tif" | "tiff" => tiff(entry, home),
        _ => Err("media.imageUnreadable".into()),
    }
}
fn heif(entry: &Entry, home: &Path) -> Result<image::RgbaImage, String> {
    let mut bytes = vec![];
    io::read(entry, home)?
        .take(io::MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(io::err)?;
    if bytes.len() as u64 > io::MAX_FILE {
        return Err("photos.tooLarge".into());
    }
    unsafe {
        let l = Library::open(c"libheif.so.1")?;
        let init: unsafe extern "C" fn(*const c_void) -> Error = l.symbol(c"heif_init")?;
        let deinit: unsafe extern "C" fn() = l.symbol(c"heif_deinit")?;
        let alloc: unsafe extern "C" fn() -> *mut c_void = l.symbol(c"heif_context_alloc")?;
        let free: unsafe extern "C" fn(*mut c_void) = l.symbol(c"heif_context_free")?;
        let limit: unsafe extern "C" fn(*mut c_void, i32) =
            l.symbol(c"heif_context_set_maximum_image_size_limit")?;
        let threads: unsafe extern "C" fn(*mut c_void, i32) =
            l.symbol(c"heif_context_set_max_decoding_threads")?;
        let read: unsafe extern "C" fn(*mut c_void, *const u8, usize, *const c_void) -> Error =
            l.symbol(c"heif_context_read_from_memory_without_copy")?;
        let handle: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> Error =
            l.symbol(c"heif_context_get_primary_image_handle")?;
        let release_handle: unsafe extern "C" fn(*mut c_void) =
            l.symbol(c"heif_image_handle_release")?;
        let width: unsafe extern "C" fn(*const c_void) -> i32 =
            l.symbol(c"heif_image_handle_get_width")?;
        let height: unsafe extern "C" fn(*const c_void) -> i32 =
            l.symbol(c"heif_image_handle_get_height")?;
        let decode: unsafe extern "C" fn(
            *const c_void,
            *mut *mut c_void,
            i32,
            i32,
            *const c_void,
        ) -> Error = l.symbol(c"heif_decode_image")?;
        let release_image: unsafe extern "C" fn(*mut c_void) = l.symbol(c"heif_image_release")?;
        let plane: unsafe extern "C" fn(*const c_void, i32, *mut i32) -> *const u8 =
            l.symbol(c"heif_image_get_plane_readonly")?;
        let iw: unsafe extern "C" fn(*const c_void, i32) -> i32 =
            l.symbol(c"heif_image_get_width")?;
        let ih: unsafe extern "C" fn(*const c_void, i32) -> i32 =
            l.symbol(c"heif_image_get_height")?;
        if init(ptr::null()).code != 0 {
            return Err("media.imageUnreadable".into());
        }
        struct Init(unsafe extern "C" fn());
        impl Drop for Init {
            fn drop(&mut self) {
                unsafe { (self.0)() }
            }
        }
        let _init = Init(deinit);
        let ctx = Object { p: alloc(), free };
        if ctx.p.is_null() {
            return Err("sys.outOfMemory".into());
        }
        limit(ctx.p, 6324);
        threads(ctx.p, 1);
        if read(ctx.p, bytes.as_ptr(), bytes.len(), ptr::null()).code != 0 {
            return Err("media.imageUnreadable".into());
        }
        let mut h = Object {
            p: ptr::null_mut(),
            free: release_handle,
        };
        if handle(ctx.p, &mut h.p).code != 0 || h.p.is_null() {
            return Err("media.imageUnreadable".into());
        }
        dimensions(width(h.p), height(h.p))?;
        let mut img = Object {
            p: ptr::null_mut(),
            free: release_image,
        };
        if decode(h.p, &mut img.p, 1, 11, ptr::null()).code != 0 || img.p.is_null() {
            return Err("media.imageUnreadable".into());
        }
        let (w, h) = dimensions(iw(img.p, 10), ih(img.p, 10))?;
        let mut stride = 0;
        let p = plane(img.p, 10, &mut stride);
        if p.is_null() || stride < w as i32 * 4 {
            return Err("media.imageUnreadable".into());
        }
        let mut out = image::RgbaImage::new(w, h);
        for y in 0..h as usize {
            out.as_mut()[y * w as usize * 4..(y + 1) * w as usize * 4].copy_from_slice(
                std::slice::from_raw_parts(p.add(y * stride as usize), w as usize * 4),
            );
        }
        Ok(out)
    }
}
fn tiff(entry: &Entry, home: &Path) -> Result<image::RgbaImage, String> {
    let file = io::read(entry, home)?;
    unsafe {
        let l = Library::open(c"libtiff.so.6")?;
        let open: unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void =
            l.symbol(c"TIFFOpen")?;
        let close: unsafe extern "C" fn(*mut c_void) = l.symbol(c"TIFFClose")?;
        let field: unsafe extern "C" fn(*mut c_void, u32, ...) -> i32 =
            l.symbol(c"TIFFGetField")?;
        let set: unsafe extern "C" fn(*mut c_void, u32, ...) -> i32 = l.symbol(c"TIFFSetField")?;
        let rgba: unsafe extern "C" fn(*mut c_void, u32, u32, *mut u32, i32, i32) -> i32 =
            l.symbol(c"TIFFReadRGBAImageOriented")?;
        let path = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd())).unwrap();
        let tif = Object {
            p: open(path.as_ptr(), c"r".as_ptr()),
            free: close,
        };
        if tif.p.is_null() {
            return Err("media.imageUnreadable".into());
        }
        let (mut w, mut h) = (0u32, 0u32);
        if field(tif.p, 256, &mut w as *mut u32) == 0 || field(tif.p, 257, &mut h as *mut u32) == 0
        {
            return Err("media.imageUnreadable".into());
        }
        dimensions(w as i32, h as i32)?;
        let mut orientation = 1u16;
        field(tif.p, 274, &mut orientation as *mut u16);
        if set(tif.p, 274, 1i32) == 0 {
            return Err("media.imageUnreadable".into());
        }
        let mut pixels = vec![0u32; w as usize * h as usize];
        if rgba(tif.p, w, h, pixels.as_mut_ptr(), 1, 1) == 0 {
            return Err("media.imageUnreadable".into());
        }
        let mut out = image::RgbaImage::new(w, h);
        for (p, &v) in out.pixels_mut().zip(&pixels) {
            let a = v >> 24;
            let ch = |sh: u32| {
                if a == 0 {
                    0
                } else {
                    (((v >> sh & 255) * 255 + a / 2) / a).min(255) as u8
                }
            };
            *p = image::Rgba([ch(0), ch(8), ch(16), a as u8]);
        }
        let mut out = image::DynamicImage::ImageRgba8(out);
        out.apply_orientation(
            image::metadata::Orientation::from_exif(orientation as u8)
                .unwrap_or(image::metadata::Orientation::NoTransforms),
        );
        Ok(out.to_rgba8())
    }
}
