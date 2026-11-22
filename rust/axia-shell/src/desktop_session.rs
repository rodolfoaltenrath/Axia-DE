//! Desktop persistence and theme hand-off belong to the context's window owner.
use crate::winmodel::{AppCommand, Session, WindowState};
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub struct Service {
    pub(crate) animation: crate::desktop_animation::Animation,
    settings: Option<(String, Reader<Value>, Subscription)>,
    windows: Reader<WindowState>,
    changes: Subscription,
    started: bool,
    saved_count: Option<usize>,
    rtl: bool,
    surfaces: BTreeMap<i64, usize>,
}

fn surface(value: &Value) -> Option<i64> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && n.fract() == 0. && *n > 0. && *n <= i32::MAX as f64)
        .map(|n| n as i64)
}

impl Service {
    pub fn new(windows: Reader<WindowState>) -> Self {
        Self {
            animation: crate::desktop_animation::Animation::new(windows.clone()),
            changes: windows.subscribe(),
            settings: None,
            windows,
            started: false,
            saved_count: None,
            rtl: false,
            surfaces: BTreeMap::new(),
        }
    }
    pub(crate) fn bind_settings(&mut self, home: &str) {
        let home = crate::settings::profile(home);
        if self.settings.as_ref().is_some_and(|(current, _, _)| current == &home) { return; }
        let reader = crate::settings::with(&home, |s| s.reader());
        let changes = reader.subscribe();
        self.settings = Some((home, reader, changes));
    }
    fn persist(&self, key: &str, value: &Value) -> Vec<Value> {
        if let Some((home, _, _)) = &self.settings {
            crate::settings::with(home, |s| {
                if s.set(key, value) { vec![json!({"kind":"settingsState","reply":s.snapshot()})] }
                else { Vec::new() }
            })
        } else { vec![json!({"kind":"settings","patch":{key:value}})] }
    }
    pub fn invalidated(&self) -> bool {
        self.changes.pending() || self.settings.as_ref().is_some_and(|(_, _, changes)| changes.pending())
    }
    fn index(&self, id: Option<i64>) -> usize {
        let state = self.windows.read();
        state
            .desktops()
            .iter()
            .position(|d| *d == id.unwrap_or(state.active_desktop()))
            .unwrap_or(0)
    }
    pub fn effects(&mut self) -> Vec<Value> {
        self.changes.take();
        if let Some((_, reader, changes)) = &self.settings {
            changes.take();
            self.animation.configure(reader.read()["reduceMotion"]==true, self.rtl);
        }
        self.animation.sync(crate::native_view::now_ms());
        if !self.started {
            return Vec::new();
        }
        let count = self.windows.read().desktops().len();
        if self.saved_count == Some(count) {
            return Vec::new();
        }
        self.saved_count = Some(count);
        self.persist("desktopCount", &json!(count))
    }
    fn at(&self, session: &mut Session, index: usize) -> Value {
        if let Some(id) = self.windows.read().desktops().get(index) {
            return json!({"result":id});
        }
        let mut reply = session
            .app_command(AppCommand::PrepareDesktops(index, None), &[])
            .unwrap_or_else(|e| json!({"error":e}));
        reply["result"] = json!(self.windows.read().desktops().get(index).copied());
        reply
    }
    pub fn command(&mut self, session: &mut Session, input: &Value) -> Value {
        let mut reply = match input["action"].as_str() {
            Some("start") if !self.started => {
                let reader = self.settings.as_ref().map(|(_, reader, _)| reader.clone());
                let borrowed = reader.as_ref().map(Reader::read);
                let settings = borrowed.as_deref().unwrap_or(&input["settings"]);
                let count =
                    crate::winmodel::desktops::clean_count(&settings["desktopCount"]) as usize;
                let was = &settings["themeDesktops"];
                let active = (crate::winmodel::desktops::clean_index(&was["active"]) as usize)
                    .min(count - 1);
                if let Some(surfaces) = was["surfaces"].as_object() {
                    for (sid, index) in surfaces {
                        let Some(sid) = sid
                            .parse::<i64>()
                            .ok()
                            .filter(|s| *s > 0 && *s <= i32::MAX as i64)
                        else {
                            continue;
                        };
                        let Some(index) = index.as_f64().filter(|i| {
                            i.fract() == 0.
                                && (0. ..crate::winmodel::desktops::MAX_DESKTOPS as f64).contains(i)
                        }) else {
                            continue;
                        };
                        self.surfaces.insert(sid, index as usize);
                    }
                }
                self.started = true;
                // Start does not rewrite an absent/invalid count until the desktop list changes.
                self.saved_count = Some(count);
                let mut reply = session
                    .app_command(AppCommand::InitializeDesktops(count, active), &[])
                    .unwrap_or_else(|e| json!({"error":e}));
                self.animation.initialize();
                let consume_handoff = !was.is_null();
                drop(borrowed);
                if consume_handoff { reply["effects"] = json!(self.persist("themeDesktops", &Value::Null)); }
                reply
            }
            Some("start") => json!({"result":null}),
            Some("at") => self.at(
                session,
                crate::winmodel::desktops::clean_index(&input["index"]) as usize,
            ),
            Some("active") => json!({"result":self.index(None)}),
            Some("index") => {
                json!({"result":input["id"].as_i64().map(|id|self.index(Some(id))).unwrap_or(0)})
            }
            Some("surface") => {
                match surface(&input["surface"]).and_then(|id| self.surfaces.get(&id).copied()) {
                    Some(index) => self.at(session, index),
                    None => json!({"result":null}),
                }
            }
            Some("theme") => {
                let state = self.windows.read();
                let mut surfaces = BTreeMap::new();
                for w in state.windows() {
                    if let Some(sid) = surface(&w.args()["surface"]) {
                        let index = state
                            .desktops()
                            .iter()
                            .position(|d| *d == w.desktop())
                            .unwrap_or(0);
                        surfaces.insert(sid.to_string(), index);
                    }
                }
                let value = json!({"active":self.index(None),"surfaces":surfaces});
                drop(state);
                let effects = if self.settings.is_some() { self.persist("themeDesktops", &value) } else { Vec::new() };
                json!({"result":value,"effects":effects})
            }
            _ => return json!({"error":"app.invalidRequest"}),
        };
        let effects = self.effects();
        if !effects.is_empty() {
            if !reply["effects"].is_array() {
                reply["effects"] = json!([]);
            }
            reply["effects"].as_array_mut().unwrap().extend(effects);
        }
        reply
    }
}

