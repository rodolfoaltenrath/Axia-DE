//! Resident preferences per user profile; the SDK is a compatibility mirror.
use axia_state::{Reader, Store};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{c_char, CStr, CString},
};
const SCHEMA: u64 = 3;

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
}
impl Model {
    pub fn reader(&self) -> Reader<Value> {
        self.state.reader()
    }
    pub fn snapshot(&self) -> Value {
        json!({"state":*self.state.read(),"revision":self.state.revision(),"dirty":self.dirty})
    }
    pub fn command(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str() {
            Some("migrate") => return json!({"value":migrate(input["value"].clone())}),
            Some("initialize") => {
                if self.initialized {
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
            Some("saved") => {
                if input["revision"].as_u64() == Some(self.state.revision()) {
                    self.dirty = false;
                }
                json!({"dirty":self.dirty})
            }
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
                self.snapshot()
            }
            _ => json!({"error":"settings.invalidRequest"}),
        }
    }
}
thread_local! {static PROFILES:RefCell<BTreeMap<String,Model>>=RefCell::new(BTreeMap::new());}
pub(crate) fn with<T>(home: &str, f: impl FnOnce(&mut Model) -> T) -> T {
    PROFILES.with(|profiles| f(profiles.borrow_mut().entry(home.into()).or_default()))
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
            (Ok(home), Some(input)) => with(home, |s| s.command(&input)),
            _ => json!({"error":"settings.invalidRequest"}),
        }
    };
    CString::new(reply.to_string()).map_or(std::ptr::null_mut(), CString::into_raw)
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
