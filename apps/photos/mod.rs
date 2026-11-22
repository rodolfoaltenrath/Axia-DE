//! Photos: native scene nodes, gestures, library, animation and editing in Rust.
mod decode;
mod formats;
mod io;
mod model;
mod render;
#[cfg(test)]
mod tests;
mod ui;
mod worker;
use crate::native_view::{async_jobs::Jobs, i18n::I18n, NativeApp};
use io::Entry;
use model::View;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
};
enum Tag {
    Ignore,
    Events,
}
struct Thumb {
    bytes: Vec<u8>,
    width: usize,
    height: usize,
}
enum Drag {
    Pan(f64, f64, f64, f64),
    Crop((f64, f64)),
    Range(String, f64, f64),
    Scroll(f64, f64),
}
pub struct App {
    home: PathBuf,
    root: PathBuf,
    folder: PathBuf,
    folder_scroll: f64,
    recursive: bool,
    folders: Vec<PathBuf>,
    entries: Vec<Entry>,
    i18n: I18n,
    theme: std::rc::Rc<Value>,
    width: f64,
    height: f64,
    rtl: bool,
    foreground: bool,
    window: String,
    args: Value,
    worker: Option<worker::Worker>,
    library: Option<worker::Library>,
    generation: u64,
    scanning: bool,
    thumbs: HashMap<String, Thumb>,
    thumb_order: VecDeque<String>,
    thumb_pending: Option<PathBuf>,
    thumb_size: f64,
    layout: Vec<(usize, f64, Option<(i32, u32)>)>,
    scroll: f64,
    selected: Option<usize>,
    current: Option<Entry>,
    view: View,
    image_size: (u32, u32),
    raster: Option<render::Raster>,
    raster_generation: u64,
    busy: bool,
    busy_quality: bool,
    quality_ms: f64,
    refine_at: i64,
    needs_refine: bool,
    dirty: bool,
    edit: bool,
    crop: Option<[u32; 4]>,
    selection: Option<((f64, f64), (f64, f64))>,
    drag: Option<Drag>,
    saving: bool,
    error: String,
    message: String,
    animated: bool,
    animation_start: i64,
    next_frame: i64,
    last_submit: i64,
    slide: bool,
    next_slide: i64,
    fullscreen: bool,
    window_mode: String,
    restore_scroll: Option<f64>,
    checkpoint_at: i64,
    checkpoint_dirty: bool,
    pinch: f64,
    jobs: Jobs<Tag>,
    events_pending: bool,
    events_at: i64,
    menus: Value,
    effects: Vec<Value>,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            home: PathBuf::new(),
            root: PathBuf::new(),
            folder: PathBuf::new(),
            folder_scroll: 0.,
            recursive: true,
            folders: vec![],
            entries: vec![],
            i18n: I18n::default(),
            theme: std::rc::Rc::new(Value::Null),
            width: 980.,
            height: 640.,
            rtl: false,
            foreground: true,
            window: String::new(),
            args: Value::Null,
            worker: None,
            library: None,
            generation: 0,
            scanning: false,
            thumbs: HashMap::new(),
            thumb_order: VecDeque::new(),
            thumb_pending: None,
            thumb_size: 140.,
            layout: vec![],
            scroll: 0.,
            selected: None,
            current: None,
            view: View::default(),
            image_size: (1, 1),
            raster: None,
            raster_generation: 0,
            busy: false,
            busy_quality: true,
            quality_ms: 0.,
            refine_at: 0,
            needs_refine: false,
            dirty: false,
            edit: false,
            crop: None,
            selection: None,
            drag: None,
            saving: false,
            error: String::new(),
            message: String::new(),
            animated: false,
            animation_start: 0,
            next_frame: 0,
            last_submit: 0,
            slide: false,
            next_slide: 0,
            fullscreen: false,
            window_mode: String::new(),
            restore_scroll: None,
            checkpoint_at: 0,
            checkpoint_dirty: false,
            pinch: 1.,
            jobs: Jobs::default(),
            events_pending: false,
            events_at: 0,
            menus: Value::Null,
            effects: vec![],
            last_tree: Value::Null,
        }
    }
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool { true }
    fn set_native_appearance(&mut self, theme: std::rc::Rc<Value>) { self.theme = theme; }
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.home = home.unwrap_or("").into();
        self.i18n = I18n::new(assets, home);
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.worker.as_ref()?;
        Some(
            if self.busy
                || self.dirty
                || self.drag.is_some()
                || self.animated
                || self.slide
                || self.thumb_pending.is_some()
                || self.needs_refine
            {
                16.
            } else if self.scanning {
                50.
            } else {
                200.
            },
        )
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        (self.saving || self.jobs.busy()).then_some(100.)
    }
    fn take_raster(&mut self, id: &str) -> Option<(usize, usize, Vec<u32>)> {
        if id != "viewport" {
            return None;
        }
        self.raster.take().map(|r| (r.width, r.height, r.pixels))
    }
    fn pixels(&self, id: &str) -> Option<(usize, usize, &[u8])> {
        self.thumbs
            .get(id)
            .map(|t| (t.width, t.height, t.bytes.as_slice()))
    }
    fn blur(&mut self) {
        self.drag = None;
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        let base = if chr.is_empty() { key } else { chr };
        let cmd = if flags & (2 | 16) != 0 && key.eq_ignore_ascii_case("p") {
            "print"
        } else {
            match key {
                "Left" | "ArrowLeft" => {
                    if self.rtl {
                        "nextImage"
                    } else {
                        "previous"
                    }
                }
                "Right" | "ArrowRight" => {
                    if self.rtl {
                        "previous"
                    } else {
                        "nextImage"
                    }
                }
                "Escape" => "escape",
                "Return" | "Enter" => "openSelected",
                "space" | " " => "slideshow",
                _ => match base {
                    "+" | "=" => "zoomIn",
                    "-" => "zoomOut",
                    "0" => "fit",
                    "1" => "actual",
                    "r" | "R" => "rotateRight",
                    "f" | "F" => "fullscreen",
                    " " => "slideshow",
                    _ => return None,
                },
            }
        };
        self.command(cmd, &Value::Null, now_ms());
        Some(self.output(now_ms()))
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] == "icon" {
            return ui::icon(input["size"].as_f64().unwrap_or(32.));
        }
        let now = now_ms();
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.rtl = self
                    .i18n
                    .entry(input["lang"].as_str().unwrap_or("en"), "meta", "direction")
                    .as_ref()
                    .and_then(Value::as_str)
                    == Some("rtl");
                let context = &input["context"];
                if context["theme"].is_object() {
                    self.theme = std::rc::Rc::new(context["theme"].clone());
                }
                self.foreground = context["focused"] != false;
                if let Some(mode) = context["windowMode"].as_str() {
                    if mode != self.window_mode {
                        self.window_mode = mode.into();
                        self.fullscreen = mode == "fullscreen";
                        if !self.fullscreen {
                            self.slide = false;
                        }
                    }
                }
                let w = context["width"]
                    .as_f64()
                    .unwrap_or(self.width)
                    .clamp(1., 4096.);
                let h = context["height"]
                    .as_f64()
                    .unwrap_or(self.height)
                    .clamp(1., 2160.);
                if (w, h) != (self.width, self.height) {
                    self.width = w;
                    self.height = h;
                    self.dirty = self.current.is_some();
                    self.clamp_scroll();
                }
                let args = &input["args"];
                self.window = args["__nativeWindow"].as_str().unwrap_or("").into();
                self.jobs.bind(args["__nativeOwner"].as_i64().unwrap_or(0));
                let boot = self.worker.is_none();
                if boot {
                    self.worker = Some(worker::Worker::default());
                    self.library = Some(worker::Library::default());
                    self.root = io::pictures(&self.home);
                    self.scan(self.root.clone(), true);
                }
                if args["path"].is_string()
                    && (args["path"] != self.args["path"] || args["nonce"] != self.args["nonce"])
                {
                    let path = PathBuf::from(args["path"].as_str().unwrap());
                    if io::supported(&path) {
                        let parent = path.parent().unwrap_or(&self.root).to_owned();
                        self.scan(parent, false);
                        self.open(
                            Entry {
                                name: path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                                path,
                                size: 0,
                                mtime: 0,
                                stamp: (0, 0, 0, 0),
                            },
                            now,
                        );
                    }
                }
                self.args = args.clone();
                if boot {
                    self.restore(&args["__nativeState"], args, now);
                }
                self.poll(now);
                if self.checkpoint_dirty && now >= self.checkpoint_at {
                    self.checkpoint();
                    self.checkpoint_at = now + 250;
                }
                if self.foreground
                    && !self.events_pending
                    && now >= self.events_at
                    && !self.window.is_empty()
                {
                    self.events_pending = true;
                    self.jobs.enqueue(Tag::Events, "windows.events", json!([]));
                }
                self.menu();
            }
            "next" => {
                for done in self.jobs.complete(&input["result"], now) {
                    match done.tag {
                        Tag::Events => {
                            self.events_pending = false;
                            self.events_at = now + 200;
                            if let Ok(v) = done.result {
                                for e in v.as_array().into_iter().flatten() {
                                    if e["kind"] == "action" {
                                        self.command(
                                            e["action"][0].as_str().unwrap_or(""),
                                            &json!({"key":e["action"][1]}),
                                            now,
                                        )
                                    }
                                }
                            }
                        }
                        Tag::Ignore => {
                            if let Err(e) = done.result {
                                self.error = e
                            }
                        }
                    }
                }
            }
            cmd => self.command(cmd, input, now),
        }
        if input["cmd"] == "sync" {
            self.sync_output(now)
        } else {
            self.output(now)
        }
    }
}
impl App {
    fn tr(&mut self, k: &str) -> String {
        self.i18n.caption(&json!({"key":format!("photos.{k}")}))
    }
    fn scan(&mut self, path: PathBuf, recursive: bool) {
        self.folder = path.clone();
        self.folder_scroll = 0.;
        self.recursive = recursive;
        self.scroll = 0.;
        self.selected = None;
        self.thumb_pending = None;
        self.thumbs.clear();
        self.thumb_order.clear();
        self.scanning = true;
        self.error.clear();
        if let Some(w) = &self.library {
            self.generation = w.scan(path, recursive, self.home.clone())
        }
    }
    fn open(&mut self, entry: Entry, now: i64) {
        if self.saving {
            return;
        }
        if let Some(w) = &self.worker {
            w.cancel()
        }
        self.effects
            .push(json!({"host":true,"kind":"title","title":entry.name}));
        self.current = Some(entry);
        self.view = View::default();
        self.needs_refine = false;
        self.image_size = (0, 0);
        self.raster = None;
        self.raster_generation += 1;
        self.busy = false;
        self.dirty = true;
        self.edit = false;
        self.crop = None;
        self.selection = None;
        self.animated = false;
        self.animation_start = now;
        self.next_frame = now;
        self.next_slide = now + 4000;
        self.error.clear();
        self.message.clear();
        self.drag = None;
    }
    fn close_viewer(&mut self) {
        if let Some(w) = &self.worker {
            w.cancel()
        }
        self.current = None;
        self.slide = false;
        self.edit = false;
        self.drag = None;
        self.busy = false;
        self.dirty = false;
        self.animated = false;
        self.needs_refine = false;
        self.raster = None;
        self.crop = None;
        self.selection = None;
    }
    fn navigate(&mut self, delta: isize, now: i64) {
        if self.saving || self.entries.is_empty() {
            return;
        }
        let index = self
            .current
            .as_ref()
            .and_then(|e| self.entries.iter().position(|x| x.path == e.path))
            .or(self.selected)
            .unwrap_or(0);
        let i = (index as isize + delta).rem_euclid(self.entries.len() as isize) as usize;
        self.selected = Some(i);
        self.open(self.entries[i].clone(), now)
    }
    fn viewport(&self) -> (u32, u32) {
        (
            self.width.max(1.) as u32,
            (self.height - if self.edit { 146. } else { 86. }).max(1.) as u32,
        )
    }
    fn folder_max_scroll(&self) -> f64 {
        (self.folders.len() as f64 * 32. - (self.height - 170.).max(1.)).max(0.)
    }
    fn columns(&self) -> usize {
        ((self.width - 180. - 32.) / (self.thumb_size + 12.))
            .floor()
            .max(1.) as usize
    }
    fn rebuild_layout(&mut self) {
        let mut out = Vec::with_capacity(self.entries.len());
        let (mut y, mut col, mut prev) = (12., 0, None);
        let cols = self.columns();
        for (i, e) in self.entries.iter().enumerate() {
            let month = ui::month(e.mtime);
            if prev != Some(month) {
                if col > 0 {
                    y += self.thumb_size + 34.;
                    col = 0
                }
                y += 32.;
                prev = Some(month);
                out.push((i, y, Some(month)))
            } else {
                out.push((i, y, None))
            }
            col += 1;
            if col >= cols {
                col = 0;
                y += self.thumb_size + 34.;
            }
        }
        self.layout = out;
    }
    fn rows(&self) -> &[(usize, f64, Option<(i32, u32)>)] {
        &self.layout
    }
    fn max_scroll(&self) -> f64 {
        self.rows().last().map_or(0., |(_, y, _)| {
            (y + self.thumb_size + 40. - (self.height - 54.)).max(0.)
        })
    }
    fn clamp_scroll(&mut self) {
        self.rebuild_layout();
        self.scroll = self.scroll.clamp(0., self.max_scroll());
    }
    fn visible(&self) -> Vec<usize> {
        self.rows()
            .iter()
            .filter_map(|(i, y, _)| {
                (*y + self.thumb_size + 34. >= self.scroll && *y <= self.scroll + self.height)
                    .then_some(*i)
            })
            .collect()
    }
    fn poll(&mut self, now: i64) {
        let results = self.worker.as_ref().map(|w| w.take()).unwrap_or_default();
        for (_, r) in results {
            match r {
                worker::ResultValue::View(result) => {
                    self.busy = false;
                    match result {
                        Ok(d) => {
                            if d.quality {
                                self.quality_ms = d.render_ms;
                            }
                            self.image_size = (d.width, d.height);
                            self.animated = d.frames > 1;
                            self.next_frame = now + d.delay.max(20) as i64;
                            self.raster = Some(d.raster);
                            self.raster_generation += 1;
                            self.error.clear()
                        }
                        Err(e) if e != "cancelled" => self.error = e,
                        _ => {}
                    }
                }
                worker::ResultValue::Save(result) => {
                    self.saving = false;
                    match result {
                        Ok(path) => {
                            let message=self.i18n.caption(&json!({"key":"photos.saved","params":{"name":path.file_name().unwrap_or_default().to_string_lossy()}}));
                            self.scan(self.folder.clone(), self.recursive);
                            self.open(
                                Entry {
                                    name: path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned(),
                                    path,
                                    size: 0,
                                    mtime: 0,
                                    stamp: (0, 0, 0, 0),
                                },
                                now,
                            );
                            self.message = message;
                        }
                        Err(e) => self.error = e,
                    }
                }
            }
        }
        let results = self.library.as_ref().map(|w| w.take()).unwrap_or_default();
        for r in results {
            match r {
                worker::LibraryResult::Scan(g, result) if g == self.generation => {
                    self.scanning = false;
                    match result {
                        Ok((mut entries, folders)) => {
                            if !self.recursive {
                                io::neighbor_sort(&mut entries)
                            }
                            self.entries = entries;
                            self.folders = folders;
                            self.selected = self
                                .current
                                .as_ref()
                                .and_then(|e| self.entries.iter().position(|x| x.path == e.path));
                            if let Some(scroll) = self.restore_scroll.take() {
                                self.scroll = scroll;
                            }
                            self.clamp_scroll();
                        }
                        Err(e) => self.error = e,
                    }
                }
                worker::LibraryResult::Thumb(g, path, result) if g == self.generation => {
                    self.thumb_pending = None;
                    let id = path.to_string_lossy().into_owned();
                    let t = match result {
                        Ok(r) => {
                            let mut bytes = Vec::with_capacity(r.pixels.len() * 4);
                            for p in r.pixels {
                                bytes.extend_from_slice(&[
                                    (p >> 16) as u8,
                                    (p >> 8) as u8,
                                    p as u8,
                                    255,
                                ])
                            }
                            Thumb {
                                bytes,
                                width: r.width,
                                height: r.height,
                            }
                        }
                        Err(_) => Thumb {
                            bytes: vec![35, 35, 38, 255],
                            width: 1,
                            height: 1,
                        },
                    };
                    while self.thumbs.len() >= 128 {
                        if let Some(k) = self.thumb_order.pop_front() {
                            self.thumbs.remove(&k);
                        } else {
                            break;
                        }
                    }
                    self.thumb_order.push_back(id.clone());
                    self.thumbs.insert(id, t);
                }
                _ => {}
            }
        }
        if self.slide && self.foreground && now >= self.next_slide && !self.saving {
            self.navigate(1, now);
        }
        if self.current.is_some() {
            if self.needs_refine && self.drag.is_none() && now >= self.refine_at {
                self.view.quality = true;
                self.needs_refine = false;
                self.dirty = true;
            }
            let (w, h) = self.viewport();
            if (self.view.width, self.view.height) != (w, h) {
                self.view.width = w;
                self.view.height = h;
                self.dirty = true;
            }
            if self.animated && self.foreground && !self.edit && now >= self.next_frame {
                self.dirty = true;
            }
            if self.dirty && !self.busy && !self.saving && now - self.last_submit >= 16 {
                if let (Some(worker), Some(entry)) = (&self.worker, &self.current) {
                    if worker
                        .submit(worker::Task::View {
                            entry: entry.clone(),
                            home: self.home.clone(),
                            view: self.view,
                            elapsed: if self.edit {
                                0
                            } else {
                                now.saturating_sub(self.animation_start) as u64
                            },
                        })
                        .is_some()
                    {
                        self.busy = true;
                        self.busy_quality = self.view.quality;
                        self.dirty = false;
                        self.last_submit = now
                    }
                }
            }
        } else if !self.scanning && self.thumb_pending.is_none() {
            let next = self.visible().into_iter().find(|&i| {
                !self
                    .thumbs
                    .contains_key(self.entries[i].path.to_string_lossy().as_ref())
            });
            if let Some(i) = next {
                let e = self.entries[i].clone();
                self.thumb_pending = Some(e.path.clone());
                if let Some(w) = &self.library {
                    w.thumbs(self.generation, vec![e], self.home.clone())
                }
            }
        }
    }
    fn sync_output(&mut self, now: i64) -> Value {
        let tree = ui::tree(self);
        let mut out = self.output(now);
        if tree == self.last_tree {
            out["same"] = json!(true)
        } else {
            self.last_tree = tree.clone();
            out["tree"] = tree
        }
        out
    }
    fn output(&mut self, now: i64) -> Value {
        let mut out = json!({});
        out["capture"] = json!(self.drag.is_some());
        if let Some(e) = self.jobs.pump(now) {
            self.effects.push(e)
        }
        if !self.effects.is_empty() {
            out["effects"] = json!(std::mem::take(&mut self.effects))
        }
        out
    }
    fn full(&mut self, on: bool) {
        self.checkpoint();
        self.fullscreen = on;
        self.effects
            .push(json!({"host":true,"kind":"fullscreen","on":on}));
    }
    fn menu(&mut self) {
        if self.window.is_empty() {
            return;
        }
        let has = self.current.is_some();
        let mut items = vec![];
        for (k, c) in [
            ("menu.print", "print"),
            ("refresh", "refresh"),
            ("slideshow", "slideshow"),
        ] {
            items.push(json!({"label":self.tr(k),"action":[c],"disabled":c!="refresh" && !has}))
        }
        let mut view = vec![];
        for (k, c) in [
            ("fit", "fit"),
            ("zoomIn", "zoomIn"),
            ("zoomOut", "zoomOut"),
            ("rotateLeft", "rotateLeft"),
            ("rotateRight", "rotateRight"),
            ("edit", "edit"),
            ("fullscreen", "fullscreen"),
        ] {
            view.push(json!({"label":self.tr(k),"action":[c],"disabled":!has}))
        }
        let menus = json!([{"label":self.tr("menu.file"),"items":items},{"label":self.tr("library"),"items":view}]);
        if menus != self.menus {
            self.menus = menus.clone();
            self.jobs
                .enqueue(Tag::Ignore, "windows.menus", json!([self.window, menus]));
        }
    }
    fn command(&mut self, cmd: &str, input: &Value, now: i64) {
        if self.saving && !matches!(cmd, "next" | "pointerMove" | "dragEnd") {
            return;
        }
        if self.current.is_some()
            && self.image_size.0 == 0
            && matches!(
                cmd,
                "pan"
                    | "wheel"
                    | "gesture"
                    | "zoomIn"
                    | "zoomOut"
                    | "actual"
                    | "rotateLeft"
                    | "rotateRight"
                    | "edit"
                    | "save"
            )
        {
            return;
        }
        let n = |k: &str| input[k].as_f64().filter(|v| v.is_finite()).unwrap_or(0.);
        let (x, y) = (n("x"), n("y") - 48.);
        let (w, h) = self.image_size;
        let mut changed = false;
        match cmd {
            "folder" => {
                if let Some(p) = input["key"].as_str() {
                    self.close_viewer();
                    self.scan(p.into(), false)
                }
            }
            "library" => {
                self.close_viewer();
                if self.fullscreen {
                    self.full(false)
                }
            }
            "all" => {
                self.close_viewer();
                self.scan(self.root.clone(), true)
            }
            "refresh" => {
                if let Some(e) = self.current.as_mut() {
                    e.stamp = (0, 0, 0, 0);
                    e.size = 0;
                }
                if let Some(w) = &self.worker {
                    w.cancel()
                }
                self.busy = false;
                self.animated = false;
                self.thumbs.clear();
                self.thumb_order.clear();
                self.scan(self.folder.clone(), self.recursive);
                self.dirty = self.current.is_some();
            }
            "select" | "open" => {
                if let Some(i) = input["key"]
                    .as_u64()
                    .map(|x| x as usize)
                    .filter(|&i| i < self.entries.len())
                {
                    self.selected = Some(i);
                    if cmd == "open" {
                        self.open(self.entries[i].clone(), now)
                    }
                }
            }
            "openSelected" => {
                if self.current.is_none() {
                    if let Some(i) = self.selected.filter(|&i| i < self.entries.len()) {
                        self.open(self.entries[i].clone(), now)
                    }
                }
            }
            "previous" => self.navigate(-1, now),
            "nextImage" => self.navigate(1, now),
            "zoomIn" | "zoomOut" => {
                self.view.zoom_at(
                    w,
                    h,
                    if cmd == "zoomIn" { 1.25 } else { 0.8 },
                    self.view.width as f64 / 2.,
                    self.view.height as f64 / 2.,
                );
                changed = true
            }
            "fit" => {
                self.view.zoom = 0.;
                self.view.cx = 0.5;
                self.view.cy = 0.5;
                changed = true
            }
            "actual" => {
                self.view.zoom = 1.;
                self.view.cx = 0.5;
                self.view.cy = 0.5;
                changed = true
            }
            "rotateLeft" | "rotateRight" => {
                self.view.rotation =
                    (self.view.rotation + if cmd == "rotateLeft" { 3 } else { 1 }) % 4;
                self.crop = None;
                self.selection = None;
                changed = true
            }
            "edit" => {
                if self.current.is_some() {
                    self.edit = !self.edit;
                    self.slide = false;
                    self.selection = None;
                    self.crop = None;
                    changed = true
                }
            }
            "reset" => {
                let (vw, vh) = self.viewport();
                self.view = View {
                    width: vw,
                    height: vh,
                    ..Default::default()
                };
                self.crop = None;
                self.selection = None;
                changed = true
            }
            "save" => {
                if let Some(e) = self.current.clone() {
                    let suffix = self.tr("edited");
                    if let Some(worker) = &self.worker {
                        worker.cancel();
                        if worker
                            .submit(worker::Task::Save {
                                entry: e,
                                home: self.home.clone(),
                                view: self.view,
                                crop: self.crop,
                                suffix,
                            })
                            .is_some()
                        {
                            self.saving = true;
                            self.busy = false;
                        }
                    }
                }
            }
            "print" => {
                if let Some(e) = &self.current {
                    self.jobs.enqueue(
                        Tag::Ignore,
                        "print.open",
                        json!([{ "title":e.name,"image":e.path }]),
                    )
                }
            }
            "fullscreen" => {
                if self.current.is_some() {
                    self.full(!self.fullscreen)
                }
            }
            "slideshow" => {
                if self.current.is_some() {
                    self.slide = !self.slide;
                    self.next_slide = now + 4000;
                    if self.slide {
                        self.full(true)
                    }
                }
            }
            "escape" => {
                if self.fullscreen {
                    self.slide = false;
                    self.full(false)
                } else if self.edit {
                    self.edit = false;
                    self.crop = None;
                    self.selection = None;
                    changed = true
                } else {
                    self.close_viewer();
                }
            }
            "thumbSmaller" | "thumbLarger" => {
                self.thumb_size = (self.thumb_size + if cmd == "thumbLarger" { 20. } else { -20. })
                    .clamp(80., 240.);
                self.clamp_scroll();
            }
            "wheel" => {
                if self.current.is_some() {
                    self.view
                        .zoom_at(w, h, (-n("deltaY") * 0.003).clamp(-1., 1.).exp(), x, y);
                    changed = true
                } else {
                    self.scroll = (self.scroll + n("deltaY")).clamp(0., self.max_scroll())
                }
            }
            "gesture" => {
                let kind = input["kind"].as_str().unwrap_or("");
                let phase = input["phase"].as_str().unwrap_or("");
                if kind == "pinch" {
                    let scale = n("dx");
                    if phase == "begin" {
                        self.pinch = 1.
                    } else if phase == "update" && scale > 0. && self.current.is_some() {
                        self.view.zoom_at(
                            w,
                            h,
                            scale / self.pinch.max(0.01),
                            self.view.width as f64 / 2.,
                            self.view.height as f64 / 2.,
                        );
                        self.pinch = scale;
                        changed = true
                    }
                } else if kind == "scroll" {
                    if self.current.is_some() {
                        let (rw, rh) = self.view.size(w, h);
                        let s = self.view.scale(w, h);
                        self.view.cx = (self.view.cx + n("dx") / (rw as f64 * s)).clamp(0., 1.);
                        self.view.cy = (self.view.cy + n("dy") / (rh as f64 * s)).clamp(0., 1.);
                        changed = true
                    } else {
                        self.scroll = (self.scroll + n("dy")).clamp(0., self.max_scroll())
                    }
                }
            }
            "pan" => {
                if input["button"].as_i64().unwrap_or(0) == 0 || input["button"] == 1 {
                    self.drag = Some(if self.edit {
                        self.selection = Some(((x, y), (x, y)));
                        Drag::Crop((x, y))
                    } else {
                        Drag::Pan(x, y, self.view.cx, self.view.cy)
                    });
                }
            }
            "range" => {
                let key = input["key"].as_str().unwrap_or("");
                if ["brightness", "contrast"].contains(&key) {
                    let frame = &input["frame"];
                    let left = frame[0].as_f64().unwrap_or(0.);
                    let width = frame[2].as_f64().unwrap_or(160.).max(1.);
                    self.drag = Some(Drag::Range(key.into(), left, width));
                    self.range(x);
                    changed = true
                }
            }
            "folderWheel" | "folderGesture" => {
                let delta = if cmd == "folderWheel" {
                    n("deltaY")
                } else if input["kind"] == "scroll" {
                    n("dy")
                } else {
                    0.
                };
                self.folder_scroll =
                    (self.folder_scroll + delta).clamp(0., self.folder_max_scroll());
            }
            "scrollbar" => {
                self.drag = Some(Drag::Scroll(n("y"), self.scroll));
            }
            "pointerMove" => match self.drag.as_ref() {
                Some(Drag::Pan(sx, sy, cx, cy)) => {
                    let (rw, rh) = self.view.size(w, h);
                    let scale = self.view.scale(w, h);
                    self.view.cx = (cx - (x - sx) / (rw as f64 * scale)).clamp(0., 1.);
                    self.view.cy = (cy - (y - sy) / (rh as f64 * scale)).clamp(0., 1.);
                    changed = true
                }
                Some(Drag::Crop(start)) => {
                    let start = *start;
                    self.selection = Some((start, (x, y)));
                    self.crop = self.view.crop(w, h, start, (x, y));
                }
                Some(Drag::Range(..)) => {
                    self.range(x);
                    changed = true
                }
                Some(Drag::Scroll(sy, scroll)) => {
                    self.scroll = (scroll
                        + (n("y") - sy) * self.max_scroll() / (self.height - 100.).max(1.))
                    .clamp(0., self.max_scroll())
                }
                None => {}
            },
            "dragEnd" => {
                self.drag = None;
                changed = true
            }
            _ => {}
        }
        if !matches!(cmd, "noop" | "gesture" | "pointerMove" | "wheel") {
            self.checkpoint_dirty = true;
        }
        if matches!(cmd, "pointerMove" | "gesture" | "wheel") && (changed || self.current.is_none())
        {
            self.checkpoint_dirty = true;
        }
        if changed && self.current.is_some() {
            if self.quality_ms > 14. && matches!(cmd, "wheel" | "gesture" | "pointerMove" | "range")
            {
                self.view.quality = false;
                self.needs_refine = true;
                self.refine_at = now + 100;
                if self.busy && self.busy_quality {
                    if let Some(w) = &self.worker {
                        w.cancel()
                    }
                    self.busy = false;
                }
            }
            self.dirty = true;
        }
    }
    fn checkpoint(&mut self) {
        self.checkpoint_dirty = false;
        if self.window.is_empty() {
            return;
        }
        let state = json!({"sourcePath":self.args["path"],"nonce":self.args["nonce"],"path":self.current.as_ref().map(|e|&e.path),"folder":self.folder,"recursive":self.recursive,"view":self.view,"edit":self.edit,"crop":self.crop,"slide":self.slide,"scroll":self.scroll,"thumbSize":self.thumb_size});
        self.effects
            .push(json!({"host":true,"kind":"windowState","state":state}));
    }
    fn restore(&mut self, state: &Value, args: &Value, now: i64) {
        if !state.is_object()
            || state["sourcePath"] != args["path"]
            || state["nonce"] != args["nonce"]
        {
            return;
        }
        if let Some(folder) = state["folder"].as_str() {
            self.scan(folder.into(), state["recursive"] != false)
        }
        self.restore_scroll = state["scroll"]
            .as_f64()
            .filter(|x| x.is_finite() && *x >= 0.);
        self.thumb_size = state["thumbSize"]
            .as_f64()
            .filter(|x| x.is_finite())
            .unwrap_or(140.)
            .clamp(80., 240.);
        if let Some(path) = state["path"].as_str() {
            let path = PathBuf::from(path);
            if io::supported(&path) {
                self.open(
                    Entry {
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        path,
                        size: 0,
                        mtime: 0,
                        stamp: (0, 0, 0, 0),
                    },
                    now,
                );
                if let Ok(mut view) = serde_json::from_value::<View>(state["view"].clone()) {
                    view.zoom = view.zoom.clamp(0., 131072.);
                    view.cx = view.cx.clamp(0., 1.);
                    view.cy = view.cy.clamp(0., 1.);
                    view.rotation %= 4;
                    view.brightness = view.brightness.clamp(-100, 100);
                    view.contrast = view.contrast.clamp(-100, 100);
                    view.quality = true;
                    self.view = view;
                }
                self.edit = state["edit"] == true;
                self.slide = state["slide"] == true;
                self.crop = serde_json::from_value(state["crop"].clone()).ok().flatten();
            }
        }
    }
    fn range(&mut self, x: f64) {
        if let Some(Drag::Range(key, left, width)) = &self.drag {
            let v = (((x - left) / width).clamp(0., 1.) * 200. - 100.).round() as i32;
            if key == "brightness" {
                self.view.brightness = v
            } else {
                self.view.contrast = v
            }
        }
    }
}

fn now_ms() -> i64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec * 1000 + t.tv_nsec / 1_000_000
}
