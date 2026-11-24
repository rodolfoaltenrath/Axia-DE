//! The native PDF viewer window (stage 1: the document, its pages and navigation).
//! Rust owns every decision: the window state machine, layout, render admission and
//! the scene. The transport only runs the helper calls Rust names (`pdf.open`,
//! `pdf.sizes`, `pdf.render`, `pdf.outline`, `pdf.release`, `pdf.close`, `pdf.recent`)
//! and keeps document and raster handles in the window's lease.
use crate::native_view::{async_jobs::Jobs, clock, i18n::I18n, NativeApp};
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
const MESSAGE_MS: i64 = 3500;
const SELECT_THROTTLE_MS: i64 = 50;
const DOUBLE_CLICK_MS: i64 = 400;
const FIND_DEBOUNCE_MS: i64 = 350;
const TOOL_COLORS: [&str; 5] = ["#ffd60a", "#30d158", "#64d2ff", "#ff6482", "#bf5af2"];

enum Tag {
    Recent,
    Open(u64),
    Sizes(u64),
    Outline(u64),
    Render { token: u64, thumb: bool, epoch: u64, started: i64 },
    Release,
    Close,
    Info { page: usize, stamp: Value, epoch: u64 },
    Select { token: u64, page: usize, epoch: u64, highlight: Option<String> },
    Events(u64),
    Edit { ticket: u64, epoch: u64 },
    Ignore,
}
struct Drag {
    page: usize,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    moved: bool,
    last: i64,
    ink: bool,
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
    info: HashMap<usize, Option<Value>>,
    hits: Option<Value>,
    find_query: String,
    find_due: Option<i64>,
    find_focus: u64,
    goto: Option<String>,
    selection: Option<Value>,
    drag: Option<Drag>,
    capture: Option<bool>,
    last_click: i64,
    events_due: i64,
    events_pending: bool,
    tool: String,
    color: usize,
    edit_state: Value,
    ink: Option<Value>,
    i18n: I18n,
    locale_revision: u64,
    window: String,
    menus: Value,
    remembered: Value,
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
            info: HashMap::new(),
            hits: None,
            find_query: String::new(),
            find_due: None,
            find_focus: 0,
            goto: None,
            selection: None,
            drag: None,
            capture: None,
            last_click: 0,
            events_due: 0,
            events_pending: false,
            tool: "select".into(),
            color: 0,
            edit_state: Value::Null,
            ink: None,
            i18n: I18n::default(),
            locale_revision: 0,
            window: String::new(),
            menus: Value::Null,
            remembered: Value::Null,
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
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
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
        self.info.clear();
        self.hits = None;
        self.selection = None;
        self.drag = None;
        self.goto = None;
        self.events_pending = false;
        self.edit_state = Value::Null;
        self.ink = None;
        self.events_due = now + 250;
        self.find_due = (!self.find_query.is_empty()).then_some(now);
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
            (Tag::Info { page, stamp, epoch }, Ok(info)) if epoch == self.epoch => {
                let reply = ask(&mut self.model, json!({"cmd":"edit","op":"infoReply","page":page,"stamp":stamp,"info":info}));
                if let Some(value) = reply.filter(|v| !v.is_null()) {
                    self.info.insert(page, Some(value));
                }
            }
            (Tag::Info { page, epoch, .. }, Err(_)) if epoch == self.epoch => {
                self.info.insert(page, Some(json!({"links":[],"fields":[],"notes":[]})));
            }
            (Tag::Select { token, page, epoch, highlight }, Ok(result)) if epoch == self.epoch => {
                if let Some(color) = highlight {
                    self.edit("highlightReply", json!({"token":token,"page":page,"result":result,"color":color}), now);
                    return;
                }
                let reply = ask(&mut self.model, json!({"cmd":"viewer","op":"selectionReply","token":token,"page":page,"result":result}));
                if let Some(value) = reply.filter(|v| !v.is_null()) {
                    self.selection = Some(value);
                }
            }
            (Tag::Edit { ticket, epoch }, result) if epoch == self.epoch => {
                let (ok, message) = match result {
                    Ok(_) => (true, String::new()),
                    Err(message) => (false, message),
                };
                self.edit("complete", json!({"ticket":ticket,"ok":ok,"message":message}), now);
            }
            (Tag::Events(epoch), result) if epoch == self.epoch => {
                self.events_pending = false;
                for event in result.ok().and_then(|r| r.as_array().cloned()).unwrap_or_default() {
                    self.event(&event, now);
                }
            }
            _ => {}
        }
    }
    /// What the helper pushed: search results page by page, search end, a crashed reader.
    fn event(&mut self, event: &Value, now: i64) {
        match event["kind"].as_str() {
            Some("find") => {
                let reply = ask(&mut self.model, json!({"cmd":"viewer","op":"findAppend","token":event["token"],"page":event["page"],"rects":event["rects"]}));
                if let Some(reply) = reply.filter(|r| !r.is_null()) {
                    self.hits = reply["hits"].as_object().map(|_| reply["hits"].clone());
                    if let (Some(page), false) = (reply["go"]["page"].as_u64(), reply["go"].is_null()) {
                        let rect = reply["go"]["rect"].clone();
                        self.go_page(page as usize, Some(&rect));
                    }
                }
            }
            Some("findDone") => {
                let reply = ask(&mut self.model, json!({"cmd":"viewer","op":"findFinish","token":event["token"]}));
                if let Some(hits) = reply.filter(|h| h.is_object()) {
                    self.hits = Some(hits);
                }
            }
            Some("crash") => self.error = json!({"key":"pdf.crashed"}),
            Some("action") => {
                let action = &event["action"];
                self.command(&json!({"cmd":action[0],"key":action[1]}), now);
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
        if let (Some(first), Some(last)) = (state["first"].as_u64(), state["last"].as_i64().filter(|l| *l >= 0)) {
            for page in first as usize..=last as usize {
                self.want_info(page);
            }
        }
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
    fn want_info(&mut self, page: usize) {
        let Some(doc) = self.doc else { return };
        if self.info.contains_key(&page) {
            return;
        }
        self.info.insert(page, None);
        if let Some(stamp) = ask(&mut self.model, json!({"cmd":"edit","op":"infoStart","page":page})) {
            self.jobs.enqueue(Tag::Info { page, stamp, epoch: self.epoch }, "pdf.pageInfo", json!([doc, page]));
        }
    }
    fn set_message(&mut self, caption: Value, now: i64) {
        self.message = Some((caption, now + MESSAGE_MS));
    }
    fn timers(&mut self, now: i64) {
        if self.find_due.is_some_and(|due| due <= now) {
            self.find_due = None;
            self.run_find();
        }
        if self.doc.is_some() && !self.events_pending && now >= self.events_due {
            let searching = self.hits.as_ref().is_some_and(|h| h["done"] != true);
            self.events_pending = true;
            self.events_due = now + if searching { 100 } else { 1000 };
            if let Some(doc) = self.doc {
                self.jobs.enqueue(Tag::Events(self.epoch), "pdf.events", json!([doc]));
            }
        }
    }
    fn run_find(&mut self) {
        let Some(doc) = self.doc else { return };
        let Some(ticket) = ask(&mut self.model, json!({"cmd":"viewer","op":"findStart","query":self.find_query})) else { return };
        self.hits = ticket["hits"].as_object().map(|_| ticket["hits"].clone());
        if let (Some(query), Some(token)) = (ticket["query"].as_str(), ticket["token"].as_u64()) {
            self.events_due = 0;
            self.jobs.enqueue(Tag::Ignore, "pdf.find", json!([doc, query, token]));
        }
    }
    fn next_hit(&mut self, delta: i64) {
        let Some(reply) = ask(&mut self.model, json!({"cmd":"viewer","op":"findNext","delta":delta})).filter(|r| !r.is_null()) else { return };
        self.hits = Some(reply["hits"].clone());
        if let Some(page) = reply["go"]["page"].as_u64() {
            let rect = reply["go"]["rect"].clone();
            self.go_page(page as usize, Some(&rect));
        }
    }
    fn select(&mut self, page: usize, from: [f64; 2], to: [f64; 2], style: i64, highlight: Option<String>) {
        let Some(doc) = self.doc else { return };
        let Some(token) = ask(&mut self.model, json!({"cmd":"viewer","op":"selectionStart"})).and_then(|t| t.as_u64()) else { return };
        self.jobs.enqueue(
            Tag::Select { token, page, epoch: self.epoch, highlight },
            "pdf.select",
            json!([doc, page, from[0], from[1], to[0], to[1], style]),
        );
    }
    fn clear_selection(&mut self) {
        ask(&mut self.model, json!({"cmd":"viewer","op":"selectionClear"}));
        self.selection = None;
    }
    fn hit(&mut self, x: f64, y: f64) -> Option<Value> {
        let origin = if self.side.is_some() && self.doc.is_some() { ui::SIDEBAR } else { 0. };
        ask(&mut self.model, json!({"cmd":"view","op":"hit","x":x - origin,"y":y - ui::TOOLBAR})).filter(|v| !v.is_null())
    }
    fn item_at(&mut self, hit: &Value, list: &str) -> Option<Value> {
        let page = hit["page"].as_u64()? as usize;
        let items = self.info.get(&page)?.as_ref()?[list].clone();
        let index = ask(&mut self.model, json!({"cmd":"viewer","op":"hitItem","items":items,"x":hit["x"],"y":hit["y"]}))?.as_i64()?;
        items.get(usize::try_from(index).ok()?).cloned()
    }
    fn frame_of(&self, page: usize) -> Option<(f64, f64)> {
        let frame = self.frames.iter().find(|f| f["page"].as_u64() == Some(page as u64))?;
        Some((frame["left"].as_f64()?, frame["top"].as_f64()?))
    }
    fn pointer_down(&mut self, v: &Value, now: i64) {
        if v["button"].as_i64() == Some(2) {
            return;
        }
        self.edit("fieldCancel", json!({}), now);
        let Some(hit) = self.hit(number(&v["x"], -1.), number(&v["y"], -1.)) else {
            self.clear_selection();
            return;
        };
        let (page, x, y) = (hit["page"].as_u64().unwrap_or(0) as usize, number(&hit["x"], 0.), number(&hit["y"], 0.));
        match self.tool.as_str() {
            "note" => {
                self.edit("noteOpen", json!({"point":hit}), now);
                return;
            }
            "ink" => {
                self.drag = Some(Drag { page, x0: x, y0: y, x1: x, y1: y, moved: false, last: 0, ink: true });
                self.edit("inkStart", json!({"point":hit}), now);
                self.capture = Some(true);
                return;
            }
            "select" => {
                if let Some(field) = self.item_at(&hit, "fields").filter(|f| f["f"]["ro"] != true) {
                    self.edit("fieldOpen", json!({"page":page,"field":field}), now);
                    return;
                }
                if let Some(link) = self.item_at(&hit, "links") {
                    if let Some(target) = link["page"].as_i64().filter(|p| *p >= 0) {
                        self.go_page(target as usize, None);
                    } else if let Some(uri) = link["uri"].as_str() {
                        self.jobs.enqueue(Tag::Ignore, "clipboard.write", json!([uri]));
                        let shown: String = uri.chars().take(80).collect();
                        self.set_message(json!({"key":"pdf.linkCopied","params":{"uri":shown}}), now);
                    }
                    return;
                }
            }
            _ => {}
        }
        self.drag = Some(Drag { page, x0: x, y0: y, x1: x, y1: y, moved: false, last: 0, ink: false });
        self.clear_selection();
        self.capture = Some(true);
    }
    fn pointer_move(&mut self, v: &Value, now: i64) {
        let Some(hit) = self.drag.as_ref().map(|d| d.page).and(self.hit(number(&v["x"], -1.), number(&v["y"], -1.))) else { return };
        let Some(drag) = self.drag.as_mut() else { return };
        if hit["page"].as_u64() != Some(drag.page as u64) {
            return;
        }
        if drag.ink {
            let scale = self.scale;
            self.edit("inkAppend", json!({"point":hit,"scale":scale}), now);
            return;
        }
        drag.x1 = number(&hit["x"], drag.x1);
        drag.y1 = number(&hit["y"], drag.y1);
        drag.moved = true;
        if now - drag.last > SELECT_THROTTLE_MS {
            drag.last = now;
            let (page, from, to) = (drag.page, [drag.x0, drag.y0], [drag.x1, drag.y1]);
            self.select(page, from, to, 0, None);
        }
    }
    fn drag_end(&mut self, now: i64) {
        self.capture = Some(false);
        let Some(drag) = self.drag.take() else { return };
        if drag.ink {
            return;
        }
        if !drag.moved {
            if now - self.last_click < DOUBLE_CLICK_MS {
                self.select(drag.page, [drag.x0, drag.y0], [drag.x0 + 0.5, drag.y0 + 0.5], 1, None);
            }
            self.last_click = now;
            return;
        }
        let highlight = (self.tool == "highlight").then(|| TOOL_COLORS[self.color].to_owned());
        self.select(drag.page, [drag.x0, drag.y0], [drag.x1, drag.y1], 0, highlight);
    }
    // ---- editing ----

    /// Sends one edit operation and applies the packet it returns.
    fn edit(&mut self, op: &str, mut facts: Value, now: i64) -> Option<Value> {
        facts["cmd"] = json!("edit");
        facts["op"] = json!(op);
        let packet = ask(&mut self.model, facts).filter(|p| !p.is_null())?;
        self.apply_packet(&packet, now);
        Some(packet)
    }
    fn apply_packet(&mut self, packet: &Value, now: i64) {
        if packet["state"].is_object() {
            self.edit_state = packet["state"].clone();
        }
        let delta = &packet["ink"];
        match delta["kind"].as_str() {
            Some("replace") => self.ink = delta["value"].as_object().map(|_| delta["value"].clone()),
            Some("stroke") => {
                if let Some(ink) = self.ink.as_mut() {
                    ink["page"] = delta["page"].clone();
                    if let Some(strokes) = ink["strokes"].as_array_mut() {
                        strokes.push(delta["point"].clone());
                    }
                }
            }
            Some("point") => {
                if let Some(stroke) = self.ink.as_mut().and_then(|i| i["strokes"].as_array_mut()).and_then(|s| s.last_mut()).and_then(|s| s.as_array_mut()) {
                    stroke.extend(delta["point"].as_array().cloned().unwrap_or_default());
                }
            }
            _ => {}
        }
        if packet["clearSelection"] == true {
            self.selection = None;
        }
        if let Some(page) = packet["invalidate"]["page"].as_u64() {
            ask(&mut self.model, json!({"cmd":"cache","op":"invalidate","page":page}));
            ask(&mut self.thumbs, json!({"cmd":"cache","op":"invalidate","page":page}));
            if packet["invalidate"]["refresh"] == true {
                self.info.remove(&(page as usize));
            }
        }
        let notice = &packet["notice"];
        if let Some(key) = notice["key"].as_str() {
            let error = match notice["code"].as_str() {
                Some(code) => json!({"key":format!("docs.err.{code}")}),
                None => json!(notice["message"].as_str().unwrap_or("")),
            };
            self.set_message(json!({"key":key,"params":{"error":error}}), now);
        }
        if let Some(name) = packet["saved"].as_str() {
            self.set_message(json!({"key":"pdf.saved","params":{"name":name}}), now);
        }
        let effect = &packet["effect"];
        if let (Some(ticket), Some(kind), Some(doc)) = (effect["ticket"].as_u64(), effect["kind"].as_str(), self.doc) {
            let payload = &effect["payload"];
            let (method, args) = match kind {
                "annot" => ("pdf.annot", json!([doc, payload])),
                "field" => ("pdf.setField", json!([doc, payload["id"], payload["value"]])),
                "undo" => ("pdf.undo", json!([doc, effect["page"]])),
                "save" => ("pdf.save", json!([doc, payload["path"]])),
                _ => return,
            };
            self.jobs.enqueue(Tag::Edit { ticket, epoch: self.epoch }, method, args);
        }
    }
    fn finish_ink(&mut self, now: i64) {
        let color = TOOL_COLORS[self.color];
        self.edit("inkFinish", json!({"color":color}), now);
    }
    fn set_tool(&mut self, tool: &str, now: i64) {
        self.tool = tool.to_owned();
        if tool != "ink" && self.ink.is_some() {
            self.finish_ink(now);
        }
    }
    fn save_copy(&mut self, now: i64) {
        let (Some(path), Some(_)) = (self.path.clone(), self.doc) else { return };
        let file = std::path::Path::new(&path);
        let dir = file.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let base = if name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case(".pdf") { name[..name.len() - 4].to_owned() } else { name };
        let first = self.i18n.caption(&json!({"key":"pdf.copyName","params":{"name":base}}));
        let mut chosen = first.clone();
        for n in 2..200 {
            if !std::path::Path::new(&format!("{dir}/{chosen}.pdf")).exists() {
                break;
            }
            chosen = format!("{first} {n}");
        }
        self.edit("save", json!({"path":format!("{dir}/{chosen}.pdf"),"name":format!("{chosen}.pdf")}), now);
    }
    fn print(&mut self) {
        if let (Some(doc), Some(path)) = (self.doc, &self.path) {
            let title = basename(path);
            self.jobs.enqueue(Tag::Ignore, "pdf.print", json!([doc, title]));
        }
    }
    fn show_in_files(&mut self) {
        if let Some(path) = &self.path {
            let parent = path.rsplit_once('/').map(|(dir, _)| dir.to_owned()).unwrap_or_default();
            self.jobs.enqueue(Tag::Ignore, "windows.open", json!(["files", {"new":true,"args":{"path":parent}}]));
        }
    }
    fn copy_selection(&mut self, now: i64) {
        if let Some(text) = self.selection.as_ref().and_then(|s| s["text"].as_str()).filter(|t| !t.is_empty()).map(str::to_owned) {
            self.jobs.enqueue(Tag::Ignore, "clipboard.write", json!([text]));
            self.set_message(json!({"key":"pdf.copied"}), now);
        }
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
        let editing = self.goto.is_some() || self.edit_state["note"].is_object() || self.edit_state["field"].is_object();
        let action = ask(&mut self.model, json!({"cmd":"viewer","op":"key","editing":editing,"hasDoc":self.doc.is_some(),"event":event}))?;
        let action = action.as_str()?.to_owned();
        if !self.perform(&action, now) {
            return None;
        }
        Some(self.request_at(&json!({"cmd":"noop"}), now))
    }
    /// A viewer action, from a key or from a menu item.
    fn perform(&mut self, action: &str, now: i64) -> bool {
        let last = self.pages.saturating_sub(1);
        let height = self.view_size().1;
        match action {
            "rotateLeft" => self.rotate(-1),
            "rotateRight" => self.rotate(1),
            "zoomIn" => self.zoom_step(1),
            "zoomOut" => self.zoom_step(-1),
            "zoomActual" => self.set_zoom(Zoom::Scale(1.)),
            "zoomWidth" => self.set_zoom(Zoom::Width),
            "zoomPage" => self.set_zoom(Zoom::Page),
            "escape" => {
                self.message = None;
                self.clear_selection();
                ask(&mut self.model, json!({"cmd":"viewer","op":"findStart","query":""}));
                self.hits = None;
                self.find_due = None;
                self.set_tool("select", now);
            }
            "find" => self.find_focus += 1,
            "copy" => self.copy_selection(now),
            "selectAll" => {
                if let Some([w, h]) = self.filled_sizes().get(self.page).copied() {
                    self.select(self.page, [0., 0.], [w, h], 0, None);
                }
            }
            "goto" => self.goto = Some(String::new()),
            "nextHit" => self.next_hit(1),
            "previousHit" => self.next_hit(-1),
            "save" | "saveCopy" => self.save_copy(now),
            "print" => self.print(),
            "showInFiles" => self.show_in_files(),
            "undo" => {
                self.edit("undo", json!({}), now);
            }
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
            _ => return false,
        }
        true
    }
    /// What this window had when it was last mounted (the same window remounting, e.g. a theme change).
    fn restore(&mut self, saved: &Value, dark: bool) {
        if !saved.is_object() {
            return;
        }
        self.zoom = match &saved["zoom"] {
            Value::String(s) if s == "page" => Zoom::Page,
            Value::Number(n) => n.as_f64().filter(|z| z.is_finite() && *z > 0.).map(Zoom::Scale).unwrap_or(Zoom::Width),
            _ => Zoom::Width,
        };
        self.single = saved["mode"] == "single";
        match &saved["side"] {
            Value::Null if saved.get("side").is_some() => self.side = None,
            Value::String(s) if s == "thumbs" || s == "outline" => self.side = Some(s.clone()),
            _ => {}
        }
        self.invert = dark && saved["invert"] == true;
        self.remembered = self.window_state();
    }
    fn window_state(&self) -> Value {
        json!({"path":self.path,"zoom":self.zoom.json(),"mode":self.mode(),"side":self.side,"invert":self.invert})
    }
    fn remember(&mut self) {
        if self.window.is_empty() || !self.booted {
            return;
        }
        let state = self.window_state();
        if state != self.remembered {
            self.remembered = state.clone();
            self.jobs.enqueue(Tag::Ignore, "windows.state", json!([self.window, state]));
        }
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
            "down" => self.pointer_down(v, now),
            "pointerMove" => self.pointer_move(v, now),
            "dragEnd" => self.drag_end(now),
            "findChange" => {
                self.find_query = v["value"].as_str().unwrap_or("").to_owned();
                self.find_due = Some(now + FIND_DEBOUNCE_MS);
            }
            "findSubmit" => {
                self.find_due = None;
                if self.hits.as_ref().is_some_and(|h| h["list"].as_array().is_some_and(|l| !l.is_empty())) {
                    self.next_hit(1);
                } else {
                    self.run_find();
                }
            }
            "gotoOpen" => {
                if self.doc.is_some() {
                    self.goto = Some(String::new());
                }
            }
            "gotoChange" => self.goto = Some(v["value"].as_str().unwrap_or("").to_owned()),
            "gotoSubmit" => {
                let value = self.goto.take().and_then(|t| t.trim().parse::<f64>().ok());
                if let Some(page) = value.filter(|n| *n >= 1. && n.is_finite()) {
                    self.go_page(page as usize - 1, None);
                }
            }
            "gotoCancel" => self.goto = None,
            "do" => {
                if let Some(action) = key.as_str() {
                    self.perform(action, now);
                }
            }
            "sideToggle" => {
                let wanted = key.as_str().map(str::to_owned);
                self.side = if self.side == wanted { None } else { wanted };
            }
            "tool" => {
                let wanted = key.as_str().unwrap_or("select");
                let next = if wanted == "ink" && self.tool == "ink" { "select" } else { wanted };
                self.set_tool(next, now);
            }
            "color" => self.color = (key.as_u64().unwrap_or(0) as usize).min(TOOL_COLORS.len() - 1),
            "fieldChange" => {
                self.edit("fieldChange", json!({"value":v["value"]}), now);
            }
            "fieldCommit" => {
                let facts = if key.is_number() { json!({"value":key}) } else { json!({}) };
                self.edit("fieldCommit", facts, now);
            }
            "fieldCancel" => {
                self.edit("fieldCancel", json!({}), now);
            }
            "noteChange" => {
                self.edit("noteChange", json!({"value":v["value"]}), now);
            }
            "noteCommit" => {
                self.edit("noteCommit", json!({}), now);
            }
            "noteCancel" => {
                self.edit("noteCancel", json!({}), now);
            }
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
        self.timers(now);
        self.sync_menus();
        self.remember();
        let mut out = json!({});
        if let Some(on) = self.capture.take() {
            out["capture"] = json!(on);
        }
        if let Some(effect) = self.jobs.pump(now) {
            out["effect"] = effect;
        }
        if let Some(title) = self.title.take() {
            out["title"] = json!(title);
        }
        // The host only takes the scene from the reply to its own `sync`, which it sends after every event.
        if v["cmd"] == "sync" {
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
        let revision = v["context"]["localeRevision"].as_u64().unwrap_or(0);
        if revision != self.locale_revision {
            self.i18n.invalidate();
            self.locale_revision = revision;
            self.menus = Value::Null;
        }
        self.i18n.lang = v["lang"].as_str().unwrap_or("en").into();
        self.width = number(&v["context"]["width"], self.width).max(1.);
        self.height = number(&v["context"]["height"], self.height).max(1.);
        if !self.booted {
            if let Some(owner) = v["args"]["__nativeOwner"].as_i64().filter(|i| *i > 0) {
                self.booted = true;
                self.jobs.bind(owner);
                self.window = v["args"]["__nativeWindow"].as_str().unwrap_or("").to_owned();
                self.restore(&v["args"]["__nativeState"], v["context"]["theme"]["mode"] == "dark");
                self.home = v["context"]["home"].as_str().map(str::to_owned).or_else(|| std::env::var("HOME").ok()).unwrap_or_default();
                self.nonce = v["args"]["nonce"].clone();
                let saved = v["args"]["__nativeState"]["path"].as_str().filter(|p| !p.is_empty()).map(str::to_owned);
                if let Some(path) = saved.or_else(|| self.pick_path(&v["args"])) {
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
    fn menu_item(&mut self, key: &str, hint: &str, action: Value, disabled: bool, checked: Option<bool>) -> Value {
        let mut item = json!({"label":self.i18n.caption(&json!({"key":format!("pdf.{key}")})),"action":action});
        if !hint.is_empty() {
            item["hint"] = json!(hint);
        }
        if disabled {
            item["disabled"] = json!(true);
        }
        if let Some(on) = checked {
            item["checked"] = json!(on);
        }
        item
    }
    fn build_menus(&mut self) -> Value {
        let (doc, path) = (self.doc.is_some(), self.path.is_some());
        let undo = self.edit_state["history"].as_array().is_some_and(|h| !h.is_empty());
        let copy = self.selection.as_ref().and_then(|s| s["text"].as_str()).is_some_and(|t| !t.is_empty());
        let (single, side, zoom, invert) = (self.single, self.side.clone(), self.zoom.clone(), self.invert);
        let top = |app: &mut App, key: &str| app.i18n.caption(&json!({"key":format!("pdf.{key}")}));
        let file = vec![
            self.menu_item("menu.saveCopy", "❖S", json!(["do", "saveCopy"]), !doc, None),
            self.menu_item("menu.print", "❖P", json!(["do", "print"]), !doc, None),
            self.menu_item("menu.showInFiles", "", json!(["do", "showInFiles"]), !path, None),
        ];
        let edit = vec![
            self.menu_item("menu.undo", "❖Z", json!(["do", "undo"]), !undo, None),
            self.menu_item("menu.copy", "❖C", json!(["do", "copy"]), !copy, None),
            self.menu_item("menu.find", "❖F", json!(["do", "find"]), false, None),
        ];
        let view = vec![
            self.menu_item("view.continuous", "", json!(["mode", "continuous"]), false, Some(!single)),
            self.menu_item("view.single", "", json!(["mode", "single"]), false, Some(single)),
            json!("-"),
            self.menu_item("side.thumbs", "", json!(["sideToggle", "thumbs"]), false, Some(side.as_deref() == Some("thumbs"))),
            self.menu_item("side.outline", "", json!(["sideToggle", "outline"]), false, Some(side.as_deref() == Some("outline"))),
            json!("-"),
            self.menu_item("zoom.in", "❖+", json!(["do", "zoomIn"]), false, None),
            self.menu_item("zoom.out", "❖−", json!(["do", "zoomOut"]), false, None),
            self.menu_item("zoom.actual", "❖0", json!(["do", "zoomActual"]), false, None),
            self.menu_item("zoom.width", "❖9", json!(["do", "zoomWidth"]), false, Some(zoom == Zoom::Width)),
            self.menu_item("zoom.page", "", json!(["do", "zoomPage"]), false, Some(zoom == Zoom::Page)),
            json!("-"),
            self.menu_item("rotateRight", "❖R", json!(["do", "rotateRight"]), false, None),
            self.menu_item("rotateLeft", "❖⇧R", json!(["do", "rotateLeft"]), false, None),
            self.menu_item("invert", "", json!(["invert"]), false, Some(invert)),
        ];
        let go = vec![
            self.menu_item("go.page", "❖⌥G", json!(["do", "goto"]), false, None),
            self.menu_item("go.first", "", json!(["do", "firstPage"]), false, None),
            self.menu_item("go.last", "", json!(["do", "lastPage"]), false, None),
        ];
        json!([
            {"label":top(self, "menu.file"),"items":file},
            {"label":top(self, "menu.edit"),"items":edit},
            {"label":top(self, "menu.view"),"items":view},
            {"label":top(self, "menu.go"),"items":go},
        ])
    }
    fn sync_menus(&mut self) {
        if self.window.is_empty() || !self.booted {
            return;
        }
        let menus = self.build_menus();
        if menus != self.menus {
            self.menus = menus.clone();
            self.jobs.enqueue(Tag::Ignore, "windows.menus", json!([self.window, menus]));
        }
    }
    fn editors(&mut self, sizes: &[[f64; 2]], view_w: f64) -> Vec<Value> {
        let mut nodes = vec![];
        let dim = self.theme["textDim"].as_str().unwrap_or("#999999").to_owned();
        let field = self.edit_state["field"].clone();
        if let (Some(page), true) = (field["page"].as_u64(), field.is_object()) {
            if let (Some(v), Some((left, top))) = (self.rect_view(&field["r"], page as usize, sizes), self.frame_of(page as usize)) {
                let (l, t) = (left + number(&v["left"], 0.), top + number(&v["top"], 0.));
                let (w, h) = (number(&v["width"], 0.), number(&v["height"], 0.));
                if field["f"]["type"] == "text" {
                    nodes.push(json!({"kind":"input","key":"field-editor","value":field["value"],"autoFocus":true,
                        "change":["fieldChange"],"submit":["fieldCommit"],"cancel":["fieldCancel"],
                        "size":(h * 0.6).clamp(10., 18.),
                        "style":{"position":"absolute","left":l,"top":t,"width":w.max(60.),"height":h.max(22.)}}));
                } else {
                    let selected = field["f"]["value"].as_i64().unwrap_or(-1);
                    let rows: Vec<Value> = field["f"]["items"].as_array().into_iter().flatten().take(20).enumerate().map(|(i, item)| {
                        let chosen = selected == i as i64;
                        ui::click(
                            ui::view(
                                json!(format!("choice-{i}")),
                                json!({"height":24,"paddingHorizontal":8,"justifyContent":"center",
                                    "backgroundColor":if chosen {self.theme["accent"].as_str().unwrap_or("#0a84ff")} else {"transparent"}}),
                                vec![ui::text(item.clone(), json!({"fontSize":12,"color":if chosen {"#ffffff"} else {self.theme["text"].as_str().unwrap_or("#ffffff")}}))],
                            ),
                            json!(["fieldCommit", i]),
                        )
                    }).collect();
                    nodes.push(ui::view(
                        "field-choices",
                        json!({"position":"absolute","left":l,"top":t + h,"width":w.max(140.),"maxHeight":200,"borderRadius":6,"overflow":"hidden",
                            "backgroundColor":self.theme["windowBg"].as_str().unwrap_or("#2a2a2e"),"borderWidth":1,
                            "borderColor":self.theme["separator"].as_str().unwrap_or("#444444"),"boxShadow":"0 8px 22px rgba(0,0,0,0.35)"}),
                        rows,
                    ));
                }
            }
        }
        let note = self.edit_state["note"].clone();
        if let (Some(page), true) = (note["page"].as_u64(), note.is_object()) {
            let point = json!([note["x"], note["y"], 1, 1]);
            if let (Some(v), Some((left, top))) = (self.rect_view(&point, page as usize, sizes), self.frame_of(page as usize)) {
                nodes.push(ui::view(
                    "note-editor",
                    json!({"position":"absolute","left":(view_w - 250.).min(left + number(&v["left"], 0.)).max(0.),"top":top + number(&v["top"], 0.),
                        "width":240,"padding":8,"gap":6,"borderRadius":8,"backgroundColor":"#fff6b0","boxShadow":"0 8px 22px rgba(0,0,0,0.35)"}),
                    vec![
                        json!({"kind":"input","key":"note-input","value":note["text"],"autoFocus":true,"placeholder":{"key":"pdf.notePlaceholder"},
                            "change":["noteChange"],"submit":["noteCommit"],"cancel":["noteCancel"],"size":13,"style":{"height":26}}),
                        ui::text(json!({"key":"pdf.noteHint"}), json!({"fontSize":10,"color":"#6b5d00"})),
                    ],
                ));
            }
        }
        let _ = dim;
        nodes
    }
    fn rect_view(&mut self, rect: &Value, page: usize, sizes: &[[f64; 2]]) -> Option<Value> {
        let size = sizes.get(page).copied()?;
        ask(&mut self.model, json!({"cmd":"rect","rect":rect,"size":size,"rot":self.rot,"scale":self.scale}))
    }
    /// Search hits, the text selection and form field outlines of one visible page.
    fn overlays(&mut self, page: usize, sizes: &[[f64; 2]]) -> Vec<Value> {
        let mut nodes = vec![];
        let node = |key: String, r: &Value, style: Value| {
            let mut base = json!({"position":"absolute","left":r["left"],"top":r["top"],"width":r["width"],"height":r["height"],"pointerEvents":"none"});
            for (k, v) in style.as_object().into_iter().flatten() {
                base[k] = v.clone();
            }
            ui::view(json!(key), base, vec![])
        };
        let current = self.hits.as_ref().and_then(|h| h["cur"].as_u64()).unwrap_or(0);
        let hits: Vec<(usize, Value)> = self
            .hits
            .as_ref()
            .and_then(|h| h["list"].as_array())
            .map(|list| list.iter().enumerate().filter(|(_, h)| h["page"].as_u64() == Some(page as u64)).map(|(i, h)| (i, h["rect"].clone())).collect())
            .unwrap_or_default();
        for (i, rect) in hits {
            if let Some(r) = self.rect_view(&rect, page, sizes) {
                let color = if i as u64 == current { "rgba(255,149,0,0.55)" } else { "rgba(255,214,10,0.42)" };
                nodes.push(node(format!("hit-{i}"), &r, json!({"borderRadius":2,"backgroundColor":color})));
            }
        }
        let ink: Vec<(usize, f64, f64)> = self
            .ink
            .as_ref()
            .filter(|i| i["page"].as_u64() == Some(page as u64))
            .and_then(|i| i["strokes"].as_array())
            .map(|strokes| {
                strokes
                    .iter()
                    .enumerate()
                    .flat_map(|(si, s)| s.as_array().into_iter().flat_map(move |s| s.chunks(2).map(move |c| (si, c))))
                    .filter_map(|(si, c)| Some((si, c.first()?.as_f64()?, c.get(1)?.as_f64()?)))
                    .collect()
            })
            .unwrap_or_default();
        let skip = ink.len().saturating_sub(1500);
        for (n, (si, x, y)) in ink.into_iter().enumerate().skip(skip) {
            if let Some(r) = self.rect_view(&json!([x - 1.1, y - 1.1, 2.2, 2.2]), page, sizes) {
                nodes.push(node(format!("ink-{si}-{n}"), &r, json!({"borderRadius":r["width"],"backgroundColor":TOOL_COLORS[self.color]})));
            }
        }
        let selection_color = if self.tool == "highlight" { format!("{}88", TOOL_COLORS[self.color]) } else { "rgba(10,132,255,0.32)".to_owned() };
        let rects: Vec<Value> = self
            .selection
            .as_ref()
            .filter(|s| s["page"].as_u64() == Some(page as u64))
            .and_then(|s| s["rects"].as_array().cloned())
            .unwrap_or_default();
        for (i, rect) in rects.iter().enumerate() {
            if let Some(r) = self.rect_view(rect, page, sizes) {
                nodes.push(node(format!("selection-{i}"), &r, json!({"backgroundColor":selection_color})));
            }
        }
        let fields: Vec<Value> = self.info.get(&page).and_then(|i| i.as_ref()).and_then(|i| i["fields"].as_array().cloned()).unwrap_or_default();
        for (i, field) in fields.iter().enumerate() {
            if matches!(field["f"]["type"].as_str(), Some("other" | "push")) {
                continue;
            }
            let readonly = field["f"]["ro"] == true;
            if let Some(r) = self.rect_view(&field["r"], page, sizes) {
                nodes.push(node(
                    format!("field-{i}"),
                    &r,
                    json!({"backgroundColor":if readonly {"transparent"} else {"rgba(10,132,255,0.10)"},
                        "borderWidth":if readonly {0} else {1},"borderColor":"rgba(10,132,255,0.45)"}),
                ));
            }
        }
        nodes
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
        let sizes = self.filled_sizes();
        let mut overlays: HashMap<u64, Vec<Value>> = HashMap::new();
        for frame in &frames {
            let Some(page) = frame["page"].as_u64() else { continue };
            overlays.insert(page, self.overlays(page as usize, &sizes));
        }
        let hit_count = self.hits.as_ref().and_then(|h| h["list"].as_array().map(Vec::len));
        let searching = self.hits.as_ref().is_some_and(|h| h["done"] != true);
        let selected = self.selection.as_ref().and_then(|s| s["text"].as_str()).map(|t| t.chars().count()).filter(|n| *n > 0);
        let status = if let Some((message, _)) = &self.message {
            message.clone()
        } else if let (Some(hits), Some(count)) = (&self.hits, hit_count) {
            if count > 0 {
                json!({"key":"pdf.matches","params":{"cur":hits["cur"].as_u64().unwrap_or(0) + 1,"count":count}})
            } else if searching {
                json!({"key":"pdf.searchingText"})
            } else {
                json!({"key":"pdf.noMatches"})
            }
        } else if let Some(count) = selected {
            json!({"key":"pdf.selected","params":{"count":count}})
        } else {
            json!(self.doc_title)
        };
        let status = if status.as_str() == Some(self.doc_title.as_str()) {
            match self.tool.as_str() {
                "ink" => json!({"key":"pdf.inkHint"}),
                "note" => json!({"key":"pdf.noteTool"}),
                "highlight" => json!({"key":"pdf.highlightHint"}),
                _ => status,
            }
        } else {
            status
        };
        let editors = self.editors(&sizes, view_w);
        let status_more = hit_count.is_some_and(|c| c > 0) && searching;
        let goto = self.goto.clone();
        let find_query = self.find_query.clone();
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
            overlays: &overlays,
            goto: goto.as_deref(),
            find_query: &find_query,
            find_focus: self.find_focus,
            status_more,
            tool: &self.tool,
            color: self.color,
            colors: &TOOL_COLORS,
            dirty: self.edit_state["dirty"] == true,
            editors: &editors,
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
