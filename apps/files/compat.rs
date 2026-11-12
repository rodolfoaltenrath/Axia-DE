//! Stable C ABI used by the rest of the desktop. No filesystem algorithm remains
//! in the C transport; existing apps keep their permissions and translated error codes.
use super::{operations as ops, policy, trash, Result};
use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr, CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, UNIX_EPOCH};

unsafe fn path<'a>(p: *const c_char) -> &'a Path {
    if p.is_null() {
        Path::new("")
    } else {
        Path::new(OsStr::from_bytes(CStr::from_ptr(p).to_bytes()))
    }
}
unsafe fn text<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        ""
    } else {
        CStr::from_ptr(p).to_str().unwrap_or("")
    }
}
unsafe fn put(dst: *mut c_char, n: usize, s: &[u8]) {
    if dst.is_null() || n == 0 {
        return;
    }
    let len = s.len().min(n - 1);
    std::ptr::copy_nonoverlapping(s.as_ptr(), dst.cast(), len);
    *dst.add(len) = 0;
}
unsafe fn answer<T>(r: Result<T>, err: *mut c_char, n: usize) -> i32 {
    match r {
        Ok(_) => {
            put(err, n, b"");
            0
        }
        Err(e) => {
            put(err, n, e.as_bytes());
            -1
        }
    }
}
fn cp(p: &Path) -> CString {
    CString::new(p.as_os_str().as_bytes()).unwrap_or_default()
}

#[no_mangle]
pub unsafe extern "C" fn fsops_init(home: *const c_char, user: *const c_char) {
    policy::init(path(home), text(user));
}
#[no_mangle]
pub unsafe extern "C" fn fsops_allowed(p: *const c_char) -> i32 {
    policy::current().allowed(path(p)) as i32
}
#[no_mangle]
pub unsafe extern "C" fn fsops_readable(p: *const c_char) -> i32 {
    policy::current().readable(path(p)) as i32
}
#[no_mangle]
pub unsafe extern "C" fn fsops_protected(p: *const c_char) -> i32 {
    policy::current().protected(path(p)) as i32
}
#[no_mangle]
pub unsafe extern "C" fn fsops_protected_below(p: *const c_char) -> i32 {
    policy::current().protected_below(path(p)) as i32
}
thread_local! {static HOME:RefCell<CString>=RefCell::new(CString::default());}
#[no_mangle]
pub extern "C" fn fsops_home() -> *const c_char {
    HOME.with(|s| {
        let current = cp(&policy::current().home);
        if s.borrow().as_bytes() != current.as_bytes() {
            *s.borrow_mut() = current;
        }
        s.borrow().as_ptr()
    })
}
#[no_mangle]
pub unsafe extern "C" fn fsops_rename(
    a: *const c_char,
    b: *const c_char,
    err: *mut c_char,
    n: usize,
) -> i32 {
    answer(ops::rename(path(a), path(b)), err, n)
}

