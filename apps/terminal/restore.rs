//! Windows that come back: saved arrangements (the menu items "Save Window Arrangement" and
//! "Restore"), the live layout that reopens the same windows with the same shells after the
//! window system restarted, and the frame of a new window sized from its profile. The trees are
//! `workspace.rs` (`saved`, `build_tabs`, `reattach_ids`); the frames of the windows only the
//! window manager knows, so they are asked for with `windows.frames`.
use super::*;
use crate::workspace::{reattach_ids, Meta, Node, Tab};
use std::collections::VecDeque;

/// How often the live layout is looked at.
const LIVE_MS: i64 = 3000;
/// More shells than this in one saved window are not a window anybody saved.
const LEAVES_MAX: usize = 64;

/// What the frames of the windows are wanted for.
pub(super) enum FramesThen {
    SaveArrangement,
    NewWindow { w: f64, h: f64 },
    Live,
}
/// A window being rebuilt from a saved arrangement: one new session per pane, then the tabs.
pub(super) struct Revive {
    saved: Value,
    leaves: Vec<Value>,
    sids: Vec<Option<Sid>>,
    left: usize,
}
/// Tabs of surviving sessions waiting for the window that will show them.
pub(super) struct Prepared {
    frame: Value,
    tabs: Vec<Tab>,
    active: Option<String>,
}
#[derive(Default)]
pub(super) struct LiveState {
    pub checked: bool,
    due: i64,
    busy: bool,
    last: String,
    prepared: VecDeque<Prepared>,
}

fn leaves_of(node: &Value, out: &mut Vec<Value>) {
    if node["type"] == "leaf" {
        out.push(node.clone());
    } else if node["a"].is_object() {
        leaves_of(&node["a"], out);
        leaves_of(&node["b"], out);
    }
}

impl LiveState {
    /// Tests drive time by hand: the next look at the layout is due now.
    #[cfg(test)]
    pub(super) fn force_due(&mut self) {
        (self.due, self.busy) = (0, false);
    }
}

impl App {
    /// A window that starts with nothing to show: one shell, as its arguments ask.
    pub(super) fn start_fresh(&mut self) {
        let id = self.default_profile();
        let args = self.boot_args.clone();
        let cwd = args["cwd"].as_str().filter(|d| d.starts_with('/')).map(str::to_owned);
        // another app can leave an ssh or sftp line typed, never run
        self.first_typed = args["typed"].as_str().filter(|t| ssh_line(t)).map(str::to_owned);
        self.create(Purpose::Window, &id, cwd);
    }

    // ---- arrangements ----

