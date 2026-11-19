use super::*;
use axia_native::NativeApp;
fn metadata() -> Value {
    json!({"registry":[{"id":"notes","name":"Notes"},{"id":"files","name":"Files"},
    {"id":"panel","builtin":true},{"id":"hidden","hidden":true},{"id":"live","live":true},{"id":"pkg","package":true}],
    "catalog":[{"id":"desktop:firefox","kind":"linux","desktopId":"firefox","shortName":"Firefox"}]})
}
fn entry(id: &str) -> Value {
    json!({"id":id,"f":{"x":40,"y":60,"w":300,"h":220}})
}
fn input(action: &str, mode: &str, saved: Value) -> Value {
    json!({"action":action,"settings":{"sessionRestore":mode,"sessionSaved":saved},
    "model":{"snap":true,"displays":[{"id":0,"x":0,"y":0,"w":1000,"h":700,"main":true,"work":{"x":0,"y":20,"w":1000,"h":650}}]}})
}
fn ops(reply: &Value, op: &str) -> Vec<Value> {
    rows(&reply["effects"])
        .iter()
        .filter(|e| e["op"] == op)
        .map(|e| e["value"].clone())
        .collect()
}
fn open(session: &mut Session, app: &str, instance: &str, args: Value) -> String {
    session.call("open",&[json!({}),json!({"app":app,"instance":instance,"args":args,"frame":{"x":30,"y":50,"w":300,"h":220}})]).unwrap()["result"].as_str().unwrap().to_owned()
}

#[test]
fn owner_survives_view_remount_and_repeated_start_and_read_only_queries_are_silent() {
    let mut session = Session::default();
    let owner = session.restore_service();
    let cloned = session.clone().restore_service();
    assert!(std::rc::Rc::ptr_eq(&owner, &cloned));
    assert!(!std::rc::Rc::ptr_eq(
        &owner,
        &Session::default().restore_service()
    ));
    let now = Instant::now();
    let start = input(
        "start",
        "ask",
        json!({"clean":false,"windows":[entry("app:notes")]}),
    );
    let first = owner
        .borrow_mut()
        .command(&mut session, &metadata(), &start, now);
    assert_eq!(ops(&first, "save")[0]["clean"], false);
    owner.borrow_mut().tick(&mut session, now + START);
    assert!(owner.borrow().question.is_some());
    assert!(owner.borrow().deadline().is_none());
    let facts = json!({"cmd":"sync","settings":{"sessionRestore":"ask"},"registry":metadata()["registry"],"catalog":metadata()["catalog"],
        "args":{"model":start["model"],"display":{"x":0,"y":0,"w":640,"h":700}},"context":{"theme":{}}});
    let mut a = View::new(session.clone());
    assert!(!a.request(&facts)["tree"]["children"].is_null());
    drop(a);
    let mut b = View::new(session.clone());
    assert!(!b.request(&facts)["tree"]["children"].is_null());
    assert!(owner
        .borrow_mut()
        .command(&mut session, &metadata(), &start, now)["effects"]
        .as_array()
        .unwrap()
        .is_empty());
    let revision = owner.borrow().revision;
    owner.borrow_mut().command(
        &mut session,
        &metadata(),
        &json!({"action":"restored","id":"app:notes"}),
        now,
    );
    assert_eq!(owner.borrow().revision, revision);
    let reply = b.key("Escape", "", 0).unwrap();
    assert_eq!(ops(&reply, "visible"), vec![json!(false)]);
    assert!(owner.borrow().question.is_none());
    assert!(b.key("Enter", "", 0).is_none());
}

#[test]
fn snapshots_read_native_geometry_and_keep_only_safe_apps_arguments_and_desktops() {
    let mut session = Session::default();
    session
        .call("setDesktops", &[json!({}), json!([10, 20]), json!(20)])
        .unwrap();
    let id = open(
        &mut session,
        "notes",
        "a",
        json!({"slug":"saved","nonce":9,"command":"bad"}),
    );
    session
        .call(
            "liftForKeyboard",
            &[json!({}), json!(id), json!({"x":0,"y":0,"w":200,"h":100})],
        )
        .unwrap();
    for (app, instance, args) in [
        ("panel", "", json!({})),
        ("hidden", "", json!({})),
        ("live", "", json!({})),
        ("notes", "dialog", json!({"dialog":true})),
        ("linux", "3", json!({})),
        ("forcequit", "", json!({})),
    ] {
        open(&mut session, app, instance, args);
    }
    let mut state = Service::new(session.reader());
    let now = Instant::now();
    let mut start = input("start", "always", Value::Null);
    start["windows"] = json!([]);
    state.command(&mut session, &metadata(), &start, now);
    state.effects.clear();
    state.tick(&mut session, now + AUTOSAVE);
    let list = &state.saved["windows"];
    assert_eq!(rows(list).len(), 1);
    assert_eq!(list[0]["id"], "app:notes");
    assert_eq!(list[0]["f"], json!({"x":30.,"y":50.,"w":300.,"h":220.}));
    assert_eq!(list[0]["args"], json!({"slug":"saved"}));
    assert_eq!(list[0]["desk"], 1);
    assert_eq!(state.saved["active"], 1);
    assert!(!state.saved.to_string().contains("nonce"));
    let count = state.effects.len();
    session
        .call(
            "patch",
            &[json!({}), json!(id), json!({"title":"Title excluded"})],
        )
        .unwrap();
    state.tick(&mut session, now + Duration::from_secs(3));
    state.tick(&mut session, now + Duration::from_secs(5));
    assert_eq!(state.effects.len(), count);
}

