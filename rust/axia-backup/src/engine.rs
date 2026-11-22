//! The backup engine: hard-linked snapshots, retention and restore (docs/SYSTEM.md, "Backups").
//!
//! Our own code, no rsync: a snapshot is a complete tree of the home folder in
//!   <dest>/LordOS-Backups/<host>-<user>/<YYYY-MM-DD-HHMMSS>/
//! Files that did not change (same size, modification time and mode as in the previous snapshot)
//! are hard links to the previous one: every snapshot looks complete but only takes the space of
//! what changed. While copying, the tree lives in .inprogress-<stamp>; it is renamed at the end
//! (atomic). An interrupted run is resumed: the next one uses that partial tree as another link
//! source and then deletes it.
//!
//! Rules:
//!   - a symbolic link is never followed: the walk uses lstat and opens with O_NOFOLLOW; links are
//!     stored as links (their text is copied, not their target);
//!   - never crosses into another filesystem (a disk mounted inside the home folder);
//!   - fifos, sockets and devices are skipped;
//!   - a file is copied to "<name>.bkpart~" and renamed when done, so a final name is always a
//!     complete file (which is what makes resuming an interrupted run possible);
//!   - deletion only happens inside the machine folder and with validated names.
//!
//! This module knows nothing about threads or JS (`service` uses it).
use crate::crypt::{self, Key};
use crate::errcode::{self, errno_name, lerr};
use axia_fs::policy::{self, Policy};
use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, Metadata};
use std::io;
use std::os::fd::{AsRawFd, IntoRawFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

pub const DIRNAME: &str = "LordOS-Backups";
pub const MAX_EXCL: usize = 64;
pub const MAX_SNAPS: usize = 4096;
/// "2026-10-17-235959"
pub const ID_LEN: usize = 17;
const CHUNK: usize = 8 << 20;
const MAX_DEPTH: usize = 128;
const DEFAULT_MIN_FREE: u64 = 512 << 20;
const PART: &[u8] = b".bkpart~";

fn errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn path_of(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(bytes))
}

fn cstr(p: &Path) -> CString {
    CString::new(p.as_os_str().as_bytes()).unwrap_or_default()
}

fn with_suffix(p: &Path, suffix: &[u8]) -> PathBuf {
    let mut s = p.as_os_str().as_bytes().to_vec();
    s.extend_from_slice(suffix);
    PathBuf::from(OsString::from_vec(s))
}

/// Is `p` equal to `root` or inside it (by name, not resolving links)?
fn under(p: &[u8], root: &[u8]) -> bool {
    let n = root.len();
    n > 0 && p.starts_with(root) && (p.len() == n || p[n] == b'/' || root[n - 1] == b'/')
}

fn mkdir_p(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    match fs::DirBuilder::new().recursive(true).mode(mode).create(path) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => Err(e),
        _ => Ok(()),
    }
}

// ---- deleting trees ----

fn rm_tree(p: &Path, dev: Option<u64>) -> io::Result<()> {
    let st = match fs::symlink_metadata(p) {
        Ok(st) => st,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !st.is_dir() {
        return fs::remove_file(p).or_else(ignore_missing);
    }
    // FTW_MOUNT: stay on the filesystem of the tree's root
    let dev = dev.unwrap_or(st.dev());
    for entry in fs::read_dir(p)? {
        let child = entry?.path();
        match fs::symlink_metadata(&child) {
            Ok(c) if c.is_dir() && c.dev() != dev => continue,
            _ => rm_tree(&child, Some(dev))?,
        }
    }
    fs::remove_dir(p).or_else(ignore_missing)
}

fn ignore_missing(e: io::Error) -> io::Result<()> {
    if e.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(e)
    }
}

/// Before deleting: folders stored with mode 0500 do not let their contents be removed.
fn chmod_dirs(p: &Path) {
    let chmod = |q: &Path| unsafe { libc::chmod(cstr(q).as_ptr(), 0o700) };
    let dir = fs::read_dir(p).or_else(|_| {
        chmod(p);
        fs::read_dir(p)
    });
    let Ok(dir) = dir else { return };
    chmod(p);
    for entry in dir.flatten() {
        let child = entry.path();
        if fs::symlink_metadata(&child).is_ok_and(|m| m.is_dir()) {
            chmod_dirs(&child);
        }
    }
}

fn rm_tree_force(p: &Path) -> io::Result<()> {
    if rm_tree(p, None).is_ok() {
        return Ok(());
    }
    chmod_dirs(p);
    rm_tree(p, None)
}

// ---- snapshot names ----

pub fn id_valid(id: &[u8]) -> bool {
    const PATTERN: &[u8; ID_LEN] = b"dddd-dd-dd-dddddd";
    id.len() == ID_LEN && id.iter().zip(PATTERN).all(|(c, p)| if *p == b'd' { c.is_ascii_digit() } else { c == p })
}

