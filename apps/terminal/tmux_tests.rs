//! tmux control mode in the native window against a fake gateway: protocol text in, tmux
//! commands and window state out. The gateway is session 101 of the window `terminal`; the tmux
//! windows become tabs of a window of their own, `tmuxwin`, which the test starts as the shell would.
use super::prefs_tests::{start, tree, Service};
use super::*;

const LAYOUT: &str = "@1\tb25d,160x48,0,0{80x48,0,0,1,79x48,81,0,2}\tmain";

fn deliver(app: &mut App, now: &mut i64, service: &mut Service, events: Vec<Value>) {
    service.events.extend(events);
    *now += 400;
    service.send(app, *now, json!({"cmd":"noop"}));
    service.drive(app, now);
}
fn tmux_data(text: &str) -> Value {
    json!({"sid":101,"type":"tmux","data":text})
}
fn commands(service: &Service) -> Vec<String> {
    service.fx.iter().filter(|f| f["op"] == "tmuxCommand").map(|f| format!("{}:{}", f["sid"], f["cmd"].as_str().unwrap_or(""))).collect()
}
fn sent(service: &Service, needle: &str) -> bool {
    commands(service).iter().any(|c| c.contains(needle))
}
/// A block as tmux writes it.
fn block(number: u32, lines: &[&str]) -> String {
    let mut text = format!("%begin {number} {number} 0\n");
    for line in lines {
        text += &format!("{line}\n");
    }
    text + &format!("%end {number} {number} 0\n")
}
/// The gateway connected and listing two panes in one window, and the window for its tabs started.
fn connected() -> (App, App, i64, Service) {
    let (mut app, mut now, mut service) = start();
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"tmux-start","data":""})]);
    assert!(sent(&service, "101:list-windows"), "{:?}", commands(&service));
    let reply = format!("{}{}", block(1, &[]), block(2, &[LAYOUT]));
    deliver(&mut app, &mut now, &mut service, vec![tmux_data(&reply)]);
    let mut win = App::default();
    let boot = json!({"cmd":"sync","args":{"__nativeOwner":10,"__nativeWindow":"tmuxwin","tmuxGateway":101},"settings":{"terminal":service.settings},
        "context":{"width":900,"height":600,"focused":true,"home":"/home/test",
            "theme":{"text":"#ffffff","textDim":"#999999","accent":"#0a84ff","separator":"#444444","mode":"dark","sidebarBg":"#2a2a2e","hover":"#333333"}}});
    let reply = win.request_at(&boot, now);
    service.absorb(&reply);
    service.drive(&mut win, &mut now);
    (app, win, now, service)
}

#[test]
fn the_gateway_asks_tmux_for_its_windows_and_makes_a_session_per_pane() {
    let (mut app, mut now, mut service) = start();
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"tmux-start","data":""})]);
    assert!(shell(|s| s.sessions[&101].tmux), "the session is a gateway now");
    deliver(&mut app, &mut now, &mut service, vec![tmux_data(&format!("{}{}", block(1, &[]), block(2, &[LAYOUT])))]);
    let asked: Vec<Value> = service.calls.iter().filter(|(m, _)| m == "term.tmuxPane").map(|(_, a)| a.clone()).collect();
    assert_eq!(asked, [json!([101, 1]), json!([101, 2])], "one session for each pane tmux has");
    assert!(sent(&service, "101:capture-pane -p -e -J -t %1 -S -2000") && sent(&service, "101:capture-pane -p -e -J -t %2 -S -2000"), "{:?}", commands(&service));
    let opened = service.calls.iter().find(|(m, _)| m == "windows.open").map(|(_, a)| a.clone());
    assert_eq!(opened, Some(json!(["terminal", {"new":true,"tmuxGateway":101}])), "the tmux windows get a Terminal window");
    assert!(tree(&service).contains("terminal.tmux.connected"), "the gateway pane says it is connected");
}

#[test]
fn the_new_window_takes_the_tabs_and_names_them() {
    let (_app, _win, _now, service) = connected();
    let tabs = shell(|s| s.ws.window("tmuxwin").map(|w| w.tabs.clone())).expect("the window has tabs");
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].title.as_deref(), Some("main"));
    assert_eq!(tabs[0].root.leaves(), [102, 103]);
    assert!(tree(&service).contains("terminal.tmux.opened"), "{}", tree(&service));
    assert_eq!(shell(|s| s.sessions[&102].gateway), Some(101));
    assert_eq!(shell(|s| s.sessions[&103].tmux_pane), 2);
}

