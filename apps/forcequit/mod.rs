//! Force Quit application. Metadata, grouping, selection, execution policy and view live in Rust.
#[cfg(test)]
mod tests;
mod ui;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet, VecDeque};

#[derive(Clone)]
struct Window {
    id: String,
    surface: Option<i32>,
    x11: bool,
    isolated: bool,
    icon: Value,
}
#[derive(Clone)]
struct Group {
    key: String,
    name: String,
    windows: Vec<Window>,
}
#[derive(Default)]
pub struct App {
    groups: Vec<Group>,
    selected: Option<String>,
    error: String,
    queue: VecDeque<Window>,
    pending: Option<Window>,
    isolated_killed: bool,
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
    let mut groups: Vec<_> = groups.into_values().collect();
    // Accents and case don't move a name ("Área" sorts with "A", as the previous localeCompare did).
    groups.sort_by_cached_key(|g| (crate::search::matching::fold(&g.name), g.key.clone()));
    groups
}
impl App {
    fn refresh(&mut self, input: &Value) {
        self.groups = groups(&input["windows"]);
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
