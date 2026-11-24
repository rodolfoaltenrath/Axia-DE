//! Terminal windows → tabs → pane trees → sessions, with no I/O. The decisions that
//! `state.js` made (which tab is active after a close, where a split goes, how a pane is
//! undone, what the saved arrangements hold) live here; the transport creates and closes the
//! real sessions and tells this model their ids. Session ids are opaque numbers.
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

pub type Sid = i64;
pub const DIVIDER: f64 = 5.;
pub const MIN_RATIO: f64 = 0.1;
pub const MAX_RATIO: f64 = 0.9;
const FONT_MIN: i32 = -6;
const FONT_MAX: i32 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Row,
    Column,
}
impl Dir {
    pub fn name(self) -> &'static str {
        match self {
            Dir::Row => "row",
            Dir::Column => "column",
        }
    }
    fn parse(v: &Value) -> Dir {
        if v == "column" { Dir::Column } else { Dir::Row }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Leaf(Sid),
    Split { id: String, dir: Dir, ratio: f64, a: Box<Node>, b: Box<Node> },
}
impl Node {
    pub fn leaves(&self) -> Vec<Sid> {
        match self {
            Node::Leaf(sid) => vec![*sid],
            Node::Split { a, b, .. } => a.leaves().into_iter().chain(b.leaves()).collect(),
        }
    }
    fn contains(&self, sid: Sid) -> bool {
        match self {
            Node::Leaf(s) => *s == sid,
            Node::Split { a, b, .. } => a.contains(sid) || b.contains(sid),
        }
    }
    fn replace_leaf(self, sid: Sid, with: &mut impl FnMut(Node) -> Node) -> Node {
        match self {
            Node::Leaf(s) if s == sid => with(Node::Leaf(s)),
            leaf @ Node::Leaf(_) => leaf,
            Node::Split { id, dir, ratio, a, b } => Node::Split {
                id,
                dir,
                ratio,
                a: Box::new(a.replace_leaf(sid, with)),
                b: Box::new(b.replace_leaf(sid, with)),
            },
        }
    }
    fn remove_leaf(self, sid: Sid) -> Option<Node> {
        match self {
            Node::Leaf(s) => (s != sid).then_some(Node::Leaf(s)),
            Node::Split { id, dir, ratio, a, b } => match (a.remove_leaf(sid), b.remove_leaf(sid)) {
                (None, rest) | (rest, None) => rest,
                (Some(a), Some(b)) => Some(Node::Split { id, dir, ratio, a: Box::new(a), b: Box::new(b) }),
            },
        }
    }
    fn set_ratio(&mut self, split: &str, value: f64) {
        if let Node::Split { id, ratio, a, b, .. } = self {
            if id == split {
                *ratio = value;
            } else {
                a.set_ratio(split, value);
                b.set_ratio(split, value);
            }
        }
    }
    fn even(&mut self) {
        if let Node::Split { ratio, a, b, .. } = self {
            *ratio = 0.5;
            a.even();
            b.even();
        }
    }
    pub fn json(&self) -> Value {
        match self {
            Node::Leaf(sid) => json!({"type":"leaf","sid":sid}),
            Node::Split { id, dir, ratio, a, b } => {
                json!({"type":"split","id":id,"dir":dir.name(),"ratio":ratio,"a":a.json(),"b":b.json()})
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: String,
    pub root: Node,
    pub focus: Sid,
    pub title: Option<String>,
    pub zoom: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub tabs: Vec<Tab>,
    pub active: String,
    pub font_delta: i32,
    pub timestamps: bool,
}
impl Window {
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == self.active).or_else(|| self.tabs.first())
    }
    pub fn active_sid(&self) -> Option<Sid> {
        self.active_tab().map(|t| t.focus)
    }
    fn tab_mut(&mut self, id: &str) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }
}
/// A pane taken out for the undo window (closed, session still alive).
#[derive(Clone, Debug, PartialEq)]
pub struct Closed {
    pub sid: Sid,
    pub wid: String,
    pub profile: Option<String>,
}
/// What removing a session left behind.
#[derive(Clone, Debug, PartialEq)]
pub enum Removed {
    /// The session was in no window.
    Unknown,
    /// Other panes or tabs remain.
    Remains,
    /// That was the last tab of the window: the window closes.
    WindowEmpty(String),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct DividerRect {
    pub split: String,
    pub dir: Dir,
    pub rect: Rect,
    /// The split's own rectangle, to turn a pointer position into a ratio.
    pub parent: Rect,
}
#[derive(Default)]
pub struct Workspace {
    windows: BTreeMap<String, Window>,
    seq: u64,
    closed: Vec<Closed>,
}

fn basename(path: &str) -> String {
    if path.is_empty() || path == "/" {
        return path.to_owned();
    }
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_owned()
}
/// What the title of one session is made of.
#[derive(Default, Clone, Debug)]
pub struct Meta {
    pub title: String,
    pub cwd: String,
    pub process: String,
    pub busy: bool,
}
/// `/home/<user>/x` is shown as `~/x`; a session with nothing to show is "Terminal".
pub fn session_title(meta: Option<&Meta>) -> String {
    let default = Meta::default();
    let m = meta.unwrap_or(&default);
    let base = if !m.title.is_empty() {
        m.title.clone()
    } else if !m.cwd.is_empty() {
        let shown = match m.cwd.strip_prefix("/home/") {
            Some(rest) => format!("~{}", rest.find('/').map_or("", |i| &rest[i..])),
            None => m.cwd.clone(),
        };
        let name = basename(&shown);
        if name.is_empty() { "~".to_owned() } else { name }
    } else {
        "Terminal".to_owned()
    };
    if m.busy && !m.process.is_empty() { format!("{base} — {}", m.process) } else { base }
}
pub fn tab_title(tab: &Tab, meta: &HashMap<Sid, Meta>) -> String {
    tab.title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| session_title(meta.get(&tab.focus)))
}

