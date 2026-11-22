//! Publish native appearance to isolated apps without a React provider.
use serde::{ser::SerializeMap, Serialize, Serializer};
use serde_json::{json, Value};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Key {
    mode: String, accent: String, effects: Option<Value>, native: bool,
    scroll_bars: String, reduce_motion: bool,
}
impl Key {
    fn from_profile(profile: &Value) -> Self {
        Self {
            mode: profile["mode"].as_str().unwrap_or("dark").into(),
            accent: profile["accent"].as_str().unwrap_or("azul").into(),
            effects: profile.get("effects").cloned(),
            native: profile["nativeAppearance"] != "axia",
            scroll_bars: profile["scrollBars"].as_str().filter(|s| !s.is_empty()).unwrap_or("auto").into(),
            reduce_motion: crate::native_style::shell_theme::truthy(&profile["reduceMotion"]),
        }
    }
    fn colors(&self) -> Value {
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

struct Palette;
impl Serialize for Palette {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(crate::native_style::shell_theme::ACCENTS.len()))?;
        for (name,color) in crate::native_style::shell_theme::ACCENTS { map.serialize_entry(name,color)?; }
        map.end()
    }
}
struct Tokens<'a>(&'a Value);
impl Serialize for Tokens<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let colors = self.0.as_object().unwrap();
        let mut map = serializer.serialize_map(Some(colors.len()+2))?;
        for (key,value) in colors { map.serialize_entry(key,value)?; }
        map.serialize_entry("accents", &Palette)?;
        map.serialize_entry("wallpapers", crate::native_style::catalog::wallpapers())?;
        map.end()
    }
}

#[derive(Default)]
pub(super) struct Service { last: Option<Key>, locale: Option<LocaleKey> }
#[derive(PartialEq, Eq)]
pub(super) struct LocaleKey { lang: String, region: String, dir: &'static str }
impl Service {
    pub(super) fn sync(&mut self, key: Key) {
        if self.last.as_ref() == Some(&key) { return; }
        let colors = key.colors();
        let encoded = serde_json::to_vec(&Tokens(&colors)).expect("serializable compiled theme");
        if publish(c"theme", &encoded) { self.last = Some(key); }
    }
    pub(super) fn resolve(profile: &Value) -> Key { Key::from_profile(profile) }
    pub(super) fn resolve_locale(state: &crate::locale::State) -> LocaleKey {
        LocaleKey { lang: state.lang.clone(), region: state.region.clone(), dir: state.dir }
    }
    pub(super) fn sync_locale(&mut self, key: LocaleKey) {
        if self.locale.as_ref() == Some(&key) { return; }
        let encoded = serde_json::to_vec(&json!({"lang":key.lang,"region":key.region,"dir":key.dir})).unwrap();
        if publish(c"locale", &encoded) { self.locale = Some(key); }
    }
}

#[cfg(not(test))]
fn publish(kind: &std::ffi::CStr, bytes: &[u8]) -> bool {
    extern "C" { fn brokersrv_publish(kind: *const std::ffi::c_char, json: *const std::ffi::c_char, len: usize) -> i32; }
    // The broker copies the bytes synchronously into its bounded state buffer.
    unsafe { brokersrv_publish(kind.as_ptr(), bytes.as_ptr().cast(), bytes.len()) == 0 }
}
#[cfg(test)]
thread_local! { static SENT: std::cell::RefCell<Vec<(String, Vec<u8>)>> = const { std::cell::RefCell::new(Vec::new()) }; }
#[cfg(test)]
fn publish(kind: &std::ffi::CStr, bytes: &[u8]) -> bool { SENT.with(|sent| sent.borrow_mut().push((kind.to_str().unwrap().into(), bytes.to_vec()))); true }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_and_axia_tokens_preserve_the_isolated_contract_and_materials() {
        for mode in ["light","dark"] { for native in [false,true] { for effects in [false,true] {
            let key = Key::from_profile(&json!({"mode":mode,"accent":"violeta","nativeAppearance":if native {"adwaita"} else {"axia"},
                "effects":effects,"scrollBars":"always","reduceMotion":true}));
            let colors = key.colors(); let encoded = serde_json::to_vec(&Tokens(&colors)).unwrap();
            assert!(encoded.len()<64*1024); let text=std::str::from_utf8(&encoded).unwrap();
            let tokens:Value=serde_json::from_slice(&encoded).unwrap();
            assert_eq!(tokens["accent"],"#bf5af2"); assert_eq!(tokens["scrollBars"],"always");
            assert_eq!(tokens["reduceMotion"],true); assert_eq!(tokens["effects"],effects);
            assert_eq!(tokens["controlRadius"],if native {4} else {7});
            assert_eq!(tokens["mode"],mode); assert_eq!(tokens["blur"],if effects {24} else {0});
            assert_eq!(tokens["wallpapers"],*crate::native_style::catalog::wallpapers());
            let ordered=["\"azul\"","\"violeta\"","\"rosa\"","\"naranja\"","\"verde\"","\"grafito\""];
            let positions:Vec<_>=ordered.iter().map(|name|text.find(name).unwrap()).collect();
            assert!(positions.windows(2).all(|pair|pair[0]<pair[1]));
        } } }
    }
    #[test]
    fn locale_projects_only_public_fields_and_ignores_catalog_only_revisions() {
        SENT.with(|sent|sent.borrow_mut().clear());
        let mut service=Service::default();
        let mut state=crate::locale::State {lang:"pt".into(),region:"pt_BR".into(),dir:"rtl",revision:1,extra:serde_json::Map::new()};
        service.sync_locale(Service::resolve_locale(&state));
        state.revision=2;state.extra.insert("private".into(),json!([1,2,3]));
        service.sync_locale(Service::resolve_locale(&state));
        state.dir="ltr";service.sync_locale(Service::resolve_locale(&state));
        SENT.with(|sent| { let sent=sent.borrow();assert_eq!(sent.len(),2);assert_eq!(sent[0].0,"locale");
            assert_eq!(serde_json::from_slice::<Value>(&sent[0].1).unwrap(),json!({"lang":"pt","region":"pt_BR","dir":"rtl"}));
            assert_eq!(serde_json::from_slice::<Value>(&sent[1].1).unwrap()["dir"],"ltr");
        });
    }
    #[test]
    fn unrelated_revisions_do_not_republish_and_current_preferences_do() {
        SENT.with(|sent|sent.borrow_mut().clear());
        let mut service=Service::default();
        let mut profile=json!({"mode":"dark","accent":"azul","effects":true,"nativeAppearance":"adwaita"});
        service.sync(Service::resolve(&profile));
        for i in 0..100 { profile["unrelated"]=json!(i); service.sync(Service::resolve(&profile)); }
        assert_eq!(SENT.with(|sent|sent.borrow().len()),1);
        profile["scrollBars"]=json!("always"); service.sync(Service::resolve(&profile));
        profile["reduceMotion"]=json!(true); service.sync(Service::resolve(&profile));
        profile["nativeAppearance"]=json!("axia"); service.sync(Service::resolve(&profile));
        SENT.with(|sent| { let sent=sent.borrow(); assert_eq!(sent.len(),4);
            let latest:Value=serde_json::from_slice(&sent.last().unwrap().1).unwrap();
            assert_eq!(latest["scrollBars"],"always");assert_eq!(latest["reduceMotion"],true);
            assert!(latest.get("nativeStyle").is_none());
        });
    }
}
