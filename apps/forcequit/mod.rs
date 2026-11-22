//! Force Quit application. Metadata, grouping, selection, execution policy and view live in Rust.
#[cfg(test)]
mod tests;
mod ui;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::rc::Rc;
use crate::native_view::windows::{WindowRef, WindowSource};

#[derive(Clone)]
pub struct Window {
    pub id: String,
    surface: Option<i32>,
    x11: bool,
    isolated: bool,
    pub icon: Value,
}
#[derive(Clone)]
pub struct Group {
    pub key: String,
    pub name: String,
    pub windows: Vec<Window>,
}
#[derive(Default)]
pub struct App {
    appearance: Option<std::rc::Rc<Value>>,
    groups: Vec<Group>,
    selected: Option<String>,
    error: String,
    queue: VecDeque<Window>,
    pending: Option<Window>,
    isolated_killed: bool,
    source: Option<Rc<dyn WindowSource>>,
    revision: Option<u64>,
    metadata: axia_state::Store<Value>,
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn groups(windows: &Value) -> Vec<Group> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    let mut seen = HashSet::new();
    for w in windows.as_array().into_iter().flatten() {
        let id = s(w, "id");
        let linux = id.starts_with("linux#");
        if id.is_empty()
            || s(w, "app") == "forcequit"
            || (!linux && w["builtin"] == true)
            || !seen.insert(id.to_owned())
        {
            continue;
        }
        let key = s(w, "key");
        if key.is_empty() {
            continue;
        }
        let group = groups.entry(key.into()).or_insert_with(|| Group {
            key: key.into(),
            name: s(w, "name").into(),
            windows: Vec::new(),
        });
        group.windows.push(Window {
            id: id.into(),
            surface: if linux {
                w["surface"]
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .filter(|n| *n > 0)
            } else {
                None
            },
            x11: linux && w["x11"] == true,
            isolated: linux && w["isolated"] == true,
            icon: w["iconData"].clone(),
        });
    }
    sorted_groups(groups)
}
fn sorted_groups(groups: BTreeMap<String, Group>) -> Vec<Group> {
    let mut groups: Vec<_> = groups.into_values().collect();
    // Accents and case don't move a name ("Área" sorts with "A", as the previous localeCompare did).
    groups.sort_by_cached_key(|g| (crate::search::matching::fold(&g.name), g.key.clone()));
    groups
}

