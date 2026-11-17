//! UI-thread image ownership. Opaque handles remain live until their references
//! are returned; the path cache owns a separate reference. No C code frees data.
use super::{
    image_sampling::{self, Pixels},
    memory,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{CStr, CString},
    ptr::{self, NonNull},
};

struct Scaled {
    pixels: Pixels,
    fit: i32,
}
// Live scene references may exceed the budget; only unused entries are evictable.
const CACHE_BYTES: u64 = 16 * 1024 * 1024;
const CACHE_ENTRIES: usize = 2048;
pub struct Image {
    pixels: Pixels,
    scales: [Option<Scaled>; 2],
    path: Option<CString>,
    refs: usize,
    cached: bool,
    managed: bool,
    used: u64,
}
impl Image {
    fn new(pixels: Pixels) -> Self {
        Self {
            pixels,
            scales: [None, None],
            path: None,
            refs: 1,
            cached: false,
            managed: false,
            used: 0,
        }
    }
    fn scaled(&mut self, w: i32, h: i32, fit: i32) -> Option<&Pixels> {
        let n = image_sampling::count(w, h)?;
        let matches = |s: &Option<Scaled>| {
            s.as_ref()
                .is_some_and(|s| s.pixels.w == w && s.pixels.h == h && s.fit == fit)
        };
        if !matches(&self.scales[0]) {
            if matches(&self.scales[1]) {
                self.scales.swap(0, 1);
            } else {
                let mut spare = self.scales[1].take();
                // Reusing capacity avoids reallocations while resizing. Retire
                // large obsolete buffers before reserving a much smaller one.
                if spare
                    .as_ref()
                    .is_some_and(|s| s.pixels.data.capacity() > n.saturating_mul(2))
                {
                    spare = None;
                }
                let old_fit = spare.as_ref().map(|s| s.fit);
                let pixels = spare.map(|s| s.pixels).unwrap_or_default();
                match self.pixels.rescale(pixels, w, h, fit) {
                    Ok(pixels) => {
                        self.scales[1] = self.scales[0].take();
                        self.scales[0] = Some(Scaled { pixels, fit });
                    }
                    Err(pixels) => {
                        self.scales[1] = old_fit.map(|fit| Scaled { pixels, fit });
                        return None;
                    }
                }
            }
        }
        Some(&self.scales[0].as_ref().unwrap().pixels)
    }
    fn bytes(&self) -> u64 {
        self.scales
            .iter()
            .flatten()
            .fold(self.pixels.bytes(), |n, s| {
                n.saturating_add(s.pixels.bytes())
            })
    }
}

