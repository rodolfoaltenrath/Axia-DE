use super::{
    io_error,
    operations::{self as ops, Entry},
    policy::{self, Policy},
    trash, Result,
};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::os::fd::{AsRawFd, FromRawFd};
use std::{cell::RefCell, rc::Rc};

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};

type Work = Box<dyn FnOnce() + Send>;
fn work(job: impl FnOnce() + Send + 'static) -> Result<()> {
    static POOL: OnceLock<Result<mpsc::SyncSender<Work>>> = OnceLock::new();
    let pool = POOL.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Work>(32);
        let rx = Arc::new(Mutex::new(rx));
        let mut count = 0;
        for n in 0..4 {
            let rx = rx.clone();
            if std::thread::Builder::new()
                .name(format!("axia-files-io-{n}"))
                .spawn(move || loop {
                    let job = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    match job {
                        Ok(job) => job(),
                        Err(_) => break,
                    }
                })
                .is_ok()
            {
                count += 1;
            }
        }
        if count == 0 {
            Err("fs.copyStartFailed".into())
        } else {
            Ok(tx)
        }
    });
    pool.as_ref()
        .map_err(Clone::clone)?
        .try_send(Box::new(job))
        .map_err(|_| "fs.tooManyCopies".into())
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Location {
    Directory(PathBuf),
    Trash,
    Recent,
    Apps,
    Network,
    Tag(String),
    Archive { file: PathBuf, inner: String },
}
impl Location {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Directory(p) => Some(p),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browser,
    Open,
    Save,
    Folder,
}
#[derive(Clone)]
pub struct Place {
    pub label: String,
    pub path: PathBuf,
}
#[derive(Clone)]
struct Filtered {
    version: u64,
    len: usize,
    query: String,
    hidden: bool,
    folder: bool,
    items: Rc<Vec<usize>>,
}
#[derive(Clone)]
pub struct Tab {
    pub location: Location,
    pub back: Vec<Location>,
    pub forward: Vec<Location>,
    pub entries: Vec<Entry>,
    entries_version: u64,
    filtered: RefCell<Option<Filtered>>,
    pub selected: BTreeSet<usize>,
    pub view: String,
    pub sort: String,
    pub descending: bool,
    pub query: String,
    pub offset: f64,
    pub anchor: usize,
    pub cursor: Option<usize>,
    pub columns: Vec<(PathBuf, Vec<Entry>)>,
    pub column_offsets: HashMap<PathBuf, f64>,
    pub archive: Vec<Value>,
}
impl Tab {
    fn replace_entries(&mut self, entries: Vec<Entry>) {
        self.entries_version = self.entries_version.wrapping_add(1);
        self.entries = entries;
    }
    fn new(location: Location) -> Self {
        Self {
            location,
            back: Vec::new(),
            forward: Vec::new(),
            entries: Vec::new(),
            entries_version: 0,
            filtered: RefCell::new(None),
            selected: BTreeSet::new(),
            view: "icons".into(),
            sort: "name".into(),
            descending: false,
            query: String::new(),
            offset: 0.,
            anchor: 0,
            cursor: None,
            columns: Vec::new(),
            column_offsets: HashMap::new(),
            archive: Vec::new(),
        }
    }
}
#[derive(Clone)]
pub enum Dialog {
    Text {
        action: String,
        title: String,
        value: String,
        paths: Vec<PathBuf>,
    },
    Confirm {
        action: String,
        paths: Vec<PathBuf>,
    },
    Info {
        entries: Vec<Entry>,
        size: Option<u64>,
        comment: String,
    },
    Preview {
        entry: Entry,
        text: Option<String>,
        image: Option<Value>,
        media: Option<u64>,
    },
    Network {
        value: Value,
    },
    Preferences,
}
enum Notice {
    Scan(u64, Result<Vec<Entry>>),
    Writable(u64, bool),
    Text(u64, Result<String>),
    Size(u64, u64),
    NetworkPrompt(u64, super::network::Prompt),
    Connected(u64, Result<PathBuf>),
    Action(Result<()>),
    Search(u64, Result<Vec<Entry>>),
}
#[derive(Default)]
pub struct Shared {
    pub clipboard: Vec<PathBuf>,
    pub cut: bool,
    pub preferences: Value,
    pub tags: HashMap<String, Vec<String>>,
    pub comments: HashMap<String, String>,
    pub apps: Vec<Value>,
    pub loaded: bool,
    pub volumes: Vec<super::volumes::Volume>,
    pub extra_mounts: Vec<(PathBuf, String)>,
    pub dragging: Option<(u64, Vec<PathBuf>, bool)>,
}
pub struct Session {
    pub id: u64,
    pub mode: Mode,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub places: Vec<Place>,
    pub title: String,
    pub name: String,
    pub multiple: bool,
    pub hidden: bool,
    pub error: Option<String>,

    pub loading: bool,
    pub dialog: Option<Dialog>,
    pub context: Option<(f64, f64)>,
    pub width: f64,
    pub height: f64,
    pub theme: Value,
    source_theme: Value,
    pub locale: String,
    pub effects: Vec<Value>,
    pub dirty: bool,
    pub closed: bool,
    pub filter: Option<regex::Regex>,
    pub policy: Policy,
    pub generation: Arc<AtomicU64>,
    tx: mpsc::Sender<Notice>,
    rx: mpsc::Receiver<Notice>,
    watch: Option<File>,
    last_load: Instant,
    last_scan: Duration,
    silent_load: bool,
    // Asked on the listing's worker: the question can block on a network mount.
    pub(super) can_write: bool,

    last_jobs: Value,
    owned_jobs: HashSet<i32>,
    /// renamed or created here: selected once the folder's next listing has it
    reveal: Option<PathBuf>,
    thumbnail_revision: u64,
    pub pdf_thumbnails: super::pdf_thumbnails::Cache,
    pub volume_busy: Option<String>,
    volume_paths: Vec<PathBuf>,
    pub side_width: f64,
    pub zoom: f64,
    zoom_save: Option<Instant>,
    zoom_wheel: f64,
    pub drag: Option<Value>,
    pub job_errors: Vec<String>,
    pub dialog_epoch: u64,
    pub network_epoch: u64,
    pub auth: Option<mpsc::Sender<Value>>,
    pub busy: bool,
    pub discovered: Vec<Entry>,
    pub network_status: Value,
    pub searching: bool,
    pub search_home: bool,
    pub folders_first: bool,
}

pub fn user_dirs(p: &Policy, config: &str) -> Vec<Place> {
    let keys: [(&str, &str, &[&str]); 6] = [
        (
            "DOWNLOAD",
            "downloads",
            &["Downloads", "Descargas", "Transferências"],
        ),
        ("DOCUMENTS", "documents", &["Documentos", "Documents"]),
        (
            "DESKTOP",
            "desktop",
            &[
                "Área de trabalho",
                "Área de Trabalho",
                "Desktop",
                "Escritorio",
            ],
        ),
        ("PICTURES", "pictures", &["Imagens", "Pictures", "Imágenes"]),
        ("MUSIC", "music", &["Músicas", "Música", "Music"]),
        ("VIDEOS", "videos", &["Vídeos", "Videos"]),
    ];
    let re =
        regex::Regex::new(r#"^\s*XDG_([A-Z]+)_DIR\s*=\s*"((?:\\.|[^"\\])*)"\s*(?:#.*)?$"#).unwrap();
    let mut configured = HashMap::new();
    for line in config.lines() {
        if let Some(c) = re.captures(line) {
            let raw = &c[2];
            let expanded = if raw == "$HOME" || raw == "${HOME}" {
                p.home.to_string_lossy().into_owned()
            } else if let Some(s) = raw
                .strip_prefix("$HOME/")
                .or_else(|| raw.strip_prefix("${HOME}/"))
            {
                p.home.join(s).to_string_lossy().into_owned()
            } else {
                raw.to_owned()
            };
            let mut value = String::new();
            let mut chars = expanded.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '\\'
                    && chars
                        .peek()
                        .is_some_and(|c| matches!(c, '"' | '$' | '`' | '\\'))
                {
                    value.push(chars.next().unwrap());
                } else {
                    value.push(ch);
                }
            }
            let path = PathBuf::from(value);
            configured.insert(c[1].to_owned(), path);
        }
    }
    keys.iter()
        .filter_map(|(key, label, names)| {
            let path = if let Some(p) = configured.get(*key) {
                p.clone()
            } else {
                names.iter().map(|n| p.home.join(n)).find(|n| n.is_dir())?
            };
            if path.is_absolute() && path != p.home && path.is_dir() {
                Some(Place {
                    label: (*label).into(),
                    path,
                })
            } else {
                None
            }
        })
        .collect()
}

