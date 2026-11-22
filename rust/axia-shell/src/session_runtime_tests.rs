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

#[test]
fn resident_notifications_expire_and_deliver_close_without_a_banner() {
    axia_clock::with(|s| *s = Default::default());
    let (_home, session, _) = fixture("notice-deadline", "off", Value::Null);
    session.activate_clock_and_notifications();
    let now = Instant::now();
    tick(now);
    session.take_service_effects();
    let notices = session.notification_service();
    let reply = notices.borrow_mut().command(&json!({"action":"push",
        "notice":{"title":"No banner","sticky":false,"timeout":1500,"sound":false}}), now);
    let id = reply["result"].as_u64().unwrap();
    tick(now);
    session.take_service_effects();
    assert_eq!(timeout(now), Some(1500.));
    tick(now + Duration::from_millis(1499));
    assert_eq!(notices.borrow().reader().read().shown.len(), 1);
    tick(now + Duration::from_millis(1500));
    assert!(notices.borrow().reader().read().shown.is_empty());
    let effects = session.take_service_effects().unwrap();
    assert!(effects.as_array().unwrap().iter().any(|e| e["kind"]=="notificationClose" && e["id"]==id));
    assert!(timeout(now + Duration::from_millis(1500)).is_none());
    tick(now + Duration::from_secs(2));
    assert!(!pending(), "closing is delivered only once without a presentation host");
}

#[test]
fn resident_clock_completes_once_and_releases_its_context() {
    axia_clock::with(|s| *s = Default::default());
    let (_home, session, _) = fixture("clock-background", "off", Value::Null);
    session.activate_clock_and_notifications();
    let weak = session.downgrade();
    let mut surviving_clone = session.clone();
    drop(session);
    let now = Instant::now();
    tick(now); surviving_clone.take_service_effects();
    // The wall deadline has elapsed before the next native tick, with no scene ever mounted.
    axia_clock::with(|s| s.native(&json!({"cmd":"timerStart","seconds":1}), axia_clock::now()-1000));
    assert_eq!(timeout(now), Some(0.));
    tick(now);
    let effects = surviving_clone.take_service_effects().unwrap();
    assert_eq!(effects.as_array().unwrap().iter().filter(|e| e["kind"]=="clockReply").count(), 1);
    assert_eq!(axia_clock::with(|s|s.timer["state"].clone()), "idle");
    tick(now);
    assert!(surviving_clone.take_service_effects().is_none());
    let service = surviving_clone.notification_service();
    assert_eq!(service.borrow().reader().read().history.iter().filter(|n| n["app"]=="clock").count(), 1);
    let ids: Vec<_> = service.borrow().reader().read().shown.iter().map(|n|n.data["id"].clone()).collect();
    for id in ids { service.borrow_mut().command(&json!({"action":"dismiss","id":id}),now); }
    surviving_clone.service_tick(now);
    assert!(timeout(now).is_none());
    drop(surviving_clone);
    assert!(weak.upgrade().is_none(), "clock and notification services must not retain the context");
    assert!(timeout(now).is_none());
}

#[test]
fn resident_ring_uses_one_deadline_and_observes_native_policy_without_a_view() {
    axia_clock::with(|s| *s = Default::default());
    let (home, session, _) = fixture("ring-background", "off", Value::Null);
    session.activate_clock_and_notifications();
    let service = session.notification_service();
    service.borrow_mut().bind_settings(&home.0);
    let now = Instant::now();
    tick(now); session.take_service_effects();
    let reply = service.borrow_mut().command(&json!({"action":"push",
        "notice":{"app":"clock","title":"Alarm","ring":true,"sound":true,"sticky":true}}), now);
    assert!(reply["result"].as_u64().is_some_and(|id| id>0));
    tick(now); session.take_service_effects();
    assert_eq!(timeout(now), Some(2500.));
    let revision = service.borrow().reader().revision();
    tick(now + Duration::from_secs(30));
    assert_eq!(timeout(now + Duration::from_secs(30)), Some(2500.), "a late pulse schedules once from now");
    assert_eq!(service.borrow().reader().revision(), revision, "audio pulses do not republish a list");
    assert!(!pending());
    crate::settings::with(&home.0, |s| { s.set("notifications", &json!({"dnd":true})); });
    assert_eq!(timeout(now + Duration::from_secs(30)), Some(0.));
    tick(now + Duration::from_secs(30));
    assert!(timeout(now + Duration::from_secs(30)).is_none(), "native policy cancels ringing without a banner");
    assert_eq!(service.borrow().reader().read().shown.len(), 1);
}

