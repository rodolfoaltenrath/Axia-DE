//! Profile locale for native services; discovery does not depend on SDK state.
use axia_native::i18n::I18n;
use axia_state::{Reader, Store};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{c_char, CStr},
    io::Read,
    path::Path,
};

#[derive(Clone, Default, PartialEq)]
pub(crate) struct State {
    pub lang: String,
    pub region: String,
    pub dir: &'static str,
    pub revision: u64,
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
        }
    }
    fn sync(&mut self) {
        let revision = self.settings.revision();
        if self.observed == Some(revision) {
            return;
        }
        let settings = self.settings.read();
        let lang = settings["language"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.automatic)
            .to_owned();
        let region = settings["region"].as_str().unwrap_or("").to_owned();
        let dir = if self
            .catalogs
            .entry(&lang, "meta", "direction")
            .as_ref()
            .and_then(Value::as_str)
            == Some("rtl")
        {
            "rtl"
        } else {
            "ltr"
        };
        self.state.set(State {
            lang,
            region,
            dir,
            revision: self.generation,
        });
        self.observed = Some(revision);
    }
    fn reload(&mut self, home: &str, configuration: bool) {
        self.catalogs.invalidate();
        if configuration {
            self.automatic = configured_language(home)
                .filter(|lang| self.catalogs.has_catalog(lang, "shell"))
                .unwrap_or_else(|| "en".into());
        }
        self.generation = self
            .generation
            .checked_add(1)
            .expect("locale revision exhausted");
        self.observed = None;
        self.sync();
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
