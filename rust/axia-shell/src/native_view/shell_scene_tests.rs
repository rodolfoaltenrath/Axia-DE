use super::*;
use crate::app_control::Controller;
use std::time::Instant;
#[path = "shell_window_input_tests.rs"]
mod window_input;
#[path = "shell_desktop_input_tests.rs"]
mod desktop_input;
#[path = "shell_swipe_tests.rs"]
mod swipe_input;

fn fixture() -> (Session,Controller,i32) {
    fixture_home(None)
}
fn fixture_home(home: Option<&str>) -> (Session,Controller,i32) {
    let root = scene::create(scene::VIEW);
    scene::set_test_root(root);
    super::super::services::displays::update(&json!({"displays":[{"id":1,"x":0,"y":0,"w":640,"h":700,"main":true}]})).unwrap();
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    super::super::with(|registry| { registry.assets = assets.to_str().unwrap().into(); registry.home = home.map(Into::into); });
    let mut session = Session::default();
    for app in ["files","notes","calculator"] { session.call("open",&[json!({}),json!({"app":app})]).unwrap(); }
    session.activate_services();
    let mut control = Controller::new(session.clone());
    control.request(&json!({"cmd":"metadata","catalog":[],"registry":[{"id":"files","name":"Files"},{"id":"notes","name":"Notes"},{"id":"calculator","name":"Calculator"}]}));
    assert_eq!(control.request(&json!({"cmd":"shellRoot","node":root}))["bound"],true);
    (session,control,root)
}