    /// Asks for the frames of every window, to save what they hold.
    pub(super) fn save_arrangement(&mut self) {
        let ids = shell(|s| s.ws.windows().map(|(id, _)| id.clone()).collect::<Vec<_>>());
        self.jobs.enqueue(Tag::Frames { then: FramesThen::SaveArrangement }, "windows.frames", json!([ids]));
    }
    pub(super) fn restore_arrangement(&mut self) {
        let saved = self.terminal_settings()["arrangement"].as_array().cloned().unwrap_or_default();
        if saved.is_empty() {
            self.toast(json!({"key":"terminal.toast.noArrangement"}), clock::now());
            return;
        }
        for window in saved {
            let mut options = json!({"new":true,"args":{"arrangement":window}});
            if window["frame"].is_object() {
                options["frame"] = window["frame"].clone();
            }
            self.jobs.enqueue(Tag::Quiet, "windows.open", json!(["terminal", options]));
        }
    }
    /// A window started with `args.arrangement`: a session for every pane, the tabs once they all exist.
    pub(super) fn revive_begin(&mut self, saved: Value) {
        let mut leaves = vec![];
        for tab in saved["tabs"].as_array().into_iter().flatten() {
            leaves_of(&tab["root"], &mut leaves);
        }
        leaves.truncate(LEAVES_MAX);
        if leaves.is_empty() {
            self.start_fresh();
            return;
        }
        self.revive = Some(Revive { saved, sids: vec![None; leaves.len()], left: leaves.len(), leaves: leaves.clone() });
        for (index, leaf) in leaves.iter().enumerate() {
            let profile = self.profile(leaf["profile"].as_str().unwrap_or(""));
            let mut options = Map::new();
            if let Some(dir) = leaf["cwd"].as_str().filter(|d| d.starts_with('/')) {
                options.insert("cwd".into(), json!(dir));
            }
            options.insert("scrollback".into(), profile["scrollback"].clone());
            self.jobs.enqueue(Tag::Revive { index }, "term.create", json!([Value::Object(options), self.window]));
        }
    }
    pub(super) fn revive_session(&mut self, index: usize, sid: Option<Sid>) {
        let Some(r) = self.revive.as_mut() else {
            if let Some(sid) = sid {
                self.jobs.enqueue(Tag::Quiet, "term.close", json!([sid]));
            }
            return;
        };
        if let Some(slot) = r.sids.get_mut(index) {
            *slot = sid;
        }
        r.left = r.left.saturating_sub(1);
        if r.left == 0 {
            self.revive_finish();
        }
    }
    fn revive_finish(&mut self) {
        let Some(r) = self.revive.take() else { return };
        let window = self.window.clone();
        let mut next = 0;
        let sids = r.sids.clone();
        let tabs = shell(|s| {
            let mut make = |_: &Value| {
                let sid = sids.get(next).copied().flatten();
                next += 1;
                sid
            };
            s.ws.build_tabs(&r.saved, &mut make)
        });
        let made: Vec<(Sid, String)> = r.sids.iter().zip(&r.leaves).filter_map(|(sid, leaf)| Some(((*sid)?, leaf["profile"].as_str().unwrap_or("").to_owned()))).collect();
        if tabs.is_empty() {
            for (sid, _) in made {
                self.jobs.enqueue(Tag::Quiet, "term.close", json!([sid]));
            }
            self.start_fresh();
            return;
        }
        let active = r.saved["active"].as_u64().map_or(0, |i| i as usize).min(tabs.len() - 1);
        let active = tabs[active].id.clone();
        for (sid, profile) in made {
            let profile = self.profile(&profile)["id"].as_str().unwrap_or("").to_owned();
            shell(|s| {
                s.sessions.insert(sid, Session { profile, ..Default::default() });
            });
        }
        shell(|s| s.ws.adopt_window(&window, tabs, Some(&active)));
        self.configure_window();
    }

    // ---- frames ----