fn pretty_id(id: &str) -> String {
    let name = id.rsplit('.').find(|part| !part.is_empty()).unwrap_or("").replace(['-', '_'], " ");
    let name = name.trim();
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// Metadata comes from the shared app catalog; window membership and handles
/// come only from the borrowed authoritative model, never the JS mirror.
fn native_groups(source: &dyn WindowSource, input: &Value, include_forcequit: bool) -> Vec<Group> {
    let registry: BTreeMap<_, _> = input["registry"].as_array().into_iter().flatten()
        .map(|app| (s(app, "id"), app)).collect();
    let catalog = input["catalog"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut groups = BTreeMap::new();
    source.visit(&mut |w: WindowRef<'_>| {
        let linux = w.id.starts_with("linux#");
        let app = registry.get(w.app).copied().unwrap_or(&Value::Null);
        if w.id.is_empty() || (!include_forcequit && w.app == "forcequit") || (!linux && app["builtin"] == true) { return; }
        let app_id = s(w.args, "appId");
        let package = registry.get(app_id).copied().filter(|app| app["package"] == true || app["isolated"] == true);
        let desktop = catalog.iter().find(|app| s(app, "kind") == "linux" &&
            crate::native_view::windows::matches_desktop(app_id, s(app, "desktopId"), s(app, "wmClass")));
        let key = crate::native_view::windows::launcher_identity(&w,
            input["registry"].as_array().map(Vec::as_slice).unwrap_or(&[]),catalog);
        let group_app = registry.get(key.as_str()).copied();
        let name = group_app.map(|app| s(app, "name").to_owned()).or_else(||
            desktop.map(|app| s(app, "shortName").to_owned()).filter(|name| !name.is_empty()))
            .unwrap_or_else(|| {
                if !linux { s(app, "name").to_owned() }
                else if let Some(package) = package { s(package, "name").to_owned() }
                else {
                    let pretty = pretty_id(app_id);
                    if pretty.is_empty() { s(app, "name").to_owned() } else { pretty }
                }
            });
        let icon_app = if linux { package.filter(|app| app["icon"] == true).or(desktop) }
            else { (app["icon"] == true).then_some(app) };
        let icon = icon_app.map(|app| app["iconData"].clone())
            .unwrap_or_else(|| json!({"kind":"letter","name":if !linux {w.id}
                else if !app_id.is_empty() {app_id} else if !w.title.is_empty() {w.title} else {"Linux"}}));
        groups.entry(key.clone()).or_insert_with(|| Group {key, name, windows: Vec::new()})
            .windows.push(Window {
                id: w.id.to_owned(),
                surface: linux.then(|| w.args["surface"].as_i64().and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0)).flatten(),
                x11: linux && w.args["x11"] == true,
                isolated: linux && w.args["pkg"] == true,
                icon,
            });
    });
    sorted_groups(groups)
}
impl App {
    pub fn metadata(&self) -> std::cell::Ref<'_, Value> { self.metadata.read() }
    pub fn metadata_reader(&self) -> axia_state::Reader<Value> { self.metadata.reader() }
    /// Window-management commands also operate on the Force Quit window itself.
    /// Keep their projection separate from the panel's selection/execution state.
    pub fn command_groups(&self) -> Vec<Group> {
        self.source.as_ref().map_or_else(Vec::new, |source| native_groups(source.as_ref(), &self.metadata.read(), true))
    }
    /// Native consumers share membership and metadata policy without JSON summaries.
    pub fn native_groups(&mut self, input: &Value) -> &[Group] {
        self.refresh(input);
        &self.groups
    }
    pub fn with_windows(source: Rc<dyn WindowSource>) -> Self {
        Self { source: Some(source), ..Self::default() }
    }
    pub fn with_window_metadata(source: Rc<dyn WindowSource>, metadata: axia_state::Store<Value>) -> Self {
        Self { source: Some(source), metadata, ..Self::default() }
    }
    fn refresh(&mut self, input: &Value) {
        self.groups = if let Some(source) = &self.source {
            let metadata = self.metadata.read();
            let facts = if input["registry"].is_array() { input } else { &metadata };
            let groups = native_groups(source.as_ref(), facts, false);
            self.revision = Some(source.revision());
            groups
        } else { groups(&input["windows"]) };
        if !self
            .groups
            .iter()
            .any(|g| Some(&g.key) == self.selected.as_ref())
        {
            self.selected = None;
        }
    }
    fn summaries(&self) -> Value {
        json!(self.groups.iter().map(|g| json!({"key":g.key,"name":g.name,"windows":g.windows.iter().map(|w| &w.id).collect::<Vec<_>>()})).collect::<Vec<_>>())
    }
    fn begin(&mut self, key: &str) -> Value {
        self.queue.clear();
        self.pending = None;
        self.isolated_killed = false;
        self.error.clear();
        self.selected = None;
        if let Some(g) = self.groups.iter().find(|g| g.key == key) {
            self.queue.extend(g.windows.iter().cloned());
        } else {
            self.error = "app.notFound".into();
        }
        self.next(None)
    }
    fn next(&mut self, result: Option<&str>) -> Value {
        if let Some(w) = self.pending.take() {
            let error = if w.x11 {
                "app.forceQuitX11"
            } else {
                result.unwrap_or("app.forceQuitFailed")
            };
            if self.error.is_empty() && !error.is_empty() {
                self.error = error.into();
            }
            if error.is_empty() && w.surface.is_some() && w.isolated {
                self.isolated_killed = true;
            }
        }
        while let Some(w) = self.queue.pop_front() {
            // Only a successful isolated-client kill suppresses its remaining windows.
            // Failed attempts are retried on the next surface; native Linux clients stay independent.
            if w.isolated && self.isolated_killed && !w.x11 {
                continue;
            }
            let effect = match w.surface {
                Some(id) => json!({"kind":if w.x11 {"closeSurface"} else {"kill"},"surface":id}),
                None => json!({"kind":"closeWindow","id":w.id}),
            };
            self.pending = Some(w);
            return json!({"effect":effect,"error":self.error});
        }
        json!({"effect":null,"error":self.error})
    }
    pub fn request(&mut self, input: &Value) -> Value {
        match s(input, "cmd") {
            "metadata" => {
                if !input["registry"].as_array().is_some_and(|apps| apps.len() <= 4096)
                    || !input["catalog"].as_array().is_some_and(|apps| apps.len() <= 4096) {
                    return json!({"error":"app.invalidRequest"});
                }
                // Updated once per catalog/locale generation by the adapter;
                // group/quit requests borrow these facts without another copy.
                self.metadata.set(json!({"registry":input["registry"],"catalog":input["catalog"]}));
                json!({"ok":true})
            }
            "groups" => {
                self.refresh(input);
                json!({"groups":self.summaries()})
            }
            "sync" => {
                self.refresh(input);
                json!({"tree":ui::tree(self,&input["context"]),"selected":self.selected,"error":self.error})
            }
            "select" => {
                let key = s(input, "key");
                self.selected = self
                    .groups
                    .iter()
                    .any(|g| g.key == key)
                    .then(|| key.to_owned());
                json!({"selected":self.selected})
            }
            "quit" => {
                self.refresh(input);
                let key = input["key"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| self.selected.clone())
                    .unwrap_or_default();
                self.begin(&key)
            }
            "next" => self.next(input["result"].as_str()),
            "icon" => ui::icon(input["size"].as_f64().unwrap_or(32.)),
            _ => json!({"error":"app.invalidRequest"}),
        }
    }
}

/// The window uses the session controller with fresh shell facts. It never
/// executes the surface handles from its last rendered snapshot.
impl crate::native_view::NativeApp for App {
    fn uses_native_appearance(&self) -> bool { true }
    fn set_native_appearance(&mut self, theme: std::rc::Rc<Value>) { self.appearance = Some(theme); }

    fn invalidated(&self) -> bool {
        self.source.as_ref().is_some_and(|source| self.revision != Some(source.revision()))
    }
    fn request(&mut self, input: &Value) -> Value {
        match s(input, "cmd") {
            "quit" => {
                let key = self.selected.take();
                self.error.clear();
                key.map_or(
                    Value::Null,
                    |key| json!({"effect":{"kind":"forceQuit","key":key}}),
                )
            }
            "next" => {
                self.error = input["result"]
                    .as_str()
                    .unwrap_or("app.forceQuitFailed")
                    .into();
                Value::Null
            }
            _ => App::request(self, input),
        }
    }
    fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
        None
    }
}
