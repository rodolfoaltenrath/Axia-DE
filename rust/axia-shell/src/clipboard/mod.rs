//! Session history model and worker. No UI, Wayland protocol or renderer ownership.
mod store;
#[cfg(test)]
mod tests;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    ffi::{c_char, CStr, CString},
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const ITEM_BYTES: usize = 1024 * 1024;
const TEXT_BYTES: usize = 16 * 1024 * 1024;
const IMAGE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ITEMS: usize = 8192;
const QUEUE_BYTES: usize = 8 * 1024 * 1024;
const QUEUE_ITEMS: usize = 128;
static SERIAL: AtomicU64 = AtomicU64::new(1);
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.
}
fn next_id() -> String {
    format!(
        "{:x}-{:x}",
        now() as u64,
        SERIAL.fetch_add(1, Ordering::Relaxed)
    )
}
fn bounded(text: &str, limit: usize) -> String {
    let mut n = text.len().min(limit);
    while !text.is_char_boundary(n) {
        n -= 1
    }
    text[..n].into()
}
pub fn classify(text: &str, image: bool) -> &'static str {
    if image {
        return "image";
    }
    let s = text.trim();
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    let p = PATTERNS.get_or_init(|| {
        [
            r"(?i)^(?:(?:https?|ftp)://\S+|www\.\S+\.\S+|mailto:\S+)$",
            r"(?i)^(?:#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})|(?:rgba?|hsla?)\([^)]*\))$",
            r"^(?:~?/|file://)[^\n]*$",
            r"(?m)[{};]\s*$|^\s*(?:function|def|class|import|export|const|let|var|if|for|while|return|#include|SELECT|fn|func|package|public|private)\b|=>|\)\s*\{",
            r"\s{2,}",
        ].iter().map(|s| regex::Regex::new(s).unwrap()).collect()
    });
    if p[0].is_match(s) {
        "link"
    } else if p[1].is_match(s) {
        "color"
    } else if p[2].is_match(s) && !p[4].is_match(s) {
        "file"
    } else if (s.contains('\n') && p[3].is_match(s))
        || (s.lines().count() > 2
            && s.lines()
                .filter(|l| {
                    l.chars().take_while(|c| c.is_whitespace()).count() >= 2 || l.contains('\t')
                })
                .count()
                >= 2)
    {
        "code"
    } else {
        "text"
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Entry {
    pub id: String,
    pub kind: String,
    #[serde(rename = "type", default)]
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub image: Option<String>,
    #[serde(default)]
    pub origin: String,
    pub time: f64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub src: Option<String>,
    #[serde(skip)]
    revision: u64,
    #[serde(skip)]
    summary: String,
    #[serde(skip)]
    characters: usize,
}
impl Entry {
    fn prepare(&mut self) {
        let text = self.text.as_deref().unwrap_or("");
        self.category = classify(text, self.image.is_some()).into();
        self.characters = text.chars().count();
        self.summary.clear();
        let mut whitespace = false;
        let mut width = 0;
        for mut c in text.chars() {
            if c.is_whitespace() {
                if whitespace {
                    continue;
                }
                whitespace = true;
                c = ' ';
            } else {
                whitespace = false;
            }
            if width + c.len_utf16() > 160 {
                break;
            }
            width += c.len_utf16();
            self.summary.push(c);
        }
    }
}
#[derive(Clone, Debug)]
struct Config {
    save: bool,
    max: usize,
    max_mb: u64,
    days: u64,
    ignore: Vec<String>,
    clear_logout: bool,
}
impl Config {
    fn parse(v: &Value) -> Self {
        Self {
            save: v["save"] != false,
            max: v["max"].as_u64().unwrap_or(500).clamp(1, 5000) as usize,
            max_mb: v["maxMB"].as_u64().unwrap_or(200).clamp(1, 4096),
            days: v["expireDays"].as_u64().unwrap_or(30).min(36500),
            ignore: v["ignoreApps"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .take(256)
                        .filter_map(|s| s.as_str().map(|s| bounded(s, 128)))
                        .collect()
                })
                .unwrap_or_default(),
            clear_logout: v["clearOnLogout"] == true,
        }
    }
}
struct Model {
    items: Vec<Entry>,
    version: u64,
    view: Option<Value>,
}
impl Model {
    fn new(items: Vec<Entry>) -> Self {
        let mut m = Self {
            items,
            version: 0,
            view: None,
        };
        for item in &mut m.items {
            item.prepare();
        }
        m.bump_all();
        m
    }
    fn bump_all(&mut self) {
        self.version += 1;
        for e in &mut self.items {
            e.revision = self.version
        }
    }
    fn prune(&mut self, c: &Config, now: f64) -> Vec<String> {
        let mut removed = Vec::new();
        self.items.retain(|e| {
            let keep = e.pinned || c.days == 0 || now - e.time <= c.days as f64 * 86400000.;
            if !keep {
                if let Some(p) = &e.image {
                    removed.push(p.clone())
                }
            }
            keep
        });
        let mut text: usize = self
            .items
            .iter()
            .map(|e| e.text.as_ref().map_or(0, String::len))
            .sum();
        let mut bytes: u64 = self.items.iter().map(|e| e.bytes).sum();
        let mut free = self.items.iter().filter(|e| !e.pinned).count();
        let mut count = self.items.len();
        for i in (0..self.items.len()).rev() {
            if !self.items[i].pinned
                && (free > c.max
                    || text > TEXT_BYTES
                    || bytes > c.max_mb * 1048576
                    || count > MAX_ITEMS)
            {
                let e = self.items.remove(i);
                free -= 1;
                count -= 1;
                text -= e.text.as_ref().map_or(0, String::len);
                bytes = bytes.saturating_sub(e.bytes);
                if let Some(p) = e.image {
                    removed.push(p)
                }
            }
        }
        removed
    }
    fn insert(&mut self, mut item: Entry) {
        item.prepare();
        if let Some(i) = self
            .items
            .iter()
            .position(|e| match (&item.text, &item.src) {
                (Some(t), _) => e.text.as_ref() == Some(t),
                (_, Some(src)) => e.image.is_some() && e.src.as_ref() == Some(src),
                _ => false,
            })
        {
            let old = self.items.remove(i);
            item.id = old.id;
            item.pinned = old.pinned;
        }
        self.version += 1;
        item.revision = self.version;
        self.items.insert(0, item);
    }
    fn act(&mut self, op: &str, id: &str, keep: bool) -> Vec<String> {
        let mut removed = Vec::new();
        if op == "clear" {
            self.items.retain(|e| {
                if keep && e.pinned {
                    return true;
                }
                if let Some(p) = &e.image {
                    removed.push(p.clone())
                }
                false
            });
            self.version += 1;
            return removed;
        }
        if let Some(i) = self.items.iter().position(|e| e.id == id) {
            self.version += 1;
            match op {
                "remove" => {
                    if let Some(p) = self.items.remove(i).image {
                        removed.push(p)
                    }
                }
                "pin" => {
                    self.items[i].pinned = !self.items[i].pinned;
                    self.items[i].revision = self.version;
                }
                "use" => {
                    let mut e = self.items.remove(i);
                    e.time = now();
                    e.revision = self.version;
                    self.items.insert(0, e);
                }
                _ => (),
            }
        }
        removed
    }
    fn query(
        &mut self,
        query: &str,
        filter: &str,
        limit: usize,
        generation: u64,
        latest: &AtomicU64,
    ) -> bool {
        let folded = query.to_lowercase();
        let mut pinned = Vec::new();
        let mut ordinary = Vec::new();
        let mut counts = HashMap::<String, usize>::new();
        counts.insert("all".into(), self.items.len());
        for item in &self.items {
            if latest.load(Ordering::Relaxed) != generation {
                return false;
            }
            *counts.entry(item.category.clone()).or_default() += 1;
            if item.pinned {
                *counts.entry("pinned".into()).or_default() += 1;
            }
            if filter != "all"
                && if filter == "pinned" {
                    !item.pinned
                } else {
                    item.category != filter
                }
            {
                continue;
            }
            if !folded.is_empty()
                && !item
                    .text
                    .as_ref()
                    .is_some_and(|t| t.to_lowercase().contains(&folded))
                && !item.origin.to_lowercase().contains(&folded)
            {
                continue;
            }
            if item.pinned {
                pinned.push(item.id.clone())
            } else {
                ordinary.push(item.id.clone())
            }
        }
        pinned.extend(ordinary);
        pinned.truncate(limit.min(MAX_ITEMS));
        self.view = Some(
            json!({"generation":generation,"query":query,"filter":filter,"limit":limit,"ids":pinned,"counts":counts}),
        );
        true
    }
    fn packet(&self, baseline: &HashMap<String, u64>, error: &Option<String>) -> String {
        let upsert: Vec<_> = self
            .items
            .iter()
            .filter(|e| baseline.get(&e.id) != Some(&e.revision))
            .map(|e| {
                // IDs have immutable text content. Pin/use/dedup updates only
                // metadata; do not copy megabytes through QuickJS again.
                let mut value=json!({"id":e.id,"kind":e.kind,"type":e.category,"origin":e.origin,"time":e.time,"pinned":e.pinned,"bytes":e.bytes,"preview":e.summary,"characters":e.characters});
                if let Some(image)=&e.image {value["image"]=json!(image);}
                if let Some(src)=&e.src {value["src"]=json!(src);}
                if !baseline.contains_key(&e.id) {if let Some(text)=&e.text {value["text"]=json!(text);}}
                value
            })
            .collect();
        json!({"version":self.version,"upsert":upsert,"order":self.items.iter().map(|e|&e.id).collect::<Vec<_>>(),"ready":true,"error":error,"view":self.view}).to_string()
    }
}
enum Command {
    Configure(Config, bool),
    View(String, String, usize, u64),
    Text(String, String),
    Image(File, String, String),
    Act(String, String, bool),
    Flush(bool, mpsc::Sender<bool>),
    #[cfg(test)]
    Barrier(mpsc::Sender<()>, mpsc::Receiver<()>),
}
impl Command {
    fn bytes(&self) -> usize {
        match self {
            Self::Text(t, _) => t.len(),
            _ => 0,
        }
    }
    fn capture(&self) -> bool {
        matches!(self, Self::Text(..) | Self::Image(..))
    }
}
struct Packet {
    json: String,
    versions: HashMap<String, u64>,
}
#[derive(Default)]
struct Shared {
    queue: VecDeque<Command>,
    bytes: usize,
    packet: Option<Packet>,
    baseline: HashMap<String, u64>,
    stop: bool,
}
struct State {
    shared: Mutex<Shared>,
    wake: Condvar,
    event: OwnedFd,
    ready: AtomicBool,
    view_generation: AtomicU64,
}
struct Service {
    state: Arc<State>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Service {
    fn new(home: PathBuf, runtime: Option<PathBuf>, config: Config) -> io::Result<Self> {
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let state = Arc::new(State {
            shared: Mutex::new(Shared::default()),
            wake: Condvar::new(),
            event: unsafe { OwnedFd::from_raw_fd(fd) },
            ready: AtomicBool::new(false),
            view_generation: AtomicU64::new(0),
        });
        let s = state.clone();
        let worker = thread::Builder::new()
            .name("axia-clipboard".into())
            .spawn(move || run(s, store::Store::new(&home, runtime.as_deref()), config))?;
        Ok(Self {
            state,
            thread: Some(worker),
        })
    }
    fn enqueue(&self, c: Command) -> bool {
        let mut s = self.state.shared.lock().unwrap();
        if let Command::View(_, _, _, generation) = &c {
            self.state
                .view_generation
                .store(*generation, Ordering::Relaxed);
            s.queue.retain(|old| !matches!(old, Command::View(..)));
        }
        let size = c.bytes();
        while s.queue.len() >= QUEUE_ITEMS || s.bytes + size > QUEUE_BYTES {
            let Some(i) = s.queue.iter().position(Command::capture) else {
                return false;
            };
            let old = s.queue.remove(i).unwrap();
            s.bytes -= old.bytes();
        }
        s.bytes += size;
        s.queue.push_back(c);
        self.state.wake.notify_one();
        true
    }
    fn take(&self) -> Option<String> {
        if !self.state.ready.load(Ordering::Acquire) {
            return None;
        }
        let mut s = self.state.shared.lock().unwrap();
        let mut value = 0u64;
        unsafe {
            libc::read(
                self.state.event.as_raw_fd(),
                &mut value as *mut _ as *mut _,
                8,
            );
        }
        let p = s.packet.take();
        self.state.ready.store(false, Ordering::Release);
        let p = p?;
        s.baseline = p.versions;
        Some(p.json)
    }
    fn flush(&self, logout: bool) -> bool {
        let (tx, rx) = mpsc::channel();
        self.enqueue(Command::Flush(logout, tx))
            && rx.recv_timeout(Duration::from_secs(2)).unwrap_or(false)
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let mut s = self.state.shared.lock().unwrap();
        s.stop = true;
        self.state.wake.notify_one();
        drop(s);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn publish(state: &State, m: &Model, error: &Option<String>) {
    let baseline = state.shared.lock().unwrap().baseline.clone();
    let packet = Packet {
        json: m.packet(&baseline, error),
        versions: m.items.iter().map(|e| (e.id.clone(), e.revision)).collect(),
    };
    let mut shared = state.shared.lock().unwrap();
    shared.packet = Some(packet);
    state.ready.store(true, Ordering::Release);
    let one = 1u64;
    unsafe {
        libc::write(state.event.as_raw_fd(), &one as *const _ as *const _, 8);
    }
}
fn run(state: Arc<State>, mut store: store::Store, mut config: Config) {
    let mut error = None;
    let items = if config.save {
        match store.load() {
            Ok(items) => items,
            Err(_) => {
                error =
                    Some("O histórico salvo foi preservado, mas não pôde ser carregado.".into());
                vec![]
            }
        }
    } else {
        vec![]
    };
    let mut model = Model::new(items);
    for p in model.prune(&config, now()) {
        store.remove(&p)
    }
    if !config.save {
        let _ = store.save(&[]);
    }
    publish(&state, &model, &error);
    let mut due: Option<Instant> = None;
    loop {
        let command = {
            let mut s = state.shared.lock().unwrap();
            loop {
                if s.stop {
                    return;
                }
                if let Some(c) = s.queue.pop_front() {
                    s.bytes -= c.bytes();
                    break Some(c);
                }
                if let Some(at) = due {
                    let wait = at.saturating_duration_since(Instant::now());
                    if wait.is_zero() {
                        break None;
                    }
                    let (next, _) = state.wake.wait_timeout(s, wait).unwrap();
                    s = next;
                } else {
                    s = state.wake.wait(s).unwrap();
                }
            }
        };
        let Some(command) = command else {
            due = None;
            if store
                .save(if config.save { &model.items } else { &[] })
                .is_err()
            {
                error =
                    Some("Não foi possível salvar o histórico da área de transferência.".into());
                publish(&state, &model, &error);
            }
            continue;
        };
        let before = model.version;
        let mut cleanup = Vec::new();
        let mut view_changed = false;
        match command {
            Command::View(query, filter, limit, generation) => {
                view_changed =
                    model.query(&query, &filter, limit, generation, &state.view_generation);
            }
            Command::Configure(c, resync) => {
                if resync {
                    let mut shared = state.shared.lock().unwrap();
                    shared.baseline.clear();
                    shared.packet = None;
                    model.view = None;
                    state.ready.store(false, Ordering::Release);
                    let mut event = 0u64;
                    unsafe {
                        libc::read(state.event.as_raw_fd(), &mut event as *mut _ as *mut _, 8);
                    }
                }
                if config.save != c.save {
                    if c.save {
                        for e in &mut model.items {
                            store.persistent(e)
                        }
                    } else {
                        for e in &mut model.items {
                            store.transient(e)
                        }
                        store.writable = true;
                        let _ = store.save(&[]);
                    }
                    model.bump_all();
                }
                config = c;
                cleanup.extend(model.prune(&config, now()));
                model.version += 1;
            }
            Command::Text(text, origin) => {
                if !text.is_empty() && !config.ignore.contains(&origin) {
                    model.insert(Entry {
                        id: next_id(),
                        kind: "text".into(),
                        category: String::new(),
                        text: Some(text),
                        image: None,
                        origin,
                        time: now(),
                        pinned: false,
                        bytes: 0,
                        src: None,
                        revision: 0,
                        summary: String::new(),
                        characters: 0,
                    });
                }
            }
            Command::Image(file, origin, source) => {
                if !config.ignore.contains(&origin) {
                    match store.image(file, config.save) {
                        Ok((path, bytes)) => {
                            if let Some(old) = model
                                .items
                                .iter()
                                .find(|e| e.src.as_deref() == Some(&source) && e.image.is_some())
                                .and_then(|e| e.image.clone())
                            {
                                cleanup.push(old);
                            }
                            model.insert(Entry {
                                id: next_id(),
                                kind: "image".into(),
                                category: "image".into(),
                                text: None,
                                image: Some(path),
                                origin,
                                time: now(),
                                pinned: false,
                                bytes,
                                src: Some(source),
                                revision: 0,
                                summary: String::new(),
                                characters: 0,
                            });
                        }
                        Err(_) => {
                            error = Some(
                                "Não foi possível guardar a imagem copiada no histórico.".into(),
                            );
                            publish(&state, &model, &error);
                        }
                    }
                }
            }
            Command::Act(op, id, keep) => {
                if op == "clear" {
                    store.writable = true;
                }
                cleanup.extend(model.act(&op, &id, keep));
            }
            #[cfg(test)]
            Command::Barrier(entered, release) => {
                let _ = entered.send(());
                let _ = release.recv();
            }
            Command::Flush(logout, ack) => {
                if logout && config.clear_logout {
                    cleanup.extend(model.act("clear", "", true));
                }
                for p in cleanup {
                    store.remove(&p)
                }
                let ok = store
                    .save(if config.save { &model.items } else { &[] })
                    .is_ok();
                due = None;
                publish(&state, &model, &error);
                let _ = ack.send(ok);
                continue;
            }
        }
        if model.version != before {
            cleanup.extend(model.prune(&config, now()));
            for p in cleanup {
                if !model.items.iter().any(|e| e.image.as_deref() == Some(&p)) {
                    store.remove(&p)
                }
            }
            due = Some(Instant::now() + Duration::from_millis(600));
        }
        if model.version != before || view_changed {
            // Intermediate copies/config changes are superseded while a batch
            // is queued. Serialize only the latest projection, not N histories.
            if state.shared.lock().unwrap().queue.is_empty() {
                publish(&state, &model, &error);
            }
        }
    }
}
static SERVICE: OnceLock<Service> = OnceLock::new();
fn configured(config: Config, resync: bool) -> Option<&'static Service> {
    if let Some(s) = SERVICE.get() {
        s.enqueue(Command::Configure(config, resync));
        return Some(s);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    let service = Service::new(home, runtime, config).ok()?;
    let _ = SERVICE.set(service);
    SERVICE.get()
}
#[no_mangle]
pub extern "C" fn axia_clipboard_fd() -> i32 {
    SERVICE.get().map_or(-1, |s| s.state.event.as_raw_fd())
}
#[no_mangle]
pub extern "C" fn axia_clipboard_take() -> *mut c_char {
    SERVICE
        .get()
        .and_then(Service::take)
        .and_then(|s| CString::new(s).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}
#[no_mangle]
pub unsafe extern "C" fn axia_clipboard_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_clipboard_request(input: *const c_char) -> *mut c_char {
    let result = (|| {
        if input.is_null() {
            return json!({"accepted":false});
        }
        let b = CStr::from_ptr(input).to_bytes();
        if b.len() > 8 * 1024 * 1024 {
            return json!({"accepted":false});
        }
        let Ok(v) = serde_json::from_slice::<Value>(b) else {
            return json!({"accepted":false});
        };
        let op = v["op"].as_str().unwrap_or("");
        if op == "kind" {
            return json!({"kind":classify(v["text"].as_str().unwrap_or(""),v["image"]==true)});
        }
        if op == "configure" {
            return json!({"accepted":configured(Config::parse(&v["config"]),v["reset"]==true).is_some()});
        }
        let Some(s) = SERVICE.get() else {
            return json!({"accepted":false});
        };
        let ok = match op {
            "view" => s.enqueue(Command::View(
                bounded(v["query"].as_str().unwrap_or(""), 512),
                bounded(v["filter"].as_str().unwrap_or("all"), 16),
                v["limit"]
                    .as_u64()
                    .unwrap_or(MAX_ITEMS as u64)
                    .min(MAX_ITEMS as u64) as usize,
                v["generation"].as_u64().unwrap_or(0),
            )),
            "load" => true,
            "flush" => s.flush(v["logout"] == true),
            "remember" => s.enqueue(Command::Text(
                bounded(v["text"].as_str().unwrap_or(""), ITEM_BYTES),
                bounded(v["origin"].as_str().unwrap_or("lordos"), 128),
            )),
            "use" | "pin" | "remove" | "clear" => s.enqueue(Command::Act(
                op.into(),
                bounded(v["id"].as_str().unwrap_or(""), 128),
                v["keepPinned"] != false,
            )),
            _ => false,
        };
        json!({"accepted":ok})
    })();
    CString::new(result.to_string()).unwrap().into_raw()
}
// C owns source strings during this call. A nonnegative fd is transferred even
// when a password/ignored/overloaded capture is rejected; it is always closed.
#[no_mangle]
pub unsafe extern "C" fn axia_clipboard_capture(
    text: *const c_char,
    len: usize,
    origin: *const c_char,
    sensitive: bool,
    fd: i32,
    source: *const c_char,
) {
    let file = if fd >= 0 {
        Some(File::from_raw_fd(fd))
    } else {
        None
    };
    if sensitive {
        return;
    }
    let Some(s) = SERVICE.get() else { return };
    let origin = if origin.is_null() {
        "lordos".into()
    } else {
        bounded(&CStr::from_ptr(origin).to_string_lossy(), 128)
    };
    if let Some(file) = file {
        if !source.is_null() {
            s.enqueue(Command::Image(
                file,
                origin,
                bounded(&CStr::from_ptr(source).to_string_lossy(), 4096),
            ));
        }
    } else if !text.is_null() {
        let bytes = std::slice::from_raw_parts(text.cast::<u8>(), len.min(ITEM_BYTES));
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => std::borrow::Cow::Borrowed(text),
            Err(e) if e.error_len().is_none() => {
                std::borrow::Cow::Borrowed(std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap())
            }
            Err(_) => String::from_utf8_lossy(bytes),
        };
        s.enqueue(Command::Text(bounded(&text, ITEM_BYTES), origin));
    }
}
// Pure classification is also available in isolated apps; it never starts the
// session service or exposes clipboard history/storage.
#[no_mangle]
pub unsafe extern "C" fn axia_clipboard_kind(text: *const c_char, image: bool) -> *const c_char {
    let text = if text.is_null() {
        std::borrow::Cow::Borrowed("")
    } else {
        CStr::from_ptr(text).to_string_lossy()
    };
    match classify(&text, image) {
        "image" => c"image".as_ptr(),
        "link" => c"link".as_ptr(),
        "color" => c"color".as_ptr(),
        "file" => c"file".as_ptr(),
        "code" => c"code".as_ptr(),
        _ => c"text".as_ptr(),
    }
}