/// The local time encoded in the name, or None.
pub fn id_time(id: &[u8]) -> Option<i64> {
    if !id_valid(id) {
        return None;
    }
    let num = |a: usize, b: usize| std::str::from_utf8(&id[a..b]).ok()?.parse::<i32>().ok();
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = num(0, 4)? - 1900;
    tm.tm_mon = num(5, 7)? - 1;
    tm.tm_mday = num(8, 10)?;
    tm.tm_hour = num(11, 13)?;
    tm.tm_min = num(13, 15)?;
    tm.tm_sec = num(15, 17)?;
    tm.tm_isdst = -1;
    Some(unsafe { libc::mktime(&mut tm) } as i64)
}

pub fn id_make(t: i64) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = t as libc::time_t;
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!(
        "{:04}-{:02}-{:02}-{:02}{:02}{:02}",
        (tm.tm_year + 1900) as u32 % 10000,
        (tm.tm_mon + 1) as u32 % 100,
        tm.tm_mday as u32 % 100,
        tm.tm_hour as u32 % 100,
        tm.tm_min as u32 % 100,
        tm.tm_sec as u32 % 100
    )
}

/// Complete snapshots of this machine, oldest first (at most `max`).
pub fn list(machine: &Path, max: usize) -> Vec<String> {
    let Ok(dir) = fs::read_dir(machine) else { return vec![] };
    let mut ids = vec![];
    for entry in dir.flatten() {
        let name = entry.file_name();
        if ids.len() >= max || !id_valid(name.as_bytes()) {
            continue;
        }
        if fs::symlink_metadata(machine.join(&name)).is_ok_and(|m| m.is_dir()) {
            ids.push(name.to_string_lossy().into_owned());
        }
    }
    ids.sort();
    ids
}

/// Safe rel: relative, no "..", ".", "//" or control characters. "" is valid.
pub fn rel_valid(rel: &[u8]) -> bool {
    if rel.len() >= 4096 / 2 || rel.first() == Some(&b'/') {
        return false;
    }
    if rel.is_empty() {
        return true;
    }
    if rel.last() == Some(&b'/') {
        return false;
    }
    rel.split(|b| *b == b'/').all(|part| !part.is_empty() && part != b"." && part != b".." && part.iter().all(|c| *c >= 0x20 && *c != 0x7f))
}

// ---- retention ----

fn floordiv(a: i64, b: i64) -> i64 {
    a.div_euclid(b) - if b < 0 && a.rem_euclid(b) != 0 { 1 } else { 0 }
}

/// Retention (pure function): which snapshots stay. `t` is oldest first.
/// Everything from the last 24 h; the newest of each day up to 30 days; the newest of each week
/// after that. The newest snapshot always stays. `tz_off`: seconds to add for local time.
pub fn retention(t: &[i64], now: i64, tz_off: i64) -> Vec<bool> {
    let n = t.len();
    let mut keep = vec![false; n];
    if n == 0 {
        return keep;
    }
    keep[n - 1] = true;
    let (mut last_day, mut last_week) = (i64::MIN, i64::MIN);
    for i in (0..n).rev() {
        let age = now - t[i];
        if age < 86400 {
            keep[i] = true;
            continue;
        }
        let local = t[i] + tz_off;
        if age < 30 * 86400 {
            let day = floordiv(local, 86400);
            if day != last_day {
                keep[i] = true;
                last_day = day;
            }
        } else {
            let week = floordiv(local - 4 * 86400, 7 * 86400); // weeks start on Monday (1970-01-05)
            if week != last_week {
                keep[i] = true;
                last_week = week;
            }
        }
    }
    keep
}

pub fn free_bytes(p: &Path) -> u64 {
    let mut v: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(cstr(p).as_ptr(), &mut v) } != 0 {
        return u64::MAX;
    }
    v.f_bavail as u64 * v.f_frsize as u64
}

/// Deletes one snapshot: first out of the list (rename is atomic), then at leisure.
pub fn delete(machine: &Path, id: &str) -> bool {
    if !id_valid(id.as_bytes()) {
        return false;
    }
    let gone = machine.join(format!(".deleting-{id}"));
    fs::rename(machine.join(id), &gone).is_ok() && rm_tree_force(&gone).is_ok()
}

/// Applies the retention, finishes interrupted deletions and, short on space (`need` bytes free),
/// drops the oldest, never the newest. Returns how many snapshots went.
pub fn prune(machine: &Path, now: i64, need: u64) -> usize {
    let mut ids = list(machine, MAX_SNAPS);
    let times: Vec<i64> = ids.iter().map(|id| id_time(id.as_bytes()).unwrap_or(-1)).collect();
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = now as libc::time_t;
    unsafe { libc::localtime_r(&t, &mut tm) };
    let keep = retention(&times, now, tm.tm_gmtoff as i64);
    let mut gone = 0;
    for i in 0..ids.len() {
        if !keep[i] && delete(machine, &ids[i]) {
            gone += 1;
            ids[i].clear();
        }
    }
    // leftovers of interrupted deletions
    if let Ok(dir) = fs::read_dir(machine) {
        for entry in dir.flatten() {
            if entry.file_name().as_bytes().starts_with(b".deleting-") {
                let _ = rm_tree_force(&entry.path());
            }
        }
    }
    // short on space: the oldest go, never the newest
    if need > 0 {
        let mut left = ids.iter().filter(|i| !i.is_empty()).count();
        for i in 0..ids.len() {
            if left <= 1 || free_bytes(machine) >= need {
                break;
            }
            if !ids[i].is_empty() && delete(machine, &ids[i]) {
                gone += 1;
                left -= 1;
                ids[i].clear();
            }
        }
    }
    gone
}

