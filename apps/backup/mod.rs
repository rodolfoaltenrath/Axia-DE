//! Native backup window. The session service retains scheduling, copying,
//! encryption and notifications; Rust owns all window policy and widgets.
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

enum Tag {
    Watch,
    Events,
    Config,
    Configure,
    Volumes,
    List(u64),
    Ls(u64),
    Path(String),
    Action,
    Header,
    Unlock,
}
#[derive(Clone)]
struct Confirmation {
    kind: String,
    id: String,
    time: i64,
    replace: bool,
}
pub struct App {
    jobs: Jobs<Tag>,
    booted: bool,
    home: String,
    i18n: I18n,
    theme: Value,
    region: String,
    hour: String,
    rtl: bool,
    width: f64,
    status: Value,
    config: Value,
    volumes: Vec<Value>,
    tab: String,
    folder: String,
    exclude: String,
    config_ready: bool,
    saving: bool,
    after_config: Option<String>,
    want_encrypt: bool,
    p1: String,
    p2: String,
    unlock_pending: bool,
    header_error: Value,
    config_error: Value,
    encryption_error: Value,
    browser_error: Value,
    snapshots: Vec<Value>,
    selected: Option<String>,
    rel: String,
    items: Vec<Value>,
    pick: Option<String>,
    replace: bool,
    ask: Option<Confirmation>,
    more: bool,
    catalog_gen: u64,
    browse_gen: u64,
    events_pending: bool,
    events_at: i64,
    volumes_pending: bool,
    volumes_at: i64,
    last_tree: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            booted: false,
            home: String::new(),
            i18n: I18n::default(),
            theme: Value::Null,
            region: String::new(),
            hour: "auto".into(),
            rtl: false,
            width: 980.,
            status: Value::Null,
            config: json!({"dest":"","interval":"hourly","pauseOnBattery":true,"exclude":[]}),
            volumes: vec![],
            tab: String::new(),
            folder: String::new(),
            exclude: String::new(),
            config_ready: false,
            saving: false,
            after_config: None,
            want_encrypt: false,
            p1: String::new(),
            p2: String::new(),
            unlock_pending: false,
            header_error: Value::Null,
            config_error: Value::Null,
            encryption_error: Value::Null,
            browser_error: Value::Null,
            snapshots: vec![],
            selected: None,
            rel: String::new(),
            items: vec![],
            pick: None,
            replace: false,
            ask: None,
            more: false,
            catalog_gen: 0,
            browse_gen: 0,
            events_pending: false,
            events_at: 0,
            volumes_pending: false,
            volumes_at: 0,
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
        self.booted
            .then_some(if self.jobs.busy() { 50. } else { 200. })
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
    fn key(&mut self, key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        if key == "Escape" && (self.ask.is_some() || self.more) {
            self.ask = None;
            self.more = false;
            Some(json!({}))
        } else {
            None
        }
    }
}
fn yes(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.),
        Value::String(s) => !s.is_empty(),
        Value::Null => false,
        _ => true,
    }
}
fn string(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn integer(v: &Value) -> i64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(0.) as i64
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn error(e: String) -> Value {
    json!({"error":e})
}
fn join(a: &str, b: &str) -> String {
    if a.is_empty() {
        b.into()
    } else {
        format!("{a}/{b}")
    }
}
fn entry_name(s: &str) -> bool {
    !s.is_empty() && !s.contains(['/', '\0']) && s != "." && s != ".."
}
impl App {
    fn state(&self) -> &str {
        string(&self.status["state"])
    }
    fn running(&self) -> bool {
        ["running", "paused"].contains(&self.state())
    }
    fn busy(&self) -> bool {
        !self.status.is_null() && !["idle", "nodest"].contains(&self.state())
    }
    fn when(&mut self, seconds: i64, now: i64) -> Value {
        if seconds == 0 {
            return json!("");
        }
        let t = crate::clock::local(seconds.saturating_mul(1000));
        let today = crate::clock::local(now);
        let time = date_format::time(&mut self.i18n, &self.region, &self.hour, &t, false);
        if (t.tm_year, t.tm_yday) == (today.tm_year, today.tm_yday) {
            json!({"key":"backup.todayAt","params":{"time":time}})
        } else {
            json!({"parts":[date_format::date(&mut self.i18n,&self.region,&t,"short")," ",time]})
        }
    }
    fn header_line(&mut self, now: i64) -> Value {
        if self.status.is_null() {
            return key("backup.unavailable");
        }
        if string(&self.status["dest"]).is_empty() {
            return key("backup.noDest");
        }
        match self.state() {
            "running" => {
                json!({"key":"backup.running","params":{"files":{"number":integer(&self.status["progress"]["files"]),"digits":0},"total":{"number":integer(&self.status["progress"]["totalFiles"]),"digits":0}}})
            }
            "paused" => key(if yes(&self.status["battery"]) {
                "backup.pausedBattery"
            } else {
                "backup.paused"
            }),
            "restoring" => key("backup.restoring"),
            "deleting" => key("backup.deleting"),
            "locked" => key("backup.lockedLine"),
            "unlocking" => key("backup.encWorking"),
            _ if self.state() == "nodest" || !yes(&self.status["destAvailable"]) => {
                key("backup.destMissing")
            }
            _ if yes(&self.status["lastOk"]) => {
                json!({"key":"backup.lastOk","params":{"when":self.when(integer(&self.status["lastOk"]),now)}})
            }
            _ => key("backup.never"),
        }
    }
    fn list(&mut self) {
        self.catalog_gen += 1;
        self.jobs.cancel(|t| matches!(t, Tag::List(_)));
        self.jobs
            .enqueue(Tag::List(self.catalog_gen), "backup.list", json!([]));
    }
    fn browse(&mut self) {
        self.browse_gen += 1;
        self.jobs.cancel(|t| matches!(t, Tag::Ls(_)));
        self.items.clear();
        self.pick = None;
        self.browser_error = Value::Null;
        if let Some(id) = &self.selected {
            self.jobs
                .enqueue(Tag::Ls(self.browse_gen), "backup.ls", json!([id, self.rel]));
        }
    }
    fn set_status(&mut self, v: Value) {
        let signature = |v: &Value| json!([v["state"], v["last"]["when"], v["dest"], v["locked"]]);
        let changed = signature(&self.status) != signature(&v);
        if self.status["dest"] != v["dest"] || self.status["locked"] != v["locked"] {
            // Snapshot IDs can repeat on another destination. Neither a cached
            // directory nor an open confirmation may follow that new disk.
            self.browse_gen += 1;
            self.jobs.cancel(|t| matches!(t, Tag::Ls(_)));
            self.items.clear();
            self.pick = None;
            self.snapshots.clear();
            self.selected = None;
            self.rel.clear();
            self.ask = None;
        }
        if yes(&v["unlock"]["done"])
            && yes(&v["unlock"]["ok"])
            && !(yes(&self.status["unlock"]["done"]) && yes(&self.status["unlock"]["ok"]))
        {
            self.p1.clear();
            self.p2.clear();
            self.want_encrypt = false;
            self.encryption_error = Value::Null;
        }
        self.status = v;
        if self.tab.is_empty() {
            self.tab = if string(&self.status["dest"]).is_empty() {
                "settings"
            } else {
                "snapshots"
            }
            .into();
        }
        if changed && self.tab == "snapshots" {
            self.list();
        }
    }
    fn configure(&mut self, config: Value) {
        if self.saving || !self.config_ready {
            return;
        }
        self.saving = true;
        self.config_error = Value::Null;
        self.jobs
            .enqueue(Tag::Configure, "backup.configure", json!([config]));
    }
    fn configure_field(&mut self, k: &str, value: Value) {
        let mut cfg = self.config.clone();
        cfg[k] = value;
        self.configure(cfg);
    }
    fn exclusion_add(&mut self) {
        if self.saving || !self.config_ready {
            return;
        }
        let raw = self.exclude.trim();
        let v = raw.strip_prefix('~').unwrap_or(raw);
        let v = if raw.starts_with('~') {
            v.strip_prefix('/').unwrap_or(v)
        } else {
            v
        };
        let v = v.trim_end_matches('/');
        if !v.is_empty() {
            let mut list = self.config["exclude"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            list.push(json!(v));
            self.configure_field("exclude", json!(list));
            self.exclude.clear();
        }
    }
    fn use_folder(&mut self) {
        if self.saving || self.folder.is_empty() {
            return;
        }
        let p = self.folder.trim();
        let expanded = if let Some(p) = p.strip_prefix('~') {
            format!("{}{p}", self.home)
        } else {
            p.into()
        };
        self.configure_field("dest", json!(expanded));
    }
    fn action(&mut self, method: &'static str, args: Value) {
        self.header_error = Value::Null;
        self.jobs.enqueue(Tag::Header, method, args);
    }
    fn turn_on(&mut self) {
        if self.saving || !self.config_ready || self.unlock_pending || self.state() == "unlocking" {
            return;
        }
        self.encryption_error = Value::Null;
        if self.p1.encode_utf16().count() < 8 {
            self.encryption_error = key("backup.encShort");
            return;
        }
        if self.p1 != self.p2 {
            self.encryption_error = key("backup.encMismatch");
            return;
        }
        if !yes(&self.config["encrypt"]) {
            self.after_config = Some(self.p1.clone());
            self.configure_field("encrypt", json!(true));
        } else {
            self.unlock(true);
        }
    }
    fn unlock(&mut self, create: bool) {
        if self.unlock_pending || self.state() == "unlocking" {
            return;
        }
        self.unlock_pending = true;
        self.encryption_error = Value::Null;
        self.jobs
            .enqueue(Tag::Unlock, "backup.unlock", json!([self.p1, create]));
    }
    fn selected_exists(&self) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|id| self.snapshots.iter().any(|s| s["id"] == *id))
    }
    fn restore(&mut self, rel: String) {
        if self.busy() || !self.selected_exists() {
            return;
        }
        self.browser_error = Value::Null;
        self.jobs.enqueue(Tag::Action,"backup.restore",json!([self.selected,rel,{"replace":self.replace,"suffix":self.i18n.caption(&key("backup.restoredSuffix"))}]));
    }
    fn ask(&mut self, kind: &str) {
        if self.busy() || !self.selected_exists() {
            return;
        }
        if let Some(s) = self
            .snapshots
            .iter()
            .find(|s| Some(string(&s["id"])) == self.selected.as_deref())
        {
            self.ask = Some(Confirmation {
                kind: kind.into(),
                id: string(&s["id"]).into(),
                time: integer(&s["time"]),
                replace: self.replace,
            });
            self.more = false;
        }
    }
    fn confirm(&mut self) {
        if self.busy() {
            return;
        }
        if let Some(ask) = self.ask.take() {
            if !self.snapshots.iter().any(|s| s["id"] == ask.id) {
                return;
            }
            self.browser_error = Value::Null;
            if ask.kind == "all" {
                self.jobs.enqueue(Tag::Action,"backup.restore",json!([ask.id,"",{"replace":ask.replace,"suffix":self.i18n.caption(&key("backup.restoredSuffix"))}]));
            } else {
                self.jobs
                    .enqueue(Tag::Action, "backup.remove", json!([ask.id]));
            }
        }
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        let Completed { tag, result } = done;
        match tag {
            Tag::Watch => self.set_status(result.unwrap_or(Value::Null)),
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + 200;
                if let Ok(v) = result {
                    for event in v.as_array().into_iter().flatten() {
                        if event["kind"] == "backup" {
                            self.set_status(event["status"].clone());
                        }
                    }
                }
            }
            Tag::Config => {
                self.config_ready = true;
                match result {
                    Ok(v) if v.is_object() => self.config = v,
                    Err(e) => self.config_error = error(e),
                    _ => {}
                }
            }
            Tag::Configure => {
                self.saving = false;
                match result {
                    Ok(v) => {
                        if v.is_object() {
                            self.config = v;
                        }
                        if let Some(pass) = self.after_config.take() {
                            self.unlock_pending = true;
                            self.jobs
                                .enqueue(Tag::Unlock, "backup.unlock", json!([pass, true]));
                        }
                    }
                    Err(e) => {
                        self.after_config = None;
                        self.config_error = error(e);
                    }
                }
            }
            Tag::Volumes => {
                self.volumes_pending = false;
                self.volumes_at = now + 5000;
                if let Ok(v) = result {
                    self.volumes = v.as_array().cloned().unwrap_or_default();
                }
            }
            Tag::List(g) if g == self.catalog_gen => match result {
                Ok(v) => {
                    self.snapshots = v
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|s| s["id"].is_string())
                        .cloned()
                        .collect();
                    self.snapshots.reverse();
                    let previous = self.selected.clone();
                    if !self.selected_exists() {
                        self.selected = self.snapshots.first().map(|s| string(&s["id"]).into());
                        self.rel.clear();
                    }
                    if previous != self.selected || self.items.is_empty() {
                        self.browse();
                    }
                }
                Err(e) => {
                    self.browser_error = error(e);
                }
            },
            Tag::Ls(g) if g == self.browse_gen => match result {
                Ok(v) => {
                    self.items = v
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|e| e["name"].as_str().is_some_and(entry_name))
                        .cloned()
                        .collect();
                    self.browser_error = Value::Null;
                }
                Err(e) => {
                    self.items.clear();
                    self.browser_error = error(e);
                }
            },
            Tag::Path(rel) => {
                if let Ok(v) = result {
                    if let Some(path) = v.as_str().filter(|s| !s.is_empty()) {
                        let path = if rel.is_empty() {
                            path.into()
                        } else {
                            join(path, &rel)
                        };
                        self.jobs.enqueue(
                            Tag::Action,
                            "windows.open",
                            json!(["files",{"new":true,"args":{"path":path}}]),
                        );
                    }
                }
            }
            Tag::Action => {
                if let Err(e) = result {
                    self.browser_error = error(e);
                }
            }
            Tag::Header => {
                if let Err(e) = result {
                    self.header_error = error(e);
                }
            }
            Tag::Unlock => {
                self.unlock_pending = false;
                if let Err(e) = result {
                    self.encryption_error = error(e);
                }
            }
            _ => {}
        }
    }
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        let cmd = input["cmd"].as_str().unwrap_or("");
        let arg = string(&input["key"]);
        // A confirmation captures its snapshot/options and blocks background actions.
        if self.ask.is_some() && !matches!(cmd, "sync" | "next" | "confirm" | "dismiss" | "close") {
            return json!({});
        }
        match cmd {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.i18n.decimal = input["context"]["decimal"].as_str().unwrap_or(".").into();
                self.i18n.group = input["context"]["group"].as_str().unwrap_or(",").into();
                self.theme = input["context"]["theme"].clone();
                self.width = input["context"]["width"]
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .unwrap_or(980.);
                self.region = input["settings"]["region"].as_str().unwrap_or("").into();
                self.hour = input["settings"]["clock"]["hour24"]
                    .as_str()
                    .unwrap_or("auto")
                    .into();
                let lang = self.i18n.lang.clone();
                self.rtl = self.i18n.entry(&lang, "meta", "direction").or_else(|| {
                    self.i18n
                        .entry(lang.split('-').next().unwrap_or(&lang), "meta", "direction")
                }) == Some(json!("rtl"));
                if !self.booted {
                    if let Some(owner) =
                        input["args"]["__nativeOwner"].as_i64().filter(|id| *id > 0)
                    {
                        self.jobs.bind(owner);
                        self.booted = true;
                        self.jobs.enqueue(Tag::Watch, "backup.watch", json!([]));
                        self.jobs.enqueue(Tag::Config, "backup.config", json!([]));
                    }
                }
                if self.booted && !self.events_pending && now >= self.events_at {
                    self.events_pending = true;
                    self.jobs.enqueue(Tag::Events, "backup.events", json!([]));
                }
                if self.booted
                    && self.tab == "settings"
                    && !self.volumes_pending
                    && now >= self.volumes_at
                {
                    self.volumes_pending = true;
                    self.jobs.enqueue(Tag::Volumes, "fs.volumes", json!([]));
                }
                let tree = ui::tree(self, now);
                let mut reply = if tree == self.last_tree {
                    json!({"same":true})
                } else {
                    self.last_tree = tree.clone();
                    json!({"tree":tree})
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
            "tab" => {
                if ["settings", "snapshots"].contains(&arg) {
                    self.tab = arg.into();
                    self.more = false;
                    if arg == "snapshots" {
                        self.list();
                    }
                }
            }
            "run" => self.action("backup.run", json!([])),
            "pause" => self.action("backup.pause", json!([!yes(&self.status["userPause"])])),
            "cancel" => self.action("backup.cancel", json!([])),
            "folder" => self.folder = string(&input["value"]).into(),
            "exclude" => self.exclude = string(&input["value"]).into(),
            "useFolder" => self.use_folder(),
            "addExclude" => self.exclusion_add(),
            "volume" => {
                if self
                    .volumes
                    .iter()
                    .any(|v| v["path"] == arg && arg != "/" && arg != self.home)
                {
                    self.configure_field("dest", json!(arg));
                }
            }
            "stopUsing" => self.configure_field("dest", json!("")),
            "interval" => {
                if ["hourly", "daily", "weekly", "manual"].contains(&arg) {
                    self.configure_field("interval", json!(arg));
                }
            }
            "battery" => self.configure_field(
                "pauseOnBattery",
                json!(self.config["pauseOnBattery"] == false),
            ),
            "removeExclude" => {
                let list: Vec<_> = self.config["exclude"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|v| **v != json!(arg))
                    .cloned()
                    .collect();
                self.configure_field("exclude", json!(list));
            }
            "wantEncrypt" => {
                if string(&self.config["dest"]).is_empty() {
                    self.encryption_error = key("backup.noDest");
                } else {
                    self.want_encrypt = true;
                }
            }
            "p1" => self.p1 = string(&input["value"]).into(),
            "p2" => self.p2 = string(&input["value"]).into(),
            "turnOn" => self.turn_on(),
            "noEncrypt" => {
                if !self.saving {
                    self.want_encrypt = false;
                    self.p1.clear();
                    self.p2.clear();
                    self.encryption_error = Value::Null;
                    if yes(&self.config["encrypt"]) {
                        self.configure_field("encrypt", json!(false));
                    }
                }
            }
            "unlock" => self.unlock(false),
            "lock" => {
                self.encryption_error = Value::Null;
                self.jobs.enqueue(Tag::Unlock, "backup.lock", json!([]));
            }
            "snapshot" => {
                if self.snapshots.iter().any(|s| s["id"] == arg) {
                    self.selected = Some(arg.into());
                    self.rel.clear();
                    self.browse();
                }
            }
            "pick" => {
                if self.items.iter().any(|i| i["name"] == arg) {
                    self.pick = Some(arg.into());
                }
            }
            "enter" => {
                if self
                    .items
                    .iter()
                    .any(|i| i["name"] == arg && yes(&i["dir"]))
                {
                    self.rel = join(&self.rel, arg);
                    self.browse();
                }
            }
            "crumb" => {
                let crumbs: Vec<_> = self.rel.split('/').filter(|s| !s.is_empty()).collect();
                if let Some(index) = input["key"].as_u64().filter(|i| *i <= crumbs.len() as u64) {
                    self.rel = crumbs[..index as usize].join("/");
                    self.browse();
                }
            }
            "replace" => self.replace = !self.replace,
            "restore" => {
                if let Some(pick) = &self.pick {
                    self.restore(join(&self.rel, pick));
                } else if !self.rel.is_empty() {
                    self.restore(self.rel.clone());
                }
            }
            "askAll" => self.ask("all"),
            "askDelete" => self.ask("delete"),
            "confirm" => self.confirm(),
            "dismiss" => {
                self.ask = None;
                self.more = false;
            }
            "more" => self.more = !self.more,
            "openFiles" => {
                if let Some(id) = &self.selected {
                    self.jobs
                        .enqueue(Tag::Path(self.rel.clone()), "backup.path", json!([id]));
                }
            }
            "close" => {
                self.p1.clear();
                self.p2.clear();
                self.after_config = None;
            }
            _ => {}
        }
        self.jobs
            .pump(now)
            .map(|effect| json!({"effect":effect}))
            .unwrap_or_else(|| json!({}))
    }
}
