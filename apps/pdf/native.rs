//! The native PDF viewer window (stage 1: the document, its pages and navigation).
//! Rust owns every decision: the window state machine, layout, render admission and
//! the scene. The transport only runs the helper calls Rust names (`pdf.open`,
//! `pdf.sizes`, `pdf.render`, `pdf.outline`, `pdf.release`, `pdf.close`, `pdf.recent`)
//! and keeps document and raster handles in the window's lease.
use crate::native_view::{async_jobs::Jobs, clock, NativeApp};
use crate::Model;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::rc::Rc;

#[path = "native_ui.rs"]
mod ui;

const GAP: f64 = 14.;
const THUMB_WIDTH: f64 = 116.;
const SIDEBAR_HEADER: f64 = 38.;
const THUMB_BUDGET: f64 = 8. * 1024. * 1024.;

enum Tag {
    Recent,
    Open(u64),
    Sizes(u64),
    Outline(u64),
    Render { token: u64, thumb: bool, epoch: u64, started: i64 },
    Release,
    Close,
}
#[derive(Clone, PartialEq)]
enum Zoom {
    Width,
    Page,
    Scale(f64),
}
impl Zoom {
    fn json(&self) -> Value {
        match self {
            Zoom::Width => json!("width"),
            Zoom::Page => json!("page"),
            Zoom::Scale(z) => json!(z),
        }
    }
}
pub struct App {
    model: Model,
    thumbs: Model,
    jobs: Jobs<Tag>,
    booted: bool,
    theme: Rc<Value>,
    width: f64,
    height: f64,
    home: String,
    nonce: Value,
    path: Option<String>,
    password: String,
    need_pass: bool,
    error: Value,
    opening: bool,
    doc: Option<u64>,
    epoch: u64,
    pages: usize,
    doc_title: String,
    sizes: Vec<[f64; 2]>,
    sizes_rev: u64,
    zoom: Zoom,
    rot: i64,
    single: bool,
    invert: bool,
    side: Option<String>,
    offset: f64,
    offset_x: f64,
    page: usize,
    keep_page: Option<usize>,
    layout_key: Option<(u64, i64, u64)>,
    sidebar_key: Option<(u64, i64)>,
    scale: f64,
    pct: f64,
    layout_height: f64,
    max_offset: f64,
    frames: Vec<Value>,
    images: HashMap<String, i64>,
    thumb_images: HashMap<String, i64>,
    outline_state: u8,
    message: Option<(Value, i64)>,
    recent: Option<Vec<(String, String)>>,
    recent_requested: bool,
    title: Option<String>,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            model: Model::default(),
            thumbs: Model::default(),
            jobs: Jobs::default(),
            booted: false,
            theme: Rc::new(Value::Null),
            width: 980.,
            height: 680.,
            home: String::new(),
            nonce: Value::Null,
            path: None,
            password: String::new(),
            need_pass: false,
            error: Value::Null,
            opening: false,
            doc: None,
            epoch: 0,
            pages: 0,
            doc_title: String::new(),
            sizes: vec![],
            sizes_rev: 0,
            zoom: Zoom::Width,
            rot: 0,
            single: false,
            invert: false,
            side: Some("thumbs".into()),
            offset: 0.,
            offset_x: 0.,
            page: 0,
            keep_page: None,
            layout_key: None,
            sidebar_key: None,
            scale: 1.,
            pct: 100.,
            layout_height: 0.,
            max_offset: 0.,
            frames: vec![],
            images: HashMap::new(),
            thumb_images: HashMap::new(),
            outline_state: 0,
            message: None,
            recent: None,
            recent_requested: false,
            title: None,
            last_tree: Value::Null,
        }
    }
}
fn ask(model: &mut Model, input: Value) -> Option<Value> {
    let reply = model.request(&input);
    if reply.get("error").is_some() { None } else { Some(reply) }
}
fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}
/// A whole non-negative number, whether the transport wrote 783 or 783.0.
fn whole(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_f64().filter(|n| *n >= 0. && n.fract() == 0. && *n < 1e9).map(|n| n as u64))
}
fn number(v: &Value, fallback: f64) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(fallback)
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool {
        true
    }
    fn set_native_appearance(&mut self, theme: Rc<Value>) {
        self.theme = theme;
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted.then_some(if self.jobs.busy() { 50. } else { 250. })
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, clock::now())
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        self.key_at(key, chr, flags, clock::now())
    }
}
impl App {
    // ---- documents ----