fn memory_fixture(name: &str) -> (Profile, Session, Store<Value>) {
    let (home, session, catalog) = fixture(name, "off", Value::Null);
    displays::update(&json!({"displays":[{"id":0,"name":"Panel","connector":"DP-1",
        "x":0,"y":0,"w":1000,"h":800}]})).unwrap();
    let memory = session.memory_service();
    memory.borrow_mut().bind_settings(&home.0);
    memory.borrow_mut().bind_catalog(catalog.reader());
    memory.borrow_mut().enable(true);
    (home, session, catalog)
}

#[test]
fn resident_memory_keeps_its_deadline_and_cache_across_context_clones_without_views() {
    let (home, mut session, _) = memory_fixture("memory-clone");
    session.call("open", &[json!({}),json!({"app":"notes","frame":{"x":40,"y":60,"w":300,"h":220}})]).unwrap();
    let now = Instant::now();
    tick(now); session.take_service_effects();
    assert_eq!(timeout(now), Some(800.));
    let weak = session.downgrade();
    let mut copy = session.clone(); drop(session);
    copy.call("setFrame", &[json!({}),json!("notes"),json!({"x":77,"y":60,"w":300,"h":220})]).unwrap();
    tick(now + Duration::from_millis(600));
    assert_eq!(timeout(now + Duration::from_millis(600)), Some(200.));
    assert!(profile(&home)["windowPlaces"]["app:notes"].is_null());
    tick(now + Duration::from_millis(800));
    assert_eq!(profile(&home)["windowPlaces"]["app:notes"]["x"], 77.);
    tick(now + Duration::from_millis(800)); copy.take_service_effects();
    assert!(timeout(now + Duration::from_millis(800)).is_none());
    copy.call("patch", &[json!({}),json!("notes"),json!({"title":"No geometry change"})]).unwrap();
    tick(now + Duration::from_secs(1));
    tick(now + Duration::from_millis(1800));
    assert!(!pending(), "unchanged placement must not write another snapshot");
    let service = copy.memory_service();
    drop(copy);
    assert!(weak.upgrade().is_none(), "a memory service reader must not retain its context");
    assert!(service.borrow().wait_ms(now + Duration::from_secs(2)).is_none());
}

#[test]
fn resident_memory_borrows_catalog_and_physical_display_revisions_without_window_changes() {
    let (home, mut session, catalog) = memory_fixture("memory-sources");
    session.call("open", &[json!({}),json!({"app":"notes","frame":{"x":140,"y":60,"w":300,"h":220}})]).unwrap();
    let now = Instant::now(); tick(now); tick(now + Duration::from_millis(800));
    tick(now + Duration::from_millis(800)); session.take_service_effects();
    assert_eq!(profile(&home)["windowPlaces"]["app:notes"]["x"], 140.);
    catalog.set(json!({"registry":[{"id":"notes","package":true}],"catalog":[]}));
    assert_eq!(timeout(now + Duration::from_secs(1)), Some(0.));
    tick(now + Duration::from_secs(1)); tick(now + Duration::from_millis(1800));
    assert_eq!(profile(&home)["windowPlaces"]["lpk:notes"]["x"], 140.);
    displays::update(&json!({"displays":[{"id":7,"name":"Moved","connector":"DP-2",
        "x":100,"y":0,"w":1000,"h":800}]})).unwrap();
    tick(now + Duration::from_secs(2)); tick(now + Duration::from_millis(2800));
    let saved = profile(&home);
    assert_eq!(saved["windowPlaces"]["lpk:notes"]["x"], 40.);
    assert_eq!(saved["windowPlaces"]["lpk:notes"]["d"], "Moved|DP-2");
    tick(now + Duration::from_millis(2800)); session.take_service_effects();
    assert!(timeout(now + Duration::from_secs(3)).is_none());
}

#[test]
fn disabled_memory_drops_its_deadline_and_enabling_collects_the_current_owner() {
    let (home, mut session, _) = memory_fixture("memory-disable");
    session.call("open", &[json!({}),json!({"app":"notes","frame":{"x":40,"y":60,"w":300,"h":220}})]).unwrap();
    let now = Instant::now(); tick(now);
    let memory = session.memory_service(); memory.borrow_mut().enable(false);
    session.call("setFrame", &[json!({}),json!("notes"),json!({"x":99,"y":60,"w":300,"h":220})]).unwrap();
    tick(now + Duration::from_secs(1));
    assert!(memory.borrow().wait_ms(now + Duration::from_secs(1)).is_none());
    assert!(profile(&home)["windowPlaces"]["app:notes"].is_null());
    memory.borrow_mut().enable(true);
    tick(now + Duration::from_secs(1)); tick(now + Duration::from_millis(1800));
    assert_eq!(profile(&home)["windowPlaces"]["app:notes"]["x"], 99.);
}
