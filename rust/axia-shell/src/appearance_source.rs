//! Shared, revisioned native app appearance derived from the authoritative profile.
use axia_state::{Reader, Store, Subscription};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap, rc::{Rc, Weak}};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Key {
    mode: String, accent: String, effects: Option<Value>, native: bool,
    scroll_bars: String, reduce_motion: bool,
}
impl Key {
    pub(crate) fn from_profile(profile: &Value) -> Self {
        Self {
            mode: profile["mode"].as_str().unwrap_or("dark").into(),
            accent: profile["accent"].as_str().unwrap_or("azul").into(),
            effects: profile.get("effects").cloned(),
            native: profile["nativeAppearance"] != "axia",
            scroll_bars: profile["scrollBars"].as_str().filter(|s| !s.is_empty()).unwrap_or("auto").into(),
            reduce_motion: crate::native_style::shell_theme::truthy(&profile["reduceMotion"]),
        }
    }
    pub(crate) fn colors(&self) -> Value {
        let mut value = json!({"mode":"dark","accent":"#0a84ff","text":"#f5f5f7","textDim":"rgba(235,235,245,0.55)",
            "windowBg":"#1f1f23","sidebarBg":"#26262b","separator":"rgba(255,255,255,0.08)",
            "hover":"rgba(255,255,255,0.07)","control":"#3a3a40","inputBg":"#2c2c31","radius":8,
            "controlRadius":7,"controlRadiusSmall":6,"groupRadius":10,"menuRadius":8,"menuItemRadius":5,
            "menuHighlight":"accent","selection":"fill","inputFocus":"ring","buttonBorder":false,"popupBg":null,
            "scrollBars":self.scroll_bars,"reduceMotion":self.reduce_motion});
        let mut source = json!({"mode":self.mode,"accent":self.accent});
        if let Some(effects) = &self.effects { source["effects"] = effects.clone(); }
        let palette = crate::native_style::shell_theme::tokens(&source);
        value.as_object_mut().unwrap().extend(palette.as_object().unwrap().iter()
            .filter(|(key,_)| key.as_str() != "accents").map(|(key,value)| (key.clone(),value.clone())));
        if self.native {
            value["nativeAppearance"] = json!("adwaita");
            crate::native_style::files_theme(&value)
        } else { value }
    }
}


struct Model {
    profile: Reader<Value>,
    changes: Subscription,
    key: Key,
    colors: Store<Rc<Value>>,
}
impl Model {
    fn new(profile: Reader<Value>) -> Self {
        let changes = profile.subscribe();
        let key = Key::from_profile(&profile.read());
        let colors = Store::new(Rc::new(key.colors()));
        Self { profile, changes, key, colors }
    }
    fn sync(&mut self) {
        if self.changes.take().is_none() { return; }
        let key = Key::from_profile(&self.profile.read());
        if self.key != key {
            self.colors.set(Rc::new(key.colors()));
            self.key = key;
        }
    }
}
thread_local! { static PROFILES: RefCell<BTreeMap<String,Weak<RefCell<Model>>>> = const { RefCell::new(BTreeMap::new()) }; }

pub(crate) struct Source {
    model: Rc<RefCell<Model>>,
    colors: Reader<Rc<Value>>,
    changes: Subscription,
}
impl Source {
    pub(crate) fn new(home: &str) -> Self {
        let home = crate::settings::profile(home);
        let model = PROFILES.with(|profiles| {
            let mut profiles = profiles.borrow_mut();
            profiles.retain(|_, owner| owner.strong_count() != 0);
            if let Some(model) = profiles.get(&home).and_then(Weak::upgrade) { return model; }
            let profile = crate::settings::with(&home, |profile| profile.reader());
            let model = Rc::new(RefCell::new(Model::new(profile)));
            profiles.insert(home, Rc::downgrade(&model));
            model
        });
        Self::bind(model)
    }
    fn bind(model: Rc<RefCell<Model>>) -> Self {
        model.borrow_mut().sync();
        let colors = model.borrow().colors.reader();
        let changes = colors.subscribe();
        Self { model, colors, changes }
    }
    #[cfg(test)]
    pub(crate) fn from_preferences(profile: Reader<Value>) -> Self {
        Self::bind(Rc::new(RefCell::new(Model::new(profile))))
    }
    pub(crate) fn pending(&self) -> bool {
        self.changes.pending() || self.model.borrow().changes.pending()
    }
    pub(crate) fn current(&mut self) -> Rc<Value> {
        self.model.borrow_mut().sync();
        self.changes.take();
        self.colors.read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consumers_share_the_plan_and_ignore_unrelated_profile_changes() {
        let profile = Store::new(json!({"mode":"dark","accent":"azul","effects":false}));
        let mut a = Source::from_preferences(profile.reader());
        let mut b = Source::bind(a.model.clone());
        let first = a.current(); assert!(Rc::ptr_eq(&first, &b.current()));
        profile.set(json!({"mode":"dark","accent":"azul","effects":false,"unrelated":[1,2,3]}));
        assert!(a.pending()); assert!(Rc::ptr_eq(&first, &a.current())); assert!(!b.pending());
        profile.set(json!({"mode":"light","accent":"violeta","effects":true}));
        assert!(b.pending()); let next = b.current(); assert!(!Rc::ptr_eq(&first, &next));
        assert!(a.pending()); assert!(Rc::ptr_eq(&next, &a.current()));
        assert_eq!(next["accent"], "#bf5af2"); assert_eq!(next["mode"], "light");
    }
    #[test]
    fn profile_registry_is_weak_and_keeps_profiles_isolated() {
        let a_home = format!("/tmp/axia-appearance-a-{}",std::process::id());
        let b_home = format!("/tmp/axia-appearance-b-{}",std::process::id());
        std::fs::create_dir_all(&a_home).unwrap(); std::fs::create_dir_all(&b_home).unwrap();
        let mut a = Source::new(&a_home); let weak = Rc::downgrade(&a.model);
        let mut again = Source::new(&a_home); let mut b = Source::new(&b_home);
        assert!(Rc::ptr_eq(&a.current(), &again.current()));
        assert!(!Rc::ptr_eq(&a.current(), &b.current()));
        let colors = a.colors.clone(); drop(a); drop(again);
        assert!(weak.upgrade().is_none(), "a plan reader must not retain its profile owner");
        assert_eq!(colors.read()["mode"], "dark");
        drop(b); std::fs::remove_dir_all(a_home).unwrap(); std::fs::remove_dir_all(b_home).unwrap();
    }
}
