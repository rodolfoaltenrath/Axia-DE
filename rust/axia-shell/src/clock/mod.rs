//! Session-owned clock engine. No window or JavaScript objects are retained.
#[cfg(test)]
mod tests;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    ffi::{c_char, CStr, CString},
};
const MIN: i64 = 60_000;
const LATE: i64 = 10 * MIN;
fn num(v: &Value, k: &str) -> i64 {
    v[k].as_i64().unwrap_or(0)
}
pub fn now() -> i64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut t) };
    t.tv_sec * 1000 + t.tv_nsec / 1_000_000
}
pub fn local(ms: i64) -> libc::tm {
    let sec = ms.div_euclid(1000) as libc::time_t;
    let mut tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&sec, &mut tm) };
    tm
}
fn at_day(now: i64, day: i32, h: i64, m: i64) -> i64 {
    let mut date = local(now);
    date.tm_mday += day;
    date.tm_hour = 12;
    date.tm_min = 0;
    date.tm_sec = 0;
    date.tm_isdst = -1;
    unsafe {
        libc::mktime(&mut date);
    }
    date.tm_hour = h.clamp(0, 23) as i32;
    date.tm_min = m.clamp(0, 59) as i32;
    date.tm_isdst = -1;
    let requested = date;
    let mut chosen = (unsafe { libc::mktime(&mut date) }) * 1000;
    // JavaScript Date chooses the earlier occurrence of an ambiguous local time.
    // Probe both DST interpretations so libc's previous lookup cannot change that choice.
    for dst in [0, 1] {
        let mut candidate = requested;
        candidate.tm_isdst = dst;
        let ms = (unsafe { libc::mktime(&mut candidate) }) * 1000;
        let t = local(ms);
        if (t.tm_year, t.tm_mon, t.tm_mday, t.tm_hour, t.tm_min)
            == (
                requested.tm_year,
                requested.tm_mon,
                requested.tm_mday,
                requested.tm_hour,
                requested.tm_min,
            )
        {
            chosen = chosen.min(ms);
        }
    }
    chosen
}

