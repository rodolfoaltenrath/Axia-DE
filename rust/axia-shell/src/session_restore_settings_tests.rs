use super::*;
use crate::{session_restore::View, winmodel::Session};
use axia_native::NativeApp;

fn fixture(name: &str) -> (String, Session) {
    let root = std::env::temp_dir().join(format!("axia-restore-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let home = root.to_str().unwrap().to_owned();
    with(&home, |s| {
        s.command(&json!({"cmd":"initialize","text":json!({"schema":3,
            "sessionRestore":"ask","sessionBrowsers":false,"unrelated":{"keep":7},
            "sessionSaved":{"clean":false,"windows":[{"id":"app:notes","f":{"x":40,"y":60,"w":300,"h":220}}]}
        }).to_string()}));
    });
    let session = Session::default();
    session.restore_service().borrow_mut().bind_settings(&home);
    (home, session)
}
fn command(session: &mut Session, action: &str) -> Value {
    session.restore_service().borrow_mut().command(session,
        &json!({"registry":[{"id":"notes","name":"Notes"}],"catalog":[]}),
        &json!({"action":action,"settingsRevision":99999,"settings":{"sessionRestore":"off","sessionSaved":null}}),
        Instant::now())
}
fn disk(home: &str) -> Value {
    serde_json::from_slice(&io::host::load(home).unwrap()).unwrap()
}

#[test]
fn restore_reads_the_profile_and_saves_durable_markers_without_any_view_or_js_writer() {
    let (home, mut session) = fixture("markers");
    let reply = command(&mut session, "start");
    assert!(reply["effects"].as_array().unwrap().iter().all(|e| e["op"] != "save"));
    assert_eq!(disk(&home)["sessionSaved"]["windows"][0]["id"], "app:notes");
    assert_eq!(disk(&home)["sessionSaved"]["clean"], false);
    assert_eq!(disk(&home)["unrelated"]["keep"], 7);
    assert!(io::host::writes().last().unwrap().2);
    let writes = io::host::writes().len();
    assert!(command(&mut session, "start")["effects"].as_array().unwrap().is_empty());
    assert_eq!(command(&mut session, "preferences")["result"], json!({"mode":"ask","browsers":false}));
    assert_eq!(io::host::writes().len(), writes);
    command(&mut session, "flush");
    assert_eq!(disk(&home)["sessionSaved"]["clean"], true);
    command(&mut session, "resume");
    assert_eq!(disk(&home)["sessionSaved"]["clean"], false);
    command(&mut session, "theme");
    assert_eq!(disk(&home)["sessionSaved"]["clean"], "theme");
    assert!(io::host::writes().iter().all(|w| w.2));
    std::fs::remove_dir(home).unwrap();
}

#[test]
fn native_policy_changes_wake_the_view_and_stale_facts_cannot_restore_it() {
    let (home, mut session) = fixture("policy");
    command(&mut session, "start");
    let mut view = View::new(session.clone());
    let facts = json!({"cmd":"sync","settings":{"sessionRestore":"always"},
        "context":{"preferenceRevision":999999,"renderEpoch":2}});
    view.request(&facts);
    let reader = with(&home, |s| s.reader());
    let revision = reader.revision();
    for _ in 0..3 { command(&mut session, "preferences"); }
    assert_eq!(reader.revision(), revision);
    with(&home, |s| { s.set("sessionRestore", &json!("off")); });
    assert!(view.invalidated());
    view.request(&facts);
    assert!(reader.read()["sessionSaved"].is_null());
    assert!(disk(&home)["sessionSaved"].is_null());
    assert_eq!(command(&mut session, "preferences")["result"]["mode"], "off");
    // Consume the service's own write, then a quiet frame must not write again.
    view.request(&facts);
    let writes = io::host::writes().len();
    view.request(&facts);
    assert!(!view.invalidated());
    assert_eq!(io::host::writes().len(), writes);
    let owner = session.restore_service();
    let invalid = owner.borrow_mut().command(&mut session, &Value::Null,
        &json!({"action":"setMode","mode":"invalid"}), Instant::now());
    assert_eq!(invalid["error"], "app.invalidRequest");
    assert_eq!(reader.read()["sessionRestore"], "off");
    std::fs::remove_dir(home).unwrap();
}

#[test]
fn failed_durable_save_keeps_native_data_without_retrying_on_quiet_frames() {
    let (home, mut session) = fixture("failure");
    io::host::fail(true);
    command(&mut session, "start");
    let snapshot = with(&home, |s| s.snapshot());
    assert_eq!(snapshot["dirty"], true);
    assert_eq!(snapshot["pending"], false);
    assert_eq!(snapshot["saveError"], "settings.saveFailed");
    let mut view = View::new(session.clone());
    for _ in 0..3 { view.request(&json!({"cmd":"sync"})); }
    assert_eq!(io::host::writes().len(), 1);
    io::host::fail(false);
    command(&mut session, "flush");
    assert_eq!(disk(&home)["sessionSaved"]["clean"], true);
    assert_eq!(with(&home, |s| s.snapshot())["dirty"], false);
    std::fs::remove_dir(home).unwrap();
}
