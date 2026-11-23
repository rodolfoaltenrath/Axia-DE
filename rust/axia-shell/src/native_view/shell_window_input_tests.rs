use super::*;

fn call(session: &mut Session, command: &str, values: &[Value]) {
    session.call(command,&[vec![json!({})],values.to_vec()].concat()).unwrap();
}
fn focus(session: &mut Session, id: &str) { call(session,"focus",&[json!(id)]); }
fn minimized(session: &Session, id: &str) -> bool { session.reader().read().window(id).unwrap().minimized() }
fn press(session: &mut Session, code: i32, key: &str, flags: i32) {
    assert_eq!(session.physical_key(code,1,key,flags,true),1);
    assert_eq!(session.physical_key(code,0,key,flags,true),1);
}
#[test]
fn physical_group_commands_share_current_membership_and_publish_one_revision() {
    let (_home,mut session,mut control,root) = input_fixture("window-groups","macos");
    call(&mut session,"setDesktops",&[json!([1,2]),json!(1)]);
    for input in [json!({"app":"notes","instance":"a"}),json!({"app":"notes","instance":"b"}),
        json!({"app":"notes","instance":"away","desktop":2}),json!({"app":"panel"})] {
        call(&mut session,"open",&[input]);
    }
    control.request(&json!({"cmd":"metadata","catalog":[],"registry":[{"id":"notes"},{"id":"files"},
        {"id":"calculator"},{"id":"panel","builtin":true}]}));
    focus(&mut session,"notes#b");
    let reader = session.reader(); let revision = reader.revision();
    assert_eq!(session.physical_key(41,1,"`",16,true),1);
    assert_eq!(reader.read().focused_window(),Some("notes"));
    assert_eq!(reader.revision(),revision+1);
    assert_eq!(session.physical_key(41,2,"`",16,true),1);
    assert_eq!(reader.revision(),revision+1,"a held shortcut must not repeat the mutation");
    session.physical_key(41,0,"`",16,true);
    press(&mut session,41,"~",17);
    assert_eq!(reader.read().focused_window(),Some("notes#b"));
    session.physical_key(42,0,"Shift",16,true);
    let revision = reader.revision();
    press(&mut session,50,"m",16);
    assert!(minimized(&session,"notes#b"));
    assert_eq!(reader.revision(),revision+1);
    assert!(!minimized(&session,"notes#a"));
    focus(&mut session,"notes#a"); let revision = reader.revision();
    press(&mut session,35,"h",16);
    assert!(["notes","notes#a","notes#b","notes#away"].iter().all(|id|minimized(&session,id)));
    assert!(!minimized(&session,"calculator"));
    assert!(!minimized(&session,"panel"));
    assert_eq!(reader.revision(),revision+1);
    focus(&mut session,"calculator"); let revision = reader.revision();
    press(&mut session,35,"h",20);
    assert!(minimized(&session,"files"));
    assert!(!minimized(&session,"calculator"));
    assert!(!minimized(&session,"panel"));
    assert_eq!(reader.revision(),revision+1);
    let effects = session.take_service_effects().unwrap();
    assert!(effects.as_array().unwrap().iter().any(|effect|effect["kind"]=="windowModel" && effect["reply"]["state"].is_object()));
    scene::destroy(root);
}

#[test]
fn physical_show_desktop_keeps_memory_per_desktop_and_revalidates_saved_members() {
    let (home,mut session,mut control,root) = input_fixture("desktop-memory","macos");
    call(&mut session,"setDesktops",&[json!([1,2]),json!(1)]);
    call(&mut session,"open",&[json!({"app":"panel"})]);
    call(&mut session,"open",&[json!({"app":"notes","instance":"pip"})]);
    call(&mut session,"setPip",&[json!("notes#pip"),json!({"x":5,"y":5,"w":160,"h":100})]);
    control.request(&json!({"cmd":"metadata","catalog":[],"registry":[{"id":"notes"},{"id":"files"},
        {"id":"calculator"},{"id":"panel","builtin":true}]}));
    call(&mut session,"minimize",&[json!("calculator")]); focus(&mut session,"notes");
    press(&mut session,87,"F11",16);
    assert!(minimized(&session,"notes") && minimized(&session,"files"));
    assert!(!minimized(&session,"panel") && !minimized(&session,"notes#pip"));
    call(&mut session,"close",&[json!("files")]);
    call(&mut session,"moveToDesktop",&[json!("notes"),json!(2)]);
    press(&mut session,87,"F11",16);
    assert!(session.reader().read().window("files").is_none());
    assert!(minimized(&session,"notes") && minimized(&session,"calculator"));
    assert!(!minimized(&session,"panel") && !minimized(&session,"notes#pip"));
    crate::settings::with(&home.0,|settings|settings.command(&json!({"cmd":"patch","patch":{"shortcutProfile":"linux"}})));
    call(&mut session,"setActiveDesktop",&[json!(2)]);
    call(&mut session,"open",&[json!({"app":"notes","instance":"other","desktop":2})]);
    press(&mut session,32,"d",16);
    assert!(minimized(&session,"notes#other"));
    press(&mut session,32,"d",16);
    assert!(!minimized(&session,"notes#other"));
    assert!(minimized(&session,"notes"));
    scene::destroy(root);
}

