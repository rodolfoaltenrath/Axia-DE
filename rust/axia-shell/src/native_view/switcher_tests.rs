use super::{switcher::Switcher, Host};
use crate::{runtime::native as scene, winmodel::Session};
use serde_json::{json, Value};

fn call(state: &mut Session, op: &str, args: &[Value]) {
    state
        .call(op, &[vec![json!({})], args.to_vec()].concat())
        .unwrap();
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
    let mut host = Host::new(node,Box::new(Switcher::with_session(session.clone())),"",None);
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
    assert_ne!(effects[0]["focus"],"notes","a removed group cannot remain the target");
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
    let mut host = Host::new(node,Box::new(Switcher::with_session(session)),"",None);
    host.facts(json!({"width":640,"args":{"nonce":11},"registry":[{"id":"notes","name":"Forged"}]}));
    assert_ne!(host.find_text("Notes"),0);
    control.request(&json!({"cmd":"metadata","catalog":[],"registry":[{"id":"notes","builtin":true}]}));
    host.tick(0.);
    assert_eq!(host.take(),Some(json!([{"kind":"switcher","nonce":11,"focus":null}])));
    assert_eq!(host.find_text("Forged"),0);
    assert!(host.deadline().is_none());
    control.request(&json!({"cmd":"metadata","catalog":[],"registry":[]}));
    assert!(host.deadline().is_none());
    host.action(&json!({"action":["commit"]}));
    assert!(host.take().is_none());
    drop(host);
    scene::destroy(node);
}
