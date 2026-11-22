use super::*;
use serde_json::json;

#[test]
fn arrangement_and_bring_all_pick_live_windows_and_commit_once() {
    let mut session = Session::default();
    session
        .call("setDesktops", &[json!({}), json!([1, 2]), json!(1)])
        .unwrap();
    for input in [
        json!({"app":"notes","instance":"a"}),
        json!({"app":"notes","instance":"b"}),
        json!({"app":"notes","instance":"away","desktop":2}),
        json!({"app":"calculator"}),
        json!({"app":"panel"}),
    ] {
        session.call("open", &[json!({}), input]).unwrap();
    }
    let mut controller = Controller::new(session.clone());
    controller.request(&json!({"cmd":"metadata","registry":[{"id":"notes"},{"id":"calculator"},{"id":"panel","builtin":true}],"catalog":[]}));
    let reader = session.reader();
    let revision = reader.revision();
    controller.request(&json!({"cmd":"windowCommand","action":"arrange","id":"calculator","layout":"left-right","ids":["panel","notes#away"],
        "facts":{"displays":[{"id":0,"x":0,"y":0,"w":640,"h":700,"work":{"x":0,"y":0,"w":640,"h":600}}]}}));
    assert_eq!(reader.revision(), revision + 1);
    {
        let state = reader.read();
        assert_eq!(state.window("calculator").unwrap().snapped(), Some("left"));
        assert_eq!(state.window("notes#b").unwrap().snapped(), Some("right"));
        assert!(state.window("notes#a").unwrap().snapped().is_none());
        assert!(state.window("notes#away").unwrap().snapped().is_none());
        assert!(state.window("panel").unwrap().snapped().is_none());
    }
    let revision = reader.revision();
    controller.request(&json!({"cmd":"windowCommand","action":"bringAll","id":"notes#a"}));
    assert_eq!(reader.read().focused_window(), Some("notes#a"));
    assert_eq!(reader.revision(), revision + 1);
    let revision = reader.revision();
    for anchor in ["missing", "panel", "notes#away"] {
        controller.request(
            &json!({"cmd":"windowCommand","action":"arrange","id":anchor,"layout":"left-right"}),
        );
        assert_eq!(reader.revision(), revision);
    }
}

#[test]
fn switcher_preparation_resolves_current_surface_and_cancel_revalidates_the_origin() {
    let mut session = Session::default();
    session.call("setDesktops",&[json!({}),json!([1,2]),json!(1)]).unwrap();
    session.call("open",&[json!({}),json!({"app":"notes"})]).unwrap();
    session.call("open",&[json!({}),json!({"app":"linux","instance":7,"args":{"surface":99,"appId":"game"}})]).unwrap();
    let mut control = Controller::new(session.clone());
    let reader = session.reader();
    let revision = reader.revision();
    let normal = control.request(&json!({"cmd":"switcherPrepare","fullscreen":false,"surface":99}));
    assert_eq!(normal["result"],json!({"opened":true,"origin":""}));
    assert_eq!(reader.revision(),revision);
    for surface in [0,-1,7,100] {
        let _probe = super::switcher_source::set(true,surface);
        assert_eq!(control.request(&json!({"cmd":"switcherPrepare","fullscreen":true,"surface":surface}))["result"]["opened"],false);
        assert_eq!(reader.revision(),revision);
    }
    let _probe = super::switcher_source::set(true,99);
    let reply = control.request(&json!({"cmd":"switcherPrepare","fullscreen":false,"surface":100,"origin":"notes","windows":[],"facts":{"active":2}}));
    assert_eq!(reply["result"],json!({"opened":true,"origin":"linux#7"}));
    assert!(reader.read().window("linux#7").unwrap().minimized());
    assert_eq!(reader.revision(),revision+1);
    assert_eq!(control.request(&json!({"cmd":"switcherPrepare","fullscreen":true,"surface":99}))["result"]["opened"],false,"an already minimized origin is stale");
    assert_eq!(reader.revision(),revision+1);
    session.call("setActiveDesktop",&[json!({}),json!(2)]).unwrap();
    let revision = reader.revision();
    control.request(&json!({"cmd":"switcherCancel","origin":"linux#7"}));
    assert_eq!(reader.revision(),revision+1);
    assert_eq!(reader.read().focused_window(),Some("linux#7"));
    assert_eq!(reader.read().active_desktop(),1);
    assert!(!reader.read().window("linux#7").unwrap().minimized());
    session.call("close",&[json!({}),json!("linux#7")]).unwrap();
    let revision = reader.revision();
    control.request(&json!({"cmd":"switcherCancel","origin":"linux#7"}));
    assert_eq!(reader.revision(),revision,"closed origins cannot change focus or recreate a window");
    for invalid in [json!({"cmd":"switcherCancel"})] {
        assert_eq!(control.request(&invalid)["error"],"app.invalidRequest");
        assert_eq!(reader.revision(),revision);
    }
    assert_eq!(Controller::default().request(&json!({"cmd":"switcherPrepare","fullscreen":false,"surface":0}))["error"],"app.invalidRequest");
}

#[test]
fn switcher_preparation_uses_the_native_probe_even_when_caller_facts_disagree() {
    let mut session = Session::default();
    session.call("open",&[json!({}),json!({"app":"linux","instance":12,"args":{"surface":97}})]).unwrap();
    let mut control = Controller::new(session.clone());
    let revision = session.reader().revision();
    assert_eq!(control.request(&json!({"cmd":"switcherPrepare","fullscreen":true,"surface":97}))["result"],json!({"opened":true,"origin":""}));
    assert_eq!(session.reader().revision(),revision);
    {
        let _probe = super::switcher_source::set(true,0);
        assert_eq!(control.request(&json!({"cmd":"switcherPrepare","fullscreen":false,"surface":97}))["result"]["opened"],false);
        assert_eq!(session.reader().revision(),revision);
    }
    {
        let _probe = super::switcher_source::set(true,97);
        assert_eq!(control.request(&json!({"cmd":"switcherPrepare","fullscreen":false,"surface":0}))["result"]["origin"],"linux#12");
        assert_eq!(session.reader().revision(),revision+1);
    }
}
