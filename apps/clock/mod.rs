//! Clock native window. The session engine lives independently in crate::clock.
#[cfg(test)]
mod tests;
mod ui;
use crate::{
    clock,
    native_view::{i18n::I18n, NativeApp},
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    ffi::CStr,
};
thread_local! {static LAST_TAB:RefCell<String>=RefCell::new("world".into());}
#[derive(Clone)]
enum Read {
    Zone,
    Zones,
    Offset(String),
}
pub struct Editor {
    pub id: String,
    pub h: i64,
    pub m: i64,
    pub days: Vec<i64>,
    pub label: String,
}
pub struct App {
    pub tab: String,
    pub picking: bool,
    pub editing: Option<Editor>,
    pub query: String,
    pub timer_fields: [i64; 3],
    pub theme: Value,
    pub width: f64,
    pub hover: String,
    pub settings: Value,
    pub lang: String,
    pub decimal: String,
    pub zone: String,
    pub zones: Vec<String>,
    pub offsets: HashMap<String, (i64, String)>,
    i18n: I18n,
    args: Value,
    queued: VecDeque<Read>,
    inflight: Option<Read>,
    loaded: bool,
    sampled: i64,
}
impl Default for App {
    fn default() -> Self {
        Self {
            tab: LAST_TAB.with(|s| s.borrow().clone()),
            picking: false,
            editing: None,
            query: String::new(),
            timer_fields: [0, 5, 0],
            theme: Value::Null,
            width: 560.,
            hover: String::new(),
            settings: json!({}),
            lang: "en".into(),
            decimal: ".".into(),
            zone: "UTC".into(),
            zones: vec!["UTC".into()],
            offsets: HashMap::new(),
            i18n: I18n::default(),
            args: Value::Null,
            queued: VecDeque::new(),
            inflight: None,
            loaded: false,
            sampled: 0,
        }
    }
}
impl NativeApp for App {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
    }
    fn request(&mut self, input: &Value) -> Value {
        App::request(self, input)
    }
    fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
        None
    }
    fn refresh_ms(&self) -> Option<f64> {
        clock::with(|s| match self.tab.as_str() {
            "world" => Some(1000.),
            "alarms" => Some(30000.),
            "timer" if s.timer["state"] == "running" => Some(250.),
            "stopwatch" if s.stopwatch["running"] == true => Some(47.),
            _ => None,
        })
    }
}
impl App {
    fn set_tab(&mut self, tab: &str) {
        if ["world", "alarms", "timer", "stopwatch"].contains(&tab) {
            if self.tab != tab {
                self.timer_fields = [0, 5, 0];
            }
            self.tab = tab.into();
            LAST_TAB.with(|s| *s.borrow_mut() = tab.into());
        }
    }
    fn format_value(&mut self, key: &str) -> Value {
        self.i18n
            .entry(&self.lang, "format", key)
            .or_else(|| self.i18n.entry("en", "format", key))
            .unwrap_or(Value::Null)
    }
    pub fn weekday(&mut self, day: i64) -> String {
        self.format_value("weekdaysShort")[day.rem_euclid(7) as usize]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| day.to_string())
    }
    pub fn time(&mut self, t: &libc::tm) -> String {
        let region = self.settings["region"].as_str().unwrap_or("").to_owned();
        let rlang = if region.is_empty() {
            self.lang.clone()
        } else {
            region.split('_').next().unwrap_or(&self.lang).to_owned()
        };
        let base = self
            .i18n
            .entry(&rlang, "format", "hour12")
            .unwrap_or_else(|| self.format_value("hour12"));
        let own = self
            .i18n
            .entry(&rlang, "format", "regions")
            .unwrap_or(Value::Null);
        let hour12 = match self.settings["clock"]["hour24"].as_str() {
            Some("24") => false,
            Some("12") => true,
            _ => own[&region]["hour12"]
                .as_bool()
                .unwrap_or(base.as_bool().unwrap_or(false)),
        };
        if hour12 {
            let period = self.format_value(if t.tm_hour < 12 { "am" } else { "pm" });
            format!(
                "{}:{:02} {}",
                if t.tm_hour % 12 == 0 {
                    12
                } else {
                    t.tm_hour % 12
                },
                t.tm_min,
                period
                    .as_str()
                    .unwrap_or(if t.tm_hour < 12 { "AM" } else { "PM" })
            )
        } else {
            format!("{:02}:{:02}", t.tm_hour, t.tm_min)
        }
    }
    pub fn i18n_caption(&mut self, value: &Value) -> String {
        self.i18n.lang = self.lang.clone();
        self.i18n.caption(value)
    }
    pub fn days(&mut self, days: &[i64]) -> Value {
        if days.is_empty() {
            return json!({"key":"clock.alarm.once"});
        }
        if days.len() == 7 {
            return json!({"key":"clock.alarm.everyDay"});
        }
        let mut sorted = days.to_vec();
        sorted.sort();
        if sorted == [1, 2, 3, 4, 5] {
            return json!({"key":"clock.alarm.weekdays"});
        }
        if sorted == [0, 6] {
            return json!({"key":"clock.alarm.weekends"});
        }
        json!([1, 2, 3, 4, 5, 6, 0]
            .into_iter()
            .filter(|d| days.contains(d))
            .map(|d| self.weekday(d))
            .collect::<Vec<_>>()
            .join(" "))
    }
    pub fn zone_list(&self) -> Vec<String> {
        let q = self.query.trim().to_lowercase();
        self.zones
            .iter()
            .filter(|z| q.is_empty() || z.to_lowercase().replace('_', " ").contains(&q))
            .take(200)
            .cloned()
            .collect()
    }
    pub fn city(zone: &str) -> String {
        zone.rsplit('/').next().unwrap_or(zone).replace('_', " ")
    }
    pub fn region(zone: &str) -> String {
        zone.rsplit_once('/')
            .map(|(a, _)| a.replace('/', " / ").replace('_', " "))
            .unwrap_or_default()
    }
    pub fn zone_time(&self, zone: &str, now: i64) -> Option<(libc::tm, i64, String)> {
        let (off, abbr) = self.offsets.get(zone)?;
        let sec = (now / 1000 + off) as libc::time_t;
        let mut tm = unsafe { std::mem::zeroed() };
        unsafe { libc::gmtime_r(&sec, &mut tm) };
        Some((tm, *off, abbr.clone()))
    }
    fn effect(&mut self, now: i64) -> Option<Value> {
        if self.inflight.is_some() {
            return None;
        }
        let read = self.queued.pop_front()?;
        let (method, args) = match &read {
            Read::Zone => ("zone", json!([])),
            Read::Zones => ("zones", json!([])),
            Read::Offset(z) => ("zoneOffset", json!([z, now / 1000])),
        };
        self.inflight = Some(read);
        Some(json!({"kind":"nativeRead","service":"time","method":method,"args":args}))
    }
    fn command(&mut self, cmd: &str, extra: Value) -> Value {
        let mut input = extra;
        if !input.is_object() {
            input = json!({});
        }
        input["cmd"] = json!(cmd);
        clock::with(|s| s.native(&input, clock::now()));
        json!({"effect":{"host":true,"kind":"clock"}})
    }
    pub fn request(&mut self, input: &Value) -> Value {
        let now = clock::now();
        let cmd = input["cmd"].as_str().unwrap_or("");
        let k = input["key"].as_str().unwrap_or("");
        match cmd {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                self.settings = input["settings"]
                    .as_object()
                    .map(|_| input["settings"].clone())
                    .unwrap_or(json!({}));
                clock::with(|s| s.sync(&self.settings["clock"]));
                self.lang = input["lang"].as_str().unwrap_or("en").into();
                self.decimal = input["context"]["decimal"].as_str().unwrap_or(".").into();
                self.theme = input["context"]["theme"].clone();
                self.width = input["context"]["width"].as_f64().unwrap_or(560.);
                if input["args"] != self.args {
                    self.args = input["args"].clone();
                    if let Some(tab) = input["args"]["tab"].as_str() {
                        self.set_tab(tab);
                    }
                }
                if !self.loaded {
                    self.loaded = true;
                    self.queued.extend([Read::Zone, Read::Zones]);
                }
                if self.tab == "world"
                    && now - self.sampled >= 1000
                    && self.queued.is_empty()
                    && self.inflight.is_none()
                {
                    self.sampled = now;
                    self.queued.push_back(Read::Offset(self.zone.clone()));
                    let config = clock::with(|s| s.config.clone());
                    for z in config["cities"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .take(100)
                    {
                        if z != self.zone {
                            self.queued.push_back(Read::Offset(z.into()));
                        }
                    }
                }
                let tree = ui::tree(self, now);
                let mut reply = json!({"tree":tree});
                if let Some(e) = self.effect(now) {
                    reply["effect"] = e;
                }
                return reply;
            }
            "next" => {
                let value = &input["result"]["value"];
                match self.inflight.take() {
                    Some(Read::Zone) => {
                        if let Some(v) = value.as_str() {
                            self.zone = v.into();
                        }
                    }
                    Some(Read::Zones) => {
                        if let Some(v) = value.as_array() {
                            self.zones = v
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect();
                        }
                    }
                    Some(Read::Offset(z)) => {
                        if let Some(off) = value["offset"].as_i64() {
                            self.offsets
                                .insert(z, (off, value["abbr"].as_str().unwrap_or("").into()));
                        }
                    }
                    _ => {}
                }
                if let Some(e) = self.effect(now) {
                    return json!({"effect":e});
                }
            }
            "tab" => self.set_tab(k),
            "add" => {
                if self.tab == "world" {
                    self.picking = true;
                    self.query.clear();
                } else if self.tab == "alarms" {
                    self.editing = Some(Editor {
                        id: String::new(),
                        h: 7,
                        m: 0,
                        days: vec![1, 2, 3, 4, 5],
                        label: String::new(),
                    });
                }
            }
            "dismiss" => {
                self.picking = false;
                self.editing = None;
            }
            "query" => {
                self.query = input["value"]
                    .as_str()
                    .unwrap_or(k)
                    .chars()
                    .take(256)
                    .collect()
            }
            "zonePick" | "zoneFirst" => {
                let zone = if cmd == "zoneFirst" {
                    self.zone_list().first().cloned().unwrap_or_default()
                } else {
                    k.into()
                };
                if self.zones.contains(&zone) {
                    let mut cities =
                        clock::with(|s| s.config["cities"].as_array().cloned().unwrap_or_default());
                    if !cities.contains(&json!(zone)) {
                        cities.push(json!(zone));
                    }
                    self.picking = false;
                    self.sampled = 0;
                    return self.command("cities", json!({"cities":cities}));
                }
            }
            "zoneRemove" => {
                let cities = clock::with(|s| {
                    s.config["cities"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|z| z.as_str() != Some(k))
                        .cloned()
                        .collect::<Vec<_>>()
                });
                return self.command("cities", json!({"cities":cities}));
            }
            "alarmEdit" => {
                let alarm = clock::with(|s| {
                    s.config["alarms"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|a| a["id"] == k)
                        .cloned()
                });
                if let Some(a) = alarm {
                    self.editing = Some(Editor {
                        id: k.into(),
                        h: a["h"].as_i64().unwrap_or(7),
                        m: a["m"].as_i64().unwrap_or(0),
                        days: a["days"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_i64)
                            .collect(),
                        label: a["label"].as_str().unwrap_or("").into(),
                    });
                }
            }
            "alarmToggle" => {
                let on = clock::with(|s| {
                    s.config["alarms"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|a| a["id"] == k)
                        .is_some_and(|a| a["on"] == true)
                });
                return self.command("alarmUpdate", json!({"id":k,"patch":{"on":!on}}));
            }
            "label" => {
                if let Some(e) = &mut self.editing {
                    e.label = input["value"]
                        .as_str()
                        .unwrap_or(k)
                        .chars()
                        .take(512)
                        .collect();
                }
            }
            "day" => {
                if let (Some(e), Ok(day)) = (&mut self.editing, k.parse::<i64>()) {
                    if (0..=6).contains(&day) {
                        if e.days.contains(&day) {
                            e.days.retain(|d| *d != day);
                        } else {
                            e.days.push(day);
                        }
                    }
                }
            }
            "step" => {
                if let Some((field, delta)) = k.split_once(':') {
                    if let Ok(delta) = delta.parse::<i64>() {
                        let delta = delta.clamp(-1, 1);
                        if field == "h" || field == "m" {
                            if let Some(e) = &mut self.editing {
                                if field == "h" {
                                    e.h = (e.h + delta).clamp(0, 23);
                                } else {
                                    e.m = (e.m + delta).clamp(0, 59);
                                }
                            }
                        } else if let Ok(i) = field.parse::<usize>() {
                            if i < 3 {
                                self.timer_fields[i] = (self.timer_fields[i] + delta)
                                    .clamp(0, if i == 0 { 23 } else { 59 });
                            }
                        }
                    }
                }
            }
            "alarmSave" => {
                if let Some(e) = self.editing.take() {
                    let alarm = json!({"h":e.h,"m":e.m,"days":e.days,"label":e.label,"on":true});
                    return if e.id.is_empty() {
                        self.command("alarmAdd", json!({"alarm":alarm}))
                    } else {
                        self.command("alarmUpdate", json!({"id":e.id,"patch":alarm}))
                    };
                }
            }
            "alarmDelete" => {
                if let Some(e) = self.editing.take() {
                    if !e.id.is_empty() {
                        return self.command("alarmRemove", json!({"id":e.id}));
                    }
                }
            }
            "preset" => {
                if let Ok(m) = k.parse::<i64>() {
                    if [1, 3, 5, 10, 15, 30].contains(&m) {
                        self.timer_fields = [0, m, 0];
                        return self.command("timerStart", json!({"seconds":m*60}));
                    }
                }
            }
            "timerStart" => {
                let sec =
                    self.timer_fields[0] * 3600 + self.timer_fields[1] * 60 + self.timer_fields[2];
                if sec > 0 {
                    return self.command(cmd, json!({"seconds":sec}));
                }
            }
            "timerPause" | "timerResume" | "timerReset" | "swStart" | "swStop" | "swLap"
            | "swReset" => return self.command(cmd, json!({})),
            "hover" => self.hover = k.into(),
            _ => {}
        }
        Value::Null
    }
}
pub fn local_abbr(t: &libc::tm) -> String {
    if t.tm_zone.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(t.tm_zone) }
            .to_string_lossy()
            .into_owned()
    }
}
