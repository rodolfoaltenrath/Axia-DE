//! Windows that come back: saved arrangements, the live layout of shells that survived, the live
//! layout being saved, and the frame of a new window.
use super::prefs_tests::{start, start_with, tree, Service};
use super::*;

fn settings_patch(service: &Service, key: &str) -> Option<Value> {
    service.fx.iter().rev().find(|f| f["kind"] == "settings" && f["patch"]["terminal"].get(key).is_some()).map(|f| f["patch"]["terminal"][key].clone())
}
/// A window of its own starting with these arguments, as the shell would mount it.
fn boot(service: &mut Service, now: &mut i64, window: &str, args: Value) -> App {
    let mut app = App::default();
    let mut args = args;
    args["__nativeOwner"] = json!(20);
    args["__nativeWindow"] = json!(window);
    let input = json!({"cmd":"sync","args":args,"settings":{"terminal":service.settings},
        "context":{"width":900,"height":600,"focused":true,"home":"/home/test",
            "theme":{"text":"#ffffff","textDim":"#999999","accent":"#0a84ff","separator":"#444444","mode":"dark","sidebarBg":"#2a2a2e","hover":"#333333"}}});
    let reply = app.request_at(&input, *now);
    service.absorb(&reply);
    service.drive(&mut app, now);
    app
}
fn leaf(cwd: &str, profile: &str) -> Value {
    json!({"type":"leaf","cwd":cwd,"profile":profile})
}

#[test]
fn saving_an_arrangement_keeps_each_window_with_its_frame_folders_and_profiles() {
    let (mut app, mut now, mut service) = start();
    service.send(&mut app, now, json!({"cmd":"do","key":"splitRight"}));
    service.drive(&mut app, &mut now);
    // the shells report their folders when the window polls them
    now += 2000;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    service.fx.clear();
    service.send(&mut app, now, json!({"cmd":"do","key":"saveArrangement"}));
    service.drive(&mut app, &mut now);
    let saved = settings_patch(&service, "arrangement").expect("the arrangement is saved");
    assert_eq!(saved.as_array().unwrap().len(), 1);
    assert_eq!(saved[0]["frame"], json!({"x":10,"y":20,"w":800,"h":500}));
    let root = &saved[0]["tabs"][0]["root"];
    assert_eq!((root["type"].as_str(), root["dir"].as_str()), (Some("split"), Some("row")));
    assert_eq!(root["a"], leaf("/home/test", "default"));
    assert!(tree(&service).contains("terminal.toast.arrangementSaved"));
}

#[test]
fn restoring_an_arrangement_opens_a_window_for_each_saved_one_and_each_rebuilds_its_tabs() {
    let window = json!({"frame":{"x":5,"y":6,"w":700,"h":400},"active":1,"tabs":[
        {"title":"build","root":{"type":"split","dir":"row","ratio":0.3,"a":leaf("/home/test/a","default"),"b":leaf("/tmp","default")}},
        {"title":null,"root":leaf("/var","default")}]});
    let (mut app, mut now, mut service) = start_with(json!({"arrangement":[window.clone()]}));
    service.send(&mut app, now, json!({"cmd":"do","key":"restoreArrangement"}));
    service.drive(&mut app, &mut now);
    let opened = service.calls.iter().find(|(m, a)| m == "windows.open" && a[1]["args"]["arrangement"].is_object()).map(|(_, a)| a.clone()).expect("a window is opened");
    assert_eq!(opened[1]["frame"], window["frame"]);
    let creates_before = service.calls.iter().filter(|(m, _)| m == "term.create").count();
    let _second = boot(&mut service, &mut now, "restored", json!({"arrangement":opened[1]["args"]["arrangement"]}));
    let made: Vec<Value> = service.calls.iter().filter(|(m, _)| m == "term.create").skip(creates_before).map(|(_, a)| a[0].clone()).collect();
    assert_eq!(made.iter().filter_map(|o| o["cwd"].as_str()).collect::<Vec<_>>(), ["/home/test/a", "/tmp", "/var"], "a shell for each pane, in the folders they were in");
    let w = shell(|s| s.ws.window("restored").cloned()).expect("the window has its tabs");
    assert_eq!(w.tabs.len(), 2);
    assert_eq!(w.tabs[0].title.as_deref(), Some("build"));
    assert_eq!(w.tabs[0].root.leaves().len(), 2);
    assert_eq!(w.active, w.tabs[1].id, "the tab that was active is active");
    assert!(matches!(&w.tabs[0].root, crate::workspace::Node::Split { ratio, .. } if (ratio - 0.3).abs() < 1e-9), "the proportions are kept");
}