// ---- snapshot ----

/// What a snapshot counts while it runs (read by the service's status, written by the engine).
pub struct Progress {
    pub total_files: AtomicU64,
    pub total_bytes: AtomicU64,
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub copied: AtomicU64,
    pub linked: AtomicU64,
    pub copied_bytes: AtomicU64,
    pub errors: AtomicU64,
    pub skipped: AtomicU64,
}

impl Progress {
    pub const fn new() -> Self {
        Self {
            total_files: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            files: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            copied: AtomicU64::new(0),
            linked: AtomicU64::new(0),
            copied_bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
        }
    }
    pub fn reset(&self) {
        for c in [&self.total_files, &self.total_bytes, &self.files, &self.bytes, &self.copied, &self.linked, &self.copied_bytes, &self.errors, &self.skipped] {
            c.store(0, Relaxed);
        }
    }
    pub fn get(c: &AtomicU64) -> u64 {
        c.load(Relaxed)
    }
}

/// The input of a snapshot.
pub struct Run<'a> {
    /// Source folder (already resolved with realpath).
    pub home: &'a Path,
    /// <dest>/LordOS-Backups/<host>-<user> (created if missing).
    pub machine: &'a Path,
    /// Prefixes relative to home ("Videos/large", ".cache").
    pub excl: Vec<String>,
    /// Real path that is never copied (the destination, if inside home).
    pub skip_real: Option<PathBuf>,
    /// True: stop as soon as possible (left to resume).
    pub cancel: Option<&'a AtomicBool>,
    /// Called between files and every ~8 MB: may block (pause); true stops.
    pub checkpoint: Option<&'a mut dyn FnMut() -> bool>,
    /// When moving to a new file (relative to home).
    pub progress: Option<&'a mut dyn FnMut(&[u8])>,
    /// Free bytes to keep on the destination (0: 512 MiB).
    pub min_free: u64,
    /// The 32-byte key if the destination is encrypted.
    pub key: Option<&'a Key>,
    pub stats: &'a Progress,
}

/// How a snapshot ended: 0 done; 1 interrupted (.inprogress left to resume); -1 error.
pub struct Outcome {
    pub code: i32,
    /// The snapshot created.
    pub id: String,
    /// A code line (errcode), when `code` is -1.
    pub error: Vec<u8>,
}

struct Walk<'a, 'b> {
    r: &'b mut Run<'a>,
    src: PathBuf,
    dst: PathBuf,
    /// Link sources: an interrupted run, the previous snapshot.
    link1: Option<PathBuf>,
    link2: Option<PathBuf>,
    home_dev: u64,
    skip: Option<(u64, u64)>,
    counting: bool,
    stop: bool,
    fatal: bool,
    error: Vec<u8>,
}

impl Walk<'_, '_> {
    fn excluded(&self, rel: &[u8]) -> bool {
        self.r.excl.iter().any(|e| under(rel, e.as_bytes()))
    }
    fn check(&mut self) -> bool {
        if self.stop {
            return true;
        }
        let cancelled = self.r.cancel.is_some_and(|c| c.load(Relaxed));
        if cancelled || self.r.checkpoint.as_mut().is_some_and(|f| f()) {
            self.stop = true;
        }
        self.stop
    }
    fn count(&self, c: &AtomicU64, n: u64) {
        c.fetch_add(n, Relaxed);
    }
}

/// `a`: the original; `b`: the one in the snapshot (encrypted: its size is the encrypted size of a).
fn same_file(a: &Metadata, b: &Metadata, encrypted: bool) -> bool {
    let want = if encrypted { crypt::enc_size(a.size()) } else { a.size() };
    b.is_file()
        && b.size() == want
        && a.mtime() == b.mtime()
        && (a.mtime_nsec() == b.mtime_nsec() || b.mtime_nsec() == 0)
        && (a.mode() & 0o7777) == (b.mode() & 0o7777)
}

/// Hard-links dst to base/rel if it is the same file.
fn try_link(base: &Option<PathBuf>, rel: &Path, st: &Metadata, dst: &Path, encrypted: bool) -> bool {
    let Some(base) = base else { return false };
    let p = base.join(rel);
    match fs::symlink_metadata(&p) {
        Ok(o) if same_file(st, &o, encrypted) => {}
        _ => return false,
    }
    match fs::hard_link(&p, dst) {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(dst);
            fs::hard_link(&p, dst).is_ok()
        }
        Err(_) => false, // EMLINK, EPERM (FAT/exFAT): copy instead
    }
}

fn is_nospace(e: i32) -> bool {
    e == libc::ENOSPC || e == libc::EDQUOT
}

