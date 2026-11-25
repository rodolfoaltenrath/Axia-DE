//! Native Player: library, queue, video and transport, mounted straight into the Rust scene.
//!
//! Rust chooses every job; the SDK services run them (`media.*`, `windows.screen`, `windows.pip`).
//! The library walk and the tag cache are read on a worker thread (`scan.rs`); `model.rs` holds the
//! rules that need no window. A window owns its player session: closing the window (or opening
//! another track) closes it, and a late result for a track that was replaced is closed on arrival.
mod model;
mod scan;
mod ui;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod tests;
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    i18n::I18n,
    KeyFacts, NativeApp,
};
pub use model::*;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Receiver;

enum Tag {
    Dirs,
    Thumbs,
    Events,
    Probe(Found),
    Open(u64),
    Thumb(String),
    Screen,
    Ignore,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    #[default]
    Songs,
    Artists,
    Albums,
    Videos,
    Queue,
}
impl View {
    fn parse(name: &str) -> Option<View> {
        Some(match name {
            "songs" => View::Songs,
            "artists" => View::Artists,
            "albums" => View::Albums,
            "videos" => View::Videos,
            "queue" => View::Queue,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            View::Songs => "songs",
            View::Artists => "artists",
            View::Albums => "albums",
            View::Videos => "videos",
            View::Queue => "queue",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Menu {
    #[default]
    Closed,
    Subs,
    Audio,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Time,
    Volume,
}
/// Height of a song row and of the title and table header above the list.
const ROW: f64 = 30.;
const MAX_REMEMBERED_QUEUE: usize = 500;
/// Thumbnails asked at the same time.
const THUMBS_IN_FLIGHT: usize = 6;
const PROBES_IN_FLIGHT: usize = 2;

#[derive(Default)]
pub struct App {
    jobs: Jobs<Tag>,
    i18n: I18n,
    theme: std::rc::Rc<Value>,
    rtl: bool,
    booted: bool,
    home: String,
    window: String,
    window_mode: String,
    width: f64,
    height: f64,
    args_seen: Value,
    // library
    tracks: Vec<Track>,
    videos: Vec<Video>,
    albums: Vec<Album>,
    artists: Vec<Artist>,
    songs: Vec<Track>,
    lib_dirty: bool,
    lib_at: i64,
    listing: Option<Receiver<scan::Listed>>,
    todo: VecDeque<Found>,
    probing: usize,
    probe_done: usize,
    probe_total: usize,
    read: usize,
    cached: usize,
    kept: usize,
    scanning: bool,
    // view
    view: View,
    sel: Option<String>,
    offset: f64,
    show_video: bool,
    menu: Menu,
    last_move: i64,
    seek_preview: Option<f64>,
    drag: Option<Drag>,
    // playback
    queue: Queue,
    volume: Option<f64>,
    id: i64,
    epoch: u64,
    opening: bool,
    open_for: Option<String>,
    state: PlayerState,
    deferred: Vec<Value>,
    ext_subs: Option<Vec<Cue>>,
    sub_stream: Option<i64>,
    restore: Option<(f64, bool)>,
    // thumbnails: None is asked for and not made yet
    thumbs: HashMap<String, Option<String>>,
    thumbs_pending: usize,
    events_pending: bool,
    events_at: i64,
    now_sent: Value,
    title: String,
    checkpoint_key: String,
    checkpoint_bucket: i64,
    effects: Vec<Value>,
    last_tree: Value,
    seed: u64,
}

impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool { true }
    fn set_native_appearance(&mut self, theme: std::rc::Rc<Value>) { self.theme = theme; }
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
        self.home = home.unwrap_or("").trim_end_matches('/').into();
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted.then_some(if self.jobs.busy() || self.listing.is_some() || self.probing > 0 {
            50.
        } else if self.state.playing {
            100.
        } else {
            250.
        })
    }
    /// A playing track must go on to the next one even when its window is not in front.
    fn background_refresh_ms(&self) -> Option<f64> {
        (self.id > 0 && self.state.playing).then_some(200.)
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
    fn key_event(&mut self, key: &str, chr: &str, flags: i32, facts: &KeyFacts) -> Option<Value> {
        self.handle_key(key, chr, flags, facts).then(|| self.output(crate::clock::now()))
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        self.key_event(key, chr, flags, &KeyFacts::default())
    }
}

fn str_value(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
impl App {
    fn tr(&mut self, key: &str) -> String {
        self.i18n.caption(&json!({"key": key}))
    }
    fn volume(&self) -> f64 {
        self.volume.unwrap_or(1.)
    }
    fn current(&self) -> Option<String> {
        self.queue.current().map(str::to_owned)
    }
    fn mode_is_window(&self) -> bool {
        matches!(self.window_mode.as_str(), "" | "window")
    }
    /// The video picture fills the window (a video is current and asked for, or the window is
    /// full screen or picture in picture).
    fn video_view(&self) -> bool {
        self.queue.current().is_some_and(is_video) && (self.show_video || !self.mode_is_window())
    }
    fn narrow(&self) -> bool {
        self.width < 720.
    }
    fn control(&mut self, command: &'static str, value: Value) {
        if self.id > 0 {
            self.jobs.enqueue(Tag::Ignore, "media.control", json!([self.id, command, value]));
        }
    }
    fn close(&mut self, id: i64) {
        if id > 0 {
            self.jobs.enqueue(Tag::Ignore, "media.close", json!([id]));
        }
    }
    fn toggle(&mut self) {
        self.control(if self.state.playing { "pause" } else { "play" }, Value::Null);
    }
    fn seek(&mut self, ms: f64) {
        self.control("seek", json!(ms.round()));
    }
    fn change_volume(&mut self, volume: f64) {
        self.volume = Some(volume);
        self.control("volume", json!(volume));
    }
    fn pick(&mut self, n: usize) -> usize {
        // a small generator is enough to shuffle a queue
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407 ^ crate::clock::now() as u64);
        ((self.seed >> 33) as usize) % n.max(1)
    }
    fn play_list(&mut self, paths: Vec<String>, index: usize) {
        self.show_video = paths.get(index).is_some_and(|p| is_video(p));
        // a new video starts with its controls showing
        self.last_move = crate::clock::now();
        self.queue.play_list(paths, index);
    }
    fn advance(&mut self, step: Advance) {
        match step {
            Advance::Stay => {}
            Advance::Restart => {
                self.control("seek", json!(0));
                self.control("play", Value::Null);
            }
            Advance::Go(n) if n as i64 == self.queue.index => {
                // the same entry again (a one-track queue): the open session starts over
                self.advance(Advance::Restart)
            }
            Advance::Go(n) => self.queue.index = n as i64,
        }
    }
    fn next(&mut self, auto: bool) {
        let n = self.queue.paths.len();
        let shuffled = if self.queue.shuffle { self.pick(n) } else { 0 };
        let step = self.queue.next(auto, |_| shuffled);
        self.advance(step);
    }
    fn prev(&mut self) {
        let step = self.queue.prev(self.state.pos);
        self.advance(step);
    }
    fn rebuild(&mut self, now: i64) {
        self.albums = albums_of(&self.tracks);
        self.artists = artists_of(&self.tracks);
        self.songs = songs_sorted(&self.tracks);
        self.lib_dirty = false;
        self.lib_at = now;
    }
    fn track_of(&self, path: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.path == path)
    }
    /// What a click on row `i` plays: the paths the current page lists.
    fn list_paths(&self) -> Vec<String> {
        match (self.view, &self.sel) {
            (View::Songs, _) => self.songs.iter().map(|t| t.path.clone()).collect(),
            (View::Albums, Some(key)) => self
                .albums
                .iter()
                .find(|a| a.key == *key)
                .map(|a| a.tracks.iter().map(|t| t.path.clone()).collect())
                .unwrap_or_default(),
            (View::Videos, _) => self.videos.iter().map(|v| v.path.clone()).collect(),
            (View::Queue, _) => self.queue.paths.clone(),
            _ => vec![],
        }
    }
    fn list_height(&self) -> f64 {
        let bottom = if self.queue.current().is_some() { 72. } else { 0. };
        (self.height.max(1.) - 52. - ROW - 4. - bottom).max(0.)
    }
    fn max_offset(&self) -> f64 {
        let rows = match self.view {
            View::Queue => self.queue.paths.len(),
            _ => self.songs.len(),
        };
        (rows as f64 * ROW - self.list_height()).max(0.)
    }
    // ----------------------------------------------------------- player session
    /// Open the track the queue points at (and close the one that was open).
    fn open_current(&mut self) {
        let current = self.current();
        if current == self.open_for {
            return;
        }
        self.epoch += 1;
        self.jobs.cancel(|t| matches!(t, Tag::Open(_)));
        self.close(self.id);
        self.id = 0;
        self.opening = false;
        self.state = PlayerState::default();
        self.deferred.clear();
        self.ext_subs = None;
        self.sub_stream = None;
        self.seek_preview = None;
        self.open_for = current.clone();
        let Some(path) = current else { return };
        self.opening = true;
        let app_name = self.tr("apps.player");
        self.jobs.enqueue(
            Tag::Open(self.epoch),
            "media.play",
            json!(["player", path, {"appName":app_name,"music":is_audio(&path),"volume":self.volume()}]),
        );
        if is_video(&path) {
            let text = axia_native::read(std::path::Path::new(&srt_path(&path)), 4 << 20);
            let cues = parse_srt(&text);
            self.ext_subs = (!cues.is_empty()).then_some(cues);
        }
    }
    fn restore_position(&mut self) {
        if let Some((pos, playing)) = self.restore.take() {
            if pos > 0. {
                self.seek(pos);
            }
            if !playing {
                self.control("pause", Value::Null);
            }
        }
    }
    fn media_event(&mut self, kind: &str, data: &Value) {
        let was_ended = self.state.state == "ended";
        self.state.reduce(kind, data);
        if kind == "ready" {
            self.restore_position();
        }
        if self.state.state == "ended" && !was_ended {
            self.next(true);
        }
    }
    fn event(&mut self, event: &Value) {
        match str_value(&event["kind"]) {
            "thumb" => self.thumbs.retain(|_, v| v.is_some()),
            "nowPlaying" => match str_value(&event["action"]) {
                "toggle" => self.toggle(),
                "next" => self.next(false),
                "prev" => self.prev(),
                "stop" => self.control("pause", Value::Null),
                _ => {}
            },
            "media" => {
                let id = event["id"].as_i64().unwrap_or(0);
                if id <= 0 {
                    return;
                }
                if id != self.id {
                    if self.opening && self.deferred.len() < 512 {
                        self.deferred.push(event.clone());
                    }
                    return;
                }
                self.media_event(str_value(&event["type"]), &event["data"]);
            }
            _ => {}
        }
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        let Completed { tag, result } = done;
        match tag {
            Tag::Dirs => {
                let (music, videos) = match &result {
                    Ok(v) => (str_value(&v["music"]).to_owned(), str_value(&v["videos"]).to_owned()),
                    Err(_) => (String::new(), String::new()),
                };
                let or = |dir: String, name: &str| if dir.starts_with('/') { dir } else { format!("{}/{name}", self.home) };
                self.listing = Some(scan::start(self.home.clone(), or(music, "Music"), or(videos, "Videos")));
            }
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + if self.state.playing { 100 } else { 200 };
                if let Ok(events) = result {
                    for event in events.as_array().into_iter().flatten() {
                        self.event(event);
                    }
                }
            }
            Tag::Probe(file) => {
                self.probing = self.probing.saturating_sub(1);
                self.probe_done += 1;
                let info = result.unwrap_or(Value::Null);
                self.tracks.push(track_from(&file, &info));
                self.read += 1;
                self.lib_dirty = true;
                if self.todo.is_empty() && self.probing == 0 {
                    self.scanning = false;
                    if cache_dirty(self.read, self.cached, self.kept) {
                        scan::write_cache(&self.home, &self.tracks);
                    }
                    self.rebuild(now);
                }
            }
            Tag::Open(epoch) => {
                if epoch != self.epoch {
                    // the track was replaced while it opened: its session must not stay open
                    if let Ok(id) = result {
                        self.close(id.as_i64().unwrap_or(0));
                    }
                    return;
                }
                self.opening = false;
                match result {
                    Ok(v) => {
                        self.id = v.as_i64().filter(|id| *id > 0 && *id <= i32::MAX as i64).unwrap_or(0);
                        if self.id == 0 {
                            self.state.error = Some("media.noHelper".into());
                        }
                    }
                    Err(e) => self.state.error = Some(e),
                }
                for event in std::mem::take(&mut self.deferred) {
                    self.event(&event);
                }
            }
            Tag::Thumb(key) => {
                self.thumbs_pending = self.thumbs_pending.saturating_sub(1);
                let src = result.ok().and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned));
                if src.is_some() {
                    self.thumbs.insert(key, src);
                }
            }
            Tag::Screen => {
                if let Ok(screen) = result {
                    let frame = pip_frame(
                        screen["width"].as_f64().unwrap_or(0.),
                        screen["height"].as_f64().unwrap_or(0.),
                        self.state.video_ratio(),
                    );
                    self.jobs.enqueue(Tag::Ignore, "windows.pip", json!([self.window, frame]));
                }
            }
            Tag::Thumbs | Tag::Ignore => {}
        }
    }
    // --------------------------------------------------------- window effects
    fn fullscreen(&mut self) {
        self.checkpoint(true);
        let on = self.window_mode != "fullscreen";
        self.effects.push(json!({"host":true,"kind":"fullscreen","on":on}));
    }
    fn pip(&mut self) {
        if self.window.is_empty() {
            return;
        }
        if self.window_mode == "pip" {
            self.jobs.enqueue(Tag::Ignore, "windows.pip", json!([self.window, null]));
        } else {
            self.checkpoint(true);
            self.jobs.enqueue(Tag::Screen, "windows.screen", json!([]));
        }
    }
    /// What survives a remount of the window (full screen and picture in picture mount it again).
    fn checkpoint(&mut self, force: bool) {
        if self.window.is_empty() {
            return;
        }
        let paths: Vec<&String> = if self.queue.paths.len() <= MAX_REMEMBERED_QUEUE {
            self.queue.paths.iter().collect()
        } else {
            self.queue.current().map(|_| &self.queue.paths[self.queue.index as usize]).into_iter().collect()
        };
        let index = if self.queue.paths.len() <= MAX_REMEMBERED_QUEUE { self.queue.index } else { 0 };
        let mut state = json!({"sourcePath":self.args_seen["path"],"nonce":self.args_seen["nonce"],
            "queue":paths,"index":index,"shuffle":self.queue.shuffle,"repeat":self.queue.repeat.name(),
            "view":self.view.name(),"sel":self.sel,"volume":self.volume(),"showVideo":self.show_video,
            "playing":self.state.playing});
        let key = state.to_string();
        let bucket = if self.state.playing { (self.state.pos / 5000.) as i64 } else { -1 };
        if !force && key == self.checkpoint_key && bucket == self.checkpoint_bucket {
            return;
        }
        self.checkpoint_key = key;
        self.checkpoint_bucket = bucket;
        state["pos"] = json!(self.state.pos);
        self.effects.push(json!({"host":true,"kind":"windowState","state":state}));
    }
    fn restore_state(&mut self, state: &Value, args: &Value) {
        if !state.is_object() || state["sourcePath"] != args["path"] || state["nonce"] != args["nonce"] {
            return;
        }
        let paths: Vec<String> = state["queue"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect();
        if paths.is_empty() {
            return;
        }
        let index = state["index"].as_u64().unwrap_or(0) as usize;
        self.queue.play_list(paths, index);
        self.queue.shuffle = state["shuffle"] == true;
        self.queue.repeat = Repeat::parse(str_value(&state["repeat"]));
        self.view = View::parse(str_value(&state["view"])).unwrap_or_default();
        self.sel = state["sel"].as_str().map(str::to_owned);
        self.volume = state["volume"].as_f64().map(|v| v.clamp(0., VOLUME_MAX));
        self.show_video = state["showVideo"] == true;
        if self.queue.current().is_some() {
            self.restore = Some((state["pos"].as_f64().unwrap_or(0.), state["playing"] != false));
        }
    }
    // ----------------------------------------------------------- thumbnails
    /// The images the current page shows: (path, size).
    fn wanted_thumbs(&self) -> Vec<(String, u32)> {
        let mut out: Vec<(String, u32)> = vec![];
        if let Some(path) = self.queue.current() {
            out.push((path.to_owned(), 256));
        }
        match (self.view, &self.sel) {
            (View::Albums, None) => out.extend(self.albums.iter().map(|a| (a.tracks[0].path.clone(), 256))),
            (View::Albums, Some(key)) => out.extend(self.albums.iter().filter(|a| a.key == *key).map(|a| (a.tracks[0].path.clone(), 256))),
            (View::Artists, None) => out.extend(self.artists.iter().map(|a| (a.tracks[0].path.clone(), 256))),
            (View::Artists, Some(name)) => out.extend(albums_of_artist(&self.albums, name).iter().map(|a| (a.tracks[0].path.clone(), 256))),
            (View::Videos, _) => out.extend(self.videos.iter().map(|v| (v.path.clone(), 320))),
            _ => {}
        }
        out
    }
    fn thumb_key(path: &str, size: u32) -> String {
        format!("{size}:{path}")
    }
    fn thumb(&self, path: &str, size: u32) -> Option<&str> {
        self.thumbs.get(&Self::thumb_key(path, size)).and_then(|v| v.as_deref())
    }
    fn request_thumbs(&mut self) {
        if self.thumbs_pending >= THUMBS_IN_FLIGHT {
            return;
        }
        for (path, size) in self.wanted_thumbs() {
            if self.thumbs_pending >= THUMBS_IN_FLIGHT {
                break;
            }
            let key = Self::thumb_key(&path, size);
            if self.thumbs.contains_key(&key) {
                continue;
            }
            self.thumbs.insert(key.clone(), None);
            self.thumbs_pending += 1;
            self.jobs.enqueue(Tag::Thumb(key), "media.thumb", json!([path, size]));
        }
    }
    // ---------------------------------------------------- Control Center entry
    fn publish_now_playing(&mut self) {
        let payload = match (self.queue.current(), self.id > 0) {
            (Some(path), true) => {
                let cover = self.thumb(path, 256).map(str::to_owned);
                let mut value = now_playing(&self.window, self.track_of(path), path, &self.state, cover.as_deref());
                // the entry changes each second, not each frame
                value["pos"] = json!((self.state.pos / 1000.).floor() * 1000.);
                value
            }
            _ => Value::Null,
        };
        if payload != self.now_sent {
            self.now_sent = payload.clone();
            self.jobs.enqueue(Tag::Ignore, "media.nowPlaying", json!([payload]));
        }
    }
    // ------------------------------------------------------------- sync
    fn poll_library(&mut self, now: i64) {
        if let Some(rx) = &self.listing {
            if let Ok(listed) = rx.try_recv() {
                self.listing = None;
                self.kept = listed.known.len();
                self.cached = listed.cached;
                self.tracks = listed.known;
                self.videos = listed.videos;
                self.probe_total = listed.todo.len();
                self.probe_done = 0;
                self.read = 0;
                self.scanning = !listed.todo.is_empty();
                self.todo = listed.todo.into();
                self.rebuild(now);
                if !self.scanning && cache_dirty(0, self.cached, self.kept) {
                    scan::write_cache(&self.home, &self.tracks);
                }
            }
        }
        while self.probing < PROBES_IN_FLIGHT {
            let Some(file) = self.todo.pop_front() else { break };
            self.probing += 1;
            self.jobs.enqueue(Tag::Probe(file.clone()), "media.probe", json!([file.path]));
        }
        // a long scan sorts at most once a second
        if self.lib_dirty && (!self.scanning || now - self.lib_at >= 1000) {
            self.rebuild(now);
        }
    }
    fn sync(&mut self, input: &Value, now: i64) -> Value {
        self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
        let context = &input["context"];
        if context["theme"].is_object() {
            self.theme = std::rc::Rc::new(context["theme"].clone());
        }
        let lang = self.i18n.lang.clone();
        self.rtl = self.i18n.entry(&lang, "meta", "direction") == Some(json!("rtl"));
        self.window_mode = context["windowMode"].as_str().unwrap_or("window").into();
        self.width = context["width"].as_f64().filter(|w| *w > 0.).unwrap_or(960.);
        self.height = context["height"].as_f64().filter(|h| *h > 0.).unwrap_or(600.);
        let args = &input["args"];
        self.window = args["__nativeWindow"].as_str().unwrap_or("").into();
        if !self.booted {
            let Some(owner) = args["__nativeOwner"].as_i64().filter(|id| *id > 0) else {
                return json!({"same":true});
            };
            self.jobs.bind(owner);
            self.booted = true;
            self.jobs.enqueue(Tag::Dirs, "media.dirs", json!([]));
            self.jobs.enqueue(Tag::Thumbs, "media.thumbs", json!([]));
        }
        // "Open with" from Files, once per request
        let fresh = args["path"].as_str().is_some()
            && (args["path"] != self.args_seen["path"] || args["nonce"] != self.args_seen["nonce"]);
        if fresh {
            let path = str_value(&args["path"]).to_owned();
            self.play_list(vec![path], 0);
        }
        let first = self.args_seen.is_null();
        self.args_seen = json!({"path":args["path"],"nonce":args["nonce"]});
        if first {
            self.restore_state(&args["__nativeState"], args);
        }
        self.poll_library(now);
        self.open_current();
        if self.booted && !self.events_pending && now >= self.events_at {
            self.events_pending = true;
            self.jobs.enqueue(Tag::Events, "media.events", json!([]));
        }
        self.offset = self.offset.clamp(0., self.max_offset());
        self.request_thumbs();
        self.publish_now_playing();
        self.checkpoint(false);
        let mut out = json!({});
        let title = self.queue.current().map(|p| base_name(p).to_owned()).unwrap_or_default();
        if !title.is_empty() && title != self.title {
            self.title = title.clone();
            out["title"] = json!(title);
        }
        let tree = ui::tree(self, now);
        if tree == self.last_tree {
            out["same"] = json!(true);
        } else {
            self.last_tree = tree.clone();
            out["tree"] = tree;
        }
        self.finish(out, now)
    }
    /// The reply of a request: window effects, the job to run next and whether the pointer is captured.
    fn finish(&mut self, mut out: Value, now: i64) -> Value {
        out["capture"] = json!(self.drag.is_some());
        let job = self.jobs.pump(now);
        let mut effects = std::mem::take(&mut self.effects);
        if let Some(effect) = job {
            effects.push(effect);
        }
        if !effects.is_empty() {
            out["effects"] = json!(effects);
        }
        out
    }
    fn output(&mut self, now: i64) -> Value {
        self.finish(json!({}), now)
    }
    fn bar_fraction(&self, x: f64, bar: (f64, f64)) -> f64 {
        bar_fraction(x, bar.0, bar.1)
    }
    fn pointer(&mut self, x: f64) {
        let geo = ui::geometry(self);
        match self.drag {
            Some(Drag::Time) => {
                let f = self.bar_fraction(x, geo.time);
                self.seek_preview = Some(f * self.state.duration);
            }
            Some(Drag::Volume) => {
                if let Some(bar) = geo.volume {
                    let f = self.bar_fraction(x, bar);
                    self.change_volume(volume_from_fraction(f));
                }
            }
            None => {}
        }
    }
    /// Space, the arrows and F. Ctrl, Alt and Super combinations stay with the shell.
    fn handle_key(&mut self, key: &str, chr: &str, flags: i32, facts: &KeyFacts) -> bool {
        let base = if !facts.base.is_empty() { facts.base } else if chr.is_empty() { key } else { chr };
        if flags & (2 | 4 | 16) != 0 {
            return false;
        }
        match key {
            " " | "space" | "Space" => self.toggle(),
            "ArrowRight" | "Right" => self.seek(seek_by(self.state.pos, true, facts.shift)),
            "ArrowLeft" | "Left" => self.seek(seek_by(self.state.pos, false, facts.shift)),
            "ArrowUp" | "Up" => self.change_volume(volume_step(self.volume(), true)),
            "ArrowDown" | "Down" => self.change_volume(volume_step(self.volume(), false)),
            _ if base.eq_ignore_ascii_case("f") && self.state.has_video() => self.fullscreen(),
            _ => return false,
        }
        true
    }
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        match self.dispatch(input, now) {
            Some(reply) => reply,
            None => self.output(now),
        }
    }
    /// A command's effect on the state. `Some` is a reply that is complete (the icon, a sync);
    /// otherwise the caller adds the pending job and the window effects.
    fn dispatch(&mut self, input: &Value, now: i64) -> Option<Value> {
        let key = &input["key"];
        match str_value(&input["cmd"]) {
            "icon" => {
                return Some(
                    axia_native::icons::app_icon("player", input["size"].as_f64().filter(|s| s.is_finite()).unwrap_or(32.).clamp(1., 512.))
                        .unwrap_or(Value::Null),
                )
            }
            "sync" => return Some(self.sync(input, now)),
            "next" => {
                for done in self.jobs.complete(&input["result"], now) {
                    self.apply(done, now);
                }
            }
            "nav" => {
                if let Some(view) = View::parse(str_value(key)) {
                    self.view = view;
                    if view != View::Queue {
                        self.sel = None;
                    }
                    self.offset = 0.;
                }
            }
            "artist" => self.sel = key.as_str().map(str::to_owned),
            "album" => {
                self.view = View::Albums;
                self.sel = key.as_str().map(str::to_owned);
            }
            "back" => self.sel = None,
            "row" => {
                let paths = self.list_paths();
                if let Some(i) = key.as_u64().map(|i| i as usize).filter(|i| *i < paths.len()) {
                    self.play_list(paths, i);
                }
            }
            "toggle" => self.toggle(),
            "prev" => self.prev(),
            "skip" => self.next(false),
            "shuffle" => self.queue.shuffle = !self.queue.shuffle,
            "repeat" => self.queue.repeat = self.queue.repeat.cycle(),
            "fullscreen" => self.fullscreen(),
            "pip" => self.pip(),
            "hideVideo" => self.show_video = false,
            "showVideo" => {
                self.show_video = true;
                self.last_move = now;
            }
            "menu" => {
                let menu = if key == "subs" { Menu::Subs } else { Menu::Audio };
                self.menu = if self.menu == menu { Menu::Closed } else { menu };
            }
            "dismiss" => self.menu = Menu::Closed,
            "subOff" => {
                self.sub_stream = None;
                self.ext_subs = None;
                self.control("subTrack", json!(-1));
                self.menu = Menu::Closed;
            }
            "subFile" => {
                self.sub_stream = None;
                self.control("subTrack", json!(-1));
                self.menu = Menu::Closed;
            }
            "subStream" => {
                if let Some(index) = key.as_i64() {
                    self.sub_stream = Some(index);
                    self.control("subTrack", json!(index));
                }
                self.menu = Menu::Closed;
            }
            "audioStream" => {
                if let Some(index) = key.as_i64() {
                    self.control("audioTrack", json!(index));
                }
                self.menu = Menu::Closed;
            }
            "poke" => self.last_move = now,
            "timeDown" | "volDown" => {
                self.drag = Some(if input["cmd"] == "timeDown" { Drag::Time } else { Drag::Volume });
                if let Some(x) = input["x"].as_f64() {
                    self.pointer(x);
                }
            }
            "pointerMove" => {
                if let Some(x) = input["x"].as_f64() {
                    self.pointer(x);
                }
            }
            "dragEnd" => {
                if self.drag.take() == Some(Drag::Time) {
                    if let Some(ms) = self.seek_preview.take() {
                        self.seek(ms);
                    }
                }
                self.seek_preview = None;
            }
            "scroll" => {
                let max = self.max_offset();
                self.offset = (self.offset + input["deltaY"].as_f64().unwrap_or(0.)).clamp(0., max);
            }
            "drop" => {
                let paths: Vec<String> = input["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str().filter(|p| is_media(p)).map(str::to_owned))
                    .collect();
                if !paths.is_empty() {
                    self.play_list(paths, 0);
                }
            }
            _ => {}
        }
        None
    }
}
