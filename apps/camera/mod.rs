//! Native camera: viewfinder, capture workflow, device/settings and thumbnails.
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    i18n::I18n,
    NativeApp,
};
use serde_json::{json, Value};
enum Tag {
    Cameras,
    Dirs,
    Pictures,
    Folder,
    Open(u64),
    View(u64),
    Name(u64),
    Photo(u64),
    Video(u64),
    Stop(u64),
    Events,
    Thumb(String),
    Thumbs,
    Prefs,
    Ignore,
}
impl Tag {
    fn epoch(&self) -> Option<u64> {
        match self {
            Self::Open(e)
            | Self::View(e)
            | Self::Name(e)
            | Self::Photo(e)
            | Self::Video(e)
            | Self::Stop(e) => Some(*e),
            _ => None,
        }
    }
}
struct Target {
    base: String,
    ext: &'static str,
    path: String,
    attempt: u32,
    mirror: bool,
    video: bool,
}
struct Shot {
    path: String,
    thumb: Option<String>,
    pending: bool,
}
pub struct App {
    jobs: Jobs<Tag>,
    booted: bool,
    i18n: I18n,
    home: String,
    pictures: String,
    folder: String,
    folder_ready: bool,
    settings_seen: Value,
    prefs: Value,
    prefs_pending: usize,
    theme: Value,
    rtl: bool,
    cameras: Vec<Value>,
    device: Option<String>,
    id: i64,
    epoch: u64,
    opening: bool,
    denied: bool,
    error: Value,
    info: Value,
    mode: String,
    mirror: bool,
    target: Option<Target>,
    photo_pending: bool,
    video_pending: bool,
    video_completed: bool,
    stopping: bool,
    rec_start: Option<i64>,
    flash_until: i64,
    shots: Vec<Shot>,
    cameras_pending: bool,
    cameras_at: i64,
    events_pending: bool,
    events_at: i64,
    deferred: Vec<Value>,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            booted: false,
            i18n: I18n::default(),
            home: String::new(),
            pictures: String::new(),
            folder: String::new(),
            folder_ready: false,
            settings_seen: Value::Null,
            prefs: json!({}),
            prefs_pending: 0,
            theme: Value::Null,
            rtl: false,
            cameras: vec![],
            device: None,
            id: 0,
            epoch: 0,
            opening: false,
            denied: false,
            error: Value::Null,
            info: Value::Null,
            mode: "photo".into(),
            mirror: true,
            target: None,
            photo_pending: false,
            video_pending: false,
            video_completed: false,
            stopping: false,
            rec_start: None,
            flash_until: 0,
            shots: vec![],
            cameras_pending: false,
            cameras_at: 0,
            events_pending: false,
            events_at: 0,
            deferred: vec![],
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
        self.booted.then_some(
            if self.jobs.busy() || self.rec_start.is_some() || self.flash_until > 0 {
                50.
            } else {
                200.
            },
        )
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        (self.rec_start.is_some() || self.video_pending).then_some(200.)
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
    fn key(&mut self, key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        [" ", "Space", "Enter"]
            .contains(&key)
            .then(|| self.request(&json!({"cmd":"shoot"})))
    }
}
fn str_value(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn stamp(t: &libc::tm) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}.{:02}.{:02}",
        t.tm_year + 1900,
        t.tm_mon + 1,
        t.tm_mday,
        t.tm_hour,
        t.tm_min,
        t.tm_sec
    )
}
fn duration(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
impl App {
    fn close(&mut self, id: i64) {
        if id > 0 {
            self.jobs.enqueue(Tag::Ignore, "media.close", json!([id]));
        }
    }
    fn set_view(&mut self) {
        if self.id > 0 {
            self.jobs.enqueue(Tag::View(self.epoch),"media.view",json!([self.id,{"mirror":if self.mirror{1}else{0},"fit":"contain","background":0xff000000u32}]));
        }
    }
    fn switch(&mut self, device: Option<String>) {
        self.epoch += 1;
        self.jobs.cancel(|t| t.epoch().is_some());
        self.close(self.id);
        self.id = 0;
        self.device = device;
        self.info = Value::Null;
        self.error = Value::Null;
        self.denied = false;
        self.rec_start = None;
        self.video_pending = false;
        self.video_completed = false;
        self.photo_pending = false;
        self.stopping = false;
        self.target = None;
        self.flash_until = 0;
        self.opening = self.device.is_some();
        if let Some(device) = &self.device {
            self.jobs.enqueue(
                Tag::Open(self.epoch),
                "media.camera",
                json!(["camera",device,{"width":1280,"height":720}]),
            );
        }
    }
    fn save_pref(&mut self, key: &str, value: Value) {
        self.prefs[key] = value;
        self.prefs_pending += 1;
        self.jobs
            .enqueue(Tag::Prefs, "settings.patch", json!([{"camera":self.prefs}]));
    }
    fn mirror(&mut self, value: bool) {
        if self.mirror != value {
            self.mirror = value;
            self.set_view();
        }
    }
    fn request_thumbs(&mut self) {
        for shot in self.shots.iter_mut().take(3) {
            if shot.thumb.is_none() && !shot.pending {
                shot.pending = true;
                self.jobs.enqueue(
                    Tag::Thumb(shot.path.clone()),
                    "media.thumb",
                    json!([shot.path, 128]),
                );
            }
        }
    }
    fn add_shot(&mut self, path: &str) {
        if path.is_empty() {
            return;
        }
        self.shots.retain(|s| s.path != path);
        self.shots.insert(
            0,
            Shot {
                path: path.into(),
                thumb: None,
                pending: false,
            },
        );
        self.shots.truncate(12);
        self.request_thumbs();
    }
    fn check_name(&mut self) {
        if let Some(t) = &mut self.target {
            let suffix = if t.attempt == 1 {
                String::new()
            } else {
                format!(" ({})", t.attempt)
            };
            t.path = format!("{}/{}{suffix}.{}", self.folder, t.base, t.ext);
            self.jobs
                .enqueue(Tag::Name(self.epoch), "fs.stat", json!([t.path]));
        }
    }
    fn shoot(&mut self, now: i64) {
        if self.id <= 0 || !self.folder_ready || self.opening {
            return;
        }
        if self.rec_start.is_some() {
            if !self.stopping {
                self.stopping = true;
                self.jobs.enqueue(
                    Tag::Stop(self.epoch),
                    "media.cameraRecord",
                    json!([self.id, "camera", null]),
                );
            }
            return;
        }
        if self.target.is_some() || self.photo_pending || self.video_pending {
            return;
        }
        let video = self.mode == "video";
        let base=self.i18n.caption(&json!({"key":if video{"camera.videoName"}else{"camera.photoName"},"params":{"date":stamp(&crate::clock::local(now))}})).replace(['/', '\\','\0'],"-");
        self.target = Some(Target {
            base,
            ext: if video { "mp4" } else { "jpg" },
            path: String::new(),
            attempt: 1,
            mirror: self.mirror,
            video,
        });
        self.video_completed = false;
        self.check_name();
    }
    fn event(&mut self, event: &Value, now: i64) {
        if event["kind"] == "thumb" {
            self.request_thumbs();
            return;
        }
        if event["kind"] != "media" {
            return;
        }
        let id = event["id"].as_i64().unwrap_or(0);
        if id <= 0 {
            return;
        }
        if id != self.id {
            if self.opening && self.deferred.len() < 256 {
                self.deferred.push(event.clone());
            }
            return;
        }
        let data = &event["data"];
        match str_value(&event["type"]) {
            "ready" | "info" => self.info = data.clone(),
            "photo" => {
                if data["ok"] == true {
                    self.add_shot(str_value(&data["path"]));
                }
            }
            "video" => {
                if data["ok"] == true {
                    self.add_shot(str_value(&data["path"]));
                }
                self.video_completed = true;
                self.video_pending = false;
                self.rec_start = None;
                self.stopping = false;
            }
            "error" => self.error = json!({"error":data["message"]}),
            "closed" => {
                self.close(self.id);
                self.id = 0;
                self.rec_start = None;
                self.video_pending = false;
                self.photo_pending = false;
                self.stopping = false;
                self.target = None;
                self.error =
                    json!({"error":if data["crashed"]==true{"crash"}else{"media.noHelper"}});
            }
            "state" if data["state"] == "failed" => {
                if self.error.is_null() {
                    self.error = json!({"error":"media.noHelper"});
                }
            }
            _ => {}
        }
        let _ = now;
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        let Completed { tag, result } = done;
        if tag.epoch().is_some_and(|e| e != self.epoch) {
            if matches!(tag, Tag::Open(_)) {
                if let Ok(id) = result {
                    self.close(id.as_i64().unwrap_or(0));
                }
            }
            return;
        }
        match tag {
            Tag::Cameras => {
                self.cameras_pending = false;
                self.cameras_at = now + 3000;
                if let Ok(v) = result {
                    let previous = self
                        .cameras
                        .iter()
                        .any(|c| Some(str_value(&c["device"])) == self.device.as_deref());
                    self.cameras = v
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|c| c["device"].is_string())
                        .cloned()
                        .collect();
                    if self.device.is_none() {
                        let chosen = self.prefs["device"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .or_else(|| {
                                self.cameras.first().map(|c| str_value(&c["device"]).into())
                            });
                        if chosen.is_some() {
                            self.switch(chosen);
                        }
                    } else if !previous
                        && self.id == 0
                        && !self.opening
                        && self
                            .cameras
                            .iter()
                            .any(|c| Some(str_value(&c["device"])) == self.device.as_deref())
                    {
                        self.switch(self.device.clone());
                    }
                }
            }
            Tag::Dirs => {
                self.pictures = result
                    .ok()
                    .and_then(|v| v["pictures"].as_str().map(str::to_owned))
                    .filter(|s| s.starts_with('/'))
                    .unwrap_or_else(|| format!("{}/Pictures", self.home));
                let folder = self
                    .i18n
                    .caption(&json!({"key":"camera.folder"}))
                    .replace(['/', '\\', '\0'], "-");
                self.folder = format!("{}/{folder}", self.pictures);
                self.jobs
                    .enqueue(Tag::Pictures, "fs.mkdir", json!([self.pictures]));
            }
            Tag::Pictures => self
                .jobs
                .enqueue(Tag::Folder, "fs.mkdir", json!([self.folder])),
            Tag::Folder => self.folder_ready = true,
            Tag::Open(_) => {
                self.opening = false;
                match result {
                    Ok(v) => {
                        self.id = v
                            .as_i64()
                            .filter(|id| *id > 0 && *id <= i32::MAX as i64)
                            .unwrap_or(0);
                        self.denied = self.id == 0;
                        self.set_view();
                    }
                    Err(e) => {
                        self.id = 0;
                        self.error = json!({"error":e});
                    }
                }
                let events = std::mem::take(&mut self.deferred);
                for event in events {
                    self.event(&event, now);
                }
            }
            Tag::View(_) => {
                if let Err(e) = result {
                    self.error = json!({"error":e});
                }
            }
            Tag::Name(_) => {
                if result.is_ok() {
                    if let Some(t) = &mut self.target {
                        t.attempt += 1;
                        if t.attempt > 999 {
                            self.target = None;
                            self.error = json!({"error":"fs.exists"});
                            return;
                        }
                    }
                    self.check_name();
                } else if let Some(target) = self.target.take() {
                    if target.video {
                        self.video_pending = true;
                        self.jobs.enqueue(Tag::Video(self.epoch),"media.cameraRecord",json!([self.id,"camera",target.path,{"mic":true,"mirror":target.mirror,"appName":self.i18n.caption(&json!({"key":"apps.camera"}))}]));
                    } else {
                        self.photo_pending = true;
                        self.flash_until = now + 160;
                        self.jobs.enqueue(
                            Tag::Photo(self.epoch),
                            "media.photo",
                            json!([self.id, target.path, target.mirror]),
                        );
                    }
                }
            }
            Tag::Photo(_) => {
                self.photo_pending = false;
                if let Err(e) = result {
                    self.error = json!({"error":e});
                }
            }
            Tag::Video(_) => {
                self.video_pending = false;
                match result {
                    Ok(v) if v == true => {
                        if !self.video_completed {
                            self.rec_start = Some(now);
                        }
                    }
                    Err(e) => self.error = json!({"error":e}),
                    _ => {}
                }
            }
            Tag::Stop(_) => {
                if result.as_ref().is_err() || result.as_ref().is_ok_and(|v| *v == false) {
                    self.stopping = false;
                    if let Err(e) = result {
                        self.error = json!({"error":e});
                    }
                }
            }
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + 100;
                if let Ok(events) = result {
                    for event in events.as_array().into_iter().flatten() {
                        self.event(event, now);
                    }
                }
            }
            Tag::Thumb(path) => {
                if let Some(shot) = self.shots.iter_mut().find(|s| s.path == path) {
                    shot.pending = false;
                    shot.thumb = result
                        .ok()
                        .and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned));
                }
            }
            Tag::Prefs => {
                self.prefs_pending = self.prefs_pending.saturating_sub(1);
                if self.prefs_pending == 0 {
                    if let Ok(v) = result {
                        if v["camera"].is_object() {
                            self.prefs = v["camera"].clone();
                            self.mirror(self.prefs["mirror"] != false);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.theme = input["context"]["theme"].clone();
                let lang = self.i18n.lang.clone();
                self.rtl = self.i18n.entry(&lang, "meta", "direction") == Some(json!("rtl"));
                let prefs = if input["settings"]["camera"].is_object() {
                    input["settings"]["camera"].clone()
                } else {
                    json!({})
                };
                if prefs != self.settings_seen {
                    self.settings_seen = prefs.clone();
                    if self.prefs_pending == 0 {
                        self.prefs = prefs;
                        self.mirror(self.prefs["mirror"] != false);
                    }
                }
                if !self.booted {
                    if let Some(owner) =
                        input["args"]["__nativeOwner"].as_i64().filter(|id| *id > 0)
                    {
                        self.jobs.bind(owner);
                        self.booted = true;
                        self.jobs.enqueue(Tag::Dirs, "media.dirs", json!([]));
                        self.jobs.enqueue(Tag::Thumbs, "media.thumbs", json!([]));
                    }
                }
                if self.booted && !self.cameras_pending && now >= self.cameras_at {
                    self.cameras_pending = true;
                    self.jobs.enqueue(Tag::Cameras, "media.cameras", json!([]));
                }
                if self.booted && !self.events_pending && now >= self.events_at {
                    self.events_pending = true;
                    self.jobs.enqueue(Tag::Events, "media.events", json!([]));
                }
                if self.flash_until > 0 && now >= self.flash_until {
                    self.flash_until = 0;
                }
                let tree = ui::tree(self, now);
                let mut reply = if tree == self.last_tree {
                    json!({"same":true})
                } else {
                    self.last_tree = tree.clone();
                    json!({"tree":tree})
                };
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
            "shoot" => self.shoot(now),
            "mode" => {
                if self.rec_start.is_none() && !self.video_pending && self.target.is_none() {
                    if let Some(mode) = input["key"]
                        .as_str()
                        .filter(|s| ["photo", "video"].contains(s))
                    {
                        self.mode = mode.into();
                    }
                }
            }
            "mirror" => {
                self.mirror(!self.mirror);
                self.save_pref("mirror", json!(self.mirror));
            }
            "device" => {
                if let Some(device) = input["key"]
                    .as_str()
                    .filter(|s| self.cameras.iter().any(|c| c["device"] == *s))
                {
                    if self.device.as_deref() != Some(device) {
                        self.switch(Some(device.into()));
                        self.save_pref("device", json!(device));
                    }
                }
            }
            "openShot" => {
                if let Some(path) = input["key"]
                    .as_str()
                    .filter(|p| self.shots.iter().any(|s| s.path == *p))
                {
                    self.jobs.enqueue(Tag::Ignore,"windows.open",json!([if path.ends_with(".mp4"){"player"}else{"photos"},{"new":true,"args":{"path":path}}]));
                }
            }
            _ => {}
        }
        self.jobs
            .pump(now)
            .map(|effect| json!({"effect":effect}))
            .unwrap_or_else(|| json!({}))
    }
}
