//! One sleeping worker; bounded, replaceable jobs and replies. No JS/C UI calls.
use super::{fold, item, s, score, section};
use crate::files::policy;
use serde_json::{json, Value};
use std::os::{
    fd::{AsRawFd, FromRawFd},
    unix::{ffi::OsStrExt, fs::MetadataExt},
};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
pub const INDEX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_JOBS: usize = 16;
#[derive(Clone)]
pub struct Config {
    pub home: PathBuf,
    pub assets: PathBuf,
    pub cache: PathBuf,
}
pub type Usage = Arc<Mutex<BTreeMap<String, (u64, f64)>>>;
pub fn trim_usage(usage: &mut BTreeMap<String, (u64, f64)>) {
    while usage.len() > 300 || usage.keys().map(String::len).sum::<usize>() > 64 * 1024 {
        let oldest = usage
            .iter()
            .min_by(|a, b| a.1 .1.total_cmp(&b.1 .1))
            .map(|(k, _)| k.clone())
            .unwrap();
        usage.remove(&oldest);
    }
}
fn keep_best(hits: &mut Vec<Value>, item: Value, limit: usize) {
    let at = hits
        .iter()
        .position(|old| num(old, "score") < num(&item, "score"))
        .unwrap_or(hits.len());
    if at < limit {
        hits.insert(at, item);
        hits.truncate(limit);
    }
}
pub enum Work {
    Query {
        q: String,
        lang: String,
        notes: bool,
        help: bool,
    },
    Preview(Value),
    DiscardPreview,
    #[cfg(test)]
    Gate(
        std::sync::mpsc::SyncSender<()>,
        std::sync::mpsc::Receiver<()>,
    ),
    Save,
    Release,
}
pub struct Job {
    pub id: u64,
    pub generation: u64,
    pub token: Arc<AtomicU64>,
    pub work: Work,
}
impl Job {
    fn cancelled(&self) -> bool {
        self.token.load(Ordering::Relaxed) != self.generation
    }
}
pub struct Reply {
    pub id: u64,
    pub generation: u64,
    pub preview: bool,
    pub value: Value,
}
#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    replies: VecDeque<Reply>,
    stop: bool,
}
pub struct Worker {
    queue: Arc<(Mutex<Queue>, Condvar)>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        let (m, c) = &*self.queue;
        let mut q = m.lock().unwrap();
        q.stop = true;
        q.jobs.clear();
        c.notify_one();
    }
}
impl Worker {
    pub fn new(config: Config, usage: Usage) -> Option<Self> {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let shared = queue.clone();
        std::thread::Builder::new()
            .name("axia-search".into())
            .stack_size(512 * 1024)
            .spawn(move || {
                let mut state = Io::new(config, usage);
                loop {
                    let job = {
                        let (m, c) = &*shared;
                        let mut q = m.lock().unwrap();
                        while q.jobs.is_empty() && !q.stop {
                            q = c.wait(q).unwrap();
                        }
                        if q.stop {
                            break;
                        }
                        q.jobs.pop_front().unwrap()
                    };
                    state.prune_previews();
                    if job.cancelled()
                        && !matches!(job.work, Work::Release | Work::Save | Work::DiscardPreview)
                    {
                        continue;
                    }
                    let preview = matches!(job.work, Work::Preview(_));
                    let value = match &job.work {
                        Work::Query {
                            q,
                            lang,
                            notes,
                            help,
                        } => {
                            state.load_usage();
                            state.query(&job, q, lang, *notes, *help)
                        }
                        Work::Preview(file) => state.preview(&job, file),
                        Work::Save => {
                            state.save_usage();
                            continue;
                        }
                        Work::Release => {
                            state.release(job.id);
                            continue;
                        }
                        Work::DiscardPreview => {
                            state.release(job.id);
                            continue;
                        }
                        #[cfg(test)]
                        Work::Gate(started, release) => {
                            let _ = started.send(());
                            let _ = release.recv();
                            continue;
                        }
                    };
                    if !job.cancelled() {
                        let mut q = shared.0.lock().unwrap();
                        // Close clears replies under this same lock. Recheck after
                        // acquiring it so a just-closed client cannot be reinserted.
                        if job.cancelled() || q.stop {
                            continue;
                        }
                        q.replies.retain(|r| r.id != job.id || r.preview != preview);
                        if q.replies.len() >= MAX_JOBS {
                            q.replies.pop_front();
                        }
                        q.replies.push_back(Reply {
                            id: job.id,
                            generation: job.generation,
                            preview,
                            value,
                        });
                    }
                }
            })
            .ok()?;
        Some(Self { queue })
    }
    pub fn submit(&self, job: Job) {
        let (m, c) = &*self.queue;
        let mut q = m.lock().unwrap();
        let preview = matches!(job.work, Work::Preview(_) | Work::DiscardPreview);
        q.jobs.retain(|j| {
            j.id != job.id
                || (matches!(j.work, Work::Preview(_) | Work::DiscardPreview) != preview
                    && !matches!(job.work, Work::Release))
        });
        if q.jobs.len() >= MAX_JOBS {
            q.jobs.pop_front();
        }
        q.jobs.push_back(job);
        c.notify_one();
    }
    pub fn drain(&self) -> Vec<Reply> {
        self.queue.0.lock().unwrap().replies.drain(..).collect()
    }
    pub fn pending(&self) -> usize {
        self.queue.0.lock().unwrap().jobs.len()
    }
    pub fn forget(&self, id: u64) {
        let mut q = self.queue.0.lock().unwrap();
        q.jobs.retain(|j| j.id != id);
        q.replies.retain(|j| j.id != id);
    }
}
pub fn read(path: &Path, max: usize) -> Option<String> {
    let (parent, name) = policy::anchored(path).ok()?;
    let mut f = policy::open_at(
        &parent,
        std::ffi::OsStr::from_bytes(name.as_bytes()),
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0,
    )
    .ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = Vec::new();
    Read::by_ref(&mut f)
        .take(max as u64)
        .read_to_end(&mut text)
        .ok()?;
    Some(String::from_utf8_lossy(&text).into_owned())
}
fn secure_dir(path: &Path) -> std::io::Result<File> {
    let mut dir = File::open("/")?;
    for c in path.components() {
        match c {
            std::path::Component::RootDir => (),
            std::path::Component::Normal(name) => {
                let name_c = policy::c_name(name)?;
                unsafe {
                    libc::mkdirat(dir.as_raw_fd(), name_c.as_ptr(), 0o700);
                }
                dir = policy::open_at(
                    &dir,
                    name,
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                    0,
                )?;
            }
            _ => return Err(std::io::Error::from_raw_os_error(libc::EINVAL)),
        }
    }
    Ok(dir)
}
fn create(dir: &File, name: &str) -> std::io::Result<File> {
    policy::open_at(
        dir,
        std::ffi::OsStr::new(name),
        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW,
        0o600,
    )
}
fn atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap();
    let dir = secure_dir(parent)?;
    let name = path.file_name().unwrap().to_string_lossy();
    let temp = format!(".{name}.{}.tmp", std::process::id());
    let mut f = create(&dir, &temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    let temp = policy::c_name(std::ffi::OsStr::new(&temp))?;
    let name = policy::c_name(path.file_name().unwrap())?;
    if unsafe {
        libc::renameat(
            dir.as_raw_fd(),
            temp.as_ptr(),
            dir.as_raw_fd(),
            name.as_ptr(),
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
struct Watch {
    fd: Option<File>,
    count: usize,
}
impl Watch {
    fn new() -> Self {
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        Self {
            fd: (fd >= 0).then(|| unsafe { File::from_raw_fd(fd) }),
            count: 0,
        }
    }
    fn add(&mut self, path: &Path) {
        if self.count >= 512 {
            return;
        }
        if let (Some(fd), Ok(path)) = (&self.fd, policy::c_name(path.as_os_str())) {
            let n = unsafe {
                libc::inotify_add_watch(
                    fd.as_raw_fd(),
                    path.as_ptr(),
                    libc::IN_CREATE
                        | libc::IN_DELETE
                        | libc::IN_MOVED_FROM
                        | libc::IN_MOVED_TO
                        | libc::IN_DELETE_SELF
                        | libc::IN_MOVE_SELF,
                )
            };
            if n >= 0 {
                self.count += 1;
            }
        }
    }
    fn dirty(&mut self) -> bool {
        let mut bytes = [0u8; 8192];
        let mut dirty = false;
        if let Some(f) = &mut self.fd {
            while f.read(&mut bytes).is_ok_and(|n| n > 0) {
                dirty = true;
            }
        }
        dirty
    }
}
struct Io {
    c: Config,
    at: Option<Instant>,
    watch: Watch,
    usage: Usage,
    loaded: bool,
    previews: BTreeMap<u64, (PathBuf, Arc<AtomicU64>, u64)>,
}
impl Drop for Io {
    fn drop(&mut self) {
        for (p, _, _) in self.previews.values() {
            let _ = fs::remove_file(p);
        }
    }
}
impl Io {
    fn new(c: Config, usage: Usage) -> Self {
        Self {
            c,
            at: None,
            watch: Watch::new(),
            usage,
            loaded: false,
            previews: BTreeMap::new(),
        }
    }
    fn load_usage(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let path = self.c.home.join(".local/share/axia/spotlight.json");
        if let Some(v) =
            read(&path, 2 * 1024 * 1024).and_then(|v| serde_json::from_str::<Value>(&v).ok())
        {
            if let Some(v) = v.as_object() {
                let mut u = self.usage.lock().unwrap();
                for (k, v) in v.iter().take(300) {
                    if k.len() <= 4096 {
                        let old = (v["n"].as_u64().unwrap_or(0), v["at"].as_f64().unwrap_or(0.));
                        if let Some(new) = u.get_mut(k) {
                            new.0 = new.0.saturating_add(old.0);
                        } else {
                            u.insert(k.clone(), old);
                        }
                    }
                }
                trim_usage(&mut u);
            }
        }
    }
    fn save_usage(&mut self) {
        self.load_usage();
        let value = {
            let u = self.usage.lock().unwrap();
            u.iter()
                .map(|(k, (n, at))| (k.clone(), json!({"n":n,"at":at})))
                .collect::<serde_json::Map<_, _>>()
        };
        let _ = atomic(
            &self.c.home.join(".local/share/axia/spotlight.json"),
            Value::Object(value).to_string().as_bytes(),
        );
    }
    fn release(&mut self, id: u64) {
        if let Some((p, _, _)) = self.previews.remove(&id) {
            let _ = fs::remove_file(p);
        }
    }
    fn prune_previews(&mut self) {
        self.previews.retain(|_, (path, token, generation)| {
            if token.load(Ordering::Relaxed) == *generation {
                true
            } else {
                let _ = fs::remove_file(path);
                false
            }
        });
    }
    fn index(&mut self, job: &Job) -> Option<PathBuf> {
        let out = self.c.cache.join("files.jsonl");
        let dirty = self.watch.dirty();
        let ttl = if self.watch.count >= 512 { 10 } else { 120 };
        if !dirty
            && self
                .at
                .is_some_and(|at| at.elapsed() < Duration::from_secs(ttl))
            && out.is_file()
        {
            return Some(out);
        }
        // A cancelled rebuild must not make the old index look fresh after its
        // invalidation event has already been consumed.
        self.at = None;
        let dir = secure_dir(&self.c.cache).ok()?;
        let tmp = self.c.cache.join(".files.tmp");
        let mut writer = BufWriter::new(create(&dir, ".files.tmp").ok()?);
        let mut queue = VecDeque::from([(self.c.home.clone(), 0)]);
        let mut bytes = 0;
        let mut count = 0;
        let start = Instant::now();
        self.watch = Watch::new();
        while let Some((dir, depth)) = queue.pop_front() {
            if job.cancelled() {
                let _ = fs::remove_file(&tmp);
                return None;
            }
            if count >= 100_000 || bytes >= INDEX_BYTES || start.elapsed() > Duration::from_secs(3)
            {
                break;
            }
            self.watch.add(&dir);
            if policy::open_dir(&dir).is_err() {
                continue;
            }
            if let Ok(list) = fs::read_dir(&dir) {
                for e in list.flatten() {
                    if start.elapsed() > Duration::from_secs(3)
                        || bytes >= INDEX_BYTES
                        || count >= 100_000
                    {
                        break;
                    }
                    if job.cancelled() {
                        let _ = fs::remove_file(&tmp);
                        return None;
                    }
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    let Ok(kind) = e.file_type() else {
                        continue;
                    };
                    if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
                        continue;
                    }
                    let Ok(m) = e.metadata() else {
                        continue;
                    };
                    let path = e.path();
                    let Some(path_text) = path.to_str() else {
                        continue;
                    };
                    let hit = json!({"path":path_text,"name":name,"dir":kind.is_dir(),"size":m.len(),"mtime":m.mtime() as f64*1000.});
                    let line = hit.to_string();
                    if bytes + line.len() as u64 + 1 > INDEX_BYTES {
                        break;
                    }
                    writer.write_all(line.as_bytes()).ok()?;
                    writer.write_all(b"\n").ok()?;
                    bytes += line.len() as u64 + 1;
                    count += 1;
                    if kind.is_dir() && depth + 1 < 6 && queue.len() < 4096 {
                        queue.push_back((path, depth + 1));
                    }
                    if count >= 100_000 {
                        break;
                    }
                }
            }
        }
        writer.flush().ok()?;
        drop(writer);
        if job.cancelled() {
            let _ = fs::remove_file(tmp);
            return None;
        }
        fs::rename(tmp, &out).ok()?;
        self.at = Some(Instant::now());
        Some(out)
    }
    fn query(&mut self, job: &Job, q: &str, lang: &str, notes: bool, help: bool) -> Value {
        let mut sections = vec![];
        if q.chars().count() >= 2 {
            if let Some(path) = self.index(job) {
                if let Some(file) = policy::anchored(&path)
                    .ok()
                    .and_then(|(parent, name)| {
                        policy::open_at(
                            &parent,
                            std::ffi::OsStr::from_bytes(name.as_bytes()),
                            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                            0,
                        )
                        .ok()
                    })
                    .filter(|file| {
                        file.metadata()
                            .is_ok_and(|m| m.is_file() && m.len() <= INDEX_BYTES)
                    })
                {
                    let mut hits = Vec::new();
                    let mut candidates = 0;
                    for line in BufReader::new(file).lines() {
                        if job.cancelled() {
                            return json!([]);
                        }
                        let Ok(line) = line else {
                            break;
                        };
                        if line.len() > 16384 {
                            continue;
                        }
                        let Ok(f) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        if !s(&f, "name").to_lowercase().contains(q) {
                            continue;
                        }
                        candidates += 1;
                        let name = s(&f, "name");
                        let stem = name.rsplit_once('.').map(|x| x.0).unwrap_or(name);
                        let rank =
                            score(&fold(stem), q, true).max(score(&fold(name), q, false) * 0.3);
                        if rank > 15. {
                            let key = format!("file:{}", s(&f, "path"));
                            let rank = rank + super::boost(&self.usage, &key)
                                - (if f["dir"] == true { 0. } else { 1. });
                            let mut it = item(
                                &key,
                                rank * 0.8,
                                json!(name),
                                json!(s(&f, "path").replacen(
                                    &self.c.home.to_string_lossy().to_string(),
                                    "~",
                                    1
                                )),
                                if f["dir"] == true {
                                    "📁"
                                } else if image_path(name) {
                                    "🖼"
                                } else {
                                    "📄"
                                },
                                json!({"type":"file","file":f,"text":text_path(name)}),
                            );
                            it["preview"] = json!({"kind":"file","file":f});
                            keep_best(&mut hits, it, 6);
                        }
                        if candidates >= 60 {
                            break;
                        }
                    }
                    hits.sort_by(|a, b| num(b, "score").total_cmp(&num(a, "score")));
                    hits.truncate(6);
                    if !hits.is_empty() {
                        sections.push(section("files", hits, None));
                    }
                }
            }
        }
        if notes && !job.cancelled() {
            let mut hits = Vec::new();
            let base = if self.c.home.join("Notes").is_dir() {
                "Notes"
            } else {
                "Notas"
            };
            if let Ok(dirs) = fs::read_dir(self.c.home.join(base)) {
                for d in dirs.flatten().take(400) {
                    if job.cancelled() {
                        return json!([]);
                    }
                    if !d.file_type().is_ok_and(|t| t.is_dir()) {
                        continue;
                    }
                    let Some(raw) = read(&d.path().join("nota.md"), 65536) else {
                        continue;
                    };
                    let (meta, body) = front(&raw);
                    let name = meta.get("title").cloned().unwrap_or_else(|| {
                        body.lines()
                            .find(|s| !s.trim().is_empty())
                            .unwrap_or("")
                            .trim_start_matches("# ")
                            .to_owned()
                    });
                    let rank = score(&fold(&name), q, true).max(if fold(body).contains(q) {
                        30.
                    } else {
                        0.
                    });
                    if rank == 0. {
                        continue;
                    }
                    let id = d.file_name().to_string_lossy().into_owned();
                    let mut it = item(
                        &format!("note:{id}"),
                        rank,
                        json!(name),
                        json!({"date":d.metadata().map(|m|m.mtime() as f64*1000.).unwrap_or(0.)}),
                        "🗒",
                        json!({"type":"note","id":id}),
                    );
                    it["preview"] = json!({"kind":"note","title":name,"text":body.chars().take(1500).collect::<String>()});
                    keep_best(&mut hits, it, 5);
                }
            }
            hits.sort_by(|a, b| num(b, "score").total_cmp(&num(a, "score")));
            hits.truncate(5);
            if !hits.is_empty() {
                sections.push(section("notes", hits, None));
            }
        }
        if help && q.chars().count() >= 2 && !job.cancelled() {
            let mut docs = BTreeMap::new();
            for l in ["en", lang] {
                if l.len() > 12
                    || !l
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                {
                    continue;
                }
                if let Ok(list) = fs::read_dir(self.c.assets.join("help").join(l)) {
                    for e in list.flatten().take(500) {
                        if e.path().extension().is_some_and(|s| s == "md") {
                            docs.insert(e.file_name(), e.path());
                        }
                    }
                }
            }
            let mut hits = Vec::new();
            for (id, path) in docs {
                if job.cancelled() {
                    return json!([]);
                }
                let Some(raw) = read(&path, 256 * 1024) else {
                    continue;
                };
                let (meta, body) = front(&raw);
                let id = id.to_string_lossy().trim_end_matches(".md").to_owned();
                let title = meta.get("title").cloned().unwrap_or_else(|| id.clone());
                let title_fold = fold(&title);
                let kw = fold(meta.get("keywords").map(String::as_str).unwrap_or(""));
                let rank = if title_fold.starts_with(q) {
                    90.
                } else if title_fold.contains(q) {
                    70.
                } else if kw.contains(q) {
                    55.
                } else if fold(body).contains(q) {
                    30.
                } else {
                    0.
                };
                if rank == 0. {
                    continue;
                }
                let raw = body
                    .chars()
                    .filter(|c| !"#*`>[]()_".contains(*c))
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let normalized = fold(&raw);
                let pos = normalized.find(q).unwrap_or(0);
                let start = normalized[..pos].chars().count().saturating_sub(60);
                let snippet = raw.chars().skip(start).take(200).collect::<String>();
                let mut it = item(
                    &format!("help:{id}"),
                    rank * 0.6,
                    json!(title),
                    json!({"key":"shell.spotlight.inHelp"}),
                    "?",
                    json!({"type":"help","id":id}),
                );
                it["preview"] = json!({"kind":"text","text":snippet});
                keep_best(&mut hits, it, 4);
            }
            hits.sort_by(|a, b| num(b, "score").total_cmp(&num(a, "score")));
            hits.truncate(4);
            if !hits.is_empty() {
                sections.push(section("help", hits, None));
            }
        }
        json!(sections)
    }
    fn preview(&mut self, job: &Job, f: &Value) -> Value {
        let path = PathBuf::from(s(f, "path"));
        if !path.starts_with(&self.c.home)
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return json!({"kind":"info"});
        }
        let Ok((parent, name)) = policy::anchored(&path) else {
            return json!({"kind":"info"});
        };
        let Ok(file) = policy::open_at(
            &parent,
            std::ffi::OsStr::from_bytes(name.as_bytes()),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW,
            0,
        ) else {
            return json!({"kind":"info"});
        };
        let Ok(m) = file.metadata() else {
            return json!({"kind":"info"});
        };
        if m.is_dir() {
            let mut names = vec![];
            if let Ok(list) = fs::read_dir(&path) {
                for e in list
                    .flatten()
                    .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    .take(10000)
                {
                    if job.cancelled() {
                        return json!({"kind":"info"});
                    }
                    names.push(json!({"name":e.file_name().to_string_lossy(),"dir":e.file_type().is_ok_and(|t|t.is_dir())}));
                    names.sort_by(|a, b| {
                        b["dir"]
                            .as_bool()
                            .cmp(&a["dir"].as_bool())
                            .then_with(|| s(a, "name").cmp(s(b, "name")))
                    });
                    names.truncate(12);
                }
            }
            return json!({"kind":"dir","items":names});
        }
        if !m.is_file() {
            return json!({"kind":"info"});
        }
        if image_path(s(f, "name")) && m.len() <= 32 * 1024 * 1024 && !job.cancelled() {
            let result = (|| -> Option<PathBuf> {
                use image::ImageDecoder;
                let mut r = image::ImageReader::new(BufReader::new(file))
                    .with_guessed_format()
                    .ok()?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(10000);
                limits.max_image_height = Some(10000);
                limits.max_alloc = Some(64 * 1024 * 1024);
                r.limits(limits);
                let mut d = r.into_decoder().ok()?;
                if d.total_bytes() > 64 * 1024 * 1024 {
                    return None;
                }
                let orientation = d
                    .orientation()
                    .unwrap_or(image::metadata::Orientation::NoTransforms);
                let mut image = image::DynamicImage::from_decoder(d).ok()?;
                if job.cancelled() {
                    return None;
                }
                image.apply_orientation(orientation);
                let image = image.thumbnail(232, 160);
                let out = self
                    .c
                    .cache
                    .join(format!("preview-{}-{}.png", job.id, job.generation));
                let mut bytes = std::io::Cursor::new(Vec::new());
                image.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
                atomic(&out, bytes.get_ref()).ok()?;
                Some(out)
            })();
            if let Some(out) = result {
                self.release(job.id);
                self.previews
                    .insert(job.id, (out.clone(), job.token.clone(), job.generation));
                self.prune_previews();
                return json!({"kind":"image","path":out.to_string_lossy()});
            }
        } else if text_path(s(f, "name")) && m.len() < 8 * 1024 * 1024 {
            let mut text = Vec::new();
            let _ = file.take(9600).read_to_end(&mut text);
            return json!({"kind":"text","text":String::from_utf8_lossy(&text).chars().take(2400).collect::<String>()});
        }
        json!({"kind":"info"})
    }
}
pub fn front(raw: &str) -> (BTreeMap<String, String>, &str) {
    let mut out = BTreeMap::new();
    if let Some(raw) = raw.strip_prefix("---\n") {
        if let Some((head, body)) = raw.split_once("\n---") {
            for l in head.lines() {
                if let Some((k, v)) = l.split_once(':') {
                    out.insert(k.to_owned(), v.trim().to_owned());
                }
            }
            return (out, body.trim_start_matches('\n'));
        }
    }
    (out, raw)
}
fn num(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or(0.)
}
pub fn image_path(s: &str) -> bool {
    s.rsplit('.').next().is_some_and(|e| {
        ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&e.to_ascii_lowercase().as_str())
    })
}
pub fn text_path(s: &str) -> bool {
    s.rsplit('.').next().is_some_and(|e| {
        [
            "txt", "md", "markdown", "json", "js", "jsx", "ts", "tsx", "mjs", "c", "h", "cpp",
            "hpp", "py", "go", "rs", "sh", "bash", "zsh", "css", "html", "htm", "xml", "yaml",
            "yml", "toml", "ini", "conf", "cfg", "log", "csv", "sql", "php", "rb", "lua", "diff",
            "patch",
        ]
        .contains(&e.to_ascii_lowercase().as_str())
    })
}
