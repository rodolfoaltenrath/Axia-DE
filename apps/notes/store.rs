//! Shared Rust session and the original Markdown/YAML storage format.
use super::persistence::{Event, Persistence, Save};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
};
pub const COLORS: [&str; 6] = ["yellow", "pink", "green", "blue", "purple", "gray"];
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Note {
    pub slug: String,
    pub body: String,
    pub title: String,
    pub color: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub desktop: bool,
    pub pinned: bool,
    pub created: String,
    pub updated: String,
    #[serde(skip)]
    pub preview: String,
    #[serde(skip)]
    pub body_revision: u64,
}
pub fn title(body: &str) -> String {
    let line = body
        .lines()
        .find(|l| !l.trim().is_empty() && !pattern!(r"^(`{3,}|---)").is_match(l.trim()))
        .unwrap_or("");
    let line =
        pattern!(r"^\s*(#{1,6}\s+|>\s*|[-*+]\s+(\[[ xX]\]\s+)?|\d+[.)]\s+)").replace(line, "");
    let line = pattern!(r"[*_`~]|!\[[^\]]*\]\([^)]*\)").replace_all(&line, "");
    line.trim().chars().take(60).collect()
}
pub fn preview(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !pattern!(r"^(`{3,}|---|#)").is_match(l))
        .unwrap_or("");
    let line = pattern!(r"^(>\s*|[-*+]\s+(\[[ xX]\]\s+)?|\d+[.)]\s+)").replace(line, "");
    pattern!(r"[*_`~]|!\[[^\]]*\]\([^)]*\)")
        .replace_all(&line, "")
        .chars()
        .take(80)
        .collect()
}
pub fn parse(text: &str) -> (Value, String) {
    let normalized = text.replace("\r\n", "\n");
    let mut meta = json!({});
    if let Some(c) = pattern!(r"^---\n([\s\S]*?)\n---\n?").captures(&normalized) {
        for line in c[1].lines() {
            if let Some(c) = pattern!(r"^(\w+):\s*(.*)$").captures(line) {
                let v = c[2].trim();
                let value = if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                    json!(v[1..v.len() - 1].replace("\\\"", "\""))
                } else if v == "true" || v == "false" {
                    json!(v == "true")
                } else if pattern!(r"^-?\d+(\.\d+)?$").is_match(v) {
                    v.parse::<f64>().ok().map_or(json!(v), |n| json!(n))
                } else {
                    json!(v)
                };
                meta[&c[1]] = value;
            }
        }
        return (meta, normalized[c[0].len()..].into());
    }
    (meta, text.into())
}
fn yaml(v: &Value) -> String {
    if let Some(n) = v.as_f64() {
        if n.fract() == 0. {
            return format!("{n:.0}");
        }
    }
    if let Some(s) = v.as_str() {
        if s.is_empty()
            || s == "true"
            || s == "false"
            || s.starts_with(char::is_whitespace)
            || s.ends_with(char::is_whitespace)
            || pattern!(r#"[:#"'\n]|^-?\d"#).is_match(s)
        {
            return format!("\"{}\"", s.replace('"', "\\\"").replace('\n', " "));
        }
        return s.into();
    }
    v.to_string()
}
pub fn serialize(n: &Note) -> String {
    let v = serde_json::to_value(n).unwrap();
    let fields = [
        "title", "color", "x", "y", "w", "h", "desktop", "pinned", "created", "updated",
    ];
    let head = fields
        .iter()
        .map(|k| format!("{k}: {}", yaml(&v[k])))
        .collect::<Vec<_>>()
        .join("\n");
    format!("---\n{head}\n---\n{}", n.body)
}
pub fn iso(ms: i64) -> String {
    let seconds = ms.div_euclid(1000) as libc::time_t;
    let mut t: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::gmtime_r(&seconds, &mut t) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        t.tm_year + 1900,
        t.tm_mon + 1,
        t.tm_mday,
        t.tm_hour,
        t.tm_min,
        t.tm_sec,
        ms.rem_euclid(1000)
    )
}
fn segment(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('.') && !s.contains(['/', '\\', '\0'])
}
pub fn image(name: &str) -> bool {
    pattern!(r"(?i)\.(png|jpe?g)$").is_match(name)
}
#[derive(Clone)]
pub struct Summary {
    pub slug: String,
    pub title: String,
    pub preview: String,
    pub color: String,
    pub updated: String,
    pub desktop: bool,
}
struct Copy {
    rx: mpsc::Receiver<Result<(), String>>,
    slug: String,
    markdown: Option<String>,
    base: String,
    range: (usize, usize),
}
pub struct Session {
    pub home: PathBuf,
    source_home: PathBuf,
    known: BTreeSet<String>,
    pub list: Vec<Note>,
    pub revision: u64,
    pub error: String,
    dirty: BTreeMap<String, (i64, u64)>,
    persistence: Persistence,
    loaded: bool,
    pending: BTreeMap<String, u64>,
    versions: BTreeMap<String, u64>,
    save_errors: BTreeMap<String, String>,
    summaries: Option<(u64, String, Rc<Vec<Summary>>)>,
    copies: Vec<Copy>,
}
impl Session {
    pub fn new(home: &str) -> Self {
        let home = PathBuf::from(home);
        let persistence = Persistence::new(home.clone());
        let error = if persistence.available() {
            String::new()
        } else {
            "Cannot start Notes storage worker".into()
        };
        Self {
            source_home: home.clone(),
            home,
            known: BTreeSet::new(),
            list: vec![],
            revision: 1,
            error,
            dirty: BTreeMap::new(),
            copies: vec![],
            persistence,
            loaded: false,
            pending: BTreeMap::new(),
            versions: BTreeMap::new(),
            save_errors: BTreeMap::new(),
            summaries: None,
        }
    }
    pub fn ready(&self) -> bool {
        self.loaded
    }
    pub fn busy(&self) -> bool {
        !self.loaded || self.saving() || !self.copies.is_empty()
    }
    pub fn saving(&self) -> bool {
        !self.dirty.is_empty() || !self.pending.is_empty()
    }
    fn receive(&mut self, now: i64) {
        while let Ok(event) = self.persistence.events.try_recv() {
            match event {
                Event::Loaded(result) => {
                    match result {
                        Ok((home, notes)) => {
                            self.home = home;
                            self.known.extend(notes.iter().map(|n| n.slug.clone()));
                            self.list = notes;
                        }
                        Err(error) => self.error = error,
                    }
                    self.loaded = true;
                    self.revision += 1;
                }
                Event::Saved {
                    slug,
                    revision,
                    result,
                } => {
                    if self.pending.get(&slug) != Some(&revision) {
                        continue;
                    }
                    self.pending.remove(&slug);
                    if let Err(error) = result {
                        if self.get(&slug).is_some() && self.versions.get(&slug) == Some(&revision)
                        {
                            self.save_errors.insert(slug.clone(), error.clone());
                            self.error = error;
                            self.dirty
                                .insert(slug, (now.saturating_add(1000), revision));
                            self.revision += 1;
                        }
                    } else if let Some(old) = self.save_errors.remove(&slug) {
                        if self.error == old {
                            self.error = self
                                .save_errors
                                .values()
                                .next()
                                .cloned()
                                .unwrap_or_default();
                        }
                        self.revision += 1;
                    }
                }
            }
        }
    }
    #[cfg(test)]
    pub fn pause_storage(&self, paused: bool) {
        self.persistence.pause(paused);
    }
    #[cfg(test)]
    pub fn queued_saves(&self) -> usize {
        self.persistence.queued()
    }
    pub fn root(&self) -> PathBuf {
        self.home.join("Notes")
    }
    pub fn path(&self, slug: &str) -> io::Result<PathBuf> {
        note_path(&self.home, slug)
    }
    pub fn sidebar(&mut self, query: &str) -> Rc<Vec<Summary>> {
        let query = query.trim().to_lowercase();
        if let Some((rev, cached_query, rows)) = &self.summaries {
            if *rev == self.revision && *cached_query == query {
                return rows.clone();
            }
        }
        let rows = Rc::new(
            self.list
                .iter()
                .filter(|n| {
                    query.is_empty()
                        || n.title.to_lowercase().contains(&query)
                        || n.body.to_lowercase().contains(&query)
                })
                .map(|n| Summary {
                    slug: n.slug.clone(),
                    title: n.title.clone(),
                    preview: n.preview.clone(),
                    color: n.color.clone(),
                    updated: n.updated.clone(),
                    desktop: n.desktop,
                })
                .collect(),
        );
        self.summaries = Some((self.revision, query, Rc::clone(&rows)));
        rows
    }
    pub fn get(&self, slug: &str) -> Option<&Note> {
        self.list.iter().find(|n| n.slug == slug)
    }
    pub fn update(&mut self, slug: &str, patch: &Value, touch: bool, now: i64) {
        if let Some(n) = self.list.iter_mut().find(|n| n.slug == slug) {
            if let Some(body) = patch["body"].as_str() {
                n.body = body.into();
                n.body_revision = n.body_revision.wrapping_add(1);
                n.preview = preview(body);
                let t = title(body);
                if !t.is_empty() {
                    n.title = t;
                }
            }
            if let Some(c) = patch["color"].as_str().filter(|c| COLORS.contains(c)) {
                n.color = c.into();
            }
            for k in ["x", "y", "w", "h"] {
                if let Some(v) = patch[k].as_f64().filter(|v| v.is_finite()) {
                    match k {
                        "x" => n.x = v,
                        "y" => n.y = v,
                        "w" => n.w = v.max(160.),
                        _ => n.h = v.max(110.),
                    }
                }
            }
            if let Some(v) = patch["desktop"].as_bool() {
                n.desktop = v;
            }
            if let Some(v) = patch["pinned"].as_bool() {
                n.pinned = v;
            }
            if touch {
                n.updated = iso(now);
            }
            self.revision += 1;
            self.versions.insert(slug.into(), self.revision);
            self.dirty.insert(slug.into(), (now + 600, self.revision));
        }
    }
    pub fn create(
        &mut self,
        body: String,
        color: &str,
        desktop: bool,
        now: i64,
    ) -> Result<String, String> {
        if !self.loaded {
            return Err("Notes are still loading".into());
        }
        let base = format!(
            "note-{}",
            iso(now)
                .replace(['-', ':'], "")
                .split('.')
                .next()
                .unwrap()
                .replace('T', "-")
        );
        let mut i = 1;
        let slug = loop {
            let s = if i == 1 {
                base.clone()
            } else {
                format!("{base}-{i}")
            };
            if self.known.contains(&s) {
                i += 1;
                continue;
            }
            let path = self.path(&s).map_err(|e| e.to_string())?;
            match fs::create_dir(path) {
                Ok(_) => break s,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => i += 1,
                Err(e) => return Err(e.to_string()),
            }
        };
        let count = self.list.iter().filter(|n| n.desktop).count();
        let n = Note {
            slug: slug.clone(),
            title: title(&body),
            preview: preview(&body),
            body_revision: 0,
            body,
            color: if COLORS.contains(&color) {
                color
            } else {
                "yellow"
            }
            .into(),
            x: 140. + (count % 6) as f64 * 30.,
            y: 90. + (count % 6) as f64 * 30.,
            w: 260.,
            h: 240.,
            desktop,
            pinned: false,
            created: iso(now),
            updated: iso(now),
        };
        self.known.insert(slug.clone());
        self.list.insert(0, n);
        self.revision += 1;
        self.versions.insert(slug.clone(), self.revision);
        self.dirty.insert(slug.clone(), (0, self.revision));
        self.flush(now, true);
        Ok(slug)
    }
    pub fn flush(&mut self, now: i64, force: bool) {
        self.receive(now);
        self.enqueue(now, force, false);
    }
    fn enqueue(&mut self, now: i64, force: bool, wait: bool) {
        let slugs: Vec<_> = self
            .dirty
            .iter()
            .filter(|(_, (at, _))| force || now >= *at)
            .map(|(slug, (_, revision))| (slug.clone(), *revision))
            .collect();
        for (slug, revision) in slugs {
            if !wait && !self.persistence.can_submit(&slug) {
                continue;
            }
            let Some(note) = self.get(&slug).cloned() else {
                self.dirty.remove(&slug);
                continue;
            };
            if self.persistence.submit(Save { note, revision }, wait) {
                self.dirty.remove(&slug);
                self.pending.insert(slug, revision);
            }
        }
    }
    pub fn delete(&mut self, slug: &str) -> Result<(), String> {
        let path = self.path(slug).map_err(|e| e.to_string())?;
        if crate::files::trash::put(&path).is_err() {
            let trash = self.root().join(".trash");
            fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
            let mut target = trash.join(slug);
            let mut i = 2;
            while target.exists() {
                target = trash.join(format!("{slug}-{i}"));
                i += 1;
            }
            fs::rename(path, target).map_err(|e| e.to_string())?;
        }
        self.dirty.remove(slug);
        self.pending.remove(slug);
        self.versions.remove(slug);
        self.list.retain(|n| n.slug != slug);
        self.revision += 1;
        Ok(())
    }
    pub fn free_name(&self, slug: &str, name: &str) -> Result<String, String> {
        let dir = self.path(slug).map_err(|e| e.to_string())?;
        let name = name.replace(['/', '\\', '\0'], "_");
        let (base, ext) = name
            .rsplit_once('.')
            .filter(|(b, _)| !b.is_empty())
            .map_or((name.as_str(), String::new()), |(b, e)| {
                (b, format!(".{e}"))
            });
        let mut out = name.clone();
        let mut i = 2;
        while dir.join(&out).exists() {
            out = format!("{base}-{i}{ext}");
            i += 1;
        }
        Ok(out)
    }
    pub fn attach(
        &mut self,
        slug: &str,
        source: &str,
        range: (usize, usize),
    ) -> Result<(), String> {
        let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
        if !source.starts_with(&self.home) || !source.is_file() {
            return Err("Attachment must be a file in home".into());
        }
        let name = self.free_name(slug, &source.file_name().unwrap().to_string_lossy())?;
        let dest = self.path(slug).map_err(|e| e.to_string())?.join(&name);
        let md = if image(&name) {
            format!("![{name}]({name})")
        } else {
            format!("[📎 {name}]({name})")
        };
        // Reserve before spawning: simultaneous attachments cannot select the same name.
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = fs::copy(source, dest)
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
        let base = self.get(slug).map(|n| n.body.clone()).unwrap_or_default();
        self.copies.push(Copy {
            rx,
            slug: slug.into(),
            markdown: Some(md),
            base,
            range,
        });
        Ok(())
    }
    pub fn insert_attachment(
        &mut self,
        slug: &str,
        md: &str,
        base: &str,
        range: (usize, usize),
        now: i64,
    ) {
        if let Some(n) = self.get(slug) {
            let body = if n.body == base {
                let chars: Vec<_> = base.chars().collect();
                let a = range.0.min(chars.len());
                let b = range.1.min(chars.len()).max(a);
                format!(
                    "{}\n{}\n{}",
                    chars[..a].iter().collect::<String>(),
                    md,
                    chars[b..].iter().collect::<String>()
                )
            } else {
                format!("{}\n\n{md}\n", n.body.trim_end())
            };
            self.update(slug, &json!({"body":body}), true, now);
        }
    }
    pub fn duplicate(&mut self, slug: &str, now: i64) -> Result<String, String> {
        let n = self.get(slug).cloned().ok_or("Missing note")?;
        let copy = self.create(n.body, &n.color, n.desktop, now)?;
        self.update(&copy, &json!({"x":n.x+30.,"y":n.y+30.}), false, now);
        let from = self.path(slug).map_err(|e| e.to_string())?;
        let to = self.path(&copy).map_err(|e| e.to_string())?;
        let home = self.home.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            fn copy_dir(from: &Path, to: &Path, home: &Path, depth: usize) -> io::Result<()> {
                if depth > 64 {
                    return Err(io::Error::other("Attachment nesting too deep"));
                }
                for e in fs::read_dir(from)? {
                    let e = e?;
                    if e.file_name() == "nota.md"
                        || e.file_name().to_string_lossy().starts_with(".nota-")
                    {
                        continue;
                    }
                    let path = fs::canonicalize(e.path())?;
                    if !path.starts_with(home) {
                        return Err(io::Error::other("Attachment outside home"));
                    }
                    let dest = to.join(e.file_name());
                    if path.is_dir() {
                        fs::create_dir_all(&dest)?;
                        copy_dir(&path, &dest, home, depth + 1)?;
                    } else {
                        fs::copy(path, dest)?;
                    }
                }
                Ok(())
            }
            let _ = tx.send(copy_dir(&from, &to, &home, 0).map_err(|e| e.to_string()));
        });
        self.copies.push(Copy {
            rx,
            slug: copy.clone(),
            markdown: None,
            base: String::new(),
            range: (0, 0),
        });
        Ok(copy)
    }
    pub fn tick(&mut self, now: i64) {
        self.receive(now);
        let mut pending = vec![];
        for c in std::mem::take(&mut self.copies) {
            match c.rx.try_recv() {
                Ok(Ok(_)) => {
                    if let Some(md) = c.markdown {
                        self.insert_attachment(&c.slug, &md, &c.base, c.range, now)
                    }
                }
                Ok(Err(e)) => {
                    self.error = e;
                    self.revision += 1;
                }
                Err(mpsc::TryRecvError::Empty) => pending.push(c),
                Err(_) => {
                    self.error = "Attachment worker stopped".into();
                }
            }
        }
        self.copies = pending;
        self.flush(now, false);
    }
    pub fn widgets(&self, w: f64, h: f64) -> Value {
        json!(self.list.iter().filter(|n|n.desktop).map(|n|json!({"id":n.slug,"type":"note","pinned":n.pinned,"rect":{"position":"absolute","left":n.x.clamp(0.,(w-80.).max(0.)),"top":n.y.clamp(0.,(h-60.).max(0.)),"width":n.w,"height":n.h}})).collect::<Vec<_>>())
    }
}
thread_local! {static SESSION:RefCell<Option<Session>>=const{RefCell::new(None)};}
pub fn with<R>(home: &str, f: impl FnOnce(&mut Session) -> R) -> R {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let root = PathBuf::from(home);
        if slot.as_ref().is_none_or(|s| s.source_home != root) {
            *slot = Some(Session::new(home));
        }
        f(slot.as_mut().unwrap())
    })
}
/// Like `with`, but None once the thread's session is gone (at process exit the window hosts are
/// dropped after thread-locals): nothing is left to flush then, `Drop for Session` did it.
pub fn try_with<R>(home: &str, f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSION.try_with(|_| ()).ok()?;
    Some(with(home, f))
}
impl Drop for Session {
    fn drop(&mut self) {
        // pending edits reach the disk when the session goes away (thread exit, another home)
        self.receive(i64::MAX);
        self.enqueue(i64::MAX, true, true);
        // Only session/process teardown joins disk I/O. Closing one window merely
        // queues its final revision, so the shared compositor remains responsive.
        self.persistence.finish();
    }
}
pub fn background(input: &Value) -> Value {
    let home = input["home"].as_str().unwrap_or("");
    if home.is_empty() {
        return json!({});
    }
    with(home, |s| {
        s.tick(crate::clock::now());
        if input["revision"].as_u64() == Some(s.revision) {
            json!({"same":true,"revision":s.revision})
        } else {
            json!({"revision":s.revision,"widgets":s.widgets(input["screen"]["width"].as_f64().unwrap_or(1920.),input["screen"]["height"].as_f64().unwrap_or(1080.))})
        }
    })
}

