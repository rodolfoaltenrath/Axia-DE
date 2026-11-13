//! Native Logs application. Existing SDK services transport journal/helper I/O;
//! Rust owns filters, generations, live scheduling, rows, export and the scene.
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{i18n::I18n, NativeApp};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::OnceLock,
};

const ROW: f64 = 22.;
const MAX_ROWS: usize = 10_000;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct Entry {
    t: i64,
    s: u64,
    p: i64,
    pid: i64,
    uid: i64,
    k: i64,
    id: String,
    u: String,
    b: String,
    m: String,
    line: usize,
}
impl Default for Entry {
    fn default() -> Self {
        Self {
            t: 0,
            s: 0,
            p: -1,
            pid: -1,
            uid: -1,
            k: 0,
            id: String::new(),
            u: String::new(),
            b: String::new(),
            m: String::new(),
            line: 0,
        }
    }
}
fn level(p: i64) -> &'static str {
    match p {
        0..=3 => "error",
        4 => "warning",
        7 => "debug",
        _ => "info",
    }
}
fn valid_match(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 95
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"@._:-".contains(&c))
}
fn parse_log(text: &str, file: &str) -> Vec<Entry> {
    static ERROR: OnceLock<regex::Regex> = OnceLock::new();
    static WARN: OnceLock<regex::Regex> = OnceLock::new();
    static SOURCE: OnceLock<regex::Regex> = OnceLock::new();
    let error = ERROR.get_or_init(|| {
        regex::Regex::new(
            r"(?-u:\b)(error|err|fail|failed|falla|fallo|abort|crash|segfault)(?-u:\b)",
        )
        .unwrap()
    });
    let warning = WARN.get_or_init(|| {
        regex::Regex::new(r"(?-u:\b)(warning|warn|could not|cannot|aviso|sin |no se pudo)(?-u:\b)")
            .unwrap()
    });
    let source = SOURCE.get_or_init(|| {
        regex::Regex::new(r"^([a-zA-Z][a-zA-Z0-9_.-]{1,30})(\[[0-9]+\])?:").unwrap()
    });
    text.split('\n')
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(i, m)| {
            let lower = m.to_lowercase();
            Entry {
                line: i + 1,
                p: if error.is_match(&lower) {
                    3
                } else if warning.is_match(&lower) {
                    4
                } else {
                    6
                },
                id: source
                    .captures(m)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str())
                    .unwrap_or(file.strip_suffix(".log").unwrap_or(file))
                    .into(),
                m: m.into(),
                ..Default::default()
            }
        })
        .collect()
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sources,
    Files,
    Follow,
    Full,
    Live,
    Export,
}
struct Job {
    method: &'static str,
    args: Value,
    kind: Kind,
    epoch: u64,
    started: i64,
    encoded: String,
}
enum Step {
    Start(Job),
    Take,
    Events,
}
pub struct App {
    owner: i64,
    epoch: u64,
    queue: VecDeque<Job>,
    pending: BTreeMap<u64, Job>,
    step: Option<Step>,
    poll_at: i64,
    source: String,
    file: String,
    level: String,
    range: String,
    query: String,
    process: String,
    live: bool,
    stick: bool,
    selected: Option<usize>,
    offset: f64,
    drag: Option<(f64, f64)>,
    entries: Vec<Entry>,
    files: Vec<Value>,
    system: bool,
    loading: bool,
    error: String,
    notice: Value,
    notice_until: i64,
    elapsed: i64,
    newest: i64,
    reload_at: Option<i64>,
    live_at: Option<i64>,
    width: f64,
    height: f64,
    theme: Value,
    i18n: I18n,
    home: PathBuf,
    region: String,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            owner: 0,
            epoch: 0,
            queue: VecDeque::new(),
            pending: BTreeMap::new(),
            step: None,
            poll_at: 0,
            source: "user".into(),
            file: String::new(),
            level: "all".into(),
            range: "hour".into(),
            query: String::new(),
            process: String::new(),
            live: true,
            stick: true,
            selected: None,
            offset: 0.,
            drag: None,
            entries: vec![],
            files: vec![],
            system: false,
            loading: false,
            error: String::new(),
            notice: Value::Null,
            notice_until: 0,
            elapsed: 0,
            newest: 0,
            reload_at: None,
            live_at: None,
            width: 1040.,
            height: 640.,
            theme: Value::Null,
            i18n: I18n::default(),
            home: PathBuf::new(),
            region: String::new(),
            last_tree: Value::Null,
        }
    }
}
impl NativeApp for App {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
        self.home = PathBuf::from(home.unwrap_or(""));
    }
    fn refresh_ms(&self) -> Option<f64> {
        if !self.pending.is_empty()
            || !self.queue.is_empty()
            || self.reload_at.is_some()
            || self.live_at.is_some()
        {
            Some(50.)
        } else if self.live {
            Some(250.)
        } else if !self.notice.is_null() {
            Some(1000.)
        } else {
            None
        }
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
    fn key(&mut self, key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        let index = self.selected.unwrap_or(0);
        match key {
            "ArrowDown" if !self.entries.is_empty() => {
                self.selected = Some((index + 1).min(self.entries.len() - 1))
            }
            "ArrowUp" if !self.entries.is_empty() => self.selected = Some(index.saturating_sub(1)),
            _ => return None,
        }
        self.stick = false;
        let p = self.selected.unwrap() as f64 * ROW;
        if p < self.offset {
            self.offset = p;
        } else if p + ROW > self.offset + self.list_height() {
            self.offset = p + ROW - self.list_height();
        }
        Some(json!({}))
    }
}
impl App {
    fn enqueue(&mut self, kind: Kind, method: &'static str, args: Value, now: i64) {
        self.queue.push_back(Job {
            kind,
            method,
            args,
            epoch: self.epoch,
            started: now,
            encoded: String::new(),
        });
    }
    fn reload(&mut self, now: i64, delay: i64) {
        self.epoch += 1;
        self.reload_at = Some(now + delay);
        self.live_at = None;
        self.queue
            .retain(|j| !matches!(j.kind, Kind::Full | Kind::Live));
    }
    fn priority(&self) -> Option<i64> {
        match self.level.as_str() {
            "error" => Some(3),
            "warning" => Some(4),
            "info" => Some(6),
            _ => None,
        }
    }
    fn query_args(&self, now: i64, since: i64) -> Value {
        let mut query = json!({"limit":3000});
        if let Some(p) = self.priority() {
            query["prio"] = json!(p);
        }
        let span = match self.range.as_str() {
            "15m" => 900_000_000,
            "hour" => 3_600_000_000,
            "day" => 86_400_000_000,
            _ => 0,
        };
        if since > 0 {
            query["since"] = json!(since);
        } else if span > 0 {
            query["since"] = json!(now.saturating_mul(1000).saturating_sub(span));
        }
        if valid_match(&self.process) {
            query["match"] = json!(self.process);
        }
        if !self.query.is_empty() {
            query["grep"] = json!(self.query);
        }
        if self.source == "kernel" {
            query["kernel"] = json!(true);
        }
        if self.source == "user" {
            query["scope"] = json!("user");
        } else if self.system {
            query["scope"] = json!("system");
        }
        query
    }
    fn read(&mut self, live: bool, now: i64) {
        if self
            .pending
            .values()
            .chain(self.queue.iter())
            .any(|j| j.epoch == self.epoch && matches!(j.kind, Kind::Full | Kind::Live))
        {
            return;
        }
        if !live {
            self.loading = true;
            self.error.clear();
        }
        let kind = if live { Kind::Live } else { Kind::Full };
        if self.source == "lordos" {
            self.enqueue(kind, "logs.tail", json!([self.file, 512 * 1024]), now);
        } else {
            let method = if self.source == "user" || self.system {
                "logs.query"
            } else {
                "logs.system"
            };
            self.enqueue(
                kind,
                method,
                json!([self.query_args(now, if live { self.newest } else { 0 })]),
                now,
            );
        }
    }
    fn apply(&mut self, job: Job, value: &Value, error: Option<&str>, now: i64) {
        if matches!(job.kind, Kind::Full | Kind::Live) && job.epoch != self.epoch {
            return;
        }
        if let Some(error) = error {
            if job.kind == Kind::Full {
                self.entries.clear();
                self.selected = None;
                self.loading = false;
                self.error = if error.contains("administrador") {
                    self.i18n.caption(&json!({"key":"logs.needAdmin"}))
                } else {
                    error.into()
                };
            }
            if job.kind == Kind::Export {
                self.notice = json!({"key":"logs.exportFailed","params":{"error":error}});
                self.notice_until = now + 5000;
            }
            return;
        }
        match job.kind {
            Kind::Sources => self.system = value["system"] == true,
            Kind::Files => {
                self.files = value.as_array().cloned().unwrap_or_default();
                self.files.sort_by(|a, b| {
                    b["mtime"]
                        .as_f64()
                        .unwrap_or(0.)
                        .total_cmp(&a["mtime"].as_f64().unwrap_or(0.))
                });
            }
            Kind::Follow => {}
            Kind::Full | Kind::Live => {
                let local = self.source == "lordos";
                let mut rows = if local {
                    parse_log(value.as_str().unwrap_or(""), &self.file)
                } else {
                    value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| serde_json::from_value::<Entry>(v.clone()).ok())
                        .collect::<Vec<_>>()
                };
                if local {
                    let q = self.query.to_lowercase();
                    let p = self.priority();
                    rows.retain(|r| {
                        p.is_none_or(|p| r.p <= p)
                            && (q.is_empty() || r.m.to_lowercase().contains(&q))
                            && (self.process.is_empty() || r.id == self.process)
                    });
                }
                if job.kind == Kind::Live && !local && rows.is_empty() {
                    return;
                }
                if job.kind == Kind::Full || local {
                    self.entries = rows;
                } else {
                    self.entries.extend(rows);
                }
                if !local && self.entries.len() > MAX_ROWS {
                    let remove = self.entries.len() - MAX_ROWS;
                    self.entries.drain(..remove);
                    self.selected = self.selected.and_then(|i| i.checked_sub(remove));
                }
                self.selected = self.selected.filter(|i| *i < self.entries.len());
                self.newest = self
                    .entries
                    .last()
                    .map(|e| e.t)
                    .filter(|t| *t > 0)
                    .unwrap_or(now.saturating_mul(1000));
                if job.kind == Kind::Full {
                    self.selected = None;
                    self.stick = true;
                    self.offset = 0.;
                    self.loading = false;
                    self.elapsed = (now - job.started).max(0);
                }
            }
            Kind::Export => {
                let path = job.args[0].as_str().unwrap_or("");
                let home = self.home.to_string_lossy();
                self.notice = json!({"key":"logs.exported","params":{"path":path.strip_prefix(home.as_ref()).map(|s|format!("~{s}")).unwrap_or_else(||path.into())}});
                self.notice_until = now + 5000;
            }
        }
    }
    fn next(&mut self, result: &Value, now: i64) {
        match self.step.take() {
            Some(Step::Start(job)) => {
                if let Some(token) = result.as_u64() {
                    self.pending.insert(token, job);
                    self.poll_at = now + 50;
                } else {
                    self.apply(
                        job,
                        &Value::Null,
                        Some(result["error"].as_str().unwrap_or("app.invalidRequest")),
                        now,
                    );
                }
            }
            Some(Step::Take) => {
                for packet in result["value"].as_array().into_iter().flatten() {
                    if let Some(part) = packet["part"].as_str() {
                        if let Some(job) = packet["token"]
                            .as_u64()
                            .and_then(|t| self.pending.get_mut(&t))
                        {
                            job.encoded.push_str(part);
                        }
                        if packet["done"] != true {
                            continue;
                        }
                    }
                    if let Some(job) = packet["token"]
                        .as_u64()
                        .and_then(|t| self.pending.remove(&t))
                    {
                        if packet["part"].is_string() {
                            match serde_json::from_str::<Value>(&job.encoded) {
                                Ok(value) => self.apply(job, &value, None, now),
                                Err(_) => {
                                    self.apply(job, &Value::Null, Some("app.invalidRequest"), now)
                                }
                            }
                        } else {
                            self.apply(job, &packet["value"], packet["error"].as_str(), now);
                        }
                    }
                }
                self.poll_at = now + 50;
            }
            Some(Step::Events) => {
                for event in result["value"].as_array().into_iter().flatten() {
                    let matches = if self.source == "lordos" {
                        event["kind"] == "file" && event["name"] == self.file
                    } else {
                        event["kind"] == "journal"
                    };
                    if self.live && matches && self.live_at.is_none() {
                        self.live_at = Some(now + 700);
                    }
                }
                self.poll_at = now + 50;
            }
            None => {}
        }
    }
    fn pump(&mut self, now: i64) -> Option<Value> {
        if self.step.is_some() || self.owner <= 0 {
            return None;
        }
        if let Some(job) = self.queue.pop_front() {
            let effect = json!({"kind":"nativeCall","service":"nativeAsync","method":"start","args":[self.owner,job.method,job.args]});
            self.step = Some(Step::Start(job));
            return Some(effect);
        }
        if now < self.poll_at {
            return None;
        }
        self.poll_at = now + 250;
        if !self.pending.is_empty() {
            let discard: Vec<_> = self
                .pending
                .iter()
                .filter_map(|(token, job)| {
                    (job.epoch != self.epoch && matches!(job.kind, Kind::Full | Kind::Live))
                        .then_some(*token)
                })
                .collect();
            for token in &discard {
                self.pending.remove(token);
            }
            self.step = Some(Step::Take);
            return Some(
                json!({"kind":"nativeRead","service":"nativeAsync","method":"take","args":[self.owner,self.pending.keys().collect::<Vec<_>>(),discard]}),
            );
        }
        if self.live {
            self.step = Some(Step::Events);
            return Some(
                json!({"kind":"nativeRead","service":"nativeAsync","method":"events","args":[self.owner]}),
            );
        }
        None
    }
    fn list_height(&self) -> f64 {
        (self.height - 40. - 40. - 26. - self.detail_height() - 26.).max(0.)
    }
    fn detail_height(&self) -> f64 {
        if self.height < 560. {
            112.
        } else {
            156.
        }
    }
    fn max_offset(&self) -> f64 {
        (self.entries.len() as f64 * ROW - self.list_height()).max(0.)
    }
    fn scroll(&mut self, offset: f64) {
        if offset.is_finite() {
            self.offset = offset.clamp(0., self.max_offset());
            self.stick = self.offset >= self.max_offset() - 2.;
        }
    }
    fn caption(&mut self, key: &str) -> String {
        self.i18n.caption(&json!({"key":key}))
    }
    fn clock(t: i64) -> String {
        let ms = t / 1000;
        let d = crate::clock::local(ms);
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            d.tm_hour,
            d.tm_min,
            d.tm_sec,
            ms.rem_euclid(1000)
        )
    }
    fn date(&mut self, t: i64, long: bool) -> String {
        let d = crate::clock::local(t / 1000);
        let lang = self.i18n.lang.clone();
        let get = |i18n: &mut I18n, key: &str| {
            i18n.entry(&lang, "format", key)
                .or_else(|| {
                    i18n.entry(lang.split(['-', '_']).next().unwrap_or("en"), "format", key)
                })
                .or_else(|| i18n.entry("en", "format", key))
                .unwrap_or(Value::Null)
        };
        let style = if long { "long" } else { "short" };
        let mut pattern = get(&mut self.i18n, "date")[style]
            .as_str()
            .unwrap_or("{dd}/{mm}/{yyyy}")
            .to_owned();
        let region_lang = self.region.split('_').next().unwrap_or("");
        if !self.region.is_empty() && region_lang == lang.split(['-', '_']).next().unwrap_or("en") {
            let own = self
                .i18n
                .entry(region_lang, "format", "regions")
                .unwrap_or(Value::Null);
            if let Some(p) = own[&self.region]["date"][style].as_str() {
                pattern = p.into();
            }
        }
        for (field, value) in [
            ("day", d.tm_mday.to_string()),
            ("dd", format!("{:02}", d.tm_mday)),
            ("mm", format!("{:02}", d.tm_mon + 1)),
            ("yyyy", (d.tm_year + 1900).to_string()),
        ] {
            pattern = pattern.replace(&format!("{{{field}}}"), &value);
        }
        for (field, key, index) in [
            ("weekday", "weekdays", d.tm_wday),
            ("wd", "weekdaysShort", d.tm_wday),
            ("month", "months", d.tm_mon),
            ("mon", "monthsShort", d.tm_mon),
        ] {
            let names = get(&mut self.i18n, key);
            pattern = pattern.replace(
                &format!("{{{field}}}"),
                names[index.max(0) as usize].as_str().unwrap_or(""),
            );
        }
        pattern
    }
    fn documents(&self) -> PathBuf {
        let config = crate::monitor::read(&self.home.join(".config/user-dirs.dirs"), 65536);
        for line in config.lines() {
            if let Some(s) = line
                .strip_prefix("XDG_DOCUMENTS_DIR=\"$HOME/")
                .and_then(|s| s.strip_suffix('"'))
            {
                return self.home.join(s);
            }
        }
        ["Documentos", "Documents"]
            .iter()
            .map(|d| self.home.join(d))
            .find(|p| p.exists())
            .unwrap_or_else(|| self.home.clone())
    }
    fn export(&mut self, now: i64) {
        let entries = self.entries.clone();
        let mut lines = String::new();
        for e in entries {
            let when = if e.t > 0 {
                format!("{} {}", self.date(e.t, false), Self::clock(e.t))
            } else {
                format!("#{}", e.line)
            };
            let lv = self.caption(&format!("logs.level.{}", level(e.p)));
            let id = if e.id.is_empty() { "-" } else { &e.id };
            let pid = if e.pid >= 0 {
                format!("[{}]", e.pid)
            } else {
                String::new()
            };
            lines.push_str(&format!("{when} {lv} {id}{pid}: {}\n", e.m));
        }
        if lines.is_empty() {
            lines.push('\n');
        }
        let d = crate::clock::local(now);
        let base = self
            .caption("logs.exportName")
            .replace(['/', '\\', '\0'], "_");
        let name = format!(
            "{base}-{:04}{:02}{:02}-{:02}{:02}{:02}.txt",
            d.tm_year + 1900,
            d.tm_mon + 1,
            d.tm_mday,
            d.tm_hour,
            d.tm_min,
            d.tm_sec
        );
        self.enqueue(
            Kind::Export,
            "logs.write",
            json!([self.documents().join(name).to_string_lossy(), lines]),
            now,
        );
    }
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        let cmd = input["cmd"].as_str().unwrap_or("");
        let key = input["key"].as_str().unwrap_or("");
        match cmd {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                self.theme = input["context"]["theme"].clone();
                self.width = input["context"]["width"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .unwrap_or(1040.)
                    .max(0.);
                self.height = input["context"]["height"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .unwrap_or(640.)
                    .max(0.);
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.i18n.decimal = input["context"]["decimal"].as_str().unwrap_or(".").into();
                self.i18n.group = input["context"]["group"].as_str().unwrap_or(",").into();
                self.region = input["settings"]["region"].as_str().unwrap_or("").into();
                if self.owner == 0 {
                    self.owner = input["args"]["__nativeOwner"].as_i64().unwrap_or(0);
                    if self.owner > 0 {
                        self.enqueue(Kind::Sources, "logs.sources", json!([]), now);
                        self.enqueue(Kind::Files, "logs.files", json!([]), now);
                        self.enqueue(Kind::Follow, "logs.follow", json!([true]), now);
                        self.reload(now, 0);
                    }
                }
                if self.reload_at.is_some_and(|at| now >= at) {
                    self.reload_at = None;
                    self.read(false, now);
                }
                if self.live_at.is_some_and(|at| now >= at) {
                    if !self
                        .pending
                        .values()
                        .chain(self.queue.iter())
                        .any(|j| j.epoch == self.epoch && matches!(j.kind, Kind::Full | Kind::Live))
                    {
                        self.live_at = None;
                        self.read(true, now);
                    }
                }
                if !self.notice.is_null() && now >= self.notice_until {
                    self.notice = Value::Null;
                }
                self.offset = if self.stick && self.selected.is_none() {
                    self.max_offset()
                } else {
                    self.offset.min(self.max_offset())
                };
                let tree = ui::tree(self);
                let mut reply = if tree == self.last_tree {
                    json!({"same":true})
                } else {
                    self.last_tree = tree.clone();
                    json!({"tree":tree})
                };
                if let Some(effect) = self.pump(now) {
                    reply["effect"] = effect;
                }
                return reply;
            }
            "next" => self.next(&input["result"], now),
            "source" => {
                if ["user", "system", "kernel", "lordos"].contains(&key) {
                    self.source = key.into();
                    self.file = input["file"].as_str().unwrap_or("").into();
                    self.enqueue(Kind::Files, "logs.files", json!([]), now);
                    self.reload(now, 0);
                }
            }
            "file" => {
                if self.files.iter().any(|f| f["name"] == key) {
                    self.source = "lordos".into();
                    self.file = key.into();
                    self.reload(now, 0);
                }
            }
            "level" => {
                if ["all", "info", "warning", "error"].contains(&key) {
                    self.level = key.into();
                    self.reload(now, 0);
                }
            }
            "range" => {
                if ["15m", "hour", "day", "all"].contains(&key) {
                    self.range = key.into();
                    self.reload(now, 0);
                }
            }
            "search" | "process" => {
                let value = input["value"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(4096)
                    .collect();
                if cmd == "search" {
                    self.query = value;
                } else {
                    self.process = value;
                }
                self.reload(now, 300);
            }
            "live" => {
                self.live = !self.live;
                if !self.live {
                    self.live_at = None;
                }
                self.enqueue(Kind::Follow, "logs.follow", json!([self.live]), now);
            }
            "select" => {
                if let Some(i) = input["key"]
                    .as_u64()
                    .filter(|i| *i < self.entries.len() as u64)
                {
                    self.selected = Some(i as usize);
                    self.stick = false;
                }
            }
            "scroll" => self.scroll(self.offset + input["deltaY"].as_f64().unwrap_or(0.)),
            "dragScroll" => {
                self.drag = Some((input["y"].as_f64().unwrap_or(0.), self.offset));
                return json!({"capture":true});
            }
            "pointerMove" => {
                if let Some((y, offset)) = self.drag {
                    self.scroll(
                        offset
                            + (input["y"].as_f64().unwrap_or(y) - y)
                                * self.entries.len() as f64
                                * ROW
                                / self.list_height().max(1.),
                    );
                }
            }
            "dragEnd" => {
                self.drag = None;
                return json!({"capture":false});
            }
            "export" => self.export(now),
            _ => {}
        }
        self.pump(now)
            .map(|effect| json!({"effect":effect}))
            .unwrap_or_else(|| json!({}))
    }
}