fn set_times(file: Option<RawFd>, path: Option<&Path>, atime: (i64, i64), mtime: (i64, i64)) {
    let ts = [libc::timespec { tv_sec: atime.0 as _, tv_nsec: atime.1 as _ }, libc::timespec { tv_sec: mtime.0 as _, tv_nsec: mtime.1 as _ }];
    unsafe {
        match (file, path) {
            (Some(fd), _) => {
                libc::futimens(fd, ts.as_ptr());
            }
            (None, Some(p)) => {
                libc::utimensat(libc::AT_FDCWD, cstr(p).as_ptr(), ts.as_ptr(), libc::AT_SYMLINK_NOFOLLOW);
            }
            _ => {}
        }
    }
}

/// -1 file error, -2 out of space (fatal), 1 interrupted, 0 ok
fn copy_data(w: &mut Walk, input: RawFd, output: RawFd) -> i32 {
    let mut buf: Vec<u8> = Vec::new();
    let mut use_cfr = true;
    loop {
        let mut k: isize;
        if use_cfr {
            k = unsafe { libc::copy_file_range(input, std::ptr::null_mut(), output, std::ptr::null_mut(), CHUNK, 0) };
            if k < 0 && matches!(errno(), libc::EXDEV | libc::EINVAL | libc::ENOSYS | libc::EOPNOTSUPP | libc::EBADF) {
                use_cfr = false;
                continue;
            }
        } else {
            if buf.is_empty() {
                buf = vec![0u8; 1 << 20];
            }
            k = unsafe { libc::read(input, buf.as_mut_ptr().cast(), buf.len()) };
            let mut done = 0isize;
            while k > 0 && done < k {
                let wr = unsafe { libc::write(output, buf[done as usize..].as_ptr().cast(), (k - done) as usize) };
                if wr < 0 && errno() == libc::EINTR {
                    continue;
                }
                if wr <= 0 {
                    k = -1;
                    break;
                }
                done += wr;
            }
        }
        if k < 0 && errno() == libc::EINTR {
            continue;
        }
        if k < 0 {
            return if is_nospace(errno()) { -2 } else { -1 };
        }
        if k == 0 {
            break;
        }
        w.count(&w.r.stats.bytes, k as u64);
        w.count(&w.r.stats.copied_bytes, k as u64);
        if w.check() {
            return 1;
        }
    }
    0
}

/// -1 file error, -2 out of space (fatal), 1 interrupted, 0 ok
fn copy_file(w: &mut Walk, src: &Path, dst: &Path) -> i32 {
    let tmp = with_suffix(dst, PART);
    for attempt in 0..3 {
        let Ok(input) = fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NOCTTY).open(src) else { return -1 };
        let Ok(a) = input.metadata() else { return -1 };
        if !a.is_file() {
            return -1;
        }
        let output = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp);
        let Ok(output) = output else {
            let e = errno();
            return if is_nospace(e) { -2 } else { -1 };
        };
        let (fin, fout) = (input.as_raw_fd(), output.as_raw_fd());
        unsafe { libc::posix_fadvise(fin, 0, 0, libc::POSIX_FADV_SEQUENTIAL) };
        let before = w.r.stats.bytes.load(Relaxed);
        let mut r = match w.r.key {
            Some(key) => {
                let stats = w.r.stats;
                let (stop, cancel) = (&mut w.stop, w.r.cancel);
                let mut checkpoint = w.r.checkpoint.take();
                let code = crypt::encrypt_fd(fin, fout, key, &mut |n| {
                    stats.bytes.fetch_add(n as u64, Relaxed);
                    stats.copied_bytes.fetch_add(n as u64, Relaxed);
                    if *stop {
                        return true;
                    }
                    if cancel.is_some_and(|c| c.load(Relaxed)) || checkpoint.as_mut().is_some_and(|f| f()) {
                        *stop = true;
                    }
                    *stop
                });
                w.r.checkpoint = checkpoint;
                code
            }
            None => copy_data(w, fin, fout),
        };
        let changed = input.metadata().map(|b| b.size() != a.size() || b.mtime() != a.mtime() || b.mtime_nsec() != a.mtime_nsec()).unwrap_or(false);
        // do not pollute the desktop's page cache with the copy
        unsafe { libc::posix_fadvise(fin, 0, 0, libc::POSIX_FADV_DONTNEED) };
        if r == 0 {
            unsafe { libc::fchmod(fout, a.mode() & 0o7777) };
            set_times(Some(fout), None, (a.atime(), a.atime_nsec()), (a.mtime(), a.mtime_nsec()));
            // no fsync per file (too slow with many small files): syncfs at the end, before the
            // snapshot is renamed
            unsafe { libc::posix_fadvise(fout, 0, 0, libc::POSIX_FADV_DONTNEED) };
        }
        drop(input);
        if unsafe { libc::close(output.into_raw_fd()) } != 0 && r == 0 {
            r = -1;
        }
        if r != 0 {
            let _ = fs::remove_file(&tmp);
            return r;
        }
        if changed && attempt < 2 {
            w.r.stats.bytes.store(before, Relaxed); // changed while being copied
            continue;
        }
        if fs::rename(&tmp, dst).is_err() {
            let _ = fs::remove_file(&tmp);
            return -1;
        }
        return 0;
    }
    -1
}

