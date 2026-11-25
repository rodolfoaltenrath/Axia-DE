//! What a session's events and a profile's triggers do in the native window: the toasts, the
//! visual bell, the "alert when the command finishes" action, the captured output of the
//! toolbelt, the actions the triggers run, and the profile that rules or the shell choose.
use super::prefs_tests::{start, start_with, tree, Service};
use super::*;

/// Delivers events the way the transport does: queued, then read by the window's next poll.
fn deliver(app: &mut App, now: &mut i64, service: &mut Service, events: Vec<Value>) {
    service.events.extend(events);
    *now += 400;
    service.send(app, *now, json!({"cmd":"noop"}));
    service.drive(app, now);
}
fn with_triggers(triggers: Value) -> Value {
    json!({"profiles":[{"id":"default","triggers":triggers}],"defaultProfile":"default"})
}
fn output(sid: i64) -> Value {
    json!({"sid":sid,"type":"output","data":null})
}
fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn a_matching_line_fires_its_trigger_once_while_it_stays_on_screen() {
    let (mut app, mut now, mut service) = start_with(with_triggers(json!([{"regex":"error (\\w+)","action":"notify","param":"failed \\1"}])));
    service.screen.insert(101, "building\nerror disk\n".into());
    deliver(&mut app, &mut now, &mut service, vec![output(101)]);
    assert_eq!(occurrences(&tree(&service), "failed disk"), 1, "{}", tree(&service));
    assert_eq!(service.count("term.screenText"), 1);
    deliver(&mut app, &mut now, &mut service, vec![output(101)]);
    assert_eq!(occurrences(&tree(&service), "failed disk"), 1, "the same line does not fire again");
    service.screen.insert(101, "building\nerror net\n".into());
    deliver(&mut app, &mut now, &mut service, vec![output(101)]);
    assert!(tree(&service).contains("failed net"), "a new line does");
}

#[test]
fn a_profile_without_triggers_never_reads_the_screen() {
    let (mut app, mut now, mut service) = start();
    deliver(&mut app, &mut now, &mut service, vec![output(101), output(101)]);
    assert_eq!(service.count("term.screenText"), 0);
}

#[test]
fn triggers_that_are_all_off_never_read_the_screen() {
    let (mut app, mut now, mut service) = start_with(with_triggers(json!([{"regex":"x","action":"bell","enabled":false}])));
    deliver(&mut app, &mut now, &mut service, vec![output(101)]);
    assert_eq!(service.count("term.screenText"), 0);
}

#[test]
fn trigger_actions_write_mark_highlight_title_and_capture() {
    let triggers = json!([
        {"regex":"Password:","action":"send","param":"secret\\n"},
        {"regex":"checkpoint","action":"mark"},
        {"regex":"WARN","action":"highlight","param":"#ffffff,#ff0000"},
        {"regex":"job (\\d+)","action":"title","param":"job \\1"},
        {"regex":"result: .*","action":"capture"},
    ]);
    let (mut app, mut now, mut service) = start_with(with_triggers(triggers));
    service.screen.insert(101, "Password:\ncheckpoint\nWARN low\njob 7 started\n  result: 42  \n".into());
    service.fx.clear();
    deliver(&mut app, &mut now, &mut service, vec![output(101)]);
    assert!(service.fx.contains(&json!({"kind":"term","op":"write","sid":101,"text":"secret\r"})), "{:?}", service.fx);
    assert!(service.fx.contains(&json!({"kind":"term","op":"mark","sid":101,"what":"set"})));
    let highlight = service.calls.iter().find(|(m, _)| m == "term.highlight").map(|(_, a)| a.clone()).unwrap();
    assert_eq!(highlight, json!([101, "WARN", 0xffffff, 0xff0000]));
    assert!(tree(&service).contains("job 7"), "the title names the tab");
    assert_eq!(shell(|s| s.captured.clone()), [(101, "result: 42".to_owned())]);
    service.send(&mut app, now, json!({"cmd":"do","key":"toolbelt"}));
    service.drive(&mut app, &mut now);
    assert!(tree(&service).contains("terminal.belt.captured") && tree(&service).contains("result: 42"), "the toolbelt lists what was captured");
}

#[test]
fn the_bell_and_attention_triggers_flash_flag_and_bring_the_window_forward() {
    let (mut app, mut now, mut service) = start_with(with_triggers(json!([
        {"regex":"ding","action":"bell"},
        {"regex":"look","action":"attention","param":"come here"},
    ])));
    service.screen.insert(101, "ding\n".into());
    deliver(&mut app, &mut now, &mut service, vec![output(101)]);
    assert!(shell(|s| s.sessions[&101].bell), "a bell trigger rings even in the pane being used");
    assert!(shell(|s| s.sessions[&101].flash_until) > 0);
    service.screen.insert(101, "look\n".into());
    service.fx.clear();
    deliver(&mut app, &mut now, &mut service, vec![output(101)]);
    assert!(tree(&service).contains("come here"));
    assert!(service.fx.contains(&json!({"kind":"focusWindow","id":"terminal"})), "{:?}", service.fx);
}

#[test]
fn the_visual_bell_marks_and_flashes_the_session() {
    let (mut app, mut now, mut service) = start();
    service.send(&mut app, now, json!({"cmd":"newTab"}));
    service.drive(&mut app, &mut now);
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"bell","data":null})]);
    assert!(shell(|s| s.sessions[&101].bell), "the background session's bell is marked");
    assert!(shell(|s| s.sessions[&101].flash_until) > 0, "and it flashes");
}

