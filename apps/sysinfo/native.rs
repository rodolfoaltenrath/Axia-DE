//! Native System Information window. Platform adapters return raw data only;
//! page state, check/benchmark policy, consent and deadlines live here.
#[path = "sharing.rs"]
mod sharing;
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
use std::rc::Rc;

#[derive(Clone)]
enum Tag {
    Snapshot(u64),
    Local(u64),
    Serials(u64),
    Modules,
    Start,
    Poll,
    Cancel,
    ProbeStart,
    ProbePoll,
    BenchReport,
    Export,
    OpenShare,
    Boot(bool),
    Ignore,
}
#[derive(Clone)]
struct Export {
    kind: String,
    attempt: usize,
    value: Value,
}
pub struct App {
    jobs: Jobs<Tag>,
    booted: bool,
    theme: Rc<Value>,
    i18n: I18n,
    home: String,
    tab: String,
    section: String,
    format: String,
    snapshot: Value,
    local: Value,
    window: String,
    serials: Value,
    modules: Value,
    serial_generation: u64,
    serial_busy: bool,
    modules_busy: bool,
    epoch: u64,
    bench: Value,
    saved: Value,
    probe: Value,
    probe_until: i64,
    probe_since: i64,
    probe_pending: bool,
    poll_pending: bool,
    poll_at: i64,
    scene: bool,
    scene_open: bool,
    message: Value,
    export: Option<Export>,
    share: Value,
    share_json: String,
    export_dir: String,
    export_names: Value,
    boot_count: u64,
    width: f64,
    run_times: Value,
}
impl Default for App {
    fn default() -> Self {
        Self {
            jobs: Jobs::default(),
            booted: false,
            theme: Rc::new(Value::Null),
            i18n: I18n::default(),
            home: String::new(),
            tab: "overview".into(),
            section: "computer".into(),
            format: "markdown".into(),
            snapshot: Value::Null,
            local: Value::Null,
            window: String::new(),
            serials: Value::Null,
            modules: Value::Null,
            serial_generation: 0,
            serial_busy: false,
            modules_busy: false,
            epoch: 0,
            bench: Value::Null,
            saved: json!([null, null, null, null]),
            probe: Value::Null,
            probe_until: 0,
            probe_since: 0,
            probe_pending: false,
            poll_pending: false,
            poll_at: 0,
            scene: false,
            scene_open: false,
            message: Value::Null,
            export: None,
            share: Value::Null,
            share_json: String::new(),
            export_dir: String::new(),
            export_names: json!([]),
            boot_count: 5,
            width: 960.,
            run_times: json!({}),
        }
    }
}
fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn key(s: &str) -> Value {
    json!({"key":s})
}
fn model(cmd: &str, args: Value) -> Value {
    crate::request(&json!({"cmd":cmd,"args":args}))
}
fn check(cmd: &str, value: Value) -> Value {
    crate::request(&json!({"cmd":cmd,"value":value}))
}
fn pretty(value: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).unwrap_or_default()
    )
}
fn firmware_date(raw: &str) -> Option<(i32, i32, i32)> {
    let parts: Vec<_> = raw.split(['-', '/']).collect();
    if parts.len() != 3 {
        return None;
    }
    let (y, m, d) = if raw.contains('/') {
        (parts[2], parts[0], parts[1])
    } else {
        (parts[0], parts[1], parts[2])
    };
    if y.len() != 4
        || m.len() != 2
        || d.len() != 2
        || ![y, m, d]
            .iter()
            .all(|s| s.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let (mut y, m, d) = (
        y.parse::<i32>().ok()?,
        m.parse::<i32>().ok()?,
        d.parse::<i32>().ok()?,
    );
    if y < 100 {
        y += 1900;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => return None,
    };
    (d >= 1 && d <= days).then_some((y, m, d))
}
fn camel(s: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in s.chars() {
        if ch == '-' {
            upper = true;
        } else {
            out.extend(
                if upper {
                    ch.to_uppercase().collect::<String>()
                } else {
                    ch.to_string()
                }
                .chars(),
            );
            upper = false;
        }
    }
    out
}
impl NativeApp for App {
    fn capture_protected(&self) -> bool {
        !self.serials.is_null()
    }
    fn uses_native_appearance(&self) -> bool {
        true
    }
    fn set_native_appearance(&mut self, t: Rc<Value>) {
        self.theme = t;
    }
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
        self.home = home.unwrap_or("").into();
    }
    fn refresh_ms(&self) -> Option<f64> {
        if self.jobs.busy() {
            Some(50.)
        } else if self.probe_until > 0 {
            Some(250.)
        } else if self.tab == "perf" && self.running() {
            Some(500.)
        } else {
            None
        }
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, axia_native::clock::now())
    }
    fn key(&mut self, key: &str, _: &str, flags: i32) -> Option<Value> {
        if key == "Escape" && self.tab == "share" {
            self.tab = "report".into();
            Some(json!({}))
        } else if key == "F5" && flags & 30 == 0 {
            self.refresh();
            Some(json!({}))
        } else {
            None
        }
    }
}
impl App {
    fn running(&self) -> bool {
        matches!(text(&self.bench["state"]), "running" | "waiting")
    }
    fn refresh(&mut self) {
        self.epoch += 1;
        self.jobs
            .cancel(|tag| matches!(tag, Tag::Snapshot(_) | Tag::Local(_)));
        self.jobs
            .enqueue(Tag::Snapshot(self.epoch), "sysinfo.snapshot", json!([]));
        self.jobs
            .enqueue(Tag::Local(self.epoch), "sysinfo.local", json!([]));
        self.hide_serials();
        self.message = Value::Null;
    }
    fn hide_serials(&mut self) {
        self.serial_generation += 1;
        self.serials = Value::Null;
        self.serial_busy = false;
        self.jobs.cancel(|tag| matches!(tag, Tag::Serials(_)));
        if self.booted {
            self.jobs
                .enqueue(Tag::Ignore, "sysinfo.hideSerials", json!([]));
        }
    }
    fn set_scene(&mut self, on: bool) {
        if on != self.scene_open {
            self.scene_open = on;
            self.jobs.enqueue(Tag::Ignore, "sysinfo.scene", json!([on]));
        }
    }
    fn schedule(&mut self, now: i64) {
        if self.tab == "perf" && self.running() && !self.poll_pending && now >= self.poll_at {
            self.poll_pending = true;
            self.jobs.enqueue(Tag::Poll, "sysinfo.benchPoll", json!([]));
        }
        if self.probe_until > 0 && now >= self.probe_until {
            self.probe_until = 0;
            self.message = key("sysinfo.probe.error.timeout");
            self.jobs.cancel(|tag| matches!(tag, Tag::ProbePoll));
            self.probe_pending = false;
        } else if self.probe_until > 0 && !self.probe_pending && now >= self.poll_at {
            self.probe_pending = true;
            self.jobs
                .enqueue(Tag::ProbePoll, "sysinfo.probePoll", json!([]));
        }
        let step = text(&self.bench["step"]);
        self.set_scene(
            self.tab == "perf"
                && self.running()
                && self.bench["mode"] == "desktop"
                && (step.starts_with("comp.") || step.starts_with("desk.")),
        );
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        let value = match done.result {
            Ok(v) => v,
            Err(error) => {
                if matches!(done.tag, Tag::Export) && error == "sysinfo.exportExists" {
                    if let Some(export) = self.export.as_mut().filter(|e| e.attempt < 99) {
                        export.attempt += 1;
                        self.write_export();
                        return;
                    }
                }
                match done.tag {
                    Tag::Serials(_) => self.serial_busy = false,
                    Tag::Modules => self.modules_busy = false,
                    Tag::Start => self.bench = json!({"state":"idle"}),
                    Tag::Poll => self.poll_pending = false,
                    Tag::ProbeStart | Tag::ProbePoll => {
                        self.probe_pending = false;
                        self.probe_until = 0;
                    }
                    Tag::Export => self.export = None,
                    _ => {}
                }
                self.message = json!({"error":error});
                return;
            }
        };
        match done.tag {
            Tag::Snapshot(epoch) if epoch == self.epoch => {
                self.bench = value["bench"].clone();
                self.saved = value["saved"].clone();
                self.probe = value["probe"].clone();
                self.snapshot = value;
                if self.tab == "share" {
                    self.prepare_share();
                }
            }
            Tag::Local(epoch) if epoch == self.epoch => self.local = value,
            Tag::Serials(generation) if generation == self.serial_generation => {
                self.serial_busy = false;
                self.serials = model("parseSerialLines", json!([value]));
            }
            Tag::Modules => {
                self.modules_busy = false;
                self.modules = model("parseMemoryLines", json!([value]));
            }
            Tag::Start => {
                if value == true {
                    self.bench = json!({"state":"waiting"});
                    self.poll_at = now;
                } else {
                    self.bench = json!({"state":"idle"});
                    self.message = key("sysinfo.perf.startFailed");
                }
            }
            Tag::Poll => {
                self.poll_pending = false;
                self.bench = value["bench"].clone();
                self.saved = value["saved"].clone();
                self.poll_at = now + 500;
            }
            Tag::Cancel => {
                self.poll_at = now;
                self.jobs.enqueue(Tag::Poll, "sysinfo.benchPoll", json!([]));
                self.poll_pending = true;
            }
            Tag::ProbeStart => {
                if value == true {
                    self.probe_until = now + 10000;
                    self.probe_since = now - 1000;
                    self.poll_at = now;
                } else {
                    self.message = key("sysinfo.probe.error.start");
                    self.probe_until = 0;
                }
            }
            Tag::ProbePoll => {
                self.probe_pending = false;
                self.poll_at = now + 250;
                if value["at"]
                    .as_i64()
                    .is_some_and(|at| at >= self.probe_since)
                {
                    self.probe = value;
                    self.probe_until = 0;
                }
            }
            Tag::BenchReport => self.snapshot["testbench"] = value,
            Tag::Boot(restart) => {
                self.snapshot["bootTest"] = value;
                if restart {
                    self.jobs.enqueue(Tag::Ignore, "sysinfo.restart", json!([]));
                }
            }
            Tag::Export => {
                let kind = self.export.take().map(|e| e.kind).unwrap_or_default();
                self.export_dir = text(&value["dir"]).into();
                self.export_names = value["names"].clone();
                self.message =
                    json!({"key":"sysinfo.report.saved","params":{"name":value["names"][0]}});
                if kind == "share" {
                    self.jobs.enqueue(
                        Tag::OpenShare,
                        "sysinfo.openFile",
                        json!([format!("{}/{}", self.export_dir, text(&value["names"][1]))]),
                    );
                }
            }
            Tag::OpenShare => {
                self.message = json!({"key":if value==true {"sysinfo.share.opened"} else {"sysinfo.share.savedOnly"},"params":{"json":self.export_names[0],"html":self.export_names[1]}})
            }
            _ => {}
        }
    }
    fn report_text(&self) -> String {
        if self.format == "json" {
            pretty(&self.snapshot["report"])
        } else {
            text(&self.snapshot["markdown"]).into()
        }
    }
    fn prepare_share(&mut self) {
        self.share = sharing::build(&self.snapshot["report"], &self.saved[0], &self.saved[2]);
        self.share_json = pretty(&self.share);
        self.tab = "share".into();
        self.message = Value::Null;
    }
    fn write_export(&mut self) {
        let Some(export) = self.export.as_ref() else {
            return;
        };
        let raw_date = if export.kind == "bench" {
            let date = axia_native::clock::local(
                (export.value["at"].as_f64().unwrap_or(0.) * 1000.) as i64,
            );
            format!(
                "{:04}-{:02}-{:02}",
                date.tm_year + 1900,
                date.tm_mon + 1,
                date.tm_mday
            )
        } else if export.kind == "share" {
            text(&export.value["createdAt"]).into()
        } else {
            text(&export.value["date"]).into()
        };
        let date =
            if !raw_date.is_empty() && raw_date.chars().all(|c| c.is_ascii_digit() || c == '-') {
                raw_date
            } else {
                "today".into()
            };
        let suffix = if export.attempt == 1 {
            String::new()
        } else {
            format!("-{}", export.attempt)
        };
        let base = if export.kind == "bench" {
            format!("lordbench-{date}{suffix}")
        } else {
            format!(
                "lordos-{}-{date}{suffix}",
                if export.kind == "share" {
                    "hwreport"
                } else {
                    "report"
                }
            )
        };
        let files = if export.kind == "share" {
            let json_name = format!("{base}.json");
            let html = sharing::page(&export.value, &json_name, &mut self.i18n);
            json!([{"name":json_name,"text":pretty(&export.value)},{"name":format!("{base}.html"),"text":html}])
        } else {
            json!([{"name":format!("{base}.{}",if export.kind=="bench" || export.value["format"]=="json" {"json"} else {"md"}),
            "text":if export.kind=="bench" {pretty(&export.value)} else {text(&export.value["text"]).to_owned()}}])
        };
        self.jobs
            .enqueue(Tag::Export, "sysinfo.save", json!([files]));
    }
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        match text(&input["cmd"]) {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.i18n.decimal = input["context"]["decimal"].as_str().unwrap_or(".").into();
                self.i18n.group = input["context"]["group"].as_str().unwrap_or(",").into();
                if input["context"]["theme"].is_object() {
                    self.theme = Rc::new(input["context"]["theme"].clone());
                }
                self.width = input["context"]["width"].as_f64().unwrap_or(960.);
                if !self.booted {
                    let owner = input["args"]["__nativeOwner"].as_i64().unwrap_or(0);
                    if owner > 0 {
                        self.booted = true;
                        self.jobs.bind(owner);
                        self.window = text(&input["args"]["__nativeWindow"]).into();
                        self.scene = input["args"]["scene"] == true;
                        if !self.scene {
                            if input["args"]["tab"] == "perf" {
                                self.tab = "perf".into();
                            }
                            self.refresh();
                        }
                    }
                }
                if input["args"].get("__nativeBluetooth").is_some() {
                    if !self.local.is_object() {
                        self.local = json!({});
                    }
                    self.local["bluetooth"] = input["args"]["__nativeBluetooth"].clone();
                }
                if !self.scene {
                    self.schedule(now);
                }
                self.run_times = json!({});
                for value in [
                    &self.snapshot["report"]["machine"]["biosDate"],
                    &self.snapshot["report"]["raspberryPi"]["bootloaderDate"],
                ] {
                    if let Some((y, m, d)) = firmware_date(text(value)) {
                        let mut date = axia_native::clock::local(0);
                        date.tm_year = y - 1900;
                        date.tm_mon = m - 1;
                        date.tm_mday = d;
                        let year = if m < 3 { y - 1 } else { y };
                        let offsets = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
                        date.tm_wday = (year + year / 4 - year / 100
                            + year / 400
                            + offsets[(m - 1) as usize]
                            + d)
                            % 7;
                        self.run_times[text(value)] = json!(axia_native::date_format::date(
                            &mut self.i18n,
                            text(&input["context"]["region"]),
                            &date,
                            "long"
                        ));
                    }
                }
                for result in arr(&self.saved)
                    .iter()
                    .chain(std::iter::once(&self.bench["result"]))
                {
                    if let Some(at) = result["at"].as_i64() {
                        let local = axia_native::clock::local(at.saturating_mul(1000));
                        let region = text(&input["context"]["region"]);
                        let hour = text(&input["context"]["hourCycle"]);
                        let date = axia_native::date_format::date(
                            &mut self.i18n,
                            region,
                            &local,
                            "numeric",
                        );
                        let time = axia_native::date_format::time(
                            &mut self.i18n,
                            region,
                            hour,
                            &local,
                            true,
                        );
                        self.run_times[at.to_string()] = json!(format!("{date} {time}"));
                    }
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
            "tab" => {
                let tab = text(&input["key"]);
                if ["overview", "hardware", "compat", "checks", "perf", "report"].contains(&tab) {
                    if tab != self.tab {
                        self.hide_serials();
                    }
                    self.tab = tab.into();
                    self.message = Value::Null;
                    self.set_scene(false);
                    if tab == "perf" {
                        self.poll_at = 0;
                        self.poll_pending = true;
                        self.jobs.enqueue(Tag::Poll, "sysinfo.benchPoll", json!([]));
                    }
                }
            }
            "section" => {
                let section = text(&input["key"]);
                if arr(&model("hardwareSections", json!([self.snapshot["report"]])))
                    .iter()
                    .any(|v| v == section)
                {
                    self.hide_serials();
                    self.section = section.into();
                    self.message = Value::Null;
                }
            }
            "refresh" => self.refresh(),
            "serials"
                if !self.serial_busy
                    && self.serials.is_null()
                    && self.snapshot["canAdmin"] == true =>
            {
                self.serial_generation += 1;
                self.serial_busy = true;
                self.jobs.enqueue(
                    Tag::Serials(self.serial_generation),
                    "sysinfo.serials",
                    json!([self.window]),
                );
            }
            "hideSerials" => self.hide_serials(),
            "modules" if !self.modules_busy && self.snapshot["canAdmin"] == true => {
                self.modules_busy = true;
                self.jobs
                    .enqueue(Tag::Modules, "sysinfo.modules", json!([]));
            }
            "benchStart" if !self.running() => {
                let mode = text(&input["key"]);
                if ["quick", "full", "desktop"].contains(&mode) {
                    self.message = Value::Null;
                    self.bench = json!({"state":"waiting","mode":mode});
                    self.jobs.enqueue(
                        Tag::Start,
                        "sysinfo.benchStart",
                        json!([mode, if mode == "full" { 180 } else { 0 }]),
                    );
                }
            }
            "benchCancel" => self
                .jobs
                .enqueue(Tag::Cancel, "sysinfo.benchCancel", json!([])),
            "probe" if self.probe_until == 0 => {
                self.probe_until = now + 10000;
                self.jobs
                    .enqueue(Tag::ProbeStart, "sysinfo.probeStart", json!([]));
            }
            "testbench" => self.jobs.enqueue(
                Tag::BenchReport,
                "sysinfo.testbench",
                json!([input["key"] == "open"]),
            ),
            "bootCount" => {
                if let Some(n) = input["key"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|n| [3, 5, 10].contains(n))
                {
                    self.boot_count = n;
                }
            }
            "bootStart" if self.snapshot["canPower"] == true => self.jobs.enqueue(
                Tag::Boot(true),
                "sysinfo.bootStart",
                json!([self.boot_count]),
            ),
            "bootCancel" => self
                .jobs
                .enqueue(Tag::Boot(false), "sysinfo.bootCancel", json!([])),
            "restart" if self.snapshot["canPower"] == true => {
                self.jobs.enqueue(Tag::Ignore, "sysinfo.restart", json!([]))
            }
            "format" => {
                if ["markdown", "json"].contains(&text(&input["key"])) {
                    self.format = text(&input["key"]).into();
                    self.message = Value::Null;
                }
            }
            "copy" => {
                self.jobs
                    .enqueue(Tag::Ignore, "clipboard.write", json!([self.report_text()]));
                self.message = key("sysinfo.report.copied");
            }
            "share" => self.prepare_share(),
            "shareBack" => {
                self.tab = "report".into();
                self.message = Value::Null;
            }
            "save" if self.export.is_none() => {
                let kind = if self.tab == "share" {
                    "share"
                } else if self.tab == "perf" {
                    "bench"
                } else {
                    "report"
                };
                let value = if kind == "share" {
                    self.share.clone()
                } else if kind == "bench" {
                    let slot = input["key"]
                        .as_str()
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0)
                        .min(3);
                    let fresh = &self.bench["result"];
                    if fresh.is_object() && (fresh["mode"] == "desktop") == (slot == 2) {
                        fresh.clone()
                    } else {
                        self.saved[slot].clone()
                    }
                } else {
                    json!({"text":self.report_text(),"format":self.format,"date":self.snapshot["report"]["date"]})
                };
                self.export = Some(Export {
                    kind: kind.into(),
                    attempt: 1,
                    value,
                });
                self.write_export();
            }
            "showFiles" if !self.export_dir.is_empty() => self.jobs.enqueue(
                Tag::Ignore,
                "windows.open",
                json!(["files",{"new":true,"args":{"path":self.export_dir}}]),
            ),
            "close" => {
                self.jobs.cancel(|_| true);
                self.serials = Value::Null;
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
