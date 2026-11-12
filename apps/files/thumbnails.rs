//! Bounded, lazy thumbnails. One decoder runs; the UI never decodes
//! full-size pictures while scrolling through a directory.
use super::policy;
use image::ImageDecoder;
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
    if c.active >= 1 || c.pending.contains(path) {
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
                let file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
                    .open(&source)?;
                let metadata = file.metadata()?;
                if !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
                    return Err("unsupported thumbnail source".into());
                }
                let mut reader =
                    image::ImageReader::new(std::io::BufReader::new(file)).with_guessed_format()?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(10000);
                limits.max_image_height = Some(10000);
                limits.max_alloc = Some(64 * 1024 * 1024);
                reader.limits(limits);
                let mut decoder = reader.into_decoder()?;
                if decoder.total_bytes() > 64 * 1024 * 1024 {
                    return Err("thumbnail pixel budget exceeded".into());
                }
                let orientation = decoder
                    .orientation()
                    .unwrap_or(image::metadata::Orientation::NoTransforms);
                let mut decoded = image::DynamicImage::from_decoder(decoder)?;
                decoded.apply_orientation(orientation);
                let thumb = decoded.thumbnail(decoded.width().min(320), decoded.height().min(240));
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
                if let Err(error) = fs::rename(&temp, &out) {
                    let _ = fs::remove_file(&temp);
                    return Err(error.into());
                }
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
                        if f != out && fs::remove_file(&f).is_ok() {
                            bytes = bytes.saturating_sub(size);
                            // Do not keep returning a PNG evicted from disk.
                            // The next visible request can recreate it lazily.
                            cache()
                                .lock()
                                .unwrap()
                                .ready
                                .retain(|_, (_, path)| PathBuf::from(&*path) != f);
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

#[cfg(test)]
mod tests {
    use super::super::{operations, tests::Home};
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn supported_images_generate_small_thumbnails_and_changed_images_refresh() {
        let h = Home::new();
        for (name, format) in [
            ("image.png", image::ImageFormat::Png),
            ("image.jpg", image::ImageFormat::Jpeg),
            ("image.webp", image::ImageFormat::WebP),
            ("image.gif", image::ImageFormat::Gif),
            ("image.bmp", image::ImageFormat::Bmp),
        ] {
            let source = h.path(name);
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                800,
                600,
                image::Rgb([40, 120, 220]),
            ))
            .save_with_format(&source, format)
            .unwrap();
            let e = operations::entry(&source).unwrap();
            let until = Instant::now() + Duration::from_secs(5);
            let path = loop {
                if let Some(p) = get(&e) {
                    break p;
                }
                assert!(Instant::now() < until, "missing thumbnail for {name}");
                std::thread::sleep(Duration::from_millis(5));
            };
            let (w, h) = image::image_dimensions(&path).unwrap();
            assert!(w <= 320 && h <= 240);
            assert_eq!(w * 3, h * 4);
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                100,
                100,
                image::Rgb([220, 20, 50]),
            ))
            .save_with_format(&source, format)
            .unwrap();
            let changed = operations::entry(&source).unwrap();
            let path2 = loop {
                if let Some(p) = get(&changed) {
                    break p;
                }
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(5));
            };
            assert_ne!(path, path2);
            assert_eq!(image::image_dimensions(&path2).unwrap(), (100, 100));
        }
    }
    #[test]
    fn broken_images_and_fifos_do_not_stall_the_decoder() {
        let h = Home::new();
        h.file("broken.png", "not a picture");
        let fifo = h.path("pipe.png");
        let raw = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
        for p in [h.path("broken.png"), fifo] {
            let e = operations::entry(&p).unwrap();
            assert!(get(&e).is_none());
            let until = Instant::now() + Duration::from_secs(2);
            loop {
                let c = cache().lock().unwrap();
                let done = c.ready.contains_key(&p) && !c.pending.contains(&p);
                drop(c);
                if done {
                    break;
                }
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(get(&e).is_none());
        }
    }
}

#[cfg(test)]
mod large_image {
    use super::super::{operations, tests::Home};
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn an_ordinary_twelve_megapixel_photo_fits_the_thumbnail_budget() {
        let h = Home::new();
        let source = h.path("phone.jpg");
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            4000,
            3000,
            image::Rgb([40, 120, 220]),
        ))
        .save_with_format(&source, image::ImageFormat::Jpeg)
        .unwrap();
        let e = operations::entry(&source).unwrap();
        let until = Instant::now() + Duration::from_secs(15);
        let path = loop {
            if let Some(p) = get(&e) {
                break p;
            }
            assert!(
                Instant::now() < until,
                "12 MP JPEG did not produce a thumbnail"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(image::image_dimensions(path).unwrap(), (320, 240));
    }
}
