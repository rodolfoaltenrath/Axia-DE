//! Two sleeping workers per window: one for visible library work and one for
//! viewer/edits. Mailboxes are bounded and latest-wins, not an unbounded channel.
use super::{
    decode,
    io::{self, Entry},
    model::View,
    render::{self, Raster},
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
};
pub enum Task {
    View {
        entry: Entry,
        home: PathBuf,
        view: View,
        elapsed: u64,
    },
    Save {
        entry: Entry,
        home: PathBuf,
        view: View,
        crop: Option<[u32; 4]>,
        suffix: String,
    },
}
pub struct Drawn {
    pub render_ms: f64,
    pub quality: bool,
    pub raster: Raster,
    pub width: u32,
    pub height: u32,
    pub frames: usize,
    pub delay: u64,
}
pub enum ResultValue {
    View(Result<Drawn, String>),
    Save(Result<PathBuf, String>),
}
struct State {
    task: Option<(u64, Task)>,
    saved: VecDeque<(u64, ResultValue)>,
    drawn: Option<(u64, ResultValue)>,
    stop: bool,
}
pub struct Worker {
    state: Arc<(Mutex<State>, Condvar)>,
    pub serial: Arc<AtomicU64>,
}
impl Default for Worker {
    fn default() -> Self {
        let state = Arc::new((
            Mutex::new(State {
                task: None,
                saved: VecDeque::new(),
                drawn: None,
                stop: false,
            }),
            Condvar::new(),
        ));
        let serial = Arc::new(AtomicU64::new(0));
        let shared = state.clone();
        let current = serial.clone();
        thread::Builder::new()
            .name("axia-photos-viewer".into())
            .spawn(move || {
                let mut cache: VecDeque<(Entry, Arc<decode::Photo>)> = VecDeque::new();
                loop {
                    let (id, task) = {
                        let (lock, wake) = &*shared;
                        let mut s = lock.lock().unwrap_or_else(|e| e.into_inner());
                        while !s.stop && s.task.is_none() {
                            s = wake.wait(s).unwrap_or_else(|e| e.into_inner())
                        }
                        if s.stop {
                            return;
                        }
                        s.task.take().unwrap()
                    };
                    let (entry, home) = match &task {
                        Task::View { entry, home, .. } | Task::Save { entry, home, .. } => {
                            (entry, home)
                        }
                    };
                    let resolved;
                    if entry.stamp == (0, 0, 0, 0) {
                        resolved = Entry::inspect(&entry.path);
                    } else {
                        resolved = Ok(entry.clone());
                    }
                    let entry = resolved.as_ref().unwrap_or(entry);
                    let photo = if let Some(i) = cache.iter().position(|(e, _)| {
                        e.path == entry.path && e.stamp == entry.stamp && e.size == entry.size
                    }) {
                        let item = cache.remove(i).unwrap();
                        let photo = item.1.clone();
                        cache.push_front(item);
                        Ok(photo)
                    } else {
                        decode::load(entry, home, id, &current).map(|p| {
                            cache.retain(|(e, _)| e.path != entry.path);
                            while cache.len() >= 8
                                || cache.iter().map(|(_, p)| p.bytes).sum::<usize>() + p.bytes
                                    > decode::CACHE_BUDGET
                            {
                                if cache.pop_back().is_none() {
                                    break;
                                }
                            }
                            cache.push_front((entry.clone(), p.clone()));
                            p
                        })
                    };
                    let result = match task {
                        Task::View { view, elapsed, .. } => {
                            ResultValue::View(photo.and_then(|p| {
                                let frame = p.frame(elapsed);
                                let started = std::time::Instant::now();
                                let raster = render::render(&p, view, frame, id, &current)
                                    .ok_or("cancelled")?;
                                Ok(Drawn {
                                    render_ms: started.elapsed().as_secs_f64() * 1000.,
                                    quality: view.quality,
                                    raster,
                                    width: p.width,
                                    height: p.height,
                                    frames: p.frames.len(),
                                    delay: p.frames[frame].delay,
                                })
                            }))
                        }
                        Task::Save {
                            entry,
                            home,
                            view,
                            crop,
                            suffix,
                        } => ResultValue::Save(photo.and_then(|p| {
                            decode::edit(&p, view, crop)
                                .and_then(|rgba| io::save(&entry, &home, &rgba, &suffix))
                        })),
                    };
                    let mut s = shared.0.lock().unwrap_or_else(|e| e.into_inner());
                    if matches!(result, ResultValue::Save(_)) {
                        if s.saved.len() < 8 {
                            s.saved.push_back((id, result))
                        }
                    } else if current.load(Ordering::Relaxed) == id {
                        s.drawn = Some((id, result));
                    }
                }
            })
            .expect("Photos worker");
        Self { state, serial }
    }
}
impl Worker {
    pub fn submit(&self, task: Task) -> Option<u64> {
        let mut s = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(s.task, Some((_, Task::Save { .. }))) {
            return None;
        }
        let id = self.serial.fetch_add(1, Ordering::Relaxed) + 1;
        s.task = Some((id, task));
        s.drawn = None;
        self.state.1.notify_one();
        Some(id)
    }
    pub fn take(&self) -> Vec<(u64, ResultValue)> {
        let mut s = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = s.saved.drain(..).collect::<Vec<_>>();
        if let Some(v) = s.drawn.take() {
            out.push(v)
        }
        out
    }
    pub fn cancel(&self) {
        self.serial.fetch_add(1, Ordering::Relaxed);
        let mut s = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(s.task, Some((_, Task::Save { .. }))) {
            s.task = None
        }
        s.drawn = None;
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.serial.fetch_add(1, Ordering::Relaxed);
        let mut s = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        s.stop = true;
        s.task = None;
        s.drawn = None;
        self.state.1.notify_one();
    }
}
pub enum LibraryResult {
    Scan(u64, Result<(Vec<Entry>, Vec<PathBuf>), String>),
    Thumb(u64, PathBuf, Result<Raster, String>),
}
enum LibraryTask {
    Scan(u64, PathBuf, bool, PathBuf),
    Thumb(u64, Entry, PathBuf),
}
struct LibraryState {
    queue: VecDeque<LibraryTask>,
    results: VecDeque<LibraryResult>,
    stop: bool,
}
pub struct Library {
    state: Arc<(Mutex<LibraryState>, Condvar)>,
    generation: Arc<AtomicU64>,
}
impl Default for Library {
    fn default() -> Self {
        let state = Arc::new((
            Mutex::new(LibraryState {
                queue: VecDeque::new(),
                results: VecDeque::new(),
                stop: false,
            }),
            Condvar::new(),
        ));
        let generation = Arc::new(AtomicU64::new(0));
        let shared = state.clone();
        let current = generation.clone();
        thread::Builder::new()
            .name("axia-photos-library".into())
            .spawn(move || {
                unsafe {
                    libc::setpriority(libc::PRIO_PROCESS, 0, 10);
                }
                loop {
                    let task = {
                        let (mut s, wake) = (
                            shared.0.lock().unwrap_or_else(|e| e.into_inner()),
                            &shared.1,
                        );
                        while !s.stop && s.queue.is_empty() {
                            s = wake.wait(s).unwrap_or_else(|e| e.into_inner())
                        }
                        if s.stop {
                            return;
                        }
                        s.queue.pop_front().unwrap()
                    };
                    let result = match task {
                        LibraryTask::Scan(g, path, recursive, home) => {
                            LibraryResult::Scan(g, io::scan(&path, recursive, &home, g, &current))
                        }
                        LibraryTask::Thumb(g, e, home) => {
                            let r = decode::thumbnail(&e, &home, g, &current);
                            LibraryResult::Thumb(g, e.path, r)
                        }
                    };
                    let g = match &result {
                        LibraryResult::Scan(g, _) | LibraryResult::Thumb(g, ..) => *g,
                    };
                    let mut s = shared.0.lock().unwrap_or_else(|e| e.into_inner());
                    if current.load(Ordering::Relaxed) == g {
                        if s.results.len() >= 32 {
                            s.results.pop_front();
                        }
                        s.results.push_back(result);
                    }
                }
            })
            .expect("Photos library worker");
        Self { state, generation }
    }
}
impl Library {
    pub fn scan(&self, path: PathBuf, recursive: bool, home: PathBuf) -> u64 {
        let g = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let mut s = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        s.queue.clear();
        s.results.clear();
        s.queue
            .push_back(LibraryTask::Scan(g, path, recursive, home));
        self.state.1.notify_one();
        g
    }
    pub fn thumbs(&self, g: u64, visible: Vec<Entry>, home: PathBuf) {
        let mut s = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        s.queue.retain(|t| matches!(t, LibraryTask::Scan(..)));
        for e in visible.into_iter().take(24) {
            s.queue.push_back(LibraryTask::Thumb(g, e, home.clone()));
        }
        self.state.1.notify_one();
    }
    pub fn take(&self) -> Vec<LibraryResult> {
        self.state
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .results
            .drain(..)
            .collect()
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        let mut s = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        s.stop = true;
        s.queue.clear();
        s.results.clear();
        self.state.1.notify_one();
    }
}
