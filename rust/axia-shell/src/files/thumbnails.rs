//! Bounded, lazy thumbnails. At most two decoders run; the UI never decodes
//! full-size pictures while scrolling through a directory.
use super::policy;
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
static REVISION: AtomicU64 = AtomicU64::new(0);
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

#[derive(Default)]
struct Cache {
    ready: HashMap<PathBuf, (u64, String)>,
    pending: HashSet<PathBuf>,
    active: usize,
}
fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Cache::default()))
}
pub fn get(entry: &super::operations::Entry) -> Option<String> {
    let path = &entry.raw;
    if !path.is_absolute() {
        return None;
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hash);
    entry.stamp.hash(&mut hash);
    entry.mtime.hash(&mut hash);
    entry.size.hash(&mut hash);
    let key = hash.finish();
    let mut c = cache().lock().unwrap();
    if let Some((version, file)) = c.ready.get(path) {
        if *version == key {
            return (!file.is_empty()).then(|| file.clone());
        }
    }
    if c.active >= 2 || c.pending.contains(path) {
        return None;
    }
    if c.ready.len() > 1024 {
        c.ready.clear();
    }
    c.active += 1;
    c.pending.insert(path.to_owned());
    drop(c);
    let path = path.to_owned();
    let task_path = path.clone();
    let spawned = std::thread::Builder::new()
        .name("axia-files-thumbnail".into())
        .spawn(move || {
            let path = task_path;
            let result = (|| -> std::result::Result<String, Box<dyn std::error::Error>> {
                let p = policy::current();
                let source = p.read(&path)?;
                let root = std::env::var_os("XDG_CACHE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| p.home.join(".cache"))
                    .join("axia/files/thumbnails");
                fs::create_dir_all(&root)?;
                let m = fs::symlink_metadata(&root)?;
                if !m.is_dir() || m.file_type().is_symlink() || m.uid() != unsafe { libc::getuid() }
                {
                    return Err("unsafe thumbnail cache".into());
                }
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
                let out = root.join(format!("{key:016x}.png"));
                if out.is_file() {
                    return Ok(out.to_string_lossy().into());
                }
                let mut reader = image::ImageReader::open(&source)?.with_guessed_format()?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(10000);
                limits.max_image_height = Some(10000);
                limits.max_alloc = Some(32 * 1024 * 1024);
                reader.limits(limits);
                let thumb = reader.decode()?.thumbnail(320, 240);
                let temp = root.join(format!(".{key:016x}-{}.tmp", std::process::id()));
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                    .open(&temp)?;
                let mut writer = std::io::BufWriter::new(file);
                let result = (|| -> std::result::Result<(), Box<dyn std::error::Error>> {
                    use std::io::Write;
                    thumb.write_to(&mut writer, image::ImageFormat::Png)?;
                    writer.flush()?;
                    Ok(())
                })();
                drop(writer);
                if result.is_err() {
                    let _ = fs::remove_file(&temp);
                    result?;
                }
                fs::rename(&temp, &out)?;
                // Disk budget: oldest entries first. This work is off the UI thread.
                let mut files: Vec<_> = fs::read_dir(&root)?
                    .flatten()
                    .filter_map(|e| {
                        e.metadata()
                            .ok()
                            .map(|m| (e.path(), m.len(), m.modified().ok()))
                    })
                    .collect();
                let mut bytes: u64 = files.iter().map(|e| e.1).sum();
                if bytes > 128 * 1024 * 1024 {
                    files.sort_by_key(|e| e.2);
                    for (f, size, _) in files {
                        if bytes <= 96 * 1024 * 1024 {
                            break;
                        }
                        if f != out && fs::remove_file(f).is_ok() {
                            bytes = bytes.saturating_sub(size);
                        }
                    }
                }
                Ok(out.to_string_lossy().into())
            })();
            let mut c = cache().lock().unwrap();
            c.active = c.active.saturating_sub(1);
            c.pending.remove(&path);
            c.ready.insert(path, (key, result.unwrap_or_default()));
            REVISION.fetch_add(1, Ordering::Relaxed);
        });
    if spawned.is_err() {
        let mut c = cache().lock().unwrap();
        c.active = c.active.saturating_sub(1);
        c.pending.remove(&path);
    }
    None
}