fn load_json(path: &Path) -> Value {
    (|| -> Option<Value> {
        use std::io::Read;
        use std::os::unix::ffi::OsStrExt;
        let (parent, name) = policy::anchored(path).ok()?;
        let file = policy::open_at(
            &parent,
            std::ffi::OsStr::from_bytes(name.as_bytes()),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW,
            0,
        )
        .ok()?;
        let m = file.metadata().ok()?;
        if !m.is_file() || m.len() > 1024 * 1024 {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 1024 * 1024 {
            return None;
        }
        serde_json::from_slice(&bytes).ok()
    })()
    .unwrap_or(json!({}))
}

fn private_json(path: &Path, data: &Value) -> Result<()> {
    let parent = path.parent().ok_or("sys.badArgument")?;
    fs::create_dir_all(parent).map_err(io_error)?;
    // The dedicated preference service can write its own protected file. It
    // still uses an anchored parent and atomic replacement, never a symlink.
    let (parent_fd, name) = policy::anchored(path).map_err(io_error)?;
    use std::os::fd::AsRawFd;
    let tmp = format!(
        ".files-prefs-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let tmp = std::ffi::OsStr::new(&tmp);
    let mut file = policy::open_at(
        &parent_fd,
        tmp,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
        0o600,
    )
    .map_err(io_error)?;
    use std::io::Write;
    let result = (|| -> std::io::Result<()> {
        file.write_all(serde_json::to_string(data).unwrap().as_bytes())?;
        file.sync_all()?;
        let tn = policy::c_name(tmp)?;
        if unsafe {
            libc::renameat(
                parent_fd.as_raw_fd(),
                tn.as_ptr(),
                parent_fd.as_raw_fd(),
                name.as_ptr(),
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        if let Ok(tn) = policy::c_name(tmp) {
            unsafe {
                libc::unlinkat(parent_fd.as_raw_fd(), tn.as_ptr(), 0);
            }
        }
    }
    result.map_err(io_error)
}
impl Shared {
    fn load(&mut self, p: &Policy, legacy: &Value) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.preferences = load_json(&p.home.join(".config/axia/files.json"));
        if self.preferences.as_object().is_none_or(|m| m.is_empty()) {
            self.preferences = legacy.clone();
        }
        if !self.preferences.is_object() {
            self.preferences = json!({});
        }
        self.tags = serde_json::from_value(load_json(&p.home.join(".local/share/axia/tags.json")))
            .unwrap_or_default();
        self.comments =
            serde_json::from_value(load_json(&p.home.join(".local/share/axia/comments.json")))
                .unwrap_or_default();
    }
    pub fn save(&self, p: &Policy) -> Result<()> {
        private_json(&p.home.join(".config/axia/files.json"), &self.preferences)
    }
    fn save_tags(&self, p: &Policy) -> Result<()> {
        private_json(
            &p.home.join(".local/share/axia/tags.json"),
            &json!(self.tags),
        )
    }
    fn save_comments(&self, p: &Policy) -> Result<()> {
        private_json(
            &p.home.join(".local/share/axia/comments.json"),
            &json!(self.comments),
        )
    }
    fn moved(&mut self, p: &Policy, from: &Path, to: &Path) {
        let a = from.to_string_lossy();
        let b = to.to_string_lossy().into_owned();
        if let Some(tags) = self.tags.remove(a.as_ref()) {
            self.tags.insert(b.clone(), tags);
            let _ = self.save_tags(p);
        }
        if let Some(comment) = self.comments.remove(a.as_ref()) {
            self.comments.insert(b, comment);
            let _ = self.save_comments(p);
        }
    }
}

impl Session {
    pub fn new(id: u64, args: &Value, shared: &mut Shared) -> Result<Self> {
        let policy = policy::current();
        shared.load(&policy, &args["preferences"]);
        if let Some(apps) = args["apps"].as_array() {
            shared.apps = apps.clone();
        }
        let mode = match args["mode"].as_str().unwrap_or("browser") {
            "open" => Mode::Open,
            "save" => Mode::Save,
            "folder" => Mode::Folder,
            _ => Mode::Browser,
        };
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| policy.home.join(".config"))
            .join("user-dirs.dirs");
        let places = user_dirs(&policy, &fs::read_to_string(config).unwrap_or_default());
        let location = match args["args"]["place"].as_str().or(args["place"].as_str()) {
            Some("trash") => Location::Trash,
            Some("recent") => Location::Recent,
            Some("apps") => Location::Apps,
            Some("network") => Location::Network,
            Some("computer") => Location::Directory(PathBuf::from("/")),
            _ => {
                let path = args["start"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or(args["args"]["path"].as_str());
                let p = path
                    .map(|s| policy.absolute(Path::new(s)))
                    .unwrap_or_else(|| {
                        let name = shared.preferences["newWindow"].as_str().unwrap_or("home");
                        places
                            .iter()
                            .find(|p| p.label == name)
                            .map(|p| p.path.clone())
                            .unwrap_or_else(|| policy.home.clone())
                    });
                Location::Directory(if p.is_file() {
                    p.parent().unwrap_or(&policy.home).to_owned()
                } else {
                    p
                })
            }
        };
        let filter = if let Some(pattern) = args["filter"].as_str().filter(|s| !s.is_empty()) {
            Some(
                regex::RegexBuilder::new(pattern)
                    .case_insensitive(args["ignoreCase"].as_bool().unwrap_or(false))
                    .size_limit(1024 * 1024)
                    .build()
                    .map_err(|_| "sys.badArgument".to_string())?,
            )
        } else {
            None
        };
        let (tx, rx) = mpsc::channel();
        let mut session = Self {
            id,
            mode,
            tabs: vec![Tab::new(location)],
            active: 0,
            places,
            title: args["title"].as_str().unwrap_or("").into(),
            name: args["name"].as_str().unwrap_or("").into(),
            multiple: args["multiple"].as_bool().unwrap_or(false),
            hidden: false,
            error: None,

            loading: false,
            dialog: None,
            context: None,
            width: 960.,
            height: 600.,
            theme: crate::native_style::files_theme(&args["theme"]),
            source_theme: Value::Null,
            locale: String::new(),
            effects: Vec::new(),
            dirty: true,
            closed: false,
            filter,
            policy,
            generation: Arc::new(AtomicU64::new(0)),
            tx,
            rx,
            watch: None,
            last_load: Instant::now(),
            last_scan: Duration::ZERO,
            silent_load: false,
            can_write: false,

            last_jobs: Value::Null,
            owned_jobs: HashSet::new(),
            reveal: None,
            thumbnail_revision: 0,
            pdf_thumbnails: super::pdf_thumbnails::Cache::default(),
            volume_busy: None,
            volume_paths: Vec::new(),
            side_width: shared.preferences["sideWidth"]
                .as_f64()
                .unwrap_or(190.)
                .clamp(150., 340.),
            zoom: shared.preferences["contentZoom"]
                .as_f64()
                .filter(|v| v.is_finite())
                .unwrap_or(1.)
                .clamp(0.75, 2.),
            zoom_save: None,
            zoom_wheel: 0.,
            drag: None,
            job_errors: Vec::new(),
            dialog_epoch: 0,
            network_epoch: 0,
            auth: None,
            busy: false,
            discovered: Vec::new(),
            network_status: Value::Null,
            searching: false,
            search_home: false,
            folders_first: shared.preferences["foldersFirst"].as_bool().unwrap_or(true),
        };
        if let Some(view) = args["args"]["view"]
            .as_str()
            .or(shared.preferences["defaultView"]["view"].as_str())
        {
            if ["icons", "list", "columns", "gallery"].contains(&view) {
                session.tab_mut().view = view.into();
            }
        }
        let initial = session.tab().location.clone();
        session.apply_view(&initial, shared);
        if let Some(v) = args["args"]["view"]
            .as_str()
            .filter(|v| ["icons", "list", "columns", "gallery"].contains(v))
        {
            session.tab_mut().view = v.into();
        }
        session.load(shared);
        Ok(session)
    }
    fn apply_view(&mut self, to: &Location, shared: &Shared) {
        let key = to
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                match to {
                    Location::Trash => ":trash",
                    Location::Recent => ":recent",
                    Location::Network => ":network",
                    Location::Apps => ":apps",
                    _ => ":",
                }
                .into()
            });
        let stored = &shared.preferences["folderViews"][&key];
        let cfg = if stored.is_object() {
            stored
        } else {
            &shared.preferences["defaultView"]
        };
        self.tab_mut().view = cfg["view"]
            .as_str()
            .filter(|v| ["icons", "list", "columns", "gallery"].contains(v))
            .unwrap_or("icons")
            .into();
        self.tab_mut().sort = cfg["sort"]["by"]
            .as_str()
            .filter(|v| ["name", "size", "date", "kind"].contains(v))
            .unwrap_or("name")
            .into();
        self.tab_mut().descending = cfg["sort"]["desc"].as_bool().unwrap_or(false);
    }
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }
    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }
    pub fn selected_entries(&self) -> impl Iterator<Item = &Entry> {
        self.tab()
            .selected
            .iter()
            .filter_map(|i| self.tab().entries.get(*i))
    }
    pub fn selected(&self) -> Vec<Entry> {
        self.selected_entries().cloned().collect()
    }
    pub fn write_dir(&self) -> Option<PathBuf> {
        writable_dir(&self.policy, self.tab().location.path()?)
    }
    pub fn filtered(&self) -> Rc<Vec<usize>> {
        let tab = self.tab();
        let folder = self.mode == Mode::Folder;
        if let Some(cached) = tab.filtered.borrow().as_ref().filter(|c| {
            c.version == tab.entries_version
                && c.len == tab.entries.len()
                && c.query == tab.query
                && c.hidden == self.hidden
                && c.folder == folder
        }) {
            return Rc::clone(&cached.items);
        }

        let query = tab.query.to_lowercase();
        let items = Rc::new(
            tab.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    (self.hidden || !e.name.starts_with('.'))
                        && (self.mode != Mode::Folder || e.dir)
                        && (e.dir || self.filter.as_ref().is_none_or(|r| r.is_match(&e.name)))
                        && (tab.query.is_empty() || e.name.to_lowercase().contains(&query))
                })
                .map(|(i, _)| i)
                .collect(),
        );
        *tab.filtered.borrow_mut() = Some(Filtered {
            version: tab.entries_version,
            len: tab.entries.len(),
            query: tab.query.clone(),
            hidden: self.hidden,
            folder,
            items: Rc::clone(&items),
        });
        items
    }
    fn reset_watch(&mut self) {
        self.watch = None;
        if let Some(path) = self.tab().location.path() {
            if let Ok(p) = policy::c_name(path.as_os_str()) {
                let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
                if fd >= 0 {
                    let f = unsafe { File::from_raw_fd(fd) };
                    if unsafe {
                        libc::inotify_add_watch(
                            fd,
                            p.as_ptr(),
                            libc::IN_CREATE
                                | libc::IN_DELETE
                                | libc::IN_MOVED_FROM
                                | libc::IN_MOVED_TO
                                | libc::IN_ATTRIB
                                | libc::IN_CLOSE_WRITE
                                | libc::IN_DELETE_SELF
                                | libc::IN_MOVE_SELF,
                        )
                    } >= 0
                    {
                        self.watch = Some(f);
                    }
                }
            }
        }
    }
    // A folder watcher's refresh keeps the current listing on screen until (and
    // unless) the new listing is actually different.
    pub(super) fn visibly_loading(&self) -> bool {
        self.loading && !self.silent_load
    }
    pub fn load(&mut self, shared: &Shared) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.silent_load = false;
        let location = self.tab().location.clone();
        self.error = None;
        self.searching = false;
        self.loading = true;
        self.last_load = Instant::now();
        self.reset_watch();
        self.dirty = true;
        if location.path().is_none() {
            self.can_write = false;
        }
        if let Location::Archive { file, .. } = &location {
            if self.tab().archive.is_empty() {
                self.effects
                    .push(json!({"kind":"archiveList","path":file,"generation":generation}));
            } else {
                self.archive_entries();
                self.loading = false;
            }
            return;
        }
        if location == Location::Network {
            self.effects
                .push(json!({"kind":"networkScan","generation":generation}));
        }
        let p = self.policy.clone();
        let tags = shared.tags.clone();
        let apps = shared.apps.clone();
        let tx = self.tx.clone();
        let epoch = self.generation.clone();
        let picker = self.mode != Mode::Browser;
        let ordering = (
            self.tab().sort.clone(),
            self.tab().descending,
            self.folders_first,
        );
        let r = work(move || {
            if epoch.load(Ordering::Relaxed) != generation {
                return;
            }
            let result = (|| -> Result<Vec<Entry>> {
                match location {
                    Location::Directory(path) => {
                        let writable = writable_dir(&p, &path).is_some();
                        let _ = tx.send(Notice::Writable(generation, writable));
                        let real = p.read(&path)?;
                        if picker && !p.allowed(&real) {
                            return Err("fs.outsideHome".into());
                        }
                        ops::list(&p, &real)
                    }
                    Location::Trash => Ok(trash::list()
                        .into_iter()
                        .filter_map(|t| {
                            ops::entry(Path::new(&t.id)).ok().map(|mut e| {
                                e.name = t.name;
                                e.original = Some(t.original);
                                e
                            })
                        })
                        .collect()),
                    Location::Recent => {
                        let (mut items, _) = ops::walk_pruned(
                            &p.home,
                            6,
                            60000,
                            ops::DurationBudget::seconds(2),
                            false,
                            &["node_modules"],
                        );
                        // newest first by the walk's own lstat; entries (one more stat each) only
                        // for the 300 shown, not for every file found
                        items.retain(|(_, m)| m.is_file());
                        items.sort_by_key(|(_, m)| std::cmp::Reverse(m.mtime()));
                        let entries: Vec<_> = items
                            .into_iter()
                            .filter_map(|(path, _)| ops::entry(&path).ok())
                            .take(300)
                            .collect();
                        Ok(entries)
                    }
                    Location::Tag(color) => Ok(tags
                        .iter()
                        .filter(|(_, colors)| colors.contains(&color))
                        .filter_map(|(path, _)| ops::entry(Path::new(path)).ok())
                        .collect()),
                    Location::Apps => Ok(apps
                        .iter()
                        .map(|a| Entry {
                            name: a["name"].as_str().unwrap_or("").into(),
                            path: format!("app:{}", a["id"].as_str().unwrap_or("")),
                            raw: PathBuf::new(),
                            dir: false,
                            link: false,
                            size: 0,
                            mtime: 0,
                            mode: 0,
                            writable: false,
                            original: None,
                            stamp: 0,
                        })
                        .collect()),
                    Location::Network => {
                        let mut entries = Vec::new();
                        for (path, _) in ops::mounts() {
                            if let Ok(e) = ops::entry(&path) {
                                entries.push(e);
                            }
                        }
                        let gvfs =
                            PathBuf::from(format!("/run/user/{}/gvfs", unsafe { libc::getuid() }));
                        if let Ok(children) = fs::read_dir(gvfs) {
                            for e in children.flatten() {
                                if let Ok(e) = ops::entry(&e.path()) {
                                    entries.push(e);
                                }
                            }
                        }
                        Ok(entries)
                    }
                    Location::Archive { .. } => unreachable!(),
                }
            })();
            let result = result.map(|mut entries| {
                sort_entries(&mut entries, &ordering.0, ordering.1, ordering.2);
                entries
            });
            let _ = tx.send(Notice::Scan(generation, result));
        });
        if let Err(e) = r {
            self.loading = false;
            self.error = Some(e);
        }
    }
    fn archive_entries(&mut self) {
        let Location::Archive { file, inner } = self.tab().location.clone() else {
            return;
        };
        let mut entries = HashMap::<String, Entry>::new();
        for item in &self.tab().archive {
            let raw = item["p"].as_str().unwrap_or("");
            if item["bad"] == true
                || raw.starts_with('/')
                || raw.contains('\0')
                || raw.split('/').any(|p| p == "..")
            {
                continue;
            }
            let path = item["p"].as_str().unwrap_or("").trim_matches('/');
            let rel = if inner.is_empty() {
                path
            } else {
                match path.strip_prefix(&(inner.clone() + "/")) {
                    Some(r) => r,
                    None => continue,
                }
            };
            if rel.is_empty() {
                continue;
            }
            let name = rel.split('/').next().unwrap();
            let dir = rel.contains('/') || item["d"].as_bool().unwrap_or(false);
            entries
                .entry(name.into())
                .and_modify(|e| e.dir |= dir)
                .or_insert(Entry {
                    name: name.into(),
                    path: format!(
                        "{}:{}{}",
                        file.display(),
                        if inner.is_empty() { "" } else { &inner },
                        if inner.is_empty() {
                            name.into()
                        } else {
                            format!("/{name}")
                        }
                    ),
                    raw: PathBuf::from(if inner.is_empty() {
                        name.into()
                    } else {
                        format!("{inner}/{name}")
                    }),
                    dir,
                    link: false,
                    size: item["s"].as_u64().unwrap_or(0),
                    mtime: item["m"].as_i64().unwrap_or(0),
                    mode: 0,
                    writable: false,
                    original: None,
                    stamp: 0,
                });
        }
        self.tab_mut()
            .replace_entries(entries.into_values().collect());
        self.sort();
    }
    fn sort(&mut self) {
        let selected: HashSet<_> = self
            .selected()
            .into_iter()
            .map(|e| (e.raw, e.path))
            .collect();
        let cursor = self
            .tab()
            .cursor
            .and_then(|i| self.tab().entries.get(i))
            .map(|e| (e.raw.clone(), e.path.clone()));
        let folders_first = self.folders_first;
        let tab = self.tab_mut();
        let key = tab.sort.clone();
        let desc = tab.descending;
        tab.entries_version = tab.entries_version.wrapping_add(1);
        sort_entries(&mut tab.entries, &key, desc, folders_first);
        let any_selected = !selected.is_empty();
        tab.cursor = cursor.and_then(|(raw, path)| {
            tab.entries
                .iter()
                .position(|e| e.raw == raw && e.path == path)
        });
        tab.anchor = tab.cursor.unwrap_or(0);
        tab.selected = if any_selected {
            tab.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| selected.contains(&(e.raw.clone(), e.path.clone())))
                .map(|(i, _)| i)
                .collect()
        } else {
            BTreeSet::new()
        };
    }
    pub fn navigate(&mut self, to: Location, shared: &Shared, history: bool) {
        if self.mode != Mode::Browser {
            if let Some(p) = to.path() {
                if !self.policy.allowed(&self.policy.absolute(p)) {
                    self.error = Some("fs.outsideHome".into());
                    self.dirty = true;
                    return;
                }
            }
        }
        if history {
            let old = self.tab().location.clone();
            if old != to {
                let tab = self.tab_mut();
                tab.back.push(old);
                tab.back.truncate(100);
                tab.forward.clear();
            }
        }
        if self.tab().view == "columns" {
            if let Some(next) = to.path() {
                let old = self.tab().location.path().map(Path::to_owned);
                let entries = self.tab().entries.clone();
                let cols = &mut self.tab_mut().columns;
                cols.retain(|(path, _)| next.starts_with(path) && next != *path);
                if let Some(old) = old.filter(|old| next.starts_with(old) && next != old) {
                    if !cols.iter().any(|(p, _)| p == &old) {
                        cols.push((old, entries));
                    }
                }
                if cols.len() > 3 {
                    cols.remove(0);
                }
            } else {
                self.tab_mut().columns.clear();
            }
        }
        let retained: HashSet<_> = self.tab().columns.iter().map(|(p, _)| p.clone()).collect();
        self.tab_mut()
            .column_offsets
            .retain(|p, _| retained.contains(p));
        let columns = self.tab().view == "columns";
        self.apply_view(&to, shared);
        if columns {
            self.tab_mut().view = "columns".into();
        }
        let tab = self.tab_mut();
        tab.location = to;
        tab.selected.clear();
        tab.offset = 0.;
        tab.query.clear();
        tab.archive.clear();
        self.context = None;
        self.load(shared);
    }
    pub fn flush_zoom(&mut self, shared: &Shared) {
        if self.zoom_save.take().is_some() {
            let _ = shared.save(&self.policy);
        }
    }
    fn set_zoom(&mut self, value: f64, shared: &mut Shared) {
        let value = value.clamp(0.75, 2.);
        if !value.is_finite() || (value - self.zoom).abs() < 0.001 {
            return;
        }
        let (columns, _, row) = self.entry_grid();
        let anchor = (self.tab().offset / row).floor() as usize * columns;
        let fraction = (self.tab().offset / row).fract();
        self.zoom = value;
        let (columns, _, row) = self.entry_grid();
        self.tab_mut().offset = ((anchor / columns) as f64 + fraction) * row;
        self.tab_mut().column_offsets.clear();
        self.clamp_scroll();
        shared.preferences["contentZoom"] = json!(value);
        self.zoom_save = Some(Instant::now() + Duration::from_millis(700));
        self.dirty = true;
    }
    fn zoom_scroll(&mut self, event: &Value, shared: &mut Shared) {
        let delta = event["deltaY"].as_f64().unwrap_or(0.);
        if !delta.is_finite() {
            return;
        }
        if self.zoom_wheel.signum() != delta.signum() {
            self.zoom_wheel = 0.;
        }
        self.zoom_wheel += delta.clamp(-384., 384.);
        let steps = (self.zoom_wheel / 48.).trunc().clamp(-8., 8.) as i32;
        self.zoom_wheel %= 48.;
        for _ in 0..steps.unsigned_abs() {
            self.zoom_step(-(steps as f64), shared);
        }
    }
    fn zoom_step(&mut self, direction: f64, shared: &mut Shared) {
        let levels = [0.75, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2.];
        let next = if direction > 0. {
            levels.into_iter().find(|v| *v > self.zoom + 0.001)
        } else {
            levels.into_iter().rev().find(|v| *v < self.zoom - 0.001)
        };
        if let Some(value) = next {
            self.set_zoom(value, shared);
        }
    }
    pub fn poll(&mut self, context: &Value, shared: &Shared) {
        if self.zoom_save.is_some_and(|due| Instant::now() >= due) {
            self.flush_zoom(shared);
        }
        let paths: Vec<_> = shared
            .volumes
            .iter()
            .filter_map(|v| v.path.clone())
            .collect();
        if paths != self.volume_paths {
            let removed: Vec<_> = self
                .volume_paths
                .iter()
                .filter(|p| !paths.contains(p))
                .cloned()
                .collect();
            self.volume_paths = paths;
            self.dirty = true;
            let affected = |loc: &Location| {
                loc.path()
                    .is_some_and(|q| removed.iter().any(|p| q.starts_with(p)))
            };
            let mut reload = false;
            for (i, tab) in self.tabs.iter_mut().enumerate() {
                tab.back.retain(|loc| !affected(loc));
                tab.forward.retain(|loc| !affected(loc));
                if affected(&tab.location) {
                    tab.location = Location::Directory(self.policy.home.clone());
                    tab.entries.clear();
                    tab.selected.clear();
                    tab.columns.clear();
                    tab.column_offsets.clear();
                    tab.offset = 0.;
                    tab.cursor = None;
                    reload |= i == self.active;
                }
            }
            if reload {
                self.load(shared);
            }
        }
        let current = policy::current();
        if current.home == self.policy.home && current.roots != self.policy.roots {
            self.policy = current;
            self.dirty = true;
        }
        let thumbs = super::thumbnails::revision();
        if thumbs != self.thumbnail_revision {
            self.thumbnail_revision = thumbs;
            self.dirty = true;
        }
        if let Some(w) = context["width"].as_f64() {
            let w = w.clamp(320., 16384.);
            if (self.width - w).abs() > 0.5 {
                self.width = w;
                self.dirty = true;
            }
        }
        if let Some(h) = context["height"].as_f64() {
            let h = h.clamp(240., 16384.);
            if (self.height - h).abs() > 0.5 {
                self.height = h;
                self.dirty = true;
            }
        }
        if context["theme"].is_object() && self.source_theme != context["theme"] {
            self.source_theme = context["theme"].clone();
            self.theme = crate::native_style::files_theme(&self.source_theme);
            self.dirty = true;
        }
        if let Some(l) = context["locale"].as_str() {
            if l != self.locale {
                self.locale = l.into();
                self.dirty = true;
            }
        }
        while let Ok(notice) = self.rx.try_recv() {
            match notice {
                Notice::Scan(generation, result)
                    if generation == self.generation.load(Ordering::Relaxed) =>
                {
                    self.loading = false;
                    self.last_scan = self.last_load.elapsed();
                    let silent = std::mem::take(&mut self.silent_load);
                    if silent && result.as_ref().is_ok_and(|e| *e == self.tab().entries) {
                        continue;
                    }
                    match result {
                        Ok(entries) => {
                            let selected: HashSet<_> = self
                                .selected()
                                .into_iter()
                                .map(|e| (e.raw, e.path))
                                .collect();
                            let cursor = self
                                .tab()
                                .cursor
                                .and_then(|i| self.tab().entries.get(i))
                                .map(|e| (e.raw.clone(), e.path.clone()));
                            self.tab_mut().replace_entries(entries);
                            self.tab_mut().cursor = cursor.and_then(|(raw, path)| {
                                self.tab()
                                    .entries
                                    .iter()
                                    .position(|e| e.raw == raw && e.path == path)
                            });
                            self.tab_mut().anchor = self.tab().cursor.unwrap_or(0);
                            if self.tab().location == Location::Network {
                                let discovered = self.discovered.clone();
                                self.tab_mut().entries.extend(discovered);
                                self.sort();
                            }
                            self.tab_mut().selected = if selected.is_empty() {
                                BTreeSet::new()
                            } else {
                                self.tab()
                                    .entries
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, e)| {
                                        selected.contains(&(e.raw.clone(), e.path.clone()))
                                    })
                                    .map(|(i, _)| i)
                                    .collect()
                            };
                        }
                        Err(e) => {
                            self.error = Some(e);
                            self.tab_mut().entries.clear();
                        }
                    }
                    self.select_revealed();
                    self.dirty = true;
                }
                Notice::Writable(generation, writable)
                    if generation == self.generation.load(Ordering::Relaxed) =>
                {
                    if self.can_write != writable {
                        self.can_write = writable;
                        self.dirty = true;
                    }
                }
                Notice::Text(generation, result) if generation == self.dialog_epoch => {
                    if let Some(Dialog::Preview { text, .. }) = &mut self.dialog {
                        match result {
                            Ok(t) => *text = Some(t),
                            Err(e) => self.error = Some(e),
                        }
                        self.dirty = true;
                    }
                }
                Notice::Size(generation, size) if generation == self.dialog_epoch => {
                    if let Some(Dialog::Info { size: s, .. }) = &mut self.dialog {
                        *s = Some(size);
                        self.dirty = true;
                    }
                }
                Notice::NetworkPrompt(epoch, prompt) if epoch == self.network_epoch => {
                    self.close_dialog();
                    self.auth = Some(prompt.reply);
                    self.dialog = Some(Dialog::Network {
                        value: prompt.value,
                    });
                    self.dirty = true;
                }
                Notice::Connected(epoch, result) if epoch == self.network_epoch => {
                    self.auth = None;
                    self.close_dialog();
                    self.busy = false;
                    match result {
                        Ok(path) => self.navigate(Location::Directory(path), shared, true),
                        Err(e) => self.error = Some(e),
                    }
                    self.dirty = true;
                }
                Notice::Action(result) => {
                    self.busy = false;
                    self.load(shared);
                    if let Err(e) = result {
                        self.error = Some(e);
                    }
                    self.dirty = true;
                }
                Notice::Search(epoch, result)
                    if epoch == self.generation.load(Ordering::Relaxed) =>
                {
                    self.loading = false;
                    match result {
                        Ok(entries) => {
                            self.tab_mut().replace_entries(entries);
                            if self.tab().location == Location::Network {
                                let discovered = self.discovered.clone();
                                self.tab_mut().entries.extend(discovered);
                                self.sort();
                            }
                            self.tab_mut().selected.clear();
                        }
                        Err(e) => self.error = Some(e),
                    }
                    self.dirty = true;
                }
                _ => (),
            }
        }
        // A folder that keeps changing must not spend most of its time being rescanned.
        let settle = (self.last_scan * 4).clamp(Duration::from_millis(250), Duration::from_secs(5));
        if !self.loading && self.last_load.elapsed() > settle {
            let mut changed = false;
            if let Some(fd) = &self.watch {
                let mut buf = [0u8; 8192];
                loop {
                    let n =
                        unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                    if n <= 0 {
                        break;
                    }
                    changed = true;
                }
            } else if self.tab().location.path().is_some()
                && self.last_load.elapsed() > Duration::from_secs(3)
            {
                changed = true;
            }
            if changed && !self.searching {
                let quiet = !self.dirty && self.error.is_none();
                self.load(shared);
                self.silent_load = self.loading;
                self.dirty = !quiet || self.error.is_some();
            }
        }
        let current_jobs = if self.owned_jobs.is_empty() {
            json!([])
        } else {
            json!(ops::jobs()
                .into_iter()
                .filter(|j| self.owned_jobs.contains(&j.id))
                .collect::<Vec<_>>())
        };
        if current_jobs != self.last_jobs {
            let was_active = self
                .last_jobs
                .as_array()
                .is_some_and(|a| a.iter().any(|j| j["done"] == false));
            for job in current_jobs.as_array().into_iter().flatten() {
                if job["failed"] == true {
                    let e = job["error"]
                        .as_str()
                        .unwrap_or("fs.copyStartFailed")
                        .to_owned();
                    if !self.job_errors.contains(&e) {
                        self.job_errors.push(e);
                    }
                }
            }
            for job in current_jobs
                .as_array()
                .into_iter()
                .flatten()
                .filter(|j| j["done"] == true)
            {
                if let Some(id) = job["id"].as_i64() {
                    ops::forget(id as i32);
                    self.owned_jobs.remove(&(id as i32));
                }
            }
            self.last_jobs = current_jobs;
            self.dirty = true;
            if was_active && !self.loading {
                self.load(shared);
            }
        }
        if self.dirty {
            self.clamp_scroll();
            if self.tab().view == "icons" || self.tab().view == "gallery" {
                let (cols, _, row) = self.entry_grid();
                let start = (self.tab().offset / row).floor() as usize * cols;
                let count = ((self.entry_area() / row).ceil() as usize + 2) * cols;
                let entries: Vec<_> = self
                    .filtered()
                    .iter()
                    .copied()
                    .skip(start)
                    .take(count.min(128))
                    .map(|i| self.tab().entries[i].clone())
                    .collect();
                self.pdf_thumbnails.queue(&entries, &mut self.effects);
            }
        }
    }
    pub fn jobs(&self) -> &Value {
        &self.last_jobs
    }
    fn open_entry(&mut self, e: Entry, shared: &Shared) {
        if e.path.starts_with("net:") {
            self.text_dialog(
                "connect",
                "files.menu.connect",
                e.raw.to_string_lossy().into_owned(),
                vec![],
            );
            return;
        }
        if e.link && e.mode & libc::S_IFMT == libc::S_IFLNK {
            self.error = Some("files.brokenLink".into());
            return;
        }
        if e.raw.to_str().is_none() {
            self.error = Some("files.filenameEncoding".into());
            return;
        }
        if e.path.starts_with("app:") {
            self.effects
                .push(json!({"kind":"launchApp","id":&e.path[4..]}));
            return;
        }
        if e.dir {
            if let Location::Archive { file, inner } = &self.tab().location {
                let to = Location::Archive {
                    file: file.clone(),
                    inner: if inner.is_empty() {
                        e.name.clone()
                    } else {
                        format!("{inner}/{}", e.name)
                    },
                };
                let flat = self.tab().archive.clone();
                let old = self.tab().location.clone();
                self.tab_mut().back.push(old);
                self.tab_mut().location = to;
                self.tab_mut().archive = flat;
                self.tab_mut().selected.clear();
                self.archive_entries();
                self.dirty = true;
            } else {
                if self.mode == Mode::Browser
                    && self.tabs.len() < 32
                    && shared.preferences["foldersInTabs"]
                        .as_bool()
                        .unwrap_or(false)
                {
                    self.tabs.push(Tab::new(Location::Directory(e.raw)));
                    self.active = self.tabs.len() - 1;
                    let loc = self.tab().location.clone();
                    self.apply_view(&loc, shared);
                    self.load(shared);
                } else {
                    self.navigate(Location::Directory(e.raw), shared, true);
                }
            }
            return;
        }
        if self.mode == Mode::Save {
            self.name = e.name;
            return;
        }
        if self.mode != Mode::Browser {
            self.choose(shared);
            return;
        }
        if let Location::Archive { file, .. } = &self.tab().location {
            self.effects.push(json!({"kind":"archiveOpen","path":file,"entry":e.raw,"generation":self.generation.load(Ordering::Relaxed)}));
            return;
        }
        if archive(&e.name) {
            self.navigate(
                Location::Archive {
                    file: e.raw,
                    inner: String::new(),
                },
                shared,
                true,
            );
            return;
        }
        let fallback = match kind(&e.name) {
            "image" => "photos",
            "pdf" => "pdf",
            "font" => "fonts",
            "video" | "audio" => "player",
            _ => "editor",
        };
        self.effects.push(
            json!({"kind":"openPath","path":e.raw,"fallback":format!("lordos-{fallback}.desktop")}),
        );
    }
    fn choose(&mut self, _shared: &Shared) {
        let paths = if self.mode == Mode::Save {
            if self.name.is_empty()
                || self.name.contains(['/', '\0'])
                || self.name == "."
                || self.name == ".."
            {
                self.error = Some("sys.badArgument".into());
                return;
            }
            let Some(dir) = self.tab().location.path() else {
                return;
            };
            let path = match self.policy.target(&dir.join(&self.name), false) {
                Ok(p) => p,
                Err(e) => {
                    self.error = Some(e);
                    return;
                }
            };
            if path.to_str().is_none() {
                self.error = Some("files.filenameEncoding".into());
                return;
            }
            if path.is_dir() {
                self.error = Some("fs.notFile".into());
                return;
            }
            if path.exists() {
                self.dialog = Some(Dialog::Confirm {
                    action: "overwrite".into(),
                    paths: vec![path],
                });
                return;
            }
            vec![path]
        } else if self.mode == Mode::Folder {
            let s = self.selected();
            if s.len() == 1 && s[0].dir {
                vec![s[0].raw.clone()]
            } else if let Some(p) = self.tab().location.path() {
                vec![p.into()]
            } else {
                return;
            }
        } else {
            let mut chosen: Vec<_> = self
                .selected()
                .into_iter()
                .filter(|e| !e.dir && self.filter.as_ref().is_none_or(|f| f.is_match(&e.name)))
                .map(|e| e.raw)
                .collect();
            if chosen.is_empty() {
                if let Some(e) = self.selected().into_iter().find(|e| e.dir) {
                    self.navigate(Location::Directory(e.raw), _shared, true);
                }
                return;
            }
            if !self.multiple {
                chosen.truncate(1);
            }
            chosen
        };
        if paths.iter().any(|p| p.to_str().is_none()) {
            self.error = Some("files.filenameEncoding".into());
            return;
        }
        self.effects.push(json!({"kind":"choose","paths":paths}));
        self.closed = true;
    }
    /// What was just renamed or created, selected (with the cursor) once the listing has it.
    fn select_revealed(&mut self) {
        let Some(want) = self.reveal.take() else { return };
        let Some(i) = self.tab().entries.iter().position(|e| e.raw == want) else {
            return;
        };
        let tab = self.tab_mut();
        tab.selected = BTreeSet::from([i]);
        tab.cursor = Some(i);
        tab.anchor = i;
    }
    fn text_dialog(&mut self, action: &str, title: &str, value: String, paths: Vec<PathBuf>) {
        self.context = None;
        self.close_dialog();
        self.dialog = Some(Dialog::Text {
            action: action.into(),
            title: title.into(),
            value,
            paths,
        });
    }
    fn close_dialog(&mut self) {
        self.dialog_epoch += 1;
        if let Some(reply) = self.auth.take() {
            let _ = reply.send(json!({"cancel":true}));
        }
        if let Some(Dialog::Preview { media, image, .. }) = self.dialog.take() {
            self.effects
                .push(json!({"kind":"closePreview","media":media,"image":image}));
        }
    }
    fn remember_view(&self, shared: &mut Shared) {
        if let Some(p) = self.tab().location.path() {
            shared.preferences["folderViews"][p.to_string_lossy().as_ref()] = json!({"view":self.tab().view,"sort":{"by":self.tab().sort,"desc":self.tab().descending}});
            if let Some(views) = shared.preferences["folderViews"].as_object_mut() {
                while views.len() > 400 {
                    let key = views
                        .keys()
                        .find(|key| *key != p.to_string_lossy().as_ref())
                        .cloned();
                    if let Some(key) = key {
                        views.remove(&key);
                    } else {
                        break;
                    }
                }
            }
            let _ = shared.save(&self.policy);
        }
    }
    pub fn event(&mut self, event: &Value, shared: &mut Shared) {
        let quiet = !self.dirty && self.error.is_none();
        self.dirty = true;
        self.error = None;
        let name = event["action"][0]
            .as_str()
            .or(event["action"].as_str())
            .unwrap_or("");
        let arg = &event["action"][1];
        // JSON travels through JavaScript: usize::MAX is not an exact Number.
        // An absent index means background, except for closing the active tab.
        let index = arg
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .unwrap_or_else(|| match name {
                "closeTab" => self.active,
                "context" | "drop" | "select" | "activate" | "dragStart" | "tab" => usize::MAX,
                _ => 0,
            });
        let value = event["value"].as_str().unwrap_or("");
        let result = (|| -> Result<()> {
            if self.closed {
                return Ok(());
            }
            if ["openWith", "compress", "extract", "preview"].contains(&name)
                && self.selected().iter().any(|e| e.raw.to_str().is_none())
            {
                return Err("files.filenameEncoding".into());
            }
            if self.mode != Mode::Browser
                && [
                    "newWindow",
                    "newTab",
                    "closeTab",
                    "copy",
                    "cut",
                    "paste",
                    "duplicate",
                    "trash",
                    "restore",
                    "delete",
                    "emptyTrash",
                    "connect",
                    "preferences",
                    "compress",
                    "extract",
                    "openWith",
                    "tag",
                    "pin",
                    "terminal",
                    "dragStart",
                    "drop",
                ]
                .contains(&name)
            {
                return Err("fs.outsideHome".into());
            }
            match name {
                "disks" => {
                    let volumes = super::volumes::parse(&event["disks"]);
                    self.policy =
                        policy::mount_roots(volumes.iter().filter_map(|v| v.path.clone()));
                    shared.volumes = volumes;
                    shared.extra_mounts = ops::mounts();
                }
                "volumeOpen" | "volumeRelease" => {
                    if self.volume_busy.is_some() {
                        return Ok(());
                    }
                    let volume = shared
                        .volumes
                        .iter()
                        .find(|v| v.id == arg.as_str().unwrap_or(""))
                        .cloned()
                        .ok_or("sys.badArgument")?;
                    if name == "volumeOpen" {
                        if let Some(path) = volume.path {
                            self.navigate(Location::Directory(path), shared, true);
                        } else if !volume.mountable {
                            self.effects
                                .push(json!({"kind":"manageDisk","disk":volume.disk}));
                        } else {
                            self.volume_busy = Some(volume.id.clone());
                            self.effects
                                .push(json!({"kind":"diskMount","device":volume.id}));
                        }
                    } else if volume.path.is_some() {
                        self.volume_busy = Some(volume.id.clone());
                        self.effects.push(json!({"kind":"diskRelease","device":if volume.removable{volume.disk}else{volume.id},"action":if volume.removable{"disk-eject"}else{"disk-unmount"}}));
                    }
                }
                "apps" => {
                    if let Some(apps) = event["apps"].as_array() {
                        shared.apps = apps.iter().take(512).cloned().collect();
                        if self.tab().location == Location::Apps {
                            self.load(shared);
                        }
                    }
                }
                "navigate" => self.navigate(
                    Location::Directory(PathBuf::from(arg.as_str().unwrap_or(""))),
                    shared,
                    true,
                ),
                "place" => {
                    let loc = match arg.as_str().unwrap_or("") {
                        "home" => Location::Directory(self.policy.home.clone()),
                        "computer" => Location::Directory(PathBuf::from("/")),
                        "trash" => Location::Trash,
                        "recent" => Location::Recent,
                        "apps" => Location::Apps,
                        "network" => Location::Network,
                        label => Location::Directory(
                            self.places
                                .iter()
                                .find(|p| p.label == label)
                                .map(|p| p.path.clone())
                                .unwrap_or(self.policy.home.clone()),
                        ),
                    };
                    self.navigate(loc, shared, true);
                }
                "back" => {
                    if let Some(to) = self.tab_mut().back.pop() {
                        let old = self.tab().location.clone();
                        self.tab_mut().forward.push(old);
                        self.navigate(to, shared, false);
                    }
                }
                "forward" => {
                    if let Some(to) = self.tab_mut().forward.pop() {
                        let old = self.tab().location.clone();
                        self.tab_mut().back.push(old);
                        self.navigate(to, shared, false);
                    }
                }
                "up" => {
                    if let Some(p) = self.tab().location.path().and_then(|p| p.parent()) {
                        self.navigate(Location::Directory(p.into()), shared, true);
                    }
                }
                "refresh" => {
                    if self.searching {
                        self.run_search()?;
                    } else {
                        self.load(shared);
                    }
                }
                "searchScope" => {
                    self.search_home = !self.search_home;
                    if !self.tab().query.is_empty() {
                        self.run_search()?;
                    }
                }
                "search" => self.run_search()?,
                "networkField" => {
                    if let Some(Dialog::Network { value: v }) = &mut self.dialog {
                        if let Some(k) = arg
                            .as_str()
                            .filter(|k| ["user", "password", "domain"].contains(k))
                        {
                            v[k] = json!(value);
                        }
                    }
                }
                "networkReply" => {
                    if let Some(reply) = self.auth.take() {
                        let response = if let Some(Dialog::Network { value }) = &self.dialog {
                            if value["type"] == "question" {
                                json!({"choice":arg})
                            } else {
                                json!({"user":value["user"],"domain":value["domain"],"password":value["password"],"guest":arg=="guest"})
                            }
                        } else {
                            json!({"cancel":true})
                        };
                        let _ = reply.send(response);
                    }
                    self.close_dialog();
                }
                "select" => {
                    if index < self.tab().entries.len() {
                        let ctrl = event["ctrl"].as_bool().unwrap_or(false)
                            || event["meta"].as_bool().unwrap_or(false);
                        let shift = event["shift"].as_bool().unwrap_or(false);
                        let multi = self.mode == Mode::Browser || self.multiple;
                        let items = self.filtered();
                        if !items.contains(&index) {
                            return Ok(());
                        }
                        let tab = self.tab_mut();
                        if shift && multi {
                            let a =
                                items
                                    .iter()
                                    .position(|i| *i == tab.anchor)
                                    .unwrap_or_else(|| {
                                        items.iter().position(|i| *i == index).unwrap_or(0)
                                    });
                            let b = items.iter().position(|i| *i == index).unwrap_or(a);
                            tab.selected = items[a.min(b)..=a.max(b)].iter().copied().collect();
                        } else if ctrl && multi {
                            if !tab.selected.remove(&index) {
                                tab.selected.insert(index);
                            }
                        } else {
                            tab.selected.clear();
                            tab.selected.insert(index);
                        }
                        if !shift {
                            tab.anchor = index;
                        }
                        tab.cursor = Some(index);
                        if self.mode == Mode::Save {
                            self.name = self.tab().entries[index].name.clone();
                        }
                    }
                }
                "openRaw" => {
                    let path = PathBuf::from(arg.as_str().ok_or("sys.badArgument")?);
                    let real = self.policy.read(&path)?;
                    let e = ops::entry(&real).map_err(io_error)?;
                    if self.mode == Mode::Folder
                        || (self.mode == Mode::Open
                            && self.filter.as_ref().is_some_and(|f| !f.is_match(&e.name)))
                    {
                        return Ok(());
                    }
                    if self.mode == Mode::Open && !e.dir {
                        if e.raw.to_str().is_none() {
                            return Err("files.filenameEncoding".into());
                        }
                        self.effects.push(json!({"kind":"choose","paths":[e.raw]}));
                        self.closed = true;
                        return Ok(());
                    }
                    self.open_entry(e, shared);
                }
                "activate" => {
                    if let Some(e) = self.tab().entries.get(index).cloned() {
                        self.tab_mut().selected = BTreeSet::from([index]);
                        self.open_entry(e, shared);
                    }
                }
                "open" => {
                    let entries = self.selected();
                    for entry in entries {
                        self.open_entry(entry, shared);
                    }
                }
                "selectAll" => {
                    let items = self.filtered();
                    let limit = if self.mode != Mode::Browser && !self.multiple {
                        1
                    } else {
                        items.len()
                    };
                    self.tab_mut().selected = items.iter().copied().take(limit).collect();
                }
                "clear" => {
                    self.tab_mut().selected.clear();
                    self.context = None;
                }
                "view" => {
                    if let Some(v) = arg
                        .as_str()
                        .filter(|v| ["icons", "list", "columns", "gallery"].contains(v))
                    {
                        self.tab_mut().view = v.into();
                        self.tab_mut().offset = 0.;
                        self.remember_view(shared);
                    }
                }
                "sort" => {
                    let key = arg.as_str().unwrap_or("name");
                    if self.tab().sort == key {
                        self.tab_mut().descending = !self.tab().descending;
                    } else {
                        self.tab_mut().sort = key.into();
                        self.tab_mut().descending = false;
                    }
                    self.sort();
                    self.remember_view(shared);
                }
                "hidden" => {
                    self.hidden = !self.hidden;
                }
                "query" => {
                    if self.searching {
                        self.load(shared);
                    }
                    self.tab_mut().query = value.into();
                    self.tab_mut().offset = 0.;
                }
                "columnWheel" => {
                    if event["ctrl"] == true {
                        self.zoom_scroll(event, shared);
                        return Ok(());
                    }
                    self.zoom_wheel = 0.;
                    let path = PathBuf::from(arg.as_str().ok_or("sys.badArgument")?);
                    if let Some((_, entries)) = self.tab().columns.iter().find(|(p, _)| *p == path)
                    {
                        let count = entries
                            .iter()
                            .filter(|e| {
                                (self.hidden || !e.name.starts_with('.'))
                                    && e.raw.to_str().is_some()
                            })
                            .count();
                        let row = self.column_row();
                        let max =
                            (count as f64 * row - (self.entry_height() - row).max(1.)).max(0.);
                        let old = self.tab().column_offsets.get(&path).copied().unwrap_or(0.);
                        self.tab_mut().column_offsets.insert(
                            path,
                            (old + event["deltaY"].as_f64().unwrap_or(0.)).clamp(0., max),
                        );
                    }
                }
                "zoom" => {
                    self.zoom_wheel = 0.;
                    self.set_zoom(arg.as_f64().unwrap_or(1.), shared);
                }
                "wheel" => {
                    if event["ctrl"] == true {
                        self.zoom_scroll(event, shared);
                        return Ok(());
                    }
                    self.zoom_wheel = 0.;
                    let delta = event["deltaY"].as_f64().unwrap_or(0.);
                    let before = self.tab().offset;
                    let tab = self.tab_mut();
                    tab.offset = (tab.offset + delta).max(0.);
                    self.clamp_scroll();
                    self.dirty = !quiet || self.tab().offset != before;
                }
                "scroll" => {
                    self.tab_mut().offset = arg.as_f64().unwrap_or(0.).max(0.);
                    self.clamp_scroll();
                }
                "newTab" => {
                    if self.tabs.len() >= 32 {
                        return Err("fs.tooManyCopies".into());
                    }
                    self.tabs.push(Tab::new(self.tab().location.clone()));
                    self.active = self.tabs.len() - 1;
                    self.load(shared);
                }
                "tab" => {
                    if index < self.tabs.len() {
                        self.active = index;
                        self.load(shared);
                    }
                }
                "closeTab" => {
                    if self.tabs.len() > 1 {
                        let index = index.min(self.tabs.len() - 1);
                        self.tabs.remove(index);
                        if index < self.active {
                            self.active -= 1;
                        }
                        self.active = self.active.min(self.tabs.len() - 1);
                        self.load(shared);
                    } else {
                        self.effects.push(json!({"kind":"close"}));
                        self.closed = true;
                    }
                }
                "newWindow" => self
                    .effects
                    .push(json!({"kind":"newWindow","path":self.tab().location.path()})),
                "copy" | "cut" => {
                    shared.clipboard = self.selected().iter().map(|e| e.raw.clone()).collect();
                    shared.cut = name == "cut";
                    self.context = None;
                }
                "copyPath" => {
                    self.effects.push(json!({"kind":"clipboardText","text":self.selected().iter().map(|e|e.path.clone()).collect::<Vec<_>>().join("\n")}));
                    self.context = None;
                }
                "paste" => {
                    let dir = self.write_dir().ok_or("fs.outsideHome")?;
                    let mut items = Vec::new();
                    for from in &shared.clipboard {
                        let mut dst = dir.join(from.file_name().ok_or("sys.badArgument")?);
                        if !shared.cut && dst == *from {
                            dst = free_name(
                                &dir,
                                &from.file_name().unwrap_or_default().to_string_lossy(),
                            );
                        }
                        if shared.cut && dst == *from {
                            continue;
                        }
                        items.push((from.clone(), dst, shared.cut));
                    }
                    if !items.is_empty() {
                        self.owned_jobs.insert(ops::start_batch(items)?);
                    }
                    if shared.cut {
                        shared.clipboard.clear();
                    }
                    self.context = None;
                }
                "duplicate" => {
                    let mut items = Vec::new();
                    for e in self.selected() {
                        let parent = e.raw.parent().ok_or("sys.badArgument")?;
                        items.push((e.raw.clone(), free_name(parent, &e.name), false));
                    }
                    if !items.is_empty() {
                        self.owned_jobs.insert(ops::start_batch(items)?);
                    }
                    self.context = None;
                }
                "rename" => {
                    let s = self.selected();
                    if s.len() == 1 {
                        self.text_dialog(
                            "rename",
                            "files.menu.rename",
                            s[0].name.clone(),
                            vec![s[0].raw.clone()],
                        );
                    }
                }
                "newFolder" => {
                    let p = self.write_dir().ok_or("fs.outsideHome")?;
                    self.text_dialog("mkdir", "files.menu.newFolder", String::new(), vec![p]);
                }
                "goTo" => self.text_dialog(
                    "goto",
                    "files.menu.goToFolder",
                    self.tab()
                        .location
                        .path()
                        .unwrap_or(&self.policy.home)
                        .to_string_lossy()
                        .into(),
                    vec![],
                ),
                "connect" => {
                    self.text_dialog("connect", "files.menu.connect", "smb://".into(), vec![])
                }
                "trash" => {
                    self.mutate(
                        "trash",
                        self.selected().iter().map(|e| e.raw.clone()).collect(),
                    )?;
                }
                "restore" => {
                    self.mutate(
                        "restore",
                        self.selected().iter().map(|e| e.raw.clone()).collect(),
                    )?;
                }
                "delete" => {
                    self.dialog = Some(Dialog::Confirm {
                        action: "delete".into(),
                        paths: self.selected().iter().map(|e| e.raw.clone()).collect(),
                    })
                }
                "emptyTrash" => {
                    if shared.preferences["warnEmptyTrash"]
                        .as_bool()
                        .unwrap_or(true)
                    {
                        self.dialog = Some(Dialog::Confirm {
                            action: "empty".into(),
                            paths: vec![],
                        });
                    } else {
                        self.mutate("empty", vec![])?;
                    }
                }
                "confirm" => {
                    if let Some(Dialog::Confirm { action, paths }) = self.dialog.take() {
                        match action.as_str() {
                            "overwrite" => {
                                if paths.iter().any(|p| p.to_str().is_none()) {
                                    return Err("files.filenameEncoding".into());
                                }
                                self.effects.push(json!({"kind":"choose","paths":paths}));
                                self.closed = true;
                            }
                            "extension" => {
                                if paths.len() != 2 {
                                    return Err("sys.badArgument".into());
                                }
                                ops::rename(&paths[0], &paths[1])?;
                                shared.moved(&self.policy, &paths[0], &paths[1]);
                                self.reveal = Some(paths[1].clone());
                                self.load(shared);
                            }
                            "delete" => self.mutate("delete", paths)?,
                            "empty" => self.mutate("empty", vec![])?,
                            _ => (),
                        }
                    }
                }
                "dragStart" => {
                    if event["button"].as_u64().unwrap_or(0) == 0
                        && self.mode == Mode::Browser
                        && index < self.tab().entries.len()
                    {
                        let paths = if self.tab().selected.contains(&index) {
                            self.selected().into_iter().map(|e| e.raw).collect()
                        } else {
                            vec![self.tab().entries[index].raw.clone()]
                        };
                        shared.dragging = Some((self.id, paths, false));
                        if event["ctrl"] != true
                            && event["meta"] != true
                            && event["shift"] != true
                            && !self.tab().selected.contains(&index)
                        {
                            self.tab_mut().selected = BTreeSet::from([index]);
                        }
                        self.drag = Some(
                            json!({"index":index,"x":event["x"],"y":event["y"],"active":false}),
                        );
                    }
                }
                "scrollDrag" => {
                    if event["button"].as_u64().unwrap_or(0) == 0 {
                        let (cols, _, row) = self.entry_grid();
                        let content = self.filtered().len().div_ceil(cols) as f64 * row;
                        let area = self.entry_area();
                        let thumb = (area * area / content.max(1.)).max(24.).min(area);
                        self.drag = Some(
                            json!({"kind":"scroll","y":event["y"],"initial":self.tab().offset,"ratio":(content-area).max(0.)/(area-thumb).max(1.)}),
                        );
                    }
                }
                "pointerMove" => {
                    if let Some(drag) = self.drag.as_ref().filter(|d| d["kind"] == "scroll") {
                        let offset = drag["initial"].as_f64().unwrap_or(0.)
                            + (event["y"].as_f64().unwrap_or(0.)
                                - drag["y"].as_f64().unwrap_or(0.))
                                * drag["ratio"].as_f64().unwrap_or(0.);
                        let before = self.tab().offset;
                        self.tab_mut().offset = offset;
                        self.clamp_scroll();
                        self.dirty = !quiet || self.tab().offset != before;
                        return Ok(());
                    }
                    if let Some(drag) = &mut self.drag {
                        if drag["kind"] == "sidebar" {
                            self.side_width = (drag["initial"].as_f64().unwrap_or(190.)
                                + event["x"].as_f64().unwrap_or(0.)
                                - drag["x"].as_f64().unwrap_or(0.))
                            .clamp(150., 340.);
                            return Ok(());
                        }
                        let dx =
                            event["x"].as_f64().unwrap_or(0.) - drag["x"].as_f64().unwrap_or(0.);
                        let dy =
                            event["y"].as_f64().unwrap_or(0.) - drag["y"].as_f64().unwrap_or(0.);
                        if dx * dx + dy * dy > 36. {
                            drag["active"] = json!(true);
                            if let Some((_, _, active)) = &mut shared.dragging {
                                *active = true;
                            }
                            drag["atX"] = event["x"].clone();
                            drag["atY"] = event["y"].clone();
                        }
                    }
                }
                "drop" => {
                    if let Some((_, paths, true)) = shared.dragging.take() {
                        let target = arg.as_str();
                        if target == Some("trash") {
                            self.mutate("trash", paths)?;
                        } else {
                            let destination = if let Some(p) = target {
                                self.policy.read(Path::new(p))?
                            } else if let Some(e) = self.tab().entries.get(index).filter(|e| e.dir)
                            {
                                e.raw.clone()
                            } else {
                                self.write_dir().ok_or("fs.outsideHome")?
                            };
                            let mut items = Vec::new();
                            for from in paths {
                                let to =
                                    destination.join(from.file_name().ok_or("sys.badArgument")?);
                                if to != from {
                                    items.push((from, to, event["alt"] != true));
                                }
                            }
                            if !items.is_empty() {
                                self.owned_jobs.insert(ops::start_batch(items)?);
                            }
                        }
                        self.context = None;
                    }
                    self.drag = None;
                }
                "resizeSide" => {
                    self.drag =
                        Some(json!({"kind":"sidebar","x":event["x"],"initial":self.side_width}));
                }
                "dragEnd" => {
                    if self.drag.as_ref().is_some_and(|d| d["kind"] == "sidebar") {
                        shared.preferences["sideWidth"] = json!(self.side_width);
                        shared.save(&self.policy)?;
                    }
                    self.drag = None;
                    if shared.dragging.as_ref().is_some_and(|d| d.0 == self.id) {
                        shared.dragging = None;
                    }
                }
                "recentServer" => {
                    if let Some(Dialog::Text { value, .. }) = &mut self.dialog {
                        *value = arg.as_str().unwrap_or("").into();
                    }
                }
                "dialogText" => {
                    if let Some(Dialog::Text { value: v, .. }) = &mut self.dialog {
                        *v = value.into();
                    }
                }
                "submit" => self.submit(shared)?,
                "cancelDialog" => self.close_dialog(),
                "name" => self.name = value.into(),
                "choose" => self.choose(shared),
                "cancel" => {
                    self.effects.push(json!({"kind":"cancel"}));
                    self.closed = true;
                }
                "info" => {
                    self.close_dialog();
                    let entries = self.selected();
                    let paths: Vec<_> = entries.iter().map(|e| e.raw.clone()).collect();
                    let comment = entries
                        .first()
                        .and_then(|e| shared.comments.get(&e.path))
                        .cloned()
                        .unwrap_or_default();
                    self.dialog = Some(Dialog::Info {
                        entries,
                        size: None,
                        comment,
                    });
                    let tx = self.tx.clone();
                    let generation = self.dialog_epoch;
                    work(move || {
                        let total = paths
                            .iter()
                            .map(|p| {
                                ops::walk(p, 64, 100000, ops::DurationBudget::millis(500.), true)
                                    .0
                                    .iter()
                                    .filter(|(_, m)| m.is_file())
                                    .map(|(_, m)| m.len())
                                    .sum::<u64>()
                            })
                            .sum();
                        let _ = tx.send(Notice::Size(generation, total));
                    })?;
                    self.context = None;
                }
                "comment" => {
                    if let Some(Dialog::Info {
                        entries, comment, ..
                    }) = &mut self.dialog
                    {
                        *comment = value.chars().take(500).collect();
                        if let Some(e) = entries.first() {
                            shared.comments.insert(e.path.clone(), comment.clone());
                            shared.save_comments(&self.policy)?;
                        }
                    }
                }
                "preview" => self.preview()?,
                "context" => {
                    if index < self.tab().entries.len() && !self.tab().selected.contains(&index) {
                        self.tab_mut().selected = BTreeSet::from([index]);
                    }
                    self.context = Some((
                        event["x"].as_f64().unwrap_or(10.),
                        event["y"].as_f64().unwrap_or(50.),
                    ));
                }
                "closeContext" => self.context = None,
                "jobCancel" => ops::cancel(index as i32),
                "jobForget" => {
                    ops::forget(index as i32);
                    self.job_errors.clear();
                }
                "preferences" => {
                    self.dialog = Some(Dialog::Preferences);
                    self.context = None;
                }
                "pref" => {
                    let key = arg.as_str().unwrap_or("");
                    if ![
                        "foldersInTabs",
                        "showExtensions",
                        "warnExtension",
                        "warnEmptyTrash",
                        "foldersFirst",
                    ]
                    .contains(&key)
                    {
                        return Err("sys.badArgument".into());
                    }
                    let default = [
                        "showExtensions",
                        "warnExtension",
                        "warnEmptyTrash",
                        "foldersFirst",
                    ]
                    .contains(&key);
                    shared.preferences[key] =
                        json!(!shared.preferences[key].as_bool().unwrap_or(default));
                    self.folders_first =
                        shared.preferences["foldersFirst"].as_bool().unwrap_or(true);
                    self.sort();
                    shared.save(&self.policy)?;
                }
                "sideWidth" => {
                    self.side_width = event["number"].as_f64().unwrap_or(190.).clamp(150., 340.);
                    shared.preferences["sideWidth"] = json!(self.side_width);
                    shared.save(&self.policy)?;
                }
                "tag" => {
                    let color = arg.as_str().unwrap_or("blue");
                    for e in self.selected() {
                        let tags = shared.tags.entry(e.path).or_default();
                        if let Some(i) = tags.iter().position(|s| s == color) {
                            tags.remove(i);
                        } else {
                            tags.push(color.into());
                        }
                    }
                    shared.save_tags(&self.policy)?;
                    self.context = None;
                }
                "tagPlace" => self.navigate(
                    Location::Tag(arg.as_str().unwrap_or("").into()),
                    shared,
                    true,
                ),
                "pin" => {
                    if let Some(path) = self.tab().location.path() {
                        let favorites = shared.preferences["favoriteFolders"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        let mut favorites = favorites;
                        let text = path.to_string_lossy();
                        if let Some(i) = favorites
                            .iter()
                            .position(|p| p["path"].as_str() == Some(text.as_ref()))
                        {
                            favorites.remove(i);
                        } else {
                            favorites.push(json!({"path":path,"name":path.file_name().unwrap_or_default().to_string_lossy()}));
                        }
                        shared.preferences["favoriteFolders"] = json!(favorites);
                        shared.save(&self.policy)?;
                    }
                }
                "terminal" => self
                    .effects
                    .push(json!({"kind":"terminal","path":self.tab().location.path()})),
                "openWith" => {
                    for e in self.selected() {
                        self.effects
                            .push(json!({"kind":"openPath","path":e.raw,"app":arg}));
                    }
                    self.context = None;
                }
                "compress" => {
                    let entries = self.selected();
                    if !entries.is_empty() {
                        let dir = self.write_dir().ok_or("fs.outsideHome")?;
                        self.effects.push(json!({"kind":"archiveCreate","paths":entries.iter().map(|e|e.raw.clone()).collect::<Vec<_>>(),"destination":free_name(&dir,"Archive.zip")}));
                        self.context = None;
                    }
                }
                "extract" => {
                    for e in self.selected() {
                        if archive(&e.name) {
                            let parent = e.raw.parent().ok_or("sys.badArgument")?;
                            self.effects.push(json!({"kind":"archiveExtract","path":e.raw,"destination":free_name(parent,e.name.split('.').next().unwrap_or("Archive"))}));
                        }
                    }
                    self.context = None;
                }
                "service" => self.service(event, shared)?,
                "key" => self.key(event, shared)?,
                _ => (),
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.error = Some(e);
            self.context = None;
        }
    }
    fn clamp_scroll(&mut self) {
        let (cols, _, row) = self.entry_grid();
        let area = self.entry_area();
        let max = (self.filtered().len().div_ceil(cols) as f64 * row - area).max(0.);
        self.tab_mut().offset = self.tab().offset.clamp(0., max);
    }
    fn mutate(&mut self, action: &str, paths: Vec<PathBuf>) -> Result<()> {
        if self.busy {
            return Err("fs.tooManyCopies".into());
        }
        self.busy = true;
        self.context = None;
        let tx = self.tx.clone();
        let action = action.to_owned();
        let result = work(move || {
            let result = (|| -> Result<()> {
                if action == "empty" {
                    return trash::empty().map(|_| ());
                }
                for p in paths {
                    match action.as_str() {
                        "trash" => {
                            trash::put(&p)?;
                        }
                        "restore" => trash::restore(&p)?,
                        "delete" => trash::remove(&p)?,
                        _ => return Err("sys.badArgument".into()),
                    }
                }
                Ok(())
            })();
            let _ = tx.send(Notice::Action(result));
        });
        if result.is_err() {
            self.busy = false;
        }
        result
    }
    fn run_search(&mut self) -> Result<()> {
        let query = self.tab().query.to_lowercase();
        if query.trim().is_empty() {
            return Ok(());
        }
        let root = if self.search_home {
            self.policy.home.clone()
        } else {
            self.tab()
                .location
                .path()
                .unwrap_or(&self.policy.home)
                .to_owned()
        };
        let root = self.policy.read(&root)?;
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let epoch = self.generation.clone();
        let tx = self.tx.clone();
        self.loading = true;
        self.searching = true;
        let result = work(move || {
            if epoch.load(Ordering::Relaxed) != generation {
                return;
            }
            let (items, _) = ops::walk(&root, 12, 60000, ops::DurationBudget::seconds(2), false);
            let result = items
                .into_iter()
                .filter(|(path, _)| {
                    path.file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&query))
                })
                .filter_map(|(path, _)| ops::entry(&path).ok())
                .take(2000)
                .collect();
            let _ = tx.send(Notice::Search(generation, Ok(result)));
        });
        if result.is_err() {
            self.loading = false;
            self.searching = false;
        }
        result
    }
    fn submit(&mut self, shared: &mut Shared) -> Result<()> {
        let Some(Dialog::Text {
            action,
            value,
            paths,
            ..
        }) = self.dialog.clone()
        else {
            return Ok(());
        };
        match action.as_str() {
            "rename" | "mkdir" => {
                if value.trim().is_empty()
                    || value == "."
                    || value == ".."
                    || value.contains('/')
                    || value.contains('\0')
                {
                    return Err("sys.badArgument".into());
                }
                let source = paths.first().ok_or("sys.badArgument")?;
                if action == "rename" {
                    let to = source.with_file_name(&value);
                    let ext = |p: &Path| p.extension().map(|e| e.to_string_lossy().to_lowercase());
                    if shared.preferences["warnExtension"]
                        .as_bool()
                        .unwrap_or(true)
                        && source.is_file()
                        && ext(source) != ext(&to)
                    {
                        self.dialog = Some(Dialog::Confirm {
                            action: "extension".into(),
                            paths: vec![source.clone(), to],
                        });
                        return Ok(());
                    }
                    ops::rename(source, &to)?;
                    shared.moved(&self.policy, source, &to);
                    self.reveal = Some(to);
                } else {
                    let folder = source.join(&value);
                    ops::mkdir(&folder)?;
                    self.reveal = Some(folder);
                }
                self.dialog = None;
                self.load(shared);
            }
            "goto" => {
                if value.contains("://") && !value.starts_with("file://") {
                    self.dialog = Some(Dialog::Text {
                        action: "connect".into(),
                        title: "files.menu.connect".into(),
                        value,
                        paths: vec![],
                    });
                    return self.submit(shared);
                }
                let base = self.tab().location.path().unwrap_or(&self.policy.home);
                let expanded = super::address::folder(&value, base, &self.policy.home)?;
                let real = self.policy.read(&expanded)?;
                if !real.is_dir() {
                    return Err("fs.notFile".into());
                }
                self.dialog = None;
                self.navigate(Location::Directory(real), shared, true);
            }
            "connect" => {
                let address = super::network::parse(&value)?;

                if ["http", "https"].contains(&address.scheme.as_str()) {
                    let mime = format!("x-scheme-handler/{}", address.scheme);
                    let app = shared
                        .apps
                        .iter()
                        .find(|a| {
                            a["kind"] == "linux"
                                && a["mimeTypes"].as_array().is_some_and(|types| {
                                    types.iter().any(|v| v.as_str() == Some(mime.as_str()))
                                })
                        })
                        .ok_or("files.networkUnavailable")?;
                    let id = app["desktopId"]
                        .as_str()
                        .unwrap_or("")
                        .trim_end_matches(".desktop");
                    self.effects
                        .push(json!({"kind":"openUri","uri":address.uri,"app":id}));
                    self.close_dialog();
                    return Ok(());
                }
                if self.busy {
                    return Err("fs.tooManyCopies".into());
                }
                self.close_dialog();
                self.busy = true;
                self.network_epoch += 1;
                let epoch = self.network_epoch;
                let tx = self.tx.clone();
                let result = work(move || {
                    let prompts = tx.clone();
                    let result = super::network::connect(address, move |prompt| {
                        prompts.send(Notice::NetworkPrompt(epoch, prompt)).is_ok()
                    });
                    let _ = tx.send(Notice::Connected(epoch, result));
                });
                if let Err(e) = result {
                    self.busy = false;
                    return Err(e);
                }
                let mut history = shared.preferences["servers"]["history"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                history.retain(|v| v.as_str() != Some(&value));
                history.insert(0, json!(value));
                history.truncate(12);
                shared.preferences["servers"]["history"] = json!(history);
                shared.save(&self.policy)?;
            }
            _ => (),
        }
        Ok(())
    }
    fn preview(&mut self) -> Result<()> {
        let Some(entry) = self.selected().first().cloned() else {
            return Ok(());
        };
        if let Location::Archive { file, .. } = &self.tab().location {
            let file = file.clone();
            self.close_dialog();
            self.effects.push(json!({"kind":"archivePreview","path":file,"entry":entry.raw,"generation":self.dialog_epoch}));
            return Ok(());
        }
        self.preview_entry(entry)
    }
    fn preview_entry(&mut self, entry: Entry) -> Result<()> {
        self.context = None;
        self.close_dialog();
        let kind = kind(&entry.name);
        self.dialog = Some(Dialog::Preview {
            entry: entry.clone(),
            text: None,
            image: None,
            media: None,
        });
        if matches!(kind, "text" | "other") {
            let path = entry.raw.clone();
            let tx = self.tx.clone();
            let generation = self.dialog_epoch;
            work(move || {
                use std::io::Read;
                let r = (|| -> Result<String> {
                    let real = policy::current().read(&path)?;
                    let (parent, name) = policy::anchored(&real).map_err(io_error)?;
                    use std::os::unix::ffi::OsStrExt;
                    let file = policy::open_at(
                        &parent,
                        std::ffi::OsStr::from_bytes(name.as_bytes()),
                        libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW,
                        0,
                    )
                    .map_err(io_error)?;
                    if !file.metadata().map_err(io_error)?.is_file() {
                        return Err("fs.notFile".into());
                    }
                    let mut bytes = Vec::new();
                    file.take(64 * 1024)
                        .read_to_end(&mut bytes)
                        .map_err(io_error)?;
                    if bytes.contains(&0) {
                        return Ok(String::new());
                    }
                    Ok(String::from_utf8_lossy(&bytes).into())
                })();
                let _ = tx.send(Notice::Text(generation, r));
            })?;
        } else if kind == "pdf" || kind == "font" {
            self.effects.push(json!({"kind":"documentPreview","path":entry.raw,"format":kind,"generation":self.dialog_epoch}));
        } else if kind == "audio" || kind == "video" {
            self.effects.push(
                json!({"kind":"mediaPreview","path":entry.raw,"generation":self.dialog_epoch}),
            );
        }
        Ok(())
    }
    fn service(&mut self, event: &Value, shared: &Shared) -> Result<()> {
        let kind = event["kind"].as_str().unwrap_or("");
        if kind == "pdfThumbnail" {
            self.pdf_thumbnails.complete(event, &mut self.effects);
            return Ok(());
        }
        if kind == "diskMount" || kind == "diskRelease" {
            self.volume_busy = None;
            self.policy = policy::mount_roots(shared.volumes.iter().filter_map(|v| v.path.clone()));
            if event["error"].is_string() {
                return Err(event["error"].as_str().unwrap().into());
            }
            if kind == "diskMount" {
                let path = PathBuf::from(event["result"].as_str().ok_or("sys.badArgument")?.trim());
                let mut paths: Vec<_> = shared
                    .volumes
                    .iter()
                    .filter_map(|v| v.path.clone())
                    .collect();
                paths.push(path.clone());
                self.policy = policy::mount_roots(paths.into_iter());
                let real = self.policy.read(&path)?;
                self.navigate(Location::Directory(real), shared, true);
            }
            return Ok(());
        }
        if ["documentPreview", "mediaPreview", "archivePreview"].contains(&kind)
            && event["generation"].as_u64() != Some(self.dialog_epoch)
        {
            if event["result"].is_object() {
                self.effects
                    .push(json!({"kind":"closePreview","image":event["result"]}));
            } else if kind == "mediaPreview" && event["result"].is_u64() {
                self.effects
                    .push(json!({"kind":"closePreview","media":event["result"]}));
            }
            return Ok(());
        }
        if ["archiveList", "networkScan"].contains(&kind)
            && event["generation"].as_u64() != Some(self.generation.load(Ordering::Relaxed))
        {
            return Ok(());
        }
        if event["error"].is_string() {
            self.loading = false;
            return Err(event["error"].as_str().unwrap().into());
        }
        match kind {
            "networkScan" => {
                if self.tab().location != Location::Network
                    || event["generation"].as_u64() != Some(self.generation.load(Ordering::Relaxed))
                {
                    return Ok(());
                }
                self.network_status = event["result"].clone();
                self.discovered = super::network::entries(&event["result"]);
                self.tab_mut()
                    .entries
                    .retain(|e| !e.path.starts_with("net:"));
                let discovered = self.discovered.clone();
                self.tab_mut().entries.extend(discovered);
                self.sort();
            }
            "archiveList" => {
                if event["generation"].as_u64() == Some(self.generation.load(Ordering::Relaxed)) {
                    self.tab_mut().archive = event["result"]["entries"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    self.archive_entries();
                    self.loading = false;
                }
            }
            "archivePreview" => {
                let path = Path::new(event["path"].as_str().ok_or("sys.badArgument")?);
                let real = self.policy.read(path)?;
                self.preview_entry(ops::entry(&real).map_err(io_error)?)?;
            }
            "archiveOpen" => {
                if let Some(path) = event["path"].as_str() {
                    self.effects.push(json!({"kind":"openPath","path":path}));
                }
            }
            "documentPreview" => {
                if event["generation"].as_u64() != Some(self.dialog_epoch) {
                    self.effects
                        .push(json!({"kind":"closePreview","image":event["result"]}));
                    return Ok(());
                }
                if let Some(Dialog::Preview { image, .. }) = &mut self.dialog {
                    *image = Some(event["result"].clone());
                }
            }
            "mediaPreview" => {
                if event["generation"].as_u64() != Some(self.dialog_epoch) {
                    self.effects
                        .push(json!({"kind":"closePreview","media":event["result"]}));
                    return Ok(());
                }
                if let Some(Dialog::Preview { media, .. }) = &mut self.dialog {
                    *media = event["result"].as_u64();
                }
            }
            "connect" => {
                self.navigate(Location::Network, shared, true);
            }
            _ => self.load(shared),
        }
        Ok(())
    }
    fn key(&mut self, e: &Value, shared: &mut Shared) -> Result<()> {
        let key = e["key"].as_str().unwrap_or("");
        let lower = key.to_lowercase();
        let modifier = e["ctrl"].as_bool().unwrap_or(false) || e["meta"].as_bool().unwrap_or(false);
        let shift = e["shift"].as_bool().unwrap_or(false);
        if modifier && self.dialog.is_none() {
            match key {
                "0" => {
                    self.zoom_wheel = 0.;
                    self.set_zoom(1., shared);
                    return Ok(());
                }
                "+" | "=" => {
                    self.zoom_step(1., shared);
                    return Ok(());
                }
                "-" => {
                    self.zoom_step(-1., shared);
                    return Ok(());
                }
                _ => (),
            }
        }
        let action = if key == "Escape" {
            if self.dialog.is_some() {
                Some("cancelDialog")
            } else if self.context.is_some() {
                Some("closeContext")
            } else if self.mode != Mode::Browser {
                Some("cancel")
            } else {
                Some("clear")
            }
        } else if self.dialog.is_some() {
            if key == "Enter" {
                Some(if matches!(self.dialog, Some(Dialog::Confirm { .. })) {
                    "confirm"
                } else {
                    "submit"
                })
            } else {
                None
            }
        } else if modifier && ["1", "2", "3", "4"].contains(&lower.as_str()) {
            let mode = match lower.as_str() {
                "2" => "list",
                "3" => "columns",
                "4" => "gallery",
                _ => "icons",
            };
            self.event(&json!({"action":["view",mode]}), shared);
            return Ok(());
        } else if modifier && lower == "tab" && self.mode == Mode::Browser {
            let next = if shift {
                (self.active + self.tabs.len() - 1) % self.tabs.len()
            } else {
                (self.active + 1) % self.tabs.len()
            };
            self.event(&json!({"action":["tab",next]}), shared);
            return Ok(());
        } else if modifier {
            match lower.as_str() {
                "a" => Some("selectAll"),
                "c" => Some("copy"),
                "x" => Some("cut"),
                "v" => Some("paste"),
                "t" => Some("newTab"),
                "w" => Some("closeTab"),
                "n" => Some(if shift { "newFolder" } else { "newWindow" }),
                "i" => Some("info"),
                "d" => Some("duplicate"),
                "g" if shift => Some("goTo"),
                "k" => Some("connect"),
                "." => Some("hidden"),
                "r" => Some("refresh"),
                "Backspace" | "backspace" => Some("trash"),
                _ => None,
            }
        } else {
            match key {
                "Enter" => Some(if self.mode == Mode::Browser {
                    "open"
                } else {
                    "choose"
                }),
                "Backspace" => Some("back"),
                "F2" => Some("rename"),
                "F5" => Some("refresh"),
                "Delete" => Some(if self.tab().location == Location::Trash {
                    "delete"
                } else {
                    "trash"
                }),
                " " => Some("preview"),
                _ => None,
            }
        };
        if let Some(action) = action {
            self.event(&json!({"action":[action,self.active]}), shared);
            return Ok(());
        }
        if e["alt"] == true && (key == "ArrowLeft" || key == "ArrowRight" || key == "ArrowUp") {
            self.event(
                &json!({"action":[match key{"ArrowLeft"=>"back","ArrowRight"=>"forward",_=>"up"}]}),
                shared,
            );
            return Ok(());
        }
        if [
            "ArrowDown",
            "ArrowUp",
            "ArrowLeft",
            "ArrowRight",
            "Home",
            "End",
            "PageDown",
            "PageUp",
        ]
        .contains(&key)
        {
            let items = self.filtered();
            if items.is_empty() {
                return Ok(());
            }
            let (cols, _, row) = self.entry_grid();
            let height = self.entry_area();
            let at = if self.tab().selected.is_empty() {
                None
            } else {
                self.tab()
                    .cursor
                    .and_then(|c| items.iter().position(|i| *i == c))
                    .or_else(|| items.iter().position(|i| self.tab().selected.contains(i)))
            };
            let previous = at.unwrap_or(0);
            let page = (height / row).floor().max(1.) as usize * cols;
            let next = if key == "End" {
                items.len() - 1
            } else if key == "Home" {
                0
            } else if at.is_none() {
                0
            } else {
                match key {
                    "ArrowDown" => (previous + cols).min(items.len() - 1),
                    "ArrowUp" => previous.saturating_sub(cols),
                    "ArrowRight" => (previous + 1).min(items.len() - 1),
                    "ArrowLeft" => previous.saturating_sub(1),
                    "End" => items.len() - 1,
                    "Home" => 0,
                    "PageDown" => (previous + page).min(items.len() - 1),
                    _ => previous.saturating_sub(page),
                }
            };
            self.event(
                &json!({"action":["select",items[next]],"shift":shift}),
                shared,
            );
            let top = (next / cols) as f64 * row;
            let offset = self.tab().offset;
            if top < offset {
                self.tab_mut().offset = top;
            } else if top + row > offset + height {
                self.tab_mut().offset = (top + row - height).max(0.);
            }
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        for id in &self.owned_jobs {
            ops::detach(*id);
        }
    }
}

pub fn kind(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "avif" | "tiff" | "svg" => "image",
        "mp4" | "mkv" | "webm" | "mov" | "avi" => "video",
        "mp3" | "ogg" | "flac" | "wav" | "m4a" | "opus" => "audio",
        "pdf" => "pdf",
        "ttf" | "otf" | "woff" => "font",
        "txt" | "md" | "rs" | "c" | "h" | "js" | "jsx" | "ts" | "tsx" | "py" | "json" | "toml"
        | "yaml" | "yml" | "xml" | "html" | "css" | "sh" | "csv" | "log" => "text",
        _ => "other",
    }
}
pub fn archive(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        ".zip", ".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.bz2", ".7z", ".rar", ".gz",
        ".xz", ".bz2", ".zst", ".iso", ".cpio",
    ]
    .iter()
    .any(|e| n.ends_with(e))
}
pub fn free_name(parent: &Path, name: &str) -> PathBuf {
    let stem = Path::new(name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..10000 {
        let candidate = parent.join(if n == 1 {
            name.to_string()
        } else {
            format!("{stem} {n}{ext}")
        });
        if fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-{}{ext}", std::process::id()))
}
fn writable_dir(policy: &Policy, path: &Path) -> Option<PathBuf> {
    let p = policy
        .target(&path.join(".axia-permission-check"), false)
        .ok()?;
    let parent = p.parent()?;
    let c = policy::c_name(parent.as_os_str()).ok()?;
    (unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0).then(|| parent.into())
}
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    natural_lowercase(a.to_lowercase().as_bytes(), b.to_lowercase().as_bytes())
}
// Both already lowercase. UTF-8 sorts bytewise in character order, and no byte
// of a multi-byte character is an ASCII digit.
fn natural_lowercase(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let run = |s: &[u8]| s.iter().take_while(|c| c.is_ascii_digit()).count();
    let (mut a, mut b) = (a, b);
    loop {
        match (a.first(), b.first()) {
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x, rest_a) = a.split_at(run(a));
                let (y, rest_b) = b.split_at(run(b));
                let strip = |s: &'_ [u8]| s.iter().take_while(|c| **c == b'0').count();
                let (x, y) = (&x[strip(x)..], &y[strip(y)..]);
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
                (a, b) = (rest_a, rest_b);
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(y);
                }
                (a, b) = (&a[1..], &b[1..]);
            }
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
        }
    }
}