impl Workspace {
    fn next_id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        format!("{prefix}{}", self.seq)
    }
    pub fn window(&self, wid: &str) -> Option<&Window> {
        self.windows.get(wid)
    }
    pub fn windows(&self) -> impl Iterator<Item = (&String, &Window)> {
        self.windows.iter()
    }
    fn new_tab(&mut self, sid: Sid) -> Tab {
        Tab { id: self.next_id("t"), root: Node::Leaf(sid), focus: sid, title: None, zoom: false }
    }
    /// A window with one tab on `sid` (a no-op when the window already exists).
    pub fn ensure_window(&mut self, wid: &str, sid: Sid) {
        if self.windows.contains_key(wid) {
            return;
        }
        let tab = self.new_tab(sid);
        self.windows.insert(
            wid.to_owned(),
            Window { active: tab.id.clone(), tabs: vec![tab], font_delta: 0, timestamps: false },
        );
    }
    /// A window made of tabs that already exist (restored or recovered sessions).
    pub fn adopt_window(&mut self, wid: &str, tabs: Vec<Tab>, active: Option<&str>) {
        let Some(first) = tabs.first() else { return };
        let active = active.filter(|a| tabs.iter().any(|t| t.id == *a)).unwrap_or(&first.id).to_owned();
        self.windows.insert(wid.to_owned(), Window { tabs, active, font_delta: 0, timestamps: false });
    }
    pub fn tab_from(&mut self, root: Node, title: Option<String>) -> Tab {
        let focus = root.leaves().first().copied().unwrap_or(0);
        Tab { id: self.next_id("t"), root, focus, title, zoom: false }
    }
    pub fn find_session(&self, sid: Sid) -> Option<(&str, &str)> {
        self.windows
            .iter()
            .find_map(|(wid, w)| w.tabs.iter().find(|t| t.root.contains(sid)).map(|t| (wid.as_str(), t.id.as_str())))
    }
    /// Every session of a window, in tab and pane order.
    pub fn sessions(&self, wid: &str) -> Vec<Sid> {
        self.windows.get(wid).map(|w| w.tabs.iter().flat_map(|t| t.root.leaves()).collect()).unwrap_or_default()
    }
    /// A window gone from the window manager: its sessions are the caller's to close.
    pub fn drop_window(&mut self, wid: &str) -> Vec<Sid> {
        let sids = self.sessions(wid);
        self.windows.remove(wid);
        sids
    }
    pub fn add_tab(&mut self, wid: &str, sid: Sid) -> Option<String> {
        let tab = self.new_tab(sid);
        let id = tab.id.clone();
        let w = self.windows.get_mut(wid)?;
        let at = w.tabs.iter().position(|t| t.id == w.active).map_or(w.tabs.len(), |i| i + 1);
        w.tabs.insert(at, tab);
        w.active = id.clone();
        Some(id)
    }
    /// The session the active tab shows, to start a new one next to it.
    pub fn current_sid(&self, wid: &str) -> Option<Sid> {
        self.windows.get(wid)?.active_sid()
    }
    pub fn select_tab(&mut self, wid: &str, tab: &str) -> Option<Sid> {
        let w = self.windows.get_mut(wid)?;
        let focus = w.tabs.iter().find(|t| t.id == tab)?.focus;
        w.active = tab.to_owned();
        Some(focus)
    }
    pub fn cycle_tab(&mut self, wid: &str, delta: i64) -> Option<Sid> {
        let w = self.windows.get(wid)?;
        let n = w.tabs.len() as i64;
        let at = w.tabs.iter().position(|t| t.id == w.active)? as i64;
        let id = w.tabs[(at + delta).rem_euclid(n) as usize].id.clone();
        self.select_tab(wid, &id)
    }
    pub fn move_tab(&mut self, wid: &str, tab: &str, delta: i64) {
        let Some(w) = self.windows.get_mut(wid) else { return };
        let Some(i) = w.tabs.iter().position(|t| t.id == tab) else { return };
        let j = i as i64 + delta;
        if j < 0 || j as usize >= w.tabs.len() {
            return;
        }
        w.tabs.swap(i, j as usize);
    }
    /// A pane takes focus (a click): its tab becomes the active one of its window.
    pub fn focus_pane(&mut self, sid: Sid) -> Option<String> {
        let (wid, tab) = self.find_session(sid).map(|(w, t)| (w.to_owned(), t.to_owned()))?;
        let w = self.windows.get_mut(&wid)?;
        w.active = tab.clone();
        w.tab_mut(&tab)?.focus = sid;
        Some(wid)
    }
    /// Splits the focused pane of the active tab; the new session takes the focus.
    pub fn split(&mut self, wid: &str, dir: Dir, sid: Sid) -> Option<String> {
        let id = self.next_id("s");
        let w = self.windows.get_mut(wid)?;
        let active = w.active.clone();
        let tab = w.tab_mut(&active)?;
        let current = tab.focus;
        let root = std::mem::replace(&mut tab.root, Node::Leaf(0));
        let split_id = id.clone();
        tab.root = root.replace_leaf(current, &mut |leaf| Node::Split {
            id: split_id.clone(),
            dir,
            ratio: 0.5,
            a: Box::new(leaf),
            b: Box::new(Node::Leaf(sid)),
        });
        tab.focus = sid;
        tab.zoom = false;
        Some(id)
    }
    pub fn set_ratio(&mut self, wid: &str, tab: &str, split: &str, ratio: f64) {
        if let Some(t) = self.windows.get_mut(wid).and_then(|w| w.tab_mut(tab)) {
            t.root.set_ratio(split, ratio.clamp(MIN_RATIO, MAX_RATIO));
        }
    }
    /// Takes a session out of its tab (and the tab, then the window, when they empty).
    /// The session itself stays open: the caller decides whether it closes.
    pub fn detach(&mut self, sid: Sid) -> Removed {
        let Some((wid, tab_id)) = self.find_session(sid).map(|(w, t)| (w.to_owned(), t.to_owned())) else {
            return Removed::Unknown;
        };
        let w = self.windows.get_mut(&wid).unwrap();
        let at = w.tabs.iter().position(|t| t.id == tab_id).unwrap();
        let root = std::mem::replace(&mut w.tabs[at].root, Node::Leaf(0));
        match root.remove_leaf(sid) {
            None => {
                w.tabs.remove(at);
                if w.tabs.is_empty() {
                    return Removed::WindowEmpty(wid);
                }
                if w.active == tab_id {
                    w.active = w.tabs[at.saturating_sub(1).min(w.tabs.len() - 1)].id.clone();
                }
            }
            Some(root) => {
                let remaining = root.leaves();
                let tab = &mut w.tabs[at];
                if !remaining.contains(&tab.focus) {
                    tab.focus = *remaining.last().unwrap();
                }
                tab.root = root;
                tab.zoom = false;
            }
        }
        Removed::Remains
    }
    /// Closes a pane but keeps its session for the undo window.
    pub fn close_pane(&mut self, sid: Sid, profile: Option<String>) -> Option<Removed> {
        let wid = self.find_session(sid)?.0.to_owned();
        self.closed.push(Closed { sid, wid, profile });
        Some(self.detach(sid))
    }
    /// The undo window of a closed pane ended: true when its session is still waiting.
    pub fn expire_closed(&mut self, sid: Sid) -> bool {
        match self.closed.iter().position(|c| c.sid == sid) {
            Some(i) => {
                self.closed.remove(i);
                true
            }
            None => false,
        }
    }
    /// Brings the last closed pane back as a new tab of its window (or of `fallback`).
    pub fn undo_close(&mut self, fallback: &str) -> Option<(String, Sid)> {
        let item = self.closed.pop()?;
        let target = if self.windows.contains_key(&item.wid) { item.wid } else { fallback.to_owned() };
        let tab = self.new_tab(item.sid);
        let w = self.windows.get_mut(&target)?;
        w.active = tab.id.clone();
        w.tabs.push(tab);
        Some((target, item.sid))
    }
    pub fn pane_to_tab(&mut self, wid: &str) -> bool {
        let Some(tab) = self.windows.get(wid).and_then(Window::active_tab) else { return false };
        if tab.root.leaves().len() < 2 {
            return false;
        }
        let sid = tab.focus;
        self.detach(sid);
        let new = self.new_tab(sid);
        if let Some(w) = self.windows.get_mut(wid) {
            w.active = new.id.clone();
            w.tabs.push(new);
        }
        true
    }
    pub fn equalize(&mut self, wid: &str) {
        if let Some(w) = self.windows.get_mut(wid) {
            let active = w.active.clone();
            if let Some(t) = w.tab_mut(&active) {
                t.root.even();
            }
        }
    }
    pub fn toggle_zoom(&mut self, wid: &str) {
        if let Some(w) = self.windows.get_mut(wid) {
            let active = w.active.clone();
            if let Some(t) = w.tab_mut(&active).filter(|t| t.root.leaves().len() > 1) {
                t.zoom = !t.zoom;
            }
        }
    }
    pub fn set_tab_title(&mut self, wid: &str, tab: &str, title: &str) {
        if let Some(t) = self.windows.get_mut(wid).and_then(|w| w.tab_mut(tab)) {
            t.title = (!title.is_empty()).then(|| title.to_owned());
        }
    }
    /// The window font size changes by `delta` steps (0 resets); true when it changed.
    pub fn zoom_font(&mut self, wid: &str, delta: i32) -> bool {
        let Some(w) = self.windows.get_mut(wid) else { return false };
        let next = if delta == 0 { 0 } else { (w.font_delta + delta).clamp(FONT_MIN, FONT_MAX) };
        let changed = next != w.font_delta;
        w.font_delta = next;
        changed
    }
    pub fn toggle_timestamps(&mut self, wid: &str) {
        if let Some(w) = self.windows.get_mut(wid) {
            w.timestamps = !w.timestamps;
        }
    }

    // ---- geometry ----

    /// Pane and divider rectangles of a tab inside `area`. Panes run left to right and top to
    /// bottom whatever the interface direction; a zoomed pane fills the area alone.
    pub fn layout(tab: &Tab, area: Rect) -> (Vec<(Sid, Rect)>, Vec<DividerRect>) {
        let (mut panes, mut dividers) = (vec![], vec![]);
        if tab.zoom {
            return (vec![(tab.focus, area)], dividers);
        }
        fn walk(n: &Node, r: Rect, panes: &mut Vec<(Sid, Rect)>, dividers: &mut Vec<DividerRect>) {
            match n {
                Node::Leaf(sid) => panes.push((*sid, r)),
                Node::Split { id, dir, ratio, a, b } => {
                    let total = if *dir == Dir::Row { r.w } else { r.h };
                    let free = (total - DIVIDER).max(0.);
                    let first = (free * ratio).round();
                    let (ra, rd, rb) = if *dir == Dir::Row {
                        (
                            Rect { w: first, ..r },
                            Rect { x: r.x + first, w: DIVIDER, ..r },
                            Rect { x: r.x + first + DIVIDER, w: (free - first).max(0.), ..r },
                        )
                    } else {
                        (
                            Rect { h: first, ..r },
                            Rect { y: r.y + first, h: DIVIDER, ..r },
                            Rect { y: r.y + first + DIVIDER, h: (free - first).max(0.), ..r },
                        )
                    };
                    walk(a, ra, panes, dividers);
                    dividers.push(DividerRect { split: id.clone(), dir: *dir, rect: rd, parent: r });
                    walk(b, rb, panes, dividers);
                }
            }
        }
        walk(&tab.root, area, &mut panes, &mut dividers);
        (panes, dividers)
    }
    /// The pane next to `from` in a direction (for keyboard navigation between panes).
    pub fn neighbor(tab: &Tab, area: Rect, from: Sid, dx: i32, dy: i32) -> Option<Sid> {
        let (panes, _) = Self::layout(tab, area);
        let (_, here) = panes.iter().find(|(s, _)| *s == from)?;
        let (cx, cy) = (here.x + here.w / 2., here.y + here.h / 2.);
        panes
            .iter()
            .filter(|(s, _)| *s != from)
            .filter(|(_, r)| {
                let (px, py) = (r.x + r.w / 2., r.y + r.h / 2.);
                (dx > 0 && px > cx) || (dx < 0 && px < cx) || (dy > 0 && py > cy) || (dy < 0 && py < cy)
            })
            .min_by(|(_, a), (_, b)| {
                let d = |r: &Rect| (r.x + r.w / 2. - cx).powi(2) + (r.y + r.h / 2. - cy).powi(2);
                d(a).total_cmp(&d(b))
            })
            .map(|(s, _)| *s)
    }

    // ---- saved layouts ----

    fn tree_json(&self, n: &Node, leaf: &impl Fn(Sid) -> Value) -> Value {
        match n {
            Node::Leaf(sid) => leaf(*sid),
            Node::Split { dir, ratio, a, b, .. } => json!({"type":"split","dir":dir.name(),"ratio":ratio,
                "a":self.tree_json(a, leaf),"b":self.tree_json(b, leaf)}),
        }
    }
    /// A window as a saved arrangement or as the live layout; `leaf` says what a pane keeps
    /// (its folder and profile, or the key of its session guardian).
    pub fn saved(&self, wid: &str, frame: Value, leaf: &impl Fn(Sid) -> Value) -> Option<Value> {
        let w = self.windows.get(wid)?;
        if w.tabs.is_empty() {
            return None;
        }
        let active = w.tabs.iter().position(|t| t.id == w.active).unwrap_or(0);
        Some(json!({"frame":frame,"active":active,"tabs":w.tabs.iter().map(|t|
            json!({"title":t.title,"root":self.tree_json(&t.root, leaf)})).collect::<Vec<_>>()}))
    }
    /// Builds a tree from a saved one, asking `make` for the session of every leaf.
    pub fn build_tree(&mut self, saved: &Value, make: &mut impl FnMut(&Value) -> Option<Sid>) -> Option<Node> {
        if saved["type"] == "leaf" {
            return make(saved).map(Node::Leaf);
        }
        let a = self.build_tree(&saved["a"], make);
        let b = self.build_tree(&saved["b"], make);
        match (a, b) {
            (Some(a), Some(b)) => {
                let ratio = saved["ratio"].as_f64().filter(|r| r.is_finite()).unwrap_or(0.5).clamp(MIN_RATIO, MAX_RATIO);
                Some(Node::Split { id: self.next_id("s"), dir: Dir::parse(&saved["dir"]), ratio, a: Box::new(a), b: Box::new(b) })
            }
            (one, other) => one.or(other),
        }
    }
    /// Tabs from a saved window, creating one session per leaf.
    pub fn build_tabs(&mut self, saved: &Value, make: &mut impl FnMut(&Value) -> Option<Sid>) -> Vec<Tab> {
        let mut out = vec![];
        for t in saved["tabs"].as_array().into_iter().flatten() {
            if let Some(root) = self.build_tree(&t["root"], make) {
                let title = t["title"].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
                out.push(self.tab_from(root, title));
            }
        }
        out
    }
}
/// The live layout names guardian keys; sessions found by those keys are put back where they
/// were, once each. Panes whose key is missing are dropped and a split with one side left
/// becomes that side. `rest` are the sessions the layout did not mention.
pub fn reattach_ids(saved: &Value, by_key: &HashMap<String, Sid>, used: &mut HashSet<Sid>) -> Option<Value> {
    if saved["type"] == "leaf" {
        let sid = saved["key"].as_str().filter(|k| !k.is_empty()).and_then(|k| by_key.get(k)).copied()?;
        if !used.insert(sid) {
            return None;
        }
        let mut leaf = saved.clone();
        leaf["sid"] = json!(sid);
        return Some(leaf);
    }
    let a = reattach_ids(&saved["a"], by_key, used);
    let b = reattach_ids(&saved["b"], by_key, used);
    match (a, b) {
        (Some(a), Some(b)) => {
            let mut split = saved.clone();
            split["a"] = a;
            split["b"] = b;
            Some(split)
        }
        (one, other) => one.or(other),
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
