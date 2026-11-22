//! The session's Window menu reads its context directly; no row list visits JS.
use super::i18n::I18n;
use crate::winmodel::{AppCommand, Session, WindowMenuAction, WindowState};
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};

pub struct WindowMenu {
    session: Option<Session>,
    windows: Reader<WindowState>,
    changes: Subscription,
    displays: Option<(Reader<Vec<Value>>, Subscription)>,
    catalog: Option<(Reader<Value>, Subscription)>,
    anchor: Option<String>,
    i18n: I18n,
    locale_revision: Value,
}
fn caption(key: &str) -> Value {
    json!({"key":key})
}
fn item(key: &str, action: &str, id: Option<&str>) -> Value {
    json!({"label":caption(key),"disabled":id.is_none(),"action":["windowMenu",{"action":action,"id":id}]})
}
impl WindowMenu {
    pub fn new(windows: Reader<WindowState>, assets: &str, home: Option<&str>) -> Self {
        let changes = windows.subscribe();
        Self {
            session: None,
            windows,
            changes,
            displays: None,
            catalog: None,
            anchor: None,
            i18n: I18n::new(assets, home),
            locale_revision: Value::Null,
        }
    }
    pub fn with_displays(session: Session, displays: Reader<Vec<Value>>, assets: &str, home: Option<&str>) -> Self {
        let changes = displays.subscribe();
        let catalog = session.catalog_reader();
        let catalog_changes = catalog.subscribe();
        Self { session: Some(session.clone()), displays: Some((displays, changes)), catalog: Some((catalog,catalog_changes)), ..Self::new(session.reader(), assets, home) }
    }
    pub fn pending(&self) -> bool {
        self.changes.pending() || self.displays.as_ref().is_some_and(|(_, changes)| changes.pending())
            || self.catalog.as_ref().is_some_and(|(_, changes)| changes.pending())
    }
    pub fn open(&mut self) {
        self.anchor = self.windows.read().focused_window().map(str::to_owned);
    }
    pub fn effect(&mut self, action: &Value) -> Value {
        let global = matches!(action["action"].as_str(), Some("overview" | "bringAll"));
        if !global
            && !action["id"]
                .as_str()
                .is_some_and(|id| self.windows.read().window(id).is_some())
        {
            return json!({});
        }
        if action["action"] == "display" {
            if let Some((source, _)) = &self.displays {
                if action["display"].is_null() || !source.read().iter().any(|display| display["id"] == action["display"]) {
                    return json!({});
                }
            }
        }
        if let Some(session) = &mut self.session {
            let selected = match action["action"].as_str() {
                Some("minimize") => Some(WindowMenuAction::Minimize),
                Some("zoom") => Some(WindowMenuAction::Zoom),
                Some("focus") => Some(WindowMenuAction::Focus),
                Some("desktop") => match action["desktop"].as_i64() {
                    Some(desktop) => Some(WindowMenuAction::MoveDesktop(Some(desktop))),
                    None => return json!({}),
                },
                Some("newDesktop") => Some(WindowMenuAction::MoveDesktop(None)),
                _ => None,
            };
            if let Some(selected) = selected {
                let Some(id) = action["id"].as_str() else { return json!({}); };
                return session.app_command(AppCommand::WindowAction(selected), &[id])
                    .map(|reply|json!({"effects":[{"kind":"windowModel","reply":reply}]}))
                    .unwrap_or_else(|_|json!({}));
            }
        }
        json!({"effects":[{"kind":"windowMenu","command":action}]})
    }
    pub fn items(&mut self, input: &Value) -> Vec<Value> {
        self.changes.take();
        if let Some((_, changes)) = &self.displays { changes.take(); }
        if let Some((_, changes)) = &self.catalog { changes.take(); }
        if self.locale_revision != input["context"]["localeRevision"] {
            self.i18n.invalidate();
            self.locale_revision = input["context"]["localeRevision"].clone();
        }
        self.i18n.lang = input["lang"].as_str().unwrap_or("en").to_owned();
        let state = self.windows.read();
        let metadata = self.catalog.as_ref().map(|(source, _)| source.read());
        let registry = metadata.as_deref().unwrap_or(input)["registry"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let app = |id: &str| registry.iter().find(|app| app["id"].as_str() == Some(id));
        let own = |w: &crate::winmodel::Window<'_>| {
            w.id().starts_with("linux#") || !app(w.app()).is_some_and(|app| app["builtin"] == true)
        };
        let window = self
            .anchor
            .as_deref()
            .and_then(|id| state.window(id))
            .filter(own);
        let id = window.as_ref().map(|w| w.id());
        let mut items = vec![
            item("shell.window.minimize", "minimize", id),
            item("shell.window.zoom", "zoom", id),
            item("shell.window.fill", "fill", id),
            item("shell.window.center", "center", id),
            json!("-"),
        ];
        items[0]["hint"] = json!("❖M");
        let others = state
            .windows()
            .filter(|w| {
                own(w)
                    && Some(w.id()) != id
                    && !w.minimized()
                    && (w.pip() || w.desktop() == state.active_desktop())
            })
            .count();
        let mut resize = Vec::new();
        for (section, zones) in [
            ("halves", vec!["left", "right", "top", "bottom"]),
            (
                "quarters",
                vec!["top-left", "top-right", "bottom-left", "bottom-right"],
            ),
            (
                "thirds",
                vec![
                    "left-third",
                    "center-third",
                    "right-third",
                    "left-two-thirds",
                    "right-two-thirds",
                ],
            ),
        ] {
            if !resize.is_empty() {
                resize.push(json!("-"));
            }
            resize.push(json!({"label":caption(&format!("shell.window.section.{section}")),"disabled":true}));
            for zone in zones {
                resize.push(json!({"label":caption(&format!("shell.window.zone.{zone}")),"action":["windowMenu",{"action":"snap","id":id,"zone":zone}]}));
            }
        }
        resize.push(json!("-"));
        resize.push(json!({"label":caption("shell.window.section.arrange"),"disabled":true}));
        for (layout, label) in [
            ("left-right", "leftRight"),
            ("right-left", "rightLeft"),
            ("top-bottom", "topBottom"),
            ("bottom-top", "bottomTop"),
            ("quarters", "quarters"),
        ] {
            resize.push(json!({"label":caption(&format!("shell.window.arrange.{label}")),"disabled":others<if layout=="quarters"{2}else{1},
                "action":["windowMenu",{"action":"arrange","id":id,"layout":layout}]}));
        }
        resize.push(json!("-"));
        resize.push(json!({"label":caption("shell.window.previousSize"),"disabled":!window.as_ref().is_some_and(|w|w.maximized()||w.snapped().is_some()),
            "action":["windowMenu",{"action":"snap","id":id,"zone":"restore"}]}));
        items.push(json!({"label":caption("shell.window.moveResize"),"disabled":id.is_none(),"submenu":if id.is_some(){json!(resize)}else{json!([])}}));
        let physical = self.displays.as_ref().map(|(source, _)| source.read());
        let displays = if let Some(displays) = &physical { displays.as_slice() }
            else { input["args"]["displays"].as_array().map(Vec::as_slice).unwrap_or(&[]) };
        if let Some(window) = &window {
            if displays.len() > 1 {
                let frame = window.frame();
                let number = |d: &Value, k: &str| d[k].as_f64().unwrap_or(0.);
                let score = |d: &Value| {
                    let x = number(d, "x");
                    let y = number(d, "y");
                    let area = (frame.x + frame.w).min(x + number(d, "w")) - (frame.x.max(x));
                    let height = (frame.y + frame.h).min(y + number(d, "h")) - (frame.y.max(y));
                    let dx = (x - frame.x - frame.w / 2.)
                        .max(0.)
                        .max(frame.x + frame.w / 2. - x - number(d, "w"));
                    let dy = (y - frame.y - frame.h / 2.)
                        .max(0.)
                        .max(frame.y + frame.h / 2. - y - number(d, "h"));
                    if area > 0. && height > 0. {
                        area * height
                    } else {
                        -dx * dx - dy * dy
                    }
                };
                let here = displays
                    .iter()
                    .enumerate()
                    .rev()
                    .max_by(|(_, a), (_, b)| score(a).total_cmp(&score(b)))
                    .map(|(i, _)| i);
                items.push(json!("-"));
                for (i, display) in displays
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| Some(*i) != here)
                {
                    let name=self.i18n.caption(&json!({"key":"shell.window.displayN","params":{"n":i+1,"name":display["name"].as_str().unwrap_or("")}}));
                    items.push(json!({"label":{"key":"shell.window.moveTo","params":{"display":name.trim()}},
                        "action":["windowMenu",{"action":"display","id":id,"display":display["id"]}]}));
                }
            }
        }
        let mut desktops = Vec::new();
        for (i, desktop) in state
            .desktops()
            .iter()
            .enumerate()
            .filter(|(_, d)| !window.as_ref().is_some_and(|w| w.desktop() == **d))
        {
            desktops.push(json!({"label":{"key":"shell.desktops.name","params":{"n":i+1}},"action":["windowMenu",{"action":"desktop","id":id,"desktop":desktop}]}));
        }
        if !desktops.is_empty() {
            desktops.push(json!("-"));
        }
        desktops.push(json!({"label":caption("shell.desktops.new"),"disabled":state.desktops().len()>=9,"action":["windowMenu",{"action":"newDesktop","id":id}]}));
        items.extend([json!("-"),json!({"label":caption("shell.desktops.moveTo"),"disabled":id.is_none(),"submenu":if id.is_some(){json!(desktops)}else{json!([])}}),
            json!({"label":caption("shell.desktops.overview"),"action":["windowMenu",{"action":"overview"}]}),json!("-"),
            json!({"label":caption("shell.window.bringAll"),"action":["windowMenu",{"action":"bringAll","id":id}]}),json!("-")]);
        let mut close = item("shell.window.close", "close", id);
        close["hint"] = json!("Alt+F4");
        items.push(close);
        let list: Vec<_> = state.windows().filter(own).collect();
        if !list.is_empty() {
            items.push(json!("-"));
        }
        for w in list {
            let title = if w.title().is_empty() {
                app(w.app())
                    .and_then(|a| a["name"].as_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or(w.app())
            } else {
                w.title()
            };
            let mut label = title.to_owned();
            if state.desktops().len() > 1 && !w.pip() && w.desktop() != state.active_desktop() {
                let n = state
                    .desktops()
                    .iter()
                    .position(|d| *d == w.desktop())
                    .unwrap_or(0)
                    + 1;
                label.push_str(" · ");
                label.push_str(
                    &self
                        .i18n
                        .caption(&json!({"key":"shell.desktops.name","params":{"n":n}})),
                );
            }
            items.push(json!({"label":label,"checked":Some(w.id())==id,"action":["windowMenu",{"action":"focus","id":w.id()}]}));
        }
        items
    }
}
