//! Native Settings. The scene and application policy live here; shell effects
//! retain the system's existing authorization and display rollback mechanisms.
mod cooling;
mod displays;
mod hardware;
mod keyboard;
mod locale;
mod network;
mod personal;
mod printers;
mod privacy;
mod system;
#[cfg(test)]
mod tests;
#[cfg(any(test, feature = "host-tests"))]
mod host_fixture;
mod trackpad;
mod ui;
mod users;
mod wallpaper;
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    i18n::I18n,
    NativeApp,
};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::VecDeque};
use ui::*;
thread_local! {static LAST_SECTION:RefCell<String>=RefCell::new("general".into());}
enum Tag {
    Read(u64),
    Watch,
    Events,
    Call(Value),
    Raw(Value),
}
pub struct App {
    jobs: Jobs<Tag>,
    effects: VecDeque<Value>,
    i18n: RefCell<I18n>,
    theme: std::rc::Rc<Value>,
    appearance_catalogs: Option<(&'static Value, &'static Value)>,
    settings: Value,
    observed_settings: Value,
    data: Value,
    state: Value,
    args: Value,
    section: String,
    query: String,
    epoch: u64,
    loaded: bool,
    read_pending: bool,
    read_at: i64,
    event_at: i64,
    events_pending: bool,
    booted: bool,
    now: i64,
    popup: Value,
    last_tree: Value,
    drag: Value,
    region: String,
    home: String,
    assets: String,
    locale_revision: Value,
    privacy: Vec<(String, Box<dyn NativeApp>)>,
    privacy_pending: Option<(u64, usize)>,
    patch_sequence: u64,
    pending_settings: std::collections::BTreeMap<String, (u64, Value)>,
    inflight: bool,
    direct_patch: Option<Vec<(String, u64)>>,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            effects: VecDeque::new(),
            i18n: RefCell::new(I18n::default()),
            theme: std::rc::Rc::new(json!({})),
            appearance_catalogs: None,
            settings: defaults(),
            observed_settings: Value::Null,
            data: json!({}),
            state: json!({"networkPage":"","trackpadTab":"point","busy":""}),
            args: Value::Null,
            section: LAST_SECTION.with(|section| section.borrow().clone()),
            query: String::new(),
            epoch: 0,
            loaded: false,
            read_pending: false,
            read_at: 0,
            event_at: 0,
            events_pending: false,
            booted: false,
            now: 0,
            popup: Value::Null,
            last_tree: Value::Null,
            drag: Value::Null,
            region: String::new(),
            home: String::new(),
            assets: String::new(),
            locale_revision: Value::Null,
            privacy: vec![],
            privacy_pending: None,
            patch_sequence: 0,
            pending_settings: std::collections::BTreeMap::new(),
            inflight: false,
            direct_patch: None,
        }
    }
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool { true }
    fn set_native_appearance(&mut self, theme: std::rc::Rc<Value>) { self.theme = theme; }
    fn set_native_appearance_catalogs(&mut self, wallpapers: &'static Value, accents: &'static Value) {
        self.appearance_catalogs = Some((wallpapers, accents));
    }
    fn blur(&mut self) {
        self.popup = Value::Null;
        self.state["vpnAutofillOpen"] = json!(false);
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        (self.booted
            && (self.jobs.busy()
                || !self.effects.is_empty()
                || self
                    .st("btUntil")
                    .as_i64()
                    .is_some_and(|until| until > self.now)
                || !s(self.st("pendingProfile")).is_empty()))
        .then_some(200.)
    }
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.assets = assets.into();
        *self.i18n.borrow_mut() = I18n::new(assets, home);
        self.home = home.unwrap_or("").into();
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted.then_some(
            if self.jobs.busy() || !self.drag.is_null() || !s(&self.state["gesture"]).is_empty() {
                50.
            } else {
                200.
            },
        )
    }
    fn uses_preferences(&self) -> bool { true }
    fn sync_with_preferences(&mut self, v: &Value, preferences: &Value) -> Value {
        self.request_with_preferences_at(v, preferences, crate::clock::now())
    }
    fn request(&mut self, v: &Value) -> Value {
        self.request_at(v, crate::clock::now())
    }
    fn key(&mut self, key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        if key == "Escape" {
            self.popup = Value::Null;
            for k in [
                "picker",
                "zonePicker",
                "regionPicker",
                "manualTime",
                "vpnImport",
                "vpnAuth",
                "addKeyboard",
            ] {
                self.state[k] = json!(false);
            }
            return Some(self.output());
        }
        None
    }
    fn pixels(&self, id: &str) -> Option<(usize, usize, &[u8])> {
        for (_, app) in &self.privacy {
            if let Some(p) = app.pixels(id) {
                return Some(p);
            }
        }
        None
    }
    fn take_raster(&mut self, id: &str) -> Option<(usize, usize, Vec<u32>)> {
        for (_, app) in &mut self.privacy {
            if let Some(p) = app.take_raster(id) {
                return Some(p);
            }
        }
        None
    }
}
impl App {
    fn tr(&self, key: &str) -> String {
        self.i18n.borrow_mut().caption(&t(key))
    }
    fn caption(&self, value: &Value) -> String {
        self.i18n.borrow_mut().caption(value)
    }
    fn ui(&self) -> Ui {
        Ui {
            theme: self.theme.clone(),
        }
    }
    fn d(&self, key: &str) -> &Value {
        &self.data[key]
    }
    fn st(&self, key: &str) -> &Value {
        get(&self.state, key)
    }
    fn pref(&self, key: &str) -> &Value {
        get(&self.settings, key)
    }
    fn admin(&self) -> bool {
        b(self.d("admin.available"))
    }
    fn standalone(&self) -> bool {
        b(&self.d("platform")["standalone"])
    }
    fn machine_admin(&self) -> bool {
        self.admin() && b(&self.d("system.info")["admin"])
    }
    fn message(&mut self, ok: bool, text: Value) {
        self.state["message"] = json!({"ok":ok,"text":text});
    }
    fn request_at(&mut self, v: &Value, now: i64) -> Value {
        self.request_with_preferences_at(v, &v["settings"], now)
    }
    fn request_with_preferences_at(&mut self, v: &Value, preferences: &Value, now: i64) -> Value {
        self.now = now;
        match s(&v["cmd"]) {
            "icon" => return system::icon(n(&v["size"]).max(1.)),
            "close" => {
                self.cleanup();
                return json!({});
            }
            "sync" => {
                if v["context"]["theme"].is_object() {
                    self.theme = std::rc::Rc::new(v["context"]["theme"].clone());
                }
                if self.locale_revision != v["context"]["localeRevision"] {
                    self.locale_revision = v["context"]["localeRevision"].clone();
                    self.i18n.borrow_mut().invalidate();
                    self.last_tree = Value::Null;
                }
                self.i18n.borrow_mut().decimal =
                    v["context"]["decimal"].as_str().unwrap_or(".").into();
                self.i18n.borrow_mut().group = v["context"]["group"].as_str().unwrap_or(",").into();
                self.region = s(&v["context"]["region"]).into();
                self.state["contentWidth"] =
                    json!((n(&v["context"]["width"]) - 268.).clamp(300., 680.));
                if let Some(lang) = v["lang"].as_str() {
                    self.i18n.borrow_mut().lang = lang.into();
                }
                if preferences.is_object() && UI_PREFERENCE_ROOTS.iter().any(|key| preferences.get(*key) != self.observed_settings.get(*key)) {
                    // The editor keeps observed/pending values for its own controls,
                    // never unrelated apps, window places or session snapshots.
                    self.observed_settings = Value::Object(UI_PREFERENCE_ROOTS.iter().filter_map(|key| preferences.get(*key).map(|v| ((*key).into(), v.clone()))).collect());
                    self.settings = defaults();
                    for (k, value) in self.observed_settings.as_object().unwrap() {
                        if self.settings[k].is_object() && value.is_object() {
                            merge(&mut self.settings[k], value);
                        } else {
                            self.settings[k] = value.clone();
                        }
                    }
                }
                for (root, (_, value)) in &self.pending_settings {
                    self.settings[root] = value.clone();
                }
                self.data["catalog"] = v["catalog"].clone();
                self.data["registry"] = v["registry"].clone();
                self.data["services"] = v["services"].clone();
                self.jobs
                    .bind(v["args"]["__nativeOwner"].as_i64().unwrap_or(0));
                if !self.booted {
                    self.booted = true;
                    self.jobs.enqueue(Tag::Watch, "desktop.watch", json!([]));
                }
                if self.args != v["args"] {
                    self.args = v["args"].clone();
                    let section = s(&self.args["section"]).to_owned();
                    if !section.is_empty() {
                        self.select(&section);
                    }
                    if let Some(page) = self.args["page"].as_str() {
                        let page = page.to_owned();
                        self.set_network_page(&page);
                    }
                }
                self.tick();
                let tree = self.tree();
                let mut out = self.output();
                if tree == self.last_tree {
                    out["same"] = json!(true);
                } else {
                    self.last_tree = tree.clone();
                    out["tree"] = tree;
                }
                return out;
            }
            "next" => {
                self.inflight = false;
                if let Some(entries) = self.direct_patch.take() {
                    for (root, revision) in entries {
                        if self
                            .pending_settings
                            .get(&root)
                            .is_some_and(|(current, _)| *current == revision)
                        {
                            self.pending_settings.remove(&root);
                            if v["result"].as_str().is_some_and(|error| !error.is_empty()) {
                                self.settings[&root] = self
                                    .observed_settings
                                    .get(&root)
                                    .cloned()
                                    .unwrap_or_else(|| defaults()[&root].clone());
                                self.message(false, json!({"error":v["result"]}));
                            }
                        }
                    }
                } else if let Some((epoch, index)) = self.privacy_pending.take() {
                    if epoch == self.epoch {
                        if let Some((_, app)) = self.privacy.get_mut(index) {
                            let out = app.request(v);
                            self.collect_privacy(index, out);
                        }
                    }
                } else {
                    for done in self.jobs.complete(&v["result"], now) {
                        self.completed(done);
                    }
                }
            }
            "do" => {
                if let Ok(mut command) = serde_json::from_str::<Value>(s(&v["key"])) {
                    command["_event"] = v.clone();
                    self.command(&command);
                }
            }
            "field" => self.field(s(&v["key"]), v["value"].clone()),
            "search" => {
                self.query = s(&v["value"]).into();
            }
            "searchSubmit" => {
                if let Some(id) = self
                    .visible_groups()
                    .first()
                    .and_then(|g| g.first())
                    .map(|x| s(&x["id"]).to_owned())
                {
                    self.select(&id);
                }
            }
            "slider" => {
                if let Ok(mut command) = serde_json::from_str::<Value>(s(&v["key"])) {
                    command["frame"] = v["frame"].clone();
                    self.drag = command;
                    self.slider(v);
                }
            }
            "pointerMove" => {
                if self.drag["op"] == "displayDrag" {
                    self.display_drag(v, false);
                } else if self.drag["op"] == "curveDrag" {
                    self.curve_drag(v, false);
                } else {
                    self.slider(v);
                }
            }
            "dragEnd" => {
                if self.drag["op"] == "displayDrag" {
                    self.display_drag(v, true);
                } else if self.drag["op"] == "curveDrag" {
                    self.curve_drag(v, true);
                } else {
                    self.slider(v);
                }
                self.drag = Value::Null;
            }
            "networkAction" => self.network_action(v),
            "privacyAction" => self.privacy_action(v),
            _ => {}
        }
        self.output()
    }
    fn output(&mut self) -> Value {
        let mut out = json!({"capture":!self.drag.is_null()});
        if self.inflight {
            return out;
        }
        if let Some(mut effect) = self.effects.pop_front() {
            if let Some(entries) = effect["__settingsWrite"].as_array() {
                self.direct_patch = Some(
                    entries
                        .iter()
                        .map(|entry| (s(&entry[0]).into(), entry[1].as_u64().unwrap_or(0)))
                        .collect(),
                );
                effect.as_object_mut().unwrap().remove("__settingsWrite");
            }
            if let Some(index) = effect["__settingsPrivacy"].as_u64() {
                self.privacy_pending = Some((self.epoch, index as usize));
                effect.as_object_mut().unwrap().remove("__settingsPrivacy");
            }
            out["effect"] = effect;
            self.inflight = true;
        } else if let Some(effect) = self.jobs.pump(self.now) {
            out["effect"] = effect;
            self.inflight = true;
        }
        out
    }
    fn read(&mut self, requests: Vec<Value>) {
        if self.read_pending {
            return;
        }
        self.read_pending = true;
        self.jobs
            .enqueue(Tag::Read(self.epoch), "desktop.read", json!([requests]));
    }
    fn call(&mut self, name: &str, args: Value, after: Value) {
        self.jobs
            .enqueue(Tag::Call(after), "desktop.call", json!([name, args]));
    }
    fn call_latest(&mut self, name: &str, args: Value, key: String) {
        if self.state["coalescedBusy"][&key] == true {
            self.state["coalescedPending"][&key] = json!({"name":name,"args":args});
            return;
        }
        if !self.state["coalescedBusy"].is_object() {
            self.state["coalescedBusy"] = json!({});
        }
        self.state["coalescedBusy"][&key] = json!(true);
        self.call(name, args, json!({"op":"coalesced","key":key}));
    }
    fn complete_latest(&mut self, after: &Value) {
        let key = s(&after["key"]);
        self.state["coalescedBusy"][key] = json!(false);
        let pending = self.state["coalescedPending"][key].take();
        if pending.is_object() {
            self.call_latest(s(&pending["name"]), pending["args"].clone(), key.into());
        }
    }
    fn admin_run(&mut self, action: &str, arg: String, quiet: bool, after: Value) {
        let quiet = quiet || action.starts_with("user-") || action.starts_with("net-");
        if !s(&self.state["busy"]).is_empty() {
            return;
        }
        if !self.admin() {
            self.message(false, t("settings.admin.unavailable"));
            return;
        }
        self.state["busy"] = json!(action);
        self.state["message"] = Value::Null;
        let mut after = after;
        after["adminAction"] = json!(action);
        self.call("admin.run", json!([action, arg, quiet]), after);
    }
    fn completed(&mut self, done: Completed<Tag>) {
        match done.tag {
            Tag::Read(epoch) => {
                self.read_pending = false;
                if epoch != self.epoch {
                    return;
                }
                if let Ok(value) = done.result {
                    if value.get("interfaceConfig").is_some() {
                        let failed = value["__errors"]["interfaceConfig"].is_string();
                        self.data["interfaceConfigError"] = json!(failed);
                        self.data["interfaceConfigName"] = if failed {
                            Value::Null
                        } else {
                            json!(s(self.st("networkPage")).strip_prefix("if:").unwrap_or(""))
                        };
                    }
                    merge(&mut self.data, &value);
                    self.loaded = true;
                    self.read_at = self.now + 1000;
                    self.reconcile();
                }
            }
            Tag::Events => {
                self.events_pending = false;
                self.event_at = self.now + 200;
                if let Ok(events) = done.result {
                    for event in arr(&events) {
                        self.event(event);
                    }
                }
            }
            Tag::Watch => {}
            Tag::Raw(after) => match done.result {
                Ok(value) => self.after(&after, &value, &json!([])),
                Err(error) => self.message(false, json!(error)),
            },
            Tag::Call(after) => {
                if !after["adminAction"].is_null() {
                    self.state["busy"] = json!("");
                    if after["adminAction"] == "power-profile" {
                        self.state["pendingProfile"] = Value::Null;
                    }
                }
                let result = done
                    .result
                    .unwrap_or_else(|error| json!({"ok":false,"error":error}));
                if result["ok"] == true {
                    self.after(&after, &result["value"], &result["log"]);
                } else {
                    self.failed(&after, &result);
                }
            }
        }
    }
    fn tick(&mut self) {
        if !self.read_pending && self.now >= self.read_at {
            self.read(self.requests());
        }
        if !self.events_pending && self.now >= self.event_at {
            self.events_pending = true;
            self.jobs.enqueue(Tag::Events, "desktop.events", json!([]));
        }
        if self.section == "bluetooth"
            && self.state["btUntil"]
                .as_i64()
                .is_some_and(|until| until > 0 && self.now >= until)
        {
            self.state["btUntil"] = json!(0);
            self.call("bt.discover", json!([false]), Value::Null);
        }
        if self.section == "network" && self.st("networkPage") == "wifi" {
            self.poll_network();
        }
        if self.section == "network"
            && self.st("networkPage") == "vpn"
            && self.now >= self.st("vpnAt").as_i64().unwrap_or(0)
            && s(self.st("busy")).is_empty()
        {
            self.state["vpnAt"] = json!(self.now + 4000);
            self.call(
                "admin.run",
                json!(["net-vpn-status", "", true]),
                json!({"op":"vpnStatus"}),
            );
        }
        if self
            .st("messageUntil")
            .as_i64()
            .is_some_and(|until| until > 0 && self.now >= until)
        {
            self.state["message"] = Value::Null;
            self.state["messageUntil"] = json!(0);
        }
        if self
            .st("nightAt")
            .as_i64()
            .is_some_and(|at| at > 0 && self.now >= at)
        {
            let value = self.state["nightTemp"].take();
            self.state["nightAt"] = json!(0);
            self.patch("night.temp", value);
        }
    }
    fn requests(&self) -> Vec<Value> {
        let mut names = vec![
            "platform",
            "system.info",
            "admin.available",
            "input.devices",
        ];
        names.extend(match self.section.as_str() {
            "general" => vec!["system.stats"],
            "appearance" => vec!["themes.list", "browserFrame.state", "browserFrame.result"],
            "wallpaper" => vec![],
            "dock" => vec![],
            "displays" => vec![
                "display.list",
                "display.brightness",
                "time.zone",
                "video.hardware",
                "video.error",
            ],
            "network" => vec![
                "net.interfaces",
                "net.routes",
                "net.dns",
                "net.vpnList",
                "net.vpnTypes",
            ],
            "bluetooth" => vec!["bt.state"],
            "sound" => vec!["sound.state", "sound.peak"],
            "notifications" => vec!["packages.list", "perms.list"],
            "printers" => vec!["print.hasCups"],
            "clipboard" => vec!["clipboard.history"],
            "language" => vec![
                "languages",
                "locale.available",
                "locale.get",
                "time.zone",
                "time.zones",
                "admin.busy",
            ],
            "datetime" => vec!["time.zone", "time.zones", "timedate.state"],
            "keyboard" => vec![
                "keyboard.list",
                "keyboard.missingDictionary",
                "shortcuts.list",
            ],
            "defaultapps" => vec!["defaults.screenshotLauncher"],
            "power" => vec!["power.state", "power.profiles", "power.buttons"],
            "cooling" => vec!["fan.status", "fan.case"],
            "firmware" => vec!["session.canPower"],
            "privacy" => vec![
                "packages.list",
                "perms.list",
                "privacy.indicators",
                "fw.status",
                "fw.state",
                "fw.poll",
                "fw.lists",
            ],
            "users" => vec!["users.list"],
            "updates" => vec!["pkg.available", "pkg.upgrades", "pkg.lastUpdate"],
            "session" => vec!["session.managed"],
            _ => vec![],
        });
        let mut requests: Vec<_> = names
            .into_iter()
            .map(|name| json!({"key":name,"name":name}))
            .collect();
        if self.section == "network" {
            if let Some(name) = s(self.st("networkPage")).strip_prefix("if:") {
                requests.push(json!({"key":"interfaceConfig","name":"net.config","args":[name]}));
            }
        }
        if self.section == "defaultapps" {
            for id in system::DEFAULT_IDS {
                requests.push(
                    json!({"key":format!("default:{id}"),"name":"defaults.info","args":[id]}),
                );
                requests.push(json!({"key":format!("candidates:{id}"),"name":"defaults.candidates","args":[id,self.d("catalog")]}));
            }
        }
        if self.section == "displays" {
            let tm = crate::clock::local(self.now);
            requests.push(json!({"key":"sun","name":"display.sun","args":[self.d("time.zone"),tm.tm_year+1900,tm.tm_mon+1,tm.tm_mday]}));
        }
        if self.section == "datetime" {
            requests.push(json!({"key":"zoneOffset","name":"time.zoneOffset","args":[self.d("time.zone"),self.now/1000]}));
            if self.st("zonePicker") == true {
                for zone in self.zone_items() {
                    requests.push(json!({"key":format!("zoneOffset:{zone}"),"name":"time.zoneOffset","args":[zone,self.now/1000]}));
                }
            }
        }
        requests
    }
    fn reconcile(&mut self) {
        if self.section == "users" {
            self.reconcile_users();
        }
        if self.section == "bluetooth" {
            let adapter = &self.d("bt.state")["adapter"];
            let identity = if adapter["powered"] == true {
                s(&adapter["path"])
            } else {
                ""
            }
            .to_owned();
            if *self.st("btAdapter") != json!(identity) {
                if !s(self.st("btAdapter")).is_empty() {
                    self.call("bt.discover", json!([false]), Value::Null);
                }
                self.state["btAdapter"] = json!(identity);
                if !identity.is_empty() {
                    self.start_bt_search();
                } else {
                    self.state["btUntil"] = json!(0);
                }
            }
        }
        if self.section == "sound" {
            let state = self.d("sound.state").clone();
            let mic = arr(&state["sources"])
                .iter()
                .filter(|d| d["monitorOf"] == -1)
                .find(|d| d["default"] == true)
                .or_else(|| arr(&state["sources"]).iter().find(|d| d["monitorOf"] == -1));
            let index = mic.map(|d| d["index"].clone()).unwrap_or(json!(-1));
            if self.st("meter") != &index {
                self.state["meter"] = index.clone();
                self.call("sound.meter", json!([index]), Value::Null);
            }
            for (kind, key) in [
                ("sink", "sinks"),
                ("source", "sources"),
                ("input", "inputs"),
            ] {
                for d in arr(&state[key]) {
                    if n(&d["level"]) > 1. {
                        self.call("sound.volume", json!([kind, d["index"], 1]), Value::Null);
                    }
                }
            }
        }
        if self.section == "printers"
            && b(self.d("print.hasCups"))
            && self.now >= self.st("printAt").as_i64().unwrap_or(0)
        {
            self.state["printAt"] = json!(self.now + 3000);
            self.call(
                "print.printers",
                json!([]),
                json!({"op":"data","key":"printers"}),
            );
            self.call(
                "print.jobs",
                json!([]),
                json!({"op":"data","key":"printJobs"}),
            );
            if self.d("found").is_null() {
                self.discover_printers();
            }
        }
        if self.section == "firmware" && self.st("firmwareRequested") != true {
            self.state["firmwareRequested"] = json!(true);
            self.firmware_check();
        }
        if self.section == "datetime" && self.st("timeRequested") != true {
            self.state["timeRequested"] = json!(true);
            self.call("timedate.refresh", json!([]), Value::Null);
        }
        if self.section == "network" && s(self.st("networkPage")).is_empty() {
            let first = arr(self.d("net.interfaces"))
                .iter()
                .find(|i| !network::vpn_interface(i) && i["type"] != "loopback");
            let page = first
                .map(|i| format!("if:{}", s(&i["name"])))
                .unwrap_or_else(|| "wifi".into());
            self.set_network_page(&page);
            self.read_at = 0;
        }
        if self.section == "language" {
            self.find_packs();
        }
        // The initial section may have arrived before platform capabilities.
        if self.standalone() && matches!(self.section.as_str(), "users" | "cooling" | "firmware") {
            self.select("general");
        }
    }
    fn select(&mut self, section: &str) {
        let mut model = crate::settings_model::Model::default();
        model.request(&json!({"cmd":"select","section":self.section}));
        let value = model
            .request(&json!({"cmd":"select","section":section,"standalone":self.standalone()}));
        let next = s(&value);
        if next == self.section {
            return;
        }
        self.leave_section();
        let mut fresh = json!({"trackpadTab":"point","busy":""});
        for key in [
            "networkPage",
            "contentWidth",
            "coalescedBusy",
            "coalescedPending",
        ] {
            if let Some(value) = self.state.get(key) {
                fresh[key] = value.clone();
            }
        }
        self.state = fresh;
        self.section = next.into();
        LAST_SECTION.with(|section| *section.borrow_mut() = self.section.clone());
        self.epoch += 1;
        self.loaded = false;
        self.read_at = 0;
        self.popup = Value::Null;
        self.state["message"] = Value::Null;
        self.state["busy"] = json!("");
        self.drag = Value::Null;
    }
    fn leave_section(&mut self) {
        if self.section == "bluetooth" {
            self.call("bt.discover", json!([false]), Value::Null);
            self.state["btAdapter"] = Value::Null;
            self.state["btUntil"] = json!(0);
        }
        if self.section == "sound" {
            self.call("sound.meter", json!([-1]), Value::Null);
            self.state["meter"] = Value::Null;
        }
        self.close_network();
        self.effects
            .retain(|effect| effect["__settingsPrivacy"].is_null());
        self.privacy.clear();
    }
    fn cleanup(&mut self) {
        self.leave_section();
    }
    fn start_bt_search(&mut self) {
        self.state["btUntil"] = json!(self.now + 40000);
        self.call("bt.discover", json!([true]), Value::Null);
    }
    fn patch(&mut self, path: &str, value: Value) {
        put(&mut self.settings, path, value);
        let root = path.split('.').next().unwrap_or(path);
        let patch = json!({root:self.settings[root].clone()});
        self.patch_many(patch);
    }
    fn patch_many(&mut self, patch: Value) {
        let Some(fields) = patch.as_object() else {
            return;
        };
        if fields.is_empty() {
            return;
        }
        self.patch_sequence += 1;
        let mut entries = vec![];
        for (root, value) in fields {
            self.settings[root] = value.clone();
            self.pending_settings
                .insert(root.into(), (self.patch_sequence, value.clone()));
            entries.push(json!([root, self.patch_sequence]));
        }
        self.effects
            .push_back(json!({"kind":"settings","patch":patch,"__settingsWrite":entries}));
        if fields.contains_key("night") {
            self.call("night.tick", json!([]), Value::Null);
        }
    }
    fn field(&mut self, path: &str, value: Value) {
        match path {
            "newUser.full" => {
                put(&mut self.state, path, value.clone());
                if self.st("nameTouched") != true {
                    self.state["newUser"]["name"] = json!(users::suggest(s(&value)));
                }
            }
            "newUser.name" => {
                self.state["nameTouched"] = json!(true);
                put(&mut self.state, path, json!(s(&value).to_lowercase()));
            }
            "clipboardApp" => put(
                &mut self.state,
                path,
                json!(s(&value)
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c))
                    .take(63)
                    .collect::<String>()),
            ),
            "vpnName" => put(
                &mut self.state,
                path,
                json!(s(&value)
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || "_-".contains(*c))
                    .take(24)
                    .collect::<String>()),
            ),
            p if p.starts_with("vpnForm.") => put(
                &mut self.state,
                path,
                json!(s(&value).replace(['\r', '\n'], "")),
            ),
            _ => put(&mut self.state, path, value),
        }
    }
    fn slider(&mut self, v: &Value) {
        let frame = &self.drag["frame"];
        let width = n(&frame[2]);
        if width <= 0. {
            return;
        }
        let fraction = ((n(&v["x"]) - n(&frame[0])) / width).clamp(0., 1.);
        let min = n(&self.drag["min"]);
        let max = n(&self.drag["max"]);
        let step = self.drag["step"].as_f64().unwrap_or(1.);
        let value = ((min + fraction * (max - min)) / step).round() * step;
        let mut command = self.drag["binding"].clone();
        command["value"] = json!(value);
        self.command(&command);
    }
    fn command(&mut self, c: &Value) -> bool {
        if !c.is_object() {
            return false;
        }
        match s(&c["op"]) {
            "state" => {
                put(&mut self.state, s(&c["path"]), c["value"].clone());
            }
            "patch" => self.patch(s(&c["path"]), c["value"].clone()),
            "select" => self.select(s(&c["value"])),
            "popup" => {
                if self.popup["anchor"] == c["anchor"] {
                    self.popup = Value::Null;
                } else {
                    self.popup = c.clone();
                }
            }
            "pick" => {
                let mut binding = self.popup["binding"].clone();
                binding["value"] = c["value"].clone();
                let changed = self.popup["value"] != c["value"];
                self.popup = Value::Null;
                if changed {
                    self.command(&binding);
                }
            }
            "closePopup" => self.popup = Value::Null,
            "call" => {
                let mut args = c["args"].clone();
                if !args.is_array() {
                    return false;
                }
                if !c["value"].is_null() {
                    args.as_array_mut().unwrap().push(c["value"].clone());
                }
                self.call(
                    s(&c["name"]),
                    args,
                    json!({"op":"reportCall","name":c["name"]}),
                );
            }
            "open" => self
                .effects
                .push_back(json!({"kind":"openApp","app":c["app"],"args":c["args"]})),
            "gesture" => {
                self.state["gesture"] = c["value"].clone();
                self.state["gestureAt"] = json!(self.now);
            }
            _ => {
                return self.personal_command(c)
                    || self.hardware_command(c)
                    || self.network_command(c)
                    || self.users_command(c)
                    || self.system_command(c);
            }
        }
        true
    }
    fn after(&mut self, after: &Value, value: &Value, log: &Value) {
        match s(&after["op"]) {
            "defaultPicked" => {
                if value == false {
                    self.message(false, t("prefs.defaults.failed"));
                }
            }
            "displayApplied" => {
                if value == false {
                    self.message(false, t("prefs.display.failed"));
                }
            }
            "coalesced" => self.complete_latest(after),
            "fanResult" => {
                if !s(value).is_empty() {
                    self.message(false, json!({"error":value}));
                }
            }
            "data" => {
                let key = s(&after["key"]);
                self.data[key] = value.clone();
                if key == "printers" && self.st("printer").is_null() {
                    self.state["printer"] = value["default"]
                        .as_str()
                        .map(|s| json!(s))
                        .unwrap_or_else(|| value["printers"][0]["name"].clone());
                }
                if key == "found" {
                    self.state["searchingPrinters"] = json!(false);
                }
            }
            "reportCall" => {
                if value == false {
                    self.message(false, t("app.invalidRequest"));
                }
                self.read_at = 0;
            }
            _ => {
                self.personal_after(after, value);
                self.hardware_after(after, value);
                self.network_after(after, value, log);
                self.users_after(after, value);
                self.system_after(after, value);
            }
        }
        if !after["success"].is_null() {
            self.message(true, after["success"].clone());
        } else if s(&after["adminAction"]).starts_with("net-")
            && after["adminAction"] != "net-vpn-status"
            && !s(value).is_empty()
        {
            let message = self.i18n.borrow_mut().status(s(value));
            self.message(true, json!(message));
        } else if matches!(s(&after["adminAction"]), "langpack" | "system-locale")
            && !s(value).is_empty()
        {
            self.message(true, value.clone());
        }
        self.read_at = 0;
    }
    fn failed(&mut self, after: &Value, result: &Value) {
        if self.network_failed(after, result) {
            return;
        }
        if after["op"] == "vpnStatus" {
            return;
        }
        if after["op"] == "coalesced" {
            self.complete_latest(after);
        }
        self.message(false, result["error"].clone());
        self.state["errorCode"] = result["code"].clone();
        self.system_failed(after, result);
        self.hardware_failed(after, result);
    }
    fn event(&mut self, event: &Value) {
        self.hardware_event(event);
        self.network_event(event);
    }
    fn visible_groups(&self) -> Vec<Vec<Value>> {
        let mut model = crate::settings_model::Model::default();
        let groups = model.request(&json!({"cmd":"catalog"}));
        let captions: serde_json::Map<String, Value> = arr(&groups)
            .iter()
            .flat_map(arr)
            .map(|x| {
                let id = s(&x["id"]);
                (
                    id.into(),
                    json!(format!(
                        "{} {}",
                        self.tr(&format!("settings.section.{id}")),
                        self.tr(&format!("settings.keywords.{id}"))
                    )),
                )
            })
            .collect();
        let ids=model.request(&json!({"cmd":"groups","query":self.query,"captions":captions,"hasPad":arr(self.d("input.devices")).iter().any(|d|d["kind"]=="touchpad"),"pi5":self.d("system.info")["board"]=="pi5","standalone":self.standalone()}));
        arr(&ids)
            .iter()
            .map(|g| {
                arr(g)
                    .iter()
                    .filter_map(|id| {
                        arr(&groups)
                            .iter()
                            .flat_map(arr)
                            .find(|x| x["id"] == *id)
                            .cloned()
                    })
                    .collect()
            })
            .collect()
    }
    fn tree(&mut self) -> Value {
        let u = self.ui();
        let mut sidebar = vec![];
        for (i, group) in self.visible_groups().into_iter().enumerate() {
            let rows=group.into_iter().map(|x|{let id=s(&x["id"]);let selected=self.section==id;let mut label=u.text(t(&format!("settings.section.{id}")),13.,false);label["style"]["flex"]=json!(1);if selected&&self.theme["selection"]!="indicator"{label["style"]["color"]=json!("#fff");}let mut row=click(view(json!({"height":28,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":10,"borderRadius":6,"backgroundColor":if selected{u.color(if self.theme["selection"]=="indicator"{"hover"}else{"accent"})}else{json!("transparent")}}),vec![system::badge(s(&x["glyph"]),s(&x["color"]),20.),label]),json!({"op":"select","value":id}));row["hoverStyle"]=json!({"backgroundColor":u.color("hover")});row}).collect();
            sidebar.push(view(json!({"gap":1,"marginTop":if i>0{12}else{0}}), rows));
        }
        if sidebar.is_empty() {
            sidebar.push(u.text(t("settings.noResults"), 12., true));
        }
        let info = self.d("system.info");
        let name = info["fullName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(s(&info["user"]));
        let initials = name
            .split_whitespace()
            .take(2)
            .filter_map(|s| s.chars().next())
            .collect::<String>()
            .to_uppercase();
        let user = click(
            view(
                json!({"flexDirection":"row","alignItems":"center","gap":10,"paddingHorizontal":8,"paddingVertical":6,"borderRadius":8,"backgroundColor":if self.section=="users"{u.color("hover")}else{json!("transparent")}}),
                vec![
                    view(
                        json!({"width":36,"height":36,"borderRadius":18,"alignItems":"center","justifyContent":"center","background":"linear-gradient(180deg, #5ac8fa, #0a6cd6)"}),
                        vec![
                            json!({"kind":"text","label":if initials.is_empty(){"?"}else{&initials},"style":{"fontSize":14,"fontWeight":600,"color":"#fff"}}),
                        ],
                    ),
                    view(
                        json!({"flex":1,"minWidth":0,"gap":1}),
                        vec![
                            u.text(name, 13., false),
                            u.text(
                                t(if b(&info["admin"]) {
                                    "settings.user.admin"
                                } else {
                                    "settings.user.standard"
                                }),
                                11.,
                                true,
                            ),
                        ],
                    ),
                ],
            ),
            json!({"op":"select","value":"users"}),
        );
        let body = match self.section.as_str() {
            "appearance" => self.appearance(),
            "wallpaper" => self.wallpaper(),
            "dock" => self.dock(),
            "clipboard" => self.clipboard(),
            "notifications" => self.notifications(),
            "trackpad" => self.trackpad(),
            "bluetooth" => self.bluetooth(),
            "sound" => self.sound(),
            "power" => self.power(),
            "cooling" => self.cooling(),
            "displays" => self.displays(),
            "users" => self.users(),
            "network" => self.network(),
            "general" => self.general(),
            "datetime" => self.datetime(),
            "language" => self.language(),
            "keyboard" => self.keyboard(),
            "defaultapps" => self.defaultapps(),
            "privacy" => self.privacy(),
            "printers" => self.printers(),
            "firmware" => self.firmware(),
            "updates" => self.updates(),
            "session" => self.session(),
            _ => Value::Null,
        };
        let mut body = vec![body];
        if !self.state["message"].is_null() {
            body.push(u.note(
                self.state["message"]["text"].clone(),
                if b(&self.state["message"]["ok"]) {
                    "ok"
                } else {
                    "error"
                },
            ));
        }
        let mut children = vec![
            view(
                json!({"width":220,"backgroundColor":u.color("sidebarBg"),"paddingTop":12,"borderRightWidth":1,"borderColor":u.color("separator")}),
                vec![
                    view(
                        json!({"paddingHorizontal":10,"gap":8}),
                        vec![
                            json!({"kind":"input","key":"settings-search","value":self.query,"placeholder":t("settings.search"),"change":["search"],"submit":["searchSubmit"]}),
                            user,
                        ],
                    ),
                    json!({"kind":"scroll","key":"settings-sidebar","style":{"flex":1,"marginTop":4},"children":[view(json!({"paddingHorizontal":10,"paddingBottom":12}),sidebar)]}),
                ],
            ),
            view(
                json!({"flex":1,"minWidth":0}),
                vec![
                    view(
                        json!({"height":48,"flexDirection":"row","alignItems":"center","paddingHorizontal":24,"borderBottomWidth":1,"borderColor":u.color("separator")}),
                        vec![{
                            let mut label = u.text(
                                t(&format!("settings.section.{}", self.section)),
                                17.,
                                false,
                            );
                            label["style"]["fontWeight"] = json!(600);
                            label
                        }],
                    ),
                    json!({"kind":"scroll","key":format!("page:{}",self.section),"style":{"flex":1},"children":[view(json!({"paddingHorizontal":24,"paddingTop":20,"alignItems":"center"}),vec![view(json!({"width":"100%","maxWidth":680}),vec![stack(body)])])]}),
                ],
            ),
        ];
        if !self.popup.is_null() {
            children.push(self.popup_tree());
        }
        if self.section == "network"
            && self.st("vpnAutofillOpen") == true
            && self.st("vpnAuth").is_object()
        {
            children.push(self.vpn_autofill_tree());
        }
        view(json!({"flex":1,"flexDirection":"row"}), children)
    }
    fn popup_tree(&self) -> Value {
        let u = self.ui();
        let p = &self.popup;
        let mut rows = vec![];
        for option in arr(&p["options"]) {
            if option[0] == "-" {
                rows.push(view(json!({"height":1,"marginVertical":4,"marginHorizontal":6,"backgroundColor":u.color("separator")}),vec![]));
            } else {
                let mut row = click(
                    view(
                        json!({"height":24,"paddingHorizontal":6,"borderRadius":self.theme["menuItemRadius"].as_f64().unwrap_or(5.),"flexDirection":"row","alignItems":"center","gap":6}),
                        vec![
                            u.text(if option[0] == p["value"] { "✓" } else { "" }, 11., false),
                            u.text(option[1].clone(), 12., false),
                        ],
                    ),
                    json!({"op":"pick","value":option[0]}),
                );
                row["hoverStyle"] = json!({"backgroundColor":u.color(if self.theme["menuHighlight"]=="subtle"{"hover"}else{"accent"})});
                row["children"][0]["style"]["width"] = json!(14);
                if self.theme["menuHighlight"] != "subtle" {
                    for child in row["children"].as_array_mut().unwrap() {
                        child["hover"] = json!({"color":"#fff"});
                    }
                }
                if option[2].is_object() {
                    row["children"]
                        .as_array_mut()
                        .unwrap()
                        .insert(1, option[2].clone());
                }
                rows.push(row);
            }
        }
        view(
            json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":1000}),
            vec![
                click(
                    view(
                        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0}),
                        vec![],
                    ),
                    op("closePopup"),
                ),
                json!({"kind":"view","key":"settings-popup","layer":"popover","fitPopover":true,"anchorKey":p["anchor"],"style":{"position":"absolute","top":28,"width":p["width"],"height":(arr(&p["options"]).len()*24+12).min(320),"padding":5,"borderRadius":self.theme["menuRadius"].as_f64().unwrap_or(8.),"backgroundColor":self.theme.get("popupBg").cloned().unwrap_or_else(||json!(if u.dark(){"#2c2c31"}else{"#ffffff"})),"border":format!("1px solid {}",s(&u.color("separator"))),"boxShadow":"0 10px 30px rgba(0,0,0,.35)"},"children":[{"kind":"scroll","style":{"flex":1},"children":rows}]}),
            ],
        )
    }
}
const UI_PREFERENCE_ROOTS: &[&str] = &[
    "mode", "accent", "effects", "reduceMotion", "scrollBars", "nativeAppearance", "filesAppearance",
    "externalWindowStyle", "dockPosition", "dockSize", "dockMagnify", "dockMagnifySize", "dockAutohide", "dockRecents",
    "clipboard", "notifications", "clock", "night", "power", "trackpad", "video",
    "browserTabsInTitlebar", "browserFramePending", "sessionRestore", "sessionBrowsers", "desktopArrows", "shortcutProfile",
    "wallpaper", "wallpaperUser", "keyboard", "keyboards", "keyboardIndex", "language", "region", "menuBarAllDisplays",
];
fn defaults() -> Value {
    json!({"mode":"dark","effects":true,"reduceMotion":false,"scrollBars":"auto","nativeAppearance":"adwaita","externalWindowStyle":false,"dockPosition":"bottom","dockSize":54,"dockMagnify":false,"dockMagnifySize":86,"dockAutohide":false,"dockRecents":false,"clipboard":{"save":true,"max":500,"maxMB":200,"expireDays":30,"ignoreApps":[],"clearOnLogout":false},"notifications":{"dnd":false,"schedule":{"on":false,"from":1320,"to":420},"grouping":"app","apps":{},"seen":[]},"clock":{"hour24":"auto","date":"short","seconds":false},"night":{"schedule":"off","from":1320,"to":420,"temp":3400,"manual":null},"power":{"offAC":600,"offBattery":180,"lockDelay":0,"sleepAC":0,"sleepBattery":900,"lowBattery":true,"menuPercent":true},"trackpad":{"speed":0.5,"tap":true,"tapDrag":true,"natural":true,"secondary":"bottom-right","threeDrag":false,"dwt":true,"scroll":true,"kinetic":true,"pinch":true,"swipe3":true,"swipe4":false},"video":{"mode":"auto","autoLower":false},"browserTabsInTitlebar":true,"sessionRestore":"ask","sessionBrowsers":true,"desktopArrows":true,"shortcutProfile":"macos"})
}

/// Fixture shared with shell scene regression tests.
#[cfg(feature = "host-tests")]
pub fn host_test_fixture() -> (App, Value) {
    let mut app = host_fixture::fixture();
    app.section = "users".into();
    app.state["userPassword"] = json!({"old":"private-old-password","pass":"private-new-password","pass2":"private-new-password"});
    let theme = (*app.theme).clone();
    (app, theme)
}
