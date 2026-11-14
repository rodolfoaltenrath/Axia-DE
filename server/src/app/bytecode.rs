//! Verified source -> QuickJS compiler bridge -> sealed, read-only inherited cache.
use super::*;
use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};
pub(super) const MAX: usize = 64 << 20;
pub(super) const HEADER: usize = 56;
const SEALS: i32 = libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE | libc::F_SEAL_SEAL;
static FD: AtomicI32 = AtomicI32::new(-1);
static STARTED: AtomicBool = AtomicBool::new(false);
extern "C" {
    fn axia_app_qjs_compile(
        src: *const c_char,
        len: usize,
        name: *const c_char,
        outlen: *mut usize,
    ) -> *mut u8;
    fn axia_app_qjs_eval(ctx: *mut c_void, bc: *const u8, len: usize, ok: *mut i32) -> i32;
}
fn digest(src: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    unsafe {
        sha256(src.as_ptr().cast(), src.len(), out.as_mut_ptr());
    }
    out
}
pub(super) fn header(source_len: usize, hash: [u8; 32], len: usize) -> [u8; HEADER] {
    let mut h = [0; HEADER];
    h[..8].copy_from_slice(b"LORDQBC1");
    h[8..16].copy_from_slice(&(source_len as u64).to_ne_bytes());
    h[16..48].copy_from_slice(&hash);
    h[48..56].copy_from_slice(&(len as u64).to_ne_bytes());
    h
}
pub(super) fn valid(bytes: &[u8], source_len: usize, hash: &[u8; 32]) -> bool {
    bytes.len() >= HEADER
        && bytes.len() <= MAX
        && &bytes[..8] == b"LORDQBC1"
        && u64::from_ne_bytes(bytes[8..16].try_into().unwrap()) == source_len as u64
        && &bytes[16..48] == hash
        && u64::from_ne_bytes(bytes[48..56].try_into().unwrap()) == (bytes.len() - HEADER) as u64
}
pub(super) fn sealed(source_len: usize, hash: [u8; 32], bc: &[u8]) -> std::io::Result<OwnedFd> {
    use std::io::Write;
    if bc.is_empty() || bc.len() > MAX - HEADER {
        return Err(std::io::Error::from_raw_os_error(libc::EFBIG));
    }
    let fd = unsafe {
        libc::memfd_create(
            c"lordos-apphost-bc".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    f.write_all(&header(source_len, hash, bc.len()))?;
    f.write_all(bc)?;
    if unsafe { libc::fcntl(f.as_raw_fd(), libc::F_ADD_SEALS, SEALS) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(f.into())
}
#[no_mangle]
pub unsafe extern "C" fn apphost_bc_start(bundle: *const c_char) {
    if bundle.is_null() || text(bundle).len() >= 1024 || STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    if codesig_state() == 0 {
        eprintln!("axia-server: no app host bytecode: code check not set up");
        return;
    }
    let path = CStr::from_ptr(bundle).to_owned();
    if std::thread::Builder::new()
        .name("apphost-bytecode".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let mut got = 0;
            let mut why = [0i8; 160];
            let src =
                unsafe { codesig_load(path.as_ptr(), MAX, &mut got, why.as_mut_ptr(), why.len()) };
            if src.is_null() {
                eprintln!(
                    "axia-server: no app host bytecode: {}",
                    unsafe { CStr::from_ptr(why.as_ptr()) }.to_string_lossy()
                );
                return;
            }
            let mut len = 0;
            let bc = unsafe { axia_app_qjs_compile(src, got, path.as_ptr(), &mut len) };
            if !bc.is_null() {
                let hash = digest(unsafe { std::slice::from_raw_parts(src.cast(), got) });
                if let Ok(fd) = sealed(got, hash, unsafe { std::slice::from_raw_parts(bc, len) }) {
                    use std::os::fd::IntoRawFd;
                    FD.store(fd.into_raw_fd(), Ordering::Release);
                    eprintln!(
                        "axia-server: app host compiled to bytecode ({} KB) for isolated apps",
                        len / 1024
                    );
                }
                unsafe {
                    libc::free(bc.cast());
                }
            }
            unsafe {
                libc::free(src.cast());
            }
        })
        .is_err()
    {
        eprintln!("axia-server: no thread for the app host bytecode");
    }
}
#[no_mangle]
pub unsafe extern "C" fn apphost_bc_dup_ro() -> i32 {
    let fd = FD.load(Ordering::Acquire);
    if fd < 0 {
        return -1;
    }
    let p = CString::new(format!("/proc/self/fd/{fd}")).unwrap();
    libc::open(p.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC)
}
#[no_mangle]
pub unsafe extern "C" fn apphost_bc_eval(
    ctx: *mut c_void,
    src: *const c_char,
    len: usize,
    fd: i32,
    ok: *mut i32,
) -> i32 {
    if ok.is_null() {
        return -1;
    }
    *ok = 0;
    if fd < 0 || src.is_null() || ctx.is_null() || len > MAX {
        return -1;
    }
    let mut st: libc::stat = std::mem::zeroed();
    let seals = libc::fcntl(fd, libc::F_GET_SEALS);
    if libc::fstat(fd, &mut st) < 0
        || st.st_size < HEADER as i64
        || st.st_size > MAX as i64
        || seals < 0
        || seals & SEALS != SEALS
    {
        return -1;
    }
    let size = st.st_size as usize;
    let p = libc::mmap(
        ptr::null_mut(),
        size,
        libc::PROT_READ,
        libc::MAP_PRIVATE,
        fd,
        0,
    );
    if p == libc::MAP_FAILED {
        return -1;
    }
    let bytes = std::slice::from_raw_parts(p.cast::<u8>(), size);
    let result = if valid(
        bytes,
        len,
        &digest(std::slice::from_raw_parts(src.cast(), len)),
    ) {
        axia_app_qjs_eval(ctx, bytes[HEADER..].as_ptr(), size - HEADER, ok)
    } else {
        -1
    };
    libc::munmap(p, size);
    result
}
