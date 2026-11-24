use super::*;

fn window() -> Workspace {
    let mut w = Workspace::default();
    w.ensure_window("terminal", 1);
    w
}
fn tab_ids(w: &Workspace, wid: &str) -> Vec<String> {
    w.window(wid).unwrap().tabs.iter().map(|t| t.id.clone()).collect()
}
fn area() -> Rect {
    Rect { x: 0., y: 0., w: 1005., h: 505. }
}

#[test]
fn a_new_tab_opens_after_the_active_one_and_becomes_active() {
    let mut w = window();
    let first = w.window("terminal").unwrap().active.clone();
    let second = w.add_tab("terminal", 2).unwrap();
    w.select_tab("terminal", &first);
    let third = w.add_tab("terminal", 3).unwrap();
    assert_eq!(tab_ids(&w, "terminal"), [first, third.clone(), second], "inserted right after the active tab");
    assert_eq!(w.window("terminal").unwrap().active, third);
    assert_eq!(w.current_sid("terminal"), Some(3));
    assert!(w.add_tab("missing", 9).is_none());
    assert_eq!(w.ensure_window("terminal", 77), (), "an existing window is left alone");
    assert_eq!(w.sessions("terminal").len(), 3);
}
#[test]
fn tabs_cycle_wrap_move_and_report_the_session_they_show() {
    let mut w = window();
    w.add_tab("terminal", 2);
    w.add_tab("terminal", 3);
    let ids = tab_ids(&w, "terminal");
    w.select_tab("terminal", &ids[0]);
    assert_eq!(w.cycle_tab("terminal", -1), Some(3), "the first tab wraps to the last");
    assert_eq!(w.cycle_tab("terminal", 1), Some(1));
    assert_eq!(w.select_tab("terminal", "nope"), None);
    w.move_tab("terminal", &ids[0], 1);
    assert_eq!(tab_ids(&w, "terminal")[1], ids[0]);
    w.move_tab("terminal", &ids[0], 5);
    w.move_tab("terminal", &tab_ids(&w, "terminal")[0], -1);
    assert_eq!(tab_ids(&w, "terminal").len(), 3, "moves past the ends do nothing");
}
#[test]
fn splitting_replaces_the_focused_pane_and_focuses_the_new_session() {
    let mut w = window();
    let s1 = w.split("terminal", Dir::Row, 2).unwrap();
    let s2 = w.split("terminal", Dir::Column, 3).unwrap();
    assert_ne!(s1, s2);
    let tab = w.window("terminal").unwrap().active_tab().unwrap();
    assert_eq!(tab.root.leaves(), [1, 2, 3].map(Sid::from), "the new pane goes after the one it split");
    assert_eq!(tab.focus, 3);
    let Node::Split { dir, ratio, b, .. } = &tab.root else { panic!("a split") };
    assert_eq!((*dir, *ratio), (Dir::Row, 0.5));
    assert!(matches!(**b, Node::Split { dir: Dir::Column, .. }), "the second split lives where the focused pane was");
    w.focus_pane(1);
    w.split("terminal", Dir::Column, 4);
    assert_eq!(w.window("terminal").unwrap().active_tab().unwrap().root.leaves(), [1, 4, 2, 3].map(Sid::from));
}
#[test]
fn removing_panes_refocuses_and_empty_tabs_pick_the_previous_tab() {
    let mut w = window();
    w.split("terminal", Dir::Row, 2);
    w.toggle_zoom("terminal");
    assert!(w.window("terminal").unwrap().active_tab().unwrap().zoom);
    assert_eq!(w.detach(2), Removed::Remains);
    let tab = w.window("terminal").unwrap().active_tab().unwrap();
    assert_eq!((tab.focus, tab.root.leaves(), tab.zoom), (1, vec![1], false), "the remaining pane has the focus and zoom resets");
    let second = w.add_tab("terminal", 5).unwrap();
    let third = w.add_tab("terminal", 6).unwrap();
    assert_eq!(w.detach(6), Removed::Remains);
    assert_eq!(w.window("terminal").unwrap().active, second, "after the last tab closes the previous one is shown");
    let _ = third;
    assert_eq!(w.detach(1), Removed::Remains);
    assert_eq!(w.detach(5), Removed::WindowEmpty("terminal".into()), "the window closes with its last session");
    assert_eq!(w.detach(99), Removed::Unknown);
}
#[test]
fn a_closed_pane_waits_for_its_undo_and_comes_back_as_a_tab() {
    let mut w = window();
    w.split("terminal", Dir::Row, 2);
    assert_eq!(w.close_pane(2, Some("work".into())), Some(Removed::Remains));
    assert_eq!(w.sessions("terminal"), [1], "the pane is out of the window at once");
    assert_eq!(w.undo_close("elsewhere"), Some(("terminal".into(), 2)));
    let win = w.window("terminal").unwrap();
    assert_eq!((win.tabs.len(), win.active_sid()), (2, Some(2)), "the pane returns as the active last tab");
    assert_eq!(w.undo_close("terminal"), None, "nothing left to undo");
    // The undo window ends: the session is the caller's to close, once.
    w.close_pane(2, None);
    assert!(w.expire_closed(2));
    assert!(!w.expire_closed(2));
    assert_eq!(w.undo_close("terminal"), None, "an expired pane cannot come back");
    // The window it came from is gone: it goes to the window that asked.
    let mut w = window();
    w.ensure_window("other", 10);
    w.split("terminal", Dir::Row, 2);
    w.close_pane(2, None);
    w.drop_window("terminal");
    assert_eq!(w.undo_close("other"), Some(("other".into(), 2)));
}
#[test]
fn a_pane_becomes_its_own_tab_only_when_it_has_company() {
    let mut w = window();
    assert!(!w.pane_to_tab("terminal"), "a single pane stays");
    w.split("terminal", Dir::Row, 2);
    assert!(w.pane_to_tab("terminal"));
    let win = w.window("terminal").unwrap();
    assert_eq!(win.tabs.len(), 2);
    assert_eq!((win.active_sid(), win.tabs[0].root.leaves()), (Some(2), vec![1]));
}
#[test]
fn ratios_are_clamped_equalized_and_zoom_needs_two_panes() {
    let mut w = window();
    let split = w.split("terminal", Dir::Row, 2).unwrap();
    let tab = w.window("terminal").unwrap().active.clone();
    w.set_ratio("terminal", &tab, &split, 0.01);
    let ratio = |w: &Workspace| match &w.window("terminal").unwrap().active_tab().unwrap().root {
        Node::Split { ratio, .. } => *ratio,
        _ => panic!("a split"),
    };
    assert_eq!(ratio(&w), MIN_RATIO);
    w.set_ratio("terminal", &tab, &split, 3.);
    assert_eq!(ratio(&w), MAX_RATIO);
    w.equalize("terminal");
    assert_eq!(ratio(&w), 0.5);
    w.detach(2);
    w.toggle_zoom("terminal");
    assert!(!w.window("terminal").unwrap().active_tab().unwrap().zoom, "one pane cannot be zoomed");
}
#[test]
fn font_steps_stay_in_range_and_zero_resets() {
    let mut w = window();
    assert!(w.zoom_font("terminal", 2));
    assert!(w.zoom_font("terminal", 100));
    assert_eq!(w.window("terminal").unwrap().font_delta, 30);
    assert!(!w.zoom_font("terminal", 1), "already at the top: nothing changes");
    assert!(w.zoom_font("terminal", -100));
    assert_eq!(w.window("terminal").unwrap().font_delta, -6);
    assert!(w.zoom_font("terminal", 0));
    assert_eq!(w.window("terminal").unwrap().font_delta, 0);
    w.toggle_timestamps("terminal");
    assert!(w.window("terminal").unwrap().timestamps);
}
#[test]
fn panes_and_dividers_fill_the_area_in_flex_order() {
    let mut w = window();
    let s = w.split("terminal", Dir::Row, 2).unwrap();
    w.split("terminal", Dir::Column, 3);
    let tab = w.window("terminal").unwrap().active_tab().unwrap().clone();
    let (panes, dividers) = Workspace::layout(&tab, area());
    // 1005 wide: (1005 - 5) / 2 = 500 each side of the 5 px divider
    assert_eq!(panes[0], (1, Rect { x: 0., y: 0., w: 500., h: 505. }));
    assert_eq!(dividers[0].split, s);
    assert_eq!(dividers[0].rect, Rect { x: 500., y: 0., w: 5., h: 505. });
    // the right half is split into two stacked panes around a horizontal divider
    assert_eq!(panes[1].1, Rect { x: 505., y: 0., w: 500., h: 250. });
    assert_eq!(panes[2].1, Rect { x: 505., y: 255., w: 500., h: 250. });
    assert_eq!(dividers[1].dir, Dir::Column);
    assert_eq!(dividers[1].parent, Rect { x: 505., y: 0., w: 500., h: 505. });
    // a zoomed pane has the whole area and no dividers
    let mut zoomed = tab.clone();
    zoomed.zoom = true;
    let (panes, dividers) = Workspace::layout(&zoomed, area());
    assert_eq!((panes, dividers.len()), (vec![(3, area())], 0));
    // keyboard navigation between panes
    assert_eq!(Workspace::neighbor(&tab, area(), 1, 1, 0), Some(2), "equally near panes: the first in order");
    assert_eq!(Workspace::neighbor(&tab, area(), 2, 0, 1), Some(3));
    assert_eq!(Workspace::neighbor(&tab, area(), 3, 0, 1), None, "no pane below the last one");
    assert_eq!(Workspace::neighbor(&tab, area(), 3, -1, 0), Some(1));
}
#[test]
fn titles_follow_the_title_the_folder_and_the_running_program() {
    let meta = |title: &str, cwd: &str, process: &str, busy: bool| Meta { title: title.into(), cwd: cwd.into(), process: process.into(), busy };
    assert_eq!(session_title(None), "Terminal");
    assert_eq!(session_title(Some(&meta("vim", "/home/ana/x", "", false))), "vim", "an explicit title wins");
    assert_eq!(session_title(Some(&meta("", "/home/ana/projects/axia", "", false))), "axia");
    assert_eq!(session_title(Some(&meta("", "/home/ana", "", false))), "~");
    assert_eq!(session_title(Some(&meta("", "/", "", false))), "/");
    assert_eq!(session_title(Some(&meta("", "/var/log/", "", false))), "log");
    assert_eq!(session_title(Some(&meta("", "/home/ana/code", "cargo", true))), "code — cargo");
    assert_eq!(session_title(Some(&meta("", "/home/ana/code", "cargo", false))), "code", "an idle shell shows no program");
    let mut w = window();
    let id = w.window("terminal").unwrap().active.clone();
    let tab = w.window("terminal").unwrap().tabs[0].clone();
    let table: HashMap<Sid, Meta> = [(1, meta("", "/home/ana/a", "", false))].into();
    assert_eq!(tab_title(&tab, &table), "a");
    w.set_tab_title("terminal", &id, "deploy");
    assert_eq!(tab_title(&w.window("terminal").unwrap().tabs[0], &table), "deploy");
    w.set_tab_title("terminal", &id, "");
    assert_eq!(tab_title(&w.window("terminal").unwrap().tabs[0], &table), "a", "an empty name returns to the automatic one");
}
#[test]
fn saved_arrangements_rebuild_the_same_tabs_with_new_sessions() {
    let mut w = window();
    w.split("terminal", Dir::Row, 2);
    w.split("terminal", Dir::Column, 3);
    let id = w.add_tab("terminal", 4).unwrap();
    w.set_tab_title("terminal", &id, "logs");
    w.select_tab("terminal", &id);
    let saved = w.saved("terminal", json!({"x":10,"y":20,"w":900,"h":500}), &|sid| json!({"type":"leaf","cwd":format!("/work/{sid}"),"profile":"p"})).unwrap();
    assert_eq!(saved["active"], 1);
    assert_eq!(saved["frame"]["w"], 900);
    let mut next = 100;
    let mut seen = vec![];
    let mut fresh = Workspace::default();
    let tabs = fresh.build_tabs(&saved, &mut |leaf| {
        next += 1;
        seen.push(leaf["cwd"].as_str().unwrap().to_owned());
        Some(next)
    });
    assert_eq!(seen, ["/work/1", "/work/2", "/work/3", "/work/4"], "one session per saved pane, in order");
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0].root.leaves(), [101, 102, 103].map(Sid::from));
    assert_eq!((tabs[1].title.as_deref(), tabs[1].focus), (Some("logs"), 104));
    fresh.adopt_window("terminal#r1", tabs, None);
    assert_eq!(fresh.window("terminal#r1").unwrap().active_sid(), Some(101));
    // a pane that cannot start leaves its sibling; an out-of-range ratio is clamped
    let broken = json!({"tabs":[{"root":{"type":"split","dir":"row","ratio":9,"a":{"type":"leaf"},"b":{"type":"leaf"}}}]});
    let mut n = 0;
    let tabs = fresh.build_tabs(&broken, &mut |_| { n += 1; (n == 2).then_some(500) });
    assert_eq!(tabs[0].root, Node::Leaf(500));
    let both = fresh.build_tabs(&broken, &mut |_| Some(7));
    assert!(matches!(both[0].root, Node::Split { ratio, .. } if ratio == MAX_RATIO));
    assert!(fresh.build_tabs(&json!({"tabs":[{"root":{"type":"leaf"}}]}), &mut |_| None).is_empty(), "no session, no tab");
}
#[test]
fn surviving_sessions_return_to_their_places_once_each() {
    let live = json!({"type":"split","dir":"row","ratio":0.4,
        "a":{"type":"leaf","key":"k1"},
        "b":{"type":"split","dir":"column","ratio":0.5,"a":{"type":"leaf","key":"gone"},"b":{"type":"leaf","key":"k2"}}});
    let by_key: HashMap<String, Sid> = [("k1".to_owned(), 11), ("k2".to_owned(), 12)].into();
    let mut used = HashSet::new();
    let tree = reattach_ids(&live, &by_key, &mut used).unwrap();
    assert_eq!(tree["a"]["sid"], 11);
    assert_eq!(tree["b"]["sid"], 12, "the split that lost a side becomes the side that remains");
    assert_eq!(used, [11, 12].into_iter().collect::<HashSet<_>>());
    assert!(reattach_ids(&live, &by_key, &mut used).is_none(), "a session is placed once");
    assert!(reattach_ids(&json!({"type":"leaf","key":""}), &by_key, &mut HashSet::new()).is_none());
    let mut w = Workspace::default();
    let node = w.build_tree(&tree, &mut |leaf| leaf["sid"].as_i64()).unwrap();
    assert_eq!(node.leaves(), [11, 12]);
}
#[test]
fn finding_and_dropping_windows_report_their_sessions() {
    let mut w = window();
    w.ensure_window("second", 20);
    w.split("second", Dir::Row, 21);
    assert_eq!(w.find_session(21).map(|(wid, _)| wid), Some("second"));
    assert_eq!(w.find_session(404), None);
    assert_eq!(w.focus_pane(20), Some("second".into()));
    assert_eq!(w.window("second").unwrap().active_sid(), Some(20));
    assert_eq!(w.focus_pane(404), None);
    assert_eq!(w.drop_window("second"), vec![20, 21], "the sessions of a closed window are returned to be closed");
    assert!(w.window("second").is_none());
    assert_eq!(w.windows().count(), 1);
}
