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
