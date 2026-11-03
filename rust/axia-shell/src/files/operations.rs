use super::{
    io_error,
    policy::{self, Policy},
    Result,
};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

#[derive(Clone, Serialize, Default)]
pub struct Job {
    pub id: i32,
    pub src: String,
    pub dst: String,
    #[serde(rename = "move")]
    pub moving: bool,
    pub done: bool,
    pub failed: bool,
    pub cancel: bool,
    pub bytes: u64,
    pub total: u64,
    pub files: u64,
    pub error: String,
}
pub struct Task {
    pub progress: Mutex<Job>,
    pub cancelled: AtomicBool,
}
fn tasks() -> &'static Mutex<Vec<Arc<Task>>> {
    static TASKS: OnceLock<Mutex<Vec<Arc<Task>>>> = OnceLock::new();
    TASKS.get_or_init(|| Mutex::new(Vec::new()))
}
pub fn jobs() -> Vec<Job> {
    tasks()
        .lock()
        .unwrap()
        .iter()
        .map(|t| t.progress.lock().unwrap().clone())
        .collect()
}
pub fn cancel(id: i32) {
    for t in tasks().lock().unwrap().iter() {
        if t.progress.lock().unwrap().id == id {
            t.cancelled.store(true, Ordering::Relaxed);
        }
    }
}
pub fn forget(id: i32) {
    tasks().lock().unwrap().retain(|t| {
        let j = t.progress.lock().unwrap();
        j.id != id || !j.done
    });
}

fn interrupted(t: &Task) -> std::io::Result<()> {
    if t.cancelled.load(Ordering::Relaxed) {
        Err(std::io::Error::from_raw_os_error(libc::ECANCELED))
    } else {
        Ok(())
    }
}
fn fd_path(fd: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd()))
}

fn timestamps(file: &File, metadata: &fs::Metadata) {
    let ts = [
        libc::timespec {
            tv_sec: metadata.atime(),
            tv_nsec: metadata.atime_nsec(),
        },
        libc::timespec {
            tv_sec: metadata.mtime(),
            tv_nsec: metadata.mtime_nsec(),
        },
    ];
    unsafe {
        libc::futimens(file.as_raw_fd(), ts.as_ptr());
    }
}