fn day_ok(a: &Value, ms: i64) -> bool {
    a["days"]
        .as_array()
        .filter(|v| !v.is_empty())
        .is_none_or(|days| days.contains(&json!(local(ms).tm_wday)))
}
pub fn next_fire(a: &Value, now: i64) -> Option<i64> {
    if a["on"] != true {
        return None;
    }
    let snooze = num(a, "snooze");
    if snooze > now {
        return Some(snooze);
    }
    (0..=7)
        .map(|d| at_day(now, d, num(a, "h"), num(a, "m")))
        .find(|ms| *ms > now && day_ok(a, *ms))
}
fn last_due(a: &Value, now: i64) -> Option<i64> {
    if a["on"] != true {
        return None;
    }
    let snooze = num(a, "snooze");
    if snooze != 0 {
        return (snooze <= now).then_some(snooze);
    }
    (0..=7)
        .map(|d| at_day(now, -d, num(a, "h"), num(a, "m")))
        .find(|ms| *ms <= now && day_ok(a, *ms))
}
pub fn timer_left(s: &Value, now: i64) -> i64 {
    match s["state"].as_str() {
        Some("running") => num(s, "endsAt").saturating_sub(now).max(0),
        Some("paused") => num(s, "left"),
        _ => num(s, "duration"),
    }
}
pub fn elapsed(s: &Value, now: i64) -> i64 {
    num(s, "acc")
        + if s["running"] == true {
            now.saturating_sub(num(s, "start"))
        } else {
            0
        }
}
pub fn duration(ms: i64, cents: bool, decimal: &str) -> String {
    let value = ms.max(0);
    let sec = value / 1000;
    let (h, m, s) = (sec / 3600, sec / 60 % 60, sec % 60);
    let base = if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    };
    if cents {
        format!("{base}{decimal}{:02}", value / 10 % 100)
    } else {
        base
    }
}
pub struct Engine {
    pub config: Value,
    observed: Value,
    pub timer: Value,
    pub stopwatch: Value,
    effects: Vec<Value>,
    next_id: u64,
}
impl Default for Engine {
    fn default() -> Self {
        Self {
            config: json!({}),
            observed: Value::Null,
            timer: json!({"state":"idle","duration":0,"endsAt":0,"left":0}),
            stopwatch: json!({"running":false,"start":0,"acc":0,"laps":[]}),
            effects: vec![],
            next_id: now().rem_euclid(100000) as u64,
        }
    }
}
impl Engine {
    pub fn sync(&mut self, config: &Value) {
        if config.is_object() && config != &self.observed {
            self.config = config.clone();
            self.observed = config.clone();
        }
    }
    fn alarms(&self) -> Vec<Value> {
        self.config["alarms"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
    fn save_alarms(&mut self, alarms: Vec<Value>) {
        self.config["alarms"] = json!(alarms);
        self.save();
    }
    fn save(&mut self) {
        self.effects
            .push(json!({"kind":"settings","patch":{"clock":self.config}}));
    }
    fn check(&mut self, now: i64) {
        let mut changed = false;
        let alarms = self
            .alarms()
            .into_iter()
            .map(|mut a| {
                if let Some(due) = last_due(&a, now.saturating_add(1000)) {
                    if due > num(&a, "fired") && now.saturating_sub(due) <= LATE {
                        self.effects
                            .push(json!({"kind":"alarm","alarm":a,"due":due}));
                        changed = true;
                        a["fired"] = json!(due);
                        a["snooze"] = json!(0);
                        if a["days"].as_array().is_none_or(Vec::is_empty) {
                            a["on"] = json!(false);
                        }
                    }
                }
                a
            })
            .collect();
        if changed {
            self.save_alarms(alarms);
        }
        if self.timer["state"] == "running" && num(&self.timer, "endsAt") <= now.saturating_add(250)
        {
            self.timer["state"] = json!("idle");
            self.timer["endsAt"] = json!(0);
            self.timer["left"] = json!(0);
            self.effects.push(json!({"kind":"timer"}));
        }
    }
    pub fn snapshot(&self, now: i64) -> Value {
        let next = self
            .alarms()
            .into_iter()
            .filter_map(|a| next_fire(&a, now).map(|ms| (ms, a)))
            .min_by_key(|(ms, _)| *ms);
        let mut due = next.as_ref().map(|(ms, _)| *ms);
        if self.timer["state"] == "running" {
            let end = num(&self.timer, "endsAt");
            due = Some(due.map_or(end, |v| v.min(end)));
        }
        let wait = due.map(|due| {
            let wait = due.saturating_sub(now);
            wait.clamp(0, if wait < 60 * MIN { MIN } else { LATE })
        });
        json!({"config":self.config,"timer":self.timer,"stopwatch":self.stopwatch,"nextAlarm":next.map(|(at,a)|json!({"at":at,"label":a["label"]})),"wait":wait})
    }
    pub fn command(&mut self, input: &Value, now: i64) -> Value {
        self.sync(&input["config"]);
        let cmd = input["cmd"].as_str().unwrap_or("");
        let mut value = Value::Null;
        match cmd {
            "nextFire"=>return json!({"value":next_fire(&input["alarm"],now)}),
            "timerLeft"=>return json!({"value":timer_left(input.get("state").unwrap_or(&self.timer),now)}),
            "swElapsed"=>return json!({"value":elapsed(input.get("state").unwrap_or(&self.stopwatch),now)}),
            "duration"=>return json!({"value":duration(num(input,"ms"),input["cents"]==true,input["decimal"].as_str().unwrap_or("."))}),
            "sync"=>self.sync(&input["config"]),
            "alarmAdd"=>{let a=&input["alarm"];let id=format!("a{}",self.next_id);self.next_id+=1;
                let mut list=self.alarms();list.push(json!({"id":id,"h":a["h"].as_i64().unwrap_or(7).clamp(0,23),"m":num(a,"m").clamp(0,59),"days":a["days"].as_array().cloned().unwrap_or_default(),"label":a["label"].as_str().unwrap_or(""),"on":true,"snooze":0,"fired":now}));self.save_alarms(list);value=json!(id);},
            "alarmSnooze"=>return self.command(&json!({"cmd":"alarmUpdate","id":input["id"],"patch":{"on":true,"snooze":now.saturating_add(9*MIN)}}),now),
            "alarmUpdate"=>{let list=self.alarms().into_iter().map(|mut a|{if a.is_object() && a["id"].is_string() && a["id"]==input["id"] {if let Some(patch)=input["patch"].as_object(){for(k,v)in patch{a[k]=v.clone();}a["snooze"]=patch.get("snooze").cloned().unwrap_or(json!(0));a["fired"]=json!(now);}}a}).collect();self.save_alarms(list);},
            "alarmRemove"=>{let list=self.alarms().into_iter().filter(|a|a["id"]!=input["id"]).collect();self.save_alarms(list);},
            "cities"=>{self.config["cities"]=input["cities"].clone();self.save();},
            "timerStart"=>{let seconds=input["seconds"].as_f64().filter(|n|n.is_finite()).unwrap_or(1.).round().max(1.).min((i64::MAX/2000) as f64) as i64;let ms=seconds*1000;self.timer=json!({"state":"running","duration":ms,"endsAt":now.saturating_add(ms),"left":ms});},
            "timerPause" if self.timer["state"]=="running"=>{let left=timer_left(&self.timer,now);self.timer["state"]=json!("paused");self.timer["left"]=json!(left);},
            "timerResume" if self.timer["state"]=="paused"=>{self.timer["state"]=json!("running");self.timer["endsAt"]=json!(now.saturating_add(num(&self.timer,"left")));},
            "timerReset"=>{self.timer["state"]=json!("idle");self.timer["endsAt"]=json!(0);self.timer["left"]=json!(0);},
            "swStart" if self.stopwatch["running"]!=true=>{self.stopwatch["running"]=json!(true);self.stopwatch["start"]=json!(now);},
            "swStop" if self.stopwatch["running"]==true=>{let acc=elapsed(&self.stopwatch,now);self.stopwatch["running"]=json!(false);self.stopwatch["acc"]=json!(acc);},
            "swLap"=>{let mut laps=self.stopwatch["laps"].as_array().cloned().unwrap_or_default();laps.insert(0,json!(elapsed(&self.stopwatch,now)));laps.truncate(200);self.stopwatch["laps"]=json!(laps);},
            "swReset"=>self.stopwatch=json!({"running":false,"start":0,"acc":0,"laps":[]}),
            _=>{}
        }
        if !matches!(cmd, "snapshot" | "swStart" | "swStop" | "swLap" | "swReset") {
            self.check(now);
        }
        let mut result = self.snapshot(now);
        result["value"] = value;
        result["effects"] = json!(std::mem::take(&mut self.effects));
        result
    }
    /// Native windows issue commands; the session bridge drains their effects.
    pub fn native(&mut self, input: &Value, now: i64) {
        let reply = self.command(input, now);
        if let Some(e) = reply["effects"].as_array() {
            self.effects.extend(e.iter().cloned());
        }
    }
}
thread_local! {static SESSION:RefCell<Engine>=RefCell::new(Engine::default());}
pub fn with<T>(f: impl FnOnce(&mut Engine) -> T) -> T {
    SESSION.with(|s| f(&mut s.borrow_mut()))
}
#[no_mangle]
pub unsafe extern "C" fn axia_clock_request(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let text = CStr::from_ptr(input).to_bytes();
    if text.len() > 4 << 20 {
        return std::ptr::null_mut();
    }
    let Ok(input) = serde_json::from_slice::<Value>(text) else {
        return std::ptr::null_mut();
    };
    let now = input["now"].as_i64().unwrap_or_else(now);
    CString::new(with(|s| s.command(&input, now)).to_string())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}
