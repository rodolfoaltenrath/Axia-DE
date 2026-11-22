use super::{menubar::MenuBar, Host};
use crate::{runtime::native as scene, winmodel::Session};
use serde_json::json;

#[test]
fn a_window_spanning_equal_display_areas_keeps_the_first_display_as_its_owner() {
    let mut session = Session::default();
    session
        .call(
            "open",
            &[
                json!({}),
                json!({"app":"notes","frame":{"x":0,"y":0,"w":640,"h":500}}),
            ],
        )
        .unwrap();
    let mut menu = super::window_menu::WindowMenu::new(session.reader(), "", None);
    menu.open();
    let items = menu.items(
        &json!({"args":{"displays":[{"id":0,"x":0,"y":0,"w":320,"h":700},
        {"id":1,"x":320,"y":0,"w":320,"h":700}]}}),
    );
    let display_moves: Vec<_> = items
        .iter()
        .filter(|item| item["action"][1]["action"] == "display")
        .collect();
    assert_eq!(display_moves.len(), 1);
    assert_eq!(display_moves[0]["action"][1]["display"], 1);
}

#[test]
fn native_window_menu_reads_and_invalidates_without_js_rows_and_rejects_a_removed_anchor() {
    let mut session = Session::default();
    session
        .call("open", &[json!({}), json!({"app":"notes"})])
        .unwrap();
    session
        .call(
            "patch",
            &[json!({}), json!("notes"), json!({"title":"Live title"})],
        )
        .unwrap();
    let node = scene::create(scene::VIEW);
    let mut host = Host::new(
        node,
        Box::new(MenuBar::with_windows(session.reader(), "", None)),
        "",
        None,
    );
    host.facts(json!({"width":640,"registry":[{"id":"notes","name":"Notes"}],"args":{"bar":{"items":[{"id":"window","label":"Window"}],"height":22}}}));
    host.action(&json!({"action":["bar","window"]}));
    assert_eq!(host.take(), Some(json!([{"kind":"barPrepare"}])));
    assert_ne!(host.find_text("Live title"), 0);
    assert!(host.deadline().is_none());
    session
        .call(
            "patch",
            &[json!({}), json!("notes"), json!({"title":"Fresh title"})],
        )
        .unwrap();
    assert_eq!(host.deadline(), Some(0.));
    host.tick(0.);
    assert_ne!(host.find_text("Fresh title"), 0);
    assert_eq!(host.find_text("Live title"), 0);
    assert!(host.take().is_none());
    assert!(host.deadline().is_none());
    session.call("close", &[json!({}), json!("notes")]).unwrap();
    host.action(&json!({"action":["windowMenu",{"action":"close","id":"notes"}]}));
    assert!(host.take().is_none());
    assert!(host.deadline().is_none());
    drop(host);
    scene::destroy(node);
}

#[test]
fn native_window_menu_keyboard_uses_native_actions_and_closed_menus_do_not_poll() {
    let mut session = Session::default();
    for app in ["notes", "calculator"] {
        session
            .call("open", &[json!({}), json!({"app":app})])
            .unwrap();
    }
    let node = scene::create(scene::VIEW);
    let mut host = Host::new(
        node,
        Box::new(MenuBar::with_windows(session.reader(), "", None)),
        "",
        None,
    );
    host.facts(json!({"width":640,"args":{"bar":{"items":[{"id":"window","label":"Window"}]}}}));
    host.action(&json!({"action":["bar","window"]}));
    host.take();
    host.key("End", "", 0);
    host.key("Enter", "", 0);
    assert_eq!(
        host.take(),
        Some(json!([{"kind":"windowMenu","command":{"action":"focus","id":"calculator"}}]))
    );
    session
        .call("open", &[json!({}), json!({"app":"new"})])
        .unwrap();
    assert!(host.deadline().is_none());
    drop(host);
    scene::destroy(node);
}

#[test]
fn bound_window_bar_executes_native_state_actions_without_effect_callbacks() {
    use super::NativeApp;
    let mut session = Session::default();
    session.call("setDesktops",&[json!({}),json!([1,2]),json!(1)]).unwrap();
    for app in ["notes","calculator"] { session.call("open",&[json!({}),json!({"app":app})]).unwrap(); }
    let reader = session.reader();
    let mut bar = MenuBar::with_session(session.clone(),"",None);
    let run = |bar: &mut MenuBar, action: serde_json::Value| bar.request(&json!({"cmd":"windowMenu","key":action}));
    let revision = reader.revision();
    let reply = run(&mut bar,json!({"action":"minimize","id":"calculator"}));
    assert_eq!(reply["effects"][0]["kind"],"windowModel");
    assert!(reader.read().window("calculator").unwrap().minimized());
    assert_eq!(reader.revision(),revision+1);
    session.call("moveToDesktop",&[json!({}),json!("calculator"),json!(2)]).unwrap();
    let revision = reader.revision();
    run(&mut bar,json!({"action":"focus","id":"calculator"}));
    assert_eq!(reader.read().focused_window(),Some("calculator"));
    assert!(!reader.read().window("calculator").unwrap().minimized());
    assert_eq!(reader.read().active_desktop(),2);
    assert_eq!(reader.revision(),revision+1);
    let revision = reader.revision();
    run(&mut bar,json!({"action":"zoom","id":"calculator"}));
    assert!(reader.read().window("calculator").unwrap().maximized());
    assert_eq!(reader.revision(),revision+1);
    let revision = reader.revision();
    run(&mut bar,json!({"action":"newDesktop","id":"calculator"}));
    assert_eq!(reader.read().desktops(),&[1,2,3]);
    assert_eq!(reader.read().window("calculator").unwrap().desktop(),3);
    assert_eq!(reader.read().active_desktop(),2);
    assert_eq!(reader.revision(),revision+1);
    let revision = reader.revision();
    run(&mut bar,json!({"action":"desktop","id":"calculator","desktop":1}));
    assert_eq!(reader.read().window("calculator").unwrap().desktop(),1);
    assert_eq!(reader.revision(),revision+1);
    let revision = reader.revision();
    for action in [json!({"action":"desktop","id":"calculator","desktop":999}),json!({"action":"newDesktop","id":"gone"})] {
        run(&mut bar,action);
    }
    assert_eq!(reader.revision(),revision);
    let reply = run(&mut bar,json!({"action":"close","id":"calculator"}));
    assert_eq!(reply["effects"][0]["kind"],"windowMenu","app close guards remain in their existing adapter");
    assert_eq!(reader.revision(),revision);
}
