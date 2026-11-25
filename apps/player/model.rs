//! Player model: the library, the queue, the player state and the subtitles, without a window.
//!
//! Everything here is a port of `library.js` and of the pure parts of `index.jsx` (the JSX is
//! still the running code until the native window is registered). Nothing here talks to a
//! service: the window decides which job to ask for and feeds the results to these functions.
use serde_json::{json, Map, Value};
use std::cmp::Ordering;

/// Audio extensions the library and "Open with" accept (`AUDIO` of `library.js`).
pub const AUDIO_EXT: [&str; 13] = [
    "mp3", "ogg", "oga", "opus", "flac", "wav", "m4a", "aac", "wma", "aif", "aiff", "ape", "mka",
];
/// Video extensions (`VIDEO` of `library.js`).
pub const VIDEO_EXT: [&str; 13] = [
    "mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "flv", "mpg", "mpeg", "ogv", "ts", "3gp",
];
/// The walk of ~/Music and ~/Videos stops at this depth and at this many files per kind.
pub const WALK_DEPTH: usize = 6;
pub const WALK_LIMIT: usize = 5000;
/// Where the tags already read are kept, relative to the home folder.
pub const CACHE_FILE: &str = ".cache/axia/music.json";
pub const CACHE_DIR: &str = ".cache/axia";
/// Largest volume (the bar is 0..1.5, so the player can amplify by half).
pub const VOLUME_MAX: f64 = 1.5;
/// The video controls hide after this long without the pointer moving.
pub const CONTROLS_HIDE_MS: i64 = 2800;
/// A "previous" past this position goes back to the start of the track instead.
pub const RESTART_MS: f64 = 3000.;

