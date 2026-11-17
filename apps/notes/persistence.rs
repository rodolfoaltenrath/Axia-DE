//! One ordered disk worker per Notes session. Snapshots are coalesced by slug;
//! neither scanning Markdown nor fsync runs on the compositor thread.
use super::store::{self, Note};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
};
const QUEUE: usize = 32;
#[derive(Clone)]
pub(super) struct Save {
    pub note: Note,
    pub revision: u64,
}
pub(super) enum Event {
    Loaded(Result<(PathBuf, Vec<Note>), String>),
    Saved {
        slug: String,
        revision: u64,
        result: Result<(), String>,
    },
}
#[derive(Default)]
struct State {
    order: VecDeque<String>,
    saves: BTreeMap<String, Save>,
    stop: bool,
    #[cfg(test)]
    paused: bool,
}
impl State {
    fn paused(&self) -> bool {
        #[cfg(test)]
        {
            self.paused
        }
        #[cfg(not(test))]
        {
            false
        }
    }
}
pub(super) struct Persistence {
    shared: Arc<(Mutex<State>, Condvar)>,
    pub events: mpsc::Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}
impl Persistence {
    pub fn new(home: PathBuf) -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let (tx, events) = mpsc::channel();
        let worker = shared.clone();
        let thread = thread::Builder::new()
            .name("axia-notes-storage".into())
            .spawn(move || {
                let loaded = (|| -> io::Result<_> {
                    let home = fs::canonicalize(home)?;
                    if !home.join("Notes").exists() && home.join("Notas").exists() {
                        fs::rename(home.join("Notas"), home.join("Notes"))?;
                    }
                    fs::create_dir_all(home.join("Notes"))?;
                    let notes = store::load(&home)?;
                    Ok((home, notes))
                })()
                .map_err(|e| e.to_string());
                let root = loaded.as_ref().ok().map(|(home, _)| home.clone());
                if tx.send(Event::Loaded(loaded)).is_err() {
                    return;
                }
                loop {
                    let save = {
                        let (lock, wake) = &*worker;
                        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                        while (state.order.is_empty() || state.paused()) && !state.stop {
                            state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                        }
                        let Some(slug) = state.order.pop_front() else {
                            return;
                        };
                        let save = state.saves.remove(&slug).unwrap();
                        wake.notify_all();
                        save
                    };
                    let result = root
                        .as_ref()
                        .ok_or_else(|| "Notes storage unavailable".to_string())
                        .and_then(|home| write(home, &save.note).map_err(|e| e.to_string()));
                    if let Err(error) = &result {
                        eprintln!("notes: cannot save {}: {error}", save.note.slug);
                    }
                    let _ = tx.send(Event::Saved {
                        slug: save.note.slug,
                        revision: save.revision,
                        result,
                    });
                }
            })
            .ok();
        Self {
            shared,
            events,
            thread,
        }
    }
    pub fn available(&self) -> bool {
        self.thread.is_some()
    }
    pub fn can_submit(&self, slug: &str) -> bool {
        let state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        self.available() && (state.saves.len() < QUEUE || state.saves.contains_key(slug))
    }
    pub fn submit(&self, save: Save, wait: bool) -> bool {
        if self.thread.is_none() {
            return false;
        }
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        while state.saves.len() >= QUEUE && !state.saves.contains_key(&save.note.slug) {
            if !wait {
                return false;
            }
            state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        if !state.saves.contains_key(&save.note.slug) {
            state.order.push_back(save.note.slug.clone());
        }
        state.saves.insert(save.note.slug.clone(), save);
        wake.notify_one();
        true
    }
    #[cfg(test)]
    pub fn pause(&self, paused: bool) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap().paused = paused;
        wake.notify_all();
    }
    #[cfg(test)]
    pub fn queued(&self) -> usize {
        self.shared.0.lock().unwrap().saves.len()
    }
    pub fn finish(&mut self) {
        {
            let (lock, wake) = &*self.shared;
            lock.lock().unwrap_or_else(|e| e.into_inner()).stop = true;
            wake.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Persistence {
    fn drop(&mut self) {
        self.finish();
    }
}
fn write(home: &Path, note: &Note) -> io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = store::note_path(home, &note.slug)?;
    let tmp = dir.join(format!(
        ".nota-{}-{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(store::serialize(note).as_bytes())?;
        file.sync_data()?;
        fs::rename(&tmp, dir.join("nota.md"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
