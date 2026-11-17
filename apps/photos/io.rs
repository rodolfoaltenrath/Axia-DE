//! Filesystem work stays on the worker. File identities are checked again on
//! open; scans don't follow directory links and saves exclusively create copies.
use std::{
    collections::{HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
pub const MAX_FILE: u64 = 128 << 20;
pub const MAX_PIXELS: u64 = 40_000_000;
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub mtime: i64,
    pub stamp: (u64, u64, i64, i64),
}
impl Entry {
    pub fn inspect(path: &Path) -> Result<Self, String> {
        let m = fs::metadata(path).map_err(err)?;
        if !m.is_file() {
            return Err("sys.badArgument".into());
        }
        Ok(Self {
            path: path.to_owned(),
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            size: m.len(),
            mtime: m.mtime(),
            stamp: (m.dev(), m.ino(), m.mtime(), m.mtime_nsec()),
        })
    }
}
pub fn err(e: std::io::Error) -> String {
    crate::files::io_error(e)
}
pub fn supported(p: &Path) -> bool {
    p.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "png"
                | "jpg"
                | "jpeg"
                | "webp"
                | "gif"
                | "bmp"
                | "heic"
                | "heif"
                | "avif"
                | "tif"
                | "tiff"
        )
    })
}
pub fn policy(home: &Path) -> crate::files::policy::Policy {
    let mut p = crate::files::policy::current();
    p.home = home.to_owned();
    if !p.roots.contains(&p.home) {
        p.roots.insert(0, p.home.clone())
    }
    p
}
pub fn read(entry: &Entry, home: &Path) -> Result<File, String> {
    let path = policy(home).read(&entry.path)?;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(err)?;
    let m = f.metadata().map_err(err)?;
    if !m.is_file()
        || m.len() > MAX_FILE
        || (m.dev(), m.ino(), m.mtime(), m.mtime_nsec()) != entry.stamp
        || m.len() != entry.size
    {
        return Err("fs.sourceChanged".into());
    }
    Ok(f)
}
pub fn pictures(home: &Path) -> PathBuf {
    let config = home.join(".config/user-dirs.dirs");
    let mut text = String::new();
    if let Ok(f) = File::open(config) {
        let _ = f.take(65536).read_to_string(&mut text);
    }
    for line in text.lines() {
        if let Some(v) = line.trim().strip_prefix("XDG_PICTURES_DIR=") {
            let v = v
                .trim()
                .trim_matches('"')
                .replace("$HOME", &home.to_string_lossy());
            let p = PathBuf::from(v);
            if p.is_absolute() {
                return p;
            }
        }
    }
    home.join("Pictures")
}
pub fn scan(
    folder: &Path,
    recursive: bool,
    home: &Path,
    generation: u64,
    current: &AtomicU64,
) -> Result<(Vec<Entry>, Vec<PathBuf>), String> {
    let root = policy(home).read(folder)?;
    let mut work = VecDeque::from([(root.clone(), 0usize)]);
    let mut seen = HashSet::new();
    let mut out = vec![];
    let mut folders = vec![];
    let mut visited = 0;
    while let Some((dir, depth)) = work.pop_front() {
        if current.load(Ordering::Relaxed) != generation {
            return Err("cancelled".into());
        }
        let listing = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(e) if depth == 0 => return Err(err(e)),
            Err(_) => continue,
        };
        for de in listing.flatten() {
            visited += 1;
            if visited > 100_000 || out.len() >= 20_000 {
                break;
            }
            let p = de.path();
            let Ok(m) = de.metadata() else { continue };
            if de.file_type().is_ok_and(|t| t.is_symlink()) {
                continue;
            }
            if m.is_dir() {
                if depth == 0 {
                    folders.push(p.clone())
                }
                if recursive && depth < 4 && seen.insert((m.dev(), m.ino())) {
                    work.push_back((p, depth + 1))
                }
            } else if m.is_file() && supported(&p) {
                if let Ok(e) = Entry::inspect(&p) {
                    out.push(e)
                }
            }
        }
        if visited > 100_000 || out.len() >= 20_000 {
            break;
        }
    }
    out.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path)));
    folders.sort();
    Ok((out, folders))
}
pub fn neighbor_sort(entries: &mut [Entry]) {
    entries.sort_by(|a, b| crate::files::natural_cmp(&a.name, &b.name));
}
// Encode completely before publishing. link() is atomic and refuses to replace
// any existing destination, including a symlink made during the name search.
pub fn save(
    entry: &Entry,
    home: &Path,
    rgba: &image::RgbaImage,
    suffix: &str,
) -> Result<PathBuf, String> {
    use std::os::fd::AsRawFd;
    let p = policy(home);
    let parent = entry.path.parent().ok_or("sys.badArgument")?;
    let jpeg = entry
        .path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    let ext = if jpeg { "jpg" } else { "png" };
    let stem = entry.path.file_stem().unwrap_or_default().to_string_lossy();
    let suffix = suffix.replace(['/', '\\', '\0'], "-");
    let mut data = std::io::Cursor::new(Vec::new());
    if jpeg {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 92)
            .encode_image(&image::DynamicImage::ImageRgb8(
                image::DynamicImage::ImageRgba8(rgba.clone()).to_rgb8(),
            ))
            .map_err(|_| "media.encodeFailed")?
    } else {
        rgba.write_to(&mut data, image::ImageFormat::Png)
            .map_err(|_| "media.encodeFailed")?
    }
    let tmp = p.target(
        &parent.join(format!(
            ".axia-photos-{}-{}.tmp",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )),
        false,
    )?;
    let (dir, tmp_name) = crate::files::policy::anchored(&tmp).map_err(err)?;
    let mut f = crate::files::policy::open_at(
        &dir,
        tmp.file_name().unwrap(),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
        0o600,
    )
    .map_err(err)?;
    let result = (|| {
        f.write_all(data.get_ref()).map_err(err)?;
        f.sync_all().map_err(err)?;
        for n in 1..=999 {
            let name = format!(
                "{stem} ({suffix}{}).{ext}",
                if n == 1 {
                    String::new()
                } else {
                    format!(" {n}")
                }
            );
            let out = p.target(&parent.join(&name), false)?;
            let name = crate::files::policy::c_name(std::ffi::OsStr::new(&name)).map_err(err)?;
            if unsafe {
                libc::linkat(
                    dir.as_raw_fd(),
                    tmp_name.as_ptr(),
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    0,
                )
            } == 0
            {
                dir.sync_all().map_err(err)?;
                return Ok(out);
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(err(e));
            }
        }
        Err("fs.exists".into())
    })();
    unsafe {
        libc::unlinkat(dir.as_raw_fd(), tmp_name.as_ptr(), 0);
    }
    result
}

static COUNTER: AtomicU64 = AtomicU64::new(0);