/// `/\.(ext|…)$/i` over the whole string.
fn has_ext(path: &str, list: &[&str]) -> bool {
    match path.rfind('.') {
        Some(at) => {
            let ext = &path[at + 1..];
            !ext.is_empty() && list.iter().any(|e| ext.eq_ignore_ascii_case(e))
        }
        None => false,
    }
}
pub fn is_audio(path: &str) -> bool {
    has_ext(path, &AUDIO_EXT)
}
pub fn is_video(path: &str) -> bool {
    has_ext(path, &VIDEO_EXT)
}
/// What Player opens from "Open with" or a drop.
pub fn is_media(path: &str) -> bool {
    is_audio(path) || is_video(path)
}
pub fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("")
}
/// The file name without its last extension (`replace(/\.[^.]+$/, '')`). A dot file keeps its
/// name (the original gave an empty title).
pub fn stem(path: &str) -> &str {
    let name = base_name(path);
    match name.rfind('.') {
        Some(at) if at > 0 && at + 1 < name.len() => &name[..at],
        _ => name,
    }
}
/// The `.srt` next to a video: `replace(/\.[^./]+$/, '.srt')`.
pub fn srt_path(path: &str) -> String {
    if let Some(at) = path.rfind('.') {
        let tail = &path[at + 1..];
        if !tail.is_empty() && !tail.contains('/') {
            return format!("{}.srt", &path[..at]);
        }
    }
    path.to_owned()
}
/// `mm:ss`, or `h:mm:ss` from one hour (`mediaFormatTime`).
pub fn format_time(ms: f64) -> String {
    let ms = if ms.is_finite() { ms } else { 0. };
    let s = (ms / 1000.).floor().max(0.) as u64;
    let (h, m, s) = (s / 3600, s % 3600 / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

// ---------------------------------------------------------------- library

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub path: String,
    pub size: u64,
    pub mtime: f64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track: i64,
    pub year: String,
    pub genre: String,
    pub duration: f64,
    pub cover: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    pub path: String,
    pub title: String,
    pub size: u64,
    pub mtime: f64,
}
/// A file the walk found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub mtime: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Album {
    pub key: String,
    pub album: String,
    pub artist: String,
    pub year: String,
    pub tracks: Vec<Track>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Artist {
    pub name: String,
    pub tracks: Vec<Track>,
}

/// JavaScript truthiness of a JSON value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
/// JavaScript truthiness of a tag, as text: `tags.title || fallback`.
fn tag(info: &Value, name: &str) -> Option<String> {
    match &info["tags"][name] {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) if n.as_f64().is_some_and(|f| f != 0.) => Some(n.to_string()),
        _ => None,
    }
}
/// `parseInt(String(v).split('/')[0], 10)`, or None for NaN.
pub fn parse_int(text: &str) -> Option<i64> {
    let text = text.split('/').next().unwrap_or("").trim_start();
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    digits[..end].parse::<i64>().ok().map(|n| n * sign)
}
/// A track from the probe result, or from the file name when the file has no tags.
pub fn track_from(file: &Found, info: &Value) -> Track {
    let artist = tag(info, "artist");
    let album_artist = tag(info, "album_artist");
    let year: String = tag(info, "date").unwrap_or_default().chars().take(4).collect();
    Track {
        path: file.path.clone(),
        size: file.size,
        mtime: file.mtime,
        title: tag(info, "title").unwrap_or_else(|| stem(&file.path).to_owned()),
        artist: artist.clone().or_else(|| album_artist.clone()).unwrap_or_default(),
        album: tag(info, "album").unwrap_or_default(),
        album_artist: album_artist.or(artist).unwrap_or_default(),
        track: tag(info, "track").and_then(|t| parse_int(&t)).unwrap_or(0),
        year,
        genre: tag(info, "genre").unwrap_or_default(),
        duration: info["duration"].as_f64().filter(|d| d.is_finite()).unwrap_or(0.),
        cover: info["streams"]
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s["type"] == "video" && truthy(&s["cover"]))),
    }
}
pub fn video_from(file: &Found) -> Video {
    Video {
        path: file.path.clone(),
        title: stem(&file.path).to_owned(),
        size: file.size,
        mtime: file.mtime,
    }
}
/// `${size}:${Math.round(mtime)}`: a changed file is read again.
pub fn cache_key(file: &Found) -> String {
    format!("{}:{}", file.size, (file.mtime + 0.5).floor() as i64)
}
impl Track {
    /// The shape `library.js` stored, so an existing cache keeps working.
    pub fn to_json(&self) -> Value {
        json!({"path":self.path,"size":self.size,"mtime":self.mtime,"title":self.title,
            "artist":self.artist,"album":self.album,"albumArtist":self.album_artist,
            "track":self.track,"year":self.year,"genre":self.genre,"duration":self.duration,
            "cover":self.cover})
    }
    pub fn from_json(v: &Value) -> Option<Track> {
        let text = |k: &str| v[k].as_str().map(str::to_owned);
        Some(Track {
            path: text("path").filter(|p| !p.is_empty())?,
            size: v["size"].as_u64().unwrap_or(0),
            mtime: v["mtime"].as_f64().unwrap_or(0.),
            title: text("title")?,
            artist: text("artist").unwrap_or_default(),
            album: text("album").unwrap_or_default(),
            album_artist: text("albumArtist").unwrap_or_default(),
            track: v["track"].as_i64().unwrap_or(0),
            year: text("year").unwrap_or_default(),
            genre: text("genre").unwrap_or_default(),
            duration: v["duration"].as_f64().unwrap_or(0.),
            cover: v["cover"] == true,
        })
    }
}
/// Split the files into the tracks the cache already knows and the ones to probe.
pub fn partition(files: &[Found], cache: &Value) -> (Vec<Track>, Vec<Found>) {
    let (mut known, mut todo) = (vec![], vec![]);
    for file in files {
        let entry = &cache[&file.path];
        match Track::from_json(&entry["t"]).filter(|_| entry["k"] == cache_key(file).as_str()) {
            Some(track) => known.push(track),
            None => todo.push(file.clone()),
        }
    }
    (known, todo)
}
/// The cache to write: every track still on disk, with the key it was read under.
pub fn cache_json(tracks: &[Track]) -> Value {
    let mut out = Map::new();
    for t in tracks {
        let file = Found { path: t.path.clone(), name: base_name(&t.path).into(), size: t.size, mtime: t.mtime };
        out.insert(t.path.clone(), json!({"k":cache_key(&file),"t":t.to_json()}));
    }
    Value::Object(out)
}
/// Whether the cache file must be rewritten: something was read, or something left the disk.
pub fn cache_dirty(read: usize, cached: usize, kept: usize) -> bool {
    read > 0 || cached != kept
}

