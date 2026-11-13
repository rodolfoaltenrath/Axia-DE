//! Native eSCL scanner. Existing SDK services retain network validation and
//! confined decoding/conversion; Rust owns the complete workflow and scene.
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    date_format,
    i18n::I18n,
    NativeApp,
};
use serde_json::{json, Value};

#[derive(Debug)]
enum Tag {
    Discover(u64),
    Caps(u64),
    Preview(u64),
    Decode(u64),
    Scan(u64),
    Directory(u64, usize),
    Name(u64),
    Copy(u64),
    CopyPoll(u64),
    Convert(u64),
    Release,
    Forget,
    Print,
}
impl Tag {
    fn generation(&self) -> Option<u64> {
        match self {
            Self::Discover(g)
            | Self::Caps(g)
            | Self::Preview(g)
            | Self::Decode(g)
            | Self::Scan(g)
            | Self::Directory(g, _)
            | Self::Name(g)
            | Self::Copy(g)
            | Self::CopyPoll(g)
            | Self::Convert(g) => Some(*g),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Busy {
    Preview,
    Scan,
}
#[derive(Clone, Debug)]
struct Image {
    id: i64,
    w: f64,
    h: f64,
}
impl Image {
    fn read(v: &Value) -> Option<Self> {
        let id = v["image"]
            .as_i64()
            .filter(|id| *id > 0 && *id <= i32::MAX as i64)?;
        let (w, h) = (v["w"].as_f64()?, v["h"].as_f64()?);
        [w, h]
            .iter()
            .all(|n| n.is_finite() && *n > 0. && *n <= 32768.)
            .then_some(Self { id, w, h })
    }
}
#[derive(Clone, Copy, Debug)]
struct Frame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}
struct Drag {
    frame: Frame,
    start: [f64; 2],
}
struct Save {
    format: String,
    res: u64,
    direct: bool,
    input: String,
    base: String,
    dir: String,
    path: String,
    attempt: usize,
    job: Option<u64>,
    poll_at: i64,
}
#[derive(Clone)]
struct Saved {
    path: String,
    format: String,
}
pub struct App {
    jobs: Jobs<Tag>,
    booted: bool,
    epoch: u64,
    last_args: Value,
    scanner: Option<Value>,
    found: Option<Vec<Value>>,
    caps: Option<Value>,
    color: String,
    res: u64,
    format: String,
    source: String,
    preview: Option<Image>,
    area: Option<[f64; 4]>,
    drag: Option<Drag>,
    busy: Option<Busy>,
    save: Option<Save>,
    saved: Option<Saved>,
    error: Value,
    notice: Value,
    i18n: I18n,
    home: String,
    region: String,
    hour_override: String,
    rtl: bool,
    width: f64,
    height: f64,
    theme: Value,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            booted: false,
            epoch: 0,
            last_args: Value::Null,
            scanner: None,
            found: None,
            caps: None,
            color: "RGB24".into(),
            res: 150,
            format: "pdf".into(),
            source: "Platen".into(),
            preview: None,
            area: None,
            drag: None,
            busy: None,
            save: None,
            saved: None,
            error: Value::Null,
            notice: Value::Null,
            i18n: I18n::default(),
            home: String::new(),
            region: String::new(),
            hour_override: "auto".into(),
            rtl: false,
            width: 860.,
            height: 620.,
            theme: Value::Null,
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
        (self.jobs.busy() || self.save.as_ref().is_some_and(|s| s.job.is_some())).then_some(50.)
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
}
fn dimension(caps: &Value, key: &str, fallback: f64) -> f64 {
    caps[key]
        .as_f64()
        .filter(|n| n.is_finite() && *n > 0. && *n <= i32::MAX as f64)
        .unwrap_or(fallback)
}
impl App {
    fn resolutions(&self) -> Vec<u64> {
        self.caps.as_ref().unwrap_or(&Value::Null)["res"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .filter(|n| *n > 0 && *n <= i32::MAX as u64)
            .collect()
    }
    fn colors(&self) -> Vec<String> {
        self.caps.as_ref().unwrap_or(&Value::Null)["colors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|s| ["RGB24", "Grayscale8", "BlackAndWhite1"].contains(s))
            .map(str::to_owned)
            .collect()
    }
    fn formats(&self) -> Vec<String> {
        let caps = self.caps.as_ref().unwrap_or(&Value::Null);
        let mut out = vec![];
        for key in ["formats", "formats2"] {
            for format in caps[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !out.iter().any(|s| s == format) {
                    out.push(format.to_owned());
                }
            }
        }
        out
    }
    fn image_format(&self) -> String {
        let formats = self.formats();
        ["image/jpeg", "image/png"]
            .iter()
            .find(|f| formats.iter().any(|s| s == **f))
            .map(|s| s.to_string())
            .or_else(|| formats.first().cloned())
            .unwrap_or_else(|| "image/jpeg".into())
    }
    fn region(&self) -> Value {
        let caps = self.caps.as_ref().unwrap_or(&Value::Null);
        let (w, h) = (
            dimension(caps, "maxW", 2550.),
            dimension(caps, "maxH", 3508.),
        );
        let [x, y, sw, sh] = self.area.unwrap_or([0., 0., 1., 1.]);
        json!({"x":(x*w).round(),"y":(y*h).round(),"w":(sw*w).round().max(30.),"h":(sh*h).round().max(30.)})
    }
    fn frame(&self) -> Option<Frame> {
        let image = self.preview.as_ref()?;
        let pane = (self.width - 280.).max(1.);
        let scale = ((pane - 40.).max(1.) / image.w)
            .min((self.height - 40.).max(1.) / image.h)
            .min(1.);
        Some(Frame {
            x: if self.rtl { 280. } else { 0. } + (pane - image.w * scale) / 2.,
            y: (self.height - image.h * scale) / 2.,
            w: image.w * scale,
            h: image.h * scale,
        })
    }
    fn point(frame: Frame, input: &Value) -> Option<[f64; 2]> {
        let x = input["x"].as_f64()?;
        let y = input["y"].as_f64()?;
        if ![x, y].iter().all(|n| n.is_finite()) {
            return None;
        }
        Some([
            ((x - frame.x) / frame.w).clamp(0., 1.),
            ((y - frame.y) / frame.h).clamp(0., 1.),
        ])
    }
    fn release(&mut self, id: i64) {
        self.jobs
            .enqueue(Tag::Release, "images.release", json!([[id]]));
    }
    fn clear_preview(&mut self) {
        if let Some(image) = self.preview.take() {
            self.release(image.id);
        }
        self.area = None;
        self.drag = None;
    }
    fn choose(&mut self, scanner: Option<Value>) {
        self.epoch += 1;
        self.jobs.cancel(|t| t.generation().is_some());
        self.clear_preview();
        self.scanner = scanner;
        self.caps = None;
        self.busy = None;
        self.save = None;
        self.error = Value::Null;
        self.notice = Value::Null;
        self.source = "Platen".into();
        self.res = 150;
        self.color = "RGB24".into();
        if let Some(device) = &self.scanner {
            self.jobs
                .enqueue(Tag::Caps(self.epoch), "scanner.caps", json!([device]));
        } else {
            self.found = None;
            self.jobs
                .enqueue(Tag::Discover(self.epoch), "scanner.discover", json!([]));
        }
    }
    fn fail(&mut self, error: String, caps: bool) {
        self.error = json!({"key":if caps{"scanner.unreachable"}else{"scanner.failed"},"params":{"error":error}});
        self.busy = None;
        self.save = None;
    }
    fn start_preview(&mut self) {
        if self.busy.is_some() || self.caps.is_none() {
            return;
        }
        let Some(device) = &self.scanner else {
            return;
        };
        let caps = self.caps.as_ref().unwrap();
        let resolutions = self.resolutions();
        let low = resolutions
            .iter()
            .copied()
            .filter(|r| *r >= 50)
            .min()
            .or_else(|| resolutions.iter().copied().min())
            .unwrap_or(75);
        let colors = self.colors();
        let color = if colors.iter().any(|s| s == "RGB24") {
            String::from("RGB24")
        } else {
            colors.first().cloned().unwrap_or_else(|| "RGB24".into())
        };
        let options = json!({"source":self.source,"color":color,"res":low,"format":self.image_format(),"x":0,"y":0,"w":dimension(caps,"maxW",2550.),"h":dimension(caps,"maxH",3508.)});
        self.busy = Some(Busy::Preview);
        self.error = Value::Null;
        self.jobs.enqueue(
            Tag::Preview(self.epoch),
            "scanner.scan",
            json!([device, options]),
        );
    }
    fn start_scan(&mut self) {
        if self.busy.is_some() || self.caps.is_none() {
            return;
        }
        let Some(device) = &self.scanner else {
            return;
        };
        let mime = match self.format.as_str() {
            "png" => "image/png",
            "jpeg" => "image/jpeg",
            _ => "application/pdf",
        };
        let direct = self.formats().iter().any(|f| f == mime);
        let mut options = self.region();
        options["source"] = json!(self.source);
        options["color"] = json!(self.color);
        options["res"] = json!(self.res);
        options["format"] = json!(if direct {
            mime.into()
        } else {
            self.image_format()
        });
        self.save = Some(Save {
            format: self.format.clone(),
            res: self.res,
            direct,
            input: String::new(),
            base: String::new(),
            dir: String::new(),
            path: String::new(),
            attempt: 1,
            job: None,
            poll_at: 0,
        });
        self.busy = Some(Busy::Scan);
        self.error = Value::Null;
        self.notice = Value::Null;
        self.jobs.enqueue(
            Tag::Scan(self.epoch),
            "scanner.scan",
            json!([device, options]),
        );
    }
    fn directories(&self) -> Vec<String> {
        let pdf = self.save.as_ref().is_some_and(|s| s.format == "pdf");
        (if pdf {
            vec!["Documentos", "Documents"]
        } else {
            vec!["Imágenes", "Pictures", "Imagens"]
        })
        .into_iter()
        .map(|d| format!("{}/{d}", self.home))
        .collect()
    }
    fn directory(&mut self, index: usize) {
        if let Some(path) = self.directories().get(index) {
            self.jobs
                .enqueue(Tag::Directory(self.epoch, index), "fs.stat", json!([path]));
        } else {
            if let Some(save) = &mut self.save {
                save.dir = self.home.clone();
            }
            self.check_name();
        }
    }
    fn base_name(&mut self, now: i64) -> String {
        let t = crate::clock::local(now);
        let date = date_format::date(&mut self.i18n, &self.region, &t, "numeric").replace('/', "-");
        let time = date_format::time(&mut self.i18n, &self.region, &self.hour_override, &t, false)
            .replace(':', ".");
        self.i18n
            .caption(&json!({"key":"scanner.fileName","params":{"date":date,"time":time}}))
            .replace(['/', '\\', '\0'], "-")
    }
    fn check_name(&mut self) {
        if let Some(s) = &mut self.save {
            let ext = if s.format == "jpeg" { "jpg" } else { &s.format };
            let suffix = if s.attempt == 1 {
                String::new()
            } else {
                format!(" {}", s.attempt)
            };
            s.path = format!("{}/{}{suffix}.{ext}", s.dir, s.base);
            self.jobs
                .enqueue(Tag::Name(self.epoch), "fs.stat", json!([s.path]));
        }
    }
    fn finish(&mut self) {
        if let Some(save) = self.save.take() {
            let name = std::path::Path::new(&save.path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            self.notice = json!({"key":"scanner.saved","params":{"name":name}});
            self.saved = Some(Saved {
                path: save.path,
                format: save.format,
            });
        }
        self.busy = None;
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        let Completed { tag, result } = done;
        if tag.generation().is_some_and(|g| g != self.epoch) {
            if let Ok(v) = result {
                if let Some(id) = v["image"].as_i64() {
                    self.release(id);
                }
            }
            return;
        }
        match tag {
            Tag::Discover(_) => {
                self.found = Some(
                    result
                        .ok()
                        .map(|r| {
                            r["scanners"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter(|s| s["lan"] == true && s["tls"] != true)
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default(),
                )
            }
            Tag::Caps(_) => match result {
                Ok(caps) => {
                    self.caps = Some(caps);
                    let res = self.resolutions();
                    self.res = if res.contains(&300) {
                        300
                    } else if res.contains(&150) {
                        150
                    } else {
                        res.first().copied().unwrap_or(150)
                    };
                    let colors = self.colors();
                    self.color = if colors.iter().any(|s| s == "RGB24") {
                        String::from("RGB24")
                    } else {
                        colors.first().cloned().unwrap_or_else(|| "RGB24".into())
                    };
                    let caps = self.caps.as_ref().unwrap();
                    if caps["platen"] != true && caps["adf"] == true {
                        self.source = "Feeder".into();
                    }
                }
                Err(error) => self.fail(error, true),
            },
            Tag::Preview(_) => match result {
                Ok(v) => {
                    if let Some(path) = v["file"].as_str() {
                        if v["format"] == "application/pdf" {
                            self.jobs.enqueue(
                                Tag::Decode(self.epoch),
                                "images.pdfThumb",
                                json!([path,{"width":520,"height":560}]),
                            );
                        } else {
                            self.jobs.enqueue(
                                Tag::Decode(self.epoch),
                                "images.decode",
                                json!([path,{"maxw":520,"maxh":560}]),
                            );
                        }
                    } else {
                        self.fail("No image returned by the scanner".into(), false)
                    }
                }
                Err(error) => self.fail(error, false),
            },
            Tag::Decode(_) => match result {
                Ok(v) => {
                    if let Some(image) = Image::read(&v) {
                        self.clear_preview();
                        self.preview = Some(image);
                        self.busy = None;
                    } else {
                        if let Some(id) = v["image"].as_i64() {
                            self.release(id);
                        }
                        self.fail("Could not decode the preview".into(), false)
                    }
                }
                Err(error) => self.fail(error, false),
            },
            Tag::Scan(_) => match result {
                Ok(v) => {
                    if let Some(path) = v["file"].as_str() {
                        let base = self.base_name(now);
                        if let Some(save) = &mut self.save {
                            save.input = path.into();
                            save.base = base;
                        }
                        self.directory(0);
                    } else {
                        self.fail("No document returned by the scanner".into(), false)
                    }
                }
                Err(error) => self.fail(error, false),
            },
            Tag::Directory(_, index) => {
                if result.as_ref().is_ok_and(|v| v["dir"] == true) {
                    let path = self.directories()[index].clone();
                    if let Some(save) = &mut self.save {
                        save.dir = path;
                    }
                    self.check_name();
                } else {
                    self.directory(index + 1);
                }
            }
            Tag::Name(_) => {
                if result.is_ok() {
                    if let Some(save) = &mut self.save {
                        save.attempt += 1;
                        if save.attempt > 1000 {
                            self.fail("Could not choose a free file name".into(), false);
                            return;
                        }
                    }
                    self.check_name();
                } else if let Some(s) = &self.save {
                    if s.direct {
                        self.jobs.enqueue(
                            Tag::Copy(self.epoch),
                            "fs.copy",
                            json!([s.input, s.path]),
                        );
                    } else {
                        self.jobs.enqueue(
                            Tag::Convert(self.epoch),
                            "images.convert",
                            json!([s.input,s.path,{"format":s.format,"native":true,"dpi":s.res}]),
                        );
                    }
                }
            }
            Tag::Copy(_) => match result {
                Ok(v) => {
                    if let Some(id) = v.as_u64().filter(|id| *id > 0) {
                        if let Some(s) = &mut self.save {
                            s.job = Some(id);
                            s.poll_at = now + 200;
                        }
                    } else {
                        self.fail("Could not save the scan".into(), false)
                    }
                }
                Err(error) => self.fail(error, false),
            },
            Tag::CopyPoll(_) => match result {
                Ok(v) => {
                    let id = self.save.as_ref().and_then(|s| s.job);
                    let job = v
                        .as_array()
                        .and_then(|jobs| jobs.iter().find(|j| j["id"].as_u64() == id));
                    if let Some(job) = job {
                        if job["done"] == true {
                            if let Some(id) = id {
                                self.jobs.enqueue(Tag::Forget, "fs.forgetJob", json!([id]));
                            }
                            if job["failed"] == true
                                || job["failed"].as_u64().is_some_and(|n| n > 0)
                                || job["error"].as_str().is_some_and(|s| !s.is_empty())
                            {
                                self.fail(
                                    job["error"]
                                        .as_str()
                                        .filter(|s| !s.is_empty())
                                        .unwrap_or("Could not save the scan")
                                        .into(),
                                    false,
                                );
                            } else {
                                self.finish();
                            }
                        } else if let Some(s) = &mut self.save {
                            s.poll_at = now + 200;
                        }
                    } else {
                        self.fail("Could not finish saving the scan".into(), false);
                    }
                }
                Err(error) => self.fail(error, false),
            },
            Tag::Convert(_) => match result {
                Ok(_) => self.finish(),
                Err(error) => self.fail(error, false),
            },
            Tag::Release | Tag::Forget | Tag::Print => {}
        }
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
                    .filter(|n| n.is_finite())
                    .unwrap_or(860.)
                    .max(0.);
                self.height = input["context"]["height"]
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .unwrap_or(620.)
                    .max(0.);
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.region = input["settings"]["region"].as_str().unwrap_or("").into();
                self.hour_override = input["settings"]["clock"]["hour24"]
                    .as_str()
                    .unwrap_or("auto")
                    .into();
                let lang = self.i18n.lang.clone();
                self.rtl = self.i18n.entry(&lang, "meta", "direction").or_else(|| {
                    self.i18n
                        .entry(lang.split('-').next().unwrap_or(&lang), "meta", "direction")
                }) == Some(json!("rtl"));
                let args =
                    json!({"scanner":input["args"]["scanner"],"nonce":input["args"]["nonce"]});
                if !self.booted {
                    if let Some(owner) =
                        input["args"]["__nativeOwner"].as_i64().filter(|id| *id > 0)
                    {
                        self.jobs.bind(owner);
                        self.booted = true;
                        self.last_args = args.clone();
                        self.choose(
                            input["args"]["scanner"]
                                .is_object()
                                .then(|| input["args"]["scanner"].clone()),
                        );
                    }
                } else if args != self.last_args {
                    self.last_args = args;
                    if input["args"]["scanner"].is_object() {
                        self.choose(Some(input["args"]["scanner"].clone()));
                    }
                }
                if let Some(save) = &mut self.save {
                    if save.job.is_some() && save.poll_at > 0 && now >= save.poll_at {
                        save.poll_at = 0;
                        self.jobs
                            .enqueue(Tag::CopyPoll(self.epoch), "fs.jobs", json!([]));
                    }
                }
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
            "choose" => {
                if self.busy.is_none() {
                    if let Some(device) = input["key"]
                        .as_u64()
                        .and_then(|i| self.found.as_ref()?.get(i as usize))
                        .cloned()
                    {
                        self.choose(Some(device));
                    }
                }
            }
            "other" => {
                if self.busy.is_none() {
                    self.choose(None);
                }
            }
            "source" => {
                if self.caps.as_ref().is_some_and(|c| {
                    if key == "Platen" {
                        c["platen"] == true
                    } else {
                        key == "Feeder" && c["adf"] == true
                    }
                }) {
                    self.source = key.into();
                }
            }
            "color" => {
                if self.colors().iter().any(|c| c == key)
                    || self.colors().is_empty() && key == "RGB24"
                {
                    self.color = key.into();
                }
            }
            "resolution" => {
                if let Some(res) = input["key"].as_u64() {
                    let allowed = self.resolutions();
                    if allowed.contains(&res)
                        || allowed.is_empty() && [75, 150, 300, 600].contains(&res)
                    {
                        self.res = res;
                    }
                }
            }
            "format" => {
                if ["pdf", "png", "jpeg"].contains(&key) {
                    self.format = key.into();
                }
            }
            "preview" => self.start_preview(),
            "scan" => self.start_scan(),
            "cropStart" => {
                if let Some(frame) = self.frame() {
                    if let Some(start) = Self::point(frame, input) {
                        self.area = Some([start[0], start[1], 0., 0.]);
                        self.drag = Some(Drag { frame, start });
                        return json!({"capture":true});
                    }
                }
            }
            "pointerMove" => {
                if let Some(d) = &self.drag {
                    if let Some([x, y]) = Self::point(d.frame, input) {
                        self.area = Some([
                            x.min(d.start[0]),
                            y.min(d.start[1]),
                            (x - d.start[0]).abs(),
                            (y - d.start[1]).abs(),
                        ]);
                    }
                }
            }
            "dragEnd" => {
                self.drag = None;
                if self.area.is_some_and(|[_, _, w, h]| w < 0.02 || h < 0.02) {
                    self.area = None;
                }
                return json!({"capture":false});
            }
            "print" => {
                if let Some(saved) = &self.saved {
                    if saved.format != "pdf" {
                        let title = std::path::Path::new(&saved.path)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        self.jobs.enqueue(
                            Tag::Print,
                            "print.open",
                            json!([{"title":title,"image":saved.path}]),
                        );
                    }
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
