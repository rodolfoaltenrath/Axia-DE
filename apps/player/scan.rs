//! The walk of the media folders and the tag cache, on a worker thread (a large folder never
//! blocks the window). The tags themselves are read by `media.probe`, one job per new file.
use super::model::*;
use serde_json::Value;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

/// What the walk found: the tracks the cache already knows, the videos, and the files to probe.
pub struct Listed {
    pub known: Vec<Track>,
    pub videos: Vec<Video>,
    pub todo: Vec<Found>,
    /// Entries in the cache file before this walk.
    pub cached: usize,
}

fn walk(dir: &Path, test: fn(&str) -> bool, out: &mut Vec<Found>, depth: usize) {
    if depth > WALK_DEPTH || out.len() > WALK_LIMIT {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by(|a, b| collate(&a.file_name().to_string_lossy(), &b.file_name().to_string_lossy()));
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        // metadata follows links, like the folder listing the JSX used; the depth bounds a loop
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.is_dir() {
            walk(&path, test, out, depth + 1);
        } else if test(&name) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0., |d| d.as_secs_f64());
            out.push(Found { path: path.to_string_lossy().into_owned(), name, size: meta.len(), mtime });
        }
    }
}
/// Files of the two folders, in the order the library shows them.
pub fn files(music: &str, videos: &str) -> (Vec<Found>, Vec<Found>) {
    let (mut audio, mut video) = (vec![], vec![]);
    if !music.is_empty() {
        walk(Path::new(music), is_audio, &mut audio, 0);
    }
    if !videos.is_empty() {
        walk(Path::new(videos), is_video, &mut video, 0);
    }
    (audio, video)
}
pub fn read_cache(home: &str) -> Value {
    let text = axia_native::read(&Path::new(home).join(CACHE_FILE), 64 << 20);
    serde_json::from_str::<Value>(&text).ok().filter(Value::is_object).unwrap_or_else(|| serde_json::json!({}))
}
/// Walk and read the cache in the background; the receiver gets one `Listed`.
pub fn start(home: String, music: String, videos: String) -> Receiver<Listed> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let (audio, video) = files(&music, &videos);
        let cache = read_cache(&home);
        let cached = cache.as_object().map_or(0, |o| o.len());
        let (known, todo) = partition(&audio, &cache);
        let videos = video.iter().map(video_from).collect();
        let _ = tx.send(Listed { known, videos, todo, cached });
    });
    rx
}
/// Write the cache without waiting for the disk. A failure only costs a re-read next time.
pub fn write_cache(home: &str, tracks: &[Track]) {
    if home.is_empty() {
        return;
    }
    let (home, cache) = (home.to_owned(), cache_json(tracks));
    std::thread::spawn(move || {
        let dir = Path::new(&home).join(CACHE_DIR);
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(Path::new(&home).join(CACHE_FILE), cache.to_string());
        }
    });
}
