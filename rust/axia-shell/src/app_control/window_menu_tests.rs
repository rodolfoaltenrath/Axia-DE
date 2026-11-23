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
    assert_eq!(normal["result"]["opened"],true);
    control.request(&json!({"cmd":"switcherCancel","nonce":normal["result"]["nonce"]}));
    assert_eq!(reader.revision(),revision);
    for surface in [0,-1,7,100] {
        let _probe = super::switcher_source::set(true,surface);
        assert_eq!(control.request(&json!({"cmd":"switcherPrepare","fullscreen":true,"surface":surface}))["result"]["opened"],false);
        assert_eq!(reader.revision(),revision);
    }
    let _probe = super::switcher_source::set(true,99);
    let reply = control.request(&json!({"cmd":"switcherPrepare","fullscreen":false,"surface":100,"origin":"notes","windows":[],"facts":{"active":2}}));
    assert_eq!(session.switcher_reader().read().as_ref().unwrap().origin,"linux#7");
    let nonce = reply["result"]["nonce"].clone();
    assert!(reader.read().window("linux#7").unwrap().minimized());
    assert_eq!(reader.revision(),revision+1);
    assert_eq!(control.request(&json!({"cmd":"switcherPrepare","fullscreen":true,"surface":99}))["result"]["opened"],false,"an already minimized origin is stale");
    assert_eq!(reader.revision(),revision+1);
    session.call("setActiveDesktop",&[json!({}),json!(2)]).unwrap();
    let revision = reader.revision();
    control.request(&json!({"cmd":"switcherCancel","nonce":nonce}));
    assert_eq!(reader.revision(),revision+1);
    assert_eq!(reader.read().focused_window(),Some("linux#7"));
    assert_eq!(reader.read().active_desktop(),1);
    assert!(!reader.read().window("linux#7").unwrap().minimized());
    let nonce = control.request(&json!({"cmd":"switcherPrepare"}))["result"]["nonce"].clone();
    session.call("close",&[json!({}),json!("linux#7")]).unwrap();
    let revision = reader.revision();
    control.request(&json!({"cmd":"switcherCancel","nonce":nonce}));
    assert_eq!(reader.revision(),revision,"closed origins cannot change focus or recreate a window");
    for invalid in [json!({"cmd":"switcherCancel","nonce":false})] {
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
    let normal = control.request(&json!({"cmd":"switcherPrepare","fullscreen":true,"surface":97}));
    assert_eq!(session.switcher_reader().read().as_ref().unwrap().origin,"");
    control.request(&json!({"cmd":"switcherCancel","nonce":normal["result"]["nonce"]}));
    assert_eq!(session.reader().revision(),revision);
    {
        let _probe = super::switcher_source::set(true,0);
        assert_eq!(control.request(&json!({"cmd":"switcherPrepare","fullscreen":false,"surface":97}))["result"]["opened"],false);
        assert_eq!(session.reader().revision(),revision);
    }
    {
        let _probe = super::switcher_source::set(true,97);
        assert_eq!(control.request(&json!({"cmd":"switcherPrepare","fullscreen":false,"surface":0}))["result"]["opened"],true);
        assert_eq!(session.switcher_reader().read().as_ref().unwrap().origin,"linux#12");
        assert_eq!(session.reader().revision(),revision+1);
    }
}