    /// The frames of the windows arrived (`null` for a window the window manager does not have).
    pub(super) fn frames_ready(&mut self, then: FramesThen, frames: &Value, now: i64) {
        match then {
            FramesThen::NewWindow { w, h } => {
                let cur = &frames[self.window.as_str()];
                let mut options = json!({"new":true});
                if cur.is_object() {
                    options["frame"] = json!({"x":(number(&cur["x"], 0.) + 30.).round() as i64, "y":(number(&cur["y"], 0.) + 30.).round() as i64, "w":w as i64, "h":h as i64});
                }
                self.jobs.enqueue(Tag::Quiet, "windows.open", json!(["terminal", options]));
            }
            FramesThen::SaveArrangement => {
                let out = Self::windows_saved(frames, false);
                let count = out.len();
                self.save_setting("arrangement", json!(out));
                self.toast(json!({"key":"terminal.toast.arrangementSaved","params":{"count":count}}), now);
            }
            FramesThen::Live => {
                let out = Self::windows_saved(frames, true);
                let text = Value::Array(out.clone()).to_string();
                let changed = shell(|s| {
                    s.live.busy = false;
                    s.live.due = now + LIVE_MS;
                    (s.live.last != text).then(|| s.live.last = text).is_some()
                });
                if changed {
                    self.save_setting("live", json!(out));
                }
            }
        }
    }
    /// A setting saved on its own: no session needs configuring again.
    fn save_setting(&mut self, key: &str, value: Value) {
        let mut cfg = self.terminal_settings();
        cfg[key] = value;
        self.settings = cfg.clone();
        self.effects.push(json!({"kind":"settings","patch":{"terminal":cfg}}));
    }
    /// What each session is, as a saved leaf: its folder (arrangements) or its guardian key (live layout).
    fn leaf_map(live: bool) -> HashMap<Sid, Value> {
        shell(|s| {
            s.sessions
                .iter()
                .map(|(sid, x)| {
                    let leaf = if live {
                        json!({"type":"leaf","key":x.info["key"].as_str().unwrap_or(""),"profile":x.profile})
                    } else {
                        let cwd = x.info["cwd"].as_str().filter(|c| !c.is_empty()).unwrap_or(&x.meta.cwd);
                        json!({"type":"leaf","cwd":cwd,"profile":x.profile})
                    };
                    (*sid, leaf)
                })
                .collect()
        })
    }
    fn windows_saved(frames: &Value, live: bool) -> Vec<Value> {
        let leaves = Self::leaf_map(live);
        let leaf = |sid: Sid| leaves.get(&sid).cloned().unwrap_or_else(|| json!({"type":"leaf"}));
        shell(|s| {
            let mut ids: Vec<&String> = s.ws.windows().map(|(id, _)| id).collect();
            ids.sort();
            ids.into_iter().filter(|id| frames[id.as_str()].is_object()).filter_map(|id| s.ws.saved(id, frames[id.as_str()].clone(), &leaf)).collect()
        })
    }
    /// Every few seconds, once the live layout was settled: the windows are asked for their frames.
    pub(super) fn live_tick(&mut self, now: i64) {
        let ask = shell(|s| {
            if !s.live.checked || s.live.busy || now < s.live.due || (s.ws.windows().next().is_none() && s.live.last.is_empty()) {
                return None;
            }
            s.live.busy = true;
            Some(s.ws.windows().map(|(id, _)| id.clone()).collect::<Vec<_>>())
        });
        if let Some(ids) = ask {
            self.jobs.enqueue(Tag::Frames { then: FramesThen::Live }, "windows.frames", json!([ids]));
        }
    }

    // ---- the live layout ----

