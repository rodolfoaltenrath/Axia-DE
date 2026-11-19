//! Desktop persistence and theme hand-off belong to the context's window owner.
use crate::winmodel::{AppCommand, Session, WindowState};
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub struct Service {
    windows: Reader<WindowState>,
    changes: Subscription,
    started: bool,
    saved_count: Option<usize>,
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
            changes: windows.subscribe(),
            windows,
            started: false,
            saved_count: None,
            surfaces: BTreeMap::new(),
        }
    }
    pub fn invalidated(&self) -> bool {
        self.changes.pending()
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
        if !self.started {
            return Vec::new();
        }
        let count = self.windows.read().desktops().len();
        if self.saved_count == Some(count) {
            return Vec::new();
        }
        self.saved_count = Some(count);
        vec![json!({"kind":"settings","patch":{"desktopCount":count}})]
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
                let settings = &input["settings"];
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
                if !was.is_null() {
                    reply["effects"] = json!([{"kind":"settings","patch":{"themeDesktops":null}}]);
                }
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
                json!({"result":{"active":self.index(None),"surfaces":surfaces}})
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
    fn invalidated(&self) -> bool {
        self.service.borrow().invalidated()
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] != "sync" {
            return json!({});
        }
        let effects = self.service.borrow_mut().effects();
        if std::mem::replace(&mut self.mounted, true) {
            return json!({"same":true,"effects":effects});
        }
        json!({"tree":{"kind":"view","style":{"pointerEvents":"none"}},"effects":effects})
    }
}