#[repr(C)]
#[derive(Clone)]
pub struct FsJob {
    id: i32,
    used: i32,
    moving: i32,
    done: i32,
    failed: i32,
    cancel: i32,
    src: [c_char; 4096],
    dst: [c_char; 4096],
    bytes: u64,
    total: u64,
    files: u64,
    error: [c_char; 256],
}
thread_local! {static SNAPSHOTS:RefCell<[Option<Box<FsJob>>;8]>=RefCell::new(std::array::from_fn(|_|None));}
#[no_mangle]
pub unsafe extern "C" fn fsjob_start(
    src: *const c_char,
    dst: *const c_char,
    moving: i32,
    err: *mut c_char,
    n: usize,
) -> i32 {
    match ops::start(path(src), path(dst), moving != 0) {
        Ok(id) => {
            put(err, n, b"");
            id
        }
        Err(e) => {
            put(err, n, e.as_bytes());
            -1
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn fsjob_get(i: i32) -> *const FsJob {
    if !(0..8).contains(&i) {
        return std::ptr::null();
    }
    let all = ops::jobs();
    let Some(j) = all.get(i as usize) else {
        return std::ptr::null();
    };
    SNAPSHOTS.with(|snap| {
        let mut snap = snap.borrow_mut();
        let item = snap[i as usize].get_or_insert_with(|| {
            Box::new(FsJob {
                id: 0,
                used: 0,
                moving: 0,
                done: 0,
                failed: 0,
                cancel: 0,
                src: [0; 4096],
                dst: [0; 4096],
                bytes: 0,
                total: 0,
                files: 0,
                error: [0; 256],
            })
        });
        item.id = j.id;
        item.used = 1;
        item.moving = j.moving as i32;
        item.done = j.done as i32;
        item.failed = j.failed as i32;
        item.cancel = j.cancel as i32;
        item.bytes = j.bytes;
        item.total = j.total;
        item.files = j.files;
        put(item.src.as_mut_ptr(), 4096, j.src.as_bytes());
        put(item.dst.as_mut_ptr(), 4096, j.dst.as_bytes());
        put(item.error.as_mut_ptr(), 256, j.error.as_bytes());
        &**item as *const FsJob
    })
}
#[no_mangle]
pub extern "C" fn fsjob_cancel(id: i32) {
    ops::cancel(id)
}
#[no_mangle]
pub extern "C" fn fsjob_forget(id: i32) {
    ops::forget(id)
}

#[no_mangle]
pub unsafe extern "C" fn fstrash_put(p: *const c_char, err: *mut c_char, n: usize) -> i32 {
    answer(trash::put(path(p)), err, n)
}
#[no_mangle]
pub unsafe extern "C" fn fstrash_put_at(
    fd: i32,
    name: *const c_char,
    real: *const c_char,
    dest: *mut c_char,
    dn: usize,
    err: *mut c_char,
    n: usize,
) -> i32 {
    if name.is_null() {
        return answer::<()>(Err("sys.badArgument".into()), err, n);
    }
    let r = trash::put_at(fd, CStr::from_ptr(name), path(real));
    match r {
        Ok(p) => {
            put(dest, dn, p.as_bytes());
            0
        }
        Err(e) => answer::<()>(Err(e), err, n),
    }
}
type TrashFn = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    i32,
    f64,
);
#[no_mangle]
pub unsafe extern "C" fn fstrash_list(callback: Option<TrashFn>, ctx: *mut c_void) {
    if let Some(cb) = callback {
        for e in trash::list() {
            let id = CString::new(e.id).unwrap_or_default();
            let name = CString::new(e.name).unwrap_or_default();
            let original = CString::new(e.original).unwrap_or_default();
            let date = CString::new(e.deleted).unwrap_or_default();
            cb(
                ctx,
                id.as_ptr(),
                name.as_ptr(),
                original.as_ptr(),
                date.as_ptr(),
                e.dir as i32,
                e.size as f64,
            );
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn fstrash_restore(p: *const c_char, err: *mut c_char, n: usize) -> i32 {
    answer(trash::restore(path(p)), err, n)
}
#[no_mangle]
pub unsafe extern "C" fn fstrash_remove(p: *const c_char, err: *mut c_char, n: usize) -> i32 {
    answer(trash::remove(path(p)), err, n)
}
#[no_mangle]
pub extern "C" fn fstrash_empty() -> i32 {
    trash::empty().map(|n| n as i32).unwrap_or(-1)
}

#[repr(C)]
pub struct FsDu {
    size: f64,
    files: libc::c_long,
    dirs: libc::c_long,
    complete: i32,
}
#[no_mangle]
pub unsafe extern "C" fn fs_du(p: *const c_char, ms: f64, out: *mut FsDu) {
    if let Some(out) = out.as_mut() {
        let (items, complete) = ops::walk(
            path(p),
            64,
            1_000_000,
            ops::DurationBudget::millis(ms),
            true,
        );
        *out = FsDu {
            size: 0.,
            files: 0,
            dirs: 0,
            complete: complete as i32,
        };
        for (_, m) in items {
            if m.is_dir() {
                out.dirs += 1;
            } else if m.is_file() {
                out.files += 1;
                out.size += m.len() as f64;
            }
        }
    }
}
type HitFn = unsafe extern "C" fn(*mut c_void, *const c_char, i32, f64, f64);
#[no_mangle]
pub unsafe extern "C" fn fs_search(
    p: *const c_char,
    needle: *const c_char,
    limit: i32,
    depth: i32,
    callback: Option<HitFn>,
    ctx: *mut c_void,
) -> i32 {
    let Some(cb) = callback else { return 0 };
    let needle = text(needle).to_lowercase();
    if needle.is_empty() {
        return 0;
    }
    let (items, _) = ops::walk(
        path(p),
        depth.clamp(0, 64) as usize,
        100_000,
        ops::DurationBudget::seconds(3),
        false,
    );
    let mut count = 0;
    for (p, m) in items.into_iter().skip(1) {
        if p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
            .contains(&needle)
        {
            let c = cp(&p);
            cb(
                ctx,
                c.as_ptr(),
                m.is_dir() as i32,
                m.len() as f64,
                m.mtime() as f64 * 1000.,
            );
            count += 1;
            if count >= limit.clamp(1, 10000) {
                break;
            }
        }
    }
    count
}
#[no_mangle]
pub unsafe extern "C" fn fs_recent(
    limit: i32,
    days: i32,
    callback: Option<HitFn>,
    ctx: *mut c_void,
) -> i32 {
    let Some(cb) = callback else { return 0 };
    let now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let (mut items, _) = ops::walk_pruned(
        &policy::current().home,
        6,
        60000,
        ops::DurationBudget::seconds(3),
        false,
        &["node_modules"],
    );
    items.retain(|(_, m)| m.is_file() && m.mtime() >= now - days.max(0) as i64 * 86400);
    items.sort_by(|a, b| b.1.mtime().cmp(&a.1.mtime()));
    items.truncate(limit.clamp(1, 500) as usize);
    let n = items.len();
    for (p, m) in items {
        let c = cp(&p);
        cb(ctx, c.as_ptr(), 0, m.len() as f64, m.mtime() as f64 * 1000.);
    }
    n as i32
}
type VolFn = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, f64, f64);
#[no_mangle]
pub unsafe extern "C" fn fs_volumes(callback: Option<VolFn>, ctx: *mut c_void) {
    if let Some(cb) = callback {
        for (p, kind) in ops::mounts() {
            let (total, avail) = if trash::share_fs(&kind) {
                (0, 0)
            } else {
                ops::free(&p).unwrap_or_default()
            };
            let p = cp(&p);
            let k = CString::new(kind).unwrap();
            cb(ctx, p.as_ptr(), k.as_ptr(), total as f64, avail as f64);
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn fsops_share_fs(kind: *const c_char) -> i32 {
    trash::share_fs(text(kind)) as i32
}

#[derive(Clone)]
struct Probe {
    when: Instant,
    result: Option<bool>,
}
fn probes() -> &'static Mutex<std::collections::HashMap<PathBuf, Probe>> {
    static P: OnceLock<Mutex<std::collections::HashMap<PathBuf, Probe>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}
#[no_mangle]
pub unsafe extern "C" fn fsops_share_down(p: *const c_char) -> i32 {
    let path = path(p);
    let Some((mount, _)) = ops::mounts()
        .into_iter()
        .filter(|(m, k)| path.starts_with(m) && trash::share_fs(k))
        .max_by_key(|(m, _)| m.as_os_str().len())
    else {
        return 0;
    };
    let mut cache = probes().lock().unwrap();
    if let Some(probe) = cache.get(&mount) {
        if probe.result.is_none() {
            return 1;
        }
        if probe.when.elapsed().as_secs() < 3 {
            return (!probe.result.unwrap()) as i32;
        }
    }
    if cache.values().filter(|p| p.result.is_none()).count() >= 8 {
        return 1;
    }
    cache.insert(
        mount.clone(),
        Probe {
            when: Instant::now(),
            result: None,
        },
    );
    std::thread::spawn(move || {
        let ok = ops::free(&mount).is_ok();
        probes().lock().unwrap().insert(
            mount,
            Probe {
                when: Instant::now(),
                result: Some(ok),
            },
        );
    });
    1
}
