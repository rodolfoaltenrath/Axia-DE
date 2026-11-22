use super::*;
use axia_state::Store;
use serde_json::{json, Value};
use std::time::Duration;

struct Profile(String);
impl Drop for Profile { fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); } }
fn fixture(name: &str, mode: &str, saved: Value) -> (Profile, Session, Store<Value>) {
    let root = std::env::temp_dir().join(format!("axia-resident-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let home = root.to_str().unwrap().to_owned();
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize","text":json!({"schema":3,
            "sessionRestore":mode,"sessionBrowsers":false,"sessionSaved":saved}).to_string()}));
    });
    let session = Session::default();
    session.restore_service().borrow_mut().bind_settings(&home);
    session.desktop_service().borrow_mut().bind_settings(&home);
    let catalog = Store::new(json!({"registry":[{"id":"notes","name":"Notes"}],"catalog":[]}));
    session.restore_service().borrow_mut().bind_catalog(catalog.reader());
    session.activate_services();
    (Profile(home), session, catalog)
}
fn command(session: &mut Session, catalog: &Store<Value>, action: &str, now: Instant) -> Value {
    session.restore_service().borrow_mut().command(session, &catalog.read(), &json!({"action":action}), now)
}
fn profile(home: &Profile) -> Value { crate::settings::with(&home.0, |s| s.snapshot())["state"].clone() }

#[test]
fn restore_deadline_and_autosave_run_without_any_scene_host() {
    let (home, mut session, catalog) = fixture("autosave", "always", Value::Null);
    let now = Instant::now();
    command(&mut session, &catalog, "start", now);
    session.call("open", &[json!({}),json!({"app":"notes","frame":{"x":40,"y":60,"w":300,"h":220}})]).unwrap();
    assert_eq!(timeout(now), Some(0.));
    tick(now);
    assert_eq!(timeout(now), Some(2000.));
    tick(now + Duration::from_secs(2));
    assert_eq!(profile(&home)["sessionSaved"]["windows"][0]["f"]["x"], 40.);
    assert_eq!(profile(&home)["sessionSaved"]["clean"], false);
    assert!(pending());
    assert_eq!(session.take_service_effects().unwrap()[0]["kind"], "settingsState");
    tick(now + Duration::from_secs(2)); // consume the writer's preference revision
    assert!(timeout(now + Duration::from_secs(2)).is_none());
    assert!(!pending());
    command(&mut session, &catalog, "flush", now + Duration::from_secs(3));
    tick(now + Duration::from_secs(3));
    assert_eq!(profile(&home)["sessionSaved"]["clean"], true);
    assert!(timeout(now + Duration::from_secs(3)).is_none());
}

#[test]
fn prompt_and_catalog_revisions_are_serviced_without_a_view() {
    let saved = json!({"clean":false,"windows":[{"id":"app:notes","f":{"x":40,"y":60,"w":300,"h":220}}]});
    let (_prompt_home, mut session, catalog) = fixture("prompt", "ask", saved.clone());
    let now = Instant::now();
    command(&mut session, &catalog, "start", now);
    tick(now);
    assert_eq!(timeout(now), Some(1500.));
    tick(now + Duration::from_millis(1500));
    let effects = session.take_service_effects().unwrap();
    assert!(effects.as_array().unwrap().iter().any(|e| e["op"]=="visible" && e["value"]==true));
    assert!(timeout(now + Duration::from_millis(1500)).is_none());
    drop(session);
    let mut clean = saved;
    clean["clean"] = json!(true);
    let (_catalog_home, mut session, catalog) = fixture("catalog", "always", clean);
    command(&mut session, &catalog, "start", now);
    catalog.set(json!({"registry":[{"id":"notes","hidden":true}],"catalog":[]}));
    assert_eq!(timeout(now), Some(0.));
    tick(now + Duration::from_millis(1500));
    let effects = session.take_service_effects().unwrap_or(json!([]));
    assert!(effects.as_array().unwrap().iter().all(|e| e["op"]!="open"), "latest catalog must suppress a hidden app");
    assert_eq!(timeout(now + Duration::from_millis(1500)), Some(10000.), "restore processed the step and entered settlement");
}

#[test]
fn context_registry_is_weak_deduplicated_and_preserves_clone_identity() {
    let a = Session::default();
    let copy = a.clone();
    let reader = a.reader();
    let weak = a.downgrade();
    a.activate_services(); copy.activate_services();
    assert_eq!(CONTEXTS.with(|s|s.borrow().len()), 1);
    let b = Session::default(); b.activate_services();
    drop(a);
    assert!(weak.upgrade().is_some());
    drop(copy);
    assert!(weak.upgrade().is_none(), "a window reader must not retain a context or its services");
    assert_eq!(reader.read().windows().count(), 0);
    assert!(timeout(Instant::now()).is_none());
    assert_eq!(CONTEXTS.with(|s|s.borrow().len()), 1);
    drop(b);
    assert!(!pending());
    assert_eq!(CONTEXTS.with(|s|s.borrow().len()), 0);
}

#[test]
fn background_preferences_coalesce_and_native_drain_is_quiet_and_context_scoped() {
    let (_home, mut session, _) = fixture("queue", "off", Value::Null);
    session.desktop_service().borrow_mut().command(&mut session, &json!({"action":"start"}));
    let control = crate::native_apps_host::NativeApp::Control(crate::app_control::Controller::new(session.clone()));
    unsafe { assert!(crate::native_apps_host::axia_native_app_take_services(&control).is_null()); }
    for _ in 0..4 {
        session.call("createDesktop", &[json!({})]).unwrap();
        tick(Instant::now());
    }
    let other = Session::default();
    assert!(other.take_service_effects().is_none());
    unsafe {
        assert!(crate::native_apps_host::axia_native_app_take_services(std::ptr::null()).is_null());
        let out = crate::native_apps_host::axia_native_app_take_services(&control);
        assert!(!out.is_null());
        let effects: Value = serde_json::from_slice(std::ffi::CStr::from_ptr(out).to_bytes()).unwrap();
        crate::native_apps_host::axia_native_app_reply_free(out);
        assert_eq!(effects.as_array().unwrap().len(), 1);
        assert_eq!(effects[0]["reply"]["state"]["desktopCount"], 5);
        assert!(crate::native_apps_host::axia_native_app_take_services(&control).is_null());
    }
}
