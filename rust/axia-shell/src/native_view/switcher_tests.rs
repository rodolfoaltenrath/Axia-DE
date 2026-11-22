use super::{switcher::Switcher, Host};
use crate::{runtime::native as scene, winmodel::Session};
use serde_json::{json, Value};

fn call(state: &mut Session, op: &str, args: &[Value]) {
    state
        .call(op, &[vec![json!({})], args.to_vec()].concat())
        .unwrap();
}
fn physical_display() -> axia_state::Reader<Vec<Value>> {
    axia_state::Store::new(vec![json!({"x":0,"y":0,"w":640,"h":700,"main":true})]).reader()
}
fn host(state: &Session, args: Value) -> (Host, i32) {
    let node = scene::create(scene::VIEW);
    let mut host = Host::new(node, Box::new(Switcher::new(state.reader())), "", None);
    host.facts(json!({"width":640,"args":args,"registry":[
        {"id":"files","name":"Files"},{"id":"notes","name":"Notes"},
        {"id":"calculator","name":"Calculator"},{"id":"linux","name":"Linux","builtin":true}],"catalog":[]}));
    (host, node)
}

#[test]
fn native_switcher_uses_mru_and_replaces_removed_members_before_commit() {
    let mut state = Session::default();
    for input in [
        json!({"app":"files"}),
        json!({"app":"notes","instance":"a"}),
        json!({"app":"notes","instance":"b"}),
        json!({"app":"calculator"}),
    ] {
        call(&mut state, "open", &[input]);
    }
    let (mut host, node) = host(&state, json!({"nonce":7}));
    assert_ne!(host.find_text("Notes"), 0);
    assert!(host.deadline().is_none());
    call(&mut state, "close", &[json!("notes#b")]);
    host.action(&json!({"action":["commit"]}));
    assert_eq!(
        host.take(),
        Some(json!([{"kind":"switcher","nonce":7,"focus":"notes#a"}]))
    );
    assert!(host.deadline().is_none());
    drop(host);
    scene::destroy(node);
}

#[test]
fn native_switcher_wraps_and_closes_once_when_all_members_disappear() {
    let mut state = Session::default();
    call(&mut state, "open", &[json!({"app":"files"})]);
    call(&mut state, "open", &[json!({"app":"notes"})]);
    let (mut host, node) = host(&state, json!({"nonce":8}));
    assert_ne!(host.find_text("Files"), 0);
    host.action(&json!({"action":["move",-1]}));
    assert_ne!(host.find_text("Notes"), 0);
    call(&mut state, "close", &[json!("files")]);
    call(&mut state, "close", &[json!("notes")]);
    host.tick(0.);
    assert_eq!(
        host.take(),
        Some(json!([{"kind":"switcher","nonce":8,"focus":null}]))
    );
    host.action(&json!({"action":["commit"]}));
    assert!(host.take().is_none());
    assert!(host.deadline().is_none());
    drop(host);
    scene::destroy(node);
}

#[test]
fn native_switcher_fullscreen_cancel_restores_only_a_live_origin() {
    for close_origin in [false, true] {
        let mut state = Session::default();
        call(&mut state, "open", &[json!({"app":"files"})]);
        call(
            &mut state,
            "open",
            &[json!({"app":"linux","instance":9,"args":{"appId":"game","surface":9}})],
        );
        call(&mut state, "minimize", &[json!("linux#9")]);
        let (mut host, node) = host(&state, json!({"nonce":9,"origin":"linux#9"}));
        assert_ne!(host.find_text("Files"), 0);
        if close_origin {
            call(&mut state, "close", &[json!("linux#9")]);
        }
        host.action(&json!({"action":["cancel"]}));
        let focus = if close_origin {
            Value::Null
        } else {
            json!("linux#9")
        };
        assert_eq!(
            host.take(),
            Some(json!([{"kind":"switcher","nonce":9,"focus":focus}]))
        );
        drop(host);
        scene::destroy(node);
    }
}