#[test]
fn restoring_with_nothing_saved_says_so() {
    let (mut app, now, mut service) = start();
    service.send(&mut app, now, json!({"cmd":"do","key":"restoreArrangement"}));
    assert!(tree(&service).contains("terminal.toast.noArrangement"));
}

#[test]
fn shells_that_survived_come_back_where_they_were_and_the_rest_get_a_tab() {
    let live = json!([
        {"frame":{"x":1,"y":2,"w":900,"h":600},"active":1,"tabs":[
            {"title":"one","root":{"type":"leaf","key":"k1","profile":"default"}},
            {"title":null,"root":{"type":"split","dir":"column","ratio":0.5,"a":{"type":"leaf","key":"k2","profile":"default"},"b":{"type":"leaf","key":"gone","profile":"default"}}}]},
        {"frame":{"x":50,"y":60,"w":500,"h":300},"active":0,"tabs":[{"title":null,"root":{"type":"leaf","key":"k3","profile":"default"}}]}]);
    let mut app = App::default();
    let mut service = Service::new_for_live(json!({"live":live}), &[(201, "k1"), (202, "k2"), (203, "k3"), (204, "loose")]);
    let mut now = 1000;
    let reply = app.request_at(&service.boot_input(), now);
    service.absorb(&reply);
    service.drive(&mut app, &mut now);
    assert!(!service.calls.iter().any(|(m, _)| m == "term.create"), "no new shell: the old ones are used");
    let w = shell(|s| s.ws.window("terminal").cloned()).expect("the first window");
    assert_eq!(w.tabs.len(), 3, "two saved tabs and one for the shell the layout did not mention");
    assert_eq!(w.tabs[1].root.leaves(), [202], "a pane whose shell is gone is dropped, its neighbour stays");
    assert_eq!(w.tabs[2].root.leaves(), [204]);
    assert_eq!(w.active, w.tabs[1].id);
    assert!(shell(|s| s.sessions.contains_key(&201) && s.sessions[&201].profile == "default"));
    let opened = service.calls.iter().find(|(m, a)| m == "windows.open" && a[1]["args"]["liveAdopt"] == true).map(|(_, a)| a.clone()).expect("the second window is opened");
    assert_eq!(opened[1]["frame"], json!({"x":50,"y":60,"w":500,"h":300}));
    let _second = boot(&mut service, &mut now, "second", json!({"liveAdopt":true}));
    assert_eq!(shell(|s| s.ws.window("second").map(|w| w.tabs[0].root.leaves())), Some(vec![203]));
    assert!(!tree(&service).is_empty());
}

#[test]
fn with_no_shell_left_over_the_first_window_makes_its_own() {
    let (_app, _now, service) = start();
    assert!(service.calls.iter().any(|(m, _)| m == "term.list"), "it looked for survivors first");
    assert_eq!(service.calls.iter().filter(|(m, _)| m == "term.create").count(), 1);
}

#[test]
fn the_live_layout_is_saved_when_it_changes_and_not_otherwise() {
    let (mut app, mut now, mut service) = start();
    let saved = settings_patch(&service, "live").expect("the live layout is saved");
    assert_eq!(saved[0]["frame"]["w"], 800);
    assert_eq!(saved[0]["tabs"][0]["root"]["type"], "leaf");
    service.fx.clear();
    shell(|s| s.live.force_due());
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    assert!(settings_patch(&service, "live").is_none(), "an unchanged layout is not saved again");
}

#[test]
fn a_new_window_takes_the_size_the_profile_asks_for() {
    let settings = json!({"profiles":[{"id":"default","columns":100,"rows":30,"padding":8,"statusBar":true}],"defaultProfile":"default"});
    let (mut app, mut now, mut service) = start_with(settings);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    now += 2000;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    service.send(&mut app, now, json!({"cmd":"do","key":"newWindow"}));
    service.drive(&mut app, &mut now);
    let opened = service.calls.iter().rev().find(|(m, _)| m == "windows.open").map(|(_, a)| a[1].clone()).expect("a window is opened");
    assert_eq!(opened["frame"], json!({"x":40,"y":50,"w":820,"h":584}), "columns and rows of the cells, padding, title bar and status bar");
}

#[test]
fn without_columns_and_rows_the_new_window_has_the_usual_size() {
    let (mut app, mut now, mut service) = start();
    service.send(&mut app, now, json!({"cmd":"do","key":"newWindow"}));
    service.drive(&mut app, &mut now);
    let opened = service.calls.iter().rev().find(|(m, _)| m == "windows.open").map(|(_, a)| a[1].clone()).unwrap();
    assert_eq!(opened, json!({"new":true}));
}
