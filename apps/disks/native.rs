//! Native disk utility. Platform calls provide facts and execute closed actions;
//! selection, confirmation, identity checks, SMART interpretation and deadlines live here.
#[path = "policy.rs"]
mod policy;
#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
#[path = "native_ui.rs"]
mod ui;
use axia_native::{
    async_jobs::{Completed, Jobs},
    i18n::I18n,
    NativeApp,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, rc::Rc};
fn arr(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn key(name: &str) -> Value {
    json!({"key":format!("disks.{name}")})
}
fn smart(cmd: &str, fields: Value) -> Value {
    let mut input = fields;
    input["cmd"] = json!(cmd);
    axia_hw::smart::request(&input).unwrap_or(Value::Null)
}
#[derive(Clone)]
struct Dialog {
    kind: String,
    disk: Value,
    part: Value,
    step: String,
    label: String,
    format: String,
    mode: String,
    typed: String,
    path: String,
    stat: Value,
    images: Value,
    generation: u64,
}
#[derive(Clone)]
struct Operation {
    action: String,
    arg: String,
    disk: Value,
    part: Value,
    dialog: bool,
    revision: u64,
    generation: u64,
}
#[derive(Clone)]
enum Tag {
    Snapshot(u64),
    Validate(Operation),
    Run(Operation),
    Health {
        disk: String,
        purpose: String,
        identity: Value,
    },
    Images(u64),
    Stat(u64, String),
    Events,
    Cancel,
    Install(String, Value),
    Ignore,
}
pub struct App {
    jobs: Jobs<Tag>,
    i18n: I18n,
    theme: Rc<Value>,
    ready: bool,
    booted: bool,
    available: bool,
    list: Value,
    args_list: Value,
    tools: Value,
    admin: bool,
    home: String,
    window: String,
    selected: String,
    part: String,
    opening: Value,
    auto_checked: Value,
    dialog: Option<Dialog>,
    generation: u64,
    busy: bool,
    health_busy: Vec<String>,
    installing: bool,
    health: BTreeMap<String, Value>,
    attrs: BTreeMap<String, bool>,
    message: Value,
    progress: Value,
    result: Value,
    job_error: Value,
    self_test_at: i64,
    self_poll: bool,
    events_at: i64,
    events_pending: bool,
    menu: bool,
    menu_active: i64,
    menu_actions: Vec<String>,
    width: f64,
    list_revision: u64,
    operation_generation: u64,
    now: i64,
    locale_revision: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            i18n: I18n::default(),
            theme: Rc::new(Value::Null),
            ready: false,
            booted: false,
            available: false,
            list: json!([]),
            args_list: Value::Null,
            tools: json!({}),
            admin: false,
            home: String::new(),
            window: String::new(),
            selected: String::new(),
            part: String::new(),
            opening: Value::Null,
            auto_checked: Value::Null,
            dialog: None,
            generation: 0,
            busy: false,
            health_busy: vec![],
            installing: false,
            health: BTreeMap::new(),
            attrs: BTreeMap::new(),
            message: Value::Null,
            progress: Value::Null,
            result: Value::Null,
            job_error: Value::Null,
            self_test_at: 0,
            self_poll: false,
            events_at: 0,
            events_pending: false,
            menu: false,
            menu_active: -1,
            menu_actions: vec![],
            width: 960.,
            list_revision: 0,
            operation_generation: 0,
            now: 0,
            locale_revision: Value::Null,
        }
    }
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
        self.home = home.unwrap_or("").into();
    }
    fn refresh_ms(&self) -> Option<f64> {
        if self.jobs.busy() {
            Some(50.)
        } else if self.self_test_running() {
            Some((self.self_test_at - self.now).max(1) as f64)
        } else {
            None
        }
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, axia_native::clock::now())
    }
    fn key(&mut self, pressed: &str, _: &str, flags: i32) -> Option<Value> {
        if pressed == "Escape" {
            if self.menu {
                self.menu = false;
                return Some(json!({}));
            }
            if self.dialog.is_some() && !self.busy {
                self.dialog = None;
                return Some(json!({}));
            }
        }
        if pressed == "F5" && flags & 30 == 0 && !self.busy {
            self.jobs.enqueue(
                Tag::Snapshot(self.list_revision),
                "disks.snapshot",
                json!([]),
            );
            return Some(json!({}));
        }
        if self.menu {
            let actions = self.menu_actions.clone();
            let count = actions.len() as i64;
            if count > 0 {
                match pressed {
                    "ArrowDown" => self.menu_active = (self.menu_active + 1).rem_euclid(count),
                    "ArrowUp" => {
                        self.menu_active = if self.menu_active < 0 {
                            count - 1
                        } else {
                            (self.menu_active - 1).rem_euclid(count)
                        }
                    }
                    "Enter" | "Return" => {
                        if let Some(action) = actions.get(self.menu_active.max(0) as usize) {
                            let action = action.to_string();
                            return Some(self.request_at(
                                &json!({"cmd":"act","key":action}),
                                axia_native::clock::now(),
                            ));
                        }
                    }
                    _ => return None,
                }
                return Some(json!({}));
            }
        }
        None
    }
}
impl App {
    fn disk(&self) -> &Value {
        arr(&self.list)
            .iter()
            .find(|disk| disk["name"] == self.selected)
            .unwrap_or(&Value::Null)
    }
    fn partition(&self) -> &Value {
        arr(&self.disk()["partitions"])
            .iter()
            .find(|part| part["name"] == self.part)
            .unwrap_or(&Value::Null)
    }
    fn actions(&self) -> Vec<&'static str> {
        [
            "mount", "unmount", "eject", "format", "label", "erase", "image",
        ]
        .into_iter()
        .filter(|a| policy::available(a, self.disk(), self.partition(), self.admin))
        .collect()
    }
    fn update_list(&mut self, value: Value) {
        let stale: Vec<_> = arr(&self.list)
            .iter()
            .filter(|old| {
                !arr(&value).iter().any(|new| {
                    policy::fingerprint(new, &Value::Null) == policy::fingerprint(old, &Value::Null)
                })
            })
            .map(|d| text(&d["name"]).to_owned())
            .collect();
        for disk in stale {
            self.health.remove(&disk);
            self.attrs.remove(&disk);
        }
        let mut rows = value.as_array().cloned().unwrap_or_default();
        rows.sort_by(|a, b| text(&a["name"]).cmp(text(&b["name"])));
        self.list = json!(rows);
        self.ensure_selection();
    }
    fn ensure_selection(&mut self) {
        if !arr(&self.list).iter().any(|d| d["name"] == self.selected) {
            self.selected = arr(&self.list)
                .iter()
                .find(|d| d["removable"] == true)
                .or_else(|| arr(&self.list).first())
                .map(|d| text(&d["name"]).to_owned())
                .unwrap_or_default();
            self.part.clear();
            self.menu = false;
        }
        if !self.part.is_empty()
            && !arr(&self.disk()["partitions"])
                .iter()
                .any(|p| p["name"] == self.part)
        {
            self.part.clear();
        }
    }
    fn self_test_running(&self) -> bool {
        self.part.is_empty()
            && self
                .health
                .get(&self.selected)
                .is_some_and(|v| v["m"]["selfTest"]["running"] == true)
    }
    fn read_health(&mut self, disk: String, purpose: &str) {
        if self.health_busy.contains(&disk) {
            return;
        }
        let Some(device) = arr(&self.list).iter().find(|d| d["name"] == disk) else {
            return;
        };
        if !policy::own_smart(device) {
            return;
        }
        let full = (self.tools["smartctl"] == true
            || self.tools["smartctl"].as_u64().is_some_and(|n| n > 0))
            && policy::smartable(device);
        let method = if purpose == "short" || purpose == "long" {
            "disks.smartTest"
        } else if purpose == "abort" {
            "disks.smartAbort"
        } else if purpose == "poll" || purpose == "started" {
            "disks.smartHealth"
        } else if full {
            "disks.smartInfo"
        } else {
            "disks.smartOwn"
        };
        self.health_busy.push(disk.clone());
        self.jobs.enqueue(
            Tag::Health {
                disk: disk.clone(),
                purpose: purpose.into(),
                identity: policy::fingerprint(device, &Value::Null),
            },
            method,
            if ["short", "long"].contains(&purpose) {
                json!([disk, purpose])
            } else {
                json!([disk])
            },
        );
    }
    fn parse_health(raw: &Value) -> Result<Value, String> {
        if raw["src"] == "own" {
            let mut summary = Value::Null;
            let mut attrs = vec![];
            for line in arr(&raw["lines"]).iter().filter_map(Value::as_str) {
                if let Some(value) = line.strip_prefix("smart ") {
                    if let Ok(v) = serde_json::from_str(value) {
                        summary = v;
                    }
                } else if let Some(value) = line.strip_prefix("attr ") {
                    if attrs.len() < 256 {
                        if let Ok(v) = serde_json::from_str::<Value>(value) {
                            attrs.push(v);
                        }
                    }
                }
            }
            return Ok(json!({"src":"own","summary":summary,"attrs":attrs,"ms":raw["ms"]}));
        }
        let body = arr(&raw["lines"])
            .iter()
            .filter_map(Value::as_str)
            .collect::<String>();
        let value: Value =
            serde_json::from_str(&body).map_err(|_| "disk.smartFailed".to_string())?;
        let exit = text(&raw["status"])
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let m = smart("parse", json!({"json":value,"exit":exit}));
        Ok(json!({"src":"smartctl","m":m,"ms":raw["ms"]}))
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        if let Tag::Health { disk, identity, .. } = &done.tag {
            let same = arr(&self.list).iter().any(|device| {
                device["name"] == *disk && policy::fingerprint(device, &Value::Null) == *identity
            });
            if !same {
                self.health_busy.retain(|name| name != disk);
                self.self_poll = false;
                return;
            }
        }
        let raw = match done.result {
            Ok(value) => value,
            Err(error) => {
                match &done.tag {
                    Tag::Health { disk, purpose, .. } => {
                        self.health_busy.retain(|d| d != disk);
                        self.self_poll = false;
                        self.self_test_at = now + 15000;
                        if purpose != "poll" && purpose != "started" {
                            self.health.insert(disk.clone(), json!({"error":error}));
                        }
                    }
                    Tag::Validate(_) | Tag::Run(_) => {
                        self.busy = false;
                        self.job_error = json!({"error":error});
                        self.message = json!({"error":error});
                        if let Some(dialog) = self.dialog.as_mut() {
                            dialog.step = "run".into();
                        }
                    }
                    Tag::Snapshot(_) => {
                        self.ready = true;
                        self.available = false;
                        self.message = json!({"error":error});
                    }
                    Tag::Events => self.events_pending = false,
                    Tag::Install(..) => {
                        self.installing = false;
                        self.message = json!({"error":error});
                    }
                    _ => self.message = json!({"error":error}),
                }
                return;
            }
        };
        match done.tag {
            Tag::Snapshot(revision) => {
                self.ready = true;
                self.available = raw["available"] == true;
                self.admin = raw["admin"] == true;
                self.tools = raw["tools"].clone();
                if raw["home"].is_string() {
                    self.home = text(&raw["home"]).into();
                }
                if revision == self.list_revision {
                    self.update_list(raw["list"].clone());
                }
            }
            Tag::Validate(operation) => {
                if operation.revision == self.list_revision {
                    self.update_list(raw["list"].clone());
                }
                self.admin = raw["admin"] == true;
                let permits = |list: &Value| {
                    policy::current(list, &operation.disk, &operation.part).is_some_and(
                        |(disk, part)| policy::available(&operation.action, disk, part, self.admin),
                    )
                };
                if permits(&self.list) && permits(&raw["list"]) {
                    self.jobs.enqueue(
                        Tag::Run(operation.clone()),
                        "disks.run",
                        json!([operation.action, operation.arg, operation.generation]),
                    );
                } else {
                    self.busy = false;
                    self.job_error = key("changed");
                    self.message = self.job_error.clone();
                    if let Some(dialog) = self.dialog.as_mut() {
                        dialog.step = "run".into();
                    }
                }
            }
            Tag::Run(operation) => {
                self.busy = false;
                self.result = raw.clone();
                let result = text(&raw);
                self.message = match operation.action.as_str() {
                    "mount" => json!({"key":"disks.mountedAt","params":{"path":result}}),
                    "unmount" => key("unmounted"),
                    "eject" => key("ejected"),
                    "format" => json!({"key":"disks.formatted","params":{"name":result}}),
                    "label" => json!({"key":"disks.renamed","params":{"label":result}}),
                    "erase" => key("erased"),
                    "image" => {
                        let vars = |key: &str| {
                            result
                                .split_whitespace()
                                .find_map(|word| word.strip_prefix(key))
                                .unwrap_or("")
                        };
                        json!({"key":"disks.imageDone","params":{"sha":vars("sha256="),"speed":vars("mbps=")}})
                    }
                    _ => Value::Null,
                };
                self.jobs.enqueue(
                    Tag::Snapshot(self.list_revision),
                    "disks.snapshot",
                    json!([]),
                );
            }
            Tag::Health { disk, purpose, .. } => {
                self.health_busy.retain(|d| d != &disk);
                self.self_poll = false;
                self.self_test_at = now + 15000;
                match Self::parse_health(&raw) {
                    Ok(result) => {
                        let m = &result["m"];
                        if ["short", "long"].contains(&purpose.as_str())
                            && m["exit"].as_u64().unwrap_or(0) & 6 != 0
                        {
                            self.message = json!({"error":"disk.smartTestFailed"});
                            return;
                        }
                        if (purpose == "poll" || purpose == "started") && m["standby"] == true {
                            return;
                        }
                        if ["short", "long"].contains(&purpose.as_str()) {
                            if let Some(old) = self.health.get_mut(&disk) {
                                old["m"]["selfTest"]["running"] = json!(true);
                                old["m"]["selfTest"]["remaining"] = json!(100);
                            }
                            self.read_health(disk, "started");
                            return;
                        }
                        if purpose == "poll" || purpose == "started" {
                            if let Some(old) = self.health.get_mut(&disk) {
                                let mut next = m["selfTest"].clone();
                                if next["supported"] != true
                                    && old["m"]["selfTest"]["supported"] == true
                                {
                                    next["supported"] = json!(true);
                                    next["polling"] = old["m"]["selfTest"]["polling"].clone();
                                }
                                if purpose == "started" && next["running"] != true {
                                    next["running"] = json!(true);
                                    next["remaining"] = json!(100);
                                }
                                old["m"]["selfTest"] = next;
                                if !arr(&m["selfTests"]).is_empty() {
                                    old["m"]["selfTests"] = m["selfTests"].clone();
                                }
                            }
                            if purpose == "poll" && m["selfTest"]["running"] != true {
                                self.read_health(disk, "read");
                            }
                            return;
                        }
                        if purpose == "abort" {
                            self.read_health(disk, "read");
                            return;
                        }
                        if result["src"] == "smartctl" {
                            let summary = smart("summarize", json!({"model":result["m"],"t":now}));
                            self.jobs
                                .enqueue(Tag::Ignore, "disks.summary", json!([disk, summary]));
                        }
                        self.health.insert(disk, result);
                    }
                    Err(error) => {
                        if purpose == "poll" || purpose == "started" {
                            self.message = json!({"error":error});
                        } else {
                            self.health.insert(disk, json!({"error":error}));
                        }
                    }
                }
            }
            Tag::Images(generation) => {
                if let Some(dialog) = self.dialog.as_mut().filter(|d| d.generation == generation) {
                    let mut files = raw.as_array().cloned().unwrap_or_default();
                    files.retain(|f| {
                        f["dir"] != true && text(&f["name"]).to_ascii_lowercase().ends_with(".iso")
                            || f["dir"] != true
                                && text(&f["name"]).to_ascii_lowercase().ends_with(".img")
                    });
                    files.sort_by(|a, b| {
                        b["mtime"]
                            .as_f64()
                            .unwrap_or(0.)
                            .total_cmp(&a["mtime"].as_f64().unwrap_or(0.))
                    });
                    files.dedup_by(|a, b| a["path"] == b["path"]);
                    files.truncate(12);
                    dialog.images = json!(files);
                    if dialog.path.is_empty() {
                        dialog.path = text(&dialog.images[0]["path"]).into();
                        let path = policy::image_path(&dialog.path, &self.home);
                        self.jobs.enqueue(
                            Tag::Stat(generation, path.clone()),
                            "disks.stat",
                            json!([path]),
                        );
                    }
                }
            }
            Tag::Stat(generation, path) => {
                if let Some(dialog) = self.dialog.as_mut().filter(|d| {
                    d.generation == generation && policy::image_path(&d.path, &self.home) == path
                }) {
                    dialog.stat = raw;
                }
            }
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + 250;
                for event in arr(&raw) {
                    if event["kind"] == "progress"
                        && event["generation"].as_u64() == Some(self.operation_generation)
                    {
                        if let Some(value) = policy::progress(text(&event["line"])) {
                            self.progress = value;
                        }
                    }
                }
            }
            Tag::Cancel => {}
            Tag::Install(disk, identity) => {
                self.installing = false;
                self.tools = raw["tools"].clone();
                if self.tools["smartctl"] == true
                    || self.tools["smartctl"].as_u64().is_some_and(|n| n > 0)
                {
                    if arr(&self.list).iter().any(|device| {
                        device["name"] == disk
                            && policy::fingerprint(device, &Value::Null) == identity
                    }) {
                        self.read_health(disk, "read");
                    }
                }
            }
            Tag::Ignore => {}
        }
    }
    fn begin(&mut self, mut operation: Operation) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.progress = Value::Null;
        self.result = Value::Null;
        self.job_error = Value::Null;
        self.message = Value::Null;
        self.operation_generation += 1;
        operation.generation = self.operation_generation;
        operation.revision = self.list_revision;
        if operation.dialog {
            if let Some(dialog) = self.dialog.as_mut() {
                dialog.step = "run".into();
            }
        }
        self.jobs
            .enqueue(Tag::Validate(operation), "disks.snapshot", json!([]));
    }
    fn dialog_valid(&self, dialog: &Dialog) -> bool {
        if !policy::available(&dialog.kind, &dialog.disk, &dialog.part, self.admin) {
            return false;
        }
        match dialog.kind.as_str() {
            "format" => {
                policy::formats(&self.tools).contains(&dialog.format.as_str())
                    && policy::label_valid(&dialog.label, &dialog.format)
            }
            "label" => {
                policy::label_valid(&dialog.label, text(&dialog.part["fstype"]))
                    && (text(&dialog.part["mountpoint"]).is_empty()
                        || text(&dialog.part["fstype"]).starts_with("ext"))
            }
            "image" => {
                policy::image_valid(&dialog.stat, &dialog.disk)
                    && !dialog.path.chars().any(char::is_control)
            }
            "erase" => ["quick", "zero"].contains(&dialog.mode.as_str()),
            _ => false,
        }
    }
    fn confirm(&mut self) {
        let Some(dialog) = self.dialog.as_ref() else {
            return;
        };
        if self.busy
            || !self.dialog_valid(dialog)
            || dialog.kind == "label" && dialog.step != "choose"
        {
            return;
        }
        if dialog.kind != "label"
            && (dialog.step != "confirm"
                || !dialog.part.is_object() && dialog.typed.trim() != text(&dialog.disk["name"]))
        {
            return;
        }
        let target = text(&policy::target(&dialog.disk, &dialog.part)["name"]);
        let arg = match dialog.kind.as_str() {
            "format" => format!(
                "{target}\n{}\n{}",
                dialog.format,
                policy::clean_label(&dialog.label, &dialog.format)
            ),
            "erase" => format!("{target}\n{}", dialog.mode),
            "label" => format!(
                "{target}\n{}",
                policy::clean_label(&dialog.label, text(&dialog.part["fstype"]))
            ),
            "image" => format!("{target}\n{}", policy::image_path(&dialog.path, &self.home)),
            _ => return,
        };
        self.begin(Operation {
            action: dialog.kind.clone(),
            arg,
            disk: dialog.disk.clone(),
            part: dialog.part.clone(),
            dialog: true,
            revision: 0,
            generation: 0,
        });
    }
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        self.now = now;
        let cmd = text(&input["cmd"]);
        let arg = text(&input["key"]);
        match cmd {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                if self.locale_revision != input["context"]["localeRevision"] {
                    self.locale_revision = input["context"]["localeRevision"].clone();
                    self.i18n.invalidate();
                }
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.i18n.decimal = input["context"]["decimal"].as_str().unwrap_or(".").into();
                self.i18n.group = input["context"]["group"].as_str().unwrap_or(",").into();
                self.width = input["context"]["width"].as_f64().unwrap_or(960.);
                if input["context"]["theme"].is_object() {
                    self.theme = Rc::new(input["context"]["theme"].clone());
                }
                if !self.booted {
                    if let Some(owner) =
                        input["args"]["__nativeOwner"].as_i64().filter(|id| *id > 0)
                    {
                        self.booted = true;
                        self.jobs.bind(owner);
                        self.window = text(&input["args"]["__nativeWindow"]).into();
                        self.jobs.enqueue(
                            Tag::Snapshot(self.list_revision),
                            "disks.snapshot",
                            json!([]),
                        );
                    }
                }
                let facts = &input["args"]["__nativeDisks"];
                if facts.is_array() && facts != &self.args_list {
                    self.list_revision += 1;
                    self.args_list = facts.clone();
                    self.update_list(facts.clone());
                }
                let opening = json!([input["args"]["disk"], input["args"]["nonce"]]);
                if opening != self.opening {
                    self.opening = opening;
                    let selected = text(&input["args"]["disk"]);
                    if !selected.is_empty() {
                        self.selected = selected.into();
                        self.part.clear();
                        self.message = Value::Null;
                    }
                }
                if self.ready {
                    self.ensure_selection();
                    if self.opening[0] == self.selected
                        && self.auto_checked != self.opening
                        && !self.health.contains_key(&self.selected)
                    {
                        self.auto_checked = self.opening.clone();
                        self.read_health(self.selected.clone(), "read");
                    }
                }
                if self.self_test_running() && !self.self_poll && now >= self.self_test_at {
                    self.self_poll = true;
                    self.read_health(self.selected.clone(), "poll");
                }
                if self.busy && !self.events_pending && now >= self.events_at {
                    self.events_pending = true;
                    self.jobs.enqueue(Tag::Events, "disks.events", json!([]));
                }
                let tree = ui::tree(self);
                let mut reply = json!({"tree":tree});
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
            "select" if self.dialog.is_none() => {
                let disk = text(&input["key"][0]);
                let part = text(&input["key"][1]);
                if arr(&self.list).iter().any(|d| d["name"] == disk) {
                    self.selected = disk.into();
                    self.part = part.into();
                    self.message = Value::Null;
                    self.menu = false;
                }
            }
            "act" if !self.busy && self.dialog.is_none() => {
                self.menu = false;
                if self.actions().contains(&arg) {
                    let disk = self.disk().clone();
                    let part = self.partition().clone();
                    if ["mount", "unmount", "eject"].contains(&arg) {
                        self.begin(Operation {
                            action: arg.into(),
                            arg: text(&policy::target(&disk, &part)["name"]).into(),
                            disk,
                            part,
                            dialog: false,
                            revision: 0,
                            generation: 0,
                        });
                    } else {
                        self.generation += 1;
                        let generation = self.generation;
                        self.dialog = Some(Dialog {
                            kind: arg.into(),
                            disk,
                            part: part.clone(),
                            step: "choose".into(),
                            label: if arg == "label" {
                                text(&part["label"]).into()
                            } else {
                                String::new()
                            },
                            format: policy::formats(&self.tools)
                                .first()
                                .unwrap_or(&"")
                                .to_string(),
                            mode: "quick".into(),
                            typed: String::new(),
                            path: String::new(),
                            stat: Value::Null,
                            images: json!([]),
                            generation,
                        });
                        self.message = Value::Null;
                        self.result = Value::Null;
                        self.job_error = Value::Null;
                        if arg == "image" {
                            self.jobs
                                .enqueue(Tag::Images(generation), "disks.images", json!([]));
                        }
                    }
                }
            }
            "dialogClose" if !self.busy => {
                self.dialog = None;
                self.jobs.enqueue(
                    Tag::Snapshot(self.list_revision),
                    "disks.snapshot",
                    json!([]),
                );
            }
            "dialogBack" if !self.busy => {
                if let Some(dialog) = self.dialog.as_mut().filter(|d| d.step == "confirm") {
                    dialog.step = "choose".into();
                    dialog.typed.clear();
                }
            }
            "dialogNext" if !self.busy => {
                if let Some(dialog) = self
                    .dialog
                    .as_ref()
                    .filter(|d| d.step == "choose" && self.dialog_valid(d))
                {
                    if dialog.kind == "label" {
                        self.confirm();
                    } else if let Some(dialog) = self.dialog.as_mut() {
                        dialog.step = "confirm".into();
                        dialog.typed.clear();
                    }
                }
            }
            "confirm" => self.confirm(),
            "label" | "typed" | "format" | "eraseMode" | "imagePath" if !self.busy => {
                let value = input["value"].as_str().unwrap_or(arg);
                if let Some(dialog) = self.dialog.as_mut().filter(|d| {
                    if cmd == "typed" {
                        d.step == "confirm"
                    } else {
                        d.step == "choose"
                    }
                }) {
                    match cmd {
                        "label" => dialog.label = value.chars().take(128).collect(),
                        "typed" => dialog.typed = value.chars().take(64).collect(),
                        "format" => {
                            if policy::formats(&self.tools).contains(&arg) {
                                dialog.format = arg.into();
                            }
                        }
                        "eraseMode" => {
                            if ["quick", "zero"].contains(&arg) {
                                dialog.mode = arg.into();
                            }
                        }
                        "imagePath" => {
                            dialog.path = value.chars().take(4096).collect();
                            dialog.stat = Value::Null;
                            let path = policy::image_path(&dialog.path, &self.home);
                            self.jobs.cancel(|tag| matches!(tag, Tag::Stat(..)));
                            self.jobs.enqueue(
                                Tag::Stat(dialog.generation, path.clone()),
                                "disks.stat",
                                json!([path]),
                            );
                        }
                        _ => {}
                    }
                }
            }
            "cancelJob" if self.busy => {
                if let Some(dialog) = self
                    .dialog
                    .as_ref()
                    .filter(|d| d.kind == "image" || d.kind == "erase" && d.mode == "zero")
                {
                    self.jobs
                        .enqueue(Tag::Cancel, "disks.cancel", json!([dialog.disk["name"]]));
                }
            }
            "health" => {
                self.message = Value::Null;
                self.read_health(self.selected.clone(), "read");
            }
            "test" if ["short", "long", "abort"].contains(&arg) => {
                if self.admin || self.disk()["removable"] == true {
                    let m = &self.health.get(&self.selected).unwrap_or(&Value::Null)["m"];
                    if m["selfTest"]["supported"] == true
                        && (arg == "abort") == (m["selfTest"]["running"] == true)
                    {
                        self.message = Value::Null;
                        self.read_health(self.selected.clone(), arg);
                    }
                }
            }
            "install" if self.admin && !self.installing => {
                self.installing = true;
                self.message = Value::Null;
                self.jobs.enqueue(
                    Tag::Install(
                        self.selected.clone(),
                        policy::fingerprint(self.disk(), &Value::Null),
                    ),
                    "disks.installSmart",
                    json!([]),
                );
            }
            "attrs" => {
                let open = self.attrs.entry(self.selected.clone()).or_default();
                *open = !*open;
            }
            "more" => {
                self.menu = !self.menu;
                self.menu_active = -1;
            }
            "menuDismiss" => self.menu = false,
            "menuHover" => self.menu_active = input["key"].as_i64().unwrap_or(-1),
            "close" => {
                self.jobs.cancel(|_| true);
                return json!({});
            }
            _ => {}
        }
        let mut reply = json!({});
        if let Some(effect) = self.jobs.pump(now) {
            reply["effect"] = effect;
        }
        reply
    }
}
