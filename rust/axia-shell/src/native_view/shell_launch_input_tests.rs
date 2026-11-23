use super::*;

fn launches(session: &Session) -> Vec<Value> {
    session.take_service_effects().unwrap_or(json!([])).as_array().unwrap().iter()
        .filter(|effect|effect["kind"]=="shellLaunch").cloned().collect()
}

#[test]
fn physical_launches_follow_native_profile_layout_and_own_repeats() {
    let (home,mut session,_,root) = input_fixture("launch-layout","linux");
    let revision = session.reader().revision();
    for (code,key,flags,app) in [(18,"e",16,"files"),(23,"i",16,"settings"),(1,"Escape",3,"monitor")] {
        assert_eq!(session.physical_key(code,1,key,flags,true),1);
        assert_eq!(session.physical_key(code,2,key,flags,true),1);
        assert_eq!(session.physical_key(code,0,key,flags,true),1);
        let effects = launches(&session);
        assert_eq!(effects.len(),1,"one launch per fresh press");
        assert_eq!(effects[0]["app"],app);
        assert_eq!(session.reader().revision(),revision,"loading/mounting remains an adapter effect");
    }
    assert_eq!(session.physical_key(125,0,"Meta",3,true),3);
    assert_eq!(session.physical_key(29,0,"Control",1,true),3);
    assert_eq!(session.physical_key(42,0,"Shift",0,true),3);
    assert_eq!(session.physical_key(23,1,"e",16,true),1,"letters follow the keyboard layout");
    assert_eq!(launches(&session)[0]["app"],"files");
    session.physical_key(23,0,"e",16,true);
    assert_eq!(session.physical_key(18,1,"ж",16,true),1,"non-Latin layouts fall back to position");
    assert_eq!(launches(&session)[0]["app"],"files");
    session.physical_key(18,0,"ж",16,true);
    for (code,key,flags,enabled) in [(18,"e",17,true),(23,"i",20,true),(1,"Escape",2,true),(18,"e",16,false),(18,";",16,true)] {
        assert_eq!(session.physical_key(code,1,key,flags,enabled),0);
    }
    crate::settings::with(&home.0,|settings|settings.command(&json!({"cmd":"patch","patch":{"shortcutProfile":"macos"}})));
    assert_eq!(session.physical_key(18,1,"e",16,true),0);
    assert_eq!(session.physical_key(1,1,"Escape",3,true),0);
    assert!(launches(&session).is_empty());
    scene::destroy(root);
}

#[test]
fn pending_launches_are_discarded_with_their_scene_and_switcher_has_priority() {
    let (_home,mut session,mut control,root) = input_fixture("launch-root","linux");
    assert_eq!(session.physical_key(18,1,"e",16,true),1);
    assert_eq!(session.physical_key(18,0,"e",16,true),1);
    let next = scene::create(scene::VIEW); scene::set_test_root(next);
    control.request(&json!({"cmd":"shellRoot","node":next}));
    assert!(launches(&session).is_empty(),"old root cannot launch into a new scene");
    control.request(&json!({"cmd":"switcherPrepare","hold":8}));
    assert_eq!(session.physical_key(18,1,"e",16,true),1);
    assert!(launches(&session).is_empty(),"an open switcher owns the key");
    control.request(&json!({"cmd":"switcherCancel"}));
    session.physical_key(18,0,"e",16,true);
    assert_eq!(session.physical_key(18,1,"e",16,true),1);
    scene::destroy(next);
    assert!(launches(&session).is_empty(),"dead root invalidates queued effects before JS runs");
    assert_eq!(session.physical_key(23,1,"i",16,true),0);
    scene::destroy(root);
}