fn entry(w: &mut Walk, rel: &Path, depth: usize) {
    if w.check() || w.fatal {
        return;
    }
    let (src, dst) = (w.src.join(rel), w.dst.join(rel));
    let st = match fs::symlink_metadata(&src) {
        Ok(st) => st,
        Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                w.count(&w.r.stats.errors, 1);
            }
            return;
        }
    };
    let ft = st.file_type();
    if ft.is_dir() {
        if st.dev() != w.home_dev || w.skip == Some((st.dev(), st.ino())) {
            return;
        }
        if depth >= MAX_DEPTH {
            w.count(&w.r.stats.errors, 1);
            return;
        }
        if !w.counting {
            use std::os::unix::fs::DirBuilderExt;
            if let Err(e) = fs::DirBuilder::new().mode(0o700).create(&dst) {
                if e.kind() != io::ErrorKind::AlreadyExists {
                    w.count(&w.r.stats.errors, 1);
                    return;
                }
            }
        }
        walk(w, rel, depth + 1);
        if !w.counting {
            unsafe { libc::chmod(cstr(&dst).as_ptr(), (st.mode() & 0o7777) | 0o700) }; // the owner can always get back in
            set_times(None, Some(&dst), (st.atime(), st.atime_nsec()), (st.mtime(), st.mtime_nsec()));
        }
        return;
    }
    if ft.is_file() {
        if w.counting {
            w.count(&w.r.stats.total_files, 1);
            w.count(&w.r.stats.total_bytes, st.size());
            return;
        }
        if let Some(progress) = w.r.progress.as_mut() {
            progress(rel.as_os_str().as_bytes());
        }
        w.count(&w.r.stats.files, 1);
        let encrypted = w.r.key.is_some();
        if try_link(&w.link1, rel, &st, &dst, encrypted) || try_link(&w.link2, rel, &st, &dst, encrypted) {
            w.count(&w.r.stats.linked, 1);
            w.count(&w.r.stats.bytes, st.size());
            return;
        }
        let mut r = copy_file(w, &src, &dst);
        if r == -2 {
            // out of space: delete old snapshots (never the newest) and retry once
            let need = st.size() + if w.r.min_free > 0 { w.r.min_free } else { DEFAULT_MIN_FREE };
            if prune(w.r.machine, now(), need) > 0 {
                r = copy_file(w, &src, &dst);
            }
            if r == -2 {
                w.fatal = true;
                w.error = errcode::code("backup.noSpace");
                return;
            }
        }
        if r == 0 {
            w.count(&w.r.stats.copied, 1);
        } else if r < 0 {
            w.count(&w.r.stats.errors, 1);
        }
        return;
    }
    if ft.is_symlink() {
        if w.counting {
            w.count(&w.r.stats.total_files, 1);
            return;
        }
        w.count(&w.r.stats.files, 1);
        let Ok(target) = fs::read_link(&src) else {
            w.count(&w.r.stats.errors, 1);
            return;
        };
        let _ = fs::remove_file(&dst);
        if std::os::unix::fs::symlink(&target, &dst).is_err() {
            w.count(&w.r.stats.errors, 1); // FAT has no symlinks
            return;
        }
        set_times(None, Some(&dst), (st.atime(), st.atime_nsec()), (st.mtime(), st.mtime_nsec()));
        return;
    }
    if !w.counting {
        w.count(&w.r.stats.skipped, 1); // fifos, sockets, devices
    }
}

fn now() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

/// The names in a folder opened without following a link at its path (the folder is closed
/// before descending: descriptors do not pile up).
fn names_nofollow(dir: &Path) -> io::Result<Vec<OsString>> {
    let fd = unsafe { libc::open(cstr(dir).as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let d = unsafe { libc::fdopendir(fd) };
    if d.is_null() {
        let e = io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(e);
    }
    let mut names = vec![];
    loop {
        let e = unsafe { libc::readdir(d) };
        if e.is_null() {
            break;
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*e).d_name.as_ptr()) }.to_bytes();
        if name != b"." && name != b".." {
            names.push(OsString::from_vec(name.to_vec()));
        }
    }
    unsafe { libc::closedir(d) };
    Ok(names)
}

fn walk(w: &mut Walk, rel: &Path, depth: usize) {
    let dir = w.src.join(rel);
    let Ok(mut names) = names_nofollow(&dir) else {
        w.count(&w.r.stats.errors, 1);
        return;
    };
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    for name in names {
        let bytes = name.as_bytes();
        if bytes.len() > 8 && bytes.ends_with(PART) {
            continue;
        }
        let child = rel.join(&name);
        if !w.excluded(child.as_os_str().as_bytes()) {
            entry(w, &child, depth);
        }
    }
}

/// A failed snapshot; `id` is the name already chosen, if any (as the C left it in the run).
fn fail(id: &str, error: Vec<u8>) -> Outcome {
    Outcome { code: -1, id: id.into(), error }
}

