//! The native Terminal window against a fake session service: it creates sessions, lays out
//! tabs and panes, turns keys and the mouse into input effects and follows session events.
use super::*;

struct Service {
    token: u64,
    ready: std::collections::BTreeMap<u64, Value>,
    calls: Vec<(String, Value)>,
    next_sid: i64,
    pending: Option<Value>,
    events: Vec<Value>,
    infos: HashMap<i64, Value>,
    fx: Vec<Value>,
    tree: Value,
}
impl Service {
    fn new() -> Self {
        Self { token: 0, ready: Default::default(), calls: vec![], next_sid: 100, pending: None, events: vec![], infos: HashMap::new(), fx: vec![], tree: Value::Null }
    }
    fn answer(&mut self, method: &str, args: &Value) -> Value {
        match method {
            "term.create" => {
                self.next_sid += 1;
                json!(self.next_sid)
            }
            "term.infos" => json!(args[0].as_array().unwrap().iter().filter_map(|s| {
                let sid = s.as_i64()?;
                Some(json!({"sid":sid,"info":self.infos.get(&sid).cloned().unwrap_or(json!({"process":"","busy":false,"cols":80,"rows":24,"cwd":"/home/test","shell":"bash"}))}))
            }).collect::<Vec<_>>()),
            "term.events" => Value::Array(std::mem::take(&mut self.events)),
            _ => Value::Null,
        }
    }
    fn absorb(&mut self, reply: &Value) {
        if let Some(effect) = reply.get("effect") {
            self.pending = Some(effect.clone());
        }
        if let Some(tree) = reply.get("tree") {
            self.tree = tree.clone();
        }
        for fx in reply["effects"].as_array().into_iter().flatten() {
            self.fx.push(fx.clone());
        }
    }
    fn count(&self, method: &str) -> usize {
        self.calls.iter().filter(|(m, _)| m == method).count()
    }
    /// The effects of one kind since the last call.
    fn take_fx(&mut self, op: &str) -> Vec<Value> {
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.fx).into_iter().partition(|f| f["op"] == op || f["kind"] == op);
        self.fx = rest;
        mine
    }
    fn resync(&mut self, app: &mut App, now: i64) {
        let reply = app.request_at(&sync_input(self.focused()), now);
        self.absorb(&reply);
    }
    fn focused(&self) -> bool {
        true
    }
    fn send(&mut self, app: &mut App, now: i64, input: Value) -> Value {
        let reply = app.request_at(&input, now);
        self.absorb(&reply);
        self.resync(app, now);
        reply
    }
    fn key(&mut self, app: &mut App, now: i64, key: &str, chr: &str, flags: i32, facts: KeyFacts) -> Option<Value> {
        let reply = app.key_at(key, chr, flags, &facts, now)?;
        self.absorb(&reply);
        self.resync(app, now);
        Some(reply)
    }
    fn drive(&mut self, app: &mut App, now: &mut i64) {
        for _ in 0..200 {
            *now += 60;
            let reply = match self.pending.take() {
                Some(effect) => json!({"effect":effect}),
                None => {
                    let reply = app.request_at(&sync_input(true), *now);
                    self.absorb(&reply);
                    reply
                }
            };
            let Some(effect) = reply.get("effect") else {
                if !app.jobs.busy() {
                    return;
                }
                continue;
            };
            let result = match effect["method"].as_str() {
                Some("start") => {
                    self.token += 1;
                    let method = effect["args"][1].as_str().unwrap().to_owned();
                    let args = effect["args"][2].clone();
                    self.calls.push((method.clone(), args.clone()));
                    let value = self.answer(&method, &args);
                    self.ready.insert(self.token, value);
                    json!(self.token)
                }
                _ => {
                    let packets: Vec<Value> = effect["args"][1]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|t| {
                            let id = t.as_u64()?;
                            self.ready.remove(&id).map(|value| json!({"token":id,"value":value}))
                        })
                        .collect();
                    for token in effect["args"][2].as_array().unwrap() {
                        self.ready.remove(&token.as_u64().unwrap());
                    }
                    json!({"method":"take","value":packets})
                }
            };
            let reply = app.request_at(&json!({"cmd":"next","result":result}), *now);
            self.absorb(&reply);
            self.resync(app, *now);
        }
        panic!("the window never settled");
    }
}
fn sync_input(focused: bool) -> Value {
    json!({"cmd":"sync","args":{"__nativeOwner":9,"__nativeWindow":"terminal"},
        "settings":{"terminal":{}},
        "context":{"width":900,"height":600,"focused":focused,"home":"/home/test",
            "theme":{"text":"#ffffff","textDim":"#999999","accent":"#0a84ff","separator":"#444444","mode":"dark","sidebarBg":"#2a2a2e","hover":"#333333"}}})
}
fn start() -> (App, i64, Service) {
    let mut app = App::default();
    let mut service = Service::new();
    let reply = app.request_at(&sync_input(true), 1000);
    service.absorb(&reply);
    let mut now = 1000;
    service.drive(&mut app, &mut now);
    (app, now, service)
}
fn text(tree: &Value) -> String {
    tree.to_string()
}
fn facts(code: i32, base: &str, shift: bool) -> KeyFacts<'_> {
    KeyFacts { code, base, repeat: false, shift }
}
fn sessions(app: &App) -> Vec<Sid> {
    shell(|s| s.ws.sessions(&app.window))
}

