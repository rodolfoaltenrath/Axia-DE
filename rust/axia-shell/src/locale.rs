//! Profile locale for native services; discovery does not depend on SDK state.
use axia_native::i18n::I18n;
use axia_state::{Reader, Store};
use serde_json::{json, Map, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{c_char, CStr, CString},
    io::Read,
    path::Path,
};

#[derive(Clone, Default, PartialEq)]
pub(crate) struct State {
    pub lang: String,
    pub region: String,
    pub dir: &'static str,
    pub revision: u64,
    pub extra: Map<String, Value>,
}
fn lang_value(text: &str) -> Option<String> {
    let mut found = None;
    for line in text.split('\0').next().unwrap_or("").split('\n') {
        let Some(("LANG", value)) = line.split_once('=') else {
            continue;
        };
        let value = value
            .strip_prefix('"')
            .map_or(value, |s| s.split('"').next().unwrap_or(""));
        if value.len() <= 64
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-@".contains(&c))
        {
            found = Some(value.to_owned());
        }
    }
    found
}
fn read_lang(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(8193).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= 8192)
        .then(|| lang_value(&String::from_utf8_lossy(&bytes)))
        .flatten()
}
fn configured(
    user: Option<String>,
    default: Option<String>,
    system: Option<String>,
) -> Option<String> {
    let value = user
        .filter(|s| !s.is_empty())
        .or_else(|| system.or(default).filter(|s| !s.is_empty()))?;
    let lang = value.split(['_', '.', '@']).next().unwrap_or("");
    (!lang.is_empty() && lang != "C").then(|| lang.into())
}
fn configured_language(home: &str) -> Option<String> {
    configured(
        read_lang(&Path::new(home).join(".config/locale.conf")),
        read_lang(Path::new("/etc/default/locale")),
        read_lang(Path::new("/etc/locale.conf")),
    )
}
struct Model {
    settings: Reader<Value>,
    observed: Option<u64>,
    state: Store<State>,
    catalogs: I18n,
    automatic: String,
    generation: u64,
    runtime: Map<String, Value>,
    active: bool,
    applied: Option<(String, &'static str)>,
}
impl Model {
    fn new(home: &str, assets: &str) -> Self {
        let mut catalogs = I18n::new(assets, Some(home));
        let automatic = configured_language(home)
            .filter(|lang| catalogs.has_catalog(lang, "shell"))
            .unwrap_or_else(|| "en".into());
        Self {
            settings: crate::settings::with(home, |s| s.reader()),
            observed: None,
            state: Store::default(),
            catalogs,
            automatic,
            generation: 0,
            runtime: Map::new(),
            active: false,
            applied: None,
        }
    }
    fn sync(&mut self) {
        let revision = self.settings.revision();
        if self.observed == Some(revision) {
            return;
        }
        if self.observed.is_some() {
            if self.settings.read()["language"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            {
                self.runtime.remove("lang");
            }
            self.runtime.remove("region");
        }
        self.observed = Some(revision);
        self.rebuild();
    }
    fn rebuild(&mut self) {
        let settings = self.settings.read();
        let lang = self
            .runtime
            .get("lang")
            .and_then(Value::as_str)
            .or_else(|| settings["language"].as_str().filter(|s| !s.is_empty()))
            .unwrap_or(&self.automatic)
            .to_owned();
        let region = self
            .runtime
            .get("region")
            .and_then(Value::as_str)
            .or_else(|| settings["region"].as_str())
            .unwrap_or("")
            .to_owned();
        let dir = match self.runtime.get("dir").and_then(Value::as_str) {
            Some("rtl") => "rtl",
            Some("ltr") => "ltr",
            _ => {
                if self
                    .catalogs
                    .entry(&lang, "meta", "direction")
                    .as_ref()
                    .and_then(Value::as_str)
                    == Some("rtl")
                {
                    "rtl"
                } else {
                    "ltr"
                }
            }
        };
        drop(settings);
        let extra = self
            .runtime
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "lang" | "region" | "revision"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if self.state.set(State {
            lang,
            region,
            dir,
            revision: self.generation,
            extra,
        }) {
            self.apply();
        }
    }
    fn apply(&mut self) {
        if !self.active {
            return;
        }
        let state = self.state.read();
        let key = (state.lang.clone(), state.dir);
        if self.applied.as_ref() == Some(&key) {
            return;
        }
        host::apply(&state.lang, state.dir == "rtl");
        self.applied = Some(key);
    }
    fn snapshot(&self) -> Value {
        let state = self.state.read();
        let mut value = state.extra.clone();
        value.insert("lang".into(), json!(state.lang));
        value.insert("region".into(), json!(state.region));
        value.insert("revision".into(), json!(state.revision));
        json!({"state":value,"direction":state.dir,"revision":self.state.revision()})
    }
    fn set(&mut self, value: &Value, replace: bool) -> Value {
        self.sync();
        let Some(value) = value.as_object() else {
            return json!({"error":"locale.invalidValue"});
        };
        if ["lang", "region"].iter().any(|key| {
            value
                .get(*key)
                .is_some_and(|v| !v.is_string() && !v.is_null())
        }) {
            return json!({"error":"locale.invalidValue"});
        }
        if replace {
            self.runtime.clear();
        }
        for (key, value) in value {
            if key == "revision" {
                continue;
            }
            if matches!(key.as_str(), "lang" | "region") && value.is_null() {
                self.runtime.remove(key);
            } else {
                self.runtime.insert(key.clone(), value.clone());
            }
        }
        self.rebuild();
        self.snapshot()
    }
    fn reload(&mut self, home: &str, configuration: bool) {
        self.sync();
        self.catalogs.invalidate();
        if configuration {
            self.automatic = configured_language(home)
                .filter(|lang| self.catalogs.has_catalog(lang, "shell"))
                .unwrap_or_else(|| "en".into());
            if self.settings.read()["language"]
                .as_str()
                .is_none_or(|s| s.is_empty())
            {
                self.runtime.remove("lang");
            }
        }
        self.generation = self
            .generation
            .checked_add(1)
            .expect("locale revision exhausted");
        self.rebuild();
    }
}
thread_local! {static PROFILES:RefCell<BTreeMap<(String,String),Model>>=RefCell::new(BTreeMap::new());}
pub(crate) fn reader(home: &str, assets: &str) -> Reader<State> {
    PROFILES.with(|profiles| {
        let mut profiles = profiles.borrow_mut();
        let model = profiles
            .entry((home.into(), assets.into()))
            .or_insert_with(|| Model::new(home, assets));
        model.sync();
        model.state.reader()
    })
}
pub(crate) fn tick() {
    PROFILES.with(|profiles| {
        for model in profiles.borrow_mut().values_mut() {
            model.sync();
        }
    });
}
pub(crate) fn pending() -> bool {
    PROFILES.with(|profiles| {
        profiles
            .borrow()
            .values()
            .any(|m| m.observed != Some(m.settings.revision()))
    })
}
pub(crate) fn snapshot_if_active(home: &str, assets: &str) -> Option<Value> {
    PROFILES.with(|profiles| {
        let mut profiles = profiles.borrow_mut();
        let model = profiles
            .get_mut(&(home.into(), assets.into()))
            .filter(|m| m.active)?;
        model.sync();
        Some(model.snapshot())
    })
}
fn request(home: &str, assets: &str, input: &Value) -> Value {
    PROFILES.with(|profiles| {
        let mut profiles = profiles.borrow_mut();
        if input["cmd"] == "initialize" {
            for model in profiles.values_mut() {
                model.active = false;
            }
        }
        let model = profiles
            .entry((home.into(), assets.into()))
            .or_insert_with(|| Model::new(home, assets));
        model.sync();
        match input["cmd"].as_str() {
            Some("initialize") => {
                model.runtime.clear();
                model.rebuild();
                model.active = true;
                model.applied = None;
                model.apply();
                model.snapshot()
            }
            Some("snapshot") => model.snapshot(),
            Some("patch" | "replace") => model.set(&input["value"], input["cmd"] == "replace"),
            Some("reload") => {
                model.reload(home, false);
                model.snapshot()
            }
            _ => json!({"error":"locale.invalidRequest"}),
        }
    })
}
#[no_mangle]
pub unsafe extern "C" fn axia_locale_request(
    home: *const c_char,
    input: *const c_char,
) -> *mut c_char {
    let result = if home.is_null() || input.is_null() {
        json!({"error":"locale.invalidRequest"})
    } else {
        let bytes = CStr::from_ptr(input).to_bytes();
        match (
            CStr::from_ptr(home).to_str(),
            (bytes.len() <= 1 << 20)
                .then(|| serde_json::from_slice::<Value>(bytes).ok())
                .flatten(),
        ) {
            (Ok(home), Some(input)) => {
                request(home, &crate::native_view::resource_roots().0, &input)
            }
            _ => json!({"error":"locale.invalidRequest"}),
        }
    };
    CString::new(result.to_string()).map_or(std::ptr::null_mut(), CString::into_raw)
}
#[cfg(not(test))]
mod host {
    use super::*;
    extern "C" {
        fn axia_locale_apply(lang: *const c_char, rtl: i32);
    }
    pub fn apply(lang: &str, rtl: bool) {
        let lang = CString::new(lang).unwrap_or_default();
        unsafe {
            axia_locale_apply(lang.as_ptr(), rtl as i32);
        }
    }
}
#[cfg(test)]
mod host {
    use super::*;
    thread_local! {static APPLIED:RefCell<Vec<(String,bool)>>=const {RefCell::new(Vec::new())};}
    pub fn apply(lang: &str, rtl: bool) {
        APPLIED.with(|a| a.borrow_mut().push((lang.into(), rtl)));
    }
    pub fn applied() -> Vec<(String, bool)> {
        APPLIED.with(|a| a.borrow().clone())
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_locale_changed(home: *const c_char, configuration: i32) {
    if home.is_null() {
        return;
    }
    let Ok(home) = CStr::from_ptr(home).to_str() else {
        return;
    };
    PROFILES.with(|profiles| {
        for ((profile, _), model) in profiles.borrow_mut().iter_mut() {
            if profile == home {
                model.reload(home, configuration != 0);
            }
        }
    });
}
#[cfg(test)]
#[path = "locale_tests.rs"]
mod tests;