/// Owns the cache's reference through a raw allocation, preserving handles
/// across hash-table moves. On TLS teardown, outstanding node references take
/// over ownership instead of being invalidated by the cache's destructor.
struct Cached(NonNull<Image>);
impl Drop for Cached {
    fn drop(&mut self) {
        unsafe {
            let image = self.0.as_mut();
            image.cached = false;
            if image.refs == 0 {
                drop(Box::from_raw(self.0.as_ptr()));
            }
        }
    }
}
struct Cache {
    paths: HashMap<CString, Cached>,
    stamp: u64,
    budget: u64,
}
impl Default for Cache {
    fn default() -> Self {
        Self { paths: HashMap::new(), stamp: 0, budget: CACHE_BYTES }
    }
}
impl Cache {
    fn bytes(&self) -> u64 {
        self.paths.iter().fold(0u64, |n, (path, entry)| {
            let image = unsafe { entry.0.as_ref() };
            n.saturating_add(image.bytes())
                .saturating_add((std::mem::size_of::<Image>() + 2 * path.to_bytes_with_nul().len()) as u64)
        })
    }
    fn trim(&mut self) {
        let mut bytes = self.bytes();
        while bytes > self.budget || self.paths.len() > CACHE_ENTRIES {
            let Some(pointer) = self.paths.values()
                .filter(|e| unsafe { e.0.as_ref().refs == 0 })
                .min_by_key(|e| unsafe { e.0.as_ref().used })
                .map(|e| e.0) else { break };
            // No key allocation, including during allocation-failure recovery.
            self.paths.retain(|path, entry| {
                if entry.0 != pointer { return true; }
                let image = unsafe { entry.0.as_ref() };
                bytes = bytes.saturating_sub(image.bytes()
                    + (std::mem::size_of::<Image>() + 2 * path.to_bytes_with_nul().len()) as u64);
                false
            });
        }
    }
    fn get(&mut self, path: &CStr, decode: impl FnOnce() -> Option<Pixels>) -> *mut Image {
        self.stamp = self.stamp.saturating_add(1);
        if let Some(entry) = self.paths.get_mut(path) {
            let image = unsafe { entry.0.as_mut() };
            let Some(refs) = image.refs.checked_add(1) else {
                return ptr::null_mut();
            };
            image.refs = refs;
            image.used = self.stamp;
            return entry.0.as_ptr();
        }
        if !memory::try_reserve_map(&mut self.paths) {
            return ptr::null_mut();
        }
        let Some(key) = memory::try_cstring(path) else {
            return ptr::null_mut();
        };
        let Some(owned_path) = memory::try_cstring(path) else {
            return ptr::null_mut();
        };
        let Some(pixels) = decode() else {
            return ptr::null_mut();
        };
        let mut image = Image::new(pixels);
        image.managed = [
            b"/axia/wallpapers/".as_slice(),
            b"/axia/tray/",
            b"/axia/search/",
        ]
        .iter()
        .any(|part| path.to_bytes().windows(part.len()).any(|p| p == *part));
        image.cached = true;
        image.used = self.stamp;
        image.path = Some(owned_path);
        let Some(image) = memory::try_box(image) else {
            return ptr::null_mut();
        };
        let pointer = NonNull::new(Box::into_raw(image)).unwrap();
        self.paths.insert(key, Cached(pointer));
        self.trim();
        pointer.as_ptr()
    }
    fn managed_bytes(&self) -> u64 {
        self.paths
            .values()
            .map(|entry| unsafe { entry.0.as_ref() })
            .filter(|image| image.managed)
            .fold(0u64, |n, image| n.saturating_add(image.bytes()))
    }
    unsafe fn release(&mut self, pointer: *mut Image) {
        let Some(image) = pointer.as_mut() else {
            return;
        };
        debug_assert!(image.refs > 0);
        image.refs -= 1;
        if image.refs != 0 {
            return;
        }
        self.stamp = self.stamp.saturating_add(1);
        image.used = self.stamp;
        if !image.cached {
            drop(Box::from_raw(pointer));
        } else if image.managed {
            // The query is owned by Image, not the hash key. Keep the removed
            // entry alive until the lookup's borrow has ended, then reclaim it.
            let path = image.path.as_deref().unwrap();
            let entry = self.paths.remove(path);
            debug_assert!(entry.is_some());
            drop(entry);
        }
        self.trim();
    }
}
thread_local! { static IMAGES: RefCell<Cache> = RefCell::new(Cache::default()); }

#[no_mangle]
pub unsafe extern "C" fn image_release(image: *mut Image) {
    if image.is_null() {
        return;
    }
    if !(*image).cached {
        // Does not touch TLS: a cache may already have been destroyed before
        // the owning scene's destructor runs on this thread.
        debug_assert!((*image).refs > 0);
        (*image).refs -= 1;
        if (*image).refs == 0 {
            drop(Box::from_raw(image));
        }
    } else {
        IMAGES.with(|cache| cache.borrow_mut().release(image));
    }
}
#[no_mangle]
pub unsafe extern "C" fn image_size(image: *const Image, w: *mut i32, h: *mut i32) {
    let (width, height) = image.as_ref().map_or((0, 0), |i| (i.pixels.w, i.pixels.h));
    if !w.is_null() {
        w.write(width);
    }
    if !h.is_null() {
        h.write(height);
    }
}
#[no_mangle]
pub extern "C" fn image_managed_bytes() -> u64 {
    IMAGES.with(|cache| cache.borrow().managed_bytes())
}
#[no_mangle]
pub extern "C" fn image_cache_bytes() -> u64 {
    IMAGES.with(|cache| cache.borrow().bytes())
}