#[test]
fn planning_skips_live_native_windows_and_prepares_desktops_in_one_revision() {
    let mut session = Session::default();
    open(&mut session, "files", "", json!({}));
    let reader = session.reader();
    let mut first = entry("app:notes");
    first["desk"] = json!(1);
    first["max"] = json!(1);
    let mut second = entry("app:notes");
    second["desk"] = json!(1);
    second["min"] = json!(1);
    let saved = json!({"clean":true,"active":1,"windows":[entry("app:files"),first,second]});
    let mut state = Service::new(reader.clone());
    let now = Instant::now();
    state.command(
        &mut session,
        &metadata(),
        &input("start", "always", saved),
        now,
    );
    state.effects.clear();
    let revision = reader.revision();
    state.tick(&mut session, now + START);
    assert_eq!(reader.revision(), revision + 1);
    assert_eq!(reader.read().desktops().len(), 2);
    let effect = state.effects.iter().find(|e| e["op"] == "open").unwrap()["value"].clone();
    assert_eq!(effect["app"], "notes");
    assert_eq!(effect["new"], false);
    let id=session.call("open",&[json!({}),json!({"app":"notes","instance":"a","frame":effect["frame"],"desktop":effect["desktop"]})]).unwrap()["result"].as_str().unwrap().to_owned();
    state.command(
        &mut session,
        &metadata(),
        &json!({"action":"opened","ticket":effect["ticket"],"id":id}),
        now + START,
    );
    assert!(reader.read().window(&id).unwrap().maximized());
    state.effects.clear();
    state.tick(&mut session, now + START + STAGGER);
    let effect = state.effects.iter().find(|e| e["op"] == "open").unwrap()["value"].clone();
    assert_eq!(effect["new"], true);
    let other = session
        .call(
            "open",
            &[
                json!({}),
                json!({"app":"notes","instance":"b","desktop":effect["desktop"]}),
            ],
        )
        .unwrap()["result"]
        .as_str()
        .unwrap()
        .to_owned();
    state.command(
        &mut session,
        &metadata(),
        &json!({"action":"opened","ticket":effect["ticket"],"id":other}),
        now + START + STAGGER,
    );
    assert!(reader.read().window(&other).unwrap().minimized());
    assert_eq!(reader.read().focused_window(), Some(id.as_str()));
    state.tick(&mut session, now + START + STAGGER + SETTLE);
    assert!(!state.hold);
    assert!(state.deadline().is_some());
}

#[test]
fn isolated_launches_are_once_claims_are_ordered_expire_and_stale_tickets_do_nothing() {
    let mut session = Session::default();
    let mut first = entry("lpk:pkg");
    first["snap"] = json!("left");
    let saved = json!({"clean":true,"windows":[first,entry("lpk:pkg")]});
    let mut state = Service::new(session.reader());
    let now = Instant::now();
    state.command(
        &mut session,
        &metadata(),
        &input("start", "always", saved),
        now,
    );
    state.effects.clear();
    state.tick(&mut session, now + START);
    state.tick(&mut session, now + START + STAGGER);
    assert_eq!(
        state.effects.iter().filter(|e| e["op"] == "launch").count(),
        1
    );
    assert_eq!(state.claims.len(), 2);
    let claim = state.command(
        &mut session,
        &metadata(),
        &json!({"action":"claim","id":"lpk:pkg"}),
        now + Duration::from_secs(2),
    )["result"]
        .clone();
    let id = open(
        &mut session,
        "linux",
        "44",
        json!({"pkg":true,"appId":"pkg"}),
    );
    let opened = json!({"action":"opened","ticket":claim["ticket"],"id":id});
    state.command(&mut session, &metadata(), &opened, now);
    assert_eq!(
        session.reader().read().window(&id).unwrap().snapped(),
        Some("left")
    );
    let revision = session.reader().revision();
    state.command(&mut session, &metadata(), &opened, now);
    assert_eq!(session.reader().revision(), revision);
    assert!(state.command(
        &mut session,
        &metadata(),
        &json!({"action":"claim","id":"lpk:pkg"}),
        now + Duration::from_secs(100)
    )["result"]
        .is_null());
}

