//! Freedesktop Trash with no-overwrite publication and fd-relative removal.
use super::{io_error, operations, policy, Result};
use serde::Serialize;
use std::ffi::CStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub original: String,
    pub deleted: String,
    pub dir: bool,
    pub size: u64,
}

fn directories() -> Vec<PathBuf> {
    let p = policy::current();
    let mut out = vec![p.home.join(".local/share/Trash")];
    for (mount, kind) in operations::mounts() {
        if !share_fs(&kind) {
            out.push(mount.join(format!(".Trash-{}", unsafe { libc::getuid() })));
        }
    }
    out
}
pub fn share_fs(kind: &str) -> bool {
    matches!(
        kind,
        "fuse.rclone" | "cifs" | "smb3" | "nfs" | "nfs4" | "fuse.sshfs" | "fuse.gvfsd-fuse"
    )
}

fn root_for(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    let p = policy::current();
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if path.starts_with(&p.home) && fs::metadata(&p.home).map_err(io_error)?.dev() == metadata.dev()
    {
        return Ok((p.home.join(".local/share/Trash"), None));
    }
    let mut top = path.to_owned();
    while let Some(parent) = top.parent() {
        match fs::metadata(parent) {
            Ok(m) if m.dev() == metadata.dev() => top = parent.to_owned(),
            _ => break,
        }
    }
    if !p.allowed(&top) {
        return Err("fs.noTrash".into());
    }
    Ok((
        top.join(format!(".Trash-{}", unsafe { libc::getuid() })),
        Some(top),
    ))
}