#[test]
fn what_a_pane_showed_is_written_to_its_session_with_the_cursor_where_it_was() {
    let (mut app, _win, mut now, mut service) = connected();
    service.fx.clear();
    let captured = format!("{}{}", block(3, &["first", "second"]), block(4, &["other"]));
    deliver(&mut app, &mut now, &mut service, vec![tmux_data(&captured)]);
    assert!(sent(&service, "101:display-message -p -t %1 \"#{cursor_x} #{cursor_y} #{pane_height}\""), "{:?}", commands(&service));
    let cursors = format!("{}{}", block(5, &["0 1 24"]), block(6, &["3 0 24"]));
    deliver(&mut app, &mut now, &mut service, vec![tmux_data(&cursors)]);
    let fed: Vec<(i64, String)> = service.fx.iter().filter(|f| f["op"] == "feed").map(|f| (f["sid"].as_i64().unwrap(), f["text"].as_str().unwrap().to_owned())).collect();
    assert_eq!(fed[0], (102, "\x1b[H\x1b[2Jfirst\r\nsecond".to_owned()));
    assert_eq!(fed[1], (102, "\x1b[1;1H".to_owned()), "the cursor row counts from the bottom of the captured lines");
    assert_eq!(fed[2], (103, "\x1b[H\x1b[2Jother".to_owned()));
    assert_eq!(service.fx.iter().filter(|f| f["op"] == "tmuxReady").count(), 2, "the server is told when each pane is ready");
}

#[test]
fn layout_changes_rename_close_and_focus_follow_tmux() {
    let (mut app, _win, mut now, mut service) = connected();
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%window-renamed @1 build\n")]);
    assert_eq!(shell(|s| s.ws.window("tmuxwin").unwrap().tabs[0].title.clone()), Some("build".to_owned()));
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%window-pane-changed @1 %2\n")]);
    assert_eq!(shell(|s| s.ws.window("tmuxwin").unwrap().tabs[0].focus), 103);
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%layout-change @1 b25d,160x48,0,0,1 b25d,160x48,0,0,1 *\n")]);
    let tab = shell(|s| s.ws.window("tmuxwin").unwrap().tabs[0].clone());
    assert_eq!(tab.root.leaves(), [102], "the pane tmux dropped is gone");
    assert_eq!(tab.focus, 102, "and the focus moved to the one that stayed");
    assert!(!shell(|s| s.sessions.contains_key(&103)));
    assert!(service.calls.iter().any(|(m, a)| m == "term.close" && a[0] == 103), "its session is closed");
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%window-close @1\n")]);
    assert!(shell(|s| s.ws.window("tmuxwin").is_none()), "the last tmux window takes its Terminal window with it");
    assert!(service.fx.iter().any(|f| f["kind"] == "closeWindow" && f["id"] == "tmuxwin"));
}

#[test]
fn a_second_tmux_window_is_another_tab_and_a_split_adds_a_pane() {
    let (mut app, _win, mut now, mut service) = connected();
    // tmux answers in order: the contents of the two panes first
    deliver(&mut app, &mut now, &mut service, vec![tmux_data(&format!("{}{}", block(10, &["a"]), block(11, &["b"])))]);
    deliver(&mut app, &mut now, &mut service, vec![tmux_data(&format!("{}{}", block(12, &["0 0 24"]), block(13, &["0 0 24"])))]);
    let layout = "@2\tb25d,160x48,0,0,3\tlogs";
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%window-add @2\n")]);
    assert!(sent(&service, "101:list-windows"));
    deliver(&mut app, &mut now, &mut service, vec![tmux_data(&block(7, &[LAYOUT, layout]))]);
    let titles: Vec<Option<String>> = shell(|s| s.ws.window("tmuxwin").unwrap().tabs.iter().map(|t| t.title.clone()).collect());
    assert_eq!(titles, [Some("main".to_owned()), Some("logs".to_owned())]);
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%layout-change @2 b25d,160x48,0,0{80x48,0,0,3,79x48,81,0,4} x *\n")]);
    assert_eq!(shell(|s| s.ws.window("tmuxwin").unwrap().tabs[1].root.leaves().len()), 2, "the new pane is in the tab");
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%window-close @2\n")]);
    let titles: Vec<Option<String>> = shell(|s| s.ws.window("tmuxwin").unwrap().tabs.iter().map(|t| t.title.clone()).collect());
    assert_eq!(titles, [Some("main".to_owned())], "a window tmux closed is closed here");
    assert!(!shell(|s| s.sessions.contains_key(&104)), "with the sessions of its panes");
}

