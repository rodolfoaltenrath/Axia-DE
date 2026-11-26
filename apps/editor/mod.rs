//! Text Editor: the native window. The project on the side, tabs, the highlighted text area with
//! several cursors, find and replace, go to line, go to file and the input method.
//!
//! The document and the editing rules are in `buffer.rs`, `edit.rs`, `search.rs` and
//! `syntax.rs`; disk access is in `files.rs`; the widget trees are in `ui.rs`. The shared
//! services only run what Rust asks for: the file dialog, the window menus, close guard and
//! events, printing and the input method.
mod buffer;
mod edit;
mod files;
mod search;
mod syntax;
mod ui;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod tests;

use crate::native_view::{
    async_jobs::{Completed, Jobs},
    i18n::I18n,
    measure_text, KeyFacts, NativeApp,
};
use axia_fs::policy::Policy;
use buffer::{pos, Cursor, Doc, Kind, Pos};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::Receiver,
};

pub(crate) const FONT: f64 = 13.;
/// Height of a line of text.
pub(crate) const LINE: f64 = 20.;
/// Weight 400 and the monospaced flag of the text engine.
pub(crate) const MONO: f64 = 1400.;
pub(crate) const PAD: f64 = 10.;
/// Extra lines drawn above and below the visible ones.
const MARGIN: usize = 6;
pub(crate) const SIDEBAR: f64 = 220.;
pub(crate) const TABS: f64 = 34.;
pub(crate) const FIND: f64 = 80.;
pub(crate) const STATUS: f64 = 26.;
pub(crate) const TREE_ROW: f64 = 24.;
const TREE_ROWS: usize = 3000;
/// A folder shown in the project panel is read again after this long.
const LIST_MS: i64 = 2000;
const BLINK_MS: i64 = 530;
const EVENTS_MS: i64 = 200;
const DOUBLE_CLICK_MS: i64 = 400;
/// A line longer than this is only drawn up to here.
const DRAWN_COLUMNS: usize = 20_000;

const CELL_RUN: usize = 400;

fn measure(text: &str) -> f64 {
    measure_text(text, FONT, MONO)
}

/// Highlighting of one tab, kept up to the first changed line.
#[derive(Default)]
struct Highlight {
    language: &'static str,
    /// `states[l]` is the state at the start of line `l`.
    states: Vec<String>,
    /// States are right up to this line.
    known: usize,
    lines: Vec<Option<Rc<Vec<syntax::Token>>>>,
}
impl Highlight {
    fn invalidate(&mut self, language: &'static str, first_changed: usize) {
        if self.language != language || self.states.is_empty() {
            *self = Highlight { language, states: vec!["root".into()], ..Highlight::default() };
            return;
        }
        self.known = self.known.min(first_changed);
        self.lines.truncate(first_changed);
        self.states.truncate(self.known + 1);
    }
    fn set_state(&mut self, line: usize, state: String) {
        if self.states.len() <= line {
            self.states.resize(line + 1, "root".into());
        }
        self.states[line] = state;
    }
    fn state(&self, line: usize) -> &str {
        self.states.get(line).map_or("root", String::as_str)
    }
    fn tokenize(&mut self, doc: &Doc, line: usize) {
        let language = syntax::by_id(self.language);
        let drawn = doc.line(line);
        let text: String = drawn[..drawn.len().min(DRAWN_COLUMNS)].iter().collect();
        let (tokens, end) = syntax::tokenize(language, &text, self.state(line));
        if self.lines.len() <= line {
            self.lines.resize(line + 1, None);
        }
        self.lines[line] = Some(Rc::new(tokens));
        if self.known == line {
            self.set_state(line + 1, end);
            self.known = line + 1;
        }
    }
    fn tokens(&mut self, doc: &Doc, line: usize) -> Rc<Vec<syntax::Token>> {
        // a far jump starts 200 lines earlier from the initial state instead of highlighting
        // everything before it (a very long block comment may look wrong for a moment)
        if line > self.known + 1500 {
            for l in self.known + 1..=line - 200 {
                self.set_state(l, "root".into());
            }
            self.known = line - 200;
        }
        while self.known < line {
            self.tokenize(doc, self.known);
        }
        if self.lines.get(line).and_then(Option::as_ref).is_none() {
            self.tokenize(doc, line);
        }
        self.lines[line].clone().unwrap_or_default()
    }
}

pub(crate) struct Tab {
    id: u64,
    /// An absolute path; none for a new document.
    path: Option<String>,
    doc: Doc,
    language: &'static str,
    tab_size: usize,
    use_tabs: bool,
    top: f64,
    left: f64,
    highlight: Highlight,
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("")
}
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