#[cfg(test)]
#[path = "desktop_session_tests.rs"]
mod tests;

/// Nonvisual observer: persistence must not depend on a theme mounting its HUD.
pub struct Observer {
    service: std::rc::Rc<std::cell::RefCell<Service>>,
    mounted: bool,
}
impl Observer {
    pub fn new(service: std::rc::Rc<std::cell::RefCell<Service>>) -> Self {
        Self {
            service,
            mounted: false,
        }
    }
}
impl axia_native::NativeApp for Observer {
    fn resources(&mut self, _assets: &str, home: Option<&str>) {
        if let Some(home) = home { self.service.borrow_mut().bind_settings(home); }
    }
    fn invalidated(&self) -> bool {
        self.service.borrow().invalidated()
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] != "sync" {
            return json!({});
        }
        let mut service = self.service.borrow_mut();
        let reduced = service.settings.as_ref().map_or(input["args"]["reduceMotion"]==true,
            |(_, reader, _)| reader.read()["reduceMotion"]==true);
        service.rtl = input["context"]["direction"]=="rtl";
        let rtl = service.rtl;
        service.animation.configure(reduced,rtl);
        let effects = service.effects();
        if std::mem::replace(&mut self.mounted, true) {
            return json!({"same":true,"effects":effects});
        }
        json!({"tree":{"kind":"view","style":{"pointerEvents":"none"}},"effects":effects})
    }
}
