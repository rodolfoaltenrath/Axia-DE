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
    let mut menu = WindowContext::new(session.reader());
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
fn keyboard_pointer_and_dismiss_revalidate_the_window_before_effects() {
    let mut session = Session::default();
    call(&mut session,"open",&[json!({"app":"notes"})]);
    call(&mut session,"patch",&[json!("notes"),json!({"windowMenu":{"x":10,"y":20}})]);
    let mut menu = WindowContext::new(session.reader()); menu.request(&input());
    menu.key("Home","",0).unwrap();
    let reply = menu.key("Enter","",0).unwrap();
    assert_eq!(reply["effects"][0]["command"]["action"],"minimize");
    assert_eq!(reply["effects"][0]["id"],"notes");
    assert!(menu.key("Escape","",0).unwrap()["effects"][0]["command"].is_null());
    let forged = json!({"cmd":"windowContext","key":{"action":"close","id":"other"}});
    assert!(menu.request(&forged)["effects"].as_array().unwrap().is_empty());
    let close = json!({"cmd":"windowContext","key":{"action":"close","id":"notes"}});
    assert_eq!(menu.request(&close)["effects"][0]["command"]["action"],"close");
    call(&mut session,"close",&[json!("notes")]);
    assert!(menu.request(&close)["effects"].as_array().unwrap().is_empty());
    assert!(menu.key("Enter","",0).is_none());
    assert!(menu.request(&input())["tree"].is_null());
}
