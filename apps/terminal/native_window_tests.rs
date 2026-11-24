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
    copy_text: String,
    clip: String,
    output: Option<String>,
    logical: String,
    found: Value,
    prefix: String,
    words: String,
    history: Vec<String>,
    replay: Value,
    copy_key: Value,
}
impl Service {
    fn new() -> Self {
        Self { token: 0, ready: Default::default(), calls: vec![], next_sid: 100, pending: None, events: vec![], infos: HashMap::new(), fx: vec![], tree: Value::Null,
            copy_text: String::new(), clip: String::new(), output: None, logical: String::new(), found: json!({"count":0,"index":-1}),
            prefix: String::new(), words: String::new(), history: vec![], replay: json!({"count":0}), copy_key: json!({"status":0}) }
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
            "term.copy" => json!(self.copy_text),
            "clipboard.read" => json!({"kind":"text","text":self.clip}),
            "term.selectOutput" => self.output.clone().map_or(Value::Null, |o| json!(o)),
            "term.logicalText" => json!(self.logical),
            "term.cursorWord" => json!(self.prefix),
            "term.words" => json!(self.words),
            "clipboard.history" => json!(self.history),
            "term.replay" => self.replay.clone(),
            "term.copyKey" => self.copy_key.clone(),
            "term.search" | "term.searchMove" | "term.setMatches" => self.found.clone(),
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

fn last_call(service: &Service, method: &str) -> Value {
    service.calls.iter().rev().find(|(m, _)| m == method).map(|(_, a)| a.clone()).unwrap_or(Value::Null)
}
fn split_window(app: &mut App, service: &mut Service, now: &mut i64) {
    app.run("splitRight", *now);
    service.send(app, *now, json!({"cmd":"noop"}));
    service.drive(app, now);
}

#[test]
fn copy_sends_the_selection_to_the_clipboard_history_and_paste_reads_it_back() {
    let (mut app, mut now, mut service) = start();
    service.copy_text = "selected words".into();
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(app.run("copy", now));
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "clipboard.remember"), json!(["selected words"]));
    service.copy_text.clear();
    let before = service.count("clipboard.remember");
    app.run("copy", now);
    service.drive(&mut app, &mut now);
    assert_eq!(service.count("clipboard.remember"), before, "an empty selection copies nothing");
    service.fx.clear();
    service.clip = "ls -la\n".into();
    app.run("paste", now);
    service.drive(&mut app, &mut now);
    let pasted = service.take_fx("paste");
    assert_eq!((pasted[0]["sid"].as_i64(), pasted[0]["text"].as_str()), (Some(101), Some("ls -la\n")), "paste is bracketed by the engine, so the text goes as it is");
}
#[test]
fn paste_variants_flatten_lines_or_quote_the_text_for_the_shell() {
    let (mut app, mut now, mut service) = start();
    service.clip = "  first \n\n  second\n third ".into();
    app.run("pasteOneLine", now);
    service.drive(&mut app, &mut now);
    assert_eq!(service.take_fx("paste")[0]["text"], "first second third");
    service.clip = "it's here".into();
    app.run("pasteEscaped", now);
    service.drive(&mut app, &mut now);
    assert_eq!(service.take_fx("paste")[0]["text"], "'it'\\''s here'");
    service.clip.clear();
    app.run("paste", now);
    service.drive(&mut app, &mut now);
    assert!(service.take_fx("paste").is_empty(), "an empty clipboard pastes nothing");
}
#[test]
fn selecting_the_last_output_copies_it_or_says_there_is_none() {
    let (mut app, mut now, mut service) = start();
    service.output = Some("build finished".into());
    app.run("selectOutput", now);
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "clipboard.remember"), json!(["build finished"]));
    assert!(text(&service.tree).contains("terminal.toast.outputCopied"));
    service.output = None;
    now += 5000;
    app.run("selectOutput", now);
    service.drive(&mut app, &mut now);
    assert!(text(&service.tree).contains("terminal.toast.noOutput"), "no marked output: the person is told why");
}
#[test]
fn closing_a_pane_that_runs_a_program_asks_first_and_an_idle_one_closes_at_once() {
    let (mut app, mut now, mut service) = start();
    split_window(&mut app, &mut service, &mut now);
    // idle shells close without asking, and say how to undo
    app.run("closePane", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert_eq!(sessions(&app), [101]);
    assert!(text(&service.tree).contains("terminal.toast.paneClosed") && !text(&service.tree).contains("terminal.dialog"));
    app.run("undoClose", now);
    // a program is running in the focused pane
    service.infos.insert(102, json!({"alive":true,"busy":true,"process":"sleep","cols":80,"rows":24}));
    now += 2000;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    app.run("closePane", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    let tree = text(&service.tree);
    assert!(tree.contains("terminal.dialog.close.pane") && tree.contains("terminal.dialog.busy") && tree.contains("sleep"), "{tree}");
    assert_eq!(sessions(&app).len(), 2, "nothing closed yet");
    // Escape cancels
    service.key(&mut app, now, "Escape", "", 0, facts(1, "", false)).expect("the dialog takes the key");
    assert!(!text(&service.tree).contains("terminal.dialog.close.pane"));
    assert_eq!(sessions(&app).len(), 2);
    // the Close button confirms
    app.run("closePane", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.send(&mut app, now, json!({"cmd":"dialogConfirm"}));
    assert_eq!(sessions(&app), [101]);
    // Enter confirms too, and a dialog takes keys that would have gone to the shell
    service.infos.insert(101, json!({"alive":true,"busy":true,"process":"vim","cols":80,"rows":24}));
    now += 2000;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    app.run("closeTab", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(text(&service.tree).contains("terminal.dialog.close.tab"));
    service.fx.clear();
    service.key(&mut app, now, "a", "a", 0, facts(30, "a", false)).expect("the dialog swallows typing");
    assert!(service.take_fx("key").is_empty());
    service.key(&mut app, now, "Enter", "", 0, facts(28, "", false)).unwrap();
    assert!(sessions(&app).is_empty(), "Enter closes the tab");
}
#[test]
fn toasts_stay_four_seconds_and_at_most_four_at_a_time() {
    let (mut app, mut now, mut service) = start();
    for _ in 0..6 {
        app.run("undoClose", now);
    }
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert_eq!(text(&service.tree).matches("terminal.toast.nothingToUndo").count(), 4, "the oldest ones make room");
    now += 4100;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(!text(&service.tree).contains("terminal.toast"), "they leave by themselves");
}
#[test]
fn the_find_bar_searches_moves_and_closes_with_the_engine() {
    let (mut app, mut now, mut service) = start();
    service.key(&mut app, now, "f", "f", 16, facts(33, "f", false)).expect("❖F opens the bar");
    assert!(text(&service.tree).contains("terminal.find.placeholder"));
    service.found = json!({"count":3,"index":0});
    service.send(&mut app, now, json!({"cmd":"findChange","value":"foo"}));
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.search"), json!([101, "foo", false]));
    assert!(text(&service.tree).contains("terminal.find.position"), "the position is shown");
    service.send(&mut app, now, json!({"cmd":"findKey","key":"Enter","shift":false}));
    assert_eq!(last_call(&service, "term.searchMove").as_array().map(|a| a.len()), None, "jobs run on the next drive");
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.searchMove"), json!([101, -1]), "Enter goes to the previous match");
    service.send(&mut app, now, json!({"cmd":"findKey","key":"Enter","shift":true}));
    service.send(&mut app, now, json!({"cmd":"findMove","key":-5}));
    service.drive(&mut app, &mut now);
    let moves: Vec<Value> = service.calls.iter().filter(|(m, _)| m == "term.searchMove").map(|(_, a)| a[1].clone()).collect();
    assert_eq!(moves, [json!(-1), json!(1), json!(-1)], "Shift+Enter reverses and a move is a single step");
    service.send(&mut app, now, json!({"cmd":"findCs"}));
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.search"), json!([101, "foo", true]), "case sensitivity searches again");
    // no results
    service.found = json!({"count":0,"index":-1});
    service.send(&mut app, now, json!({"cmd":"findChange","value":"zzz"}));
    service.drive(&mut app, &mut now);
    assert!(text(&service.tree).contains("terminal.find.none"));
    service.send(&mut app, now, json!({"cmd":"findClose"}));
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.search"), json!([101, "", false]), "closing clears the highlights");
    assert!(!text(&service.tree).contains("terminal.find.placeholder"));
}
#[test]
fn regular_expression_search_hands_the_engine_line_start_and_length_triples() {
    let (mut app, mut now, mut service) = start();
    app.run("find", now);
    service.logical = "foo\nbar Foo foo\n\nñandú foo".into();
    service.found = json!({"count":4,"index":3});
    service.send(&mut app, now, json!({"cmd":"findRegex"}));
    service.send(&mut app, now, json!({"cmd":"findChange","value":"fo+"}));
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.logicalText"), json!([101]));
    assert_eq!(last_call(&service, "term.setMatches"), json!([101, [0, 0, 3, 1, 4, 3, 1, 8, 3, 3, 6, 3]]), "characters, not bytes: ñandú counts five");
    // case sensitive: the capital F no longer matches
    service.send(&mut app, now, json!({"cmd":"findCs"}));
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.setMatches"), json!([101, [0, 0, 3, 1, 8, 3, 3, 6, 3]]));
    // an invalid pattern is reported, not searched
    let before = service.count("term.setMatches");
    service.send(&mut app, now, json!({"cmd":"findChange","value":"("}));
    service.drive(&mut app, &mut now);
    assert_eq!(service.count("term.setMatches"), before);
    assert!(text(&service.tree).contains("terminal.find.badRegex"));
}
#[test]
fn the_find_bar_follows_the_focused_pane() {
    let (mut app, mut now, mut service) = start();
    app.run("find", now);
    service.send(&mut app, now, json!({"cmd":"findChange","value":"x"}));
    service.drive(&mut app, &mut now);
    split_window(&mut app, &mut service, &mut now);
    assert_eq!(last_call(&service, "term.search"), json!([102, "x", false]), "the search moves to the new pane");
    assert!(service.calls.iter().any(|(m, a)| m == "term.search" && a == &json!([101, "", false])), "and is cleared in the old one");
}

fn second_window(service: &mut Service, now: &mut i64) -> App {
    let mut other = App::default();
    let mut input = sync_input(true);
    input["args"]["__nativeWindow"] = json!("terminal#2");
    let reply = other.request_at(&input, *now);
    service.absorb(&reply);
    service.drive(&mut other, now);
    other
}
fn writes(service: &mut Service, op: &str) -> Vec<String> {
    service.take_fx(op).iter().map(|f| f["text"].as_str().unwrap_or("").to_owned()).collect()
}

#[test]
fn autocomplete_offers_the_words_on_screen_and_types_the_rest_of_the_one_chosen() {
    let (mut app, mut now, mut service) = start();
    service.prefix = "gi".into();
    service.words = "git\ngit-lfs\n\ngist".into();
    app.run("autocomplete", now);
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.words"), json!([101, "gi"]));
    let tree = text(&service.tree);
    assert!(tree.contains("terminal.popup.complete") && tree.contains("git-lfs") && tree.contains("terminal.popup.keys"), "{tree}");
    service.fx.clear();
    service.key(&mut app, now, "ArrowDown", "", 0, facts(108, "", false)).expect("the popup takes the arrows");
    service.key(&mut app, now, "Enter", "", 0, facts(28, "", false)).expect("and Enter");
    assert_eq!(writes(&mut service, "write"), ["t-lfs"], "only what the prefix lacks is typed");
    assert!(!text(&service.tree).contains("terminal.popup.complete"), "the popup closes");
    service.prefix = "zz".into();
    service.words = String::new();
    app.run("autocomplete", now);
    service.drive(&mut app, &mut now);
    assert!(text(&service.tree).contains("terminal.popup.completeEmpty"));
    service.fx.clear();
    service.key(&mut app, now, "a", "a", 0, facts(30, "a", false)).expect("a list popup swallows typing");
    assert!(service.take_fx("key").is_empty());
    service.key(&mut app, now, "Escape", "", 0, facts(1, "", false)).unwrap();
    assert!(!text(&service.tree).contains("terminal.popup"));
}
#[test]
fn command_history_lists_the_newest_first_without_repeats_and_types_the_choice() {
    let (mut app, mut now, mut service) = start();
    for cmd in ["ls -la", "cargo test", "ls -la"] {
        service.events.push(json!({"sid":101,"type":"run","data":cmd}));
    }
    now += 300;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    app.run("commandHistory", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    let tree = text(&service.tree);
    assert!(tree.find("ls -la").unwrap() < tree.find("cargo test").unwrap(), "the repeated command moved to the front: {tree}");
    assert_eq!(tree.matches("ls -la").count(), 1);
    service.fx.clear();
    service.key(&mut app, now, "ArrowDown", "", 0, facts(108, "", false)).unwrap();
    service.key(&mut app, now, "Enter", "", 0, facts(28, "", false)).unwrap();
    assert_eq!(writes(&mut service, "write"), ["cargo test"], "typed, not run");
}
#[test]
fn paste_history_shows_lines_flattened_and_pastes_the_original_text() {
    let (mut app, mut now, mut service) = start();
    service.history = vec!["one\ntwo".into(), "three".into()];
    app.run("pasteHistory", now);
    service.drive(&mut app, &mut now);
    assert!(text(&service.tree).contains("one⏎ two"), "a line break is shown as ⏎");
    service.fx.clear();
    service.key(&mut app, now, "Enter", "", 0, facts(28, "", false)).unwrap();
    assert_eq!(writes(&mut service, "paste"), ["one\ntwo"], "the text goes back as it was copied");
}
#[test]
fn open_quickly_finds_sessions_in_any_window_and_brings_their_window_forward() {
    let (mut app, mut now, mut service) = start();
    let mut other = second_window(&mut service, &mut now);
    service.events.push(json!({"sid":101,"type":"title","data":"notes"}));
    service.events.push(json!({"sid":102,"type":"title","data":"server logs"}));
    now += 300;
    service.send(&mut other, now, json!({"cmd":"noop"}));
    service.drive(&mut other, &mut now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    app.run("openQuickly", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(text(&service.tree).contains("terminal.popup.quick") && text(&service.tree).contains("server logs"), "{}", text(&service.tree));
    service.send(&mut app, now, json!({"cmd":"quickChange","value":"LOG"}));
    let tree = text(&service.tree);
    assert!(tree.contains("server logs") && tree.contains("\"item-0\"") && !tree.contains("\"item-1\""), "the filter ignores case and leaves one match");
    service.fx.clear();
    service.send(&mut app, now, json!({"cmd":"quickPick"}));
    assert_eq!(shell(|s| s.ws.current_sid("terminal#2")), Some(102));
    assert!(service.fx.iter().any(|f| f["kind"] == "focusWindow" && f["id"] == "terminal#2"), "a session of another window brings it forward");
    assert!(!text(&service.tree).contains("terminal.popup.quick"));
}
#[test]
fn the_tab_title_can_be_set_by_hand_and_cleared_back_to_automatic() {
    let (mut app, mut now, mut service) = start();
    app.run("editTitle", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(text(&service.tree).contains("terminal.popup.title"));
    service.send(&mut app, now, json!({"cmd":"titleChange","value":"  deploy "}));
    let reply = service.send(&mut app, now, json!({"cmd":"titleDone"}));
    assert!(text(&service.tree).contains("deploy") && !text(&service.tree).contains("terminal.popup.title"));
    let _ = reply;
    app.run("editTitle", now);
    service.send(&mut app, now, json!({"cmd":"titleChange","value":""}));
    service.send(&mut app, now, json!({"cmd":"titleDone"}));
    assert!(text(&service.tree).contains("Terminal") && !text(&service.tree).contains("deploy"), "empty means automatic again");
    now += 1;
    let _ = now;
}
#[test]
fn the_composer_sends_a_line_or_just_the_text_and_broadcast_reaches_every_pane() {
    let (mut app, mut now, mut service) = start();
    app.run("composer", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(text(&service.tree).contains("terminal.composer.hint"));
    service.send(&mut app, now, json!({"cmd":"composerChange","value":"echo hi"}));
    service.fx.clear();
    service.send(&mut app, now, json!({"cmd":"composerKey","key":"Enter","shift":false}));
    assert_eq!(writes(&mut service, "paste"), ["echo hi\n"], "Enter sends the line");
    service.send(&mut app, now, json!({"cmd":"composerChange","value":"half"}));
    service.send(&mut app, now, json!({"cmd":"composerKey","key":"Enter","shift":true}));
    assert_eq!(writes(&mut service, "paste"), ["half"], "Shift+Enter leaves it unrun");
    // with broadcast on, every pane of the tab gets the same input
    split_window(&mut app, &mut service, &mut now);
    app.run("broadcast", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(text(&service.tree).contains("terminal.broadcasting") && text(&service.tree).contains("terminal.composer.broadcast"));
    service.send(&mut app, now, json!({"cmd":"composerChange","value":"date"}));
    service.fx.clear();
    service.send(&mut app, now, json!({"cmd":"composerKey","key":"Enter","shift":false}));
    let pasted = service.take_fx("paste");
    assert_eq!(pasted.iter().map(|f| f["sid"].as_i64().unwrap()).collect::<Vec<_>>(), [101, 102]);
    service.key(&mut app, now, "a", "a", 0, facts(30, "a", false)).unwrap();
    assert_eq!(service.take_fx("key").len(), 2, "typing reaches both panes too");
    service.send(&mut app, now, json!({"cmd":"composerClose"}));
    assert!(!text(&service.tree).contains("terminal.composer"));
}
#[test]
fn the_toolbelt_lists_jobs_commands_directories_and_notes_and_makes_room() {
    let (mut app, mut now, mut service) = start();
    let wide = app.pane_rect(101).unwrap().w;
    service.history = vec!["copied text".into()];
    service.events.push(json!({"sid":101,"type":"run","data":"make"}));
    service.events.push(json!({"sid":101,"type":"cwd","data":"/home/test/it's"}));
    now += 300;
    service.send(&mut app, now, json!({"cmd":"noop"}));
    service.drive(&mut app, &mut now);
    app.run("toolbelt", now);
    service.drive(&mut app, &mut now);
    let tree = text(&service.tree);
    for needle in ["terminal.belt.jobs", "terminal.belt.commands", "make", "terminal.belt.clipboard", "copied text", "terminal.belt.dirs", "~/it's", "terminal.belt.profiles", "terminal.belt.notes"] {
        assert!(tree.contains(needle), "the toolbelt shows {needle}: {tree}");
    }
    assert_eq!(app.pane_rect(101).unwrap().w, wide - 240., "the panes give the toolbelt its 240 px");
    service.fx.clear();
    service.send(&mut app, now, json!({"cmd":"beltCommand","key":0}));
    service.send(&mut app, now, json!({"cmd":"beltClip","key":0}));
    service.send(&mut app, now, json!({"cmd":"beltDir","key":0}));
    assert_eq!(writes(&mut service, "write"), ["make", "cd '/home/test/it'\\''s'\r"], "commands are typed and folders are quoted");
    assert_eq!(writes(&mut service, "paste"), ["copied text"]);
    service.send(&mut app, now, json!({"cmd":"beltProfile","key":"default"}));
    service.drive(&mut app, &mut now);
    assert_eq!(sessions(&app).len(), 2, "a profile row opens a tab");
    service.send(&mut app, now, json!({"cmd":"beltNoteChange","value":"  call back "}));
    service.send(&mut app, now, json!({"cmd":"beltNoteAdd"}));
    assert!(text(&service.tree).contains("call back"));
    service.send(&mut app, now, json!({"cmd":"beltNoteDelete","key":0}));
    assert!(!text(&service.tree).contains("call back"), "a note goes when clicked");
    app.run("toolbelt", now);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert_eq!(app.pane_rect(102).unwrap().w, wide, "closing it gives the room back");
}
#[test]
fn instant_replay_steps_through_the_recorded_screens_and_goes_back_to_now() {
    let (mut app, mut now, mut service) = start();
    service.replay = json!({"count":0});
    app.run("instantReplay", now);
    service.drive(&mut app, &mut now);
    assert!(text(&service.tree).contains("terminal.toast.noReplay") && !text(&service.tree).contains("Instant Replay"));
    service.replay = json!({"count":5,"index":4,"time":1000});
    now += 5000;
    app.run("instantReplay", now);
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.replay"), json!([101, "enter"]));
    let tree = text(&service.tree);
    assert!(tree.contains("Instant Replay") && tree.contains("5 / 5") && tree.contains("terminal.replay.when"), "{tree}");
    service.replay = json!({"count":5,"index":3,"time":900});
    service.fx.clear();
    service.key(&mut app, now, "ArrowLeft", "", 0, facts(105, "", false)).expect("the replay takes the arrows");
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.replay"), json!([101, 3]));
    assert!(text(&service.tree).contains("4 / 5"));
    service.key(&mut app, now, "a", "a", 0, facts(30, "a", false)).unwrap();
    assert!(service.take_fx("key").is_empty(), "nothing reaches the shell while replaying");
    service.key(&mut app, now, "Escape", "", 0, facts(1, "", false)).unwrap();
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.replay"), json!([101, "exit"]));
    assert!(!text(&service.tree).contains("Instant Replay"), "back to now");
}
#[test]
fn copy_mode_hands_its_keys_to_the_engine_and_copies_what_it_selects() {
    let (mut app, mut now, mut service) = start();
    service.fx.clear();
    app.run("copyMode", now);
    service.drive(&mut app, &mut now);
    assert_eq!(service.take_fx("copyMode")[0]["on"], true);
    assert!(text(&service.tree).contains("terminal.copyModeBar"));
    service.copy_key = json!({"status":0});
    service.key(&mut app, now, "j", "j", 0, facts(36, "j", false)).expect("copy mode takes every key");
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.copyKey"), json!([101, "j", false]));
    assert!(text(&service.tree).contains("terminal.copyModeBar"), "status 0 keeps moving");
    service.key(&mut app, now, "v", "", 2, facts(47, "v", false)).unwrap();
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "term.copyKey"), json!([101, "v", true]), "Ctrl uses the base key");
    assert!(service.take_fx("key").is_empty(), "no key reached the shell");
    service.copy_key = json!({"status":2,"text":"picked text"});
    service.key(&mut app, now, "y", "y", 0, facts(21, "y", false)).unwrap();
    service.drive(&mut app, &mut now);
    assert_eq!(last_call(&service, "clipboard.remember"), json!(["picked text"]));
    assert!(text(&service.tree).contains("terminal.toast.copied") && !text(&service.tree).contains("terminal.copyModeBar"), "copied, and copy mode ends");
}