struct InputHome(String);
impl Drop for InputHome { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
fn input_fixture(name: &str, profile: &str) -> (InputHome,Session,Controller,i32) {
    let home = InputHome(std::env::temp_dir().join(format!("axia-input-{name}-{}",std::process::id())).to_str().unwrap().into());
    std::fs::create_dir_all(&home.0).unwrap();
    let initialized = crate::settings::with(&home.0,|settings|settings.command(&json!({"cmd":"initialize","text":json!({"schema":3,"shortcutProfile":profile}).to_string()})));
    assert!(initialized.get("error").is_none());
    let (session,control,root) = fixture_home(Some(&home.0));
    (home,session,control,root)
}

#[test]
fn physical_switcher_keys_own_repeats_fast_commit_and_late_releases() {
    let (_home,mut session,mut control,root) = input_fixture("releases","macos");
    assert_eq!(session.physical_key(15,1,"Tab",16,false),0,"the theme has not enabled shortcuts");
    assert_eq!(session.physical_key(125,1,"Meta",16,true),0);
    assert_eq!(session.physical_key(15,1,"Tab",16,true),1);
    let first = control.request(&json!({"cmd":"switcherStatus"}))["nonce"].as_u64().unwrap();
    assert_eq!(handle(&mut control),0);
    assert_eq!(session.physical_key(125,0,"Meta",0,true),3);
    tick(&mut session);
    assert_eq!(session.reader().read().focused_window(),Some("notes"));
    assert_eq!(session.physical_key(15,2,"Tab",0,true),1);
    assert_eq!(session.physical_key(15,0,"Tab",0,true),1);
    assert_eq!(session.physical_key(15,0,"Tab",0,true),0,"a release is claimed only once");
    assert_eq!(session.physical_key(15,1,"Tab",17,true),1);
    tick(&mut session);
    assert_eq!(session.physical_key(15,2,"Tab",17,true),1,"repeat steps instead of opening a second nonce");
    assert!(control.request(&json!({"cmd":"switcherStatus"}))["nonce"].as_u64().unwrap()>first);
    assert_eq!(session.physical_key(1,1,"Escape",17,true),1);
    assert!(!control.request(&json!({"cmd":"switcherStatus"}))["opened"].as_bool().unwrap());
    assert_eq!(session.physical_key(1,0,"Escape",17,true),1);
    assert_eq!(session.physical_key(125,0,"Meta",1,true),3,"must not become a Super tap after Escape");
    assert_eq!(session.physical_key(42,0,"Shift",0,true),3);
    assert_eq!(session.physical_key(15,0,"Tab",0,true),1);
    assert_eq!(session.physical_key(30,1,"a",0,true),0);
    tick(&mut session);
    let effects = session.take_service_effects().unwrap();
    let keyboard = effects.as_array().unwrap().iter().position(|effect|effect["kind"]=="shellKeyboard").unwrap();
    assert!(effects.as_array().unwrap().iter().enumerate().all(|(index,effect)|!effect["reply"]["state"].is_object() || index<keyboard));
    assert!(session.service_wait(Instant::now()).is_none());
    scene::destroy(root);
}

#[test]
fn physical_activation_and_profile_cancellation_read_native_preferences() {
    let (home,mut session,mut control,root) = input_fixture("profiles","linux");
    assert_eq!(session.physical_key(15,1,"Tab",6,true),0,"Ctrl+Alt+Tab is not reserved");
    assert_eq!(session.physical_key(15,1,"Tab",12,true),0,"AltGr is not Alt");
    assert_eq!(session.physical_key(15,1,"Tab",4,true),1);
    tick(&mut session);
    crate::settings::with(&home.0,|settings|settings.command(&json!({"cmd":"patch","patch":{"shortcutProfile":"macos"}})));
    assert_eq!(session.service_wait(Instant::now()),Some(0.));
    tick(&mut session);
    assert_eq!(control.request(&json!({"cmd":"switcherStatus"}))["opened"],false);
    assert_eq!(session.physical_key(56,0,"Alt",0,true),3);
    assert_eq!(session.physical_key(15,0,"Tab",0,true),1);
    assert_eq!(session.physical_key(15,1,"Tab",4,true),0,"the same combination now belongs to the app");
    assert_eq!(session.physical_key(15,1,"Tab",16,true),1);
    tick(&mut session);
    assert_eq!(session.physical_key(96,1,"Enter",16,true),1);
    assert_eq!(session.reader().read().focused_window(),Some("notes"));
    assert_eq!(session.physical_key(125,0,"Meta",0,true),3);
    scene::destroy(root); tick(&mut session);
    assert_eq!(session.physical_key(15,1,"Tab",16,true),0,"an obsolete root cannot capture input");
}
fn tick(session: &mut Session) { session.service_tick(Instant::now()); }
fn handle(control: &mut Controller) -> u32 { control.request(&json!({"cmd":"switcherStatus"}))["handle"].as_u64().unwrap() as u32 }

#[test]
fn resident_mount_queues_fast_release_and_closes_without_a_js_component() {
    let (mut session,mut control,root) = fixture();
    let layer = control.request(&json!({"cmd":"shellLayer","z":40}))["node"].as_i64().unwrap() as i32;
    assert_eq!(control.request(&json!({"cmd":"shellLayer","z":40}))["node"],layer);
    let sibling = scene::create(scene::VIEW); scene::insert(layer,sibling,0);
    let opened = control.request(&json!({"cmd":"switcherPrepare","hold":8}));
    let nonce = opened["result"]["nonce"].as_u64().unwrap();
    assert_eq!(handle(&mut control),0);
    assert_eq!(control.request(&json!({"cmd":"shellKeyboard"}))["busy"],true);
    assert_eq!(control.request(&json!({"cmd":"switcherKey","key":"Meta","mods":0,"down":false,"shift":false}))["consumed"],true);
    tick(&mut session);
    assert_eq!(session.reader().read().focused_window(),Some("notes"));
    assert_eq!(control.request(&json!({"cmd":"switcherStatus"}))["opened"],false);
    assert_eq!(handle(&mut control),0);
    assert!(scene::exists(sibling));
    let effects = session.take_service_effects().unwrap();
    assert!(effects.as_array().unwrap().iter().any(|effect|effect["kind"]=="switcher" && effect["nonce"]==nonce && effect["reply"].is_object()));
    let hud = control.request(&json!({"cmd":"desktopHudStatus"}))["handle"].as_u64().unwrap() as u32;
    assert!(super::super::with(|registry|registry.owned.keys().copied().collect::<Vec<_>>()==vec![hud]));
    assert!(session.service_wait(Instant::now()).is_none());
    control.request(&json!({"cmd":"switcherPrepare","hold":8}));
    tick(&mut session);
    let mounted = handle(&mut control); assert!(mounted>0);
    assert_ne!(super::super::with_host(mounted,|host|host.find_text("Calculator")),Some(0));
    let bogus = std::ffi::CString::new(json!({"args":{"nonce":0},"registry":[],"context":{"theme":{"text":"#ff0000"}}}).to_string()).unwrap();
    unsafe { super::super::axia_view_facts(mounted,bogus.as_ptr()); }
    assert_ne!(super::super::with_host(mounted,|host|host.find_text("Calculator")),Some(0));
    control.request(&json!({"cmd":"switcherCancel"})); tick(&mut session);
    assert!(super::super::with_host(mounted,|_|()).is_none());
    assert!(scene::exists(sibling));
    scene::destroy(root);
}

#[test]
fn native_hosts_do_not_retain_a_context_and_cannot_destroy_recycled_nodes() {
    let (mut session,mut control,root) = fixture();
    let weak = session.downgrade();
    control.request(&json!({"cmd":"switcherPrepare","hold":8})); tick(&mut session);
    let status = control.request(&json!({"cmd":"switcherStatus"}));
    let mounted = status["handle"].as_u64().unwrap() as u32;
    let node = status["node"].as_i64().unwrap() as i32;
    let generation = scene::generation(node);
    scene::destroy(node);
    let reused = scene::create(scene::VIEW);
    assert_eq!(reused,node);
    assert_ne!(scene::generation(reused),generation);
    scene::insert(root,reused,0);
    tick(&mut session);
    assert!(scene::exists(reused));
    assert!(super::super::with_host(mounted,|_|()).is_none());
    let current = handle(&mut control); assert!(current>mounted);
    drop(control); drop(session);
    assert!(weak.upgrade().is_none(),"the registered native host must borrow the owner weakly");
    assert!(super::super::with_host(current,|_|()).is_none());
    assert!(scene::exists(reused));
    scene::destroy(root);
}

#[test]
fn replacing_the_root_cancels_the_old_opening_and_rejects_stale_keys() {
    let (mut session,mut control,root) = fixture();
    let nonce = control.request(&json!({"cmd":"switcherPrepare","hold":8}))["result"]["nonce"].as_u64().unwrap();
    tick(&mut session); let old = handle(&mut control);
    let next = scene::create(scene::VIEW); scene::set_test_root(next);
    assert_eq!(control.request(&json!({"cmd":"shellRoot","node":root}))["bound"],false);
    assert_eq!(control.request(&json!({"cmd":"shellRoot","node":next}))["bound"],true);
    assert!(super::super::with_host(old,|_|()).is_none());
    assert_eq!(control.request(&json!({"cmd":"shellKeyboard"}))["busy"],false);
    let opened = control.request(&json!({"cmd":"switcherPrepare","hold":8}));
    assert!(opened["result"]["nonce"].as_u64().unwrap()>nonce);
    assert_eq!(control.request(&json!({"cmd":"switcherKey","nonce":nonce,"key":"Meta","mods":0,"down":false,"shift":false}))["consumed"],false);
    tick(&mut session); assert!(handle(&mut control)>old);
    scene::destroy(next); tick(&mut session);
    assert_eq!(control.request(&json!({"cmd":"switcherStatus"}))["opened"],false);
    assert!(session.service_wait(Instant::now()).is_none());
    scene::destroy(root);
}

#[test]
fn superseded_completions_are_filtered_before_the_compatibility_drain() {
    let (mut session,mut control,root) = fixture();
    let first = control.request(&json!({"cmd":"switcherPrepare","hold":8}))["result"]["nonce"].as_u64().unwrap();
    control.request(&json!({"cmd":"switcherKey","key":"Meta","mods":0,"down":false,"shift":false}));
    tick(&mut session);
    assert!(session.service_pending());
    let second = control.request(&json!({"cmd":"switcherPrepare","hold":8}))["result"]["nonce"].as_u64().unwrap();
    assert!(second>first);
    let effects = session.take_service_effects().unwrap();
    assert!(effects.as_array().unwrap().iter().all(|effect|effect["kind"]!="switcher"));
    assert_eq!(control.request(&json!({"cmd":"switcherStatus"}))["nonce"],second);
    tick(&mut session);
    control.request(&json!({"cmd":"switcherCancel"})); tick(&mut session);
    scene::destroy(root);
}
