//! Axia Store: discover, open and manage apps. A native view: Rust builds its
//! trees and the native view host mounts them (no React). The shell runs only the
//! effects that belong to it: package and signature calls, launching apps.
mod art;
mod catalog;
#[cfg(test)]
mod tests;
mod ui;
use crate::native_view::paint::Image;
use catalog::{Entry, Group};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};
#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Discover,
    Group(Group),
    Categories,
    Category(String),
    Updates,
    Installed,
    Search,
    App(String),
    /// Confirmation before installing a package file.
    File(String),
}
impl Page {
    fn parse(id: &str) -> Self {
        match id {
            "categories" => Self::Categories,
            "updates" => Self::Updates,
            "installed" => Self::Installed,
            _ => Group::ALL
                .into_iter()
                .find(|g| g.id() == id)
                .map_or(Self::Discover, Self::Group),
        }
    }
    /// The sidebar entry that stays selected on this page.
    pub fn nav(&self) -> &'static str {
        match self {
            Self::Discover | Self::Search | Self::App(_) => "discover",
            Self::Group(g) => g.id(),
            Self::Categories | Self::Category(_) => "categories",
            Self::Updates => "updates",
            Self::Installed | Self::File(_) => "installed",
        }
    }
}
/// What an effect's result is for (results come back in order).
#[derive(Clone, Debug, PartialEq)]
enum Tag {
    Packages,
    Status(String),
    Inspect(String),
    Install(String),
    Remove(String),
    Restore(String),
    Key(String),
    Share(String),
}
/// A package file found in the home folder.
#[derive(Clone, Debug, PartialEq)]
pub struct PackageFile {
    pub path: String,
    pub name: String,
    pub size: u64,
}
pub struct App {
    pub page: Page,
    history: Vec<Page>,
    pub query: String,
    args: Value,
    pub entries: Vec<Entry>,
    /// Installed Axia packages, as `lord.apps.packages()` lists them.
    pub packages: Vec<Value>,
    pub statuses: BTreeMap<String, Value>,
    pub files: Vec<PackageFile>,
    /// The file page's `inspectPackage` result (or `{error}`).
    pub inspect: Option<Value>,
    /// A notice: success and its caption.
    pub message: Option<(bool, Value)>,
    effects: VecDeque<(Tag, Value)>,
    inflight: VecDeque<Tag>,
    pub images: HashMap<String, Arc<Image>>,
    /// Icon trees of apps that draw their own (see `DRAWN`), by id and size.
    pub icons: HashMap<(String, u32), Value>,
    /// The session's language (`lang` in the sync).
    pub lang: String,
    /// Installed desktop apps and the catalog's generation; replaced in tests.
    desktop: fn() -> Vec<crate::applications::DesktopFacts>,
    generation: fn() -> u64,
    /// What `entries` was built from: the shell's catalog, the generation, the language.
    built_from: Option<(Value, u64, String)>,
    home: Option<PathBuf>,
}
impl Default for App {
    fn default() -> Self {
        Self {
            page: Page::Discover,
            history: Vec::new(),
            query: String::new(),
            args: Value::Null,
            entries: Vec::new(),
            packages: Vec::new(),
            statuses: BTreeMap::new(),
            files: Vec::new(),
            inspect: None,
            message: None,
            effects: VecDeque::new(),
            inflight: VecDeque::new(),
            images: HashMap::new(),
            icons: HashMap::new(),
            lang: "en".into(),
            desktop: crate::applications::desktop_facts,
            generation: crate::applications::desktop_generation,
            built_from: None,
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }
}
/// Rust mounts the store into the scene (no React).
impl crate::native_view::NativeApp for App {
    fn request(&mut self, input: &Value) -> Value {
        App::request(self, input)
    }
    fn pixels(&self, id: &str) -> Option<(usize, usize, &[u8])> {
        self.images
            .get(id)
            .map(|i| (i.width, i.height, i.data.as_slice()))
    }
}
/// Compiled apps whose icons the store can draw (they are native views too).
const DRAWN: [&str; 16] = ["about", "appabout", "calculator", "characters", "clock", "forcequit", "help", "monitor", "store", "logs", "fonts", "scanner", "recorder", "backup", "camera", "notes"];
/// A manifest string, possibly localized ({"en": …, "pt-BR": …}): the session's
/// language, then its base language, then English, then any.
pub fn catalog_text(v: &Value, lang: &str) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Object(m) => {
            let base = lang.split(['-', '_']).next().unwrap_or(lang);
            m.get(lang)
                .or_else(|| m.get(base))
                .or_else(|| m.get("en"))
                .or_else(|| m.values().next())
                .and_then(Value::as_str)?
                .to_owned()
        }
        _ => return None,
    };
    (!s.is_empty()).then_some(s)
}
impl App {
    fn effect(&mut self, tag: Tag, effect: Value) {
        self.effects.push_back((tag, effect));
    }
    fn read(&mut self, tag: Tag, service: &str, method: &str, args: Value) {
        self.effect(
            tag,
            json!({"kind":"nativeRead","service":service,"method":method,"args":args}),
        );
    }
    fn call(&mut self, tag: Tag, service: &str, method: &str, args: Value) {
        self.effect(
            tag,
            json!({"kind":"nativeCall","service":service,"method":method,"args":args}),
        );
    }
    /// Effects the shell runs without answering (they carry `host`).
    fn shell(&mut self, effect: Value) {
        let mut effect = effect;
        effect["host"] = json!(true);
        self.effects.push_back((Tag::Packages, effect));
    }
    fn next_effect(&mut self) -> Value {
        match self.effects.pop_front() {
            Some((tag, effect)) => {
                if effect["host"] != true {
                    self.inflight.push_back(tag);
                }
                effect
            }
            None => Value::Null,
        }
    }
    fn go(&mut self, page: Page) {
        if page != self.page {
            let old = std::mem::replace(&mut self.page, page);
            self.history.push(old);
            if self.history.len() > 32 {
                self.history.remove(0);
            }
        }
        self.message = None;
        self.enter();
    }
    /// Data a page needs when it opens.
    fn enter(&mut self) {
        match self.page.clone() {
            Page::Installed => {
                self.scan_files();
                self.read(Tag::Packages, "apps", "packages", json!([]));
            }
            Page::File(path) => {
                self.inspect = None;
                self.read(
                    Tag::Inspect(path.clone()),
                    "apps",
                    "inspectPackage",
                    json!([path]),
                );
                // To say which installed version the file replaces.
                self.read(Tag::Packages, "apps", "packages", json!([]));
            }
            _ => {}
        }
    }
    /// `.lpk` files in the home folder, Downloads and the Desktop (as the old Store).
    fn scan_files(&mut self) {
        let Some(home) = self.home.clone() else {
            return;
        };
        let mut out = Vec::new();
        for dir in [
            "",
            "Downloads",
            "Transferências",
            "Descargas",
            "Desktop",
            "Área de trabalho",
            "Escritorio",
        ] {
            let Ok(list) = std::fs::read_dir(home.join(dir)) else {
                continue;
            };
            for e in list.flatten().take(4096) {
                let name = e.file_name().to_string_lossy().into_owned();
                let Ok(meta) = e.metadata() else {
                    continue;
                };
                if meta.is_file() && name.to_lowercase().ends_with(".lpk") {
                    out.push(PackageFile {
                        path: e.path().to_string_lossy().into_owned(),
                        name,
                        size: meta.len(),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out.dedup_by(|a, b| a.path == b.path);
        out.truncate(64);
        self.files = out;
    }
    /// The shell answers a failed package or signature call with `{error: code}`.
    fn error_of(result: &Value) -> Option<Value> {
        match result {
            Value::Object(o) if o.contains_key("error") => Some(json!({"error":o["error"]})),
            _ => None,
        }
    }
    fn result(&mut self, result: &Value) {
        let Some(tag) = self.inflight.pop_front() else {
            return;
        };
        let error = Self::error_of(result);
        // nativeRead answers {method, value}.
        let value = result.get("value").cloned().unwrap_or(Value::Null);
        match tag {
            Tag::Packages => {
                if error.is_none() {
                    self.packages = value
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|mut p| {
                            // The manifest arrives as JSON text.
                            if let Some(text) = p["manifest"].as_str() {
                                p["manifest"] = serde_json::from_str(text).unwrap_or(Value::Null);
                            }
                            p
                        })
                        .collect();
                    // Signature states are shown on Installed only (one read each).
                    if self.page == Page::Installed {
                        let ids: Vec<String> = self
                            .packages
                            .iter()
                            .filter_map(|p| p["id"].as_str().map(str::to_owned))
                            .collect();
                        for id in ids.into_iter().take(64) {
                            self.read(Tag::Status(id.clone()), "sign", "status", json!([id]));
                        }
                    }
                }
            }
            Tag::Status(id) => {
                if error.is_none() {
                    self.statuses.insert(id, value);
                }
            }
            Tag::Inspect(path) => {
                if self.page == Page::File(path) {
                    self.inspect = Some(error.map_or(value, |e| e));
                }
            }
            Tag::Install(_) => {
                self.message = Some(match error {
                    Some(e) => (false, e),
                    None => (true, json!({"key":"store.axia.installed"})),
                });
                self.after_change();
            }
            Tag::Remove(id) => {
                let name = self.package_name(&id);
                self.message = Some(match error {
                    Some(e) => (false, e),
                    None if result == &json!(false) => {
                        (false, json!({"key":"store.axia.removeFailed"}))
                    }
                    None => (
                        true,
                        json!({"key":"store.axia.removed","params":{"name":name}}),
                    ),
                });
                self.after_change();
            }
            Tag::Restore(id) => {
                let name = self.package_name(&id);
                self.message = Some(match error {
                    Some(e) => (false, e),
                    None => (
                        true,
                        json!({"key":"store.restore.done","params":{"name":name}}),
                    ),
                });
                self.after_change();
            }
            Tag::Key(id) => match error {
                Some(e) => self.message = Some((false, e)),
                None => {
                    let status = self.statuses.get(&id).cloned().unwrap_or(Value::Null);
                    let version = status["version"].as_str().unwrap_or("0").to_owned();
                    // As the former Store: "<id>@<version> by <signer>".
                    let based_on = match status["signer"].as_str().filter(|s| !s.is_empty()) {
                        Some(signer) => format!("{id}@{version} by {signer}"),
                        None => format!("{id}@{version}"),
                    };
                    let manifest = self
                        .packages
                        .iter()
                        .find(|p| p["id"] == id.as_str())
                        .map(|p| p["manifest"].clone())
                        .unwrap_or(Value::Null);
                    let mut mine = json!({"name":manifest["name"],"description":manifest["description"],
                        "icon":"icon.png","size":manifest["size"],"minSize":manifest["minSize"],
                        "permissions":manifest["permissions"],"id":id,"version":format!("{version}+mine"),
                        "basedOn":based_on});
                    if mine["permissions"].is_null() {
                        mine["permissions"] = json!([]);
                    }
                    let out = format!("{}/{id}-mine.lpk", self.downloads());
                    self.call(
                        Tag::Share(id.clone()),
                        "sign",
                        "share",
                        json!([
                            id,
                            serde_json::to_string_pretty(&mine).unwrap_or_default(),
                            out
                        ]),
                    );
                }
            },
            Tag::Share(_) => {
                self.message = Some(match error {
                    Some(e) => (false, e),
                    None => (
                        true,
                        json!({"key":"store.share.saved","params":{"path":result.as_str().unwrap_or("")}}),
                    ),
                });
            }
        }
    }
    /// The Downloads folder: xdg-user-dirs, then the usual names, then home.
    fn downloads(&self) -> String {
        let Some(home) = &self.home else {
            return ".".into();
        };
        let conf = std::fs::read_to_string(home.join(".config/user-dirs.dirs")).unwrap_or_default();
        let xdg = conf.lines().find_map(|l| {
            l.strip_prefix("XDG_DOWNLOAD_DIR=\"$HOME/")
                .and_then(|r| r.strip_suffix('"'))
                .map(str::to_owned)
        });
        let dir = xdg
            .or_else(|| {
                ["Downloads", "Transferências", "Descargas"]
                    .into_iter()
                    .find(|d| home.join(d).is_dir())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        home.join(dir).to_string_lossy().into_owned()
    }
    /// After installing, removing or restoring: the shell refreshes its app list and
    /// the store reads the packages again.
    fn after_change(&mut self) {
        self.shell(json!({"kind":"refreshPackages"}));
        self.scan_files();
        self.read(Tag::Packages, "apps", "packages", json!([]));
    }
    fn sync(&mut self, input: &Value) {
        if input["args"] != self.args {
            self.args = input["args"].clone();
            if let Some(path) = self.args["install"].as_str().filter(|p| !p.is_empty()) {
                self.go(Page::File(path.to_owned()));
            } else if let Some(page) = self.args["page"].as_str() {
                self.go(Page::parse(page));
            }
        }
        if let Some(lang) = input["lang"].as_str().filter(|l| !l.is_empty()) {
            self.lang = lang.to_owned();
        }
        // Rebuild the entries only when the shell's catalog, the desktop files or the
        // language changed: a sync follows every click and every effect's answer.
        let key = (input["catalog"].clone(), (self.generation)(), self.lang.clone());
        if self.built_from.as_ref() != Some(&key) {
            let catalog = key.0.as_array().map(Vec::as_slice).unwrap_or(&[]);
            self.entries = catalog::collect(catalog, &(self.desktop)(), &DRAWN, &self.lang);
            self.built_from = Some(key);
        }
    }
    /// A package's display name (its id when the manifest has none).
    fn package_name(&self, id: &str) -> String {
        self.packages
            .iter()
            .find(|p| p["id"] == id)
            .and_then(|p| catalog_text(&p["manifest"]["name"], &self.lang))
            .unwrap_or_else(|| id.to_owned())
    }
    /// Available updates. The store's catalog will provide them; none until then.
    pub fn updates(&self) -> Vec<Value> {
        Vec::new()
    }
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }
    pub fn request(&mut self, input: &Value) -> Value {
        let cmd = input["cmd"].as_str().unwrap_or("");
        let key = input["key"].as_str().unwrap_or("");
        match cmd {
            "icon" => return art::store_icon(input["size"].as_f64().unwrap_or(32.)),
            "close" => {
                self.images.clear();
                self.icons.clear();
                return Value::Null;
            }
            "sync" => self.sync(input),
            "page" => {
                // A sidebar page starts a new history: it has no "back".
                self.query.clear();
                self.history.clear();
                self.page = Page::parse(key);
                self.message = None;
                self.enter();
            }
            "category" => self.go(Page::Category(key.into())),
            "app" => {
                if self.entry(key).is_some() {
                    self.go(Page::App(key.into()));
                }
            }
            "back" => {
                if let Some(page) = self.history.pop() {
                    self.page = page;
                    self.message = None;
                    self.enter();
                }
            }
            "query" => {
                self.query = key.chars().take(256).collect();
                if self.query.trim().is_empty() {
                    if self.page == Page::Search {
                        self.page = self.history.pop().unwrap_or(Page::Discover);
                    }
                } else if self.page != Page::Search {
                    self.go(Page::Search);
                }
            }
            "open" => {
                if self.entry(key).is_some() {
                    self.shell(json!({"kind":"launchApp","app":key}));
                }
            }
            "file" => self.go(Page::File(key.into())),
            "install" => {
                if self.page == Page::File(key.into()) {
                    self.call(
                        Tag::Install(key.into()),
                        "apps",
                        "installPackage",
                        json!([key]),
                    );
                }
            }
            "remove" => {
                if self.packages.iter().any(|p| p["id"] == key) {
                    self.call(
                        Tag::Remove(key.into()),
                        "apps",
                        "removePackage",
                        json!([key]),
                    );
                }
            }
            "restore" => {
                if self.statuses.contains_key(key) {
                    self.call(Tag::Restore(key.into()), "sign", "restore", json!([key]));
                }
            }
            "share" => {
                if self.statuses.contains_key(key) {
                    self.read(Tag::Key(key.into()), "sign", "userKey", json!([]));
                }
            }
            "dismiss" => self.message = None,
            "next" => self.result(&input["result"]),
            _ => {}
        }
        if cmd != "sync" {
            return json!({"effect": self.next_effect()});
        }
        let tree = ui::tree(self, &input["context"]);
        json!({"tree": tree, "effect": self.next_effect(), "title": "Axia Store"})
    }
}
