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

#[test]
fn bound_window_menu_borrows_physical_displays_and_rebuilds_after_hotplug() {
    let mut session = Session::default();
    session.call("open", &[json!({}),json!({"app":"notes","frame":{"x":10,"y":40,"w":200,"h":200}})]).unwrap();
    let source = axia_state::Store::new(vec![json!({"id":0,"name":"Internal","x":0,"y":0,"w":640,"h":700})]);
    let mut menu = super::window_menu::WindowMenu::with_displays(session.clone(),source.reader(),"",None);
    menu.open();
    let stale = json!({"args":{"displays":[{"id":9,"name":"Forged","x":0,"y":0,"w":100,"h":100},
        {"id":10,"name":"Forged second","x":100,"y":0,"w":100,"h":100}]}});
    let moves = |items: Vec<serde_json::Value>| items.into_iter().filter(|item|item["action"][1]["action"]=="display").collect::<Vec<_>>();
    assert!(moves(menu.items(&stale)).is_empty(),"stale JS displays must not replace the physical source");
    assert!(!menu.pending());
    source.update(|displays| { displays.push(json!({"id":1,"name":"External","x":640,"y":0,"w":800,"h":600})); ((),true) });
    assert!(menu.pending());
    let items = moves(menu.items(&stale));
    assert_eq!(items.len(),1);
    assert_eq!(items[0]["action"][1]["display"],1);
    assert_eq!(menu.effect(&items[0]["action"][1])["effects"][0]["kind"],"windowMenu","geometry effects remain with the adapter");
    assert!(!menu.pending());
    session.call("setFrame",&[json!({}),json!("notes"),json!({"x":700,"y":40,"w":200,"h":200})]).unwrap();
    assert_eq!(moves(menu.items(&stale))[0]["action"][1]["display"],0,"window geometry chooses the owning display");
    source.update(|displays| { displays.remove(0); ((),true) });
    assert!(menu.pending());
    assert!(moves(menu.items(&stale)).is_empty(),"unplug removes the stale destination");
    source.update(|displays| { displays.clear(); ((),true) });
    assert!(moves(menu.items(&stale)).is_empty(),"an empty physical source must not fall back to forged JS");
    assert_eq!(menu.effect(&items[0]["action"][1]),json!({}),"a queued action cannot target a disconnected display");
    assert_eq!(menu.effect(&json!({"action":"display","id":"notes","display":90})),json!({}));
    source.update(|displays| { displays.push(json!({"id":2,"name":"Reconnected","x":0,"y":0,"w":1000,"h":700})); ((),true) });
    assert!(menu.pending());
    assert!(moves(menu.items(&stale)).is_empty());
}

#[test]
fn hotplug_refreshes_the_open_native_menu_without_facts_and_closed_menus_stay_idle() {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"),"/../../assets");
    let mut session = Session::default();
    session.call("open",&[json!({}),json!({"app":"notes","frame":{"x":10,"y":40,"w":200,"h":200}})]).unwrap();
    let source = axia_state::Store::new(vec![json!({"id":0,"name":"Internal","x":0,"y":0,"w":640,"h":700})]);
    let node = scene::create(scene::VIEW);
    let mut host = Host::new(node,Box::new(MenuBar::with_displays(session,source.reader(),assets,None)),assets,None);
    host.facts(json!({"width":1000,"lang":"en","args":{"bar":{"items":[{"id":"window","label":"Window"}]}}}));
    host.action(&json!({"action":["bar","window"]}));
    host.take();
    assert_eq!(host.find_text("Move to External (2)"),0);
    source.update(|displays| { displays.push(json!({"id":1,"name":"External","x":640,"y":0,"w":800,"h":600})); ((),true) });
    assert_eq!(host.deadline(),Some(0.));
    host.tick(0.);
    assert_ne!(host.find_text("Move to External (2)"),0,"a physical revision repaints the open menu without new facts");
    assert!(host.deadline().is_none());
    source.update(|displays| { displays.pop(); ((),true) });
    assert_eq!(host.deadline(),Some(0.));
    host.tick(0.);
    assert_eq!(host.find_text("Move to External (2)"),0);
    host.key("Escape","",0);
    source.update(|displays| { displays.clear(); ((),true) });
    assert!(host.deadline().is_none(),"a disconnected display must not wake a closed menu");
    drop(host);
    scene::destroy(node);
}

#[test]
fn window_menu_borrows_session_catalog_and_recreation_preserves_its_source() {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"),"/../../assets");
    let mut session = Session::default();
    for app in ["notes","panel"] { session.call("open",&[json!({}),json!({"app":app})]).unwrap(); }
    let node = scene::create(scene::VIEW);
    let mut host = Host::new(node,Box::new(MenuBar::with_session(session.clone(),assets,None)),assets,None);
    let facts = json!({"width":1000,"lang":"en","registry":[{"id":"notes","name":"Forged Notes","builtin":true},{"id":"panel","name":"Forged panel"}],"args":{"bar":{"items":[{"id":"window","label":"Window"}]}}});
    host.facts(facts.clone());
    host.action(&json!({"action":["bar","window"]}));
    host.take();
    assert_eq!(host.find_text("Forged panel"),0,"an empty native catalog cannot fall back to view facts");
    let mut control = crate::app_control::Controller::new(session.clone());
    let metadata = json!({"cmd":"metadata","catalog":[],"registry":[{"id":"notes","name":"Native Notes"},{"id":"panel","name":"Native panel","builtin":true}]});
    assert_eq!(control.request(&metadata)["ok"],true);
    assert_eq!(host.deadline(),Some(0.));
    host.tick(0.);
    assert_ne!(host.find_text("Native Notes"),0);
    for title in ["Native panel","Forged Notes","Forged panel"] { assert_eq!(host.find_text(title),0); }
    assert!(host.deadline().is_none());
    let catalog = session.catalog_reader();
    let revision = catalog.revision();
    drop(control);
    let mut control = crate::app_control::Controller::new(session.clone());
    assert_eq!(catalog.revision(),revision,"recreating the adapter must not replace or clear the catalog");
    assert_eq!(control.request(&metadata)["ok"],true);
    assert_eq!(catalog.revision(),revision,"identical metadata does not invalidate readers");
    let updated = json!({"cmd":"metadata","catalog":[],"registry":[{"id":"notes","name":"Updated Notes","builtin":true},{"id":"panel","name":"Visible panel"}]});
    control.request(&updated);
    host.tick(0.);
    assert_eq!(host.find_text("Native Notes"),0);
    assert_ne!(host.find_text("Visible panel"),0);
    host.facts(facts);
    assert_eq!(host.find_text("Forged panel"),0);
    let mut other = crate::app_control::Controller::new(Session::default());
    other.request(&metadata);
    assert_eq!(catalog.read()["registry"][0]["name"],"Updated Notes","another context cannot replace this catalog");
    host.key("Escape","",0);
    control.request(&metadata);
    assert!(host.deadline().is_none(),"catalog revisions do not wake closed menus");
    drop(host);
    scene::destroy(node);
}
