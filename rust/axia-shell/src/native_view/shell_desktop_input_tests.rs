use super::*;

fn call(session: &mut Session, command: &str, values: &[Value]) {
    session.call(command,&[vec![json!({})],values.to_vec()].concat()).unwrap();
}
fn patch(home: &InputHome, value: Value) {
    crate::settings::with(&home.0,|settings|settings.command(&json!({"cmd":"patch","patch":value})));
}
fn locale(home: &InputHome, dir: &str) {
    let home = std::ffi::CString::new(home.0.as_str()).unwrap();
    let input = std::ffi::CString::new(json!({"cmd":"patch","value":{"lang":"en","dir":dir}}).to_string()).unwrap();
    unsafe { let reply = crate::locale::axia_locale_request(home.as_ptr(),input.as_ptr()); assert!(!reply.is_null()); drop(std::ffi::CString::from_raw(reply)); }
}
fn press(session: &mut Session, code: i32, key: &str, flags: i32) {
    assert_eq!(session.physical_key(code,1,key,flags,true),1);
    assert_eq!(session.physical_key(code,0,key,flags,true),1);
}
fn hud(control: &mut Controller) -> u32 { control.request(&json!({"cmd":"desktopHudStatus"}))["handle"].as_u64().unwrap() as u32 }
fn text(handle: u32) -> String { super::super::super::with_host(handle,|host|host.text()).unwrap_or_default() }

#[test]
fn physical_desktop_arrows_read_current_preferences_locale_and_model() {
    let (home,mut session,mut control,root) = input_fixture("desktop-arrows","macos");
    control.bind_settings(&home.0); locale(&home,"ltr");
    call(&mut session,"setDesktops",&[json!([10,20,30]),json!(20)]);
    let reader = session.reader(); let revision = reader.revision();
    assert_eq!(session.physical_key(105,1,"ArrowLeft",2,true),1);
    assert_eq!(reader.read().active_desktop(),10); assert_eq!(reader.revision(),revision+1);
    assert_eq!(session.physical_key(105,2,"ArrowLeft",2,true),1);
    assert_eq!(reader.revision(),revision+1);
    assert_eq!(session.physical_key(29,0,"Control",0,true),3);
    assert_eq!(session.physical_key(105,0,"ArrowLeft",0,true),1);
    patch(&home,json!({"desktopArrows":false}));
    assert_eq!(session.physical_key(106,1,"ArrowRight",2,true),0);
    press(&mut session,106,"ArrowRight",18);
    assert_eq!(reader.read().active_desktop(),20);
    locale(&home,"rtl");
    press(&mut session,105,"ArrowLeft",18);
    assert_eq!(reader.read().active_desktop(),30);
    patch(&home,json!({"shortcutProfile":"linux","desktopArrows":true}));
    assert_eq!(session.physical_key(105,1,"ArrowLeft",2,true),0);
    press(&mut session,106,"ArrowRight",18);
    assert_eq!(reader.read().active_desktop(),20);
    assert_eq!(session.physical_key(106,1,"ArrowRight",19,true),0);
    assert_eq!(session.physical_key(106,1,"ArrowRight",18,false),0);
    scene::destroy(root); tick(&mut session);
    assert_eq!(session.physical_key(106,1,"ArrowRight",18,true),0);
}

