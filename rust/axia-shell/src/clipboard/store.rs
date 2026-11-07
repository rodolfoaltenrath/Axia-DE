//! Worker-only disk access. The history format and legacy HOME location stay compatible.
use super::{Entry, IMAGE_BYTES, TEXT_BYTES};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
const FILE_BYTES: u64 = 32 * 1024 * 1024;
// Keep every successfully saved index readable within the import budget, even
// when JSON escaping expands control characters to six bytes each.
struct IndexWriter<W> {
    file: W,
    remaining: u64,
}
impl<W: Write> Write for IndexWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(invalid());
        }
        let written = self.file.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
pub struct Store {
    pub dir: PathBuf,
    pub runtime: Option<PathBuf>,
    pub writable: bool,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "clipboard history is not a private regular file",
    )
}
fn private_dir(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => (),
            Ok(_) => return Err(invalid()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(&current)?;
            }
            Err(e) => return Err(e),
        }
    }
    if fs::metadata(path)?.uid() != unsafe { libc::geteuid() } {
        return Err(invalid());
    }
    Ok(())
}
fn open(path: &Path, limit: u64) -> io::Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let m = f.metadata()?;
    if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.len() > limit {
        return Err(invalid());
    }
    Ok(f)
}
fn image_name(name: &str) -> bool {
    name.starts_with("img-")
        && name.ends_with(".png")
        && name.len() > 8
        && name.len() < 96
        && name[4..name.len() - 4]
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
impl Store {
    pub fn new(home: &Path, runtime: Option<&Path>) -> Self {
        Self {
            dir: home.join(".local/share/axia/clipboard"),
            runtime: runtime.map(|p| p.join("axia")),
            writable: true,
        }
    }
    pub fn owned(&self, path: &Path) -> bool {
        let name = path.file_name().and_then(|p| p.to_str()).unwrap_or("");
        (path.parent() == Some(self.dir.as_path()) && image_name(name))
            || (path.parent() == self.runtime.as_deref()
                && name.starts_with("clip-rust-")
                && name.ends_with(".png")
                && !name.contains('/')
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b)))
    }
    pub fn image_exists(&self, path: &Path) -> bool {
        self.owned(path) && open(path, IMAGE_BYTES).is_ok()
    }
    pub fn load(&mut self) -> io::Result<Vec<Entry>> {
        let path = self.dir.join("history.json");
        if !path.exists() {
            // Still reject a dangling symlink instead of overwriting it.
            if fs::symlink_metadata(&path).is_ok() {
                self.writable = false;
                return Err(invalid());
            }
            return Ok(vec![]);
        }
        let result = (|| {
            private_dir(&self.dir)?;
            let file = open(&path, FILE_BYTES)?;
            let v: Value = serde_json::from_reader(file.take(FILE_BYTES + 1))?;
            if v["version"] != 1 || !v["items"].is_array() {
                return Err(invalid());
            }
            let mut entries = Vec::new();
            let mut total = 0usize;
            for raw in v["items"].as_array().unwrap() {
                let Ok(mut e) = serde_json::from_value::<Entry>(raw.clone()) else {
                    continue;
                };
                if e.id.is_empty()
                    || e.id.len() > 128
                    || e.id.contains('\0')
                    || !e.time.is_finite()
                    || e.origin.len() > 128
                {
                    continue;
                }
                if let Some(ref text) = e.text {
                    e.image = None;
                    e.bytes = 0;
                    if text.len() > super::ITEM_BYTES {
                        return Err(invalid());
                    }
                    total = total.saturating_add(text.len());
                } else if let Some(ref image) = e.image {
                    if !self.image_exists(Path::new(image)) {
                        continue;
                    }
                    e.bytes = open(Path::new(image), IMAGE_BYTES)?.metadata()?.len();
                } else {
                    continue;
                }
                if total > TEXT_BYTES || entries.len() >= super::MAX_ITEMS {
                    return Err(invalid());
                }
                // Runtime preview names restart their counter each session. They
                // identify repeated captures only until logout, never after import.
                e.src = None;
                e.kind = if e.image.is_some() { "image" } else { "text" }.into();
                if !entries.iter().any(|old: &Entry| old.id == e.id) {
                    entries.push(e)
                }
            }
            Ok(entries)
        })();
        if result.is_err() {
            self.writable = false
        }
        result
    }
    fn atomic(
        &self,
        path: &Path,
        write: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> io::Result<()> {
        let parent = path.parent().ok_or_else(invalid)?;
        private_dir(parent)?;
        // Refuse links/devices/FIFOs; rename never follows a final symlink.
        match fs::symlink_metadata(path) {
            Ok(m)
                if !m.is_file()
                    || m.file_type().is_symlink()
                    || m.uid() != unsafe { libc::geteuid() } =>
            {
                return Err(invalid())
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => (),
        }
        let temp = parent.join(format!(
            ".clipboard-{}-{}",
            std::process::id(),
            super::next_id()
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            write(&mut file)?;
            file.sync_all()?;
            fs::rename(&temp, path)?;
            File::open(parent)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
    pub fn save(&self, items: &[Entry]) -> io::Result<()> {
        if !self.writable {
            return Err(invalid());
        }
        self.atomic(&self.dir.join("history.json"), |f| {
            let mut buffer = io::BufWriter::with_capacity(64 * 1024, f);
            let writer = IndexWriter {
                file: &mut buffer,
                remaining: FILE_BYTES,
            };
            serde_json::to_writer(writer, &serde_json::json!({"version":1,"items":items}))?;
            buffer.flush()
        })
    }
    pub fn image(&self, mut source: File, save: bool) -> io::Result<(String, u64)> {
        let m = source.metadata()?;
        if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.len() > IMAGE_BYTES {
            return Err(invalid());
        }
        let path = if save {
            self.dir.join(format!("img-{}.png", super::next_id()))
        } else {
            self.runtime
                .as_ref()
                .ok_or_else(invalid)?
                .join(format!("clip-rust-{}.png", super::next_id()))
        };
        let mut bytes = 0;
        self.atomic(&path, |out| {
            let mut signature = [0; 8];
            source.read_exact(&mut signature)?;
            if &signature != b"\x89PNG\r\n\x1a\n" {
                return Err(invalid());
            }
            out.write_all(&signature)?;
            bytes = 8 + io::copy(&mut source.take(IMAGE_BYTES - 7), out)?;
            if bytes > IMAGE_BYTES {
                return Err(invalid());
            }
            Ok(())
        })?;
        Ok((path.to_string_lossy().into_owned(), bytes))
    }
    pub fn remove(&self, path: &str) {
        let p = Path::new(path);
        if self.owned(p) && private_dir(p.parent().unwrap()).is_ok() {
            let _ = fs::remove_file(p);
        }
    }
    pub fn transient(&self, entry: &mut Entry) {
        let Some(path) = entry.image.as_ref() else {
            return;
        };
        if Path::new(path).parent() != Some(self.dir.as_path()) {
            return;
        }
        if let Ok(f) = open(Path::new(path), IMAGE_BYTES) {
            if let Ok((new, bytes)) = self.image(f, false) {
                let old = path.clone();
                entry.image = Some(new);
                entry.bytes = bytes;
                self.remove(&old);
            }
        }
    }
    pub fn persistent(&self, entry: &mut Entry) {
        let Some(path) = entry.image.as_ref() else {
            return;
        };
        if Path::new(path).parent() == Some(self.dir.as_path()) {
            return;
        }
        if let Ok(f) = open(Path::new(path), IMAGE_BYTES) {
            if let Ok((new, bytes)) = self.image(f, true) {
                let old = path.clone();
                entry.image = Some(new);
                entry.bytes = bytes;
                self.remove(&old);
            }
        }
    }
}