#[test]
fn shutdown_freezes_before_closing_external_exit_preserves_longer_saved_list_and_resume_rearms() {
    let mut session = Session::default();
    let a = open(&mut session, "notes", "a", json!({}));
    let b = open(&mut session, "notes", "b", json!({}));
    let mut state = Service::new(session.reader());
    let now = Instant::now();
    state.command(
        &mut session,
        &metadata(),
        &input("start", "always", Value::Null),
        now,
    );
    state.tick(&mut session, now + AUTOSAVE);
    assert_eq!(rows(&state.saved["windows"]).len(), 2);
    session.call("close", &[json!({}), json!(b)]).unwrap();
    state.command(
        &mut session,
        &metadata(),
        &json!({"action":"exit"}),
        now + Duration::from_secs(3),
    );
    assert_eq!(rows(&state.saved["windows"]).len(), 2);
    assert_eq!(state.saved["clean"], true);
    assert!(state.deadline().is_none());
    session.call("close", &[json!({}), json!(a)]).unwrap();
    state.effects.clear();
    state.tick(&mut session, now + Duration::from_secs(20));
    assert!(state.effects.is_empty());
    state.command(
        &mut session,
        &metadata(),
        &json!({"action":"resume"}),
        now + Duration::from_secs(21),
    );
    assert_eq!(state.saved["clean"], false);
    state.tick(&mut session, now + Duration::from_secs(23));
    assert!(rows(&state.saved["windows"]).is_empty());
    state.command(
        &mut session,
        &metadata(),
        &json!({"action":"theme"}),
        now + Duration::from_secs(24),
    );
    assert_eq!(state.saved["clean"], "theme");
    assert!(state.deadline().is_none());
}

#[test]
fn off_cancels_pending_restore_and_a_theme_marker_never_relaunches_apps() {
    let mut session = Session::default();
    let saved = json!({"clean":false,"windows":[entry("app:notes")]});
    let now = Instant::now();
    let mut state = Service::new(session.reader());
    state.command(
        &mut session,
        &metadata(),
        &input("start", "always", saved.clone()),
        now,
    );
    let off = state.command(
        &mut session,
        &metadata(),
        &input("configure", "off", Value::Null),
        now + Duration::from_millis(100),
    );
    assert_eq!(ops(&off, "save"), vec![Value::Null]);
    state.tick(&mut session, now + Duration::from_secs(10));
    assert!(state.question.is_none());
    assert!(state.deadline().is_none());
    let mut state = Service::new(session.reader());
    let mut saved = saved;
    saved["clean"] = json!("theme");
    state.command(
        &mut session,
        &metadata(),
        &input("start", "ask", saved),
        now,
    );
    state.effects.clear();
    state.tick(&mut session, now + START);
    assert!(state.question.is_none());
    assert!(state.steps.is_empty());
    assert!(!state.hold);
}

#[test]
fn stale_view_preferences_cannot_reenable_a_restore_disabled_by_a_newer_command() {
    let mut session = Session::default();
    let owner = session.restore_service();
    let now = Instant::now();
    let mut start = input(
        "start",
        "ask",
        json!({"clean":false,"windows":[entry("app:notes")]}),
    );
    start["settingsRevision"] = json!(1);
    owner
        .borrow_mut()
        .command(&mut session, &metadata(), &start, now);
    let mut off = input("configure", "off", Value::Null);
    off["settingsRevision"] = json!(2);
    owner
        .borrow_mut()
        .command(&mut session, &metadata(), &off, now);
    let mut view = View::new(session.clone());
    view.request(&json!({"cmd":"sync","settings":{"sessionRestore":"ask"},
        "context":{"preferenceRevision":1,"renderEpoch":99},"args":{"model":start["model"]}}));
    assert_eq!(owner.borrow().mode, "off");
    assert!(owner.borrow().deadline().is_none());
    owner
        .borrow_mut()
        .tick(&mut session, now + Duration::from_secs(5));
    assert!(owner.borrow().question.is_none());
}

#[test]
fn invalid_saved_data_is_cleaned_and_snapshot_limit_and_pending_login_membership_are_native() {
    let mut session = Session::default();
    for i in 0..14 {
        open(
            &mut session,
            "notes",
            &i.to_string(),
            json!({"slug":i.to_string(),"nonce":i}),
        );
    }
    let mut state = Service::new(session.reader());
    state.metadata = metadata();
    assert_eq!(state.snapshot().len(), 12);
    assert_eq!(state.snapshot()[0]["args"]["slug"], "2");
    let list = json!([
        entry("app:forcequit"),
        entry("linux:bad"),
        entry("desktop:../../bad"),
        entry("app:files")
    ]);
    assert_eq!(clean_list(&list).len(), 1);
    let mut empty = Session::default();
    let now = Instant::now();
    let saved = json!({"clean":true,"windows":[entry("app:notes"),entry("app:files")]});
    let mut state = Service::new(empty.reader());
    state.command(
        &mut empty,
        &metadata(),
        &input("start", "always", saved),
        now,
    );
    state.tick(&mut empty, now + START);
    assert_eq!(
        state.command(
            &mut empty,
            &metadata(),
            &json!({"action":"restored","id":"app:files"}),
            now
        )["result"],
        true
    );
}