enum Input<'a> {
    Rgba(&'a [u8]),
    Argb(&'a [u32]),
}
impl Input<'_> {
    fn write(self, pixels: &mut Pixels) {
        match self {
            Self::Rgba(p) => pixels.rgba(p),
            Self::Argb(p) => pixels.argb(p),
        }
    }
}
unsafe fn replace(image: *mut Image, w: i32, h: i32, input: Input<'_>) -> *mut Image {
    if let Some(old) = image.as_mut() {
        if old.path.is_none() && old.pixels.w == w && old.pixels.h == h {
            input.write(&mut old.pixels);
            old.scales = [None, None];
            return image;
        }
    }
    let Some(mut pixels) = Pixels::empty(w, h) else {
        return ptr::null_mut();
    };
    input.write(&mut pixels);
    let Some(new) = memory::try_box(Image::new(pixels)) else {
        return ptr::null_mut();
    };
    let new = Box::into_raw(new);
    // Only a completed replacement consumes the previous reference.
    image_release(image);
    new
}
/// Adopt an opaque worker-rendered frame in premultiplied ARGB; no
/// conversion, source clone or image decoding occurs on the scene thread.
pub(crate) unsafe fn owned_argb(image: *mut Image, w: i32, h: i32, data: Vec<u32>) -> *mut Image {
    if image_sampling::count(w, h) != Some(data.len()) { return image; }
    let pixels = Pixels { w, h, data, opaque: true };
    if let Some(old) = image.as_mut().filter(|i| i.path.is_none() && i.refs == 1) {
        old.pixels = pixels;
        old.scales = [None, None];
        return image;
    }
    let Some(new) = memory::try_box(Image::new(pixels)) else { return image; };
    let new = Box::into_raw(new);
    image_release(image);
    new
}
#[no_mangle]
pub unsafe extern "C" fn image_pixels(
    image: *mut Image,
    w: i32,
    h: i32,
    rgba: *const u8,
) -> *mut Image {
    let Some(n) = image_sampling::count(w, h) else {
        return ptr::null_mut();
    };
    if rgba.is_null() {
        return ptr::null_mut();
    }
    replace(
        image,
        w,
        h,
        Input::Rgba(std::slice::from_raw_parts(rgba, n * 4)),
    )
}
#[no_mangle]
pub unsafe extern "C" fn image_argb(
    image: *mut Image,
    w: i32,
    h: i32,
    argb: *const u32,
) -> *mut Image {
    let Some(n) = image_sampling::count(w, h) else {
        return ptr::null_mut();
    };
    if argb.is_null() {
        return ptr::null_mut();
    }
    replace(
        image,
        w,
        h,
        Input::Argb(std::slice::from_raw_parts(argb, n)),
    )
}