#[test]
fn native_window_keys_follow_layout_and_native_profile_and_balance_late_releases() {
    let (home,mut session,_,root) = input_fixture("window-layout","macos");
    call(&mut session,"open",&[json!({"app":"notes","instance":"second"})]);
    focus(&mut session,"notes");
    press(&mut session,50,"h",16); // layout's h, even though its physical position is M.
    assert!(minimized(&session,"notes") && minimized(&session,"notes#second"));
    focus(&mut session,"notes"); focus(&mut session,"notes#second");
    assert_eq!(session.physical_key(50,1,"ь",16,true),1); // non-Latin layout falls back to position.
    assert!(minimized(&session,"notes#second") && !minimized(&session,"notes"));
    let revision = session.reader().revision();
    assert_eq!(session.physical_key(125,0,"Meta",0,true),3);
    assert_eq!(session.physical_key(50,2,"m",0,true),1);
    assert_eq!(session.physical_key(50,0,"m",0,true),1);
    assert_eq!(session.reader().revision(),revision);
    for (key,flags,enabled) in [(";",16,true),("m",18,true),("m",16,false)] {
        assert_eq!(session.physical_key(50,1,key,flags,enabled),0);
        assert_eq!(session.reader().revision(),revision);
    }
    crate::settings::with(&home.0,|settings|settings.command(&json!({"cmd":"patch","patch":{"shortcutProfile":"linux"}})));
    focus(&mut session,"notes#second");
    assert_eq!(session.physical_key(50,1,"m",16,true),0);
    assert_eq!(session.physical_key(35,1,"h",20,true),0);
    press(&mut session,35,"h",16);
    assert!(minimized(&session,"notes#second") && !minimized(&session,"notes"));
    focus(&mut session,"notes#second");
    press(&mut session,41,"`",4);
    assert_eq!(session.reader().read().focused_window(),Some("notes"));
    scene::destroy(root);
    assert_eq!(session.physical_key(35,1,"h",16,true),0);
}

#[test]
fn explicit_open_after_native_profile_change_keeps_the_new_opening() {
    let (home,mut session,mut control,root) = input_fixture("window-profile-open","macos");
    crate::settings::with(&home.0,|settings|settings.command(&json!({"cmd":"patch","patch":{"shortcutProfile":"linux"}})));
    let reply = control.request(&json!({"cmd":"switcherPrepare","hold":8}));
    assert_eq!(reply["result"]["opened"],true);
    tick(&mut session);
    assert_eq!(control.request(&json!({"cmd":"switcherStatus"}))["nonce"],reply["result"]["nonce"]);
    assert_ne!(handle(&mut control),0);
    scene::destroy(root);
}

#[test]
fn open_switcher_precedes_window_shortcuts() {
    let (_home,mut session,mut control,root) = input_fixture("window-priority","macos");
    press(&mut session,15,"Tab",16); tick(&mut session);
    let revision = session.reader().revision();
    press(&mut session,50,"m",16);
    assert_eq!(session.reader().revision(),revision);
    assert!(!minimized(&session,"calculator"));
    assert_eq!(control.request(&json!({"cmd":"switcherStatus"}))["opened"],true);
    press(&mut session,1,"Escape",16);
    press(&mut session,50,"m",16);
    assert!(minimized(&session,"calculator"));
    scene::destroy(root);
}
