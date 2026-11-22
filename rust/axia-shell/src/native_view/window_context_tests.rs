use super::*;
use crate::winmodel::Session;

fn call(session: &mut Session, op: &str, args: &[Value]) {
    let mut all = vec![json!({})]; all.extend_from_slice(args);
    session.call(op,&all).unwrap();
}
fn input() -> Value {
    json!({"cmd":"sync","args":{"id":"notes","x":9999,"items":["forged"],"maximized":true,"desktops":[999]}})
}
#[test]
fn context_rows_coordinates_and_checks_follow_native_revisions() {
    let mut session = Session::default();
    call(&mut session,"setDesktops",&[json!([10,20]),json!(10)]);
    call(&mut session,"open",&[json!({"app":"notes"})]);
    call(&mut session,"patch",&[json!("notes"),json!({"windowMenu":{"x":100,"y":80}})]);
    let mut menu = WindowContext::new(session.clone());
    let tree = menu.request(&input())["tree"].clone();
    assert_eq!(tree["x"],100.); assert_eq!(tree["y"],80.);
    assert_eq!(tree["items"][1]["checked"],false);
    assert_eq!(tree["items"][2]["submenu"][0]["label"]["params"]["n"],2);
    assert_eq!(tree["items"][2]["submenu"][0]["action"][1]["desktop"],20);
    assert_eq!(menu.request(&input())["same"],true);
    menu.request(&json!({"cmd":"menuHover","key":2}));
    assert_eq!(menu.request(&input())["tree"]["sub"],2);
    call(&mut session,"patch",&[json!("notes"),json!({"windowMenu":{"x":200,"y":160}})]);
    call(&mut session,"toggleMaximize",&[json!("notes")]);
    assert!(menu.invalidated());
    let tree = menu.request(&input())["tree"].clone();
    assert_eq!(tree["x"],200.); assert_eq!(tree["items"][1]["checked"],true);
    assert!(!menu.invalidated());
    for _ in 0..7 { call(&mut session,"createDesktop",&[]); }
    menu.request(&json!({"cmd":"menuHover","key":2}));
    let tree = menu.request(&input())["tree"].clone();
    assert_eq!(tree["sub"],2,"the first hover after a native revision must survive row preparation");
    assert_eq!(tree["items"][2]["submenu"].as_array().unwrap().last().unwrap()["disabled"],true);
    call(&mut session,"patch",&[json!("notes"),json!({"windowMenu":null})]);
    assert!(menu.request(&input())["tree"].is_null());
}
#[test]
fn keyboard_pointer_and_dismiss_revalidate_and_mutate_the_native_owner() {
    let mut session = Session::default();
    call(&mut session,"open",&[json!({"app":"notes"})]);
    let mut menu = WindowContext::new(session.clone());
    let reader = session.reader();
    let open = |session: &mut Session, menu: &mut WindowContext| {
        call(session,"patch",&[json!("notes"),json!({"windowMenu":{"x":10,"y":20}})]);
        menu.request(&input());
    };
    open(&mut session,&mut menu);
    menu.key("Home","",0).unwrap();
    let revision = reader.revision();
    let reply = menu.key("Enter","",0).unwrap();
    assert_eq!(reply["effects"][0]["kind"],"windowModel");
    assert_eq!(reader.revision(),revision+1);
    assert!(reader.read().window("notes").unwrap().minimized());
    assert!(reader.read().window("notes").unwrap().window_menu().is_none());
    open(&mut session,&mut menu);
    let revision = reader.revision();
    let frame = reader.read().window("notes").unwrap().frame();
    let zoom = menu.request(&json!({"cmd":"windowContext","key":{"action":"zoom","id":"notes"}}));
    assert_eq!(zoom["effects"].as_array().unwrap().len(),1);
    assert_eq!(reader.revision(),revision+1);
    assert!(reader.read().window("notes").unwrap().maximized());
    assert_eq!(reader.read().window("notes").unwrap().frame(),frame,"maximizing retains normal geometry");
    assert!(reader.read().window("notes").unwrap().window_menu().is_none());
    open(&mut session,&mut menu);
    let revision = reader.revision();
    assert_eq!(menu.key("Escape","",0).unwrap()["effects"][0]["kind"],"windowModel");
    assert_eq!(reader.revision(),revision+1);
    assert!(reader.read().window("notes").unwrap().window_menu().is_none());
    open(&mut session,&mut menu);
    let forged = json!({"cmd":"windowContext","key":{"action":"close","id":"other"}});
    assert!(menu.request(&forged)["effects"].as_array().unwrap().is_empty());
    assert!(reader.read().window("notes").unwrap().window_menu().is_some());
    let close = json!({"cmd":"windowContext","key":{"action":"close","id":"notes"}});
    let reply = menu.request(&close);
    assert_eq!(reply["effects"][0]["kind"],"windowModel");
    assert_eq!(reply["effects"][1]["command"]["action"],"close");
    assert!(reader.read().window("notes").is_some(),"the app's close guard must still run");
    assert!(reader.read().window("notes").unwrap().window_menu().is_none());
    call(&mut session,"close",&[json!("notes")]);
    assert!(menu.request(&close)["effects"].as_array().unwrap().is_empty());
    assert!(menu.key("Enter","",0).is_none());
    assert!(menu.request(&input())["tree"].is_null());
}

#[test]
fn native_menu_moves_and_dismissal_publish_one_window_revision() {
    let mut session = Session::default();
    call(&mut session,"open",&[json!({"app":"notes"})]);
    let reader = session.reader();
    let mut menu = WindowContext::new(session.clone());
    let open = |session: &mut Session, menu: &mut WindowContext| {
        call(session,"patch",&[json!("notes"),json!({"windowMenu":{"x":10,"y":20}})]);
        menu.request(&input());
    };
    open(&mut session,&mut menu);
    let revision = reader.revision();
    let action = json!({"cmd":"windowContext","key":{"action":"newDesktop","id":"notes"}});
    let reply = menu.request(&action);
    assert_eq!(reader.revision(),revision+1);
    assert_eq!(reader.read().desktops(),&[1,2]);
    assert_eq!(reader.read().window("notes").unwrap().desktop(),2);
    assert!(reader.read().window("notes").unwrap().window_menu().is_none());
    assert_eq!(reply["effects"].as_array().unwrap().len(),1);
    assert_eq!(reply["effects"][0]["kind"],"windowModel");
    let revision = reader.revision();
    assert!(menu.request(&action)["effects"].as_array().unwrap().is_empty());
    assert_eq!(reader.revision(),revision);
    assert_eq!(reader.read().active_desktop(),1);
    while reader.read().desktops().len()<crate::winmodel::desktops::MAX_DESKTOPS { call(&mut session,"createDesktop",&[]); }
    open(&mut session,&mut menu);
    let revision = reader.revision();
    menu.request(&action);
    assert_eq!(reader.revision(),revision+1,"capacity failure only dismisses the menu");
    assert_eq!(reader.read().desktops().len(),crate::winmodel::desktops::MAX_DESKTOPS);
    assert_eq!(reader.read().window("notes").unwrap().desktop(),2);
    assert!(reader.read().window("notes").unwrap().window_menu().is_none());
}