#[cfg(not(test))]
fn decode(path: &CStr) -> Option<Pixels> {
    use std::{
        fs::OpenOptions,
        io::Read,
        os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        path::Path,
    };
    const MAX_FILE: usize = 64 * 1024 * 1024;
    let path = Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let info = file.metadata().ok()?;
    if !info.is_file() || info.len() > MAX_FILE as u64 {
        return None;
    }
    let mut data = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        if data.len().checked_add(n)? > MAX_FILE || !memory::try_reserve(&mut data, n) {
            return None;
        }
        data.extend_from_slice(&chunk[..n]);
    }
    let decoded = axia_codecs::images::decode(&data, 32768, 64 * 1024 * 1024)?;
    let (w, h) = (decoded.width() as i32, decoded.height() as i32);
    image_sampling::count(w, h)?;
    let mut pixels = Pixels::empty(w, h)?;
    pixels.rgba(decoded.as_raw());
    Some(pixels)
}
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn image_get(path: *const std::ffi::c_char) -> *mut Image {
    if path.is_null() {
        return ptr::null_mut();
    }
    let path = CStr::from_ptr(path);
    let image = IMAGES.with(|cache| cache.borrow_mut().get(path, || decode(path)));
    if image.is_null() {
        eprintln!("image: could not load {path:?}");
    }
    image
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Clip {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}
#[repr(C)]
#[cfg(not(test))]
pub struct Canvas {
    px: *mut u32,
    w: i32,
    h: i32,
    stride: i32,
    clip: Clip,
}
struct Draw {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}
fn draw_bounds(clip: Clip, x: f32, y: f32, w: f32, h: f32) -> Option<Draw> {
    // Validate before casts, additions, reservations or touching image pixels.
    if clip.x0 >= clip.x1 || clip.y0 >= clip.y1 || ![x, y, w, h].into_iter().all(f32::is_finite) {
        return None;
    }
    let (x, y, w, h) = (x.round(), y.round(), w.round(), h.round());
    if x < i32::MIN as f32 || x >= i32::MAX as f32 || y < i32::MIN as f32 || y >= i32::MAX as f32 {
        return None;
    }
    if w <= 0. || h <= 0. || w > 32768. || h > 32768. {
        return None;
    }
    let (x, y, w, h) = (x as i32, y as i32, w as i32, h as i32);
    image_sampling::count(w, h)?;
    if x >= clip.x1
        || y >= clip.y1
        || i64::from(x) + i64::from(w) <= i64::from(clip.x0)
        || i64::from(y) + i64::from(h) <= i64::from(clip.y0)
    {
        return None;
    }
    // canvas_blit does int arithmetic on these sums.
    x.checked_add(w)?;
    y.checked_add(h)?;
    Some(Draw { x, y, w, h })
}
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn image_draw(
    canvas: *mut Canvas,
    image: *mut Image,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    fit: i32,
    opacity: f32,
) {
    unsafe extern "C" {
        fn canvas_blit(
            canvas: *mut Canvas,
            src: *const u32,
            w: i32,
            h: i32,
            x: i32,
            y: i32,
            radius: f32,
            opacity: f32,
            opaque: i32,
        );
    }
    if canvas.is_null()
        || image.is_null()
        || !radius.is_finite()
        || !opacity.is_finite()
        || opacity <= 0.
    {
        return;
    }
    let Some(draw) = draw_bounds((*canvas).clip, x, y, w, h) else {
        return;
    };
    let image = &mut *image;
    let before = image.bytes();
    let pixels = if draw.w == image.pixels.w && draw.h == image.pixels.h {
        &image.pixels
    } else {
        let Some(pixels) = image.scaled(draw.w, draw.h, fit) else {
            // Painting happens after damage_walk cleared the dirty bits. Keep
            // the affected nodes eligible for a new frame, including cached
            // transformed/drag layers; an allocation failure is temporary.
            super::with_scene(|scene, ui| scene.retry_image(image as *mut Image, ui));
            return;
        };
        pixels
    };
    canvas_blit(
        canvas,
        pixels.data.as_ptr(),
        pixels.w,
        pixels.h,
        draw.x,
        draw.y,
        radius.max(0.),
        opacity.clamp(0., 1.),
        i32::from(pixels.opaque),
    );
    // Scaling can grow entries after insertion. Trim only when storage changed,
    // after canvas_blit has finished borrowing the pixels.
    if image.cached && image.bytes() != before {
        IMAGES.with(|cache| cache.borrow_mut().trim());
    }
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;
