//! Native clipboard storage and nonblocking transfer engines. The compositor
//! calls these APIs on its event-loop thread; protocol resources never cross FFI.
use std::time::{Duration, Instant};
use std::{
    cell::RefCell,
    ffi::{c_char, CStr, CString},
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    ptr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
const LIMIT: usize = 16 << 20;
const MEMORY: usize = 64 << 20;
const STEP: usize = 256 << 10;
static MEMORY_USED: AtomicUsize = AtomicUsize::new(0);
static TRANSFERS: AtomicUsize = AtomicUsize::new(0);
fn bounded_add(counter: &AtomicUsize, amount: usize, limit: usize) -> bool {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let Some(next) = current.checked_add(amount).filter(|v| *v <= limit) else {
            return false;
        };
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
}
fn charge(n: usize) -> bool {
    bounded_add(&MEMORY_USED, n, MEMORY)
}
struct Chunk([u8; 65536]);
impl Drop for Chunk {
    fn drop(&mut self) {
        unsafe {
            libc::explicit_bzero(self.0.as_mut_ptr().cast(), self.0.len());
        }
    }
}
#[derive(Default)]
struct Bytes {
    data: Vec<u8>,
    charged: usize,
}
impl Drop for Bytes {
    fn drop(&mut self) {
        unsafe {
            libc::explicit_bzero(self.data.as_mut_ptr().cast(), self.data.len());
        }
        MEMORY_USED.fetch_sub(self.charged, Ordering::Relaxed);
    }
}
impl Bytes {
    fn reserve(&mut self, needed: usize) -> bool {
        if needed <= self.data.capacity() {
            return true;
        }
        if needed > LIMIT + 1 {
            return false;
        }
        let want = needed
            .max(self.data.capacity().saturating_mul(2))
            .max(8192)
            .min(LIMIT + 1);
        if !charge(want) {
            return false;
        }
        let mut next = Self {
            data: Vec::new(),
            charged: want,
        };
        if next.data.try_reserve_exact(want).is_err() {
            return false;
        }
        let actual = next.data.capacity();
        if actual > want && !charge(actual - want) {
            return false;
        }
        next.charged = actual;
        next.data.extend_from_slice(&self.data);
        std::mem::swap(self, &mut next); // scrub before freeing the old allocation
        true
    }
    fn append(&mut self, bytes: &[u8]) -> bool {
        if !self.reserve(self.data.len() + bytes.len()) {
            return false;
        }
        self.data.extend_from_slice(bytes);
        true
    }
    fn copy(bytes: &[u8]) -> Option<Self> {
        let mut b = Self::default();
        b.append(bytes).then_some(b)
    }
}
struct Text {
    bytes: Arc<Bytes>,
    len: usize,
}
impl Text {
    fn take(mut b: Bytes) -> Option<Self> {
        let len = b.data.iter().position(|b| *b == 0).unwrap_or(b.data.len());
        if !b.append(&[0]) {
            return None;
        }
        Some(Self {
            bytes: Arc::new(b),
            len,
        })
    }
    fn ptr(&self) -> *const c_char {
        self.bytes.data.as_ptr().cast()
    }
}
struct Native {
    runtime: Option<PathBuf>,
    text: Option<Text>,
    text_gen: u32,
    origin: CString,
    sensitive: bool,
    png: Option<Arc<Bytes>>,
    image_gen: u32,
    image_active: bool,
    image_path: CString,
    image_origin: CString,
    image_sensitive: bool,
    primary: Option<Text>,
    primary_gen: u32,
    offer_gen: u32,
    epochs: [u64; 2],
}
impl Default for Native {
    fn default() -> Self {
        Self {
            runtime: std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
            text: None,
            text_gen: 0,
            origin: c"terminal".into(),
            sensitive: false,
            png: None,
            image_gen: 0,
            image_active: false,
            image_path: CString::default(),
            image_origin: c"linux".into(),
            image_sensitive: false,
            primary: None,
            primary_gen: 0,
            offer_gen: 1,
            epochs: [0; 2],
        }
    }
}
thread_local! { static NATIVE: RefCell<Native> = RefCell::new(Native::default()); }
fn origin(value: &[u8], default: &[u8], max: usize) -> CString {
    let value = if value.is_empty() { default } else { value };
    CString::new(&value[..value.len().min(max)]).unwrap_or_default()
}
impl Native {
    fn invalidate(&mut self, primary: bool) {
        let i = primary as usize;
        self.epochs[i] = self.epochs[i].wrapping_add(1);
    }
    fn text(&mut self, b: Bytes, name: &[u8], sensitive: bool, primary: bool) -> bool {
        let Some(text) = Text::take(b) else {
            return false;
        };
        if primary {
            self.primary = Some(text);
            self.primary_gen = self.primary_gen.wrapping_add(1);
        } else {
            self.image_active = false;
            self.text = Some(text);
            self.text_gen = self.text_gen.wrapping_add(1);
            self.origin = origin(name, b"terminal", 63);
            self.sensitive = sensitive;
        }
        true
    }
    fn image(&mut self, b: Bytes, name: &[u8], sensitive: bool) -> bool {
        if b.data.len() <= 8 || !b.data.starts_with(b"\x89PNG\r\n\x1a\n") {
            return false;
        }
        self.image_gen = self.image_gen.wrapping_add(1);
        self.image_path =
            preview(&b.data, self.image_gen, self.runtime.as_deref()).unwrap_or_default();
        self.png = Some(Arc::new(b));
        self.image_active = true;
        self.image_origin = origin(name, b"linux", 127);
        self.image_sensitive = sensitive;
        true
    }
}
// Same runtime preview names as the existing image/history adapters. No decoded
// surface or second PNG allocation; opening a file pins it for the history worker.
fn directory(path: &Path) -> std::io::Result<()> {
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => (),
            Ok(_) => return Err(std::io::ErrorKind::InvalidInput.into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(&current)?
            }
            Err(e) => return Err(e),
        }
    }
    if fs::metadata(path)?.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}
