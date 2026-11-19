//! Remembered places read the authoritative owner; only saved preferences leave Rust.
use crate::winmodel::{Rect, Window, WindowState};
use axia_native::{
    windows::{dock_identity, WindowRef},
    NativeApp,
};
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
    let twins = windows
        .read()
        .windows()
        .filter(|w| identity(w, metadata) == key)
        .count();
    let mut options = input["options"].clone();
    if !options.is_object() {
        options = json!({});
    }
    if options["shift"].is_null() {
        options["shift"] = json!(twins * 30);
    }
    let areas: Vec<_> = rows(&input["displays"])
        .iter()
        .map(|d| d["work"].clone())
        .collect();
    json!({"frame":crate::winmodel::restore_frame(place,&input["displays"],&json!(areas),&options)})
}

/// Merge into the latest preferences at the point of persistence, preserving order.
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

pub struct Memory {
    windows: Reader<WindowState>,
    changes: Subscription,
    due: Option<Instant>,
    last: BTreeMap<String, (String, Value)>,
    initialized: bool,
    configuration_revision: Value,
}
impl Memory {
    pub fn new(windows: Reader<WindowState>) -> Self {
        let changes = windows.subscribe();
        Self {
            windows,
            changes,
            due: None,
            last: BTreeMap::new(),
            initialized: false,
            configuration_revision: Value::Null,
        }
    }
    fn sync(&mut self, input: &Value, now: Instant) -> Value {
        let changed = self.changes.take().is_some();
        if input["args"]["enabled"] != true {
            self.due = None;
            return json!({});
        }
        let configured = self.configuration_revision != input["context"]["renderEpoch"];
        if configured {
            self.configuration_revision = input["context"]["renderEpoch"].clone();
        }
        if changed || configured || !self.initialized {
            self.due.get_or_insert(now + SAVE_DELAY);
        }
        self.initialized = true;
        if !self.due.is_some_and(|due| now >= due) {
            return json!({});
        }
        self.due = None;
        let state = self.windows.read();
        let registry = rows(&input["registry"]);
        let displays = rows(&input["args"]["displays"]);
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
            json!({"effects":[{"kind":"windowPlaces","updates":updates}]})
        } else {
            json!({})
        }
    }
}
impl NativeApp for Memory {
    fn invalidated(&self) -> bool {
        self.changes.pending()
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.due
            .map(|due| due.saturating_duration_since(Instant::now()).as_secs_f64() * 1000. + 0.01)
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        self.refresh_ms()
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] == "sync" {
            self.sync(input, Instant::now())
        } else {
            json!({})
        }
    }
}