#[test]
fn the_actions_of_the_window_become_tmux_commands() {
    let (_app, mut win, now, mut service) = connected();
    service.fx.clear();
    for (action, command) in [
        ("splitRight", "101:split-window -h -t %1"),
        ("splitDown", "101:split-window -v -t %1"),
        ("newTab", "101:new-window"),
        ("closePane", "101:kill-pane -t %1"),
        ("closeTab", "101:kill-window -t @1"),
        ("equalize", "101:select-layout -t %1 tiled"),
        ("maximizePane", "101:resize-pane -Z -t %1"),
    ] {
        service.fx.clear();
        service.send(&mut win, now, json!({"cmd":"do","key":action}));
        assert!(commands(&service).contains(&command.to_owned()), "{action}: {:?}", commands(&service));
    }
    assert_eq!(shell(|s| s.ws.window("tmuxwin").unwrap().tabs[0].root.leaves().len()), 2, "nothing was closed here: tmux answers with a new layout");
    service.fx.clear();
    service.send(&mut win, now, json!({"cmd":"paneDown","key":103,"button":0,"x":700,"y":200,"frame":[0,0,1,1]}));
    assert!(commands(&service).contains(&"101:select-pane -t %2".to_owned()), "clicking a pane selects it in tmux: {:?}", commands(&service));
}

#[test]
fn escape_on_the_gateway_lets_go_of_tmux_and_the_other_keys_stay_out_of_it() {
    let (mut app, _win, now, mut service) = connected();
    service.fx.clear();
    service.key(&mut app, now, "a", 0, 30);
    assert!(service.fx.iter().all(|f| f["op"] != "key" && f["op"] != "write"), "the gateway takes no typing: {:?}", service.fx);
    service.key(&mut app, now, "Escape", 0, 1);
    assert!(commands(&service).contains(&"101:detach-client".to_owned()), "{:?}", commands(&service));
}

#[test]
fn closing_the_tmux_window_detaches() {
    let (_app, mut win, now, mut service) = connected();
    service.fx.clear();
    win.on_event(&json!({"kind":"closeRequested"}));
    service.send(&mut win, now, json!({"cmd":"dialogConfirm"}));
    assert!(commands(&service).contains(&"101:detach-client".to_owned()), "{:?}", commands(&service));
    assert!(service.fx.iter().any(|f| f["kind"] == "closeWindow" && f["id"] == "tmuxwin"));
}

#[test]
fn ending_the_connection_closes_the_pane_sessions_and_says_why() {
    let (mut app, _win, mut now, mut service) = connected();
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%exit server exited\n")]);
    assert!(!shell(|s| s.sessions.contains_key(&102) || s.sessions.contains_key(&103)));
    assert!(!shell(|s| s.sessions[&101].tmux), "the gateway is a plain shell again");
    assert!(tree(&service).contains("terminal.tmux.detached") && tree(&service).contains("server exited"), "{}", tree(&service));
    assert!(shell(|s| s.ws.window("tmuxwin").is_none()), "the window that showed it closes");
}

#[test]
fn the_end_event_ends_it_without_a_message() {
    let (mut app, _win, mut now, mut service) = connected();
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"tmux-end","data":""})]);
    assert!(!shell(|s| s.sessions.contains_key(&102)));
    assert!(!tree(&service).contains("terminal.tmux.detached"));
}

#[test]
fn a_resize_tells_tmux_the_size_of_the_tab_once() {
    let (mut app, _win, mut now, mut service) = connected();
    service.fx.clear();
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":102,"type":"resize","data":""})]);
    app.tmux_resize_now(101);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    assert!(sent(&service, "101:refresh-client -C 161x24"), "two panes of 80 columns and a divider: {:?}", commands(&service));
    service.fx.clear();
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":102,"type":"resize","data":""})]);
    app.tmux_resize_now(101);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    assert!(!sent(&service, "refresh-client"), "the same size is not sent again");
}

#[test]
fn a_tmux_error_is_shown_and_a_layout_that_does_not_parse_is_ignored() {
    let (mut app, _win, mut now, mut service) = connected();
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%begin 9 9 0\nunknown command\n%error 9 9 0\n")]);
    assert!(tree(&service).contains("tmux: unknown command"), "{}", tree(&service));
    deliver(&mut app, &mut now, &mut service, vec![tmux_data("%layout-change @1 garbage\n")]);
    assert_eq!(shell(|s| s.ws.window("tmuxwin").unwrap().tabs[0].root.leaves()), [102, 103], "the tab is as it was");
}

#[test]
fn sessions_still_in_control_mode_after_a_restart_are_taken_up_without_a_launch_answer() {
    let (mut app, mut now, mut service) = start();
    // the first infos poll says the session is in control mode: the window did not see it start
    shell(|s| s.recovered = false);
    app.on_infos(&json!([{"sid":101,"info":{"tmux":true,"cols":80,"rows":24}}]));
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(sent(&service, "101:list-windows"), "the windows are asked for at once");
    // no answer is owed to a launch command: the first block is the answer to list-windows
    deliver(&mut app, &mut now, &mut service, vec![tmux_data(&block(1, &[LAYOUT]))]);
    assert_eq!(service.calls.iter().filter(|(m, _)| m == "term.tmuxPane").count(), 2);
}
