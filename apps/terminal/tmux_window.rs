//! tmux in control mode inside the native window: the session where `tmux -CC` runs is the
//! gateway, each tmux window becomes a tab of a Terminal window of its own and each tmux pane a
//! pane whose session the server makes. The text and the trees are `tmux.rs`; this is what the
//! window does with them: commands to tmux, sessions for panes, tabs for windows, the size.
use super::*;
use super::tmux::{self, Item, Reply};
use crate::workspace::Tab;
use std::collections::HashSet;

/// A window resize waits this long for the next one before telling tmux the new size.
const RESIZE_MS: i64 = 150;

/// One `tmux -CC` connection: what tmux told us and what we built from it.
pub(super) struct Gateway {
    proto: tmux::Protocol,
    /// tmux pane number → its session, and the tmux window each pane is in.
    panes: HashMap<i64, Sid>,
    pane_win: HashMap<i64, String>,
    /// tmux window id → the tab that shows it.
    wins: HashMap<String, String>,
    /// The Terminal window that shows the tmux windows, once it exists.
    wid: Option<String>,
    /// Tabs built while there is no window to hold them, and whether one was asked for.
    fresh: Vec<Tab>,
    opening: bool,
    /// Panes whose session is on its way, and the layouts that wait for them.
    asked: HashSet<i64>,
    waiting: HashMap<String, (String, Option<String>)>,
    /// The size tmux was last told, and a resize in the making.
    size: String,
    resize_due: i64,
    resize_sid: Sid,
    resize_busy: bool,
}
impl Gateway {
    fn new(resumed: bool) -> Self {
        Self {
            proto: tmux::Protocol::new(resumed),
            panes: HashMap::new(),
            pane_win: HashMap::new(),
            wins: HashMap::new(),
            wid: None,
            fresh: vec![],
            opening: false,
            asked: HashSet::new(),
            waiting: HashMap::new(),
            size: String::new(),
            resize_due: 0,
            resize_sid: 0,
            resize_busy: false,
        }
    }
}