/// `a.localeCompare(b)` for the library's sort orders: case and accents do not decide,
/// then the plain text breaks ties so the order is stable.
pub fn collate(a: &str, b: &str) -> Ordering {
    let (fa, fb) = (axia_native::matching::fold(a), axia_native::matching::fold(b));
    fa.cmp(&fb).then_with(|| a.cmp(b))
}
fn by_track_then_title(x: &Track, y: &Track) -> Ordering {
    x.track.cmp(&y.track).then_with(|| collate(&x.title, &y.title))
}
/// Albums by album artist then title; tracks by number then title.
pub fn albums_of(tracks: &[Track]) -> Vec<Album> {
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut list: Vec<Album> = vec![];
    for t in tracks {
        let key = format!("{}\u{0}{}", t.album_artist, t.album);
        match at.get(&key) {
            Some(i) => list[*i].tracks.push(t.clone()),
            None => {
                at.insert(key.clone(), list.len());
                list.push(Album {
                    key,
                    album: t.album.clone(),
                    artist: t.album_artist.clone(),
                    year: t.year.clone(),
                    tracks: vec![t.clone()],
                });
            }
        }
    }
    for a in &mut list {
        a.tracks.sort_by(by_track_then_title);
    }
    list.sort_by(|a, b| collate(&a.artist, &b.artist).then_with(|| collate(&a.album, &b.album)));
    list
}
pub fn artists_of(tracks: &[Track]) -> Vec<Artist> {
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut list: Vec<Artist> = vec![];
    for t in tracks {
        let name = if t.album_artist.is_empty() { &t.artist } else { &t.album_artist };
        match at.get(name) {
            Some(i) => list[*i].tracks.push(t.clone()),
            None => {
                at.insert(name.clone(), list.len());
                list.push(Artist { name: name.clone(), tracks: vec![t.clone()] });
            }
        }
    }
    list.sort_by(|a, b| collate(&a.name, &b.name));
    list
}
/// The Songs view: every track by title.
pub fn songs_sorted(tracks: &[Track]) -> Vec<Track> {
    let mut list = tracks.to_vec();
    list.sort_by(|a, b| collate(&a.title, &b.title));
    list
}
/// The albums an artist's page lists (`a.artist === sel || some track by sel`).
pub fn albums_of_artist<'a>(albums: &'a [Album], name: &str) -> Vec<&'a Album> {
    albums
        .iter()
        .filter(|a| a.artist == name || a.tracks.iter().any(|t| t.artist == name))
        .collect()
}

// -------------------------------------------------------------- subtitles

