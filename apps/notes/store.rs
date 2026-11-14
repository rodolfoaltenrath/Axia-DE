//! Shared Rust session and the original Markdown/YAML storage format.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
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
}
fn re(s: &str) -> regex::Regex {
    regex::Regex::new(s).unwrap()
}
pub fn title(body: &str) -> String {
    let line = body
        .lines()
        .find(|l| !l.trim().is_empty() && !re(r"^(`{3,}|---)").is_match(l.trim()))
        .unwrap_or("");
    let line = re(r"^\s*(#{1,6}\s+|>\s*|[-*+]\s+(\[[ xX]\]\s+)?|\d+[.)]\s+)").replace(line, "");
    let line = re(r"[*_`~]|!\[[^\]]*\]\([^)]*\)").replace_all(&line, "");
    line.trim().chars().take(60).collect()
}
pub fn preview(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !re(r"^(`{3,}|---|#)").is_match(l))
        .unwrap_or("");
    let line = re(r"^(>\s*|[-*+]\s+(\[[ xX]\]\s+)?|\d+[.)]\s+)").replace(line, "");
    re(r"[*_`~]|!\[[^\]]*\]\([^)]*\)")
        .replace_all(&line, "")
        .chars()
        .take(80)
        .collect()
}
pub fn parse(text: &str) -> (Value, String) {
    let normalized = text.replace("\r\n", "\n");
    let mut meta = json!({});
    if let Some(c) = re(r"^---\n([\s\S]*?)\n---\n?").captures(&normalized) {
        for line in c[1].lines() {
            if let Some(c) = re(r"^(\w+):\s*(.*)$").captures(line) {
                let v = c[2].trim();
                let value = if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                    json!(v[1..v.len() - 1].replace("\\\"", "\""))
                } else if v == "true" || v == "false" {
                    json!(v == "true")
                } else if re(r"^-?\d+(\.\d+)?$").is_match(v) {
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
            || re(r#"[:#"'\n]|^-?\d"#).is_match(s)
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
    re(r"(?i)\.(png|jpe?g)$").is_match(name)
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
    pub list: Vec<Note>,
    pub revision: u64,
    pub error: String,
    dirty: BTreeMap<String, i64>,
    copies: Vec<Copy>,
}
impl Session {
    pub fn new(home: &str) -> Self {
        let mut s = Self {
            home: fs::canonicalize(home).unwrap_or_else(|_| PathBuf::from(home)),
            list: vec![],
            revision: 1,
            error: String::new(),
            dirty: BTreeMap::new(),
            copies: vec![],
        };
        if !s.home.join("Notes").exists() && s.home.join("Notas").exists() {
            let _ = fs::rename(s.home.join("Notas"), s.home.join("Notes"));
        }
        let _ = fs::create_dir_all(s.home.join("Notes"));
        s.load();
        s
    }
    pub fn root(&self) -> PathBuf {
        self.home.join("Notes")
    }
    pub fn path(&self, slug: &str) -> io::Result<PathBuf> {
        if !segment(slug) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Invalid note folder",
            ));
        }
        let root = fs::canonicalize(self.root())?;
        if !root.starts_with(&self.home) {
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
    fn load(&mut self) {
        let Ok(entries) = fs::read_dir(self.root()) else {
            return;
        };
        for entry in entries.flatten() {
            let slug = entry.file_name().to_string_lossy().to_string();
            if !segment(&slug) {
                continue;
            }
            let Ok(path) = self.path(&slug) else {
                continue;
            };
            let Ok(file) = fs::canonicalize(path.join("nota.md")) else {
                continue;
            };
            if !file.starts_with(&self.home) {
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
            self.list.push(Note {
                slug,
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
        self.list.sort_by(|a, b| b.updated.cmp(&a.updated));
    }
    pub fn get(&self, slug: &str) -> Option<&Note> {
        self.list.iter().find(|n| n.slug == slug)
    }
    pub fn update(&mut self, slug: &str, patch: &Value, touch: bool, now: i64) {
        if let Some(n) = self.list.iter_mut().find(|n| n.slug == slug) {
            if let Some(body) = patch["body"].as_str() {
                n.body = body.into();
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
            self.dirty.insert(slug.into(), now + 600);
            self.revision += 1;
        }
    }
    pub fn create(
        &mut self,
        body: String,
        color: &str,
        desktop: bool,
        now: i64,
    ) -> Result<String, String> {
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
        self.list.insert(0, n);
        self.dirty.insert(slug.clone(), 0);
        self.flush(now, true);
        self.revision += 1;
        Ok(slug)
    }
    pub fn flush(&mut self, now: i64, force: bool) {
        let slugs: Vec<_> = self
            .dirty
            .iter()
            .filter(|(_, t)| force || now >= **t)
            .map(|(s, _)| s.clone())
            .collect();
        for slug in slugs {
            if let Some(n) = self.get(&slug) {
                let result = (|| -> io::Result<()> {
                    static SEQ: AtomicU64 = AtomicU64::new(0);
                    let dir = self.path(&slug)?;
                    let tmp = dir.join(format!(
                        ".nota-{}-{}.tmp",
                        std::process::id(),
                        SEQ.fetch_add(1, Ordering::Relaxed)
                    ));
                    let mut f = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&tmp)?;
                    f.write_all(serialize(n).as_bytes())?;
                    f.sync_data()?;
                    fs::rename(tmp, dir.join("nota.md"))
                })();
                match result {
                    Ok(_) => {
                        self.dirty.remove(&slug);
                    }
                    Err(e) => {
                        self.error = e.to_string();
                    }
                }
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
        let root = fs::canonicalize(home).unwrap_or_else(|_| PathBuf::from(home));
        if slot.as_ref().is_none_or(|s| s.home != root) {
            *slot = Some(Session::new(home));
        }
        f(slot.as_mut().unwrap())
    })
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
