use super::*;
use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const LIMIT: u64 = 256 * 1024;
thread_local! {
    static ROOT: RefCell<CString> = RefCell::new(CString::default());
    static GLOBS: RefCell<Option<Option<Vec<u8>>>> = const { RefCell::new(None) };
}
pub(super) fn root() -> Vec<u8> {
    ROOT.with(|r| r.borrow().as_bytes().to_vec())
}
pub(super) fn system(suffix: &[u8]) -> PathBuf {
    let mut p = root();
    p.extend_from_slice(suffix);
    PathBuf::from(OsStr::from_bytes(&p))
}
pub(super) fn home() -> Option<PathBuf> {
    env("HOME")
        .filter(|p| p.starts_with(b"/"))
        .map(|p| PathBuf::from(OsStr::from_bytes(&p)))
}
fn config() -> Option<PathBuf> {
    env("XDG_CONFIG_HOME")
        .filter(|p| p.starts_with(b"/"))
        .map(|p| PathBuf::from(OsStr::from_bytes(&p)))
        .or_else(|| home().map(|p| p.join(".config")))
}
pub(super) fn read(path: &Path) -> Option<Vec<u8>> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let m = f.metadata().ok()?;
    if !m.is_file() || m.len() > LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    (&mut f).take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > LIMIT {
        return None;
    }
    // Existing text consumers stop at NUL; preserve that behavior.
    if let Some(i) = bytes.iter().position(|b| *b == 0) {
        bytes.truncate(i);
    }
    Some(bytes)
}
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
fn write_atomic(path: &Path, text: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    // Never truncate an existing temp file (including hard links). Publish only
    // after a complete write and fsync; clean up only the file we created.
    let (tmp, mut file) = loop {
        let tmp = dir.join(format!(
            ".axia-defaults-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
        {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        file.write_all(text)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub(super) fn load() -> Vec<(Vec<u8>, bool)> {
    let mut paths = Vec::new();
    if let Some(c) = config() {
        paths.push((c.join("mimeapps.list"), true));
    }
    paths.push((system(b"/etc/xdg/mimeapps.list"), false));
    if let Some(h) = home() {
        paths.push((h.join(".local/share/applications/mimeapps.list"), true));
    }
    for s in [
        b"/usr/local/share/applications/mimeapps.list".as_slice(),
        b"/usr/share/applications/mimeapps.list",
        b"/usr/share/applications/defaults.list",
    ] {
        paths.push((system(s), false));
    }
    paths
        .into_iter()
        .map(|(p, u)| (read(&p).unwrap_or_default(), u))
        .collect()
}
pub(super) fn is_removed(files: &[(Vec<u8>, bool)], upto: usize, m: &[u8], id: &[u8]) -> bool {
    files
        .iter()
        .take(upto.saturating_add(1))
        .any(|(t, _)| mime::removed(t, m, id))
}
fn edit(m: &[u8], id: Option<&[u8]>) -> i32 {
    if !mime::valid(m) || id.is_some_and(|v| !mime::desktop_valid(v)) {
        return -1;
    }
    let Some(dir) = config() else {
        return -1;
    };
    let path = dir.join("mimeapps.list");
    // Missing is different from unreadable/oversized/symlink: preserve invalid files.
    let old = match read(&path) {
        Some(v) => v,
        None if fs::symlink_metadata(&path)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Vec::new()
        }
        None => return -1,
    };
    let text = match id {
        Some(id) => mime::set(&old, m, id),
        None => mime::unset(&old, m),
    };
    if old == text {
        return 0;
    }
    if write_atomic(&path, &text).is_ok() {
        0
    } else {
        -1
    }
}

#[no_mangle]
pub unsafe extern "C" fn defaults_set_root(p: *const c_char) {
    let p = bytes(p);
    ROOT.with(|r| {
        *r.borrow_mut() = cstring(if p.starts_with(b"/") {
            &p[..p.len().min(255)]
        } else {
            b""
        })
    });
    GLOBS.with(|g| *g.borrow_mut() = None);
    desktop::invalidate();
}
#[no_mangle]
pub unsafe extern "C" fn defaults_lookup(m: *const c_char, out: *mut c_char, n: usize) -> i32 {
    let m = bytes(m);
    if !mime::valid(m) {
        return -1;
    }
    let files = load();
    for (i, (t, _)) in files.iter().enumerate() {
        if let Some(id) = mime::entries(t, b"Default Applications", m).find(|v| v.len() < n) {
            if !is_removed(&files, i, m, id) {
                return output(out, n, id);
            }
        }
    }
    -1
}
#[no_mangle]
pub unsafe extern "C" fn defaults_set(m: *const c_char, id: *const c_char) -> i32 {
    edit(bytes(m), Some(bytes(id)))
}
#[no_mangle]
pub unsafe extern "C" fn defaults_unset(m: *const c_char) -> i32 {
    edit(bytes(m), None)
}
#[no_mangle]
pub unsafe extern "C" fn defaults_guess(p: *const c_char, out: *mut c_char, n: usize) -> i32 {
    GLOBS.with(|g| {
        let mut g = g.borrow_mut();
        if g.is_none() {
            *g = Some(read(&system(b"/usr/share/mime/globs2")));
        }
        g.as_ref()
            .and_then(|g| g.as_ref())
            .and_then(|t| mime::guess(t, bytes(p), n))
            .map_or(-1, |v| output(out, n, &v))
    })
}
#[no_mangle]
pub unsafe extern "C" fn defaults_terminal(out: *mut c_char, n: usize) -> i32 {
    let Some(t) = config().and_then(|p| read(&p.join("xdg-terminals.list"))) else {
        return -1;
    };
    for line in t.split(|b| *b == b'\n') {
        let line = line.trim_ascii_start();
        let line = line.strip_prefix(b"+").unwrap_or(line).trim_ascii_start();
        let id = line
            .split(|b| b" :\t\r".contains(b))
            .next()
            .unwrap_or_default();
        if mime::desktop_valid(id) && id.len() < n {
            return output(out, n, id);
        }
    }
    -1
}
#[no_mangle]
pub unsafe extern "C" fn defaults_set_terminal(id: *const c_char) -> i32 {
    let id = bytes(id);
    if !mime::desktop_valid(id) {
        return -1;
    }
    let Some(dir) = config() else {
        return -1;
    };
    let mut text = "# Written by Axia DE (Settings → Default Apps)\n"
        .as_bytes()
        .to_vec();
    text.extend_from_slice(id);
    text.push(b'\n');
    if write_atomic(&dir.join("xdg-terminals.list"), &text).is_ok() {
        0
    } else {
        -1
    }
}