#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}
/// The first `h:m:s[,.]fraction` in the text, in ms (the fraction is cut or padded to three digits).
fn srt_time(text: &str) -> f64 {
    let b = text.as_bytes();
    // the end of the run of digits that starts at `from`
    let digits = |from: usize| from + b[from.min(b.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
    'start: for start in 0..b.len() {
        // a match inside a run of digits is the one that starts the run
        if !b[start].is_ascii_digit() || (start > 0 && b[start - 1].is_ascii_digit()) {
            continue;
        }
        let mut at = start;
        let mut parts = [0u64; 3];
        for (i, part) in parts.iter_mut().enumerate() {
            let end = digits(at);
            if end == at {
                continue 'start;
            }
            *part = text[at..end].parse().unwrap_or(0);
            at = end;
            if i < 2 {
                if b.get(at) != Some(&b':') {
                    continue 'start;
                }
                at += 1;
            }
        }
        if !matches!(b.get(at), Some(b',') | Some(b'.')) {
            continue;
        }
        let end = digits(at + 1);
        if end == at + 1 {
            continue;
        }
        let mut fraction: String = text[at + 1..end].chars().take(3).collect();
        while fraction.len() < 3 {
            fraction.push('0');
        }
        let ms: u64 = fraction.parse().unwrap_or(0);
        return (((parts[0] * 60 + parts[1]) * 60 + parts[2]) * 1000 + ms) as f64;
    }
    0.
}
/// `replace(/<[^>]+>/g, '')`: a tag needs at least one character before its `>`.
fn strip_markup(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        match after.find('>') {
            Some(close) if close > 0 => {
                out.push_str(&rest[..open]);
                rest = &after[close + 1..];
            }
            _ => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
/// `replace(/\{\\[^}]*\}/g, '')`: the style overrides of .ass lines
fn strip_overrides(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find("{\\") {
        match rest[open + 2..].find('}') {
            Some(i) => {
                out.push_str(&rest[..open]);
                rest = &rest[open + 2 + i + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}
/// `.srt` subtitles in ms.
pub fn parse_srt(text: &str) -> Vec<Cue> {
    let text = text.replace('\r', "");
    let mut blocks: Vec<Vec<&str>> = vec![vec![]];
    for line in text.split('\n') {
        if line.trim().is_empty() {
            if !blocks.last().is_some_and(Vec::is_empty) {
                blocks.push(vec![]);
            }
        } else {
            blocks.last_mut().unwrap().push(line);
        }
    }
    let mut out = vec![];
    for lines in blocks {
        let Some(i) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let mut times = lines[i].split("-->");
        let (a, b) = (times.next().unwrap_or(""), times.next().unwrap_or(""));
        let body = strip_overrides(&strip_markup(&lines[i + 1..].join("\n")));
        if !body.is_empty() {
            out.push(Cue { start: srt_time(a), end: srt_time(b), text: body });
        }
    }
    out
}
/// The text on screen at `pos`: every cue that covers it, one per line.
pub fn cue_at(cues: &[Cue], pos: f64) -> String {
    cues.iter()
        .filter(|c| c.start <= pos && pos < c.end)
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

// ------------------------------------------------------------------ queue

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Off,
    All,
    One,
}
impl Repeat {
    pub fn parse(text: &str) -> Repeat {
        match text {
            "all" => Repeat::All,
            "one" => Repeat::One,
            _ => Repeat::Off,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Repeat::Off => "off",
            Repeat::All => "all",
            Repeat::One => "one",
        }
    }
    /// The repeat button: off → all → one → off.
    pub fn cycle(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}
/// What "next" does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advance {
    /// Nothing: the queue is empty, or it ended without repeat.
    Stay,
    /// Repeat one: seek to 0 and play again.
    Restart,
    /// Open the track at this index.
    Go(usize),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Queue {
    pub paths: Vec<String>,
    pub index: i64,
    pub shuffle: bool,
    pub repeat: Repeat,
}
impl Default for Queue {
    fn default() -> Self {
        Queue { paths: vec![], index: -1, shuffle: false, repeat: Repeat::Off }
    }
}
impl Queue {
    pub fn repeat(&self) -> Repeat {
        self.repeat
    }
    pub fn current(&self) -> Option<&str> {
        usize::try_from(self.index).ok().and_then(|i| self.paths.get(i)).map(String::as_str)
    }
    /// Replace the queue and start at `index` (a click on a row, a drop, "Open with").
    pub fn play_list(&mut self, paths: Vec<String>, index: usize) {
        self.index = if index < paths.len() { index as i64 } else { -1 };
        self.paths = paths;
    }
    /// `pick(n)` gives a random number below `n` (shuffle).
    pub fn next(&self, auto: bool, pick: impl FnOnce(usize) -> usize) -> Advance {
        let len = self.paths.len();
        if len == 0 {
            return Advance::Stay;
        }
        if auto && self.repeat() == Repeat::One {
            return Advance::Restart;
        }
        let mut n = if self.shuffle { pick(len).min(len - 1) } else { (self.index + 1).max(0) as usize };
        if n >= len {
            if self.repeat() != Repeat::All && auto {
                return Advance::Stay;
            }
            n = 0;
        }
        Advance::Go(n)
    }
    /// "Previous": past three seconds it restarts the track, else it steps back (never below 0).
    pub fn prev(&self, pos_ms: f64) -> Advance {
        if pos_ms > RESTART_MS {
            return Advance::Restart;
        }
        Advance::Go((self.index - 1).max(0) as usize)
    }
}

// ----------------------------------------------------------------- player

/// What the player reports (`reduce` of `sdk/media.js`): the state for the window.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub info: Value,
    pub pos: f64,
    pub duration: f64,
    pub playing: bool,
    pub state: String,
    pub subs: Vec<Cue>,
    pub error: Option<String>,
    pub closed: bool,
}
impl Default for PlayerState {
    fn default() -> Self {
        PlayerState {
            info: Value::Null,
            pos: 0.,
            duration: 0.,
            playing: false,
            state: "loading".into(),
            subs: vec![],
            error: None,
            closed: false,
        }
    }
}
impl PlayerState {
    pub fn reduce(&mut self, kind: &str, data: &Value) {
        let duration = data["duration"].as_f64().filter(|d| *d != 0.);
        match kind {
            "info" => {
                self.info = data.clone();
                self.duration = duration.unwrap_or(self.duration);
            }
            "pos" => {
                self.pos = data["pos"].as_f64().unwrap_or(self.pos);
                self.duration = duration.unwrap_or(self.duration);
                self.playing = data["playing"] == true;
            }
            "state" => {
                let state = data["state"].as_str().unwrap_or("").to_owned();
                self.playing = state == "playing";
                self.state = state;
            }
            "sub" => {
                let cue = Cue {
                    start: data["start"].as_f64().unwrap_or(0.),
                    end: data["end"].as_f64().unwrap_or(0.),
                    text: data["text"].as_str().unwrap_or("").to_owned(),
                };
                self.subs.retain(|c| c.end > cue.start - 60000.);
                if self.subs.len() > 200 {
                    let drop = self.subs.len() - 200;
                    self.subs.drain(..drop);
                }
                self.subs.push(cue);
            }
            "error" => self.error = Some(data["message"].as_str().unwrap_or("").to_owned()),
            "ready" => {
                self.info = data.clone();
                self.state = "ready".into();
            }
            "closed" => {
                self.closed = true;
                if data["crashed"] == true {
                    self.error = Some("crash".into());
                }
            }
            "done" => self.state = "done".into(),
            _ => {}
        }
    }
    /// The stream list of the file (`info.streams`).
    fn streams(&self, kind: &str) -> Vec<&Value> {
        self.info["streams"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["type"] == kind)
            .collect()
    }
    /// The text subtitle tracks the movie carries.
    pub fn subtitle_streams(&self) -> Vec<&Value> {
        self.streams("subtitle").into_iter().filter(|s| truthy(&s["text"])).collect()
    }
    pub fn audio_streams(&self) -> Vec<&Value> {
        self.streams("audio")
    }
    /// The picture ratio (height over width) for the picture in picture, 9/16 when unknown.
    pub fn video_ratio(&self) -> f64 {
        let video = self.streams("video").into_iter().next();
        let h = video.and_then(|v| v["height"].as_f64()).filter(|h| *h != 0.).unwrap_or(9.);
        let w = video.and_then(|v| v["width"].as_f64()).filter(|w| *w != 0.).unwrap_or(16.);
        h / w
    }
    /// A video that has a picture (`info.video >= 0`).
    pub fn has_video(&self) -> bool {
        self.info["video"].as_i64().is_some_and(|v| v >= 0)
    }
}
/// The label of a subtitle or audio track: title and language, else "Track n".
pub fn track_label(stream: &Value) -> Option<String> {
    let title = stream["title"].as_str().filter(|s| !s.is_empty());
    let lang = stream["lang"].as_str().filter(|s| !s.is_empty() && *s != "und");
    let parts: Vec<&str> = [title, lang].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

// ------------------------------------------------------------- the controls

/// Volume from a click on the bar (0..1 of its width): steps of 0.01 up to 1.5.
pub fn volume_from_fraction(f: f64) -> f64 {
    ((f.clamp(0., 1.) * 150.).round()) / 100.
}
pub fn volume_step(volume: f64, up: bool) -> f64 {
    if up {
        (volume + 0.1).min(VOLUME_MAX)
    } else {
        (volume - 0.1).max(0.)
    }
}
/// Left/right: 5 s, 30 s with Shift. Never before the start.
pub fn seek_by(pos: f64, forward: bool, shift: bool) -> f64 {
    let step = if shift { 30000. } else { 5000. };
    if forward {
        pos + step
    } else {
        (pos - step).max(0.)
    }
}
/// A click or a release on the time bar: the position in ms.
pub fn seek_from_fraction(f: f64, duration: f64) -> f64 {
    (f.clamp(0., 1.) * duration).round()
}
/// The fraction of a bar's width under a pointer at `x` (`at()` of the JSX `Bar`).
pub fn bar_fraction(x: f64, left: f64, width: f64) -> f64 {
    if !x.is_finite() {
        return 0.;
    }
    ((x - left) / if width != 0. { width } else { 1. }).clamp(0., 1.)
}
/// Whether the video controls show: always while paused or on a menu, else until the pointer
/// has been still for `CONTROLS_HIDE_MS`.
pub fn controls_visible(video: bool, playing: bool, menu_open: bool, last_move: i64, now: i64) -> bool {
    !video || !playing || menu_open || now - last_move <= CONTROLS_HIDE_MS
}
/// The window of the picture in picture: 400 wide, bottom right of the screen.
pub fn pip_frame(screen_w: f64, screen_h: f64, ratio: f64) -> Value {
    let w = 400.;
    let h = (w * ratio).round();
    json!({"x":screen_w - w - 24.,"y":screen_h - h - 96.,"w":w,"h":h})
}
/// What is published for the Control Center and the media keys (`nowPlaying`).
pub fn now_playing(window: &str, track: Option<&Track>, path: &str, state: &PlayerState, cover: Option<&str>) -> Value {
    let tags = &state.info["tags"];
    let pick = |own: Option<&str>, tag: &str| -> String {
        own.filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| tags[tag].as_str().filter(|s| !s.is_empty()).map(str::to_owned))
            .unwrap_or_default()
    };
    let title = {
        let t = pick(track.map(|t| t.title.as_str()), "title");
        if t.is_empty() { base_name(path).to_owned() } else { t }
    };
    json!({"app":"player","window":window,"title":title,
        "artist":pick(track.map(|t| t.artist.as_str()), "artist"),
        "album":pick(track.map(|t| t.album.as_str()), "album"),
        "cover":cover,"playing":state.playing,"pos":state.pos,"duration":state.duration})
}