#[test]
fn bound_switcher_reads_catalog_revisions_and_revalidates_before_commit() {
    let mut session = Session::default();
    for app in ["files","notes","calculator"] { call(&mut session,"open",&[json!({"app":app})]); }
    let mut control = crate::app_control::Controller::new(session.clone());
    let metadata = |name: &str, builtin: bool| json!({"cmd":"metadata","catalog":[],"registry":[
        {"id":"files","name":"Files"},{"id":"notes","name":name,"builtin":builtin},{"id":"calculator","name":"Calculator"}]});
    control.request(&metadata("Notes",false));
    let node = scene::create(scene::VIEW);
    let mut host = Host::new(node,Box::new(Switcher::with_displays(session.clone(),physical_display())),"",None);
    let stale = json!({"width":640,"args":{"nonce":10},"registry":[{"id":"notes","name":"Forged","builtin":true}],"catalog":[]});
    host.facts(stale.clone());
    assert_ne!(host.find_text("Notes"),0);
    assert!(host.deadline().is_none());
    control.request(&metadata("Native Notes",false));
    assert_eq!(host.deadline(),Some(0.));
    host.tick(0.);
    assert_ne!(host.find_text("Native Notes"),0,"catalog rename must preserve the selected group");
    host.facts(stale);
    assert_ne!(host.find_text("Native Notes"),0);
    assert_eq!(host.find_text("Forged"),0);
    control.request(&metadata("Panel",true));
    // No tick between removal of the selected group and its queued commit.
    host.action(&json!({"action":["commit"]}));
    let effects = host.take().unwrap();
    assert_eq!(effects[0]["kind"],"switcher");
    assert_eq!(effects[0]["nonce"],10);
    assert!(effects[0]["focus"].is_null(),"native focus must not return as a JS command");
    assert_ne!(session.reader().read().focused_window(),Some("notes"),"a removed group cannot remain the target");
    assert!(effects[0]["reply"].is_object());
    assert!(session.reader().read().window("notes").is_some(),"only catalog membership changed");
    control.request(&metadata("Notes",false));
    assert!(host.deadline().is_none(),"finished switchers do not spin on catalog changes");
    host.action(&json!({"action":["commit"]}));
    assert!(host.take().is_none());
    drop(host);
    scene::destroy(node);
}

#[test]
fn native_catalog_emptying_closes_the_switcher_once_without_stale_fallback() {
    let mut session = Session::default();
    call(&mut session,"open",&[json!({"app":"notes"})]);
    let mut control = crate::app_control::Controller::new(session.clone());
    control.request(&json!({"cmd":"metadata","catalog":[],"registry":[{"id":"notes","name":"Notes"}]}));
    let node = scene::create(scene::VIEW);
    let mut host = Host::new(node,Box::new(Switcher::with_displays(session,physical_display())),"",None);
    host.facts(json!({"width":640,"args":{"nonce":11},"registry":[{"id":"notes","name":"Forged"}]}));
    assert_ne!(host.find_text("Notes"),0);
    control.request(&json!({"cmd":"metadata","catalog":[],"registry":[{"id":"notes","builtin":true}]}));
    host.tick(0.);
    assert_eq!(host.take(),Some(json!([{"kind":"switcher","nonce":11,"reply":null}])));
    assert_eq!(host.find_text("Forged"),0);
    assert!(host.deadline().is_none());
    control.request(&json!({"cmd":"metadata","catalog":[],"registry":[]}));
    assert!(host.deadline().is_none());
    host.action(&json!({"action":["commit"]}));
    assert!(host.take().is_none());
    drop(host);
    scene::destroy(node);
}