#[test]
fn linux_add_and_close_use_one_transaction_and_revalidate_current_windows() {
    let (home,mut session,mut control,root) = input_fixture("desktop-add-close","linux");
    control.bind_settings(&home.0); locale(&home,"ltr");
    call(&mut session,"setDesktops",&[json!([10,20]),json!(20)]);
    let reader = session.reader(); let revision = reader.revision();
    assert_eq!(session.physical_key(32,1,"d",18,true),1);
    assert_eq!(reader.read().desktops(),&[10,20,21]); assert_eq!(reader.read().active_desktop(),21);
    assert_eq!(reader.revision(),revision+1);
    assert_eq!(session.physical_key(32,2,"d",18,true),1); assert_eq!(reader.revision(),revision+1);
    session.physical_key(32,0,"d",18,true);
    call(&mut session,"open",&[json!({"app":"notes","instance":"new","desktop":21})]);
    let revision = reader.revision();
    press(&mut session,62,"F4",18);
    assert_eq!(reader.read().desktops(),&[10,20]); assert_eq!(reader.read().active_desktop(),20);
    assert_eq!(reader.read().window("notes#new").unwrap().desktop(),20);
    assert_eq!(reader.revision(),revision+1);
    patch(&home,json!({"shortcutProfile":"macos"}));
    assert_eq!(session.physical_key(32,1,"d",18,true),0);
    assert_eq!(session.physical_key(62,1,"F4",18,true),0);
    scene::destroy(root);
}

#[test]
fn native_hud_mount_survives_services_and_protects_sibling_and_recycled_nodes() {
    let (_home,mut session,mut control,root) = input_fixture("desktop-hud-owner","macos");
    let weak = session.downgrade(); let handle = hud(&mut control); assert!(handle>0);
    let status = control.request(&json!({"cmd":"desktopHudStatus"}));
    let node = status["node"].as_i64().unwrap() as i32;
    let layer = control.request(&json!({"cmd":"shellLayer","z":80}))["node"].as_i64().unwrap() as i32;
    let sibling = scene::create(scene::VIEW); scene::insert(layer,sibling,0);
    control.request(&json!({"cmd":"switcherPrepare","hold":8})); tick(&mut session);
    control.request(&json!({"cmd":"switcherCancel"})); tick(&mut session);
    assert_eq!(hud(&mut control),handle); assert!(scene::exists(sibling));
    scene::destroy(node); let recycled = scene::create(scene::VIEW); assert_eq!(recycled,node);
    scene::insert(root,recycled,0); tick(&mut session);
    let next = hud(&mut control); assert!(next>handle); assert!(scene::exists(recycled));
    assert!(super::super::super::with_host(handle,|_|()).is_none());
    drop(control); drop(session);
    assert!(weak.upgrade().is_none()); assert!(super::super::super::with_host(next,|_|()).is_none());
    assert!(scene::exists(sibling)); assert!(scene::exists(recycled)); scene::destroy(root);
}

#[test]
fn native_boundary_hud_works_for_physical_and_explicit_commands_without_js() {
    let (home,mut session,mut control,root) = input_fixture("desktop-boundary","macos");
    control.bind_settings(&home.0); locale(&home,"ltr"); tick(&mut session);
    let handle = hud(&mut control); assert!(text(handle).is_empty());
    call(&mut session,"setDesktops",&[json!([10,20]),json!(10)]); tick(&mut session);
    let revision = session.reader().revision();
    press(&mut session,105,"ArrowLeft",2);
    assert_eq!(session.reader().revision(),revision);
    assert!(text(handle).contains("Desktop 1"));
    assert!(super::super::super::with_host(handle,|host|host.deadline()).flatten().is_some());
    call(&mut session,"setOverview",&[json!(true)]); tick(&mut session);
    assert!(text(handle).is_empty());
    assert!(super::super::super::with_host(handle,|host|host.deadline()).flatten().is_none());
    call(&mut session,"setOverview",&[json!(false)]); tick(&mut session);
    assert!(text(handle).is_empty());
    control.request(&json!({"cmd":"desktopCommand","action":"step","step":-1}));
    assert!(text(handle).contains("Desktop 1"));
    call(&mut session,"setDesktops",&[json!([10]),json!(10)]); tick(&mut session);
    control.request(&json!({"cmd":"desktopSession","action":"arrow","direction":"left"}));
    assert!(text(handle).is_empty());
    scene::destroy(root); tick(&mut session);
    assert_eq!(hud(&mut control),0);
    assert!(super::super::super::with_host(handle,|_|()).is_none());
}