    fn close_document(&mut self) {
        if let Some(doc) = self.doc.take() {
            self.jobs.enqueue(Tag::Close, "pdf.close", json!([doc]));
        }
        for image in self.images.drain().chain(self.thumb_images.drain()).map(|(_, id)| id) {
            self.jobs.enqueue(Tag::Release, "pdf.release", json!([image]));
        }
        ask(&mut self.model, json!({"cmd":"cache","op":"close"}));
        ask(&mut self.thumbs, json!({"cmd":"cache","op":"close"}));
        self.jobs.cancel(|tag| matches!(tag, Tag::Open(_) | Tag::Sizes(_) | Tag::Outline(_) | Tag::Render { .. }));
    }
    fn open(&mut self, path: String, now: i64) {
        self.close_document();
        self.model = Model::default();
        self.thumbs = Model::default();
        ask(&mut self.thumbs, json!({"cmd":"cache","op":"configure","budget":THUMB_BUDGET}));
        self.epoch += 1;
        self.title = Some(basename(&path));
        self.path = Some(path.clone());
        self.error = Value::Null;
        self.opening = true;
        self.pages = 0;
        self.sizes.clear();
        self.sizes_rev += 1;
        self.offset = 0.;
        self.offset_x = 0.;
        self.page = 0;
        self.frames.clear();
        self.layout_key = None;
        self.sidebar_key = None;
        self.outline_state = 0;
        self.message = None;
        let _ = now;
        self.jobs.enqueue(Tag::Open(self.epoch), "pdf.open", json!([path, self.password]));
    }
    fn apply(&mut self, done: crate::native_view::async_jobs::Completed<Tag>, now: i64) {
        match (done.tag, done.result) {
            (Tag::Recent, result) => {
                self.recent = Some(
                    result
                        .ok()
                        .and_then(|list| list.as_array().cloned())
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|e| Some((e["name"].as_str()?.to_owned(), e["path"].as_str()?.to_owned())))
                        .collect(),
                );
            }
            (Tag::Open(epoch), result) => {
                if epoch != self.epoch {
                    if let Some(doc) = result.ok().and_then(|v| v["doc"].as_u64()) {
                        self.jobs.enqueue(Tag::Close, "pdf.close", json!([doc]));
                    }
                    return;
                }
                self.opening = false;
                match result {
                    Ok(info) => {
                        let Some(doc) = info["doc"].as_u64() else { return };
                        self.doc = Some(doc);
                        self.need_pass = false;
                        self.error = Value::Null;
                        self.pages = info["pages"].as_u64().unwrap_or(0) as usize;
                        self.doc_title = info["title"].as_str().unwrap_or("").into();
                        self.sizes = sizes_of(&info["sizes"]);
                        self.sizes_rev += 1;
                        ask(&mut self.model, json!({"cmd":"edit","op":"open","pages":self.pages}));
                        if self.pages > 1 {
                            self.jobs.enqueue(Tag::Sizes(epoch), "pdf.sizes", json!([doc]));
                        }
                    }
                    Err(code) if code == "password" => {
                        self.need_pass = true;
                        self.error = if self.password.is_empty() { Value::Null } else { json!({"key":"pdf.wrongPassword"}) };
                    }
                    Err(code) if code == "invalid" => self.error = json!({"key":"pdf.invalid"}),
                    Err(message) => self.error = json!({"key":"pdf.openError","params":{"error":message}}),
                }
            }
            (Tag::Sizes(epoch), Ok(all)) if epoch == self.epoch => {
                let sizes = sizes_of(&all["sizes"]);
                if !sizes.is_empty() {
                    self.sizes = sizes;
                    self.sizes_rev += 1;
                }
            }
            (Tag::Outline(epoch), result) if epoch == self.epoch => {
                let items = result.ok().map(|r| r["items"].clone()).filter(Value::is_array).unwrap_or(json!([]));
                ask(&mut self.model, json!({"cmd":"sidebar","op":"outline","items":items}));
                self.outline_state = 2;
            }
            (Tag::Render { token, thumb, epoch, started }, result) => {
                let model = if thumb { &mut self.thumbs } else { &mut self.model };
                let done = result.ok().and_then(|r| Some((r["image"].as_i64()?, whole(&r["w"])?, whole(&r["h"])?)));
                let Some((image, w, h)) = done else {
                    ask(model, json!({"cmd":"cache","op":"fail","token":token}));
                    return;
                };
                if epoch != self.epoch {
                    self.jobs.enqueue(Tag::Release, "pdf.release", json!([image]));
                    return;
                }
                let reply = ask(model, json!({"cmd":"cache","op":"complete","token":token,"w":w,"h":h,"ms":now - started,"now":now}));
                let value = reply.as_ref().map(|r| r["value"].clone()).filter(|v| !v.is_null());
                if let Some(reply) = &reply {
                    self.release(thumb, &reply["release"]);
                }
                match value.and_then(|v| v["key"].as_str().map(str::to_owned)) {
                    Some(key) => {
                        let map = if thumb { &mut self.thumb_images } else { &mut self.images };
                        map.insert(key, image);
                    }
                    None => self.jobs.enqueue(Tag::Release, "pdf.release", json!([image])),
                }
            }
            _ => {}
        }
    }
    fn release(&mut self, thumb: bool, keys: &Value) {
        let map = if thumb { &mut self.thumb_images } else { &mut self.images };
        for key in keys.as_array().into_iter().flatten().filter_map(Value::as_str) {
            if let Some(image) = map.remove(key) {
                self.jobs.enqueue(Tag::Release, "pdf.release", json!([image]));
            }
        }
    }
    // ---- geometry and rendering ----

    fn filled_sizes(&self) -> Vec<[f64; 2]> {
        let first = self.sizes.first().copied().unwrap_or([595., 842.]);
        (0..self.pages).map(|i| self.sizes.get(i).copied().unwrap_or(first)).collect()
    }
    fn view_size(&self) -> (f64, f64) {
        let side = if self.side.is_some() && self.doc.is_some() { ui::SIDEBAR } else { 0. };
        ((self.width - side).max(0.), (self.height - ui::TOOLBAR - ui::STATUS).max(0.))
    }
    fn go_page(&mut self, page: usize, rect: Option<&Value>) {
        let Some(next) = ask(
            &mut self.model,
            json!({"cmd":"view","op":"goto","page":page,"rect":rect.cloned().unwrap_or(Value::Null)}),
        ) else {
            return;
        };
        if let Some(page) = next["page"].as_u64() {
            self.page = page as usize;
        }
        if let Some(offset) = next["offset"].as_f64() {
            self.offset = offset;
        }
    }
    fn layout(&mut self) {
        let sizes = self.filled_sizes();
        let (view_w, view_h) = self.view_size();
        let page = self.page.min(sizes.len().saturating_sub(1));
        let size = sizes.get(page).copied().unwrap_or([595., 842.]);
        let shown = ask(&mut self.model, json!({"cmd":"shown","size":size,"rot":self.rot})).unwrap_or(json!(size));
        let scale = ask(
            &mut self.model,
            json!({"cmd":"viewer","op":"scale","zoom":self.zoom.json(),"size":shown,"view":{"w":view_w,"h":view_h}}),
        );
        let Some(scale) = scale else { return };
        self.scale = number(&scale["scale"], 1.);
        self.pct = number(&scale["pct"], 100.);
        let key = (self.sizes_rev, self.rot, self.scale.to_bits());
        if self.layout_key != Some(key) {
            if let Some(layout) = ask(&mut self.model, json!({"cmd":"layout","sizes":sizes,"rot":self.rot,"scale":self.scale,"gap":GAP})) {
                self.layout_height = number(&layout["height"], 0.);
                self.layout_key = Some(key);
                if let Some(top) = self.keep_page.take() {
                    // after a zoom change: keep in view what was at the top
                    let request = self.viewport_request(view_w, view_h);
                    ask(&mut self.model, request);
                    self.go_page(top, None);
                }
            }
        }
        if self.sidebar_key != Some((self.sizes_rev, self.rot)) {
            let request = json!({"cmd":"sidebar","op":"configure","sizes":self.filled_sizes(),"rot":self.rot});
            ask(&mut self.model, request);
            self.sidebar_key = Some((self.sizes_rev, self.rot));
        }
    }
    fn viewport_request(&self, view_w: f64, view_h: f64) -> Value {
        json!({"cmd":"view","op":"viewport","mode":self.mode(),"page":self.page,"offset":self.offset,"offsetX":self.offset_x,"view":{"w":view_w,"h":view_h}})
    }
    fn mode(&self) -> &'static str {
        if self.single { "single" } else { "continuous" }
    }
    fn viewport(&mut self, now: i64) {
        let (view_w, view_h) = self.view_size();
        let request = self.viewport_request(view_w, view_h);
        let Some(mut state) = ask(&mut self.model, request) else { return };
        let (max_offset, max_x) = (number(&state["maxOffset"], 0.), number(&state["maxX"], 0.));
        let (offset, offset_x) = (self.offset.clamp(0., max_offset), self.offset_x.clamp(0., max_x));
        if offset != self.offset || offset_x != self.offset_x {
            self.offset = offset;
            self.offset_x = offset_x;
            let request = self.viewport_request(view_w, view_h);
            if let Some(again) = ask(&mut self.model, request) {
                state = again;
            }
        }
        self.max_offset = max_offset;
        if !self.single {
            let current = state["currentPage"].as_u64().unwrap_or(0) as usize;
            if current != self.page {
                self.page = current;
            }
        }
        self.frames = state["frames"].as_array().cloned().unwrap_or_default();
        let (rot, invert) = (self.rot, self.invert);
        let mut keep = vec![];
        for job in state["renders"].as_array().into_iter().flatten() {
            let (Some(page), Some(width)) = (job["page"].as_u64(), job["width"].as_f64()) else { continue };
            keep.push(page);
            self.want(false, page, width, rot, invert, job["priority"].as_i64().unwrap_or(0), now);
        }
        if let Some(reply) = ask(&mut self.model, json!({"cmd":"cache","op":"drop","keep":keep})) {
            let tokens: Vec<u64> = reply["value"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
            self.jobs.cancel(|tag| matches!(tag, Tag::Render { token, thumb: false, .. } if tokens.contains(token)));
        }
        self.thumbnails(now);
    }
    fn want(&mut self, thumb: bool, page: u64, width: f64, rot: i64, invert: bool, priority: i64, now: i64) {
        let Some(doc) = self.doc else { return };
        let model = if thumb { &mut self.thumbs } else { &mut self.model };
        let Some(reply) = ask(model, json!({"cmd":"cache","op":"begin","page":page,"width":width,"rot":rot,"inv":invert})) else { return };
        self.release(thumb, &reply["release"]);
        let ticket = &reply["value"];
        let Some(token) = ticket["token"].as_u64() else { return };
        let width = number(&ticket["width"], width);
        self.jobs.enqueue(
            Tag::Render { token, thumb, epoch: self.epoch, started: now },
            "pdf.render",
            json!([doc, page, width, rot, invert, priority]),
        );
    }
    fn thumbnails(&mut self, now: i64) {
        if self.side.as_deref() == Some("outline") {
            if self.outline_state == 0 {
                if let Some(doc) = self.doc {
                    self.outline_state = 1;
                    self.jobs.enqueue(Tag::Outline(self.epoch), "pdf.outline", json!([doc]));
                }
            }
            return;
        }
        if self.side.is_none() {
            return;
        }
        let height = (self.height - ui::TOOLBAR - ui::STATUS - SIDEBAR_HEADER).max(0.);
        let Some(reply) = ask(&mut self.model, json!({"cmd":"sidebar","op":"viewport","height":height,"page":self.page})) else { return };
        let (rot, invert) = (self.rot, self.invert);
        for frame in reply["frames"].as_array().into_iter().flatten() {
            if let Some(page) = frame["page"].as_u64() {
                self.want(true, page, THUMB_WIDTH, rot, invert, 0, now);
            }
        }
    }
    fn image_for(&mut self, thumb: bool, page: u64, width: f64) -> Option<i64> {
        let (rot, invert) = (self.rot, self.invert);
        let model = if thumb { &mut self.thumbs } else { &mut self.model };
        let probe = |op: &str| json!({"cmd":"cache","op":op,"page":page,"width":width,"rot":rot,"inv":invert});
        let entry = ask(model, probe("get")).map(|r| r["value"].clone()).filter(|v| !v.is_null()).or_else(|| {
            ask(model, probe("best")).map(|r| r["value"].clone()).filter(|v| !v.is_null())
        })?;
        let key = entry["key"].as_str()?;
        let map = if thumb { &self.thumb_images } else { &self.images };
        map.get(key).copied()
    }
    // ---- input ----

    fn zoom_step(&mut self, step: i64) {
        if let Some(z) = ask(&mut self.model, json!({"cmd":"viewer","op":"zoom","scale":self.scale,"step":step})) {
            self.set_zoom(Zoom::Scale(number(&z, 1.)));
        }
    }
    fn set_zoom(&mut self, zoom: Zoom) {
        self.keep_page = Some(self.page);
        self.zoom = zoom;
    }
    fn rotate(&mut self, delta: i64) {
        if let Some(r) = ask(&mut self.model, json!({"cmd":"viewer","op":"rotate","rot":self.rot,"delta":delta})) {
            self.rot = r.as_i64().unwrap_or(0);
        }
    }
    fn scroll(&mut self, value: f64, delta: f64, max: f64) -> f64 {
        ask(&mut self.model, json!({"cmd":"viewer","op":"scroll","value":value,"delta":delta,"max":max}))
            .map(|v| number(&v, value))
            .unwrap_or(value)
    }
    fn max_x(&mut self) -> f64 {
        let (view_w, view_h) = self.view_size();
        let request = self.viewport_request(view_w, view_h);
        let width = ask(&mut self.model, request);
        width.map(|v| number(&v["maxX"], 0.)).unwrap_or(0.)
    }
    fn wheel(&mut self, v: &Value, now: i64) {
        let event = json!({"deltaY":v["deltaY"],"ctrl":v["ctrl"],"meta":v["meta"],"shift":v["shift"],
            "precise":false,"notches":v["notches"].as_i64().is_some()});
        let Some(action) = ask(&mut self.model, json!({"cmd":"viewer","op":"wheel","single":self.single,"now":now as f64,"event":event})) else { return };
        match action["kind"].as_str() {
            Some("zoom") => self.zoom_step(action["step"].as_i64().unwrap_or(0)),
            Some("page") => self.go_page((self.page as i64 + action["delta"].as_i64().unwrap_or(0)).max(0) as usize, None),
            Some("horizontal") => {
                let max = self.max_x();
                self.offset_x = self.scroll(self.offset_x, number(&action["delta"], 0.), max);
            }
            Some("vertical") => self.offset = self.scroll(self.offset, number(&action["delta"], 0.), self.max_offset),
            _ => {}
        }
    }
    pub fn key_at(&mut self, key: &str, chr: &str, flags: i32, now: i64) -> Option<Value> {
        let event = json!({"key":key,"base":key,"char":chr,"shift":flags & 1 != 0,"ctrl":flags & 2 != 0,"alt":flags & 4 != 0,"meta":flags & 16 != 0});
        let action = ask(&mut self.model, json!({"cmd":"viewer","op":"key","editing":false,"hasDoc":self.doc.is_some(),"event":event}))?;
        let action = action.as_str()?.to_owned();
        let last = self.pages.saturating_sub(1);
        let height = self.view_size().1;
        match action.as_str() {
            "rotateLeft" => self.rotate(-1),
            "rotateRight" => self.rotate(1),
            "zoomIn" => self.zoom_step(1),
            "zoomOut" => self.zoom_step(-1),
            "zoomActual" => self.set_zoom(Zoom::Scale(1.)),
            "zoomWidth" => self.set_zoom(Zoom::Width),
            "escape" => self.message = None,
            "pageDown" | "down" | "pageUp" | "up" => {
                let forward = action == "pageDown" || action == "down";
                if self.single {
                    self.go_page((self.page as i64 + if forward { 1 } else { -1 }).max(0) as usize, None);
                } else if let Some(v) = ask(
                    &mut self.model,
                    json!({"cmd":"viewer","op":"keyScroll","action":action,"value":self.offset,"max":self.max_offset,"height":height}),
                ) {
                    self.offset = number(&v, self.offset);
                }
            }
            "nextPage" => self.go_page(self.page + 1, None),
            "previousPage" => self.go_page(self.page.saturating_sub(1), None),
            "firstPage" => self.go_page(0, None),
            "lastPage" => self.go_page(last, None),
            _ => return None,
        }
        Some(self.request_at(&json!({"cmd":"noop"}), now))
    }
    fn pick_path(&self, args: &Value) -> Option<String> {
        let path = args["path"].as_str().filter(|p| !p.is_empty())?;
        Some(if path.starts_with('/') { path.to_owned() } else { format!("{}/{path}", self.home) })
    }
    fn command(&mut self, v: &Value, now: i64) {
        let key = &v["key"];
        match v["cmd"].as_str().unwrap_or("") {
            "toggleSide" => self.side = if self.side.is_some() { None } else { Some("thumbs".into()) },
            "side" => self.side = key.as_str().map(str::to_owned),
            "page" => self.go_page(key.as_u64().unwrap_or(0) as usize, None),
            "pick" => {
                if let Some(page) = key.as_str().and_then(|k| {
                    ask(&mut self.model, json!({"cmd":"sidebar","op":"pick","key":k})).and_then(|r| r.as_u64())
                }) {
                    self.go_page(page as usize, None);
                }
            }
            "toggle" => {
                if let Some(k) = key.as_str() {
                    ask(&mut self.model, json!({"cmd":"sidebar","op":"toggle","key":k}));
                }
            }
            "scroll" => {
                ask(&mut self.model, json!({"cmd":"sidebar","op":"scroll","delta":number(&v["deltaY"],0.)}));
            }
            "zoom" => self.zoom_step(key.as_i64().unwrap_or(0)),
            "zoomToggle" => self.set_zoom(if self.zoom == Zoom::Width { Zoom::Page } else { Zoom::Width }),
            "rotate" => self.rotate(key.as_i64().unwrap_or(1)),
            "invert" => self.invert = !self.invert,
            "mode" => self.single = key == "single",
            "wheel" => self.wheel(v, now),
            "open" => {
                if let Some(path) = key.as_str() {
                    self.password.clear();
                    self.need_pass = false;
                    self.open(path.to_owned(), now);
                }
            }
            "passwordChange" => self.password = v["value"].as_str().unwrap_or("").to_owned(),
            "password" => {
                if let Some(path) = self.path.clone() {
                    self.need_pass = false;
                    self.open(path, now);
                }
            }
            "reopen" => {
                if let Some(path) = self.path.clone() {
                    self.password.clear();
                    self.open(path, now);
                }
            }
            _ => {}
        }
    }
    pub fn request_at(&mut self, v: &Value, now: i64) -> Value {
        match v["cmd"].as_str().unwrap_or("") {
            "icon" => return ui::icon(number(&v["size"], 32.)),
            "sync" => self.context(v, now),
            "next" => {
                for done in self.jobs.complete(&v["result"], now) {
                    self.apply(done, now);
                }
            }
            "noop" => {}
            _ => self.command(v, now),
        }
        if self.doc.is_some() {
            self.layout();
            self.viewport(now);
        }
        let mut out = json!({});
        if let Some(effect) = self.jobs.pump(now) {
            out["effect"] = effect;
        }
        if let Some(title) = self.title.take() {
            out["title"] = json!(title);
        }
        if v["cmd"] == "sync" || v["cmd"] == "next" || !self.last_tree.is_null() {
            let tree = self.scene(now);
            if tree != self.last_tree {
                self.last_tree = tree.clone();
                out["tree"] = tree;
            } else {
                out["same"] = json!(true);
            }
        }
        out
    }
    fn context(&mut self, v: &Value, now: i64) {
        if v["context"]["theme"].is_object() {
            self.theme = Rc::new(v["context"]["theme"].clone());
        }
        self.width = number(&v["context"]["width"], self.width).max(1.);
        self.height = number(&v["context"]["height"], self.height).max(1.);
        if !self.booted {
            if let Some(owner) = v["args"]["__nativeOwner"].as_i64().filter(|i| *i > 0) {
                self.booted = true;
                self.jobs.bind(owner);
                self.home = v["context"]["home"].as_str().map(str::to_owned).or_else(|| std::env::var("HOME").ok()).unwrap_or_default();
                if let Some(path) = self.pick_path(&v["args"]) {
                    self.nonce = v["args"]["nonce"].clone();
                    self.open(path, now);
                }
            }
        } else if !v["args"]["nonce"].is_null() && v["args"]["nonce"] != self.nonce {
            self.nonce = v["args"]["nonce"].clone();
            if let Some(path) = self.pick_path(&v["args"]) {
                self.password.clear();
                self.open(path, now);
            }
        }
        if self.booted && self.path.is_none() && !self.recent_requested {
            self.recent_requested = true;
            self.jobs.enqueue(Tag::Recent, "pdf.recent", json!([]));
        }
        if self.message.as_ref().is_some_and(|(_, until)| *until <= now) {
            self.message = None;
        }
    }
    fn scene(&mut self, _now: i64) -> Value {
        let (view_w, view_h) = self.view_size();
        let mut images = HashMap::new();
        let frames = self.frames.clone();
        for frame in &frames {
            if let (Some(page), Some(width)) = (frame["page"].as_u64(), frame["width"].as_f64()) {
                if let Some(image) = self.image_for(false, page, width) {
                    images.insert(page, image);
                }
            }
        }
        let side = self.side.clone();
        let has_doc = self.doc.is_some();
        let sidebar = if has_doc && side.is_some() {
            let mut thumbs = json!({});
            if side.as_deref() == Some("thumbs") {
                let height = (self.height - ui::TOOLBAR - ui::STATUS - SIDEBAR_HEADER).max(0.);
                if let Some(reply) = ask(&mut self.model, json!({"cmd":"sidebar","op":"viewport","height":height,"page":self.page})) {
                    for frame in reply["frames"].as_array().into_iter().flatten() {
                        if let Some(page) = frame["page"].as_u64() {
                            if let Some(image) = self.image_for(true, page, THUMB_WIDTH) {
                                thumbs[page.to_string()] = json!(image);
                            }
                        }
                    }
                }
            }
            let theme: serde_json::Map<String, Value> = ["text", "textDim", "accent", "separator", "mode", "control", "controlRadiusSmall", "sidebarBg"]
                .iter()
                .map(|k| ((*k).to_owned(), self.theme[*k].clone()))
                .collect();
            ask(
                &mut self.model,
                json!({"cmd":"sidebar","op":"tree","side":side,"images":thumbs,"invert":self.invert,
                    "loading":side.as_deref() == Some("outline") && self.outline_state != 2,"theme":theme,"document":self.epoch}),
            )
            .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let scrollbar = (has_doc && !self.single && self.max_offset > 0. && self.layout_height > 0.).then(|| {
            let height = (view_h * view_h / self.layout_height).max(30.);
            ((view_h - height) * (self.offset / self.max_offset), height)
        });
        let status = match &self.message {
            Some((message, _)) => message.clone(),
            None => json!(self.doc_title),
        };
        let theme = self.theme.clone();
        let error = self.error.clone();
        let password = self.password.clone();
        let page_images = images;
        let frames_ref = &self.frames;
        let facts = ui::Facts {
            theme: &theme,
            width: self.width,
            has_doc,
            opening: self.opening,
            need_pass: self.need_pass,
            password: &password,
            error: &error,
            path: self.path.is_some(),
            pages: self.pages,
            page: self.page,
            pct: self.pct,
            single: self.single,
            invert: self.invert,
            side: side.as_deref(),
            status,
            frames: frames_ref,
            images: &page_images,
            scrollbar,
            recent: self.recent.as_deref(),
            sidebar,
            view_h,
            view_w,
        };
        ui::tree(&facts)
    }
}
fn sizes_of(value: &Value) -> Vec<[f64; 2]> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some([s[0].as_f64().filter(|n| n.is_finite() && *n > 0.)?, s[1].as_f64().filter(|n| n.is_finite() && *n > 0.)?]))
        .collect()
}
#[cfg(test)]
#[path = "native_window_tests.rs"]
mod tests;