/// Takes a snapshot of `r.home` into `r.machine`, `now` being its time.
pub fn snapshot(r: &mut Run, mut now: i64) -> Outcome {
    r.stats.reset();
    let hs = match fs::symlink_metadata(r.home) {
        Ok(m) if m.is_dir() => m,
        _ => return fail("", errcode::code("backup.sourceUnreadable")),
    };
    let skip = r.skip_real.as_ref().and_then(|p| fs::metadata(p).ok()).map(|m| (m.dev(), m.ino()));
    let machine = r.machine.to_path_buf();
    if let Err(e) = mkdir_p(&machine, 0o700) {
        let err = lerr(256, "backup.createFailed", &[("path", machine.as_os_str().as_bytes()), ("errno", &errno_name(e.raw_os_error().unwrap_or(0)))]);
        return fail("", err);
    }
    let ids = list(&machine, MAX_SNAPS);
    let mut id = id_make(now);
    while ids.last().is_some_and(|last| id.as_str() <= last.as_str()) {
        now += 1;
        id = id_make(now); // always later than the previous one
    }
    let link2 = ids.last().map(|last| machine.join(last));

    // interrupted runs: the newest is a link source; the others are dropped
    let mut old: Option<PathBuf> = None;
    let mut newest = OsString::new();
    let names: Vec<OsString> = fs::read_dir(&machine).map(|d| d.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
    for name in &names {
        let b = name.as_bytes();
        if !b.starts_with(b".inprogress-") || !id_valid(&b[12..]) {
            continue;
        }
        if b > newest.as_bytes() {
            if !newest.is_empty() {
                let _ = rm_tree_force(&machine.join(&newest));
            }
            newest = name.clone();
        } else {
            let _ = rm_tree_force(&machine.join(name));
        }
    }
    if !newest.is_empty() {
        // renamed so it does not clash with the new one and is known to be the old one
        let resumed = machine.join(format!(".resume-{}", String::from_utf8_lossy(&newest.as_bytes()[12..])));
        if fs::rename(machine.join(&newest), &resumed).is_ok() {
            old = Some(resumed);
        }
    }
    // also a .resume- left by an interruption during a resume
    let mut resumes: Vec<&OsString> = names.iter().filter(|n| n.as_bytes().starts_with(b".resume-")).collect();
    resumes.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    for name in resumes.into_iter().rev() {
        let p = machine.join(name);
        if old.as_ref() == Some(&p) {
            continue;
        }
        if old.is_none() {
            old = Some(p);
        } else {
            let _ = rm_tree_force(&p);
        }
    }

    // minimum free space before starting
    let min_free = if r.min_free > 0 { r.min_free } else { DEFAULT_MIN_FREE };
    if free_bytes(&machine) < min_free {
        prune(&machine, now, min_free);
    }

    let stats = r.stats;
    let src = r.home.to_path_buf();
    let mut w = Walk { r, src, dst: PathBuf::new(), link1: old.clone(), link2, home_dev: hs.dev(), skip, counting: true, stop: false, fatal: false, error: vec![] };
    // count (for progress)
    walk(&mut w, Path::new(""), 0);
    w.counting = false;
    if w.stop {
        return Outcome { code: 1, id, error: vec![] };
    }
    w.dst = machine.join(format!(".inprogress-{id}"));
    {
        use std::os::unix::fs::DirBuilderExt;
        if let Err(e) = fs::DirBuilder::new().mode(0o700).create(&w.dst) {
            return fail(&id, lerr(256, "backup.destUnwritable", &[("errno", &errno_name(e.raw_os_error().unwrap_or(0)))]));
        }
    }
    walk(&mut w, Path::new(""), 0);
    if w.fatal {
        // left half done, to resume when there is space
        return Outcome { code: -1, id, error: w.error };
    }
    if w.stop {
        return Outcome { code: 1, id, error: vec![] };
    }
    let dst = w.dst.clone();
    unsafe { libc::chmod(cstr(&dst).as_ptr(), (hs.mode() & 0o7777) | 0o700) };
    if let Ok(d) = fs::File::open(&dst) {
        unsafe { libc::syncfs(d.as_raw_fd()) };
    }
    let fin = machine.join(&id);
    if let Err(e) = fs::rename(&dst, &fin) {
        return fail(&id, lerr(256, "backup.snapshotFailed", &[("errno", &errno_name(e.raw_os_error().unwrap_or(0)))]));
    }
    if let Ok(d) = fs::File::open(&machine) {
        let _ = d.sync_all();
    }
    if let Some(old) = old {
        let _ = rm_tree_force(&old);
    }
    prune(&machine, now, 0);
    let _ = stats;
    Outcome { code: 0, id, error: vec![] }
}

// ---- restore ----

/// What a restore counted.
#[derive(Default, Debug, Clone)]
pub struct RestoreStats {
    pub files: u64,
    pub bytes: u64,
    pub kept_both: u64,
    pub replaced: u64,
    pub skipped: u64,
    pub errors: u64,
    /// A code line, when something specific failed.
    pub error: Vec<u8>,
}

struct Rest<'a> {
    o: &'a mut RestoreStats,
    cancel: Option<&'a AtomicBool>,
    key: Option<&'a Key>,
    suffix: &'a [u8],
    replace: bool,
    policy: Policy,
}