#[test]
fn switcher_opening_survives_controller_recreation_and_rejects_stale_cancel() {
    let mut session = Session::default();
    session.call("open",&[json!({}),json!({"app":"notes"})]).unwrap();
    let opening = session.switcher_reader();
    let changes = opening.subscribe();
    changes.take();
    let mut control = Controller::new(session.clone());
    for invalid in [json!({"hold":-1}),json!({"hold":2147483648u64}),json!({"back":1})] {
        let mut request = invalid;
        request["cmd"] = json!("switcherPrepare");
        assert_eq!(control.request(&request)["error"],"app.invalidRequest");
        assert!(opening.read().is_none());
    }
    let first = control.request(&json!({"cmd":"switcherPrepare","hold":8,"back":true,"nonce":500,"origin":"forged"}));
    let nonce = first["result"]["nonce"].as_u64().unwrap();
    assert_ne!(nonce,500);
    assert!(changes.take().is_some());
    assert_eq!(opening.read().as_ref().unwrap().hold,8);
    assert!(opening.read().as_ref().unwrap().back);
    assert!(opening.read().as_ref().unwrap().origin.is_empty());
    drop(control);
    let mut control = Controller::new(session.clone());
    assert_eq!(control.request(&json!({"cmd":"switcherPrepare"}))["result"]["opened"],false);
    assert!(changes.take().is_none());
    assert_eq!(control.request(&json!({"cmd":"switcherCancel","nonce":nonce}))["result"],true);
    let second = control.request(&json!({"cmd":"switcherPrepare","hold":4}));
    let next = second["result"]["nonce"].as_u64().unwrap();
    assert!(next>nonce);
    changes.take();
    let revision = session.reader().revision();
    assert_eq!(control.request(&json!({"cmd":"switcherCancel","nonce":nonce,"origin":"notes"}))["result"],false);
    assert_eq!(opening.read().as_ref().unwrap().nonce,next);
    assert!(changes.take().is_none());
    assert_eq!(session.reader().revision(),revision);
}

#[test]
fn switcher_keys_use_native_hold_and_identity_without_mutating_the_model() {
    let session = Session::default();
    let mut control = Controller::new(session.clone());
    let first = control.request(&json!({"cmd":"switcherPrepare","hold":8}));
    let nonce = first["result"]["nonce"].as_u64().unwrap();
    assert!(first["result"].get("hold").is_none());
    // Recreating the transport cannot lose the held modifiers or opening.
    let mut control = Controller::new(session.clone());
    let revision = session.reader().revision();
    let opening = session.switcher_reader();
    let opening_revision = opening.revision();
    let key = |control: &mut Controller, key, mods, down, shift| control.request(&json!({
        "cmd":"switcherKey","nonce":nonce,"key":key,"mods":mods,"down":down,"shift":shift,
        "hold":2,"origin":"forged","back":true,
    }));
    for (name, mods, down, shift, action) in [
        ("Tab",8,true,false,json!(["move",1])),
        ("Tab",9,true,true,json!(["move",-1])),
        ("ArrowRight",9,true,true,json!(["move",1])),
        ("ArrowLeft",8,true,false,json!(["move",-1])),
        ("Enter",8,true,false,json!(["commit"])),
        ("Escape",8,true,false,json!(["cancel"])),
        ("Tab",8,false,false,Value::Null),
        ("x",8,true,false,Value::Null),
        ("Shift",8,false,false,Value::Null),
        ("Meta",0,false,false,json!(["commit"])),
        ("Alt",2,false,false,json!(["commit"])),
    ] {
        assert_eq!(key(&mut control,name,mods,down,shift),json!({"consumed":true,"action":action}));
    }
    for (field,value) in [("nonce",json!(-1)),("key",Value::Null),("mods",json!(-1)),
        ("mods",json!(2147483648u64)),("down",json!(1)),("shift",Value::Null)] {
        let mut input = json!({"cmd":"switcherKey","nonce":nonce,"key":"Tab","mods":8,"down":true,"shift":false});
        input[field] = value;
        assert_eq!(control.request(&input)["error"],"app.invalidRequest");
    }
    assert_eq!(session.reader().revision(),revision);
    assert_eq!(opening.revision(),opening_revision);
    control.request(&json!({"cmd":"switcherCancel","nonce":nonce}));
    assert_eq!(key(&mut control,"Tab",8,true,false),json!({"consumed":false}));
    control.request(&json!({"cmd":"switcherPrepare","hold":2}));
    assert_eq!(key(&mut control,"Meta",0,false,false),json!({"consumed":false}));
    let current = opening.read().as_ref().unwrap().nonce;
    assert_eq!(control.request(&json!({"cmd":"switcherKey","nonce":current,"key":"Alt","mods":0,"down":false,"shift":false}))["action"],json!(["commit"]));
}