// Lowercased once per file: a sort compares each name about 15 times.
struct SortKey {
    not_folder: bool,
    number: i128,
    kind: &'static str,
    name: String,
    descending: bool,
}
impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let order = self
            .number
            .cmp(&other.number)
            .then_with(|| self.kind.cmp(other.kind))
            .then_with(|| natural_lowercase(self.name.as_bytes(), other.name.as_bytes()));
        self.not_folder
            .cmp(&other.not_folder)
            .then(if self.descending {
                order.reverse()
            } else {
                order
            })
    }
}
impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for SortKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for SortKey {}

pub(super) fn sort_entries(
    entries: &mut [Entry],
    key: &str,
    descending: bool,
    folders_first: bool,
) {
    entries.sort_by_cached_key(|e| SortKey {
        not_folder: folders_first && !e.dir,
        number: match key {
            "size" => e.size as i128,
            "date" => e.mtime as i128,
            _ => 0,
        },
        kind: if key == "kind" { kind(&e.name) } else { "" },
        name: e.name.to_lowercase(),
        descending,
    });
}

#[cfg(test)]
mod regression {
    use super::super::tests::Home;
    use super::*;
    #[test]
    fn stale_directory_results_cannot_replace_the_current_listing() {
        let h = Home::new();
        h.file("keep.txt", "keep");
        let (mut s, shared) = h.session(json!({}));
        let old = s.generation.load(Ordering::Relaxed).saturating_sub(1);
        s.tx.send(Notice::Scan(old, Err("sys.badArgument".into())))
            .unwrap();
        s.poll(&json!({}), &shared);
        assert!(s.error.is_none());
        assert!(s.tab().entries.iter().any(|e| e.name == "keep.txt"));
    }
    #[test]
    fn stale_preview_resources_are_released_and_errors_ignored() {
        let h = Home::new();
        let (mut s, mut shared) = h.session(json!({}));
        s.dialog_epoch = 7;
        s.event(&json!({"action":["service"],"kind":"documentPreview","generation":6,"result":{"id":123}}),&mut shared);
        assert!(s
            .effects
            .iter()
            .any(|e| e["kind"] == "closePreview" && e["image"]["id"] == 123));
        s.event(&json!({"action":["service"],"kind":"documentPreview","generation":6,"error":"old error"}),&mut shared);
        assert!(s.error.is_none());
    }
    #[test]
    fn changed_dialog_ignores_the_previous_text_read() {
        let h = Home::new();
        let (mut s, shared) = h.session(json!({}));
        s.dialog_epoch = 9;
        s.tx.send(Notice::Text(8, Err("old error".into()))).unwrap();
        s.poll(&json!({}), &shared);
        assert!(s.error.is_none());
    }
    #[test]
    fn unsafe_archive_paths_are_not_browsable() {
        let h = Home::new();
        let (mut s, _) = h.session(json!({}));
        s.tab_mut().location = Location::Archive {
            file: h.path("archive.zip"),
            inner: String::new(),
        };
        s.tab_mut().archive = vec![
            json!({"p":"../../outside","bad":true}),
            json!({"p":"/etc/passwd"}),
            json!({"p":"safe.txt","s":10}),
        ];
        s.archive_entries();
        assert_eq!(s.tab().entries.len(), 1);
        assert_eq!(s.tab().entries[0].name, "safe.txt");
    }
    #[test]
    fn archive_preview_requests_extraction_before_reading() {
        let h = Home::new();
        let (mut s, _) = h.session(json!({}));
        s.tab_mut().location = Location::Archive {
            file: h.path("archive.zip"),
            inner: String::new(),
        };
        s.tab_mut().archive = vec![json!({"p":"note.txt","s":10})];
        s.archive_entries();
        s.tab_mut().selected.insert(0);
        s.preview().unwrap();
        assert_eq!(s.effects.last().unwrap()["kind"], "archivePreview");
        assert!(s.dialog.is_none());
    }
}