#[test]
fn a_silenced_profile_ignores_the_bell() {
    let quiet = json!({"profiles":[{"id":"default","bell":"none"}],"defaultProfile":"default"});
    let (mut app, mut now, mut service) = start_with(quiet);
    service.send(&mut app, now, json!({"cmd":"newTab"}));
    service.drive(&mut app, &mut now);
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"bell","data":null})]);
    assert!(!shell(|s| s.sessions[&101].bell) && shell(|s| s.sessions[&101].flash_until) == 0);
}

#[test]
fn the_flash_overlay_is_drawn_only_while_it_lasts() {
    let (mut app, now, mut service) = start();
    shell(|s| s.sessions.get_mut(&101).unwrap().flash_until = now + 100_000);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(tree(&service).contains("\"key\":\"flash\""), "while it lasts");
    shell(|s| s.sessions.get_mut(&101).unwrap().flash_until = now - 1);
    service.send(&mut app, now, json!({"cmd":"noop"}));
    assert!(!tree(&service).contains("\"key\":\"flash\""), "and not after");
}

#[test]
fn alert_when_the_command_finishes_reports_it_once() {
    let (mut app, mut now, mut service) = start();
    service.send(&mut app, now, json!({"cmd":"do","key":"alertNextMark"}));
    assert!(tree(&service).contains("terminal.toast.alertOn"));
    assert!(shell(|s| s.sessions[&101].alert));
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"run","data":"make all"}), json!({"sid":101,"type":"command","data":"2"})]);
    let shown = tree(&service);
    assert!(shown.contains("terminal.toast.finishedCmd") && shown.contains("make all"), "{shown}");
    assert!(!shell(|s| s.sessions[&101].alert), "it asks once");
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"command","data":"0"})]);
    assert_eq!(occurrences(&tree(&service), "terminal.toast.finishedCmd"), 1, "the next command says nothing");
    service.send(&mut app, now, json!({"cmd":"do","key":"alertNextMark"}));
    service.send(&mut app, now, json!({"cmd":"do","key":"alertNextMark"}));
    assert!(tree(&service).contains("terminal.toast.alertOff"), "asking again cancels it");
}

#[test]
fn notifications_attention_and_clipboard_requests_from_the_shell_reach_the_window() {
    let (mut app, mut now, mut service) = start();
    service.clip = "from the program".into();
    service.fx.clear();
    deliver(
        &mut app,
        &mut now,
        &mut service,
        vec![json!({"sid":101,"type":"notify","data":"Build done"}), json!({"sid":101,"type":"attention","data":null}), json!({"sid":101,"type":"copy","data":null})],
    );
    let shown = tree(&service);
    assert!(shown.contains("Build done") && shown.contains("terminal.toast.attention") && shown.contains("terminal.toast.copied"), "{shown}");
    assert!(service.fx.contains(&json!({"kind":"focusWindow","id":"terminal"})));
    assert!(service.calls.iter().any(|(m, a)| m == "clipboard.remember" && a[0] == "from the program"), "what the program copied is kept in the history");
}

#[test]
fn rules_switch_the_profile_by_folder_and_give_it_back_when_none_fits() {
    let settings = json!({"profiles":[{"id":"default","name":"Home"},{"id":"prod","name":"Prod","rules":"~/prod/**","badge":"PROD"}],"defaultProfile":"default"});
    let (mut app, mut now, mut service) = start_with(settings);
    let configures = service.count("term.configure");
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"cwd","data":"/home/test/prod/api"})]);
    assert_eq!(shell(|s| s.sessions[&101].profile.clone()), "prod");
    assert_eq!(shell(|s| s.sessions[&101].badge.clone()), "PROD", "the profile's badge comes with it");
    assert!(service.count("term.configure") > configures, "the session is configured with the new profile");
    assert!(tree(&service).contains("terminal.toast.autoProfile") && tree(&service).contains("Prod"));
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"cwd","data":"/home/test/prod/api/src"})]);
    assert_eq!(shell(|s| s.sessions[&101].profile.clone()), "prod", "the same profile again changes nothing");
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"cwd","data":"/home/test/docs"})]);
    assert_eq!(shell(|s| s.sessions[&101].profile.clone()), "default", "out of the folder the first profile returns");
    assert_eq!(shell(|s| s.sessions[&101].badge.clone()), "");
}

#[test]
fn the_shell_can_name_the_profile_and_the_badge_survives_a_profile_change() {
    let settings = json!({"profiles":[{"id":"default"},{"id":"work","name":"Work"}],"defaultProfile":"default"});
    let (mut app, mut now, mut service) = start_with(settings);
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"badge","data":"MINE"}), json!({"sid":101,"type":"profile","data":"Work"})]);
    assert_eq!(shell(|s| s.sessions[&101].profile.clone()), "work", "found by name");
    assert_eq!(shell(|s| s.sessions[&101].badge.clone()), "MINE", "the program's badge stays when the profile has none");
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"profile","data":"default"})]);
    assert_eq!(shell(|s| s.sessions[&101].profile.clone()), "default", "found by id");
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"profile","data":"nobody"})]);
    assert_eq!(shell(|s| s.sessions[&101].profile.clone()), "default", "an unknown name changes nothing");
}

#[test]
fn a_reported_host_selects_the_rules_that_name_it() {
    let settings = json!({"profiles":[{"id":"default"},{"id":"box","name":"Box","rules":"devbox"}],"defaultProfile":"default"});
    let (mut app, mut now, mut service) = start_with(settings);
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"host","data":"me@devbox"})]);
    assert_eq!(shell(|s| s.sessions[&101].profile.clone()), "box");
    deliver(&mut app, &mut now, &mut service, vec![json!({"sid":101,"type":"host","data":"other"})]);
    assert_eq!(shell(|s| s.sessions[&101].profile.clone()), "default");
}
