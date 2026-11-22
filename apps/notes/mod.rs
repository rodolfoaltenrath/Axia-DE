//! Notes is entirely native: session storage, editor, Markdown and desktop cards.
// Each literal owns one compiled pattern. No parsing, locks or allocations on hits.
macro_rules! pattern {
    ($source:literal) => {{
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| regex::Regex::new($source).unwrap())
    }};
}
mod persistence;
mod editor;
mod emoji;
use axia_native::markdown;
pub mod store;
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::{
    async_jobs::{Completed, Jobs},
    i18n::I18n,
    NativeApp,
};
use serde_json::{json, Value};
use std::path::PathBuf;
enum Tag {
    Ignore,
    Events,
    Composition,
    Clipboard(String, String, (usize, usize)),
    Image(String, String, String, (usize, usize)),
}
#[derive(Clone)]
struct Pick {
    path: String,
    dir: bool,
    name: String,
}
struct Drag {
    kind: String,
    point: (f64, f64),
    rect: (f64, f64, f64, f64),
}
pub struct App {
    home: String,
    i18n: I18n,
    theme: Value,
    width: f64,
    height: f64,
    screen: (f64, f64),
    rtl: bool,
    widget: bool,
    geometry: (f64, f64, f64, f64),
    mount_geometry: (f64, f64, f64, f64),
    widget_slug: String,
    sel: Option<String>,
    query: String,
    sidebar_offset: f64,
    sidebar_drag: Option<(f64, f64)>,
    mode: String,
    toc: bool,
    panel: String,
    emoji_query: String,
    emoji_cat: String,
    picker: PathBuf,
    picks: Vec<Pick>,
    hover: bool,
    drag: Option<Drag>,
    editor: editor::Editor,
    jobs: Jobs<Tag>,
    booted: bool,
    window: String,
    args: Value,
    events_pending: bool,
    events_at: i64,
    menus: Value,
    effects: Vec<Value>,
    last_tree: Value,
    presentation: Value,
    ui_revision: u64,
    sidebar_tree: crate::native_view::retained::RetainedTree,
    reader_tree: crate::native_view::retained::RetainedTree,
    input_context: bool,
    foreground: bool,
    composition: Value,
    region: String,
    error: String,
}
impl Default for App {
    fn default() -> Self {
        Self {
            home: String::new(),
            i18n: I18n::default(),
            theme: Value::Null,
            width: 860.,
            height: 560.,
            screen: (1920., 1080.),
            rtl: false,
            widget: false,
            geometry: (0., 0., 260., 240.),
            mount_geometry: (0., 0., 260., 240.),
            widget_slug: String::new(),
            sel: None,
            query: String::new(),
            sidebar_offset: 0.,
            sidebar_drag: None,
            mode: "read".into(),
            toc: true,
            panel: String::new(),
            emoji_query: String::new(),
            emoji_cat: "caras".into(),
            picker: PathBuf::new(),
            picks: vec![],
            hover: false,
            drag: None,
            editor: editor::Editor::default(),
            jobs: Jobs::default(),
            booted: false,
            window: String::new(),
            args: Value::Null,
            events_pending: false,
            events_at: 0,
            menus: Value::Null,
            effects: vec![],
            last_tree: Value::Null,
            presentation: Value::Null,
            ui_revision: 0,
            sidebar_tree: Default::default(),
            reader_tree: Default::default(),
            input_context: false,
            foreground: true,
            composition: Value::Null,
            region: String::new(),
            error: String::new(),
        }
    }
}
impl NativeApp for App {
    fn retained_tree(&self, key: &str) -> Option<&Value> {
        self.sidebar_tree.tree(key).or_else(|| self.reader_tree.tree(key))
    }
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
        self.home = home
            .and_then(|h| std::fs::canonicalize(h).ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| home.unwrap_or("").into());
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.booted
            .then(|| if self.mode == "edit" || self.jobs.busy() || store::with(&self.home, |s| s.busy()) {
                50.
            } else {
                200.
            })
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        (self.jobs.busy() || !self.home.is_empty() && store::with(&self.home, |s| s.busy())).then_some(100.)
    }
    fn text_input(&self) -> bool {
        self.foreground && self.mode == "edit" && self.editor.focused && self.panel.is_empty()
    }
    fn blur(&mut self) {
        self.editor.focused = false;
        self.sidebar_drag = None;
        if self.widget {
            self.mode = "read".into();
        }
        if !self.home.is_empty() {
            store::with(&self.home, |s| s.flush(crate::clock::now(), true));
        }
    }
    fn paste(&mut self, text: &str) -> Option<Value> {
        if !self.text_input() {
            return None;
        }
        self.editor.insert(text, crate::clock::now());
        self.save_editor();
        Some(self.output(crate::clock::now()))
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        let now = crate::clock::now();
        if self.widget && !self.text_input() && self.panel.is_empty() {
            return None;
        }
        if key == "Escape" && !self.panel.is_empty() {
            self.panel.clear();
            return Some(self.output(now));
        }
        if !self.panel.is_empty() {
            return None;
        }
        if flags & (2 | 16) != 0 {
            if key.eq_ignore_ascii_case("n") {
                self.new_note(now);
                return Some(self.output(now));
            }
            if key.eq_ignore_ascii_case("e") {
                self.set_mode(if self.mode == "edit" { "read" } else { "edit" });
                return Some(self.output(now));
            }
        }
        if !self.text_input() {
            return None;
        }
        if let Some(result) = self.editor.key(key, chr, flags, now) {
            if let Some(copy) = result["copy"].as_str() {
                self.effects
                    .push(json!({"kind":"copy","host":true,"text":copy}));
            }
            if result["paste"] == true {
                self.clipboard();
            }
            if result["escape"] == true {
                self.set_mode("read");
            }
        }
        self.save_editor();
        Some(self.output(now))
    }
    fn uses_preferences(&self) -> bool { true }
    fn sync_with_preferences(&mut self, v: &Value, preferences: &Value) -> Value {
        self.request_with_preferences_at(v, preferences, crate::clock::now())
    }
    fn request(&mut self, v: &Value) -> Value {
        self.request_at(v, crate::clock::now())
    }
}
impl App {
    fn note(&self) -> Option<store::Note> {
        if self.home.is_empty() {
            return None;
        }
        store::with(&self.home, |s| {
            if self.widget {
                return s.get(&self.widget_slug).cloned();
            }
            let slug = self
                .sel
                .as_deref()
                .filter(|slug| s.get(slug).is_some())
                .map(str::to_owned)
                .or_else(|| s.sidebar(&self.query).first().map(|n| n.slug.clone()));
            slug.and_then(|slug| s.get(&slug).cloned())
        })
    }
    fn save_editor(&mut self) {
        if let Some(note) = self.note() {
            if note.body != self.editor.body {
                store::with(&self.home, |s| {
                    s.update(
                        &note.slug,
                        &json!({"body":self.editor.body}),
                        true,
                        crate::clock::now(),
                    )
                });
            }
        }
    }
    fn set_mode(&mut self, mode: &str) {
        if let Some(n) = self.note() {
            if self.mode != mode {
                self.editor.reset(&n.body);
            }
            self.mode = mode.into();
            self.editor.focused = mode == "edit";
            if mode == "read" {
                store::with(&self.home, |s| s.flush(crate::clock::now(), true));
            }
        }
    }
    fn new_note(&mut self, now: i64) {
        if self.home.is_empty() {
            return;
        }
        let body = format!(
            "# {}\n\n",
            self.i18n.caption(&json!({"key":"notes.newTitle"}))
        );
        match store::with(&self.home, |s| s.create(body, "yellow", false, now)) {
            Ok(slug) => {
                self.sel = Some(slug);
                self.sidebar_offset = 0.;
                self.mode = "read".into();
                self.set_mode("edit");
            }
            Err(e) => self.error = e,
        }
    }
    fn update(&mut self, patch: Value, touch: bool, now: i64) {
        if let Some(n) = self.note() {
            store::with(&self.home, |s| s.update(&n.slug, &patch, touch, now));
        }
    }
    fn output(&mut self, now: i64) -> Value {
        let context = self.text_input();
        if self.booted && context != self.input_context {
            self.input_context = context;
            if !context {
                self.composition = Value::Null;
            }
            self.jobs
                .enqueue(Tag::Ignore, "inputMethod.context", json!([context]));
        }
        let mut out = json!({});
        if let Some(e) = self.jobs.pump(now) {
            out["effect"] = e;
        }
        if !self.effects.is_empty() {
            out["effects"] = json!(std::mem::take(&mut self.effects));
        }
        out["capture"] = json!(self.drag.is_some() || self.editor.drag.is_some() || self.sidebar_drag.is_some());
        out
    }
    fn clipboard(&mut self) {
        if let Some(n) = self.note() {
            self.jobs.enqueue(
                Tag::Clipboard(n.slug, n.body, self.editor.range()),
                "clipboard.read",
                json!([]),
            );
        }
    }
    fn file_list(&mut self) {
        self.picks.clear();
        let root = PathBuf::from(&self.home);
        let Ok(path) = std::fs::canonicalize(&self.picker) else {
            return;
        };
        if !path.starts_with(&root) {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let Ok(path) = std::fs::canonicalize(entry.path()) else {
                    continue;
                };
                if !path.starts_with(&root) {
                    continue;
                }
                let dir = path.is_dir();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.')
                    || (!dir
                        && self.panel == "image"
                        && !pattern!(r"(?i)\.(png|jpe?g|gif|bmp|webp)$")
                            .is_match(&name))
                {
                    continue;
                }
                self.picks.push(Pick {
                    path: path.to_string_lossy().into_owned(),
                    dir,
                    name,
                });
            }
        }
        self.picks.sort_by(|a, b| {
            b.dir
                .cmp(&a.dir)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    }
    fn insert(&mut self, md: &str, now: i64) {
        if let Some(n) = self.note() {
            if self.mode == "edit" {
                self.editor.insert(md, now);
                self.save_editor();
            } else {
                store::with(&self.home, |s| {
                    s.update(
                        &n.slug,
                        &json!({"body":format!("{}\n\n{md}\n",n.body.trim_end())}),
                        true,
                        now,
                    )
                });
            }
        }
    }
    fn menu(&mut self) {
        if self.window.is_empty() || self.widget {
            return;
        }
        let note = self.note();
        let tr = |s: &str, i: &mut I18n| json!(i.caption(&json!({"key":s})));
        let item = |label: Value, cmd: &str, disabled: bool| json!({"label":label,"action":[cmd],"disabled":disabled});
        let mut desktop = item(
            tr("notes.showDesktop", &mut self.i18n),
            "desktop",
            note.is_none(),
        );
        desktop["checked"] = json!(note.as_ref().is_some_and(|n| n.desktop));
        let menus = json!([{"label":tr("notes.menu.file",&mut self.i18n),"items":[item(tr("notes.new",&mut self.i18n),"new",false),item(tr("notes.duplicate",&mut self.i18n),"duplicate",note.is_none()),desktop,item(tr("notes.openFolder",&mut self.i18n),"folder",note.is_none()),item(tr("notes.print",&mut self.i18n),"print",note.is_none()),item(tr("notes.delete",&mut self.i18n),"delete",note.is_none())]},{"label":tr("notes.menu.view",&mut self.i18n),"items":[item(tr(if self.mode=="edit"{"notes.read"}else{"notes.edit"},&mut self.i18n),"toggle",note.is_none()),item(tr("notes.toc",&mut self.i18n),"toc",false)]}]);
        let mut menus = menus;
        let formats = [("bold","bold"),("italic","italic"),("link","link"),("h1","h1"),("h2","h2"),("bullets","bullet"),("numbers","num"),("task","task"),("code","code")].into_iter().map(|(label,kind)| json!({"label":tr(&format!("notes.fmt.{label}"), &mut self.i18n),"action":["format",kind],"disabled":note.is_none()})).collect::<Vec<_>>();
        menus.as_array_mut().unwrap().insert(
            1,
            json!({"label":tr("notes.menu.format",&mut self.i18n),"items":formats}),
        );
        if menus != self.menus {
            self.menus = menus.clone();
            self.jobs
                .enqueue(Tag::Ignore, "windows.menus", json!([self.window, menus]));
        }
    }
    fn apply(&mut self, done: Completed<Tag>, now: i64) {
        match done.tag {
            Tag::Composition => {
                if let Ok(state) = done.result {
                    if self.text_input() {
                        self.composition = state;
                    }
                }
            }
            Tag::Events => {
                self.events_pending = false;
                self.events_at = now + 100;
                if let Ok(v) = done.result {
                    for e in v.as_array().into_iter().flatten() {
                        if e["kind"] == "action" {
                            let action = &e["action"];
                            self.command(
                                action[0].as_str().unwrap_or(""),
                                &json!({"key":action[1]}),
                                now,
                            );
                        }
                    }
                }
            }
            Tag::Clipboard(slug, base, range) => {
                if let Ok(v) = done.result {
                    if v["kind"] == "image" {
                        let result = store::with(&self.home, |s| {
                            let dir = s.path(&slug).map_err(|e| e.to_string())?;
                            let mut i = 1;
                            while dir.join(format!("image-{i}.png")).exists() {
                                i += 1;
                            }
                            let name = format!("image-{i}.png");
                            Ok::<_, String>((
                                dir.join(&name).to_string_lossy().into_owned(),
                                format!("![{name}]({name})"),
                            ))
                        });
                        match result {
                            Ok((path, md)) => self.jobs.enqueue(
                                Tag::Image(slug, md, base, range),
                                "clipboard.image",
                                json!([path]),
                            ),
                            Err(e) => self.error = e,
                        }
                    } else if let Some(text) = v["text"].as_str() {
                        if self.note().is_some_and(|n| n.slug == slug) {
                            self.editor.insert(text, now);
                            self.save_editor();
                        }
                    }
                }
            }
            Tag::Image(slug, md, base, range) => match done.result {
                Ok(v) if v == true => store::with(&self.home, |s| {
                    s.insert_attachment(&slug, &md, &base, range, now)
                }),
                Err(e) => self.error = e,
                _ => {}
            },
            Tag::Ignore => {
                if let Err(e) = done.result {
                    self.error = e
                }
            }
        }
    }
    fn command(&mut self, cmd: &str, input: &Value, now: i64) {
        if self.home.is_empty() {
            return;
        }
        let arg = input["key"].as_str().unwrap_or("");
        let allowed = match self.panel.as_str() {
            "delete" => ["confirmDelete", "dismiss", "hover"].contains(&cmd),
            "image" | "attach" => {
                ["pickHome", "pickUp", "pickFile", "dismiss", "hover"].contains(&cmd)
            }
            "emoji" => [
                "emoji",
                "emojiFirst",
                "emojiQuery",
                "emojiCat",
                "dismiss",
                "hover",
            ]
            .contains(&cmd),
            _ => true,
        };
        if !allowed {
            return;
        }
        if matches!(self.panel.as_str(), "more" | "insert") {
            self.panel.clear();
        }
        match cmd {
            "new" => self.new_note(now),
            "select" => {
                if store::with(&self.home, |s| s.get(arg).is_some()) {
                    self.sel = Some(arg.into());
                    self.mode = "read".into();
                    self.editor.focused = false;
                }
            }
            "query" => { self.query = input["value"].as_str().unwrap_or(arg).into(); self.sidebar_offset = 0.; }
            "sidebarWheel" => {
                self.sidebar_offset = (self.sidebar_offset + input["deltaY"].as_f64().unwrap_or(0.)).max(0.);
            }
            "sidebarDrag" => self.sidebar_drag = Some((input["y"].as_f64().unwrap_or(0.), self.sidebar_offset)),
            "mode" => {
                if ["read", "edit"].contains(&arg) {
                    self.set_mode(arg);
                }
            }
            "toggle" => self.set_mode(if self.mode == "edit" { "read" } else { "edit" }),
            "toc" => self.toc = !self.toc,

            "color" => self.update(
                json!({ "color":arg} ),
                false,
                now,
            ),
            "desktop" => {
                if let Some(n) = self.note() {
                    self.update(
                        json!({ "desktop":!n.desktop} ),
                        false,
                        now,
                    );
                }
            }
            "pin" => {
                if let Some(n) = self.note() {
                    self.update(
                        json!({ "pinned":!n.pinned} ),
                        false,
                        now,
                    );
                }
            }
            "hideWidget" => {
                self.update(
                    json!({ "desktop":false} ),
                    false,
                    now,
                );
                store::with(&self.home, |s| s.flush(now, true));
            }
            "openWindow" => {
                if let Some(n) = self.note() {
                    self.jobs.enqueue(
                        Tag::Ignore,
                        "windows.open",
                        json!(["notes", { "args":{ "slug":n.slug, "nonce":now} } ]),
                    );
                }
            }
            "duplicate" => {
                if let Some(n) = self.note() {
                    match store::with(&self.home, |s| s.duplicate(&n.slug, now)) {
                        Ok(slug) => self.sel = Some(slug),
                        Err(e) => self.error = e,
                    }
                }
            }
            "delete" => self.panel = "delete".into(),
            "confirmDelete" => {
                if let Some(n) = self.note() {
                    if let Err(e) = store::with(&self.home, |s| s.delete(&n.slug)) {
                        self.error = e;
                    }
                    self.sel = None;
                    self.panel.clear();
                    self.mode = "read".into();
                }
            }
            "folder" => {
                if let Some(n) = self.note() {
                    let path = store::with(&self.home, |s| s.path(&n.slug).ok());
                    if let Some(path) = path {
                        self.jobs.enqueue(
                            Tag::Ignore,
                            "windows.open",
                            json!(["files", { "new":true, "args":{ "path":path} } ]),
                        );
                    }
                }
            }
            "print" => {
                if let Some(n) = self.note() {
                    self.jobs.enqueue(
                        Tag::Ignore,
                        "print.open",
                        json!([{ "title":if n.title.is_empty(){ self.i18n.caption(&json!({ "key":"notes.untitled"} ))} else{ n.title} , "text":n.body} ]),
                    );
                }
            }
            "panel" => {
                if ["emoji", "image", "attach", "more", "insert"].contains(&arg) {
                    self.panel = arg.into();
                    if arg == "image" || arg == "attach" {
                        self.picker = PathBuf::from(&self.home);
                        self.file_list();
                    }
                }
            }
            "dismiss" => self.panel.clear(),
            "emojiQuery" => self.emoji_query = input["value"].as_str().unwrap_or("").into(),
            "emojiCat" => {
                if emoji::CATEGORIES.contains(&arg) {
                    self.emoji_cat = arg.into();
                }
            }
            "emoji" => {
                self.insert(arg, now);
                self.panel.clear();
            }
            "emojiFirst" => {
                if let Some(e) = emoji::search(&self.emoji_query).first() {
                    self.insert(&e.ch.clone(), now);
                    self.panel.clear();
                }
            }
            "pickHome" => {
                self.picker = PathBuf::from(&self.home);
                self.file_list();
            }
            "pickUp" => {
                if let Some(parent) = self.picker.parent().filter(|p| p.starts_with(&self.home)) {
                    self.picker = parent.to_path_buf();
                    self.file_list();
                }
            }
            "pickFile" => {
                if let Some(pick) = self.picks.iter().find(|p| p.path == arg).cloned() {
                    if pick.dir {
                        self.picker = PathBuf::from(pick.path);
                        self.file_list();
                    } else if let Some(n) = self.note() {
                        let range = if self.mode == "edit" {
                            self.editor.range()
                        } else {
                            let end = n.body.chars().count();
                            (end, end)
                        };
                        if let Err(e) =
                            store::with(&self.home, |s| s.attach(&n.slug, &pick.path, range))
                        {
                            self.error = e;
                        }
                        self.panel.clear();
                    }
                }
            }
            "toggleTask" => {
                if let (Some(n), Some(line)) = (self.note(), input["key"].as_u64()) {
                    let mut lines = n.body.split('\n').map(str::to_owned).collect::<Vec<_>>();
                    if let Some(l) = lines.get_mut(line as usize) {
                        let r = pattern!(r"\[([ xX])\]");
                        if let Some(c) = r.captures(l) {
                            let m = c.get(1).unwrap();
                            let (a, b) = (m.start(), m.end());
                            l.replace_range(a..b, if &c[1] == " " { "x" } else { " " });
                        }
                    }
                    self.update(
                        json!({ "body":lines.join("\n")} ),
                        true,
                        now,
                    );
                }
            }
            "link" => {
                if !arg.to_lowercase().starts_with("http:")
                    && !arg.to_lowercase().starts_with("https:")
                {
                    if let Some(n) = self.note() {
                        let path = if arg.starts_with('/') {
                            PathBuf::from(arg)
                        } else {
                            store::with(&self.home, |s| s.path(&n.slug).unwrap_or_default())
                                .join(arg)
                        };
                        if let Ok(path) = std::fs::canonicalize(path) {
                            if path.starts_with(&self.home) {
                                let edit=pattern!(r"(?i)\.(txt|md|js|jsx|ts|json|c|h|py|sh|css|html|ya?ml|toml|ini|csv|log)$").is_match(&path.to_string_lossy());
                                let path = if edit {
                                    path
                                } else {
                                    path.parent().unwrap_or(&path).to_path_buf()
                                };
                                self.jobs.enqueue(
                                    Tag::Ignore,
                                    "windows.open",
                                    json!([if edit{ "editor"} else{ "files"} , { "new":!edit, "args":{ "path":path} } ]),
                                );
                            }
                        }
                    }
                }
            }
            "tocJump" => self.effects.push(json!({ "kind":"scrollTo", "host":true, "key":"reading", "target":format!("heading-{arg}")} )),
            "format" => {
                if self.mode != "edit" {
                    self.set_mode("edit");
                }
                self.editor.format(arg, now);
                self.save_editor();
            }
            "acceptEmoji" => {
                self.editor
                    .accept(input["key"].as_u64().unwrap_or(0) as usize, now);
                self.save_editor();
            }
            "editorDown" => {
                if let (Some(x), Some(y)) = (input["x"].as_f64(), input["y"].as_f64()) {
                    self.editor.down(x, y, input["shift"] == true, now);
                }
            }
            "editorWheel" => {
                self.editor.scroll = (self.editor.scroll + input["deltaY"].as_f64().unwrap_or(0.))
                    .clamp(0., self.editor.max_scroll())
            }

            "hover" => self.hover = arg == "on",
            "dragStart" => {
                let x = input["x"].as_f64().unwrap_or(0.) + self.mount_geometry.0;
                let y = input["y"].as_f64().unwrap_or(0.) + self.mount_geometry.1;
                self.drag = Some(Drag {
                    kind: arg.into(),
                    point: (x, y),
                    rect: self.geometry,
                });
            }
            "pointerMove" => {
                if let Some((y, offset)) = self.sidebar_drag {
                    let count = store::with(&self.home, |s| s.sidebar(&self.query).len());
                    let height = ui::sidebar_height(self);
                    let full = count as f64 * ui::SIDEBAR_ROW;
                    let thumb = (height * height / full.max(1.)).max(24.).min(height);
                    self.sidebar_offset = (offset + (input["y"].as_f64().unwrap_or(y) - y)
                        * (full - height).max(0.) / (height - thumb).max(1.)).clamp(0., (full - height).max(0.));
                } else if let Some(d) = &self.drag {
                    let dx = input["x"].as_f64().unwrap_or(0.) + self.mount_geometry.0 - d.point.0;
                    let dy = input["y"].as_f64().unwrap_or(0.) + self.mount_geometry.1 - d.point.1;
                    let (x, y, w, h) = d.rect;
                    let patch = if d.kind == "move" {
                        json!({ "x":(x+dx).clamp(0., (self.screen.0-60.).max(0.)), "y":(y+dy).clamp(24., (self.screen.1-40.).max(24.)), "w":w, "h":h} )
                    } else {
                        json!({ "x":x, "y":y, "w":(w+dx).max(160.), "h":(h+dy).max(110.)} )
                    };
                    self.geometry = (
                        patch["x"].as_f64().unwrap(),
                        patch["y"].as_f64().unwrap(),
                        patch["w"].as_f64().unwrap(),
                        patch["h"].as_f64().unwrap(),
                    );
                    self.update(patch, false, now);
                } else if let (Some(x), Some(y)) = (input["x"].as_f64(), input["y"].as_f64()) {
                    self.editor.drag_to(x, y);
                }
            }
            "dragEnd" => {
                self.sidebar_drag = None;
                self.drag = None;
                self.editor.drag = None;
                store::with(&self.home, |s| s.flush(now, true));
            }
            _ => {}
        }
    }

    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        self.request_with_preferences_at(input, &input["settings"], now)
    }
    fn request_with_preferences_at(&mut self, input: &Value, preferences: &Value, now: i64) -> Value {
        let cmd = input["cmd"].as_str().unwrap_or("");
        match cmd {
            "icon" => return ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                let presentation = json!([input["context"], input["lang"], preferences["region"]]);
                if presentation != self.presentation {
                    self.presentation = presentation;
                    self.ui_revision = self.ui_revision.wrapping_add(1);
                }
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").into();
                self.theme = input["context"]["theme"].clone();
                self.foreground = input["context"]["focused"].as_bool().unwrap_or(true);
                self.region = preferences["region"].as_str().unwrap_or("").into();
                self.width = input["context"]["width"].as_f64().unwrap_or(860.);
                self.height = input["context"]["height"].as_f64().unwrap_or(560.);
                self.screen = (
                    input["context"]["screen"]["width"]
                        .as_f64()
                        .unwrap_or(1920.),
                    input["context"]["screen"]["height"]
                        .as_f64()
                        .unwrap_or(1080.),
                );
                let lang = self.i18n.lang.clone();
                self.rtl = self.i18n.entry(&lang, "meta", "direction") == Some(json!("rtl"));
                let args = input["args"].clone();
                self.widget = args["widget"].is_string();
                self.window = args["__nativeWindow"].as_str().unwrap_or("").into();
                if args != self.args {
                    if let Some(slug) = args["widget"].as_str().or_else(|| args["slug"].as_str()) {
                        self.sel = Some(slug.into());
                        self.widget_slug = slug.into();
                        if self.args["widget"] != args["widget"]
                            || self.args["slug"] != args["slug"]
                            || self.args["nonce"] != args["nonce"]
                        {
                            self.mode = "read".into();
                            self.editor.focused = false;
                        }
                    }
                    if let Some(r) = args["rect"].as_object() {
                        let get = |k: &str, f: f64| r.get(k).and_then(Value::as_f64).unwrap_or(f);
                        self.mount_geometry = (
                            get("left", 0.),
                            get("top", 0.),
                            get("width", 260.),
                            get("height", 240.),
                        );
                        if self.drag.is_none() {
                            self.geometry = self.mount_geometry;
                        }
                    }
                    self.args = args;
                }
                if !self.booted {
                    if let Some(owner) = input["args"]["__nativeOwner"].as_i64().filter(|i| *i > 0)
                    {
                        self.jobs.bind(owner);
                        self.booted = true;
                    }
                }
                if !self.home.is_empty() {
                    store::with(&self.home, |s| s.tick(now));
                    if let Some(n) = self.note() {
                        self.sel = Some(n.slug.clone());
                        if self.editor.body != n.body {
                            let focused = self.editor.focused;
                            self.editor.reset(&n.body);
                            self.editor.focused = focused;
                        }
                        if self.widget {
                            self.geometry = (n.x, n.y, n.w, n.h);
                        }
                    }
                }
                if self.booted
                    && self.foreground
                    && (!self.widget || self.text_input())
                    && !self.events_pending
                    && now >= self.events_at
                {
                    self.events_pending = true;
                    self.jobs.enqueue(Tag::Events, "windows.events", json!([]));
                    if self.text_input() {
                        self.jobs
                            .enqueue(Tag::Composition, "inputMethod.state", json!([]));
                    }
                }
                self.menu();
                let tree = ui::tree(self, now);
                let mut reply = self.output(now);
                if tree == self.last_tree {
                    reply["same"] = json!(true);
                } else {
                    self.last_tree = tree.clone();
                    reply["tree"] = tree;
                }
                return reply;
            }
            "next" => {
                for done in self.jobs.complete(&input["result"], now) {
                    self.apply(done, now);
                }
            }
            "close" => {
                self.editor.focused = false;
                if !self.home.is_empty() {
                    // at process exit the session may be gone already (it flushed in its Drop)
                    if store::try_with(&self.home, |s| s.flush(now, true)).is_none() {
                        return json!({});
                    }
                }
            }
            _ => self.command(cmd, input, now),
        }
        self.output(now)
    }
}