#[test]
fn bound_switcher_focuses_in_one_native_revision_and_restores_fullscreen_origin() {
    for cancel in [false,true] {
        let mut session = Session::default();
        call(&mut session,"setDesktops",&[json!([1,2]),json!(1)]);
        call(&mut session,"open",&[json!({"app":"notes","desktop":2})]);
        call(&mut session,"open",&[json!({"app":"linux","instance":9,"args":{"appId":"game","surface":9}})]);
        call(&mut session,"minimize",&[json!("linux#9")]);
        call(&mut session,"setActiveDesktop",&[json!(1)]);
        let mut control = crate::app_control::Controller::new(session.clone());
        control.request(&json!({"cmd":"metadata","catalog":[],"registry":[{"id":"notes","name":"Notes"},{"id":"linux","builtin":true}]}));
        let node = scene::create(scene::VIEW);
        let mut host = Host::new(node,Box::new(Switcher::with_displays(session.clone(),physical_display())),"",None);
        host.facts(json!({"width":640,"args":{"nonce":12,"origin":"linux#9"}}));
        assert_ne!(host.find_text("Notes"),0);
        let revision = session.reader().revision();
        host.action(&json!({"action":[if cancel { "cancel" } else { "commit" }]}));
        let target = if cancel { "linux#9" } else { "notes" };
        assert_eq!(session.reader().read().focused_window(),Some(target));
        assert_eq!(session.reader().read().active_desktop(),if cancel {1} else {2});
        assert!(!session.reader().read().window(target).unwrap().minimized());
        assert_eq!(session.reader().revision(),revision+1);
        let effects = host.take().unwrap();
        assert_eq!(effects[0]["kind"],"switcher");
        assert_eq!(effects[0]["reply"]["state"]["views"]["front"],target);
        assert!(effects[0].get("focus").is_none());
        host.action(&json!({"action":["commit"]}));
        assert!(host.take().is_none());
        assert_eq!(session.reader().revision(),revision+1);
        drop(host);
        scene::destroy(node);
    }
}

#[test]
fn newer_switcher_generation_prevents_stale_focus_and_drop_keeps_the_current_owner() {
    use super::NativeApp;
    let mut session = Session::default();
    for app in ["files","notes","calculator"] { call(&mut session,"open",&[json!({"app":app})]); }
    let mut control = crate::app_control::Controller::new(session.clone());
    control.request(&json!({"cmd":"metadata","catalog":[],"registry":[{"id":"files","name":"Files"},{"id":"notes","name":"Notes"},{"id":"calculator","name":"Calculator"}]}));
    let sync = |switcher: &mut Switcher, nonce| switcher.request(&json!({"cmd":"sync","args":{"nonce":nonce}}));
    let mut stale = Switcher::with_displays(session.clone(),physical_display());
    sync(&mut stale,20);
    let mut current = Switcher::with_displays(session.clone(),physical_display());
    sync(&mut current,21);
    let revision = session.reader().revision();
    stale.request(&json!({"cmd":"commit"}));
    assert_eq!(session.reader().revision(),revision);
    assert!(sync(&mut stale,20)["effects"][0]["reply"].is_null());
    drop(stale);
    current.request(&json!({"cmd":"commit"}));
    assert_eq!(session.reader().read().focused_window(),Some("notes"));
    assert_eq!(session.reader().revision(),revision+1);
    assert!(sync(&mut current,21)["effects"][0]["reply"].is_object());
    let abandoned = Switcher::with_displays(session.clone(),physical_display());
    drop(abandoned);
    let mut next = Switcher::with_displays(session.clone(),physical_display());
    sync(&mut next,22);
    next.request(&json!({"cmd":"commit"}));
    assert!(sync(&mut next,22)["effects"][0]["reply"].is_object());
}

