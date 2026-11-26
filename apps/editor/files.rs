//! The editor's disk access, confined by the same policy as the desktop's file service: it reads
//! and writes inside the home folder and the mounted disks, never in the protected folders.
//! Writing is atomic and keeps the permissions.
use axia_fs::policy::Policy;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_OPEN: u64 = 8 << 20;
const MAX_WRITE: usize = 32 << 20;
/// "Go to File": how many files and how deep.
const MAX_FILES: usize = 4000;
const MAX_DEPTH: usize = 8;
/// A folder of the project panel shows at most this many entries.
const MAX_ENTRIES: usize = 3000;
const SKIP: [&str; 7] = ["node_modules", ".git", "build", "dist", ".cache", "__pycache__", "target"];

#[derive(Debug, PartialEq)]
pub enum ReadError {
    TooBig(u64),
    /// A NUL byte or text that is not UTF-8: saving it back would corrupt it.
    Binary,
    /// An error line for the catalogs ("fs.systemFolder", "sys.errno errno=…").
    Failed(String),
}

pub fn read(policy: &Policy, path: &Path) -> Result<String, ReadError> {
    let failed = |e: std::io::Error| ReadError::Failed(axia_fs::io_error(e));
    let real = policy.read(path).map_err(ReadError::Failed)?;
    // non-blocking: a FIFO must not hang the window; only regular files are read
    let file = fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(real).map_err(failed)?;
    let meta = file.metadata().map_err(failed)?;
    if !meta.is_file() {
        return Err(ReadError::Failed("fs.notRegular".into()));
    }
    if meta.len() > MAX_OPEN {
        return Err(ReadError::TooBig(meta.len()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_OPEN + 1).read_to_end(&mut bytes).map_err(failed)?;
    if bytes.len() as u64 > MAX_OPEN {
        return Err(ReadError::TooBig(bytes.len() as u64));
    }
    if bytes.contains(&0) {
        return Err(ReadError::Binary);
    }
    String::from_utf8(bytes).map_err(|_| ReadError::Binary)
}

/// Creates or replaces a file. The new text is complete on disk before it takes the name.
pub fn write(policy: &Policy, path: &Path, text: &str) -> Result<(), String> {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    if text.len() > MAX_WRITE {
        return Err("fs.tooLarge max=32%20MB".into());
    }
    let target = policy.target(path, false)?;
    let mode = match fs::symlink_metadata(&target) {
        Ok(meta) if !meta.is_file() => return Err("fs.notRegular".into()),
        Ok(meta) => meta.permissions().mode() & 0o7777,
        Err(_) => 0o644,
    };
    let folder = target.parent().ok_or("sys.badArgument")?;
    let temporary = folder.join(format!(".axia-write-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &target)
    })();
    result.map_err(|e| {
        let _ = fs::remove_file(&temporary);
        axia_fs::io_error(e)
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    pub dir: bool,
}
/// The visible entries of a folder: folders first, then by name without case or accents.
pub fn list(policy: &Policy, folder: &Path) -> Vec<Item> {
    let Ok(real) = policy.read(folder) else { return Vec::new() };
    let Ok(entries) = fs::read_dir(real) else { return Vec::new() };
    let mut out: Vec<(bool, String, Item)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            // a link to a folder opens like a folder
            let dir = fs::metadata(entry.path()).is_ok_and(|m| m.is_dir());
            Some((!dir, axia_native::matching::fold(&name), Item { name, dir }))
        })
        .take(MAX_ENTRIES)
        .collect();
    out.sort_by(|a, b| (a.0, &a.1, &a.2.name).cmp(&(b.0, &b.1, &b.2.name)));
    out.into_iter().map(|(_, _, item)| item).collect()
}

/// The files below `root`, as paths relative to it, skipping build and cache folders. Links to
/// folders are not followed, so a cycle cannot repeat files.
pub fn walk(policy: &Policy, root: &Path) -> Vec<String> {
    fn visit(policy: &Policy, folder: PathBuf, relative: &str, depth: usize, out: &mut Vec<String>) {
        if out.len() >= MAX_FILES || depth > MAX_DEPTH {
            return;
        }
        for item in list(policy, &folder) {
            let name = if relative.is_empty() { item.name.clone() } else { format!("{relative}/{}", item.name) };
            let path = folder.join(&item.name);
            if !item.dir {
                out.push(name);
            } else if !SKIP.contains(&item.name.as_str()) && fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                visit(policy, path, &name, depth + 1, out);
            }
            if out.len() >= MAX_FILES {
                return;
            }
        }
    }
    let mut out = Vec::new();
    visit(policy, root.to_owned(), "", 0, &mut out);
    out
}