// Every recursive descent is relative to open directory handles and refuses
// symlinks. Symlink entries are copied as links and never traversed.
fn copy_at(
    source: &File,
    sn: &std::ffi::OsStr,
    dest: &File,
    dn: &std::ffi::OsStr,
    task: &Task,
    depth: usize,
    created: &mut bool,
) -> std::io::Result<()> {
    interrupted(task)?;
    if depth > 64 {
        return Err(std::io::Error::from_raw_os_error(libc::ELOOP));
    }
    let name = policy::c_name(sn)?;
    let target = policy::c_name(dn)?;
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            source.as_raw_fd(),
            name.as_ptr(),
            st.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let st = unsafe { st.assume_init() };
    match st.st_mode & libc::S_IFMT {
        libc::S_IFLNK => {
            let mut bytes = vec![0u8; 4096];
            let n = unsafe {
                libc::readlinkat(
                    source.as_raw_fd(),
                    name.as_ptr(),
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                )
            };
            if n < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if n as usize == bytes.len() {
                return Err(std::io::Error::from_raw_os_error(libc::ENAMETOOLONG));
            }
            bytes.truncate(n as usize);
            let link = std::ffi::CString::new(bytes).unwrap();
            if unsafe { libc::symlinkat(link.as_ptr(), dest.as_raw_fd(), target.as_ptr()) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if depth == 0 {
                *created = true;
            }
            task.progress.lock().unwrap().files += 1;
        }
        libc::S_IFREG => {
            let mut input = policy::open_at(
                source,
                sn,
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                0,
            )?;
            let meta = input.metadata()?;
            if !meta.is_file() {
                return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
            }
            let mut output = policy::open_at(
                dest,
                dn,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
                0o600,
            )?;
            if depth == 0 {
                *created = true;
            }
            let mut buf = vec![0; 256 * 1024];
            loop {
                interrupted(task)?;
                let n = input.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                output.write_all(&buf[..n])?;
                task.progress.lock().unwrap().bytes += n as u64;
            }
            output.set_permissions(fs::Permissions::from_mode(meta.mode() & 0o777))?;
            timestamps(&output, &meta);
            output.sync_all()?;
            task.progress.lock().unwrap().files += 1;
        }
        libc::S_IFDIR => {
            let input = policy::open_at(
                source,
                sn,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                0,
            )?;
            if unsafe { libc::mkdirat(dest.as_raw_fd(), target.as_ptr(), 0o700) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if depth == 0 {
                *created = true;
            }
            let output = policy::open_at(
                dest,
                dn,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                0,
            )?;
            for item in fs::read_dir(fd_path(&input))? {
                let item = item?;
                copy_at(
                    &input,
                    &item.file_name(),
                    &output,
                    &item.file_name(),
                    task,
                    depth + 1,
                    created,
                )?;
            }
            let meta = input.metadata()?;
            output.set_permissions(fs::Permissions::from_mode(meta.mode() & 0o777))?;
            timestamps(&output, &meta);
        }
        _ => return Err(std::io::Error::from_raw_os_error(libc::ENOTSUP)),
    }
    Ok(())
}

pub fn remove_tree(path: &Path) -> std::io::Result<()> {
    let (parent, name) = policy::anchored(path)?;
    remove_at(&parent, std::ffi::OsStr::from_bytes(name.as_bytes()), 0)
}
fn remove_at(parent: &File, name: &std::ffi::OsStr, depth: usize) -> std::io::Result<()> {
    if depth > 64 {
        return Err(std::io::Error::from_raw_os_error(libc::ELOOP));
    }
    let n = policy::c_name(name)?;
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            n.as_ptr(),
            st.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let st = unsafe { st.assume_init() };
    let directory = (st.st_mode & libc::S_IFMT) == libc::S_IFDIR;
    if directory {
        let dir = policy::open_at(
            parent,
            name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        )?;
        for entry in fs::read_dir(fd_path(&dir))? {
            remove_at(&dir, &entry?.file_name(), depth + 1)?;
        }
    }
    if unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            n.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub fn rename(from: &Path, to: &Path) -> Result<()> {
    let p = policy::current();
    let from = p.target(from, true)?;
    let to = p.target(to, true)?;
    policy::rename_noreplace(&from, &to).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            "fs.exists".into()
        } else {
            io_error(e)
        }
    })
}