#[test]
fn physical_displays_reposition_and_resize_without_restarting_selection() {
    use super::NativeApp;
    let mut session = Session::default();
    let registry: Vec<_> = (0..18).map(|n|json!({"id":format!("probe{n}"),"name":format!("Probe {n}")})).collect();
    for app in &registry { call(&mut session,"open",&[json!({"app":app["id"]})]); }
    let mut control = crate::app_control::Controller::new(session.clone());
    control.request(&json!({"cmd":"metadata","registry":registry,"catalog":[]}));
    let source = axia_state::Store::new(vec![json!({"x":-800,"y":0,"w":800,"h":600}),json!({"x":0,"y":20,"w":640,"h":700,"main":true})]);
    let mut switcher = Switcher::with_displays(session,source.reader());
    let input = json!({"cmd":"sync","args":{"nonce":30,"display":{"x":9999,"y":9999,"w":1,"h":1}},"context":{"width":9999,"height":9999}});
    let label = |tree: &Value| tree["children"][0]["children"][1]["children"][0]["label"].clone();
    let columns = |tree: &Value| tree["children"][0]["children"][0]["children"].as_array().unwrap().len();
    let tree = switcher.request(&input)["tree"].clone();
    let selected = label(&tree);
    assert_eq!(tree["style"]["left"],0);
    assert_eq!(tree["style"]["top"],20);
    assert_eq!(tree["style"]["width"],640);
    assert_eq!(columns(&tree),5);
    assert!(!switcher.invalidated());
    source.update(|displays| { displays[0]["main"]=json!(true);displays[0]["x"]=json!(-900);displays[0]["w"]=json!(1000);displays[1]["main"]=json!(false);((),true) });
    assert!(switcher.invalidated());
    let tree = switcher.request(&input)["tree"].clone();
    assert_eq!(tree["style"]["left"],-900);
    assert_eq!(tree["style"]["width"],1000);
    assert_eq!(columns(&tree),8);
    assert_eq!(label(&tree),selected);
    source.update(|displays| { displays.clear();((),true) });
    assert!(switcher.request(&input)["tree"]["children"].is_null(),"empty physical source cannot use forged geometry");
    source.update(|displays| { displays.push(json!({"x":40,"y":60,"w":320,"h":480}));((),true) });
    let tree = switcher.request(&input)["tree"].clone();
    assert_eq!(tree["style"]["left"],40);
    assert_eq!(tree["style"]["height"],480);
    assert_eq!(columns(&tree),2);
    assert_eq!(label(&tree),selected,"reconnection must preserve the selected group");
    switcher.request(&json!({"cmd":"cancel"}));
    source.update(|displays| { displays.clear();((),true) });
    assert!(!switcher.invalidated(),"display changes cannot wake a finished switcher");
}

#[test]
fn physical_display_changes_wake_the_host_without_facts_and_keep_the_focus_target() {
    let mut session = Session::default();
    for app in ["files","notes","calculator"] { call(&mut session,"open",&[json!({"app":app})]); }
    let mut control = crate::app_control::Controller::new(session.clone());
    control.request(&json!({"cmd":"metadata","registry":[{"id":"files","name":"Files"},{"id":"notes","name":"Notes"},{"id":"calculator","name":"Calculator"}],"catalog":[]}));
    let source = axia_state::Store::new(vec![json!({"x":0,"y":0,"w":640,"h":700})]);
    let node = scene::create(scene::VIEW);
    let mut host = Host::new(node,Box::new(Switcher::with_displays(session.clone(),source.reader())),"",None);
    host.facts(json!({"width":9999,"args":{"nonce":31}}));
    assert_ne!(host.find_text("Notes"),0);
    source.update(|displays| { displays.clear();((),true) });
    assert_eq!(host.deadline(),Some(0.));
    host.tick(0.);
    assert_eq!(host.find_text("Notes"),0);
    assert!(host.take().is_none(),"temporary disconnection does not finish selection");
    source.update(|displays| { displays.push(json!({"x":40,"y":0,"w":1000,"h":600}));((),true) });
    host.tick(0.);
    assert_ne!(host.find_text("Notes"),0);
    assert!(host.deadline().is_none());
    host.action(&json!({"action":["commit"]}));
    assert_eq!(session.reader().read().focused_window(),Some("notes"));
    assert!(host.take().is_some());
    source.update(|displays| { displays.clear();((),true) });
    assert!(host.deadline().is_none());
    drop(host);
    scene::destroy(node);
}