impl App {
    /// The events of a gateway and of the sessions of its panes; false for any other.
    pub(super) fn tmux_event(&mut self, sid: Sid, kind: &str, data: &Value, now: i64) -> bool {
        match kind {
            "tmux-start" => self.tmux_start(sid, false),
            "tmux" => self.tmux_lines(sid, data.as_str().unwrap_or("")),
            "tmux-end" => self.tmux_teardown(sid, ""),
            "resize" => {
                if let Some(gw) = shell(|s| s.sessions.get(&sid).and_then(|x| x.gateway)) {
                    shell(|s| {
                        if let Some(g) = s.gateways.get_mut(&gw) {
                            (g.resize_due, g.resize_sid) = (now + RESIZE_MS, sid);
                        }
                    });
                }
            }
            _ => return false,
        }
        true
    }
    pub(super) fn tmux_start(&mut self, gw: Sid, resumed: bool) {
        let fresh = shell(|s| {
            let ok = s.sessions.contains_key(&gw) && !s.gateways.contains_key(&gw);
            if ok {
                s.gateways.insert(gw, Gateway::new(resumed));
                if let Some(x) = s.sessions.get_mut(&gw) {
                    x.tmux = true;
                }
            }
            ok
        });
        if fresh {
            self.tmux_list(gw);
        }
    }
    fn tmux_list(&mut self, gw: Sid) {
        self.tmux_send(gw, "list-windows -F \"#{window_id}\t#{window_layout}\t#{window_name}\"", Reply::Windows);
    }
    /// A command for tmux; its answer is read as `reply` says.
    fn tmux_send(&mut self, gw: Sid, cmd: &str, reply: Reply) {
        if shell(|s| s.gateways.get_mut(&gw).map(|g| g.proto.expect(reply)).is_some()) {
            self.effect("tmuxCommand", gw, json!({"cmd":cmd}));
        }
    }
    /// A command that is not answered in a block (detaching ends the connection instead).
    fn tmux_raw(&mut self, gw: Sid, cmd: &str) {
        self.effect("tmuxCommand", gw, json!({"cmd":cmd}));
    }
    fn tmux_lines(&mut self, gw: Sid, text: &str) {
        let items = shell(|s| s.gateways.get_mut(&gw).map(|g| g.proto.feed(text))).unwrap_or_default();
        for item in items {
            if !shell(|s| s.gateways.contains_key(&gw)) {
                break;
            }
            self.tmux_item(gw, item);
        }
    }
    fn tmux_item(&mut self, gw: Sid, item: Item) {
        match item {
            Item::Answer(Reply::Ignore, _) => {}
            Item::Answer(Reply::Windows, lines) => self.tmux_windows(gw, &lines),
            Item::Answer(Reply::Capture { pane }, lines) => {
                let ask = format!("display-message -p -t %{pane} \"#{{cursor_x}} #{{cursor_y}} #{{pane_height}}\"");
                self.tmux_send(gw, &ask, Reply::Cursor { pane, lines });
            }
            Item::Answer(Reply::Cursor { pane, lines }, answer) => self.tmux_screen(gw, pane, &lines, answer.first().map_or("", String::as_str)),
            Item::Failed(text) => self.toast_for(gw, json!(format!("tmux: {text}")), clock::now()),
            Item::Layout { window, layout } => self.tmux_layout(gw, &window, &layout, None),
            Item::ListWindows => self.tmux_list(gw),
            Item::WindowClosed(id) => self.tmux_window_closed(gw, &id),
            Item::WindowRenamed { window, name } => {
                shell(|s| {
                    if let Some((wid, tab)) = s.gateways.get(&gw).and_then(|g| Some((g.wid.clone()?, g.wins.get(&window)?.clone()))) {
                        s.ws.set_tab_title(&wid, &tab, &name);
                    }
                });
            }
            Item::PaneChanged { window, pane } => {
                shell(|s| {
                    let Some(g) = s.gateways.get(&gw) else { return };
                    if let (Some(wid), Some(tab), Some(sid)) = (g.wid.clone(), g.wins.get(&window).cloned(), g.panes.get(&pane).copied()) {
                        s.ws.set_tab_focus(&wid, &tab, sid);
                    }
                });
            }
            Item::Exit(reason) => self.tmux_teardown(gw, &reason),
        }
    }
    /// What `list-windows` said: every window is laid out, the ones tmux no longer has go.
    fn tmux_windows(&mut self, gw: Sid, lines: &[String]) {
        let mut seen: HashSet<String> = HashSet::new();
        for line in lines {
            let mut fields = line.split('\t');
            let (Some(id), Some(layout)) = (fields.next().filter(|f| !f.is_empty()), fields.next().filter(|f| !f.is_empty())) else { continue };
            seen.insert(id.to_owned());
            self.tmux_layout(gw, id, layout, fields.next().map(str::to_owned));
        }
        let stale: Vec<String> = shell(|s| s.gateways.get(&gw).map(|g| g.wins.keys().filter(|id| !seen.contains(*id)).cloned().collect())).unwrap_or_default();
        for id in stale {
            self.tmux_window_closed(gw, &id);
        }
        self.tmux_open_fresh(gw);
    }
    /// A tmux window has this layout: panes without a session ask for one and the layout waits
    /// for them; otherwise the tab is built (or its tree replaced).
    fn tmux_layout(&mut self, gw: Sid, id: &str, text: &str, name: Option<String>) {
        let Some(layout) = tmux::parse_layout(text) else { return };
        let live = layout.panes();
        let gone: Vec<(i64, Sid)> = shell(|s| {
            s.gateways
                .get(&gw)
                .map(|g| g.panes.iter().filter(|(n, _)| g.pane_win.get(*n).is_some_and(|w| w == id) && !live.contains(*n)).map(|(n, sid)| (*n, *sid)).collect())
                .unwrap_or_default()
        });
        for (_, sid) in gone {
            self.remove_session(sid);
        }
        let missing: Vec<i64> = shell(|s| {
            let Some(g) = s.gateways.get_mut(&gw) else { return vec![] };
            for n in &live {
                g.pane_win.insert(*n, id.to_owned());
            }
            live.iter().filter(|n| !g.panes.contains_key(*n)).copied().collect()
        });
        if !missing.is_empty() {
            let ask: Vec<i64> = shell(|s| {
                let Some(g) = s.gateways.get_mut(&gw) else { return vec![] };
                g.waiting.insert(id.to_owned(), (text.to_owned(), name));
                missing.iter().filter(|n| g.asked.insert(**n)).copied().collect()
            });
            for pane in ask {
                self.jobs.enqueue(Tag::TmuxPane { gw, pane }, "term.tmuxPane", json!([gw, pane]));
            }
            return;
        }
        let built = shell(|s| {
            let panes = s.gateways.get(&gw)?.panes.clone();
            let root = tmux::to_tree(&layout, &|n| panes.get(&n).copied(), &mut || s.ws.new_split_id())?;
            let g = s.gateways.get_mut(&gw)?;
            g.waiting.remove(id);
            let title = name.clone();
            let known = g.wid.clone().zip(g.wins.get(id).cloned()).filter(|(wid, _)| s.ws.window(wid).is_some());
            match known {
                Some((wid, tab)) => {
                    s.ws.set_layout(&wid, &tab, root, title);
                }
                None => {
                    let tab = s.ws.tab_from(root, name.filter(|n| !n.is_empty()));
                    let g = s.gateways.get_mut(&gw)?;
                    g.wins.insert(id.to_owned(), tab.id.clone());
                    match g.wid.clone().filter(|wid| s.ws.window(wid).is_some()) {
                        Some(wid) => {
                            s.ws.push_tab(&wid, tab);
                        }
                        None => g.fresh.push(tab),
                    }
                }
            }
            Some(())
        });
        if built.is_some() {
            self.tmux_open_fresh(gw);
        }
    }
    /// Tabs with no window yet: a Terminal window is opened for them (it takes them when it starts).
    fn tmux_open_fresh(&mut self, gw: Sid) {
        let open = shell(|s| {
            s.gateways.get_mut(&gw).is_some_and(|g| {
                let ask = !g.fresh.is_empty() && !g.opening;
                g.opening |= ask;
                ask
            })
        });
        if open {
            self.jobs.enqueue(Tag::Quiet, "windows.open", json!(["terminal", {"new":true,"tmuxGateway":gw}]));
        }
    }
    /// A window just started with `tmuxGateway`: it takes the tabs built for it.
    pub(super) fn tmux_adopt(&mut self, gw: Sid) -> bool {
        let window = self.window.clone();
        shell(|s| {
            let Some(g) = s.gateways.get_mut(&gw) else { return false };
            if g.fresh.is_empty() {
                return false;
            }
            let tabs = std::mem::take(&mut g.fresh);
            g.wid = Some(window.clone());
            g.opening = false;
            s.ws.adopt_window(&window, tabs, None);
            true
        })
    }
    fn tmux_window_closed(&mut self, gw: Sid, id: &str) {
        let gone: Vec<Sid> = shell(|s| {
            let Some(g) = s.gateways.get_mut(&gw) else { return vec![] };
            g.wins.remove(id);
            g.waiting.remove(id);
            let numbers: Vec<i64> = g.pane_win.iter().filter(|(_, w)| *w == id).map(|(n, _)| *n).collect();
            numbers
                .into_iter()
                .filter_map(|n| {
                    g.pane_win.remove(&n);
                    g.panes.remove(&n)
                })
                .collect()
        });
        for sid in gone {
            self.remove_session(sid);
        }
    }
    /// The session of a pane arrived: it is set up like its gateway, and the contents tmux has
    /// for the pane are asked for.
    pub(super) fn tmux_pane_ready(&mut self, gw: Sid, pane: i64, sid: Option<Sid>) {
        let alive = shell(|s| s.gateways.get_mut(&gw).map(|g| g.asked.remove(&pane)).is_some());
        let Some(sid) = sid else { return };
        if !alive {
            // the connection ended while the session was being made
            self.jobs.enqueue(Tag::Quiet, "term.close", json!([sid]));
            return;
        }
        shell(|s| {
            let profile = s.sessions.get(&gw).map(|x| x.profile.clone()).unwrap_or_default();
            s.sessions.insert(sid, Session { profile, gateway: Some(gw), tmux_pane: pane, ..Default::default() });
            if let Some(g) = s.gateways.get_mut(&gw) {
                g.panes.insert(pane, sid);
            }
        });
        self.configure(sid);
        self.tmux_send(gw, &format!("capture-pane -p -e -J -t %{pane} -S -2000"), Reply::Capture { pane });
        let waiting: Vec<(String, (String, Option<String>))> = shell(|s| s.gateways.get(&gw).map(|g| g.waiting.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default());
        for (id, (layout, name)) in waiting {
            self.tmux_layout(gw, &id, &layout, name);
        }
    }
    /// What the pane showed (history and colors) and where the cursor was: written to the new session.
    fn tmux_screen(&mut self, gw: Sid, pane: i64, lines: &[String], cursor: &str) {
        let Some(sid) = shell(|s| s.gateways.get(&gw).and_then(|g| g.panes.get(&pane).copied())) else { return };
        let rows = shell(|s| s.sessions.get(&sid).and_then(|x| x.info["rows"].as_i64())).filter(|r| *r > 0).unwrap_or(24);
        self.effect("feed", sid, json!({"text":format!("\x1b[H\x1b[2J{}", lines.join("\r\n"))}));
        let numbers: Vec<Option<i64>> = cursor.split(' ').map(|t| t.parse().ok()).collect();
        if let [Some(x), Some(y), Some(height)] = numbers[..] {
            // the cursor row counts from the bottom: the pane here can be another height
            let last = (lines.len() as i64).min(rows) - 1;
            let row = (last - (height - 1) + y).clamp(0, rows - 1);
            self.effect("feed", sid, json!({"text":format!("\x1b[{};{}H", row + 1, x + 1)}));
        }
        self.effect("tmuxReady", sid, json!({}));
    }
    /// The connection ended (or was ended): the pane sessions go and the gateway is a shell again.
    pub(super) fn tmux_teardown(&mut self, gw: Sid, reason: &str) {
        let Some(g) = shell(|s| s.gateways.remove(&gw)) else { return };
        shell(|s| {
            if let Some(x) = s.sessions.get_mut(&gw) {
                x.tmux = false;
            }
        });
        for sid in g.panes.values() {
            self.remove_session(*sid);
        }
        if !reason.is_empty() {
            self.toast_for(gw, json!({"key":"terminal.tmux.detached","params":{"reason":reason}}), clock::now());
        }
    }
    /// A session left (its shell ended or its pane was closed): no connection keeps it in its maps.
    pub(super) fn tmux_forget(&mut self, sid: Sid) {
        if shell(|s| s.gateways.contains_key(&sid)) {
            self.tmux_teardown(sid, "");
        }
        shell(|s| {
            for g in s.gateways.values_mut() {
                let numbers: Vec<i64> = g.panes.iter().filter(|(_, v)| **v == sid).map(|(n, _)| *n).collect();
                for n in numbers {
                    g.panes.remove(&n);
                    g.pane_win.remove(&n);
                }
            }
        });
    }
    /// Esc on the gateway lets go of tmux, which keeps running.
    pub(super) fn tmux_detach(&mut self, gw: Sid) {
        self.tmux_raw(gw, "detach-client");
    }
    /// The Terminal window that shows tmux is closing: tmux is let go.
    pub(super) fn tmux_window_closing(&mut self) {
        let gateways: Vec<Sid> = shell(|s| s.gateways.iter().filter(|(_, g)| g.wid.as_deref() == Some(self.window.as_str())).map(|(gw, _)| *gw).collect());
        for gw in gateways {
            self.tmux_detach(gw);
        }
    }
    /// The window's actions on a tmux pane are tmux commands; true when tmux took the action.
    pub(super) fn tmux_action(&mut self, action: &str, sid: Sid) -> bool {
        let Some((gw, pane)) = shell(|s| s.sessions.get(&sid).and_then(|x| x.gateway.map(|gw| (gw, x.tmux_pane)))) else { return false };
        if !shell(|s| s.gateways.contains_key(&gw)) {
            return false;
        }
        let target = format!("%{pane}");
        let command = match action {
            "newTab" => "new-window".to_owned(),
            "splitRight" => format!("split-window -h -t {target}"),
            "splitDown" => format!("split-window -v -t {target}"),
            "closePane" => format!("kill-pane -t {target}"),
            "closeTab" => {
                let window = shell(|s| {
                    let (_, tab) = s.ws.find_session(sid)?;
                    s.gateways.get(&gw)?.wins.iter().find(|(_, t)| t.as_str() == tab).map(|(w, _)| w.clone())
                });
                match window {
                    Some(w) => format!("kill-window -t {w}"),
                    None => return true,
                }
            }
            "equalize" => format!("select-layout -t {target} tiled"),
            "maximizePane" => format!("resize-pane -Z -t {target}"),
            _ => return false,
        };
        self.tmux_send(gw, &command, Reply::Ignore);
        true
    }
    /// Clicking a pane selects it in tmux too.
    pub(super) fn tmux_focus(&mut self, sid: Sid) {
        if let Some((gw, pane)) = shell(|s| s.sessions.get(&sid).and_then(|x| x.gateway.map(|gw| (gw, x.tmux_pane)))) {
            self.tmux_send(gw, &format!("select-pane -t %{pane}"), Reply::Ignore);
        }
    }
    /// Sessions that were still in control mode when the window started (the server restarted): the
    /// connection goes on, and no answer is owed to a command that was not sent.
    pub(super) fn tmux_recover(&mut self, infos: &[(Sid, bool)]) {
        for (sid, tmux) in infos {
            if *tmux && shell(|s| s.sessions.get(sid).is_some_and(|x| x.gateway.is_none())) {
                self.tmux_start(*sid, true);
            }
        }
    }
    /// A resize that stopped changing: ask for the sizes of the tab's panes and tell tmux.
    pub(super) fn tmux_timers(&mut self, now: i64) {
        let due: Vec<(Sid, Sid)> = shell(|s| {
            s.gateways
                .iter_mut()
                .filter(|(_, g)| g.resize_due > 0 && now >= g.resize_due && !g.resize_busy)
                .map(|(gw, g)| {
                    g.resize_due = 0;
                    g.resize_busy = true;
                    (*gw, g.resize_sid)
                })
                .collect()
        });
        for (gw, sid) in due {
            let leaves = shell(|s| {
                let (wid, tab) = s.ws.find_session(sid)?;
                s.ws.window(wid)?.tabs.iter().find(|t| t.id == tab).map(|t| t.root.leaves())
            });
            match leaves {
                Some(leaves) => self.jobs.enqueue(Tag::TmuxSize { gw, sid }, "term.infos", json!([leaves])),
                None => shell(|s| {
                    if let Some(g) = s.gateways.get_mut(&gw) {
                        g.resize_busy = false;
                    }
                }),
            }
        }
    }
    pub(super) fn tmux_resized(&mut self, gw: Sid, sid: Sid, infos: &Value) {
        shell(|s| {
            if let Some(g) = s.gateways.get_mut(&gw) {
                g.resize_busy = false;
            }
        });
        let size: HashMap<Sid, (i64, i64)> = infos
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| Some((e["sid"].as_i64()?, (e["info"]["cols"].as_i64()?, e["info"]["rows"].as_i64()?))))
            .collect();
        let wanted = shell(|s| {
            let (wid, tab) = s.ws.find_session(sid)?;
            let root = s.ws.window(wid)?.tabs.iter().find(|t| t.id == tab)?.root.clone();
            let (w, h) = tmux::window_size(&root, &|leaf| size.get(&leaf).copied().unwrap_or((80, 24)));
            Some(format!("{w}x{h}"))
        });
        let Some(wanted) = wanted else { return };
        let changed = shell(|s| s.gateways.get_mut(&gw).is_some_and(|g| g.size != wanted && { g.size = wanted.clone(); true }));
        if changed {
            self.tmux_send(gw, &format!("refresh-client -C {wanted}"), Reply::Ignore);
        }
    }
}

#[cfg(test)]
impl App {
    /// Tests drive time by hand: a resize in the making is due now.
    pub(super) fn tmux_resize_now(&mut self, gw: Sid) {
        shell(|s| {
            if let Some(g) = s.gateways.get_mut(&gw) {
                g.resize_due = 1;
            }
        });
    }
}