    /// The first window of this run: are there shells left from before? They come back where they were.
    pub(super) fn live_begin(&mut self) {
        shell(|s| s.live.checked = true);
        self.jobs.enqueue(Tag::LiveList, "term.list", json!([]));
    }
    pub(super) fn live_listed(&mut self, sids: Vec<Sid>) {
        if sids.is_empty() {
            self.start_fresh();
        } else {
            self.jobs.enqueue(Tag::LiveInfos, "term.infos", json!([sids]));
        }
    }
    pub(super) fn live_infos(&mut self, infos: &Value) {
        let all: Vec<(Sid, Value)> = infos.as_array().into_iter().flatten().filter_map(|i| Some((i["sid"].as_i64()?, i["info"].clone()))).collect();
        let by_key: HashMap<String, Sid> = all.iter().filter_map(|(sid, info)| Some((info["key"].as_str().filter(|k| !k.is_empty())?.to_owned(), *sid))).collect();
        let saved = self.terminal_settings()["live"].as_array().cloned().unwrap_or_default();
        let mut used = std::collections::HashSet::new();
        let mut profiles: HashMap<Sid, String> = HashMap::new();
        let mut windows: Vec<Prepared> = vec![];
        for window in saved {
            let mut tabs_json = vec![];
            for tab in window["tabs"].as_array().into_iter().flatten() {
                if let Some(root) = reattach_ids(&tab["root"], &by_key, &mut used) {
                    let mut leaves = vec![];
                    leaves_of(&root, &mut leaves);
                    for leaf in leaves {
                        if let Some(sid) = leaf["sid"].as_i64() {
                            profiles.insert(sid, leaf["profile"].as_str().unwrap_or("").to_owned());
                        }
                    }
                    tabs_json.push(json!({"title":tab["title"],"root":root}));
                }
            }
            let rebuilt = json!({"tabs":tabs_json});
            let tabs = shell(|s| s.ws.build_tabs(&rebuilt, &mut |leaf| leaf["sid"].as_i64()));
            if tabs.is_empty() {
                continue;
            }
            let index = window["active"].as_u64().map_or(0, |i| i as usize).min(tabs.len() - 1);
            windows.push(Prepared { frame: window["frame"].clone(), active: Some(tabs[index].id.clone()), tabs });
        }
        // shells the layout did not mention: a tab each, in the first window
        let rest: Vec<Sid> = all.iter().map(|(sid, _)| *sid).filter(|sid| !used.contains(sid)).collect();
        for sid in &rest {
            let tab = shell(|s| s.ws.tab_from(Node::Leaf(*sid), None));
            match windows.first_mut() {
                Some(first) => first.tabs.push(tab),
                None => windows.push(Prepared { frame: Value::Null, active: Some(tab.id.clone()), tabs: vec![tab] }),
            }
        }
        let count = all.len();
        let restored = all.iter().any(|(_, info)| info["restored"] == true);
        for (sid, info) in all {
            let profile = self.profile(profiles.get(&sid).map_or("", String::as_str));
            let badge = if profile["badge"].as_str().is_some_and(|b| !b.is_empty()) { profile["badge"].clone() } else { info["badge"].clone() };
            let meta = Meta { title: info["title"].as_str().unwrap_or("").to_owned(), cwd: info["cwd"].as_str().unwrap_or("").to_owned(), ..Default::default() };
            let session = Session { profile: profile["id"].as_str().unwrap_or("").to_owned(), badge: badge.as_str().unwrap_or("").to_owned(), meta, info, ..Default::default() };
            shell(|s| s.sessions.insert(sid, session));
        }
        let mut windows = windows.into_iter();
        let Some(first) = windows.next() else {
            self.start_fresh();
            return;
        };
        let window = self.window.clone();
        shell(|s| s.ws.adopt_window(&window, first.tabs, first.active.as_deref()));
        for next in windows {
            let mut options = json!({"new":true,"args":{"liveAdopt":true}});
            if next.frame.is_object() {
                options["frame"] = next.frame.clone();
            }
            shell(|s| s.live.prepared.push_back(next));
            self.jobs.enqueue(Tag::Quiet, "windows.open", json!(["terminal", options]));
        }
        self.configure_window();
        if restored {
            self.toast(json!({"key":"terminal.toast.restored","params":{"count":count}}), clock::now());
            self.jobs.enqueue(Tag::Quiet, "term.ackRestored", json!([]));
        }
    }
    /// A window started with `liveAdopt`: it takes the next set of tabs prepared for it.
    pub(super) fn live_adopt(&mut self) -> bool {
        let window = self.window.clone();
        shell(|s| match s.live.prepared.pop_front() {
            Some(p) => {
                s.ws.adopt_window(&window, p.tabs, p.active.as_deref());
                true
            }
            None => false,
        })
    }
    /// The frame a new window gets from the default profile's columns and rows, when it has them.
    pub(super) fn new_window(&mut self) {
        let profile = self.profile(&self.default_profile());
        let sid = shell(|s| s.ws.current_sid(&self.window));
        let cell = sid.and_then(|sid| shell(|s| s.sessions.get(&sid).map(|x| (number(&x.info["cellWidth"], 0.), number(&x.info["cellHeight"], 0.)))));
        let (columns, rows, padding) = (number(&profile["columns"], 0.), number(&profile["rows"], 0.), number(&profile["padding"], 0.));
        match cell {
            Some((cw, ch)) if columns > 0. && rows > 0. && cw > 0. => {
                let status = if profile["statusBar"] == true { 24. } else { 0. };
                let (w, h) = ((columns * cw + 2. * padding + 4.).round(), (rows * ch + 2. * padding + 64. + status).round());
                self.jobs.enqueue(Tag::Frames { then: FramesThen::NewWindow { w, h } }, "windows.frames", json!([[self.window]]));
            }
            _ => self.jobs.enqueue(Tag::Quiet, "windows.open", json!(["terminal", {"new":true}])),
        }
    }
}