impl Rest<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.is_some_and(|c| c.load(Relaxed))
    }
    /// May dst be written? Its (resolved) folder must be inside the lord.fs roots and the final
    /// name must not fall inside a protected folder.
    fn writable_target(&self, dst: &Path) -> bool {
        let (Some(parent), Some(name)) = (dst.parent(), dst.file_name()) else { return false };
        if parent == Path::new("/") || parent.as_os_str().is_empty() {
            return false;
        }
        let Ok(real) = fs::canonicalize(parent) else { return false };
        if !self.policy.allowed(&real) {
            return false;
        }
        !self.policy.protected(&real.join(name)) && !self.policy.protected(&real)
    }
}

/// "photo.jpg" + " (restored)" → "photo (restored).jpg", and if that exists "photo (restored) 2.jpg"
fn alt_name(dst: &Path, suffix: &[u8]) -> Option<PathBuf> {
    let bytes = dst.as_os_str().as_bytes();
    let name_at = bytes.iter().rposition(|b| *b == b'/').map_or(0, |i| i + 1);
    let name = &bytes[name_at..];
    let dot = match name.iter().rposition(|b| *b == b'.') {
        Some(0) | None => bytes.len(),
        Some(i) => name_at + i,
    };
    for k in 1..1000 {
        let mut out = bytes[..dot].to_vec();
        out.extend_from_slice(suffix);
        if k > 1 {
            out.extend_from_slice(format!(" {k}").as_bytes());
        }
        out.extend_from_slice(&bytes[dot..]);
        let p = path_of(&out);
        if fs::symlink_metadata(&p).is_err_and(|e| e.kind() == io::ErrorKind::NotFound) {
            return Some(p);
        }
    }
    None
}

fn restore_file(x: &mut Rest, src: &Path, dst: &Path, st: &Metadata) -> i32 {
    let tmp = with_suffix(dst, PART);
    let Ok(input) = fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(src) else { return -1 };
    let create = || fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp);
    let output = match create() {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&tmp);
            create()
        }
        other => other,
    };
    let Ok(output) = output else { return -1 };
    let (fin, fout) = (input.as_raw_fd(), output.as_raw_fd());
    let mut r;
    if let Some(key) = x.key {
        let (o, cancel) = (&mut *x.o, x.cancel);
        r = crypt::decrypt_fd(fin, fout, key, &mut |n| {
            o.bytes += n as u64;
            cancel.is_some_and(|c| c.load(Relaxed))
        });
        if r == -3 {
            x.o.error = errcode::code("backup.verifyFailed");
        }
        if r < 0 {
            r = -1;
        }
    } else {
        r = 0;
        let mut buf = vec![0u8; 1 << 20];
        while r == 0 {
            let k = unsafe { libc::read(fin, buf.as_mut_ptr().cast(), buf.len()) };
            if k < 0 && errno() == libc::EINTR {
                continue;
            }
            if k < 0 {
                r = -1;
                break;
            }
            if k == 0 {
                break;
            }
            let mut done = 0isize;
            while done < k {
                let w = unsafe { libc::write(fout, buf[done as usize..].as_ptr().cast(), (k - done) as usize) };
                if w < 0 && errno() == libc::EINTR {
                    continue;
                }
                if w <= 0 {
                    r = -1;
                    break;
                }
                done += w;
            }
            x.o.bytes += k as u64;
            if x.cancelled() {
                r = 1;
            }
        }
    }
    unsafe { libc::fchmod(fout, st.mode() & 0o7777) };
    set_times(Some(fout), None, (st.atime(), st.atime_nsec()), (st.mtime(), st.mtime_nsec()));
    drop(input);
    if unsafe { libc::close(output.into_raw_fd()) } != 0 && r == 0 {
        r = -1;
    }
    if r != 0 || fs::rename(&tmp, dst).is_err() {
        let _ = fs::remove_file(&tmp);
        return if r != 0 { r } else { -1 };
    }
    0
}

