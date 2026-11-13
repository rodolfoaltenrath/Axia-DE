//! Native voice recorder: Rust owns state, waveform, edits and the entire view.
#[cfg(test)]
mod tests;
mod ui;
mod wave;
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    date_format,
    i18n::I18n,
    NativeApp,
};
use serde_json::{json, Value};

#[derive(Debug)]
enum Tag {
    Dirs,
    Music,
    Folder,
    Sound,
    Events,
    List(u64),
    Wave(u64),
    Name(u64),
    Record,
    Play(u64),
    Export(u64),
    Rename(u64, String),
    Ignore,
}
#[derive(Clone)]
struct Entry {
    path: String,
    mtime: i64,
}
#[derive(Clone, Default)]
struct Wave {
    peaks: Vec<f64>,
    duration: f64,
}
enum Operation {
    Record,
    Export {
        input: String,
        format: String,
        range: Option<[f64; 2]>,
        select: bool,
    },
}
struct Target {
    base: String,
    ext: String,
    path: String,
    attempt: u32,
    operation: Operation,
}
pub struct App {
    jobs: Jobs<Tag>,
    booted: bool,
    home: String,
    music: String,
    folder: String,
    i18n: I18n,
    region: String,
    hour: String,
    theme: Value,
    rtl: bool,
    width: f64,
    entries: Vec<Entry>,
    list_gen: u64,
    selection: Option<String>,
    selection_gen: u64,
    wave: Option<Wave>,
    rec: i64,
    paused: bool,
    stopping: bool,
    rec_ms: f64,
    levels: Vec<f64>,
    record_start: bool,
    play: i64,
    play_start: bool,
    playing: bool,
    pos: f64,
    seek_at: Option<i64>,
    seek_to: f64,
    trim_paused: bool,
    trim: Option<[f64; 2]>,
    drag: Option<usize>,
    renaming: Option<String>,
    format: String,
    source: String,
    sound: Value,
    target: Option<Target>,
    op_gen: u64,
    exporting: bool,
    message: Value,
    message_until: i64,
    events_at: i64,
    events_pending: bool,
    deferred: Vec<Value>,
    pixels: Vec<u8>,
    raster_key: Value,
    raster_generation: u64,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            booted: false,
            home: String::new(),
            music: String::new(),
            folder: String::new(),
            i18n: I18n::default(),
            region: String::new(),
            hour: "auto".into(),
            theme: Value::Null,
            rtl: false,
            width: 800.,
            entries: vec![],
            list_gen: 0,
            selection: None,
            selection_gen: 0,
            wave: None,
            rec: 0,
            paused: false,
            stopping: false,
            rec_ms: 0.,
            levels: vec![],
            record_start: false,
            play: 0,
            play_start: false,
            playing: false,
            pos: 0.,
            seek_at: None,
            seek_to: 0.,
            trim_paused: false,
            trim: None,
            drag: None,
            renaming: None,
            format: "opus".into(),
            source: String::new(),
            sound: Value::Null,
            target: None,
            op_gen: 0,
            exporting: false,
            message: Value::Null,
            message_until: 0,
            events_at: 0,
            events_pending: false,
            deferred: vec![],
            pixels: vec![],
            raster_key: Value::Null,
            raster_generation: 0,
            last_tree: Value::Null,
        }
    }
}
impl NativeApp for App {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
        self.home = home.unwrap_or("").trim_end_matches('/').into();
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted
            .then_some(if self.jobs.busy() || self.rec > 0 || self.play > 0 {
                50.
            } else {
                200.
            })
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        (self.rec > 0 || self.play > 0 || self.record_start || self.play_start).then_some(100.)
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
    fn pixels(&self, id: &str) -> Option<(usize, usize, &[u8])> {
        (id == "wave" && !self.pixels.is_empty()).then_some((
            self.wave_width() - 2,
            104,
            &self.pixels,
        ))
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        if self.renaming.is_some() || flags & (2 | 16) != 0 {
            return None;
        }
        let cmd = if (key == " " || key == "Space") && self.selection.is_some() && self.rec == 0 {
            "play"
        } else if key.eq_ignore_ascii_case("r") || chr.eq_ignore_ascii_case("r") {
            "record"
        } else {
            return None;
        };
        Some(self.request(&json!({"cmd":cmd})))
    }
}
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn stem(path: &str) -> &str {
    basename(path)
        .rsplit_once('.')
        .map_or(basename(path), |(s, _)| s)
}
fn audio(path: &str) -> bool {
    path.rsplit('.').next().is_some_and(|s| {
        ["wav", "opus", "ogg", "mp3", "m4a", "flac"].contains(&s.to_ascii_lowercase().as_str())
    })
}
fn finite(v: &Value) -> f64 {
    v.as_f64()
        .filter(|n| n.is_finite() && *n >= 0.)
        .unwrap_or(0.)
}
fn duration(ms: f64) -> String {
    let s = (ms.max(0.) / 1000.).floor() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
impl App {
    fn notice(&mut self, key: &str, params: Value, now: i64) {
        self.message = json!({"key":key,"params":params});
        self.message_until = now + 3500;
    }
    fn close_media(&mut self, id: i64) {
        if id > 0 {
            self.jobs.enqueue(Tag::Ignore, "media.close", json!([id]));
        }
    }
    fn control(&mut self, id: i64, cmd: &str, value: Value) {
        if id > 0 {
            self.jobs
                .enqueue(Tag::Ignore, "media.control", json!([id, cmd, value]));
        }
    }
    fn reload(&mut self) {
        if !self.folder.is_empty() {
            self.list_gen += 1;
            self.jobs.cancel(|t| matches!(t, Tag::List(_)));
            self.jobs
                .enqueue(Tag::List(self.list_gen), "fs.list", json!([self.folder]));
        }
    }
    fn select(&mut self, path: Option<String>) {
        self.selection_gen += 1;
        self.jobs
            .cancel(|t| matches!(t, Tag::Wave(_) | Tag::Play(_)));
        self.close_media(self.play);
        self.play = 0;
        self.play_start = false;
        self.playing = false;
        self.pos = 0.;
        self.seek_at = None;
        self.wave = None;
        self.trim = None;
        self.drag = None;
        self.renaming = None;
        self.selection = path;
        if let Some(path) = &self.selection {
            self.jobs.enqueue(
                Tag::Wave(self.selection_gen),
                "media.waveform",
                json!([path, 400]),
            );
        }
    }
    fn microphones(&self) -> Vec<&Value> {
        self.sound["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["monitorOf"].as_i64() == Some(-1))
            .collect()
    }
    fn source_name(&self) -> String {
        if !self.source.is_empty() {
            return self.source.clone();
        }
        let m = self.microphones();
        m.iter()
            .find(|m| m["default"] == true)
            .or(m.first())
            .and_then(|m| m["name"].as_str())
            .unwrap_or("")
            .into()
    }
    fn wave_width(&self) -> usize {
        (self.width - 280.).clamp(200., 760.).round() as usize
    }
    fn wave_left(&self) -> f64 {
        if self.rtl {
            (self.width - 240. - 20. - self.wave_width() as f64).max(20.)
        } else {
            260.
        }
    }
    fn wave_point(&self, input: &Value) -> Option<f64> {
        let x = input["x"].as_f64().filter(|x| x.is_finite())?;
        Some(((x - self.wave_left()) / self.wave_width() as f64).clamp(0., 1.))
    }
    fn update_pixels(&mut self) {
        let played = self
            .wave
            .as_ref()
            .filter(|w| w.duration > 0. && self.play > 0)
            .map_or(0., |w| self.pos / w.duration);
        let key = json!([
            self.selection_gen,
            self.wave_width(),
            self.theme["mode"],
            played,
            self.trim,
            self.wave.as_ref().map(|w| &w.peaks)
        ]);
        if key != self.raster_key {
            self.raster_key = key;
            self.raster_generation += 1;
            self.pixels = self.wave.as_ref().map_or(vec![], |w| {
                wave::draw(
                    &w.peaks,
                    self.wave_width() - 2,
                    104,
                    self.theme["mode"] == "dark",
                    played,
                    self.trim,
                )
            });
        }
    }
    fn check_name(&mut self) {
        if let Some(t) = &mut self.target {
            let suffix = if t.attempt == 1 {
                String::new()
            } else {
                format!(" {}", t.attempt)
            };
            t.path = format!("{}/{}{suffix}.{}", self.folder, t.base, t.ext);
            self.jobs
                .enqueue(Tag::Name(self.op_gen), "fs.stat", json!([t.path]));
        }
    }
    fn record(&mut self, now: i64) {
        if self.rec > 0 {
            if !self.stopping {
                self.stopping = true;
                self.jobs
                    .enqueue(Tag::Ignore, "media.stop", json!([self.rec]));
            }
            return;
        }
        if self.record_start || self.target.is_some() || self.exporting || self.folder.is_empty() {
            return;
        }
        let t = crate::clock::local(now);
        let date = date_format::date(&mut self.i18n, &self.region, &t, "short");
        let time = date_format::time(&mut self.i18n, &self.region, &self.hour, &t, false)
            .replace(':', ".");
        let base = self
            .i18n
            .caption(&json!({"key":"recorder.name","params":{"date":date,"time":time}}))
            .replace(['/', '\\', '\0'], "-");
        self.op_gen += 1;
        self.record_start = true;
        self.target = Some(Target {
            base,
            ext: "wav".into(),
            path: String::new(),
            attempt: 1,
            operation: Operation::Record,
        });
        self.check_name();
    }
    fn play(&mut self) {
        if self.play > 0 {
            self.playing = !self.playing;
            self.trim_paused = false;
            self.control(
                self.play,
                if self.playing { "play" } else { "pause" },
                Value::Null,
            );
            return;
        }
        if self.play_start {
            return;
        }
        if let Some(path) = &self.selection {
            self.play_start = true;
            self.jobs.enqueue(Tag::Play(self.selection_gen),"media.play",json!(["recorder",path,{"music":true,"appName":self.i18n.caption(&json!({"key":"apps.recorder"}))}]));
        }
    }
    fn export(&mut self, trimmed: bool, now: i64) {
        if self.exporting || self.target.is_some() {
            return;
        }
        let Some(input) = self.selection.clone() else {
            return;
        };
        let format = if trimmed && input.to_ascii_lowercase().ends_with(".wav") {
            "wav".to_owned()
        } else {
            self.format.clone()
        };
        let range = if trimmed {
            self.trim
                .zip(self.wave.as_ref())
                .map(|(r, w)| [(r[0] * w.duration).round(), (r[1] * w.duration).round()])
        } else {
            None
        };
        if trimmed && range.is_none() {
            return;
        }
        let suffix = if trimmed {
            format!(
                " ({})",
                self.i18n.caption(&json!({"key":"recorder.trimmed"}))
            )
        } else {
            String::new()
        };
        self.op_gen += 1;
        self.exporting = true;
        self.notice("recorder.working", json!({}), now);
        self.message_until = 0;
        self.target = Some(Target {
            base: format!("{}{suffix}", stem(&input)),
            ext: format.clone(),
            path: String::new(),
            attempt: 1,
            operation: Operation::Export {
                input,
                format,
                range,
                select: trimmed,
            },
        });
        self.check_name();
    }
    fn rename(&mut self, now: i64) {
        let Some(name) = self.renaming.take() else {
            return;
        };
        let name = name.trim().replace(['/', '\\', '\0'], "-");
        if name.is_empty() || self.exporting {
            return;
        }
        if let Some(path) = &self.selection {
            let ext = basename(path).rsplit('.').next().unwrap_or("wav");
            let to = format!("{}/{name}.{ext}", self.folder);
            if &to != path {
                self.jobs.enqueue(
                    Tag::Rename(self.selection_gen, to.clone()),
                    "fs.rename",
                    json!([path, to]),
                );
            }
        }
        self.message_until = now + 3500;
    }
    fn replay(&mut self, now: i64) {
        let events = std::mem::take(&mut self.deferred);
        for event in events {
            self.event(&event, now);
        }
    }
    fn event(&mut self, e: &Value, now: i64) {
        if e["kind"] == "sound" {
            self.sound = e["data"].clone();
            return;
        }
        if e["kind"] != "media" {
            return;
        }
        let id = e["id"].as_i64().unwrap_or(0);
        if id <= 0 {
            return;
        }
        if id != self.rec && id != self.play {
            if (self.record_start || self.play_start) && self.deferred.len() < 256 {
                self.deferred.push(e.clone());
            }
            return;
        }
        let d = &e["data"];
        let typ = e["type"].as_str().unwrap_or("");
        if id == self.rec {
            match typ {
                "level" => {
                    self.rec_ms = finite(&d["ms"]);
                    self.levels
                        .push((finite(&d["level"]) / 1000.).clamp(0., 1.));
                    if self.levels.len() > 181 {
                        self.levels.remove(0);
                    }
                }
                "done" | "closed" => {
                    self.close_media(self.rec);
                    self.rec = 0;
                    self.paused = false;
                    self.stopping = false;
                    self.reload();
                    if d["ok"] != false {
                        if let Some(path) = d["path"].as_str() {
                            self.select(Some(path.into()));
                        }
                    }
                }
                "error" => {
                    self.notice("recorder.denied", json!({}), now);
                    self.close_media(self.rec);
                    self.rec = 0;
                    self.paused = false;
                    self.stopping = false;
                    self.reload();
                }
                "state" if d["state"] == "failed" => {
                    self.close_media(self.rec);
                    self.rec = 0;
                    self.paused = false;
                    self.stopping = false;
                    self.notice("recorder.denied", json!({}), now);
                    self.reload();
                }
                _ => {}
            }
        } else {
            match typ {
                "pos" => {
                    self.pos = finite(&d["pos"]);
                    self.playing = d["playing"] == true;
                    if let (Some(trim), Some(w)) = (self.trim, &self.wave) {
                        if self.pos > trim[1] * w.duration && !self.trim_paused {
                            self.trim_paused = true;
                            self.playing = false;
                            self.control(self.play, "pause", Value::Null);
                        }
                    }
                }
                "state" => {
                    self.playing = d["state"] == "playing";
                    if d["state"] == "ended" || d["state"] == "failed" {
                        self.close_media(self.play);
                        self.play = 0;
                        self.playing = false;
                        self.seek_at = None;
                    }
                }
                "closed" | "error" => {
                    self.close_media(self.play);
                    self.play = 0;
                    self.playing = false;
                    self.seek_at = None;
                }
                _ => {}
            }
        }
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        let Completed { tag, result } = done;
        match tag {
            Tag::Dirs => {
                self.music = result
                    .ok()
                    .and_then(|r| r["music"].as_str().map(str::to_owned))
                    .filter(|p| p.starts_with('/'))
                    .unwrap_or_else(|| format!("{}/Music", self.home));
                let folder = self
                    .i18n
                    .caption(&json!({"key":"recorder.folder"}))
                    .replace(['/', '\\', '\0'], "-");
                self.folder = format!(
                    "{}/{}",
                    self.music,
                    if folder.is_empty() {
                        "Recordings"
                    } else {
                        &folder
                    }
                );
                self.jobs
                    .enqueue(Tag::Music, "fs.mkdir", json!([self.music]));
            }
            Tag::Music => self
                .jobs
                .enqueue(Tag::Folder, "fs.mkdir", json!([self.folder])),
            Tag::Folder => self.reload(),
            Tag::Sound => {
                if let Ok(v) = result {
                    self.sound = v;
                }
            }
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + 100;
                if let Ok(v) = result {
                    for e in v.as_array().into_iter().flatten() {
                        self.event(e, now);
                    }
                }
            }
            Tag::List(g) if g == self.list_gen => {
                self.entries = result
                    .ok()
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter(|e| {
                        e["dir"] != true
                            && e["name"]
                                .as_str()
                                .is_some_and(|n| audio(n) && !n.contains('/'))
                    })
                    .filter_map(|e| {
                        let name = e["name"].as_str()?.to_owned();
                        Some(Entry {
                            path: format!("{}/{name}", self.folder),
                            mtime: e["mtime"].as_f64().unwrap_or(0.) as i64,
                        })
                    })
                    .collect();
                self.entries.sort_by(|a, b| b.mtime.cmp(&a.mtime));
            }
            Tag::Wave(g) if g == self.selection_gen => {
                self.wave = result.ok().filter(Value::is_object).map(|v| Wave {
                    duration: finite(&v["duration"]),
                    peaks: v["peaks"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .take(400)
                        .map(|p| finite(p).min(1000.))
                        .collect(),
                });
            }
            Tag::Name(g) if g == self.op_gen => {
                if result.is_ok() {
                    if let Some(t) = &mut self.target {
                        t.attempt += 1;
                        if t.attempt > 1000 {
                            self.target = None;
                            self.record_start = false;
                            self.exporting = false;
                            self.notice("recorder.exportError", json!({}), now);
                            return;
                        }
                    }
                    self.check_name();
                } else if let Some(t) = &self.target {
                    match &t.operation {
                        Operation::Record => {
                            self.jobs.enqueue(Tag::Record,"media.record",json!(["recorder",t.path,{"source":self.source,"appName":self.i18n.caption(&json!({"key":"apps.recorder"}))}]));
                            self.target = None;
                        }
                        Operation::Export {
                            input,
                            format,
                            range,
                            ..
                        } => {
                            let mut options = json!({"format":format});
                            if let Some(r) = range {
                                options["from"] = json!(r[0]);
                                options["to"] = json!(r[1]);
                            }
                            self.jobs.enqueue(
                                Tag::Export(g),
                                "media.transcode",
                                json!(["recorder", input, t.path, options]),
                            );
                        }
                    }
                }
            }
            Tag::Record => {
                self.record_start = false;
                self.rec = result
                    .ok()
                    .and_then(|v| v.as_i64())
                    .filter(|id| *id > 0)
                    .unwrap_or(0);
                self.paused = false;
                self.stopping = false;
                self.rec_ms = 0.;
                self.levels.clear();
                if self.rec == 0 {
                    self.notice("recorder.denied", json!({}), now);
                }
                self.replay(now);
            }
            Tag::Play(g) => {
                if g != self.selection_gen {
                    if let Ok(id) = result {
                        self.close_media(id.as_i64().unwrap_or(0));
                    }
                    return;
                }
                self.play_start = false;
                self.play = result
                    .ok()
                    .and_then(|v| v.as_i64())
                    .filter(|id| *id > 0)
                    .unwrap_or(0);
                self.playing = self.play > 0;
                self.trim_paused = false;
                if self.play > 0 {
                    if let (Some(trim), Some(w)) = (self.trim, &self.wave) {
                        self.seek_to = trim[0] * w.duration;
                        self.seek_at = Some(now + 200);
                    }
                }
                self.replay(now);
            }
            Tag::Export(g) if g == self.op_gen => {
                self.exporting = false;
                let target = self.target.take();
                if let Ok(r) = result {
                    if r["ok"] == true {
                        let path = r["path"]
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| target.as_ref().map(|t| t.path.clone()))
                            .unwrap_or_default();
                        self.notice("recorder.exported", json!({"name":basename(&path)}), now);
                        self.reload();
                        if target.is_some_and(|t| {
                            matches!(t.operation, Operation::Export { select: true, .. })
                        }) {
                            self.select(Some(path));
                        }
                        return;
                    }
                }
                self.notice("recorder.exportError", json!({}), now);
            }
            Tag::Rename(g, to) => {
                if result.is_ok() {
                    if g == self.selection_gen {
                        self.select(Some(to));
                    }
                } else {
                    self.notice("recorder.renameError", json!({}), now);
                }
                self.reload();
            }
            _ => {}
        }
    }
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.region = input["settings"]["region"].as_str().unwrap_or("").into();
                self.hour = input["settings"]["clock"]["hour24"]
                    .as_str()
                    .unwrap_or("auto")
                    .into();
                self.theme = input["context"]["theme"].clone();
                self.width = input["context"]["width"]
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .unwrap_or(800.)
                    .max(0.);
                let lang = self.i18n.lang.clone();
                self.rtl = self.i18n.entry(&lang, "meta", "direction") == Some(json!("rtl"));
                if !self.booted {
                    if let Some(owner) =
                        input["args"]["__nativeOwner"].as_i64().filter(|id| *id > 0)
                    {
                        self.jobs.bind(owner);
                        self.booted = true;
                        self.jobs.enqueue(Tag::Dirs, "media.dirs", json!([]));
                        self.jobs.enqueue(Tag::Sound, "media.sound", json!([]));
                    }
                }
                if self.booted && !self.events_pending && now >= self.events_at {
                    self.events_pending = true;
                    self.jobs.enqueue(Tag::Events, "media.events", json!([]));
                }
                if self.seek_at.is_some_and(|t| now >= t) {
                    self.seek_at = None;
                    self.control(self.play, "seek", json!(self.seek_to));
                }
                if self.message_until > 0 && now >= self.message_until {
                    self.message = Value::Null;
                    self.message_until = 0;
                }
                self.update_pixels();
                let tree = ui::tree(self);
                let mut reply = if tree == self.last_tree {
                    json!({"same":true})
                } else {
                    self.last_tree = tree.clone();
                    json!({"tree":tree})
                };
                reply["capture"] = json!(self.drag.is_some());
                if let Some(effect) = self.jobs.pump(now) {
                    reply["effect"] = effect;
                }
                return reply;
            }
            "next" => {
                for done in self.jobs.complete(&input["result"], now) {
                    self.apply(done, now);
                }
            }
            "select" => {
                if let Some(path) = input["key"]
                    .as_str()
                    .filter(|p| self.entries.iter().any(|e| e.path == *p))
                {
                    self.select(Some(path.into()));
                }
            }
            "record" => self.record(now),
            "pause" => {
                if self.rec > 0 && !self.stopping {
                    self.paused = !self.paused;
                    self.control(
                        self.rec,
                        if self.paused { "pause" } else { "play" },
                        Value::Null,
                    );
                }
            }
            "play" => self.play(),
            "source" => {
                if self.rec == 0 && !self.record_start {
                    if let Some(s) = input["key"]
                        .as_str()
                        .filter(|s| self.microphones().iter().any(|m| m["name"] == *s))
                    {
                        self.source = s.into();
                    }
                }
            }
            "format" => {
                if let Some(fmt) = input["key"]
                    .as_str()
                    .filter(|s| ["opus", "mp3", "wav"].contains(s))
                {
                    self.format = fmt.into();
                }
            }
            "trim" => {
                if self.selection.is_some() {
                    self.trim = if self.trim.is_some() {
                        None
                    } else {
                        Some([0.1, 0.9])
                    };
                    self.trim_paused = false;
                }
            }
            "export" => self.export(false, now),
            "saveTrim" => self.export(true, now),
            "rename" => {
                if let Some(path) = &self.selection {
                    self.renaming = Some(stem(path).into());
                }
            }
            "name" => {
                if self.renaming.is_some() {
                    self.renaming = Some(input["value"].as_str().unwrap_or("").into());
                }
            }
            "renameSubmit" => self.rename(now),
            "renameCancel" => self.renaming = None,
            "waveDown" => {
                if let Some(f) = self.wave_point(input) {
                    if let Some(trim) = self.trim {
                        self.drag = Some(if (f - trim[0]).abs() < (f - trim[1]).abs() {
                            0
                        } else {
                            1
                        });
                        return json!({"capture":true});
                    } else if let Some(w) = &self.wave {
                        self.control(self.play, "seek", json!(f * w.duration));
                    }
                }
            }
            "pointerMove" => {
                if let (Some(edge), Some(mut trim), Some(f)) =
                    (self.drag, self.trim, self.wave_point(input))
                {
                    trim[edge] = f;
                    if trim[0] > trim[1] {
                        trim.swap(0, 1);
                        self.drag = Some(1 - edge);
                    }
                    self.trim = Some(trim);
                }
            }
            "dragEnd" => {
                self.drag = None;
                return json!({"capture":false});
            }
            _ => {}
        }
        self.jobs
            .pump(now)
            .map(|effect| json!({"effect":effect}))
            .unwrap_or_else(|| json!({}))
    }
}