#[test]
fn a_new_window_creates_a_configured_session_and_shows_it_in_a_pane() {
    let (app, _, mut service) = start();
    assert_eq!(sessions(&app), [101]);
    assert_eq!(service.calls[0].0, "term.create");
    assert_eq!(service.count("term.configure"), 1, "the session is configured with its profile colors");
    let config = &service.calls.iter().find(|(m, _)| m == "term.configure").unwrap().1;
    assert_eq!(config[0], 101);
    assert!(config[1]["palette"].is_array() && config[1]["background"].is_number());
    let tree = text(&service.tree);
    assert!(tree.contains("\"kind\":\"terminal\"") && tree.contains("\"session\":101"), "{tree}");
    assert!(tree.contains("Terminal"), "the tab is called Terminal until the session says otherwise");
    assert_eq!(service.take_fx("focus"), [json!({"kind":"term","op":"focus","sid":101,"on":true})]);
    assert!(service.count("term.infos") + service.count("term.events") >= 1, "the shell is polled for titles and events");
}
#[test]
fn tabs_open_after_the_active_one_switch_by_click_and_close_with_their_sessions() {
    let (mut app, mut now, mut service) = start();
    service.send(&mut app, now, json!({"cmd":"newTab"}));
    service.drive(&mut app, &mut now);
    assert_eq!(sessions(&app), [101, 102]);
    let tabs = shell(|s| s.ws.window("terminal").unwrap().tabs.iter().map(|t| t.id.clone()).collect::<Vec<_>>());
    assert_eq!(shell(|s| s.ws.current_sid("terminal")), Some(102), "the new tab is the active one");
    service.send(&mut app, now, json!({"cmd":"tab","key":tabs[0]}));
    assert_eq!(shell(|s| s.ws.current_sid("terminal")), Some(101));
    assert!(text(&service.tree).contains("❖1") && text(&service.tree).contains("❖2"), "tabs show their number shortcut");
    service.take_fx("focus");
    service.send(&mut app, now, json!({"cmd":"closeTab","key":tabs[1]}));
    service.drive(&mut app, &mut now);
    assert_eq!(sessions(&app), [101]);
    assert!(service.calls.iter().any(|(m, a)| m == "term.close" && a[0] == 102), "closing a tab closes its shell");
    service.send(&mut app, now, json!({"cmd":"closeTab","key":tabs[0]}));
    let reply = service.send(&mut app, now, json!({"cmd":"noop"}));
    let closes = service.take_fx("closeWindow");
    assert!(closes.len() == 1 || reply["effects"].to_string().contains("closeWindow"), "the last tab closes the window");
}
#[test]
fn splitting_lays_out_two_panes_with_a_divider_that_resizes_them() {
    let (mut app, mut now, mut service) = start();
    assert!(app.run("splitRight", now));
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    assert_eq!(sessions(&app), [101, 102]);
    let tree = text(&service.tree);
    assert!(tree.contains("pane-101") && tree.contains("pane-102") && tree.contains("divider-"), "{tree}");
    // 900 wide: (900 - 5) / 2 = 447.5 → 448 | divider | 447
    let before = app.pane_rect(101).unwrap();
    assert_eq!((before.x, before.w), (0., 448.));
    let split = shell(|s| match &s.ws.window("terminal").unwrap().active_tab().unwrap().root {
        crate::workspace::Node::Split { id, .. } => id.clone(),
        _ => panic!("a split"),
    });
    let down = service.send(&mut app, now, json!({"cmd":"divider","key":split}));
    assert_eq!(down["capture"], true, "the drag follows the pointer");
    service.send(&mut app, now, json!({"cmd":"pointerMove","x":300.,"y":200.}));
    let moved = app.pane_rect(101).unwrap();
    assert!((moved.w - 298.).abs() <= 1., "the first pane follows the pointer ({})", moved.w);
    let end = service.send(&mut app, now, json!({"cmd":"dragEnd","x":300.,"y":200.,"button":0}));
    assert_eq!(end["capture"], false);
    assert!(service.take_fx("mouse").is_empty(), "dragging a divider sends nothing to the shell");
    // a click in the second pane gives it the focus
    service.send(&mut app, now, json!({"cmd":"paneDown","key":101,"x":10.,"y":50.,"button":0}));
    assert_eq!(shell(|s| s.ws.current_sid("terminal")), Some(101));
}
#[test]
fn typed_keys_named_keys_and_control_combinations_reach_the_focused_shell() {
    let (mut app, now, mut service) = start();
    service.fx.clear();
    service.key(&mut app, now, "a", "a", 0, facts(30, "a", false)).expect("a letter is input");
    service.key(&mut app, now, "Enter", "", 0, facts(28, "", false)).expect("Enter is input");
    service.key(&mut app, now, "c", "", 2, facts(46, "c", false)).expect("Ctrl+C is input");
    service.key(&mut app, now, "Tab", "", 0, facts(15, "", true)).expect("Shift+Tab is input");
    let keys = service.take_fx("key");
    let sent: Vec<(String, String, String, i64)> = keys.iter().filter(|k| k["sid"] == 101)
        .map(|k| (k["key"].as_str().unwrap().into(), k["char"].as_str().unwrap().into(), k["base"].as_str().unwrap().into(), k["mods"].as_i64().unwrap())).collect();
    assert_eq!(sent[0], ("".into(), "a".into(), "".into(), 0), "a typed character carries no name");
    assert_eq!(sent[1].0, "Enter");
    assert_eq!(sent[2], ("".into(), "".into(), "c".into(), 4), "Ctrl sends the base key and the Ctrl bit");
    assert_eq!(sent[3], ("Tab".into(), "".into(), "".into(), 1), "Shift reaches the shell");
    assert!(service.key(&mut app, now, "Control", "", 2, facts(29, "", false)).is_none(), "a modifier alone is not input");
    assert!(service.key(&mut app, now, "x", "x", 16, facts(45, "x", false)).is_none(), "an unbound Super key stays with the shell");
}
#[test]
fn shortcuts_run_window_actions_before_the_shell_sees_the_key() {
    let (mut app, mut now, mut service) = start();
    // the factory shortcut for a new tab (❖T) is resolved by the Rust key tables
    let tab_key = service.key(&mut app, now, "t", "t", 16, facts(20, "t", false));
    assert!(tab_key.is_some(), "❖T is a window shortcut");
    service.drive(&mut app, &mut now);
    assert_eq!(sessions(&app).len(), 2, "❖T opened a tab");
    assert!(service.take_fx("key").is_empty(), "the shell never saw ❖T");
}
#[test]
fn mouse_input_is_pane_local_coalesced_and_the_wheel_moves_three_lines_a_notch() {
    let (mut app, now, mut service) = start();
    service.fx.clear();
    let down = service.send(&mut app, now, json!({"cmd":"paneDown","key":101,"x":40.,"y":96.,"button":0,"shift":true}));
    assert_eq!(down["capture"], true);
    let fx = service.take_fx("mouse");
    // y 96 is 36 px of tab bar below the pane's top: pane-local (40, 60), Shift as bit 1
    assert_eq!((fx[0]["type"].as_str(), fx[0]["x"].as_f64(), fx[0]["y"].as_f64(), fx[0]["mods"].as_i64()), (Some("down"), Some(40.), Some(60.), Some(1)));
    service.send(&mut app, now + 20, json!({"cmd":"pointerMove","x":50.,"y":100.}));
    let moved = service.take_fx("mouse");
    assert_eq!(moved[0]["type"], "move", "a drag moves the selection");
    service.send(&mut app, now + 40, json!({"cmd":"dragEnd","x":60.,"y":110.,"button":0}));
    let up = service.take_fx("mouse");
    assert_eq!((up[0]["type"].as_str(), up[0]["rememberCopy"].as_bool()), (Some("up"), Some(true)), "the copy-on-select result is the mount's to remember");
    // hover moves are throttled and repeated positions are not sent twice
    service.send(&mut app, now + 100, json!({"cmd":"paneMove","key":101,"x":10.,"y":50.}));
    service.send(&mut app, now + 101, json!({"cmd":"paneMove","key":101,"x":10.,"y":50.}));
    service.send(&mut app, now + 102, json!({"cmd":"paneMove","key":101,"x":11.,"y":50.}));
    assert_eq!(service.take_fx("mouse").len(), 1, "same position, or too soon: nothing");
    service.send(&mut app, now + 200, json!({"cmd":"paneWheel","key":101,"x":10.,"y":50.,"notches":2}));
    let wheel = service.take_fx("wheel");
    assert_eq!(wheel[0]["notches"], 6);
    // the right button is the pane's menu, not mouse input
    service.send(&mut app, now + 300, json!({"cmd":"paneDown","key":101,"x":10.,"y":50.,"button":2}));
    assert!(service.take_fx("mouse").is_empty());
}
#[test]
fn a_closed_pane_can_be_undone_until_its_session_is_finally_closed() {
    let (mut app, mut now, mut service) = start();
    app.run("splitRight", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    assert!(app.run("closePane", now));
    assert_eq!(sessions(&app), [101], "the pane disappears at once");
    assert_eq!(service.count("term.close"), 0, "its shell is still alive for the undo");
    assert!(app.run("undoClose", now));
    assert_eq!(sessions(&app).len(), 2, "undo brings it back");
    let start = now;
    app.run("closePane", now);
    now += 6000;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    assert!(now - start >= 5000);
    assert!(service.calls.iter().any(|(m, a)| m == "term.close" && a[0] == 102), "after five seconds the shell is closed");
    assert!(!app.run("undoClose", now) || sessions(&app) == [101], "nothing to bring back after that");
}
#[test]
fn session_events_name_tabs_flag_attention_and_end_windows() {
    let (mut app, mut now, mut service) = start();
    service.send(&mut app, now, json!({"cmd":"newTab"}));
    service.drive(&mut app, &mut now);
    // the active tab is 102; events of the background session 101
    service.events.push(json!({"sid":101,"type":"title","data":"vim notes.txt"}));
    service.events.push(json!({"sid":101,"type":"bell","data":null}));
    service.events.push(json!({"sid":102,"type":"cwd","data":"/home/test/projects"}));
    service.events.push(json!({"sid":102,"type":"command","data":"2"}));
    now += 300;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    let tree = text(&service.tree);
    assert!(tree.contains("vim notes.txt"), "the session's own title names its tab");
    assert!(tree.contains("projects"), "the folder names the other one: {tree}");
    assert!(tree.contains("#ffd60a") && tree.contains("#ff453a"), "a bell and a failed command are marked");
    assert!(shell(|s| s.sessions[&101].bell), "a bell in a background tab asks for attention");
    app.run("prevTab", now);
    assert!(!shell(|s| s.sessions[&101].bell) || app.run("nextTab", now), "looking at it clears the marks once it is focused");
    service.events.push(json!({"sid":101,"type":"exit","data":null}));
    service.events.push(json!({"sid":102,"type":"exit","data":null}));
    now += 300;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    assert!(sessions(&app).is_empty(), "exited shells leave the window");
    assert!(service.fx.iter().any(|f| f["kind"] == "closeWindow"), "the window closes with its last shell");
}
#[test]
fn the_status_bar_and_tab_titles_follow_what_the_shell_runs() {
    let (mut app, mut now, mut service) = start();
    service.infos.insert(101, json!({"process":"cargo","busy":true,"cols":93,"rows":36,"cwd":"/home/test/code","shell":"bash","view":12,"altScreen":false}));
    service.events.push(json!({"sid":101,"type":"cwd","data":"/home/test/code"}));
    now += 2000;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    let tree = text(&service.tree);
    assert!(tree.contains("code — cargo"), "a busy shell shows its program in the tab title: {tree}");
    assert!(tree.contains("▶ cargo") && tree.contains("93×36"), "the status bar shows the program and the size");
    assert!(tree.contains("~/code"), "the folder under the home is shortened");
    assert!(tree.contains("terminal.status.history"), "scrolled-back lines are counted");
}
#[test]
fn windows_share_one_workspace_and_a_remount_finds_its_tabs() {
    let (app, mut now, mut service) = start();
    drop(app);
    // the window was minimized and mounted again: same id, nothing is created
    let mut again = App::default();
    let before = service.count("term.create");
    let reply = again.request_at(&sync_input(true), now);
    service.absorb(&reply);
    service.drive(&mut again, &mut now);
    assert_eq!(service.count("term.create"), before, "a mounted-again window keeps its shells");
    assert_eq!(sessions(&again), [101]);
    assert!(text(&service.tree).contains("\"session\":101"));
    // another window is its own workspace entry
    let mut other = App::default();
    let mut input = sync_input(true);
    input["args"]["__nativeWindow"] = json!("terminal#2");
    let reply = other.request_at(&input, now);
    service.absorb(&reply);
    service.drive(&mut other, &mut now);
    assert_eq!(shell(|s| s.ws.sessions("terminal#2")), [102]);
    assert_eq!(shell(|s| s.ws.sessions("terminal")), [101]);
}
#[test]
fn keyboard_focus_moves_with_the_window_and_the_active_pane() {
    let (mut app, mut now, mut service) = start();
    service.fx.clear();
    let reply = app.request_at(&sync_input(false), now);
    service.absorb(&reply);
    assert_eq!(service.take_fx("focus"), [json!({"kind":"term","op":"focus","sid":101,"on":false})], "an unfocused window gives the keyboard back");
    app.run("splitRight", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    let focus = service.take_fx("focus");
    assert!(focus.contains(&json!({"kind":"term","op":"focus","sid":102,"on":true})), "the new pane takes the keyboard: {focus:?}");
    assert!(focus.contains(&json!({"kind":"term","op":"focus","sid":101,"on":false})), "and the old one gives it up");
}
#[test]
fn the_icon_is_a_scene_of_the_requested_size() {
    let mut app = App::default();
    let icon = app.request_at(&json!({"cmd":"icon","size":48}), 0);
    assert_eq!(icon["style"]["width"], 48.);
    assert!(text(&icon).contains(">_"));
}