fn restore_entry(x: &mut Rest, src: &Path, dst: &Path, depth: usize) {
    if x.cancelled() || depth > MAX_DEPTH {
        return;
    }
    let Ok(st) = fs::symlink_metadata(src) else {
        x.o.errors += 1;
        return;
    };
    if !x.writable_target(dst) {
        x.o.skipped += 1;
        return;
    }
    let mut cur = fs::symlink_metadata(dst).ok();
    let mut target = dst.to_path_buf();

    if st.is_dir() {
        if cur.as_ref().is_some_and(|c| !c.is_dir()) {
            if x.replace {
                if fs::remove_file(dst).is_err() {
                    x.o.errors += 1;
                    return;
                }
                x.o.replaced += 1;
                cur = None;
            } else if let Some(alt) = alt_name(dst, x.suffix) {
                target = alt;
                x.o.kept_both += 1;
                cur = None;
            } else {
                x.o.errors += 1;
                return;
            }
        }
        let existed = cur.is_some();
        if !existed {
            use std::os::unix::fs::DirBuilderExt;
            if fs::DirBuilder::new().mode(0o700).create(&target).is_err() {
                x.o.errors += 1;
                return;
            }
        }
        let Ok(dir) = fs::read_dir(src) else {
            x.o.errors += 1;
            return;
        };
        let names: Vec<OsString> = dir.flatten().map(|e| e.file_name()).collect();
        for name in names {
            restore_entry(x, &src.join(&name), &target.join(&name), depth + 1);
        }
        if !existed {
            unsafe { libc::chmod(cstr(&target).as_ptr(), (st.mode() & 0o7777) | 0o700) };
            set_times(None, Some(&target), (st.atime(), st.atime_nsec()), (st.mtime(), st.mtime_nsec()));
        }
        return;
    }
    if !st.is_file() && !st.file_type().is_symlink() {
        x.o.skipped += 1;
        return;
    }
    if let Some(cur) = &cur {
        let mut plain = st.size();
        if x.key.is_some() && st.is_file() {
            plain = crypt::plain_size(st.size()).unwrap_or(u64::MAX);
        }
        if st.is_file() && cur.is_file() && cur.size() == plain && cur.mtime() == st.mtime() {
            x.o.skipped += 1; // identical: nothing to do
            return;
        }
        if cur.is_dir() || !x.replace {
            let Some(alt) = alt_name(dst, x.suffix) else {
                x.o.errors += 1;
                return;
            };
            target = alt;
            x.o.kept_both += 1;
        } else {
            x.o.replaced += 1;
        }
    }
    let r = if st.is_file() {
        restore_file(x, src, &target, &st)
    } else {
        let tmp = with_suffix(&target, PART);
        match fs::read_link(src) {
            Ok(link) => {
                let _ = fs::remove_file(&tmp);
                if std::os::unix::fs::symlink(&link, &tmp).is_err() || fs::rename(&tmp, &target).is_err() {
                    let _ = fs::remove_file(&tmp);
                    -1
                } else {
                    0
                }
            }
            Err(_) => -1,
        }
    };
    if r < 0 {
        x.o.errors += 1;
    } else if r == 0 {
        x.o.files += 1;
    }
}

/// Restores `rel` ("" = everything) from a snapshot into base/rel (base: normally home).
/// `replace` false: if the target exists and differs, the restored copy goes next to it as
/// "name<suffix>" (suffix comes from the UI, no '/'); true: it is replaced. Never writes where
/// lord.fs refuses to (protected) nor outside its roots (allowed).
/// Returns 0 done, 1 interrupted, -1 error (`out.error`).
pub fn restore(machine: &Path, id: &str, rel: &str, base: &Path, replace: bool, suffix: &[u8], cancel: Option<&AtomicBool>, key: Option<&Key>, out: &mut RestoreStats) -> i32 {
    *out = RestoreStats::default();
    if !id_valid(id.as_bytes()) || !rel_valid(rel.as_bytes()) {
        out.error = errcode::code("backup.badPath");
        return -1;
    }
    if suffix.is_empty() || suffix.len() > 64 || suffix.contains(&b'/') || suffix.iter().any(|c| *c < 0x20) {
        out.error = errcode::code("backup.badSuffix");
        return -1;
    }
    let Ok(snapreal) = fs::canonicalize(machine.join(id)) else {
        out.error = lerr(256, "backup.noSuchBackup", &[("id", id.as_bytes())]);
        return -1;
    };
    let policy = policy::current();
    let mut x = Rest { o: out, cancel, key, suffix, replace, policy };
    let src = if rel.is_empty() { snapreal.clone() } else { snapreal.join(rel) };
    // no links along the path inside the snapshot: the resolved folder must stay inside
    if !rel.is_empty() {
        let ok = src.parent().and_then(|p| fs::canonicalize(p).ok()).is_some_and(|real| under(real.as_os_str().as_bytes(), snapreal.as_os_str().as_bytes()));
        if !ok {
            x.o.error = errcode::code("backup.badPath");
            return -1;
        }
    }
    let Some(breal) = fs::canonicalize(base).ok().filter(|b| x.policy.allowed(b)) else {
        x.o.error = errcode::code("backup.destNotAllowed");
        return -1;
    };
    if rel.is_empty() {
        // everything: the snapshot's contents, inside base
        if let Ok(dir) = fs::read_dir(&src) {
            for e in dir.flatten() {
                if x.cancelled() {
                    break;
                }
                restore_entry(&mut x, &src.join(e.file_name()), &breal.join(e.file_name()), 0);
            }
        }
    } else {
        // create the missing folders up to the target, never through links to forbidden places
        let mut cur = breal;
        let parts: Vec<&str> = rel.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            let p = cur.join(part);
            if fs::symlink_metadata(&p).is_err() {
                use std::os::unix::fs::DirBuilderExt;
                if let Err(e) = fs::DirBuilder::new().mode(0o700).create(&p) {
                    x.o.error = lerr(256, "backup.createFailed", &[("path", p.as_os_str().as_bytes()), ("errno", &errno_name(e.raw_os_error().unwrap_or(0)))]);
                    return -1;
                }
            }
            match fs::canonicalize(&p) {
                Ok(pr) if x.policy.allowed(&pr) && !x.policy.protected(&pr) => cur = pr,
                _ => {
                    x.o.error = errcode::code("backup.destNotAllowed");
                    return -1;
                }
            }
        }
        let dst = cur.join(parts[parts.len() - 1]);
        restore_entry(&mut x, &src, &dst, 0);
    }
    if x.cancelled() {
        return 1;
    }
    0
}