fn preview(bytes: &[u8], generation: u32, runtime: Option<&Path>) -> Option<CString> {
    let runtime = runtime?;
    if !runtime.is_absolute() {
        return None;
    }
    let dir = runtime.join("axia");
    directory(&dir).ok()?;
    let path = dir.join(format!("clip-{generation}.png"));
    // Unlink first: do not truncate a hard link placed at a predictable name.
    if fs::symlink_metadata(&path).is_ok() {
        fs::remove_file(&path).ok()?;
    }
    let result = (|| {
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        out.write_all(bytes)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&path);
        return None;
    }
    if generation > 10 {
        let _ = fs::remove_file(dir.join(format!("clip-{}.png", generation - 10)));
    }
    CString::new(path.as_os_str().as_encoded_bytes()).ok()
}
struct Permit;
impl Permit {
    fn new() -> Option<Self> {
        bounded_add(&TRANSFERS, 1, 32).then(|| Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        TRANSFERS.fetch_sub(1, Ordering::Relaxed);
    }
}
pub struct NativeReader {
    fd: OwnedFd,
    bytes: Bytes,
    kind: i32,
    origin: Vec<u8>,
    sensitive: bool,
    epoch: u64,
    progress: Instant,
    _permit: Permit,
}
pub struct NativeWriter {
    fd: OwnedFd,
    bytes: Arc<Bytes>,
    len: usize,
    off: usize,
    _permit: Permit,
    progress: Instant,
}
fn nonblocking(fd: i32) -> bool {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0
}
unsafe fn input<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        b""
    } else {
        CStr::from_ptr(p).to_bytes()
    }
}
#[no_mangle]
pub extern "C" fn axia_native_reset() {
    NATIVE.with(|n| *n.borrow_mut() = Native::default());
}
#[no_mangle]
pub extern "C" fn axia_native_text() -> *const c_char {
    NATIVE.with(|n| n.borrow().text.as_ref().map_or(c"".as_ptr(), Text::ptr))
}
#[no_mangle]
pub extern "C" fn axia_native_text_gen() -> u32 {
    NATIVE.with(|n| n.borrow().text_gen)
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_set_text(text: *const u8, len: usize) -> bool {
    if text.is_null() || len > LIMIT {
        return false;
    }
    let Some(b) = Bytes::copy(std::slice::from_raw_parts(text, len)) else {
        return false;
    };
    NATIVE.with(|n| {
        let mut n = n.borrow_mut();
        if !n.text(b, b"terminal", false, false) {
            return false;
        }
        n.invalidate(false);
        true
    })
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_mark(name: *const c_char, sensitive: bool) {
    NATIVE.with(|n| {
        let mut n = n.borrow_mut();
        n.origin = origin(input(name), b"lordos", 63);
        n.sensitive = sensitive;
    });
}
#[no_mangle]
pub extern "C" fn axia_native_origin() -> *const c_char {
    NATIVE.with(|n| n.borrow().origin.as_ptr())
}
#[no_mangle]
pub extern "C" fn axia_native_sensitive() -> bool {
    NATIVE.with(|n| n.borrow().sensitive)
}
#[no_mangle]
pub extern "C" fn axia_native_image_gen() -> u32 {
    NATIVE.with(|n| n.borrow().image_gen)
}
#[no_mangle]
pub extern "C" fn axia_native_is_image() -> bool {
    NATIVE.with(|n| n.borrow().image_active)
}
#[no_mangle]
pub extern "C" fn axia_native_image_path() -> *const c_char {
    NATIVE.with(|n| {
        let n = n.borrow();
        if n.image_active {
            n.image_path.as_ptr()
        } else {
            c"".as_ptr()
        }
    })
}
#[no_mangle]
pub extern "C" fn axia_native_image_origin() -> *const c_char {
    NATIVE.with(|n| n.borrow().image_origin.as_ptr())
}
#[no_mangle]
pub extern "C" fn axia_native_image_sensitive() -> bool {
    NATIVE.with(|n| n.borrow().image_sensitive)
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_png(len: *mut usize) -> *const u8 {
    NATIVE.with(|n| {
        let n = n.borrow();
        let b = n.png.as_ref().filter(|_| n.image_active);
        if !len.is_null() {
            *len = b.map_or(0, |b| b.data.len());
        }
        b.map_or(ptr::null(), |b| b.data.as_ptr())
    })
}
#[no_mangle]
pub extern "C" fn axia_native_text_active() {
    NATIVE.with(|n| n.borrow_mut().image_active = false);
}
#[no_mangle]
pub extern "C" fn axia_native_offer_gen() -> u32 {
    NATIVE.with(|n| n.borrow().offer_gen)
}
#[no_mangle]
pub extern "C" fn axia_native_offer_advance() {
    NATIVE.with(|n| {
        let mut n = n.borrow_mut();
        n.offer_gen = n.offer_gen.wrapping_add(1);
    });
}
#[no_mangle]
pub extern "C" fn axia_native_invalidate(primary: bool) {
    NATIVE.with(|n| n.borrow_mut().invalidate(primary));
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_primary(len: *mut usize, gen: *mut u32) -> *const c_char {
    NATIVE.with(|n| {
        let n = n.borrow();
        if !len.is_null() {
            *len = n.primary.as_ref().map_or(0, |t| t.len);
        }
        if !gen.is_null() {
            *gen = n.primary_gen;
        }
        n.primary.as_ref().map_or(c"".as_ptr(), Text::ptr)
    })
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_set_primary(text: *const u8, len: usize) -> bool {
    if text.is_null() || len > LIMIT {
        return false;
    }
    let Some(b) = Bytes::copy(std::slice::from_raw_parts(text, len)) else {
        return false;
    };
    NATIVE.with(|n| {
        let mut n = n.borrow_mut();
        if !n.text(b, b"", false, true) {
            return false;
        }
        n.invalidate(true);
        true
    })
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_set_png(
    png: *const u8,
    len: usize,
    name: *const c_char,
    sensitive: bool,
) -> bool {
    if png.is_null() || len > LIMIT {
        return false;
    }
    let Some(b) = Bytes::copy(std::slice::from_raw_parts(png, len)) else {
        return false;
    };
    NATIVE.with(|n| {
        let mut n = n.borrow_mut();
        if !n.image(b, input(name), sensitive) {
            return false;
        }
        n.invalidate(false);
        true
    })
}
fn managed(path: &Path) -> bool {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return false;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !name.ends_with(".png") {
        return false;
    }
    [
        ("XDG_RUNTIME_DIR", "axia", "clip-"),
        ("HOME", ".local/share/axia/clipboard", "img-"),
    ]
    .iter()
    .any(|(env, dir, prefix)| {
        std::env::var_os(env).is_some_and(|base| {
            path.parent() == Some(Path::new(&base).join(dir).as_path())
                && name.starts_with(prefix)
                && name.len() > prefix.len() + 4
        })
    })
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_use_image(path: *const c_char) -> bool {
    let path = Path::new(std::ffi::OsStr::from_encoded_bytes_unchecked(input(path)));
    if !managed(path) {
        return false;
    }
    let result = (|| {
        let mut f = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .ok()?;
        let m = f.metadata().ok()?;
        if !m.is_file() || m.uid() != libc::geteuid() || m.len() > LIMIT as u64 {
            return None;
        }
        let mut bytes = Bytes::default();
        let mut chunk = Chunk([0; 65536]);
        loop {
            let got = f.read(&mut chunk.0).ok()?;
            if got == 0 {
                break;
            }
            if bytes.data.len() + got > LIMIT || !bytes.append(&chunk.0[..got]) {
                return None;
            }
        }
        Some(bytes)
    })();
    let Some(bytes) = result else {
        return false;
    };
    NATIVE.with(|n| {
        let mut n = n.borrow_mut();
        if !n.image(bytes, b"lordos", false) {
            return false;
        }
        n.invalidate(false);
        true
    })
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_mime_rank(mime: *const c_char) -> i32 {
    match input(mime) {
        b"text/plain;charset=utf-8" => 4,
        b"UTF8_STRING" => 3,
        b"text/plain" => 2,
        b"STRING" | b"TEXT" => 1,
        _ => 0,
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_mime_choose(mimes: *const *const c_char, count: usize) -> i32 {
    if mimes.is_null() {
        return -1;
    }
    let mimes = std::slice::from_raw_parts(mimes, count.min(32));
    for preferred in [0, 1, 2] {
        for (i, m) in mimes.iter().enumerate() {
            let bytes = input(*m);
            if match preferred {
                0 => bytes == b"text/plain;charset=utf-8",
                1 => axia_native_mime_rank(*m) > 0,
                _ => bytes == b"image/png",
            } {
                return i as i32;
            }
        }
    }
    -1
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_mime_secret(
    mimes: *const *const c_char,
    count: usize,
) -> bool {
    !mimes.is_null()
        && std::slice::from_raw_parts(mimes, count.min(32))
            .iter()
            .any(|m| input(*m) == b"x-kde-passwordManagerHint")
}
// All constructors own fd, including rejection. Poll: 0 pending, 1 committed,
// -1 failed/stale. The caller destroys its event source then frees the engine.
#[no_mangle]
pub unsafe extern "C" fn axia_native_read_new(
    fd: i32,
    kind: i32,
    name: *const c_char,
    sensitive: bool,
) -> *mut NativeReader {
    if fd < 0 {
        return ptr::null_mut();
    }
    let fd = OwnedFd::from_raw_fd(fd);
    let Some(permit) = Permit::new() else {
        return ptr::null_mut();
    };
    if !(0..=2).contains(&kind) || !nonblocking(fd.as_raw_fd()) {
        return ptr::null_mut();
    }
    let epoch = NATIVE.with(|n| n.borrow().epochs[(kind == 2) as usize]);
    Box::into_raw(Box::new(NativeReader {
        fd,
        bytes: Bytes::default(),
        kind,
        origin: input(name)[..input(name).len().min(127)].to_vec(),
        sensitive,
        epoch,
        progress: Instant::now(),
        _permit: permit,
    }))
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_read_poll(reader: *mut NativeReader) -> i32 {
    let Some(r) = reader.as_mut() else {
        return -1;
    };
    if NATIVE.with(|n| n.borrow().epochs[(r.kind == 2) as usize]) != r.epoch {
        return -1;
    }
    let mut chunk = Chunk([0; 65536]);
    let mut work = 0;
    loop {
        let got = libc::read(r.fd.as_raw_fd(), chunk.0.as_mut_ptr().cast(), chunk.0.len());
        if got > 0 {
            let got = got as usize;
            if r.bytes.data.len() + got > LIMIT || !r.bytes.append(&chunk.0[..got]) {
                return -1;
            }
            r.progress = Instant::now();
            work += got;
            if work >= STEP {
                return 0;
            }
            continue;
        }
        if got == 0 {
            if r.bytes.data.is_empty() {
                return -1;
            }
            let bytes = std::mem::take(&mut r.bytes);
            return NATIVE.with(|n| {
                let mut n = n.borrow_mut();
                let ok = if r.kind == 1 {
                    n.image(bytes, &r.origin, r.sensitive)
                } else {
                    let ok = n.text(bytes, &r.origin, r.sensitive, r.kind == 2);
                    if ok && r.kind == 0 {
                        n.image_active = false;
                    }
                    ok
                };
                if ok {
                    1
                } else {
                    -1
                }
            });
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::EAGAIN) => return 0,
            _ => return -1,
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_read_free(reader: *mut NativeReader) {
    if !reader.is_null() {
        drop(Box::from_raw(reader));
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_write_new(fd: i32, kind: i32) -> *mut NativeWriter {
    if fd < 0 {
        return ptr::null_mut();
    }
    let fd = OwnedFd::from_raw_fd(fd);
    let Some(permit) = Permit::new() else {
        return ptr::null_mut();
    };
    if !nonblocking(fd.as_raw_fd()) {
        return ptr::null_mut();
    }
    let snapshot = NATIVE.with(|n| {
        let n = n.borrow();
        match kind {
            0 => n.text.as_ref().map(|t| (t.bytes.clone(), t.len)),
            1 => n
                .png
                .as_ref()
                .filter(|_| n.image_active)
                .map(|b| (b.clone(), b.data.len())),
            2 => n.primary.as_ref().map(|t| (t.bytes.clone(), t.len)),
            3 if if n.image_active {
                n.image_sensitive
            } else {
                n.sensitive
            } =>
            {
                Bytes::copy(b"secret").map(|b| (Arc::new(b), 6))
            }
            _ => None,
        }
    });
    let Some((bytes, len)) = snapshot else {
        return ptr::null_mut();
    };
    Box::into_raw(Box::new(NativeWriter {
        fd,
        bytes,
        len,
        off: 0,
        progress: Instant::now(),
        _permit: permit,
    }))
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_write_poll(writer: *mut NativeWriter) -> i32 {
    let Some(w) = writer.as_mut() else {
        return -1;
    };
    let mut work = 0;
    while w.off < w.len {
        let count = (w.len - w.off).min(STEP - work);
        let got = libc::write(
            w.fd.as_raw_fd(),
            w.bytes.data[w.off..].as_ptr().cast(),
            count,
        );
        if got > 0 {
            w.progress = Instant::now();
            w.off += got as usize;
            work += got as usize;
            if work >= STEP && w.off < w.len {
                return 0;
            }
            continue;
        }
        if got < 0 {
            match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EINTR) => continue,
                Some(libc::EAGAIN) => return 0,
                _ => (),
            }
        }
        return -1;
    }
    1
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_write_free(writer: *mut NativeWriter) {
    if !writer.is_null() {
        drop(Box::from_raw(writer));
    }
}
#[cfg(test)]
#[path = "native_tests.rs"]
mod native_tests;

// X11's INCR protocol delivers properties instead of descriptors. It uses the
// same bounded/scrubbed payloads and immutable snapshots as the Wayland engine.
pub struct NativeAccumulator {
    bytes: Bytes,
    primary: bool,
    epoch: u64,
    _permit: Permit,
}
pub struct NativeSnapshot {
    bytes: Arc<Bytes>,
    len: usize,
    _permit: Permit,
}
#[no_mangle]
pub extern "C" fn axia_native_acc_new(primary: bool) -> *mut NativeAccumulator {
    let Some(permit) = Permit::new() else {
        return ptr::null_mut();
    };
    let epoch = NATIVE.with(|n| {
        let mut n = n.borrow_mut();
        n.invalidate(primary);
        n.epochs[primary as usize]
    });
    Box::into_raw(Box::new(NativeAccumulator {
        bytes: Bytes::default(),
        primary,
        epoch,
        _permit: permit,
    }))
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_acc_append(
    acc: *mut NativeAccumulator,
    data: *const u8,
    len: usize,
) -> bool {
    let Some(a) = acc.as_mut() else {
        return false;
    };
    if data.is_null()
        || a.bytes.data.len().saturating_add(len) > LIMIT
        || NATIVE.with(|n| n.borrow().epochs[a.primary as usize]) != a.epoch
    {
        return false;
    }
    a.bytes.append(std::slice::from_raw_parts(data, len))
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_acc_finish(
    acc: *mut NativeAccumulator,
    kind: i32,
    latin1: bool,
    name: *const c_char,
) -> usize {
    let Some(a) = acc.as_mut() else {
        return 0;
    };
    if !(0..=2).contains(&kind)
        || (kind == 2) != a.primary
        || NATIVE.with(|n| n.borrow().epochs[a.primary as usize]) != a.epoch
    {
        return 0;
    }
    let mut bytes = std::mem::take(&mut a.bytes);
    if bytes.data.is_empty() {
        return 0;
    }
    if latin1 {
        let mut converted = Bytes::default();
        for b in &bytes.data {
            let mut encoded = [0; 2];
            let slice = if *b < 128 {
                encoded[0] = *b;
                &encoded[..1]
            } else {
                encoded[0] = 0xc0 | (*b >> 6);
                encoded[1] = 0x80 | (*b & 0x3f);
                &encoded[..2]
            };
            if converted.data.len() + slice.len() > LIMIT || !converted.append(slice) {
                return 0;
            }
        }
        bytes = converted;
    }
    let len = bytes.data.len();
    NATIVE.with(|n| {
        let mut n = n.borrow_mut();
        let ok = if kind == 1 {
            n.image(bytes, input(name), false)
        } else {
            n.text(
                bytes,
                if kind == 2 { b"" } else { input(name) },
                false,
                kind == 2,
            )
        };
        if ok {
            n.invalidate(a.primary);
            len
        } else {
            0
        }
    })
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_acc_free(acc: *mut NativeAccumulator) {
    if !acc.is_null() {
        drop(Box::from_raw(acc));
    }
}
#[no_mangle]
pub extern "C" fn axia_native_snapshot_new(kind: i32, latin1: bool) -> *mut NativeSnapshot {
    let Some(permit) = Permit::new() else {
        return ptr::null_mut();
    };
    let snapshot = NATIVE.with(|n| {
        let n = n.borrow();
        match kind {
            0 if !n.sensitive => n.text.as_ref().map(|t| (t.bytes.clone(), t.len)),
            1 if !n.image_sensitive => n
                .png
                .as_ref()
                .filter(|_| n.image_active)
                .map(|b| (b.clone(), b.data.len())),
            2 => n.primary.as_ref().map(|t| (t.bytes.clone(), t.len)),
            _ => None,
        }
    });
    let Some((mut bytes, mut len)) = snapshot else {
        return ptr::null_mut();
    };
    if latin1 {
        let mut converted = Bytes::default();
        // Decode invalid UTF-8 in place: from_utf8_lossy would allocate an
        // uncharged replacement String as large as three times the input.
        for chunk in bytes.data[..len].utf8_chunks() {
            for c in chunk.valid().chars() {
                if !converted.append(&[if c as u32 <= 255 { c as u8 } else { b'?' }]) {
                    return ptr::null_mut();
                }
            }
            if !chunk.invalid().is_empty() && !converted.append(b"?") {
                return ptr::null_mut();
            }
        }
        len = converted.data.len();
        bytes = Arc::new(converted);
    }
    Box::into_raw(Box::new(NativeSnapshot {
        bytes,
        len,
        _permit: permit,
    }))
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_snapshot_data(
    snapshot: *const NativeSnapshot,
    len: *mut usize,
) -> *const u8 {
    let Some(s) = snapshot.as_ref() else {
        if !len.is_null() {
            *len = 0;
        }
        return ptr::null();
    };
    if !len.is_null() {
        *len = s.len;
    }
    s.bytes.data.as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_snapshot_free(snapshot: *mut NativeSnapshot) {
    if !snapshot.is_null() {
        drop(Box::from_raw(snapshot));
    }
}

// Event sources keep descriptors alive, so the host must retire stale/stalled
// engines even when their descriptor will never become readable/writable again.
#[no_mangle]
pub unsafe extern "C" fn axia_native_read_expired(reader: *const NativeReader) -> bool {
    reader.as_ref().is_none_or(|r| {
        r.progress.elapsed() >= Duration::from_secs(10)
            || NATIVE.with(|n| n.borrow().epochs[(r.kind == 2) as usize]) != r.epoch
    })
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_write_expired(writer: *const NativeWriter) -> bool {
    writer
        .as_ref()
        .is_none_or(|w| w.progress.elapsed() >= Duration::from_secs(10))
}
