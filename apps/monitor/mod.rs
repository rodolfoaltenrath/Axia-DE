//! Monitor: all application behavior, metrics, drawing and views are owned by Rust.
mod charts;
pub mod compat;
mod connections;
mod countries;
mod dashboard;
mod fan;
mod geo;
mod history;
mod memory;
mod metrics;
mod paint;
mod processes;
mod rules;
mod sensors;
mod system;
#[cfg(test)]
mod tests;
mod theme;
mod ui;
mod worker;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
pub fn read(path: &Path, limit: usize) -> String {
    let Ok(f) = fs::File::open(path) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    if f.take(limit as u64).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
pub fn entries(path: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .take(20000)
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
}
pub fn number(text: &str, key: &str) -> f64 {
    text.lines()
        .find_map(|l| {
            l.strip_prefix(key)
                .and_then(|s| s.split_whitespace().next())
                .and_then(|n| n.parse::<f64>().ok())
                .filter(|n| n.is_finite())
        })
        .unwrap_or(0.)
}
pub fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tab {
    Overview,
    Cpu,
    Memory,
    Network,
    Disks,
    Processes,
    Cooling,
    Voltage,
}
impl Tab {
    pub const ALL: [Self; 8] = [
        Self::Overview,
        Self::Cpu,
        Self::Memory,
        Self::Network,
        Self::Disks,
        Self::Processes,
        Self::Cooling,
        Self::Voltage,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Network => "network",
            Self::Disks => "disks",
            Self::Processes => "processes",
            Self::Cooling => "cooling",
            Self::Voltage => "voltage",
        }
    }
    pub fn parse(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|t| t.id() == id)
            .unwrap_or(Self::Overview)
    }
}
pub struct App {
    lease: Option<worker::Lease>,
    pub tab: Tab,
    pub section: String,
    pub query: String,
    pub mine: bool,
    pub sort: String,
    pub reverse: bool,
    pub selected: Option<(i32, u64)>,
    pub detail_tab: String,
    pub confirming: bool,
    pub disk: String,
    pub app: String,
    pub scope: String,
    pub protocol: String,
    pub history_open: bool,
    pub history_span: u64,
    pub map_zoom: f64,
    pub map_lon: f64,
    pub map_lat: f64,
    pub images: charts::Images,
    /// Icon rasters survive rebuilds; they only change with the theme.
    pub icons: charts::Images,
    pub snapshot: Arc<worker::Snapshot>,
    pub frozen: Option<Arc<worker::Snapshot>>,
    pub prefs: Value,
    pub services: Value,
    pub error: Option<String>,
    pub cleared: bool,
    args: Value,
    effects: VecDeque<Value>,
    pub generation: u64,
    pub voltage_history: VecDeque<(u64, f64)>,
    voltage_key: String,
    pub form: Value,
    pub catalog: Value,
    pub rules_data: Value,
    requested_app: String,
    pub form_error: Option<String>,
    pub map_request: String,
    pub disk_history: std::collections::BTreeMap<String, VecDeque<(u64, f64)>>,
    /// What the last widget tree was built from; an identical sync reuses it.
    built: Option<Built>,
}
#[derive(PartialEq)]
struct Built {
    seq: u64,
    context: Value,
    settings: Value,
    services: Value,
    catalog: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            lease: None,
            tab: Tab::Overview,
            section: String::new(),
            query: String::new(),
            mine: false,
            sort: "cpu".into(),
            reverse: true,
            selected: None,
            detail_tab: "memory".into(),
            confirming: false,
            disk: String::new(),
            app: String::new(),
            scope: String::new(),
            protocol: String::new(),
            history_open: false,
            history_span: 3600,
            map_zoom: 1.,
            map_lon: -180.,
            map_lat: 90.,
            images: Default::default(),
            icons: Default::default(),
            snapshot: Default::default(),
            frozen: None,
            prefs: json!({}),
            services: json!({}),
            error: None,
            cleared: false,
            args: Value::Null,
            effects: Default::default(),
            generation: 0,
            voltage_history: Default::default(),
            voltage_key: String::new(),
            form: json!({"action":"deny","proto":"","dir":"out","dst":"","port":"","url":"","format":"domains"}),
            catalog: Value::Null,
            rules_data: Value::Null,
            requested_app: String::new(),
            form_error: None,
            map_request: String::new(),
            disk_history: Default::default(),
            built: None,
        }
    }
}
impl App {
    fn service(&mut self) -> Arc<worker::Service> {
        if self.lease.is_none() {
            self.lease = Some(worker::Service::shared().lease());
        }
        self.lease.as_ref().unwrap().service.clone()
    }
    pub fn pixels(&self, id: &str) -> Option<&charts::Raster> {
        self.images
            .get(id)
            .or_else(|| self.icons.get(id))
            .map(Arc::as_ref)
    }
    /// A privacy panel shown inside Settings: no sidebar or header.
    pub fn embedded(&self) -> bool {
        self.args["section"].as_str().is_some_and(|s| !s.is_empty())
    }
    fn effect(&mut self, kind: &str, args: Value) {
        let mut effect = args;
        effect["kind"] = json!(kind);
        self.effects.push_back(effect);
    }
    fn pref(&mut self, key: &str) {
        let current = self.prefs[key].as_bool().unwrap_or(key != "menuItem");
        self.prefs[key] = json!(!current);
        let prefs = self.prefs.clone();
        self.effect("settings", json!({"patch":{"monitor":prefs}}));
    }
    fn query_history(&mut self) {
        let now = worker::wall() / 1000;
        let span = self.history_span;
        self.service().job(worker::Job::Query(json!({"from":now.saturating_sub(span),"step":if span>86400{3600}else{60},"n":if span>86400{span/3600}else{span/60},"app":self.app})));
    }
    fn sync(&mut self, input: &Value) {
        if input["args"] != self.args {
            self.args = input["args"].clone();
            self.built = None;
            if let Some(tab) = input["args"]["tab"].as_str() {
                self.tab = Tab::parse(tab)
            }
            if let Some(disk) = input["args"]["disk"].as_str() {
                self.disk = disk.into()
            }
            self.section = input["args"]["section"].as_str().unwrap_or("").into();
        }
        self.prefs = input["settings"]["monitor"].clone();
        self.services = input["services"].clone();
        self.catalog = input["catalog"].clone();
        if !self.app.is_empty() && self.requested_app != self.app {
            self.requested_app = self.app.clone();
            self.effect(
                "nativeRead",
                json!({"service":"fw","method":"rules","args":[self.app]}),
            );
        }
        let service = self.service();
        let want = worker::Want {
            processes: self.tab == Tab::Processes,
            connections: self.tab == Tab::Network && self.prefs["connections"] != false,
            geo: self.prefs["geo"] != false,
            voltage: self.tab == Tab::Voltage,
            cooling: self.tab == Tab::Cooling,
            detail: if self.tab == Tab::Processes {
                self.selected
            } else {
                None
            },
        };
        service.update(self.lease.as_ref().unwrap(), want);
        service.history(self.prefs["history"] != false);
        self.snapshot = service.snapshot();
        if let Some(last) = self.snapshot.samples.back() {
            for disk in arr(&last["disk"]["disks"]) {
                if let (Some(name), Some(t), Some(v)) = (
                    disk["name"].as_str(),
                    disk["tempT"].as_u64(),
                    disk["temp"].as_f64(),
                ) {
                    let history = self.disk_history.entry(name.into()).or_default();
                    if history.back().is_none_or(|p| p.0 != t) {
                        history.push_back((t, v));
                        while history.len() > 600 {
                            history.pop_front();
                        }
                    }
                }
            }
        }
        self.error = self.snapshot.result["error"].as_str().map(str::to_owned);
        self.cleared |= self.snapshot.result["cleared"] == true;
        if self.tab == Tab::Voltage {
            if let Some((key, v)) = ui::primary_voltage(&self.snapshot.sensors["voltage"]) {
                if key != self.voltage_key {
                    self.voltage_history.clear();
                    self.voltage_key = key;
                }
                let t = self.snapshot.sensors["t"].as_u64().unwrap_or(0);
                if self.voltage_history.back().is_none_or(|p| p.0 != t) {
                    self.voltage_history.push_back((t, v));
                    while self.voltage_history.len() > 150 {
                        self.voltage_history.pop_front();
                    }
                }
            }
        }
    }
    pub fn request(&mut self, input: &Value) -> Value {
        let cmd = input["cmd"].as_str().unwrap_or("");
        let key = input["key"].as_str().unwrap_or("");
        match cmd {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "close" => {
                self.lease = None;
                self.images.clear();
                self.built = None;
                return Value::Null;
            }
            "sync" => self.sync(input),
            "tab" => {
                self.tab = Tab::parse(key);
                self.query.clear();
                self.section.clear();
                self.frozen = None;
            }
            "showApps" => {
                // "Show applications…" on a dashboard card: the busiest first.
                self.tab = Tab::Processes;
                self.section.clear();
                self.query.clear();
                if ["cpu", "rss"].contains(&key) {
                    self.sort = key.into();
                    self.reverse = true;
                }
            }
            "query" => self.query = key.chars().take(256).collect(),
            "mine" => self.mine = !self.mine,
            "sort" => {
                if self.sort == key {
                    self.reverse = !self.reverse
                } else {
                    self.sort = key.into();
                    self.reverse = true;
                }
            }
            "select" => {
                if let Some((pid, st)) = key
                    .split_once(':')
                    .and_then(|(p, s)| Some((p.parse().ok()?, s.parse().ok()?)))
                {
                    self.selected = Some((pid, st));
                    self.detail_tab = "memory".into();
                }
            }
            "detailTab" => self.detail_tab = key.into(),
            "detailClose" => {
                self.selected = None;
                self.confirming = false;
                self.error = None;
            }
            "confirm" => self.confirming = true,
            "cancel" => self.confirming = false,
            "signal" => {
                if self.confirming && matches!(key, "TERM" | "KILL") {
                    if let Some((pid, st)) = self.selected {
                        self.service().job(worker::Job::Signal(pid, st, key.into()));
                    }
                    self.confirming = false;
                }
            }
            "disk" => self.disk = key.into(),
            "openDisks" => self.effect("openApp", json!({"app":"disks","args":{"disk":self.disk}})),
            "settings" => self.effect("openApp", json!({"app":"settings","args":{"section":key}})),
            "pref" => self.pref(key),
            "clearHistory" => {
                self.service().job(worker::Job::Clear);
                self.cleared = false;
            }
            "history" => {
                self.history_open = !self.history_open;
                if self.history_open {
                    self.query_history();
                }
            }
            "historySpan" => {
                self.history_span = key.parse::<u64>().unwrap_or(3600).clamp(3600, 30 * 86400);
                self.query_history();
            }
            "app" => {
                self.app = key.into();
                if self.history_open {
                    self.query_history();
                }
            }
            "scope" => self.scope = key.into(),
            "protocol" => self.protocol = key.into(),
            "zoom" => {
                self.map_zoom =
                    (self.map_zoom * if key == "out" { 0.8 } else { 1.25 }).clamp(1., 16.);
            }
            "pan" => {
                let step = 30. / self.map_zoom;
                match key {
                    "left" => self.map_lon -= step,
                    "right" => self.map_lon += step,
                    "up" => self.map_lat = (self.map_lat + step).min(90.),
                    "down" => self.map_lat = (self.map_lat - step).max(-90.),
                    _ => {}
                }
            }
            "fanForce" => {
                self.service().job(worker::Job::FanForce(
                    self.snapshot.fan["force"].as_str().unwrap_or("off") == "off",
                ));
            }
            "pause" => {
                self.frozen = if self.frozen.is_some() {
                    None
                } else {
                    Some(self.snapshot.clone())
                }
            }
            "section" => self.section = key.into(),
            "field" => {
                if [
                    "dst", "port", "proto", "action", "dir", "url", "format", "editId",
                ]
                .contains(&key)
                {
                    self.form[key] = json!(input["value"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(2048)
                        .collect::<String>());
                    self.form_error = None;
                }
            }
            "destination" => {
                self.app = input["value"]["app"].as_str().unwrap_or("").into();
                self.form = json!({"dst":input["value"]["dst"],"proto":"","port":"","action":"deny","dir":"out"});
                self.section = "rules".into();
                self.requested_app.clear();
            }
            "editRule" => {
                if let Some(rule) = arr(&self.rules_data).iter().find(|r| {
                    r["id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| r["id"].to_string())
                        == key
                }) {
                    self.form = rule.clone();
                    self.form["editId"] = json!(key);
                }
            }
            "saveRule" => match rules::validate(&self.form, &self.app) {
                Ok(rule) => {
                    let edit = self.form["editId"].as_str().unwrap_or("");
                    let args = if edit.is_empty() {
                        json!([rule])
                    } else {
                        json!([edit, rule])
                    };
                    let method = if edit.is_empty() {
                        "addRule"
                    } else {
                        "updateRule"
                    };
                    self.effect(
                        "nativeCall",
                        json!({"service":"fw","method":method,"args":args}),
                    );
                    self.requested_app.clear();
                    self.form["editId"] = Value::Null;
                }
                Err(error) => self.form_error = Some(error.into()),
            },
            "restriction" => {
                let mut flags =
                    arr(&self.services["fw"]["state"]["restricted"][&self.app]).to_vec();
                if let Some(i) = flags.iter().position(|v| v == key) {
                    flags.remove(i);
                } else if [
                    "no-internet",
                    "no-lan",
                    "no-network",
                    "no-incoming",
                    "no-camera",
                    "no-mic",
                    "no-screen",
                    "no-usb",
                    "no-bluetooth",
                    "no-files",
                    "no-notifications",
                    "no-clipboard",
                ]
                .contains(&key)
                {
                    flags.push(json!(key));
                }
                self.effect(
                    "nativeCall",
                    json!({"service":"fw","method":"setRestricted","args":[self.app,flags]}),
                );
            }
            "addList" => {
                let url = self.form["url"].as_str().unwrap_or("");
                let format = self.form["format"].as_str().unwrap_or("domains");
                if url.starts_with("https://")
                    && url.len() <= 2048
                    && ["domains", "hosts", "ips"].contains(&format)
                {
                    self.effect(
                        "nativeCall",
                        json!({"service":"fw","method":"addList","args":[url,format]}),
                    );
                } else {
                    self.form_error = Some("badUrl".into())
                }
            }
            "rule" => {
                if let Some((method, value)) = key.split_once(':') {
                    let methods = [
                        "setAppRule",
                        "setTrusted",
                        "setRestricted",
                        "setMode",
                        "removeRule",
                        "removeList",
                        "updateLists",
                        "setList",
                    ];
                    if methods.contains(&method) {
                        let args = match method {
                            "setMode" => json!([value]),
                            "updateLists" => json!([true]),
                            "removeRule" | "removeList" => json!([value]),
                            "setAppRule" => json!([
                                self.app,
                                if value == "default" {
                                    Value::Null
                                } else {
                                    json!(value)
                                }
                            ]),
                            "setTrusted" => json!([self.app, value == "true"]),
                            "setList" => {
                                let (id, on) = value.rsplit_once(':').unwrap_or((value, "false"));
                                json!([id, on == "true"])
                            }
                            _ => json!([self.app, value]),
                        };
                        self.effect(
                            "nativeCall",
                            json!({"service":"fw","method":method,"args":args}),
                        );
                    }
                }
            }
            "next" => {
                if let Some(error) = input["result"]["error"].as_str() {
                    self.error = Some(error.into());
                }
                if input["result"]["method"] == "rules" {
                    self.rules_data = input["result"]["value"].clone();
                }
            }
            _ => {}
        }
        if cmd != "sync" {
            // Any command can change what is shown: the next sync rebuilds.
            self.built = None;
            return json!({"effect":self.effects.pop_front(),"error":self.error});
        }
        // The host re-renders for reasons unrelated to Monitor (window list, theme,
        // timers). Without new samples, input or context, reuse the previous tree.
        let shown = self.frozen.as_ref().unwrap_or(&self.snapshot).seq;
        let unchanged = self.built.as_ref().is_some_and(|b| {
            b.seq == shown
                && b.context == input["context"]
                && b.settings == input["settings"]
                && b.services == input["services"]
                && b.catalog == input["catalog"]
        });
        if unchanged {
            return json!({"same":true,"effect":self.effects.pop_front(),"title":{"key":"monitor.title"},"generation":self.generation});
        }
        self.built = Some(Built {
            seq: shown,
            context: input["context"].clone(),
            settings: input["settings"].clone(),
            services: input["services"].clone(),
            catalog: input["catalog"].clone(),
        });
        self.generation = self.generation.wrapping_add(1);
        self.images.clear();
        let tree = ui::tree(self, &input["context"]);
        json!({"tree":tree,"effect":self.effects.pop_front(),"title":{"key":"monitor.title"},"generation":self.generation})
    }
}