pub(super) fn note_path(home: &Path, slug: &str) -> io::Result<PathBuf> {
    if !segment(slug) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Invalid note folder",
        ));
    }
    let root = fs::canonicalize(home.join("Notes"))?;
    if !root.starts_with(home) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Notes is outside home",
        ));
    }
    let path = root.join(slug);
    if path.exists() && !fs::canonicalize(&path)?.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Note is outside Notes",
        ));
    }
    Ok(path)
}

pub(super) fn load(home: &Path) -> io::Result<Vec<Note>> {
    let entries = fs::read_dir(home.join("Notes"))?;
    let mut list = Vec::new();
    for entry in entries.flatten() {
        let slug = entry.file_name().to_string_lossy().to_string();
        if !segment(&slug) {
            continue;
        }
        let Ok(path) = note_path(home, &slug) else {
            continue;
        };
        let Ok(file) = fs::canonicalize(path.join("nota.md")) else {
            continue;
        };
        if !file.starts_with(home) {
            continue;
        }
        if !fs::metadata(&file).is_ok_and(|m| m.is_file()) {
            continue;
        }
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        let (m, body) = parse(&text);
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as i64);
        let number = |k: &str, f: f64| m[k].as_f64().filter(|n| n.is_finite()).unwrap_or(f);
        let title = m["title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| title(&body));
        list.push(Note {
            slug,
            preview: preview(&body),
            body_revision: 0,
            body,
            title,
            color: m["color"]
                .as_str()
                .filter(|c| COLORS.contains(c))
                .unwrap_or("yellow")
                .into(),
            x: number("x", 80.),
            y: number("y", 80.),
            w: number("w", 260.).max(160.),
            h: number("h", 240.).max(110.),
            desktop: m["desktop"] == true,
            pinned: m["pinned"] == true,
            created: m["created"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| iso(modified)),
            updated: m["updated"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| iso(modified)),
        });
    }
    list.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(list)
}