impl Tab {
    fn new(id: u64, path: Option<String>, text: &str) -> Self {
        let name = path.as_deref().unwrap_or("");
        let wide = [".go", ".c", ".h", ".cpp", ".mk"].iter().any(|e| name.ends_with(e)) || name.contains("Makefile");
        let sample: String = text.chars().take(20_000).collect();
        let indented = sample.split('\n').find(|l| l.starts_with(char::is_whitespace)).unwrap_or("");
        let use_tabs = indented.contains('\t') || name.ends_with("Makefile") || name.ends_with(".go");
        Tab {
            id,
            language: syntax::for_name(name).id,
            path,
            doc: Doc::new(text),
            tab_size: if wide { 4 } else { 2 },
            use_tabs,
            top: 0.,
            left: 0.,
            highlight: Highlight::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PickMode {
    Open,
    Folder,
    /// Save the tab; with `then`, go on closing it (and the window, when it was closing).
    Save { tab: u64, then: Option<bool> },
}
#[derive(Debug, PartialEq)]
enum Overlay {
    None,
    /// The shared file dialog is open.
    Picking,
    GoToFile { query: String, selected: usize },
    Languages { query: String, selected: usize },
    GoToLine { query: String },
    /// Unsaved changes of a tab; `window` when the whole window is closing.
    Confirm { tab: u64, window: bool },
}

#[derive(Default)]
struct FindBar {
    open: bool,
    query: search::Query,
    replacement: String,
    found: search::Found,
    /// What `found` was computed for: tab, document version and query.
    key: Option<(u64, u64, search::Query)>,
    /// Changes to give the search field the focus again.
    focus: u64,
}

/// What a drag in the text area is selecting.
#[derive(Clone, Debug)]
enum Drag {
    Characters(Pos),
    Words(Pos, Pos),
    Lines(usize),
    Column { start: Pos, x: f64, base: Vec<Cursor> },
}

enum Tag {
    Events,
    Pick(PickMode),
    Composition,
    Ignore,
}

pub struct App {
    jobs: Jobs<Tag>,
    effects: Vec<Value>,
    i18n: I18n,
    theme: Rc<Value>,
    dark: bool,
    rtl: bool,
    booted: bool,
    focused: bool,
    home: String,
    /// Tests confine disk access to a scratch home.
    policy: Option<Policy>,
    window: String,
    width: f64,
    height: f64,
    cell: f64,
    args_seen: Value,
    tabs: Vec<Tab>,
    active: u64,
    next_id: u64,
    // project panel
    panel: bool,
    /// The project folder relative to home ("" is home itself), or absolute outside it.
    root: String,
    expanded: BTreeSet<String>,
    listings: HashMap<String, (i64, Rc<Vec<files::Item>>)>,
    tree_top: f64,
    // text area
    editing: bool,
    drag: Option<Drag>,
    /// Where the text area is in the window, as its last press reported.
    origin: (f64, f64),
    clicks: (i64, usize, Pos),
    blink_from: i64,
    // overlays and bars
    overlay: Overlay,
    find: FindBar,
    message: Option<(bool, String)>,
    walking: Option<Receiver<Vec<String>>>,
    walked: Vec<String>,
    // what the shell was last told
    menus: Value,
    title: String,
    guard: Option<bool>,
    input_context: bool,
    composition: Value,
    events_pending: bool,
    events_at: i64,
    last_tree: Value,
}

impl Default for App {
    fn default() -> Self {
        App {
            jobs: Jobs::default(),
            effects: Vec::new(),
            i18n: I18n::default(),
            theme: Rc::new(Value::Null),
            dark: true,
            rtl: false,
            booted: false,
            focused: true,
            home: String::new(),
            policy: None,
            window: String::new(),
            width: 900.,
            height: 600.,
            cell: 7.8,
            args_seen: Value::Null,
            tabs: vec![Tab::new(1, None, "")],
            active: 1,
            next_id: 2,
            panel: true,
            root: String::new(),
            expanded: BTreeSet::new(),
            listings: HashMap::new(),
            tree_top: 0.,
            editing: true,
            drag: None,
            origin: (SIDEBAR, TABS),
            clicks: (0, 0, Pos::default()),
            blink_from: 0,
            overlay: Overlay::None,
            find: FindBar::default(),
            message: None,
            walking: None,
            walked: Vec::new(),
            menus: Value::Null,
            title: String::new(),
            guard: None,
            input_context: false,
            composition: Value::Null,
            events_pending: false,
            events_at: 0,
            last_tree: Value::Null,
        }
    }
}

fn number(v: &Value) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(0.)
}
fn text_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

impl App {
    fn policy(&self) -> Policy {
        self.policy.clone().unwrap_or_else(axia_fs::policy::current)
    }
    fn tr(&mut self, key: &str) -> String {
        self.i18n.t(&format!("editor.{key}"), &Map::new())
    }
    fn tr_with(&mut self, key: &str, vars: Value) -> String {
        let vars = vars.as_object().cloned().unwrap_or_default();
        self.i18n.t(&format!("editor.{key}"), &vars)
    }

    // ------------------------------------------------------------- paths
    fn absolute(&self, path: &str) -> String {
        match path {
            p if p.starts_with('/') => p.to_owned(),
            "" => self.home.clone(),
            p => format!("{}/{p}", self.home),
        }
    }
    fn relative(&self, absolute: &str) -> String {
        match absolute.strip_prefix(&self.home) {
            Some("") => String::new(),
            Some(rest) if rest.starts_with('/') => rest[1..].to_owned(),
            _ => absolute.to_owned(),
        }
    }

    // ------------------------------------------------------------- tabs
    fn index(&self) -> usize {
        self.tabs.iter().position(|t| t.id == self.active).unwrap_or(0)
    }
    pub(crate) fn tab(&self) -> &Tab {
        &self.tabs[self.index()]
    }
    fn tab_mut(&mut self) -> &mut Tab {
        let i = self.index();
        &mut self.tabs[i]
    }
    fn tab_by_id(&self, id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }
    fn tab_name(&mut self, id: u64) -> String {
        match self.tab_by_id(id).and_then(|t| t.path.as_deref()).map(|p| base_name(p).to_owned()) {
            Some(name) => name,
            None => self.tr("untitled"),
        }
    }
    fn add_tab(&mut self, path: Option<String>, text: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab::new(id, path, text));
        id
    }
    fn activate(&mut self, id: u64, now: i64) {
        self.active = id;
        self.drag = None;
        self.focus_text(now);
    }
    fn step_tab(&mut self, forward: bool, now: i64) {
        let n = self.tabs.len();
        let i = (self.index() + if forward { 1 } else { n - 1 }) % n;
        self.activate(self.tabs[i].id, now);
    }
    fn focus_text(&mut self, now: i64) {
        self.editing = true;
        self.blink_from = now;
    }
    fn close_tab(&mut self, id: u64, force: bool, now: i64) {
        if !force && self.tab_by_id(id).is_some_and(|t| t.doc.dirty()) {
            self.activate(id, now);
            self.overlay = Overlay::Confirm { tab: id, window: false };
            return;
        }
        let Some(i) = self.tabs.iter().position(|t| t.id == id) else { return };
        self.tabs.remove(i);
        if self.tabs.is_empty() {
            self.active = self.add_tab(None, "");
        } else if self.active == id {
            self.active = self.tabs[i.saturating_sub(1)].id;
        }
        self.focus_text(now);
    }
    /// The window asked to close: shows the first tab with unsaved changes, or closes.
    fn close_window(&mut self, now: i64) {
        match self.tabs.iter().find(|t| t.doc.dirty()).map(|t| t.id) {
            Some(id) => {
                self.activate(id, now);
                self.overlay = Overlay::Confirm { tab: id, window: true };
            }
            None => self.jobs.enqueue(Tag::Ignore, "windows.close", json!([self.window])),
        }
    }
    /// A confirmed tab was saved or discarded.
    fn finish_close(&mut self, id: u64, window: bool, now: i64) {
        self.overlay = Overlay::None;
        if window {
            self.close_window(now);
        } else {
            self.close_tab(id, true, now);
        }
    }

    // ------------------------------------------------------------- files
    fn open_path(&mut self, path: &str, from_tree: bool, now: i64) {
        self.overlay = Overlay::None;
        if let Some(id) = self.tabs.iter().find(|t| t.path.as_deref() == Some(path)).map(|t| t.id) {
            self.activate(id, now);
            return;
        }
        let name = base_name(path).to_owned();
        match files::read(&self.policy(), &PathBuf::from(path)) {
            Ok(text) => {
                // the project follows the first file opened from outside the tree
                let follows = !from_tree && self.root.is_empty() && self.tabs.iter().all(|t| t.path.is_none());
                let replace = matches!(self.tabs.as_slice(), [only] if only.path.is_none() && !only.doc.dirty() && only.doc.is_empty());
                let id = self.add_tab(Some(path.to_owned()), &text);
                if replace {
                    self.tabs.remove(0);
                }
                if follows {
                    self.root = self.relative(folder_of(path));
                    self.tree_top = 0.;
                }
                self.message = None;
                self.activate(id, now);
            }
            Err(error) => {
                let why = match error {
                    files::ReadError::TooBig(size) => {
                        let size = self.i18n.caption(&json!({"bytes":size}));
                        self.tr_with("tooBig", json!({"size":size}))
                    }
                    files::ReadError::Binary => self.tr("binary"),
                    files::ReadError::Failed(code) => self.i18n.error(&code),
                };
                self.message = Some((false, format!("{name}: {why}")));
                self.focus_text(now);
            }
        }
    }
    fn save_to(&mut self, id: u64, path: &str) -> bool {
        let Some(text) = self.tab_by_id(id).map(|t| t.doc.text()) else { return false };
        match files::write(&self.policy(), &PathBuf::from(path), &text) {
            Ok(()) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                    tab.doc.mark_saved();
                    tab.doc.seal();
                    if tab.path.as_deref() != Some(path) {
                        tab.language = syntax::for_name(path).id;
                        tab.path = Some(path.to_owned());
                    }
                }
                self.listings.clear();
                let message = self.tr_with("saved", json!({"name":base_name(path)}));
                self.message = Some((true, message));
                true
            }
            Err(code) => {
                let error = self.i18n.error(&code);
                let message = self.tr_with("saveError", json!({"error":error}));
                self.message = Some((false, message));
                false
            }
        }
    }
    fn pick(&mut self, mode: PickMode) {
        let tab = match mode {
            PickMode::Save { tab, .. } => self.tab_by_id(tab),
            _ => Some(self.tab()),
        };
        let path = tab.and_then(|t| t.path.clone());
        let (kind, title) = match mode {
            PickMode::Open => ("open", "dialog.open"),
            PickMode::Folder => ("folder", "dialog.folder"),
            PickMode::Save { .. } => ("save", "dialog.save"),
        };
        let start = path.as_deref().map_or_else(|| self.root.clone(), |p| self.relative(folder_of(p)));
        let name = path.as_deref().map_or("", base_name).to_owned();
        let title = self.tr(title);
        self.overlay = Overlay::Picking;
        self.jobs.enqueue(Tag::Pick(mode), "files.pick", json!([{"mode":kind,"title":title,"start":start,"name":name}]));
    }
    fn picked(&mut self, mode: PickMode, path: Option<String>, now: i64) {
        self.overlay = Overlay::None;
        self.focus_text(now);
        let Some(path) = path else { return };
        let absolute = self.absolute(&path);
        match mode {
            PickMode::Open => self.open_path(&absolute, false, now),
            PickMode::Folder => {
                self.root = self.relative(&absolute);
                self.expanded.clear();
                self.listings.clear();
                self.tree_top = 0.;
                self.panel = true;
            }
            PickMode::Save { tab, then } => {
                if self.save_to(tab, &absolute) {
                    if let Some(window) = then {
                        self.finish_close(tab, window, now);
                    }
                }
            }
        }
    }
    fn save(&mut self) {
        let (id, path) = (self.active, self.tab().path.clone());
        match path {
            Some(path) => {
                self.save_to(id, &path);
            }
            None => self.pick(PickMode::Save { tab: id, then: None }),
        }
    }

    // ------------------------------------------------------------- project panel
    fn listing(&mut self, relative: &str, now: i64) -> Rc<Vec<files::Item>> {
        if let Some((at, items)) = self.listings.get(relative) {
            if now - at < LIST_MS {
                return items.clone();
            }
        }
        let items = Rc::new(files::list(&self.policy(), &PathBuf::from(self.absolute(relative))));
        self.listings.insert(relative.to_owned(), (now, items.clone()));
        items
    }
    /// The rows of the project: (path relative to home, name, folder, depth).
    fn tree_rows(&mut self, now: i64) -> Vec<(String, String, bool, usize)> {
        fn visit(app: &mut App, folder: &str, depth: usize, now: i64, out: &mut Vec<(String, String, bool, usize)>) {
            for item in app.listing(folder, now).iter() {
                if out.len() >= TREE_ROWS {
                    return;
                }
                let path = if folder.is_empty() { item.name.clone() } else { format!("{folder}/{}", item.name) };
                let open = item.dir && app.expanded.contains(&path);
                out.push((path.clone(), item.name.clone(), item.dir, depth));
                if open && depth < 32 {
                    visit(app, &path, depth + 1, now, out);
                }
            }
        }
        let mut out = Vec::new();
        let root = self.root.clone();
        visit(self, &root, 0, now, &mut out);
        out
    }

    // ------------------------------------------------------------- text area
    pub(crate) fn area_size(&self) -> (f64, f64) {
        let width = self.width - if self.panel { SIDEBAR } else { 0. };
        let height = self.height - TABS - STATUS - if self.find.open { FIND } else { 0. };
        (width.max(1.), height.max(1.))
    }
    pub(crate) fn gutter(&self) -> f64 {
        let digits = self.tab().doc.line_count().to_string().len().max(3);
        (digits as f64 * self.cell + 22.).round()
    }
    fn metrics(&self) -> edit::Metrics {
        edit::Metrics { cell: self.cell, measure }
    }
    fn page(&self) -> usize {
        ((self.area_size().1 / LINE).floor() as usize).saturating_sub(1).max(1)
    }
    /// Runs `change` on the active document with its indentation style.
    fn edit<R>(&mut self, change: impl FnOnce(&mut Doc, &edit::Style) -> R) -> R {
        let (metrics, page) = (self.metrics(), self.page());
        let tab = self.tab_mut();
        let style = edit::Style {
            language: syntax::by_id(tab.language),
            tab: tab.tab_size,
            use_tabs: tab.use_tabs,
            page,
            metrics,
        };
        change(&mut tab.doc, &style)
    }
    pub(crate) fn x_of(&self, p: Pos) -> f64 {
        let tab = self.tab();
        let line = tab.doc.line(p.l);
        self.metrics().width(&edit::expand(&line[..p.c.min(line.len())], tab.tab_size))
    }
    /// Scrolls so that `p` (or the primary cursor) is in view.
    fn reveal(&mut self, p: Option<Pos>) {
        let (width, height) = self.area_size();
        let p = p.unwrap_or_else(|| self.tab().doc.primary().head());
        let room = width - self.gutter() - PAD * 2.;
        // with several cursors the view follows `p` only when none is visible
        let columns: Vec<f64> = if self.tab().doc.cursors.len() > 1 {
            self.tab().doc.cursors.iter().map(|k| self.x_of(k.head())).collect()
        } else {
            vec![self.x_of(p)]
        };
        let x = self.x_of(p);
        let tab = self.tab_mut();
        let y = p.l as f64 * LINE;
        if y < tab.top {
            tab.top = y;
        }
        if y + LINE > tab.top + height {
            tab.top = y + LINE - height;
        }
        if !columns.iter().any(|&c| c >= tab.left && c <= tab.left + room - 10.) {
            if x < tab.left {
                tab.left = (x - 40.).max(0.);
            }
            if x > tab.left + room - 10. {
                tab.left = x - room + 60.;
            }
        }
        tab.top = tab.top.clamp(0., ((tab.doc.line_count() - 1) as f64 * LINE).max(0.));
    }
    fn applied(&mut self, reply: edit::Reply, now: i64) {
        if let Some(text) = reply.copy {
            self.effects.push(json!({"kind":"copy","host":true,"text":text}));
        }
        if reply.paste {
            self.effects.push(json!({"kind":"paste","host":true}));
        }
        if reply.changed {
            self.blink_from = now;
            self.reveal(reply.reveal);
        }
        if let Some((key, shift)) = reply.window {
            self.shortcut(&key, shift, now);
        }
    }
    /// Selects `from..to` (or puts the cursor at `from`) and shows it a third from the top.
    fn jump(&mut self, from: Pos, to: Option<Pos>, now: i64) {
        self.edit(|doc, _| {
            edit::go_to(doc, from, to);
            doc.seal();
        });
        let height = self.area_size().1;
        let line = self.tab().doc.primary().l;
        self.reveal(None);
        let tab = self.tab_mut();
        if (line as f64 * LINE - tab.top - height / 3.).abs() > height / 3. {
            tab.top = (line as f64 * LINE - height / 3.).max(0.);
        }
        self.blink_from = now;
    }
    /// The position under a point of the window.
    fn position_at(&self, x: f64, y: f64) -> Pos {
        let tab = self.tab();
        let l = ((y - self.origin.1 + tab.top) / LINE).floor().clamp(0., (tab.doc.line_count() - 1) as f64) as usize;
        pos(l, self.column_at(l, x))
    }
    fn column_at(&self, line: usize, x: f64) -> usize {
        let tab = self.tab();
        let style = edit::Style {
            language: syntax::by_id(tab.language),
            tab: tab.tab_size,
            use_tabs: tab.use_tabs,
            page: 1,
            metrics: self.metrics(),
        };
        edit::column_at(tab.doc.line(line), x - self.origin.0 - self.gutter() - PAD + tab.left, &style)
    }
    fn press(&mut self, v: &Value, now: i64) {
        self.overlay = Overlay::None;
        self.focus_text(now);
        if let (Some(x), Some(y)) = (v["frame"][0].as_f64(), v["frame"][1].as_f64()) {
            self.origin = (x, y);
        }
        let (x, y) = (number(&v["x"]), number(&v["y"]));
        let p = self.position_at(x, y);
        let (at, count, last) = self.clicks;
        let count = if now - at < DOUBLE_CLICK_MS && last.l == p.l && last.c.abs_diff(p.c) < 2 { count + 1 } else { 1 };
        self.clicks = (now, count, p);
        let (alt, shift) = (v["alt"] == true, v["shift"] == true);
        let doc = &mut self.tab_mut().doc;
        let drag = if alt {
            // Alt-click adds a cursor; Alt-drag selects a column
            let base = doc.cursors.clone();
            doc.cursors.push(Cursor::at(p));
            doc.normalize();
            Drag::Column { start: p, x, base }
        } else if shift {
            let anchor = doc.primary().anchor();
            doc.cursors = vec![Cursor::range(anchor, p)];
            Drag::Characters(anchor)
        } else if count == 2 {
            let (from, to) = edit::word_at(doc, p);
            doc.cursors = vec![Cursor::range(from, to)];
            Drag::Words(from, to)
        } else if count >= 3 {
            doc.cursors = vec![Cursor::at(p)];
            edit::select_line(doc);
            Drag::Lines(p.l)
        } else {
            doc.cursors = vec![Cursor::at(p)];
            Drag::Characters(p)
        };
        doc.seal();
        self.drag = Some(drag);
        self.reveal(None);
    }
    fn drag_to(&mut self, x: f64, y: f64, now: i64) {
        let Some(drag) = self.drag.clone() else { return };
        let p = self.position_at(x, y);
        let cursors = match &drag {
            Drag::Characters(anchor) => vec![Cursor::range(*anchor, p)],
            Drag::Words(from, to) => {
                let word = edit::word_at(&self.tab().doc, p);
                vec![if p < *from { Cursor::range(*to, word.0) } else { Cursor::range(*from, word.1) }]
            }
            Drag::Lines(line) => {
                let (a, b) = (p.l.min(*line), p.l.max(*line));
                let doc = &self.tab().doc;
                let end = if b + 1 < doc.line_count() { pos(b + 1, 0) } else { pos(b, doc.len(b)) };
                vec![Cursor::range(pos(a, 0), end)]
            }
            Drag::Column { start, x: start_x, base } => {
                let mut cursors = base.clone();
                for l in start.l.min(p.l)..=start.l.max(p.l) {
                    cursors.push(Cursor::range(pos(l, self.column_at(l, *start_x)), pos(l, self.column_at(l, x))));
                }
                cursors
            }
        };
        let doc = &mut self.tab_mut().doc;
        doc.cursors = cursors;
        if matches!(drag, Drag::Column { .. }) {
            doc.normalize();
        }
        self.blink_from = now;
        self.reveal(None);
    }
    fn wheel(&mut self, delta: f64, sideways: bool) {
        let height = self.area_size().1;
        let tab = self.tab_mut();
        if sideways {
            tab.left = (tab.left + delta).max(0.);
        } else {
            let max = (tab.doc.line_count() as f64 * LINE - height + LINE * 3.).max(0.);
            tab.top = (tab.top + delta).clamp(0., max);
        }
    }
    /// The lines to draw.
    pub(crate) fn visible(&self) -> (usize, usize) {
        let tab = self.tab();
        let first = ((tab.top / LINE).floor() as usize).saturating_sub(MARGIN);
        let count = (self.area_size().1 / LINE).ceil() as usize + 1 + MARGIN * 2;
        (first.min(tab.doc.line_count() - 1), (first + count).min(tab.doc.line_count() - 1))
    }

    // ------------------------------------------------------------- find and replace
    fn refresh_find(&mut self) {
        if !self.find.open {
            return;
        }
        let key = (self.active, self.tab().doc.version, self.find.query.clone());
        if self.find.key.as_ref() != Some(&key) {
            self.find.found = search::find(&self.tab().doc, &self.find.query);
            self.find.key = Some(key);
        }
    }
    fn open_find(&mut self) {
        let k = self.tab().doc.primary();
        if self.find.query.text.is_empty() && k.has_selection() && k.l == k.al {
            self.find.query.text = self.tab().doc.text_in(k.start(), k.end());
        }
        self.find.open = true;
        self.find.focus += 1;
        // the search field takes the keyboard
        self.editing = false;
        self.reveal(None);
    }
    fn close_find(&mut self, now: i64) {
        self.find.open = false;
        self.find.key = None;
        self.find.found = search::Found::default();
        self.focus_text(now);
    }
    fn step_match(&mut self, forward: bool, now: i64) {
        self.refresh_find();
        let doc = &self.tab().doc;
        let found = if forward { self.find.found.next(doc) } else { self.find.found.previous(doc) };
        if let Some((from, to)) = found.map(|m| (m.from(), m.to())) {
            self.jump(from, Some(to), now);
        }
    }
    fn replace_one(&mut self, now: i64) {
        self.refresh_find();
        let doc = &self.tab().doc;
        if let Some(edit) = self.find.found.selected(doc).map(|m| self.find.found.edit(doc, m, &self.find.replacement)) {
            self.tab_mut().doc.transact(vec![edit], Kind::Edit, now, |_, rs, _| vec![Cursor::at(rs[0].end)]);
        }
        self.step_match(true, now);
    }
    fn replace_all(&mut self, now: i64) {
        self.refresh_find();
        let doc = &self.tab().doc;
        let edits: Vec<_> = self.find.found.list.iter().map(|m| self.find.found.edit(doc, m, &self.find.replacement)).collect();
        let count = edits.len();
        if count == 0 {
            return;
        }
        self.tab_mut().doc.transact(edits, Kind::Edit, now, |_, rs, _| rs.last().map(|r| vec![Cursor::at(r.end)]).unwrap_or_default());
        let message = self.i18n.t_count(
            "editor.find.replaced",
            &Map::from_iter([("count".to_owned(), json!(count))]),
            count as f64,
        );
        self.message = Some((true, message));
        self.reveal(None);
    }

    // ------------------------------------------------------------- pickers
    /// The rows of the open list: (key, folder part). At most 40, best matches first.
    fn choices(&mut self) -> Vec<(String, String)> {
        let (query, files) = match &self.overlay {
            Overlay::GoToFile { query, .. } => (query.clone(), true),
            Overlay::Languages { query, .. } => (query.clone(), false),
            _ => return Vec::new(),
        };
        let plain = self.tr("plain");
        let all: Vec<(String, String)> = if files {
            self.walked.iter().map(|p| (p.clone(), p.clone())).collect()
        } else {
            syntax::languages()
                .iter()
                .map(|l| (if l.id == "plain" { plain.clone() } else { l.name.to_owned() }, l.id.to_owned()))
                .collect()
        };
        if query.is_empty() {
            return all.into_iter().take(40).collect();
        }
        let mut scored: Vec<_> = all.into_iter().filter_map(|row| search::fuzzy(&query, &row.0).map(|s| (s, row))).collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().take(40).map(|(_, row)| row).collect()
    }
    fn choose(&mut self, index: usize, now: i64) {
        let Some((_, value)) = self.choices().into_iter().nth(index) else { return };
        match self.overlay {
            Overlay::GoToFile { .. } => {
                let root = self.absolute(&self.root.clone());
                self.open_path(&format!("{root}/{value}"), false, now);
            }
            Overlay::Languages { .. } => {
                self.tab_mut().language = syntax::by_id(&value).id;
                self.overlay = Overlay::None;
                self.focus_text(now);
            }
            _ => {}
        }
    }
    fn move_choice(&mut self, down: bool) {
        let count = self.choices().len();
        if let Overlay::GoToFile { selected, .. } | Overlay::Languages { selected, .. } = &mut self.overlay {
            *selected = if down { (*selected + 1).min(count.saturating_sub(1)) } else { selected.saturating_sub(1) };
        }
    }
    fn close_overlay(&mut self, now: i64) {
        if self.overlay != Overlay::Picking {
            self.overlay = Overlay::None;
            self.walking = None;
            self.focus_text(now);
        }
    }

    // ------------------------------------------------------------- commands
    /// A command of the menus, the shortcuts and the buttons.
    fn run(&mut self, name: &str, now: i64) {
        if self.overlay != Overlay::None {
            return;
        }
        match name {
            "new" => {
                let id = self.add_tab(None, "");
                self.activate(id, now);
            }
            "open" => self.pick(PickMode::Open),
            "openFolder" => self.pick(PickMode::Folder),
            "save" => self.save(),
            "saveAs" => self.pick(PickMode::Save { tab: self.active, then: None }),
            "close" => self.close_tab(self.active, false, now),
            "find" => self.open_find(),
            "findNext" => self.step_match(true, now),
            "findPrev" => self.step_match(false, now),
            "gotoLine" => {
                self.editing = false;
                self.overlay = Overlay::GoToLine { query: String::new() };
            }
            "gotoFile" => {
                self.editing = false;
                self.walked.clear();
                let (policy, root) = (self.policy(), PathBuf::from(self.absolute(&self.root)));
                let (send, receive) = std::sync::mpsc::channel();
                let _ = std::thread::Builder::new()
                    .name("axia-editor-walk".into())
                    .spawn(move || send.send(files::walk(&policy, &root)));
                self.walking = Some(receive);
                self.overlay = Overlay::GoToFile { query: String::new(), selected: 0 };
            }
            "language" => {
                self.editing = false;
                self.overlay = Overlay::Languages { query: String::new(), selected: 0 };
            }
            "print" => {
                let title = self.tab_name(self.active);
                let text = self.tab().doc.text();
                self.jobs.enqueue(Tag::Ignore, "print.open", json!([{"title":title,"text":text,"mono":true}]));
            }
            "toggleProject" => {
                self.panel = !self.panel;
                self.reveal(None);
            }
            "nextTab" => self.step_tab(true, now),
            "prevTab" => self.step_tab(false, now),
            _ => {
                if let Some(reply) = self.edit(|doc, style| edit::command(doc, name, style, now)) {
                    self.focus_text(now);
                    self.applied(reply, now);
                }
            }
        }
    }
    /// A shortcut that is not an editing one.
    fn shortcut(&mut self, key: &str, shift: bool, now: i64) -> bool {
        let name = match (key, shift) {
            ("escape", _) => {
                if self.find.open {
                    self.close_find(now);
                }
                return true;
            }
            ("s", false) => "save",
            ("s", true) => "saveAs",
            ("o", false) => "open",
            ("o", true) => "openFolder",
            ("n", _) => "new",
            ("w", _) => "close",
            ("f", _) => "find",
            ("g", false) => "findNext",
            ("g", true) => "findPrev",
            ("l", _) => "gotoLine",
            ("t", _) => "gotoFile",
            ("\\", _) => "toggleProject",
            ("p", _) => "print",
            _ => return false,
        };
        self.run(name, now);
        true
    }

    fn handle_key(&mut self, key: &str, chr: &str, flags: i32, facts: &KeyFacts, now: i64) -> bool {
        if self.overlay != Overlay::None {
            if key == "Escape" {
                self.close_overlay(now);
                return true;
            }
            return false;
        }
        let (ctrl, alt, meta) = (flags & 2 != 0, flags & 4 != 0, flags & 16 != 0);
        let shift = facts.shift || flags & 1 != 0;
        let base = if facts.base.is_empty() { key } else { facts.base }.to_lowercase();
        if (ctrl || meta) && key == "Tab" {
            self.step_tab(!shift, now);
            return true;
        }
        if !self.editing {
            // a field has the keyboard: only the window shortcuts work
            return (ctrl || meta)
                && !alt
                && matches!(base.as_str(), "s" | "o" | "n" | "w" | "t" | "l" | "g" | "p")
                && self.shortcut(&base, shift, now);
        }
        let k = edit::Key { name: key, text: chr, base: &base, shift, ctrl, alt, meta };
        match self.edit(|doc, style| edit::key(doc, &k, style, now)) {
            Some(reply) => {
                self.applied(reply, now);
                true
            }
            None => false,
        }
    }

    // ------------------------------------------------------------- host
    fn event(&mut self, event: &Value, now: i64) {
        match event["kind"].as_str() {
            Some("action") => self.run(&text_of(&event["action"]).to_owned(), now),
            Some("closeRequested") => self.close_window(now),
            _ => {}
        }
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        match done.tag {
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + EVENTS_MS;
                if let Ok(events) = done.result {
                    for event in events.as_array().into_iter().flatten() {
                        self.event(event, now);
                    }
                }
            }
            Tag::Pick(mode) => match done.result {
                Ok(path) => self.picked(mode, path.as_str().map(str::to_owned), now),
                Err(code) => {
                    self.picked(mode, None, now);
                    let error = self.i18n.error(&code);
                    self.message = Some((false, error));
                }
            },
            Tag::Composition => {
                if let Ok(state) = done.result {
                    if self.input_context {
                        self.composition = state;
                    }
                }
            }
            Tag::Ignore => {
                if let Err(code) = done.result {
                    let error = self.i18n.error(&code);
                    self.message = Some((false, error));
                }
            }
        }
    }
    fn context(&mut self, input: &Value, now: i64) {
        self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
        let context = &input["context"];
        if context["theme"].is_object() {
            self.theme = Rc::new(context["theme"].clone());
        }
        self.dark = self.theme["mode"] != "light";
        self.rtl = context["direction"] == "rtl";
        if let Some(width) = context["width"].as_f64().filter(|w| *w > 1.) {
            self.width = width;
        }
        if let Some(height) = context["height"].as_f64().filter(|h| *h > 1.) {
            self.height = height;
        }
        self.focused = context["focused"] != false;
        // the host rounds a width up to whole pixels: a long run keeps the cell exact
        let cell = measure(&"0".repeat(CELL_RUN)) / CELL_RUN as f64;
        if cell > 0. {
            self.cell = cell;
        }
        let args = &input["args"];
        if !self.booted {
            if let Some(owner) = args["__nativeOwner"].as_i64().filter(|o| *o > 0) {
                self.jobs.bind(owner);
                self.booted = true;
                self.window = text_of(&args["__nativeWindow"]).to_owned();
            }
        }
        // "Open with" and opening from Files: a new path or a new request for the same one
        let wanted = json!([args["path"], args["nonce"]]);
        if self.booted && wanted != self.args_seen {
            self.args_seen = wanted;
            if let Some(path) = args["path"].as_str().filter(|p| !p.is_empty()) {
                let absolute = self.absolute(path);
                self.open_path(&absolute, false, now);
            }
        }
    }
    fn command_input(&mut self, command: &str, v: &Value, now: i64) {
        let index = v["key"].as_u64().unwrap_or(0);
        let value = v["value"].as_str();
        // a click on a button takes the keyboard from a field: it goes back to the text
        const BUTTONS: [&str; 13] = [
            "menu", "tab", "closeTab", "treeToggle", "treeOpen", "findRegex", "findCase", "findNext", "findPrev", "replace",
            "replaceAll", "useTabs", "tabSize",
        ];
        if v["button"].is_i64() && self.overlay == Overlay::None && (BUTTONS.contains(&command) || command == "eol") {
            self.focus_text(now);
        }
        match command {
            "menu" => self.run(&text_of(&v["key"]).to_owned(), now),
            "tab" => {
                self.close_overlay(now);
                self.activate(index, now);
            }
            "closeTab" => self.close_tab(index, false, now),
            "areaDown" => self.press(v, now),
            "pointerMove" => self.drag_to(number(&v["x"]), number(&v["y"]), now),
            "dragEnd" => self.drag = None,
            "areaWheel" => self.wheel(number(&v["deltaY"]), v["shift"] == true),
            "treeWheel" => self.tree_top = (self.tree_top + number(&v["deltaY"])).max(0.),
            "treeOpen" => {
                let path = text_of(&v["key"]).to_owned();
                let absolute = self.absolute(&path);
                self.open_path(&absolute, true, now);
            }
            "treeToggle" => {
                let path = text_of(&v["key"]).to_owned();
                if !self.expanded.remove(&path) {
                    self.expanded.insert(path);
                }
            }
            "findQuery" => {
                self.find.query.text = value.unwrap_or("").to_owned();
                self.editing = false;
            }
            "findReplacement" => {
                self.find.replacement = value.unwrap_or("").to_owned();
                self.editing = false;
            }
            "findKey" => {
                self.editing = false;
                self.step_match(v["shift"] != true, now);
            }
            "findRegex" => self.find.query.regex = !self.find.query.regex,
            "findCase" => self.find.query.case_sensitive = !self.find.query.case_sensitive,
            "findNext" => self.step_match(true, now),
            "findPrev" => self.step_match(false, now),
            "findClose" => self.close_find(now),
            "replace" => self.replace_one(now),
            "replaceAll" => self.replace_all(now),
            "listQuery" => {
                if let Overlay::GoToFile { query, selected } | Overlay::Languages { query, selected } = &mut self.overlay {
                    *query = value.unwrap_or("").to_owned();
                    *selected = 0;
                }
            }
            "listKey" => self.move_choice(v["key"] == "ArrowDown"),
            "listSubmit" => {
                if let Overlay::GoToFile { selected, .. } | Overlay::Languages { selected, .. } = self.overlay {
                    self.choose(selected, now);
                }
            }
            "choose" => self.choose(index as usize, now),
            "lineQuery" => {
                if let Overlay::GoToLine { query } = &mut self.overlay {
                    *query = value.unwrap_or("").chars().filter(|c| c.is_ascii_digit() || *c == ':').collect();
                }
            }
            "lineSubmit" => {
                if let Overlay::GoToLine { query } = &self.overlay {
                    let mut parts = query.split(':').map(|p| p.parse::<usize>().ok());
                    let line = parts.next().flatten().filter(|l| *l > 0);
                    let column = parts.next().flatten().filter(|c| *c > 0).unwrap_or(1);
                    self.close_overlay(now);
                    if let Some(line) = line {
                        self.jump(pos(line - 1, column - 1), None, now);
                    }
                }
            }
            "overlayClose" => self.close_overlay(now),
            "discard" => {
                if let Overlay::Confirm { tab, window } = self.overlay {
                    if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab) {
                        t.doc.mark_saved();
                    }
                    self.finish_close(tab, window, now);
                }
            }
            "confirmSave" => {
                if let Overlay::Confirm { tab, window } = self.overlay {
                    match self.tab_by_id(tab).and_then(|t| t.path.clone()) {
                        Some(path) => {
                            if self.save_to(tab, &path) {
                                self.finish_close(tab, window, now);
                            }
                        }
                        None => self.pick(PickMode::Save { tab, then: Some(window) }),
                    }
                }
            }
            "useTabs" => {
                let tab = self.tab_mut();
                tab.use_tabs = !tab.use_tabs;
            }
            "tabSize" => {
                let tab = self.tab_mut();
                tab.tab_size = match tab.tab_size {
                    2 => 4,
                    4 => 8,
                    _ => 2,
                };
                self.reveal(None);
            }
            "eol" => {
                let doc = &mut self.tab_mut().doc;
                doc.eol = if doc.eol == "\n" { "\r\n" } else { "\n" };
                doc.touch();
            }
            "drop" => {
                if let Some(path) = v["paths"][0].as_str() {
                    let absolute = self.absolute(path);
                    self.open_path(&absolute, false, now);
                }
            }
            _ => {}
        }
    }
    fn sync_shell(&mut self) {
        if self.window.is_empty() {
            return;
        }
        let dirty = self.tabs.iter().any(|t| t.doc.dirty());
        if self.guard != Some(dirty) {
            self.guard = Some(dirty);
            self.jobs.enqueue(Tag::Ignore, "windows.closeGuard", json!([self.window, dirty]));
        }
        let menus = ui::menus(self);
        if menus != self.menus {
            self.menus = menus.clone();
            self.jobs.enqueue(Tag::Ignore, "windows.menus", json!([self.window, menus]));
        }
    }
    /// What goes back after any request.
    fn output(&mut self, now: i64) -> Value {
        let mut out = json!({});
        if self.booted {
            let context = self.takes_text();
            if context != self.input_context {
                self.input_context = context;
                if !context {
                    self.composition = Value::Null;
                }
                self.jobs.enqueue(Tag::Ignore, "inputMethod.context", json!([context]));
            }
            if let Some(effect) = self.jobs.pump(now) {
                out["effect"] = effect;
            }
        }
        if !self.effects.is_empty() {
            out["effects"] = json!(std::mem::take(&mut self.effects));
        }
        out["capture"] = json!(self.drag.is_some());
        out
    }
    fn takes_text(&self) -> bool {
        self.focused && self.editing && self.overlay == Overlay::None
    }
    fn sync(&mut self, input: &Value, now: i64) -> Value {
        self.context(input, now);
        if let Some(receive) = &self.walking {
            if let Ok(found) = receive.try_recv() {
                self.walked = found;
                self.walking = None;
            }
        }
        if self.booted && self.focused && !self.events_pending && now >= self.events_at {
            self.events_pending = true;
            self.jobs.enqueue(Tag::Events, "windows.events", json!([]));
            if self.takes_text() {
                self.jobs.enqueue(Tag::Composition, "inputMethod.state", json!([]));
            }
        }
        self.refresh_find();
        self.sync_shell();
        let tree = ui::tree(self, now);
        let mut out = self.output(now);
        let dirty = if self.tab().doc.dirty() { " •" } else { "" };
        let title = format!("{}{dirty}", self.tab_name(self.active));
        if title != self.title {
            self.title = title.clone();
            out["title"] = json!(title);
        }
        if tree == self.last_tree {
            out["same"] = json!(true);
        } else {
            self.last_tree = tree.clone();
            out["tree"] = tree;
        }
        out
    }
    pub(crate) fn request_at(&mut self, input: &Value, now: i64) -> Value {
        match text_of(&input["cmd"]) {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => return self.sync(input, now),
            "next" => {
                for done in self.jobs.complete(&input["result"], now) {
                    self.apply(done, now);
                }
            }
            command => self.command_input(command, input, now),
        }
        self.output(now)
    }
}

impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool {
        true
    }
    fn set_native_appearance(&mut self, theme: Rc<Value>) {
        self.theme = theme;
    }
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
        self.home = home
            .and_then(|h| std::fs::canonicalize(h).ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| home.unwrap_or("").into());
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted.then(|| if self.jobs.busy() || self.walking.is_some() { 50. } else { EVENTS_MS as f64 })
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        self.jobs.busy().then_some(100.)
    }
    fn text_input(&self) -> bool {
        self.takes_text()
    }
    fn blur(&mut self) {
        self.editing = false;
        self.drag = None;
    }
    fn paste(&mut self, text: &str) -> Option<Value> {
        if !self.takes_text() {
            return None;
        }
        let now = crate::clock::now();
        self.edit(|doc, _| edit::paste(doc, text, now));
        self.applied(edit::Reply::changed(true), now);
        Some(self.output(now))
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        self.key_event(key, chr, flags, &KeyFacts::default())
    }
    fn key_event(&mut self, key: &str, chr: &str, flags: i32, facts: &KeyFacts) -> Option<Value> {
        let now = crate::clock::now();
        self.handle_key(key, chr, flags, facts, now).then(|| self.output(now))
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, crate::clock::now())
    }
}
