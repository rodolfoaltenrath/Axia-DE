//! Resident preferences per user profile; the SDK is a compatibility mirror.
use axia_state::{Reader, Store};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{c_char, CStr, CString},
    time::{Duration, Instant},
};
const SCHEMA: u64 = 3;
const SAVE_AFTER: Duration = Duration::from_millis(300);
const MAX_BYTES: usize = 1024 * 1024;
#[path = "settings_io.rs"]
mod io;

fn defaults() -> Value {
    serde_json::from_str(include_str!("../../../sdk/settings-defaults.json"))
        .expect("bundled settings defaults")
}
fn schema(value: &Value) -> f64 {
    match value {
        Value::Null => 0.,
        Value::Bool(on) => {
            if *on {
                1.
            } else {
                0.
            }
        }
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                0.
            } else {
                s.parse().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}
pub(crate) fn migrate(mut value: Value) -> Value {
    if !value.is_object() {
        value = json!({});
    }
    if schema(&value["schema"]) < 1. {
        value["schema"] = json!(1);
    }
    if schema(&value["schema"]) < 2. {
        value["externalWindowStyle"] = json!(false);
        value["nativeAppearance"] = if !crate::winmodel::truthy(&value["filesAppearance"]) {
            json!("adwaita")
        } else {
            value["filesAppearance"].clone()
        };
        value["schema"] = json!(2);
    }
    if schema(&value["schema"]) < 3. {
        for key in [
            "motion",
            "motionPaths",
            "motionFiles",
            "motionUser",
            "motionRate",
            "remoteDesks",
            "loginWallpaper",
        ] {
            value.as_object_mut().unwrap().remove(key);
        }
        value["schema"] = json!(3);
    }
    value
}
#[derive(Default)]
pub(crate) struct Model {
    state: Store<Value>,
    initialized: bool,
    dirty: bool,
    until: Option<Instant>,
    save_error: Option<&'static str>,
}
impl Model {
    pub fn reader(&self) -> Reader<Value> {
        self.state.reader()
    }
    pub fn snapshot(&self) -> Value {
        json!({"state":*self.state.read(),"revision":self.state.revision(),"dirty":self.dirty,"pending":self.until.is_some(),"saveError":self.save_error})
    }
    pub fn command(&mut self, input: &Value) -> Value {
        self.command_at(input, Instant::now())
    }
    fn command_at(&mut self, input: &Value, now: Instant) -> Value {
        match input["cmd"].as_str() {
            Some("migrate") => return json!({"value":migrate(input["value"].clone())}),
            Some("initialize") => {
                if self.initialized {
                    self.rearm(now);
                    return self.snapshot();
                }
                let mut value = defaults();
                let mut invalid = false;
                match input["text"].as_str().filter(|s| !s.is_empty()) {
                    Some(text) => match serde_json::from_str::<Value>(text) {
                        Ok(raw) if raw.is_object() => {
                            value
                                .as_object_mut()
                                .unwrap()
                                .extend(raw.as_object().unwrap().clone());
                            value = migrate(value);
                        }
                        _ => invalid = true,
                    },
                    None => value["schema"] = json!(SCHEMA),
                }
                self.state.update(|s| {
                    *s = value;
                    ((), true)
                });
                self.initialized = true;
                let mut reply = self.snapshot();
                reply["invalid"] = json!(invalid);
                return reply;
            }
            _ => {}
        }
        if !self.initialized {
            return json!({"error":"settings.notInitialized"});
        }
        match input["cmd"].as_str() {
            Some("snapshot") => self.snapshot(),
            Some("patch" | "replace") => {
                let changed = if input["cmd"] == "patch" {
                    let Some(patch) = input["patch"].as_object() else {
                        return json!({"error":"settings.invalidValue"});
                    };
                    self.state.update(|s| {
                        let changed = patch.iter().any(|(k, v)| s.get(k) != Some(v));
                        if changed {
                            s.as_object_mut().unwrap().extend(patch.clone());
                        }
                        (changed, changed)
                    })
                } else {
                    let next = input["value"].clone();
                    if !next.is_object() {
                        return json!({"error":"settings.invalidValue"});
                    }
                    self.state.update(|s| {
                        if *s == next {
                            (false, false)
                        } else {
                            *s = next;
                            (true, true)
                        }
                    })
                };
                self.dirty |= changed;
                if changed {
                    self.until = Some(now + SAVE_AFTER);
                }
                self.snapshot()
            }
            _ => json!({"error":"settings.invalidRequest"}),
        }
    }
    pub fn rearm(&mut self, now: Instant) {
        if self.dirty && self.until.is_none() {
            self.until = Some(now + SAVE_AFTER);
        }
    }
    pub fn wait_ms(&self, now: Instant) -> Option<f64> {
        self.until
            .map(|at| at.saturating_duration_since(now).as_secs_f64() * 1000. + 0.01)
    }
    pub fn save(&mut self, home: &str, durable: bool) -> Value {
        if !self.initialized {
            return json!({"error":"settings.notInitialized"});
        }
        self.until = None;
        let bytes = serde_json::to_vec_pretty(&*self.state.read()).expect("JSON preferences");
        if bytes.len() > MAX_BYTES {
            self.save_error = Some("settings.tooLarge");
            return json!({"saved":false,"error":"settings.tooLarge"});
        }
        let revision = self.state.revision();
        let saved = io::write(home, &bytes, durable);
        if saved && self.state.revision() == revision {
            self.dirty = false;
        }
        self.save_error = (!saved).then_some("settings.saveFailed");
        json!({"saved":saved,"dirty":self.dirty})
    }
    pub fn flush_due(&mut self, home: &str, now: Instant) {
        if self.until.is_some_and(|until| now >= until) {
            self.save(home, false);
        }
    }
    fn request(&mut self, home: &str, input: &Value) -> Value {
        if input["cmd"] == "save" {
            return self.save(home, input["durable"] == true);
        }
        if input["cmd"] == "initialize" && !self.initialized {
            let bytes = io::load(home);
            let text = bytes.as_deref().map(String::from_utf8_lossy);
            return self.command(&json!({"cmd":"initialize","text":text}));
        }
        self.command(input)
    }
    fn external_saved(&mut self, text: &str) {
        if !serde_json::from_str::<Value>(text).is_ok_and(|v| v.is_object()) {
            return;
        }
        let mut loaded = Self::default();
        loaded.command(&json!({"cmd":"initialize","text":text}));
        let value = loaded.state.read().clone();
        self.state.update(|s| {
            let changed = *s != value;
            *s = value;
            ((), changed)
        });
        self.initialized = true;
        self.dirty = false;
        self.until = None;
        self.save_error = None;
    }
}
thread_local! {static PROFILES:RefCell<BTreeMap<String,Model>>=RefCell::new(BTreeMap::new());}
pub(crate) fn with<T>(home: &str, f: impl FnOnce(&mut Model) -> T) -> T {
    PROFILES.with(|profiles| f(profiles.borrow_mut().entry(home.into()).or_default()))
}
pub(crate) fn tick() {
    let now = Instant::now();
    PROFILES.with(|profiles| {
        for (home, model) in profiles.borrow_mut().iter_mut() {
            model.flush_due(home, now);
        }
    });
}
pub(crate) fn timeout() -> Option<f64> {
    let now = Instant::now();
    PROFILES.with(|profiles| {
        profiles
            .borrow()
            .values()
            .filter_map(|m| m.wait_ms(now))
            .min_by(f64::total_cmp)
    })
}
pub(crate) fn profile(home: &str) -> String {
    std::fs::canonicalize(home)
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "/nonexistent".into())
}
#[no_mangle]
pub unsafe extern "C" fn axia_settings_request(
    home: *const c_char,
    input: *const c_char,
) -> *mut c_char {
    let reply = if home.is_null() || input.is_null() {
        json!({"error":"settings.invalidRequest"})
    } else {
        let input = CStr::from_ptr(input).to_bytes();
        let parsed = (input.len() <= 4 << 20)
            .then(|| serde_json::from_slice::<Value>(input).ok())
            .flatten();
        match (CStr::from_ptr(home).to_str(), parsed) {
            (Ok(home), Some(input)) => with(home, |s| s.request(home, &input)),
            _ => json!({"error":"settings.invalidRequest"}),
        }
    };
    CString::new(reply.to_string()).map_or(std::ptr::null_mut(), CString::into_raw)
}
#[no_mangle]
pub unsafe extern "C" fn axia_settings_written(
    home: *const c_char,
    text: *const c_char,
    len: usize,
) {
    if home.is_null() || text.is_null() || len > MAX_BYTES {
        return;
    }
    if let (Ok(home), Ok(text)) = (
        CStr::from_ptr(home).to_str(),
        std::str::from_utf8(std::slice::from_raw_parts(text.cast::<u8>(), len)),
    ) {
        with(home, |s| s.external_saved(text));
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
