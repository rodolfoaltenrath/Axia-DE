//! Remembered places read the authoritative owner; only saved preferences leave Rust.
use crate::winmodel::{Rect, Window, WindowState};
use axia_native::windows::{dock_identity, WindowRef};
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const SAVE_DELAY: Duration = Duration::from_millis(800);
#[cfg(test)]
#[path = "window_memory_tests.rs"]
mod tests;
fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn identity(w: &Window<'_>, facts: &Value) -> String {
    dock_identity(
        &WindowRef {
            id: w.id(),
            app: w.app(),
            args: w.args(),
            title: w.title(),
        },
        rows(&facts["registry"]),
        rows(&facts["catalog"]),
    )
}
fn display_for(frame: Rect, displays: &[Value]) -> Option<&Value> {
    let score = |d: &Value| {
        let number = |key: &str| d[key].as_f64().unwrap_or(0.);
        let (x, y, w, h) = (number("x"), number("y"), number("w"), number("h"));
        let overlap = (frame.x + frame.w).min(x + w) - frame.x.max(x);
        let height = (frame.y + frame.h).min(y + h) - frame.y.max(y);
        let dx = (x - frame.x - frame.w / 2.)
            .max(0.)
            .max(frame.x + frame.w / 2. - x - w + 1.);
        let dy = (y - frame.y - frame.h / 2.)
            .max(0.)
            .max(frame.y + frame.h / 2. - y - h + 1.);
        if overlap > 0. && height > 0. {
            overlap * height
        } else {
            -dx * dx - dy * dy
        }
    };
    displays
        .iter()
        .rev()
        .max_by(|a, b| score(a).total_cmp(&score(b)))
}

pub fn restore(windows: &Reader<WindowState>, metadata: &Value, input: &Value) -> Value {
    let Some(key) = input["key"].as_str() else {
        return json!({"frame":null});
    };
    let Some(place) = rows(&input["places"])
        .iter()
        .find(|entry| entry[0] == key)
        .map(|entry| &entry[1])
    else {
        return json!({"frame":null});
    };
    restore_place(windows, metadata, input, key, place)
}

pub fn restore_profile(windows: &Reader<WindowState>, metadata: &Value, input: &Value, preferences: &Value) -> Value {
    let Some(key) = input["key"].as_str() else { return json!({"frame":null}); };
    let Some(place) = preferences["windowPlaces"].get(key) else { return json!({"frame":null}); };
    restore_place(windows, metadata, input, key, place)
}
fn restore_place(windows: &Reader<WindowState>, metadata: &Value, input: &Value, key: &str, place: &Value) -> Value {
    let options = if input["options"]["shift"].is_null() {
        let twins = windows.read().windows().filter(|w| identity(w, metadata) == key).count();
        let mut options = input["options"].as_object().cloned().unwrap_or_default();
        options.insert("shift".into(), json!(twins * 30));
        std::borrow::Cow::Owned(Value::Object(options))
    } else {
        // Explicit placement needs neither a native-window scan nor an options copy.
        std::borrow::Cow::Borrowed(&input["options"])
    };
    let areas: Vec<_> = rows(&input["displays"])
        .iter()
        .map(|d| d["work"].clone())
        .collect();
    json!({"frame":crate::winmodel::restore_frame(place,&input["displays"],&json!(areas),&options)})
}

/// Merge into the latest preferences at the point of persistence, preserving order.
#[cfg(test)]
pub fn merge(places: &Value, updates: &Value) -> Value {
    let mut all = rows(places).to_vec();
    for update in rows(updates) {
        let Some(key) = update[0].as_str() else {
            continue;
        };
        all.retain(|entry| entry[0] != key);
        all.push(update.clone());
        if all.len() > crate::winmodel::MAX_PLACES {
            all.drain(..all.len() - crate::winmodel::MAX_PLACES);
        }
    }
    json!({"entries":all})
}

/// The profile's JSON object is sorted by serde_json. Keep its recency order
/// separately so rebasing/persistence does not turn eviction into key sorting.
fn save_profile(home: &str, updates: &Value) -> Value {
    crate::settings::with(home, |settings| {
        let reader = settings.reader();
        let (places, order) = {
            let profile = reader.read();
            let mut entries = Vec::new();
            let mut seen = std::collections::BTreeSet::new();
            let places = profile["windowPlaces"].as_object();
            for key in rows(&profile["windowPlacesOrder"]).iter().filter_map(Value::as_str) {
                if let Some(value) = places.and_then(|map| map.get(key)).filter(|_| seen.insert(key)) {
                    entries.push((key, value));
                }
            }
            if let Some(places) = places {
                for (key, value) in places {
                    if seen.insert(key.as_str()) { entries.push((key.as_str(), value)); }
                }
            }
            for update in rows(updates) {
                let Some(key) = update[0].as_str() else { continue; };
                entries.retain(|(old, _)| *old != key);
                entries.push((key, &update[1]));
            }
            entries.drain(..entries.len().saturating_sub(crate::winmodel::MAX_PLACES));
            let order = json!(entries.iter().map(|(key, _)| *key).collect::<Vec<_>>());
            let places = Value::Object(entries.into_iter().map(|(key, value)| (key.into(), value.clone())).collect());
            (places, order)
        };
        if settings.set_many(&[("windowPlaces", &places), ("windowPlacesOrder", &order)]) {
            json!({"effects":[{"kind":"settingsState","reply":settings.snapshot()}]})
        } else { json!({}) }
    })
}