fn mkdir_private_tree(path: &Path) -> Result<()> {
    // Refuse symlinks in every existing component, including the trash's files
    // and info folders: metadata must not redirect deletion outside the trash.
    let mut current = PathBuf::from("/");
    for part in path.components() {
        if let std::path::Component::Normal(n) = part {
            current.push(n);
            match fs::symlink_metadata(&current) {
                Ok(m) if m.is_dir() && !m.file_type().is_symlink() => (),
                Ok(_) => return Err("fs.noTrash".into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current).map_err(io_error)?
                }
                Err(e) => return Err(io_error(e)),
            }
        }
    }
    Ok(())
}
fn ensure(root: &Path) -> Result<()> {
    mkdir_private_tree(root)?;
    if fs::metadata(root).map_err(io_error)?.uid() != unsafe { libc::getuid() } {
        return Err("fs.noTrash".into());
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    for dir in [root.join("files"), root.join("info")] {
        mkdir_private_tree(&dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    Ok(())
}
fn percent_encode(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(b) {
            out.push(*b as char)
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn percent_decode(s: &str) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut n = 0;
    while n < b.len() {
        if b[n] == b'%' && n + 2 < b.len() {
            let h = (b[n + 1] as char).to_digit(16);
            let l = (b[n + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (h, l) {
                let v = (h * 16 + l) as u8;
                if v == 0 {
                    return Err("fs.trashBadOrigin".into());
                }
                out.push(v);
                n += 3;
                continue;
            }
        }
        out.push(b[n]);
        n += 1;
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(out)))
}
fn timestamp() -> String {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
        if libc::localtime_r(&t, tm.as_mut_ptr()).is_null() {
            return String::new();
        }
        let tm = tm.assume_init();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

pub fn put(path: &Path) -> Result<String> {
    let p = policy::current();
    let path = p.target(path, true)?;
    let (parent, name) = policy::anchored(&path).map_err(io_error)?;
    put_at(parent.as_raw_fd(), &name, &path)
}

pub fn put_at(fd: i32, name: &CStr, real: &Path) -> Result<String> {
    let p = policy::current();
    if !p.allowed(real) {
        return Err("fs.outsideHome".into());
    }
    if p.protected(real) || p.protected_below(real) {
        return Err("fs.protected".into());
    }
    if name.to_bytes().contains(&b'/') && fd != libc::AT_FDCWD {
        return Err("sys.badArgument".into());
    }
    let (root, top) = root_for(real)?;
    if real.starts_with(&root) {
        return Err("fs.alreadyInTrash".into());
    }
    ensure(&root)?;
    let base = real.file_name().ok_or("sys.badArgument")?.to_string_lossy();
    let base: String = base
        .chars()
        .scan(0, |bytes, c| {
            *bytes += c.len_utf8();
            (*bytes <= 220).then_some(c)
        })
        .collect();
    for n in 1..10000 {
        let entry = if n == 1 {
            base.clone()
        } else {
            format!("{base}.{n}")
        };
        let info = root.join("info").join(format!("{entry}.trashinfo"));
        let dest = root.join("files").join(&entry);
        if fs::symlink_metadata(&dest).is_ok() {
            continue;
        }
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&info)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(e)),
        };
        let original = top
            .as_ref()
            .and_then(|t| real.strip_prefix(t).ok())
            .unwrap_or(real);
        let contents = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            percent_encode(original),
            timestamp()
        );
        if let Err(e) = file
            .write_all(contents.as_bytes())
            .and_then(|_| file.sync_all())
        {
            let _ = fs::remove_file(&info);
            return Err(io_error(e));
        }
        let (to, to_name) = policy::anchored(&dest).map_err(io_error)?;
        let rc = unsafe {
            libc::renameat2(
                fd,
                name.as_ptr(),
                to.as_raw_fd(),
                to_name.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if rc < 0 {
            let e = std::io::Error::last_os_error();
            let _ = fs::remove_file(&info);
            return Err(if e.raw_os_error() == Some(libc::EXDEV) {
                "fs.trashOtherDisk".into()
            } else {
                io_error(e)
            });
        }
        return Ok(dest.to_string_lossy().into());
    }
    Err("fs.trashEntryFailed".into())
}
fn validate(id: &Path) -> Result<(PathBuf, String)> {
    for dir in directories() {
        if let Ok(tail) = id.strip_prefix(dir.join("files")) {
            let mut c = tail.components();
            let name = match (c.next(), c.next()) {
                (Some(std::path::Component::Normal(n)), None) => n.to_string_lossy().into_owned(),
                _ => continue,
            };
            policy::open_dir(&dir.join("files")).map_err(|_| "fs.notInTrash".to_string())?;
            match policy::open_dir(&dir.join("info")) {
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err("fs.notInTrash".into()),
            }
            return Ok((dir, name));
        }
    }
    Err("fs.notInTrash".into())
}
fn info(root: &Path, name: &str) -> Result<(PathBuf, String)> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.join("info").join(format!("{name}.trashinfo")))
        .map_err(io_error)?;
    use std::io::Read;
    let mut text = String::new();
    f.take(64 * 1024)
        .read_to_string(&mut text)
        .map_err(io_error)?;
    let raw = text
        .lines()
        .find_map(|l| l.strip_prefix("Path="))
        .ok_or("fs.trashBadOrigin")?;
    let path = percent_decode(raw)?;
    let path = if path.is_absolute() {
        path
    } else {
        root.parent().ok_or("fs.trashBadOrigin")?.join(path)
    };
    let date = text
        .lines()
        .find_map(|l| l.strip_prefix("DeletionDate="))
        .unwrap_or("")
        .into();
    Ok((path, date))
}
pub fn list() -> Vec<Item> {
    let mut out = Vec::new();
    for root in directories() {
        if policy::open_dir(&root.join("files")).is_err() {
            continue;
        }
        let entries = match fs::read_dir(root.join("files")) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten().take(5000) {
            let name = e.file_name().to_string_lossy().into_owned();
            let (original, date) = info(&root, &name).unwrap_or_default();
            if let Ok(meta) = fs::symlink_metadata(e.path()) {
                out.push(Item {
                    id: e.path().to_string_lossy().into(),
                    name,
                    original: original.to_string_lossy().into(),
                    deleted: date,
                    dir: meta.is_dir(),
                    size: meta.len(),
                });
            }
        }
    }
    out
}
pub fn restore(id: &Path) -> Result<()> {
    let (root, name) = validate(id)?;
    let (original, _) = info(&root, &name)?;
    let p = policy::current();
    let target = p.target(&original, true).map_err(|e| {
        if e.starts_with("sys.errno") {
            "fs.trashOriginGone".into()
        } else {
            e
        }
    })?;
    policy::rename_noreplace(id, &target).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            "fs.trashRestoreExists".into()
        } else {
            io_error(e)
        }
    })?;
    fs::remove_file(root.join("info").join(format!("{name}.trashinfo"))).map_err(io_error)?;
    Ok(())
}
pub fn remove(id: &Path) -> Result<()> {
    let (root, name) = validate(id)?;
    operations::remove_tree(id).map_err(io_error)?;
    let _ = fs::remove_file(root.join("info").join(format!("{name}.trashinfo")));
    Ok(())
}
pub fn empty() -> Result<usize> {
    let mut n = 0;
    for item in list() {
        remove(Path::new(&item.id))?;
        n += 1;
    }
    Ok(n)
}
