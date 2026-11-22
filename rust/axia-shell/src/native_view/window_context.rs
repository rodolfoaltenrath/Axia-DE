//! The own-frame window menu reads rows and coordinates from the native owner.
use super::{tree::Tree, NativeApp};
use crate::winmodel::{AppCommand, Session, WindowMenuAction, WindowState};
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};

pub struct WindowContext {
    session: Session,
    windows: Reader<WindowState>,
    changes: Subscription,
    id: String,
    menu: Tree,
}
impl WindowContext {
    pub fn new(session: Session) -> Self {
        let windows = session.reader();
        let changes = windows.subscribe();
        Self { session, windows, changes, id: String::new(), menu: Tree::default() }
    }
    fn tree(&self) -> Value {
        let state = self.windows.read();
        let Some(window) = state.window(&self.id) else { return Value::Null; };
        let Some((x,y)) = window.window_menu() else { return Value::Null; };
        let item = |key: &str, action: &str| json!({"label":{"key":key},
            "action":["windowContext",{"action":action,"id":self.id}]});
        let mut desktops: Vec<_> = state.desktops().iter().enumerate()
            .filter(|(_,desktop)| **desktop != window.desktop())
            .map(|(index,desktop)| json!({"label":{"key":"shell.desktops.name","params":{"n":index+1}},
                "action":["windowContext",{"action":"desktop","id":self.id,"desktop":desktop}]})).collect();
        if !desktops.is_empty() { desktops.push(json!("-")); }
        let mut new = item("shell.desktops.new", "newDesktop");
        new["disabled"] = json!(state.desktops().len() >= crate::winmodel::desktops::MAX_DESKTOPS);
        desktops.push(new);
        let mut maximize = item("shell.window.maximize", "zoom");
        maximize["checked"] = json!(window.maximized());
        json!({"kind":"menu","key":"window-context","absolute":true,"x":x,"y":y,"layer":"popover",
            "items":[item("shell.window.minimize","minimize"),maximize,
                {"label":{"key":"shell.desktops.moveTo"},"submenu":desktops},"-",item("shell.window.close","close")],
            "events":{"onDismiss":["dismiss"]}})
    }
    fn prepare_input(&mut self) {
        if self.changes.pending() {
            self.changes.take();
            let tree = self.tree();
            self.menu.request(&json!({"cmd":"sync","args":{"tree":tree}}));
            self.menu.invalidate();
        }
    }
    fn route(&mut self, mut reply: Value) -> Value {
        if let Some(effects) = reply["effects"].as_array_mut() {
            let mut result = Vec::new();
            for effect in std::mem::take(effects) {
                if effect["kind"] != "tree" { continue; }
                let current = self.windows.read().window(&self.id).is_some_and(|window| window.window_menu().is_some());
                if !current { continue; }
                let action = &effect["action"];
                let command = &action[1];
                let selected = if action[0] == "dismiss" { WindowMenuAction::Dismiss }
                    else if action[0] == "windowContext" && command["id"] == self.id {
                        match command["action"].as_str() {
                            Some("minimize") => WindowMenuAction::Minimize,
                            Some("zoom") => WindowMenuAction::Zoom,
                            Some("desktop") => match command["desktop"].as_i64() {
                                Some(desktop) => WindowMenuAction::MoveDesktop(Some(desktop)),
                                None => continue,
                            },
                            Some("newDesktop") => WindowMenuAction::MoveDesktop(None),
                            Some("close") => WindowMenuAction::Dismiss,
                            _ => continue,
                        }
                    } else { continue; };
                if let Ok(reply) = self.session.app_command(AppCommand::WindowMenu(selected), &[&self.id]) {
                    result.push(json!({"kind":"windowModel","reply":reply}));
                    // App close guards still belong to the existing adapter.
                    if action[0] == "windowContext" && command["action"] == "close" {
                        result.push(json!({"kind":"windowContext","id":self.id,"command":command}));
                    }
                }
            }
            *effects = result;
        }
        reply
    }
}
impl NativeApp for WindowContext {
    fn uses_native_appearance(&self) -> bool { true }
    fn uses_shell_appearance(&self) -> bool { true }
    fn invalidated(&self) -> bool { self.changes.pending() }
    fn request(&mut self, input: &Value) -> Value {
        let reply = if input["cmd"] == "sync" {
            self.changes.take();
            self.id = input["args"]["id"].as_str().unwrap_or("").to_owned();
            let tree = self.tree();
            self.menu.request(&json!({"cmd":"sync","args":{"tree":tree}}))
        } else { self.prepare_input(); self.menu.request(input) };
        self.route(reply)
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        self.prepare_input();
        let reply = self.menu.key(key,chr,flags)?;
        Some(self.route(reply))
    }
}

#[cfg(test)]
#[path = "window_context_tests.rs"]
mod tests;
