use super::*;
struct PreferenceProbe {
    reader: axia_state::Reader<Value>,
    log: Rc<RefCell<Vec<(Value, Value)>>>,
}
impl NativeApp for PreferenceProbe {
    fn uses_preferences(&self) -> bool { true }
    fn request(&mut self, input: &Value) -> Value {
        assert_ne!(input["cmd"], "sync", "session sync must borrow native preferences");
        json!({})
    }
    fn sync_with_preferences(&mut self, input: &Value, preferences: &Value) -> Value {
        assert!(std::ptr::eq(preferences, &*self.reader.read()), "profile must not be cloned");
        assert!(input.get("settings").is_none());
        assert_eq!(input["context"]["preferenceRevision"], self.reader.revision());
        self.log.borrow_mut().push((input.clone(), preferences["probe"].clone()));
        json!({"tree":{"kind":"text","label":preferences["probe"].to_string()}})
    }
}
#[test]
fn bound_preferences_borrow_the_owner_ignore_stale_facts_and_wake_without_transport() {
    let root = std::env::temp_dir().join(format!("axia-view-preferences-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let home = root.to_str().unwrap();
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    let reader = crate::settings::with(home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.set("probe", &json!(1));
        s.reader()
    });
    let mount = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = Host::new(mount, Box::new(PreferenceProbe { reader, log:log.clone() }), assets, Some(home));
    host.bind_locale(home, assets);
    let stale = json!({"settings":{"probe":999},"context":{"preferenceRevision":999}});
    host.facts(stale.clone());
    assert_eq!(log.borrow().len(), 1);
    assert_eq!(log.borrow()[0].1, 1);
    assert!(host.deadline().is_none());
    host.facts(stale.clone());
    assert_eq!(log.borrow().len(), 1);
    crate::settings::with(home, |s| { s.set("probe", &json!(2)); });
    assert_eq!(host.deadline(), Some(0.));
    host.tick(1000.);
    assert_eq!(log.borrow().len(), 2);
    assert_eq!(log.borrow()[1].1, 2);
    assert!(host.deadline().is_none());
    host.facts(stale);
    host.tick(2000.);
    assert_eq!(log.borrow().len(), 2);
    assert!(host.deadline().is_none());
    drop(host);
    scene::destroy(mount);
    std::fs::remove_dir_all(root).unwrap();
}
struct Probe(Rc<RefCell<Vec<Value>>>);
impl NativeApp for Probe {
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] != "sync" {
            return json!({});
        }
        self.0.borrow_mut().push(input.clone());
        json!({"tree":{"kind":"text","label":{"key":"apps.clock"}}})
    }
}
#[test]
fn bound_views_read_locale_without_transport_and_wake_only_on_changes() {
    let root = std::env::temp_dir().join(format!("axia-view-locale-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let home = root.to_str().unwrap();
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    crate::settings::with(home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.set("language", &json!("pt"));
        s.set("region", &json!("pt_PT"));
    });
    let mount = scene::create(scene::VIEW);
    unsafe {
        let n = crate::runtime::node_get(mount);
        (*n).w = 640.;
        (*n).h = 480.;
    }
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = Host::new(mount, Box::new(Probe(log.clone())), assets, Some(home));
    host.bind_locale(home, assets);
    let stale = json!({"lang":"en","context":{"region":"en_US","direction":"rtl","decimal":"!","group":"!","localeRevision":999}});
    host.facts(stale.clone());
    assert_eq!(log.borrow()[0]["lang"], "pt");
    assert_eq!(log.borrow()[0]["context"]["region"], "pt_PT");
    assert_eq!(log.borrow()[0]["context"]["direction"], "ltr");
    assert_eq!(log.borrow()[0]["context"]["decimal"], ",");
    assert_eq!(log.borrow()[0]["context"]["group"], " ");
    assert_eq!(log.borrow()[0]["context"]["localeRevision"], 0);
    assert!(host.text().contains("Relógio"));
    assert!(host.deadline().is_none());
    host.facts(stale);
    host.tick(1000.);
    assert_eq!(
        log.borrow().len(),
        1,
        "stale transport cannot repaint native locale"
    );
    crate::settings::with(home, |s| {
        s.set("language", &json!("en"));
        s.set("region", &json!("en_GB"));
    });
    crate::locale::tick();
    assert_eq!(host.deadline(), Some(0.));
    host.tick(2000.);
    assert_eq!(log.borrow().len(), 2);
    assert_eq!(log.borrow()[1]["lang"], "en");
    assert_eq!(log.borrow()[1]["context"]["region"], "en_GB");
    assert!(host.text().contains("Clock"));
    assert!(host.deadline().is_none());
    let folder = root.join(".local/share/axia/locales/en");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("apps.json"), r#"{"clock":"Reloaded view"}"#).unwrap();
    std::fs::write(folder.join("meta.json"), r#"{"direction":"rtl"}"#).unwrap();
    let c_home = std::ffi::CString::new(home).unwrap();
    unsafe {
        crate::locale::axia_locale_changed(c_home.as_ptr(), 0);
    }
    assert_eq!(host.deadline(), Some(0.));
    host.tick(3000.);
    assert_eq!(log.borrow()[2]["context"]["direction"], "rtl");
    assert_eq!(log.borrow()[2]["context"]["localeRevision"], 1);
    assert!(host.text().contains("Reloaded view"));
    assert!(host.deadline().is_none());
    host.tick(4000.);
    assert_eq!(log.borrow().len(), 3);
    crate::settings::with(home, |s| { s.set("unrelated", &json!(true)); });
    crate::locale::tick();
    host.tick(5000.);
    assert_eq!(log.borrow().len(), 3, "presentation-only views do not subscribe to preferences");
    assert!(host.deadline().is_none());
    drop(host);
    scene::destroy(mount);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn unbound_app_host_retains_local_locale_facts_and_null_session_is_rejected() {
    let mount = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = Host::new(mount, Box::new(Probe(log.clone())), "", None);
    host.facts(json!({"lang":"ar","context":{"region":"ar_EG","direction":"rtl","decimal":"!"}}));
    assert_eq!(log.borrow()[0]["lang"], "ar");
    assert_eq!(log.borrow()[0]["context"]["decimal"], "!");
    assert!(host.deadline().is_none());
    assert_eq!(
        unsafe {
            super::super::axia_view_mount_session(
                mount,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    drop(host);
    scene::destroy(mount);
}

struct AppearanceProbe {
    catalogs_bound: bool,
    theme: Rc<Value>,
    log: Rc<RefCell<Vec<Rc<Value>>>>,
}
impl NativeApp for AppearanceProbe {
    fn uses_native_appearance(&self) -> bool { true }
    fn set_native_appearance_catalogs(&mut self, wallpapers: &'static Value, accents: &'static Value) {
        assert!(std::ptr::eq(wallpapers, crate::native_style::catalog::wallpapers()));
        assert!(std::ptr::eq(accents, crate::native_style::catalog::accents()));
        self.catalogs_bound = true;
    }
    fn set_native_appearance(&mut self, theme: Rc<Value>) { self.theme = theme; }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] != "sync" { return json!({}); }
        assert!(input["context"].get("theme").is_none());
        assert!(input["context"].get("scale").is_none());
        assert!(self.catalogs_bound);
        self.log.borrow_mut().push(self.theme.clone());
        json!({"tree":{"kind":"text","label":self.theme["accent"]}})
    }
}
#[test]
fn native_appearance_is_shared_ignores_stale_facts_and_only_repaints_for_changed_colors() {
    let root=std::env::temp_dir().join(format!("axia-view-appearance-{}",std::process::id()));
    std::fs::create_dir_all(&root).unwrap(); let home=root.to_str().unwrap();
    let assets=concat!(env!("CARGO_MANIFEST_DIR"),"/../../assets");
    crate::settings::with(home,|s|{s.command(&json!({"cmd":"initialize"}));s.set("accent",&json!("azul"));});
    let mut owner=crate::native_style::appearance::Source::new(home);
    let expected=owner.current();
    let mount=scene::create(scene::VIEW);let log=Rc::new(RefCell::new(Vec::new()));
    let mut host=Host::new(mount,Box::new(AppearanceProbe{catalogs_bound:false,theme:Rc::new(Value::Null),log:log.clone()}),assets,Some(home));
    host.bind_locale(home,assets);
    let stale=json!({"context":{"theme":{"accent":"#ff0000"},"scale":{"body":999}}});
    host.facts(stale.clone());assert_eq!(log.borrow().len(),1);
    assert!(Rc::ptr_eq(&expected,&log.borrow()[0]));
    host.facts(stale.clone());assert_eq!(log.borrow().len(),1);
    crate::settings::with(home,|s|{s.set("unrelated",&json!([1,2,3]));});
    assert_eq!(host.deadline(),Some(0.));host.tick(1000.);
    assert_eq!(log.borrow().len(),1);assert!(host.deadline().is_none());
    crate::settings::with(home,|s|{s.set("accent",&json!("violeta"));});
    assert_eq!(host.deadline(),Some(0.));host.tick(2000.);
    assert_eq!(log.borrow().len(),2);assert_eq!(log.borrow()[1]["accent"],"#bf5af2");
    assert!(Rc::ptr_eq(&owner.current(),&log.borrow()[1]));
    host.facts(stale);host.tick(3000.);assert_eq!(log.borrow().len(),2);
    assert!(host.deadline().is_none());drop(host);scene::destroy(mount);
    std::fs::remove_dir_all(root).unwrap();
}
