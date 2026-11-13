//! Native font manager. FreeType stays in the confined document helper. Rust
//! owns selection, layout, generations, character grids and file-job sequencing.
mod blocks;
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    i18n::I18n,
    NativeApp,
};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Debug)]
struct Family {
    name: String,
    styles: Vec<Value>,
    mono: bool,
    color: bool,
    user: bool,
    file: bool,
}
fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn is_user(path: &str, home: &str) -> bool {
    !home.is_empty()
        && (path.starts_with(&format!("{home}/.local/share/fonts/"))
            || path.starts_with(&format!("{home}/.fonts/")))
}
fn valid_font(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        ["ttf", "otf", "ttc", "otc"].contains(&ext.to_ascii_lowercase().as_str())
    })
}
fn group_fonts(list: &Value, home: &str) -> Vec<Family> {
    let mut groups = Vec::<Family>::new();
    let mut index = HashMap::<String, usize>::new();
    for font in list.as_array().into_iter().flatten() {
        let path = string(font, "path");
        if path.is_empty() {
            continue;
        }
        let name = string(font, "family").to_owned();
        let i = *index.entry(name.clone()).or_insert_with(|| {
            let i = groups.len();
            groups.push(Family {
                name,
                styles: vec![],
                mono: false,
                color: false,
                user: false,
                file: false,
            });
            i
        });
        let g = &mut groups[i];
        if !g
            .styles
            .iter()
            .any(|s| s["path"] == font["path"] && s["index"] == font["index"])
        {
            g.styles.push(font.clone());
        }
        g.mono |= font["mono"] == true;
        g.color |= font["color"] == true;
        g.user |= is_user(path, home);
    }
    let mut out = groups;
    for g in &mut out {
        g.styles.sort_by(|a, b| {
            (a["italic"] == true)
                .cmp(&(b["italic"] == true))
                .then_with(|| {
                    a["weight"]
                        .as_f64()
                        .unwrap_or(0.)
                        .total_cmp(&b["weight"].as_f64().unwrap_or(0.))
                })
        });
    }
    // QuickJS localeCompare normalizes NFC and compares Unicode scalars.
    // Keep source insertion order for canonically equivalent family names.
    out.sort_by_cached_key(|f| crate::launcher::normalize_nfc(&f.name));
    out
}
#[derive(Clone, Debug)]
enum After {
    Initial,
    Installed(String),
    Removed { name: String, error: Option<String> },
}
#[derive(Debug)]
enum Tag {
    List(After),
    Open(u64),
    Info(u64),
    Stat(u64),
    Sample(u64, usize),
    Grid(u64),
    Release,
    Mkdir(usize),
    CheckDir(usize),
    Destination,
    Copy,
    CopyPoll,
    Forget,
    Trash,
}
#[derive(Clone, Debug)]
struct Image {
    id: i64,
    w: f64,
    h: f64,
    present: Vec<bool>,
}
impl Image {
    fn read(v: &Value) -> Option<Self> {
        let id = v["image"]
            .as_i64()
            .filter(|i| *i > 0 && *i <= i32::MAX as i64)?;
        let (w, h) = (v["w"].as_f64()?, v["h"].as_f64()?);
        if ![w, h]
            .iter()
            .all(|n| n.is_finite() && *n > 0. && *n <= 32768.)
        {
            return None;
        }
        Some(Self {
            id,
            w,
            h,
            present: v["data"]["present"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| v == &Value::Bool(true) || v.as_i64().is_some_and(|n| n != 0))
                .collect(),
        })
    }
}
struct Installation {
    source: String,
    dest: String,
    name: String,
    job: Option<u64>,
    poll_at: i64,
}
struct Removal {
    name: String,
    pending: usize,
    error: Option<String>,
}
pub struct App {
    jobs: Jobs<Tag>,
    booted: bool,
    families: Vec<Family>,
    file_font: Option<Family>,
    file_path: String,
    last_args: Value,
    collection: String,
    query: String,
    selection: Option<String>,
    style: usize,
    tab: String,
    text: String,
    size: f64,
    family_offset: f64,
    font: Option<Value>,
    font_epoch: u64,
    file_epoch: u64,
    render_epoch: u64,
    grid_epoch: u64,
    info: Option<Value>,
    bytes: Option<u64>,
    samples: [Option<Image>; 2],
    sample_error: [bool; 2],
    sample_at: Option<i64>,
    grid: Option<Image>,
    grid_dirty: bool,
    ranges: Vec<[u32; 2]>,
    block: usize,
    chunk: usize,
    pick: Option<u32>,
    install: Option<Installation>,
    removal: Option<Removal>,
    notice: Value,
    notice_until: i64,
    i18n: I18n,
    home: String,
    theme: Value,
    width: f64,
    height: f64,
    drag_size: bool,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            booted: false,
            families: vec![],
            file_font: None,
            file_path: String::new(),
            last_args: Value::Null,
            collection: "all".into(),
            query: String::new(),
            selection: None,
            style: 0,
            tab: "sample".into(),
            text: String::new(),
            size: 36.,
            family_offset: 0.,
            font: None,
            font_epoch: 0,
            file_epoch: 0,
            render_epoch: 0,
            grid_epoch: 0,
            info: None,
            bytes: None,
            samples: [None, None],
            sample_error: [false, false],
            sample_at: None,
            grid: None,
            grid_dirty: false,
            ranges: vec![],
            block: 0,
            chunk: 0,
            pick: None,
            install: None,
            removal: None,
            notice: Value::Null,
            notice_until: 0,
            i18n: I18n::default(),
            home: String::new(),
            theme: Value::Null,
            width: 960.,
            height: 620.,
            drag_size: false,
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
        if self.jobs.busy() || self.sample_at.is_some() || self.grid_dirty || self.install.is_some()
        {
            Some(50.)
        } else if !self.notice.is_null() {
            Some(1000.)
        } else {
            None
        }
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
}
impl App {
    fn visible(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        self.families
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                ((match self.collection.as_str() {
                    "user" => f.user,
                    "mono" => f.mono,
                    "color" => f.color,
                    _ => true,
                }) && (q.is_empty() || f.name.to_lowercase().contains(&q)))
                .then_some(i)
            })
            .collect()
    }
    fn family(&self) -> Option<&Family> {
        if self.file_font.is_some()
            && (self.selection.as_deref() == Some("file")
                || self.selection.is_none() && !self.file_path.is_empty())
        {
            return self.file_font.as_ref();
        }
        let visible = self.visible();
        if let Some(name) = &self.selection {
            visible
                .into_iter()
                .map(|i| &self.families[i])
                .find(|f| &f.name == name)
        } else {
            visible.first().map(|i| &self.families[*i])
        }
    }
    fn already_installed(&self) -> bool {
        self.file_font.as_ref().is_some_and(|file| {
            self.families.iter().any(|f| {
                f.name == file.name
                    && f.styles
                        .iter()
                        .any(|s| s["style"] == file.styles[0]["style"])
            })
        })
    }
    fn family_height(&self) -> f64 {
        (self.height - 220.).max(0.)
    }
    fn family_max(&self) -> f64 {
        ((self.visible().len() + usize::from(self.file_font.is_some())) as f64 * 26.
            - self.family_height())
        .max(0.)
    }
    fn blocks(&self) -> Vec<usize> {
        blocks::BLOCKS
            .iter()
            .enumerate()
            .filter_map(|(i, (_, a, b))| {
                self.ranges
                    .iter()
                    .any(|[x, y]| x <= b && y >= a)
                    .then_some(i)
            })
            .collect()
    }
    fn current_block(&self) -> Option<(&'static str, u32, u32)> {
        self.blocks().get(self.block).map(|i| blocks::BLOCKS[*i])
    }
    fn codepoints(&self) -> Vec<u32> {
        let Some((_, a, b)) = self.current_block() else {
            return vec![];
        };
        let start = a.saturating_add((self.chunk as u32).saturating_mul(256));
        let mut out = vec![];
        for c in start..=b {
            if self.ranges.iter().any(|[x, y]| c >= *x && c <= *y) {
                out.push(c);
                if out.len() == 256 {
                    break;
                }
            }
        }
        out
    }
    fn block_width(&self) -> f64 {
        (self.width - 230. - 48. - 16. - 256.).clamp(80., 200.)
    }
    fn cell(&self) -> f64 {
        ((self.width - 230. - 48. - 16. - self.block_width()) / 16.)
            .floor()
            .clamp(16., 40.)
    }
    fn sample_width(&self) -> f64 {
        (self.width - 230. - 48. - 36.).round().clamp(1., 8192.)
    }
    fn fg(&self) -> &'static str {
        if self.theme["mode"] == "dark" {
            "#f2f2f7"
        } else {
            "#1c1c1e"
        }
    }
    fn release_ids(&mut self, ids: Vec<i64>) {
        if !ids.is_empty() {
            self.jobs
                .enqueue(Tag::Release, "fonts.release", json!([ids]));
        }
    }
    fn release_samples(&mut self) {
        let ids = self
            .samples
            .iter_mut()
            .filter_map(|s| s.take().map(|i| i.id))
            .collect();
        self.release_ids(ids);
        self.sample_error = [false, false];
    }
    fn release_grid(&mut self) {
        if let Some(g) = self.grid.take() {
            self.release_ids(vec![g.id]);
        }
    }
    fn sample_changed(&mut self, now: i64) {
        self.render_epoch += 1;
        self.jobs.cancel(|t| matches!(t, Tag::Sample(..)));
        self.release_samples();
        self.sample_at = (self.tab == "sample" && self.font.is_some()).then_some(now + 60);
    }
    fn grid_changed(&mut self) {
        self.grid_epoch += 1;
        self.jobs.cancel(|t| matches!(t, Tag::Grid(..)));
        self.release_grid();
        self.pick = None;
        self.grid_dirty = self.tab == "chars" && self.info.is_some();
    }
    fn select_font(&mut self, now: i64) {
        let name = self.family().map(|f| f.name.clone());
        if self.font.as_ref().and_then(|f| f["family"].as_str()) != name.as_deref() {
            self.style = 0;
        }
        let font = self
            .family()
            .and_then(|f| {
                f.styles
                    .get(self.style.min(f.styles.len().saturating_sub(1)))
            })
            .cloned()
            .map(|mut font| {
                if let Some(name) = &name {
                    font["family"] = json!(name);
                }
                font
            });
        if self.font.as_ref().map(|f| (&f["path"], &f["index"]))
            == font.as_ref().map(|f| (&f["path"], &f["index"]))
        {
            self.font = font;
            return;
        }
        self.font = font;
        self.font_epoch += 1;
        self.info = None;
        self.bytes = None;
        self.ranges.clear();
        self.block = 0;
        self.chunk = 0;
        self.jobs
            .cancel(|t| matches!(t, Tag::Info(..) | Tag::Stat(..)));
        self.sample_changed(now);
        self.grid_changed();
        if let Some(font) = &self.font {
            self.jobs.enqueue(
                Tag::Info(self.font_epoch),
                "fonts.info",
                json!([font["path"], font["index"].as_u64().unwrap_or(0)]),
            );
            self.jobs
                .enqueue(Tag::Stat(self.font_epoch), "fs.stat", json!([font["path"]]));
        }
    }
    fn render_samples(&mut self) {
        self.sample_at = None;
        if self.tab != "sample" || self.sample_width() < 40. {
            return;
        }
        let Some(font) = &self.font else {
            return;
        };
        let text = if self.text.is_empty() {
            self.i18n.caption(&json!({"key":"fonts.pangram"}))
        } else {
            self.text.clone()
        };
        let alphabet = self.i18n.caption(&json!({"key":"fonts.alphabet"}));
        for (slot, lines) in [
            (0, json!([{"text":text,"size":self.size}])),
            (
                1,
                json!([12, 18, 24, 36]
                    .iter()
                    .map(|size| json!({"text":alphabet,"size":size}))
                    .collect::<Vec<_>>()),
            ),
        ] {
            self.jobs.enqueue(Tag::Sample(self.render_epoch,slot),"fonts.sample",json!([font["path"],{"lines":lines,"width":self.sample_width(),"fg":self.fg(),"index":font["index"].as_u64().unwrap_or(0)}]));
        }
    }
    fn render_grid(&mut self) {
        self.grid_dirty = false;
        if self.tab != "chars" {
            return;
        }
        let cps = self.codepoints();
        let Some(font) = &self.font else {
            return;
        };
        if cps.is_empty() {
            return;
        }
        self.jobs.enqueue(Tag::Grid(self.grid_epoch),"fonts.grid",json!([font["path"],{"cps":cps,"cell":self.cell(),"cols":16,"fg":self.fg(),"index":font["index"].as_u64().unwrap_or(0)}]));
    }
    fn message(&mut self, value: Value, now: i64) {
        self.notice = value;
        self.notice_until = now + 4000;
    }
    fn failure(&mut self, error: String, now: i64) {
        self.message(json!({"key":"fonts.error","params":{"error":error}}), now);
    }
    fn open(&mut self, path: &str) {
        self.file_epoch += 1;
        self.file_path = path.into();
        self.file_font = None;
        self.jobs.cancel(|t| matches!(t, Tag::Open(..)));
        self.jobs
            .enqueue(Tag::Open(self.file_epoch), "fonts.info", json!([path, 0]));
    }
    fn install(&mut self) {
        if self.install.is_some() || self.removal.is_some() || self.already_installed() {
            return;
        }
        let Some(family) = self.family().filter(|f| f.file) else {
            return;
        };
        let source = self.file_path.clone();
        let name = family.name.clone();
        let file = std::path::Path::new(&source)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if file.is_empty() || self.home.is_empty() {
            return;
        }
        self.install = Some(Installation {
            source: source.clone(),
            dest: format!("{}/.local/share/fonts/{file}", self.home),
            name,
            job: None,
            poll_at: 0,
        });
        self.prepare_dir(0);
    }
    fn dir(&self, index: usize) -> String {
        format!(
            "{}/{}",
            self.home,
            [".local", ".local/share", ".local/share/fonts"][index]
        )
    }
    fn prepare_dir(&mut self, index: usize) {
        self.jobs
            .enqueue(Tag::Mkdir(index), "fs.mkdir", json!([self.dir(index)]));
    }
    fn next_dir(&mut self, index: usize) {
        if index < 2 {
            self.prepare_dir(index + 1);
        } else if let Some(install) = &self.install {
            self.jobs
                .enqueue(Tag::Destination, "fs.stat", json!([install.dest]));
        }
    }
    fn remove(&mut self) {
        if self.install.is_some() || self.removal.is_some() {
            return;
        }
        let Some(family) = self.family().filter(|f| !f.file && f.user) else {
            return;
        };
        let name = family.name.clone();
        let paths: BTreeSet<_> = family
            .styles
            .iter()
            .map(|f| string(f, "path").to_owned())
            .filter(|p| is_user(p, &self.home))
            .collect();
        if paths.is_empty() {
            return;
        }
        self.removal = Some(Removal {
            name,
            pending: paths.len(),
            error: None,
        });
        for path in paths {
            self.jobs.enqueue(Tag::Trash, "fs.trash", json!([path]));
        }
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        let Completed { tag, result } = done;
        match tag {
            Tag::List(after) => {
                if let Ok(value) = result {
                    self.families = group_fonts(&value, &self.home);
                } else {
                    self.families.clear();
                }
                match after {
                    After::Initial => {}
                    After::Installed(name) => {
                        self.file_font = None;
                        self.file_path.clear();
                        self.selection = Some(name.clone());
                        self.install = None;
                        self.message(json!({"key":"fonts.installed","params":{"name":name}}), now);
                    }
                    After::Removed { name, error } => {
                        self.selection = None;
                        self.removal = None;
                        if let Some(error) = error {
                            self.failure(error, now);
                        } else {
                            self.message(
                                json!({"key":"fonts.removed","params":{"name":name}}),
                                now,
                            );
                        }
                    }
                }
                self.select_font(now);
            }
            Tag::Open(epoch) => {
                if epoch == self.file_epoch {
                    if let Ok(info) = result {
                        let name = string(&info, "family").to_owned();
                        self.file_font = Some(Family {
                            name: name.clone(),
                            styles: vec![
                                json!({"family":name,"style":info["style"],"path":self.file_path,"index":0,"mono":info["mono"]}),
                            ],
                            mono: info["mono"] == true,
                            color: info["color"] == true,
                            user: false,
                            file: true,
                        });
                        self.selection = Some("file".into());
                        self.style = 0;
                        self.tab = "sample".into();
                        self.select_font(now);
                    } else {
                        self.file_path.clear();
                        self.file_font = None;
                        self.message(json!({"key":"fonts.invalid"}), now);
                        self.select_font(now);
                    }
                }
            }
            Tag::Info(epoch) => {
                if epoch == self.font_epoch {
                    self.info = Some(result.unwrap_or_else(|_| json!({"error":true})));
                    self.ranges = self.info.as_ref().unwrap()["ranges"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|r| {
                            let a = r[0].as_u64()?;
                            let b = r[1].as_u64()?;
                            (a <= b && b <= 0x10ffff).then_some([a as u32, b as u32])
                        })
                        .collect();
                    self.block = 0;
                    self.chunk = 0;
                    self.grid_changed();
                }
            }
            Tag::Stat(epoch) => {
                if epoch == self.font_epoch {
                    self.bytes = result.ok().and_then(|v| v["size"].as_u64());
                }
            }
            Tag::Sample(epoch, slot) => {
                if epoch == self.render_epoch && self.tab == "sample" {
                    match result {
                        Ok(v) => {
                            self.samples[slot] = Image::read(&v);
                            if self.samples[slot].is_none() {
                                if let Some(id) = v["image"].as_i64() {
                                    self.release_ids(vec![id]);
                                }
                                self.sample_error[slot] = true;
                            }
                        }
                        Err(_) => self.sample_error[slot] = true,
                    }
                } else if let Ok(v) = result {
                    if let Some(id) = v["image"].as_i64() {
                        self.release_ids(vec![id]);
                    }
                }
            }
            Tag::Grid(epoch) => {
                if epoch == self.grid_epoch && self.tab == "chars" {
                    if let Ok(v) = result {
                        self.grid = Image::read(&v);
                        if self.grid.is_none() {
                            if let Some(id) = v["image"].as_i64() {
                                self.release_ids(vec![id]);
                            }
                        }
                    }
                } else if let Ok(v) = result {
                    if let Some(id) = v["image"].as_i64() {
                        self.release_ids(vec![id]);
                    }
                }
            }
            Tag::Release | Tag::Forget => {}
            Tag::Mkdir(index) => {
                if result.is_ok() {
                    self.next_dir(index);
                } else {
                    self.jobs
                        .enqueue(Tag::CheckDir(index), "fs.stat", json!([self.dir(index)]));
                }
            }
            Tag::CheckDir(index) => {
                if result.as_ref().is_ok_and(|v| v["dir"] == true) {
                    self.next_dir(index);
                } else {
                    self.install = None;
                    self.failure(
                        result.err().unwrap_or_else(|| "Not a directory".into()),
                        now,
                    );
                }
            }
            Tag::Destination => {
                if result.is_ok() {
                    self.install = None;
                    self.message(json!({"key":"fonts.already"}), now);
                } else if let Some(i) = &self.install {
                    self.jobs
                        .enqueue(Tag::Copy, "fs.copy", json!([i.source, i.dest]));
                }
            }
            Tag::Copy => match result {
                Ok(v) => {
                    if let Some(id) = v.as_u64() {
                        if let Some(i) = &mut self.install {
                            i.job = Some(id);
                            i.poll_at = now + 200;
                        }
                    } else {
                        self.install = None;
                        self.failure("Could not copy font".into(), now);
                    }
                }
                Err(error) => {
                    self.install = None;
                    self.failure(error, now);
                }
            },
            Tag::CopyPoll => {
                if let Some(i) = &self.install {
                    let id = i.job.unwrap_or(0);
                    match result {
                        Ok(v) => {
                            if let Some(job) = v
                                .as_array()
                                .and_then(|jobs| jobs.iter().find(|j| j["id"].as_u64() == Some(id)))
                            {
                                if job["done"] == true {
                                    self.jobs.enqueue(Tag::Forget, "fs.forgetJob", json!([id]));
                                    if job["failed"] == true
                                        || job["failed"].as_u64().unwrap_or(0) > 0
                                        || !string(job, "error").is_empty()
                                    {
                                        let error = string(job, "error").to_owned();
                                        self.install = None;
                                        self.failure(error, now);
                                    } else {
                                        self.jobs.enqueue(
                                            Tag::List(After::Installed(i.name.clone())),
                                            "fonts.list",
                                            json!([true]),
                                        );
                                        if let Some(i) = &mut self.install {
                                            i.job = None;
                                        }
                                    }
                                } else if let Some(i) = &mut self.install {
                                    i.poll_at = now + 200;
                                }
                            } else {
                                self.install = None;
                                self.failure("Could not find font copy job".into(), now);
                            }
                        }
                        Err(error) => {
                            self.install = None;
                            self.failure(error, now);
                        }
                    }
                }
            }
            Tag::Trash => {
                if let Some(r) = &mut self.removal {
                    r.pending = r.pending.saturating_sub(1);
                    if let Err(error) = result {
                        r.error = Some(error);
                    }
                    if r.pending == 0 {
                        self.jobs.enqueue(
                            Tag::List(After::Removed {
                                name: r.name.clone(),
                                error: r.error.clone(),
                            }),
                            "fonts.list",
                            json!([true]),
                        );
                    }
                }
            }
        }
    }
    fn size_from_x(&mut self, x: f64, now: i64) {
        if x.is_finite() {
            let size = (8. + ((x - (self.width - 230.)) / 120.).clamp(0., 1.) * 112.).round();
            if self.size != size {
                self.size = size;
                self.sample_changed(now);
            }
        }
    }
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        let cmd = input["cmd"].as_str().unwrap_or("");
        let key = input["key"].as_str().unwrap_or("");
        match cmd {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                let old_width = self.sample_width();
                let old_cell = self.cell();
                let mode = self.theme["mode"].clone();
                let lang = self.i18n.lang.clone();
                self.theme = input["context"]["theme"].clone();
                self.width = input["context"]["width"]
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .unwrap_or(960.)
                    .max(0.);
                self.height = input["context"]["height"]
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .unwrap_or(620.)
                    .max(0.);
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                if !self.booted {
                    if let Some(owner) = input["args"]["__nativeOwner"].as_i64().filter(|n| *n > 0)
                    {
                        self.jobs.bind(owner);
                        self.booted = true;
                        self.jobs
                            .enqueue(Tag::List(After::Initial), "fonts.list", json!([false]));
                    }
                }
                let args = json!({"path":input["args"]["path"],"nonce":input["args"]["nonce"]});
                if args != self.last_args {
                    self.last_args = args;
                    if let Some(path) = input["args"]["path"].as_str().filter(|p| valid_font(p)) {
                        self.open(path);
                    }
                }
                if old_width != self.sample_width()
                    || mode != self.theme["mode"]
                    || lang != self.i18n.lang
                {
                    self.sample_changed(now);
                }
                if old_cell != self.cell() || mode != self.theme["mode"] {
                    self.grid_changed();
                }
                if self.sample_at.is_some_and(|at| now >= at) {
                    self.render_samples();
                }
                if self.grid_dirty {
                    self.render_grid();
                }
                if let Some(i) = &mut self.install {
                    if i.job.is_some() && i.poll_at > 0 && now >= i.poll_at {
                        i.poll_at = 0;
                        self.jobs.enqueue(Tag::CopyPoll, "fs.jobs", json!([]));
                    }
                }
                if !self.notice.is_null() && now >= self.notice_until {
                    self.notice = Value::Null;
                }
                self.family_offset = self.family_offset.clamp(0., self.family_max());
                let title = self
                    .family()
                    .map(|f| json!(f.name))
                    .unwrap_or_else(|| json!({"key":"apps.fonts"}));
                let tree = ui::tree(self);
                let mut reply = if tree == self.last_tree {
                    json!({"same":true,"title":title})
                } else {
                    self.last_tree = tree.clone();
                    json!({"tree":tree,"title":title})
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
            "search" => {
                self.query = input["value"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(4096)
                    .collect();
                self.select_font(now);
            }
            "collection" => {
                if ["all", "user", "mono", "color"].contains(&key) {
                    self.collection = key.into();
                    self.selection = None;
                    self.style = 0;
                    self.family_offset = 0.;
                    self.select_font(now);
                }
            }
            "family" => {
                if self.families.iter().any(|f| f.name == key)
                    || key == "file" && self.file_font.is_some()
                {
                    self.selection = Some(key.into());
                    self.style = 0;
                    self.select_font(now);
                }
            }
            "style" => {
                if let Some(i) = input["key"].as_u64().filter(|i| {
                    self.family()
                        .is_some_and(|f| (*i as usize) < f.styles.len())
                }) {
                    self.style = i as usize;
                    self.select_font(now);
                }
            }
            "tab" => {
                if ["sample", "chars", "info"].contains(&key) {
                    self.tab = key.into();
                    self.sample_changed(now);
                    self.grid_changed();
                }
            }
            "sampleText" => {
                self.text = input["value"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(16_000)
                    .collect();
                self.sample_changed(now);
            }
            "sizeDown" => {
                self.drag_size = true;
                self.size_from_x(input["x"].as_f64().unwrap_or(0.), now);
                return json!({"capture":true});
            }
            "pointerMove" => {
                if self.drag_size {
                    self.size_from_x(input["x"].as_f64().unwrap_or(0.), now);
                }
            }
            "dragEnd" => {
                self.drag_size = false;
                return json!({"capture":false});
            }
            "familyWheel" => {
                let n = self.family_offset + input["deltaY"].as_f64().unwrap_or(0.);
                if n.is_finite() {
                    self.family_offset = n.clamp(0., self.family_max());
                }
            }
            "block" => {
                if let Some(i) = input["key"]
                    .as_u64()
                    .filter(|i| (*i as usize) < self.blocks().len())
                {
                    self.block = i as usize;
                    self.chunk = 0;
                    self.grid_changed();
                }
            }
            "chunk" => {
                if let Some((_, a, b)) = self.current_block() {
                    let max = ((b - a) as usize) / 256;
                    self.chunk = if key == "next" {
                        (self.chunk + 1).min(max)
                    } else {
                        self.chunk.saturating_sub(1)
                    };
                    self.grid_changed();
                }
            }
            "glyph" => {
                if let Some(i) = input["key"].as_u64().map(|i| i as usize) {
                    if self
                        .grid
                        .as_ref()
                        .is_some_and(|g| g.present.get(i) == Some(&true))
                    {
                        self.pick = self.codepoints().get(i).copied();
                    }
                }
            }
            "copy" => {
                if let Some(c) = self.pick.and_then(char::from_u32) {
                    return json!({"effect":{"kind":"copy","host":true,"text":c.to_string()}});
                }
            }
            "install" => self.install(),
            "remove" => self.remove(),
            _ => {}
        }
        self.jobs
            .pump(now)
            .map(|effect| json!({"effect":effect}))
            .unwrap_or_else(|| json!({}))
    }
}