pub struct Memory {
    settings: Option<String>,
    windows: Reader<WindowState>,
    changes: Subscription,
    due: Option<Instant>,
    last: BTreeMap<String, (String, Value)>,
    initialized: bool,
    catalog: Option<(Reader<Value>, Subscription)>,
    displays: Reader<Vec<Value>>,
    display_changes: Subscription,
    enabled: bool,
    configured: bool,
}
impl Memory {
    pub fn new(windows: Reader<WindowState>, displays: Reader<Vec<Value>>) -> Self {
        let changes = windows.subscribe();
        let display_changes = displays.subscribe();
        Self {
            settings: None,
            windows,
            changes,
            due: None,
            last: BTreeMap::new(),
            initialized: false,
            catalog: None,
            displays,
            display_changes,
            enabled: false,
            configured: false,
        }
    }
    pub(crate) fn bind_settings(&mut self, home: &str) {
        self.settings = Some(crate::settings::profile(home));
    }
    pub(crate) fn bind_catalog(&mut self, reader: Reader<Value>) {
        let changes = reader.subscribe();
        self.catalog = Some((reader, changes));
    }
    pub(crate) fn enable(&mut self, enabled: bool) {
        if self.enabled != enabled { self.enabled = enabled; self.configured = true; }
        if !enabled { self.due = None; }
    }
    pub(crate) fn invalidated(&self) -> bool {
        self.enabled && (self.configured || !self.initialized || self.changes.pending() || self.display_changes.pending()
            || self.catalog.as_ref().is_some_and(|(_, changes)| changes.pending()))
    }
    pub(crate) fn wait_ms(&self, now: Instant) -> Option<f64> {
        if !self.enabled { return None; }
        if self.invalidated() { return Some(0.); }
        self.due.map(|due| due.saturating_duration_since(now).as_secs_f64() * 1000.)
    }
    pub(crate) fn tick(&mut self, now: Instant) -> Value {
        let changed = self.changes.take().is_some();
        let metadata_changed = self.catalog.as_ref().is_some_and(|(_, changes)| changes.take().is_some());
        let configured = std::mem::take(&mut self.configured) | self.display_changes.take().is_some();
        if !self.enabled {
            self.due = None;
            return json!({});
        }
        if changed || configured || metadata_changed || !self.initialized {
            self.due.get_or_insert(now + SAVE_DELAY);
        }
        self.initialized = true;
        if !self.due.is_some_and(|due| now >= due) {
            return json!({});
        }
        self.due = None;
        let state = self.windows.read();
        let reader = self.catalog.as_ref().map(|(reader, _)| reader.clone());
        let metadata = reader.as_ref().map(Reader::read);
        let empty = Value::Null;
        let input = metadata.as_deref().unwrap_or(&empty);
        let registry = rows(&input["registry"]);
        let displays = self.displays.read();
        let displays = displays.as_slice();
        if displays.is_empty() {
            return json!({});
        }
        let mut updates = Vec::new();
        for w in state.windows() {
            if !crate::window_policy::remembers(crate::winmodel::truthy(&w.args()["dialog"]))
                || (!w.id().starts_with("linux#")
                    && registry
                        .iter()
                        .any(|app| app["id"] == w.app() && app["builtin"] == true))
            {
                continue;
            }
            let key = identity(&w, input);
            if key == "linux:" || key.starts_with("linux:linux#") {
                continue;
            }
            let Some(display) = display_for(w.placement_frame(), displays) else {
                continue;
            };
            let place = w.place_on(display);
            if place.is_null()
                || self
                    .last
                    .get(w.id())
                    .is_some_and(|old| old.0 == key && old.1 == place)
            {
                continue;
            }
            self.last
                .insert(w.id().to_owned(), (key.clone(), place.clone()));
            updates.push(json!([key, place]));
        }
        self.last.retain(|id, _| state.window(id).is_some());
        if !updates.is_empty() {
            if let Some(home) = &self.settings { return save_profile(home, &json!(updates)); }
            #[cfg(test)]
            { return json!({"effects":[{"kind":"windowPlaces","updates":updates}]}); }
            #[cfg(not(test))]
            { json!({}) }
        } else {
            json!({})
        }
    }
}