struct Prepared {
    src: PathBuf,
    dst: PathBuf,
    moving: bool,
    original: fs::Metadata,
}
fn prepare(src: &Path, dst: &Path, moving: bool) -> Result<Prepared> {
    let p = policy::current();
    let src = if moving {
        p.target(src, true)?
    } else {
        let abs = p.absolute(src);
        let parent = p.read(abs.parent().ok_or("sys.badArgument")?)?;
        parent.join(abs.file_name().ok_or("sys.badArgument")?)
    };
    let dst = p.target(dst, true)?;
    if dst == src || dst.starts_with(&src) {
        return Err("fs.copyIntoItself".into());
    }
    let original = fs::symlink_metadata(&src).map_err(io_error)?;
    if fs::symlink_metadata(&dst).is_ok() {
        return Err("fs.exists".into());
    }
    Ok(Prepared {
        src,
        dst,
        moving,
        original,
    })
}
fn perform(item: Prepared, task: &Task, id: i32) -> std::io::Result<()> {
    let Prepared {
        src,
        dst,
        moving,
        original,
    } = item;
    interrupted(task)?;
    if moving {
        match policy::rename_noreplace(&src, &dst) {
            Ok(()) => {
                task.progress.lock().unwrap().files += 1;
                return Ok(());
            }
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => (),
            Err(e) => return Err(e),
        }
    }
    let staging = dst.with_file_name(format!(
        ".axia-copy-{}-{id}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut created = false;
    let result = (|| -> std::io::Result<()> {
        task.progress.lock().unwrap().total +=
            walk(&src, 64, 1_000_000, DurationBudget::seconds(5), true)
                .0
                .iter()
                .map(|(_, m)| if m.is_file() { m.len() } else { 0 })
                .sum::<u64>();
        let (a, an) = policy::anchored(&src)?;
        let (b, bn) = policy::anchored(&staging)?;
        copy_at(
            &a,
            std::ffi::OsStr::from_bytes(an.as_bytes()),
            &b,
            std::ffi::OsStr::from_bytes(bn.as_bytes()),
            task,
            0,
            &mut created,
        )?;
        interrupted(task)?;
        policy::rename_noreplace(&staging, &dst)?;
        if moving {
            let now = fs::symlink_metadata(&src)?;
            if (now.dev(), now.ino()) != (original.dev(), original.ino()) {
                return Err(std::io::Error::from_raw_os_error(libc::ESTALE));
            }
            remove_tree(&src)?;
        }
        Ok(())
    })();
    if result.is_err() && created && fs::symlink_metadata(&staging).is_ok() {
        let _ = remove_tree(&staging);
    }
    result
}
pub fn start(src: &Path, dst: &Path, moving: bool) -> Result<i32> {
    start_batch(vec![(src.into(), dst.into(), moving)])
}
// A multi-selection is one bounded job, not one slot/thread per selected file.
pub fn start_batch(items: Vec<(PathBuf, PathBuf, bool)>) -> Result<i32> {
    if items.is_empty() || items.len() > 4096 {
        return Err("sys.badArgument".into());
    }
    let items: Vec<_> = items
        .into_iter()
        .map(|(s, d, m)| prepare(&s, &d, m))
        .collect::<Result<_>>()?;
    static SEQ: AtomicI32 = AtomicI32::new(1);
    let mut all = tasks().lock().unwrap();
    if all.len() >= 8 {
        return Err("fs.tooManyCopies".into());
    }
    let id = SEQ.fetch_add(1, Ordering::Relaxed);
    let first = &items[0];
    let task = Arc::new(Task {
        progress: Mutex::new(Job {
            id,
            src: first.src.to_string_lossy().into(),
            dst: first.dst.to_string_lossy().into(),
            moving: first.moving,
            ..Job::default()
        }),
        cancelled: AtomicBool::new(false),
    });
    all.push(task.clone());
    drop(all);
    let worker = task.clone();
    let spawned = std::thread::Builder::new()
        .name("axia-files-copy".into())
        .spawn(move || {
            for item in items {
                if let Err(e) = perform(item, &worker, id) {
                    let cancelled = e.raw_os_error() == Some(libc::ECANCELED);
                    let mut j = worker.progress.lock().unwrap();
                    j.failed = true;
                    if j.error.is_empty() {
                        j.error = io_error(e);
                    }
                    if cancelled {
                        break;
                    }
                }
            }
            let mut j = worker.progress.lock().unwrap();
            j.cancel = worker.cancelled.load(Ordering::Relaxed);
            j.done = true;
        });
    if spawned.is_err() {
        tasks()
            .lock()
            .unwrap()
            .retain(|t| t.progress.lock().unwrap().id != id);
        return Err("fs.copyStartFailed".into());
    }
    Ok(id)
}

pub struct DurationBudget(Instant);
impl DurationBudget {
    pub fn seconds(n: u64) -> Self {
        Self(Instant::now() + std::time::Duration::from_secs(n))
    }
    pub fn millis(n: f64) -> Self {
        Self(Instant::now() + std::time::Duration::from_millis(n.clamp(1.0, 5000.0) as u64))
    }
}
pub fn walk(
    root: &Path,
    depth: usize,
    limit: usize,
    budget: DurationBudget,
    hidden: bool,
) -> (Vec<(PathBuf, fs::Metadata)>, bool) {
    let mut queue = vec![(root.to_owned(), 0)];
    let mut out = Vec::new();
    let mut complete = true;
    while let Some((path, d)) = queue.pop() {
        if out.len() >= limit || Instant::now() > budget.0 {
            complete = false;
            break;
        }
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        if meta.is_dir() {
            if d < depth {
                match fs::read_dir(&path) {
                    Ok(entries) => {
                        for e in entries.flatten() {
                            if hidden || !e.file_name().as_bytes().starts_with(b".") {
                                queue.push((e.path(), d + 1));
                            }
                        }
                    }
                    Err(_) => complete = false,
                }
            } else {
                complete = false;
            }
        }
        out.push((path, meta));
    }
    (out, complete)
}

#[derive(Clone, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub link: bool,
    pub size: u64,
    pub mtime: i64,
    pub mode: u32,
    pub writable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
    #[serde(skip)]
    pub raw: PathBuf,
    #[serde(skip)]
    pub stamp: u64,
}
pub fn entry(path: &Path) -> std::io::Result<Entry> {
    let link = fs::symlink_metadata(path)?;
    let meta = if link.file_type().is_symlink() {
        fs::metadata(path).unwrap_or(link.clone())
    } else {
        link.clone()
    };
    let c = policy::c_name(path.as_os_str())?;
    Ok(Entry {
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into(),
        path: path.to_string_lossy().into(),
        dir: meta.is_dir(),
        link: link.file_type().is_symlink(),
        size: meta.len(),
        mtime: meta.mtime() * 1000,
        mode: meta.mode(),
        writable: unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0,
        original: None,
        raw: path.into(),
        stamp: (meta.mtime_nsec() as u64) ^ meta.ino(),
    })
}
pub fn list(p: &Policy, path: &Path) -> Result<Vec<Entry>> {
    let path = p.read(path)?;
    let mut out = Vec::new();
    for item in fs::read_dir(path).map_err(io_error)? {
        let item = item.map_err(io_error)?;
        if let Ok(e) = entry(&item.path()) {
            out.push(e);
        }
        if out.len() >= 100_000 {
            break;
        }
    }
    Ok(out)
}
pub fn mkdir(path: &Path) -> Result<()> {
    let path = policy::current().target(path, false)?;
    let (parent, name) = policy::anchored(&path).map_err(io_error)?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o755) } < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(())
}
pub fn mounts() -> Vec<(PathBuf, String)> {
    let p = policy::current();
    fs::read_to_string("/proc/mounts")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            it.next()?;
            let raw = it.next()?;
            let kind = it.next()?.to_string();
            let bytes = raw.as_bytes();
            let mut out = Vec::new();
            let mut n = 0;
            while n < bytes.len() {
                if bytes[n] == b'\\'
                    && n + 3 < bytes.len()
                    && bytes[n + 1..n + 4]
                        .iter()
                        .all(|b| (b'0'..=b'7').contains(b))
                {
                    out.push(
                        (bytes[n + 1] - b'0') * 64 + (bytes[n + 2] - b'0') * 8 + bytes[n + 3]
                            - b'0',
                    );
                    n += 4;
                } else {
                    out.push(bytes[n]);
                    n += 1;
                }
            }
            let path = PathBuf::from(std::ffi::OsString::from_vec(out));
            if p.roots
                .iter()
                .skip(1)
                .any(|r| path.starts_with(r) && path != *r)
                || path.starts_with(p.home.join(".local/share/axia/remote"))
            {
                Some((path, kind))
            } else {
                None
            }
        })
        .collect()
}
pub fn free(path: &Path) -> std::io::Result<(u64, u64)> {
    let c = policy::c_name(path.as_os_str())?;
    let mut s = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(c.as_ptr(), s.as_mut_ptr()) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let s = unsafe { s.assume_init() };
    Ok((s.f_blocks * s.f_frsize, s.f_bavail * s.f_frsize))
}
