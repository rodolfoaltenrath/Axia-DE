//! App selection and scene read the context's window model directly.
use super::NativeApp;
use crate::{
    winmodel::{Session, WindowReader, WindowState},
    AxiaSwitcher,
};
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::rc::Rc;

struct Entry {
    key: String,
    name: String,
    window: String,
    icon: Value,
    desktop: Option<usize>,
    members: Vec<String>,
}

pub struct Switcher {
    appearance: Option<std::rc::Rc<Value>>,
    surface: Value,
    windows: Reader<WindowState>,
    changes: Subscription,
    catalog: crate::forcequit::App,
    selector: AxiaSwitcher,
    entries: Vec<Entry>,
    begun: bool,
    done: bool,
    origin: String,
    nonce: Value,
    effects: Vec<Value>,
}
impl Switcher {
    pub fn new(windows: Reader<WindowState>) -> Self {
        let catalog = crate::forcequit::App::with_windows(Rc::new(WindowReader(windows.clone())));
        Self::with_catalog(windows,catalog)
    }
    pub fn with_session(session: Session) -> Self {
        let windows = session.reader();
        let catalog = crate::forcequit::App::with_window_metadata(Rc::new(WindowReader(windows.clone())),session.catalog_store());
        Self::with_catalog(windows,catalog)
    }
    fn with_catalog(windows: Reader<WindowState>, catalog: crate::forcequit::App) -> Self {
        let changes = windows.subscribe();
        Self {
            appearance: None,
            surface: Value::Null,
            windows,
            changes,
            catalog,
            selector: AxiaSwitcher::default(),
            entries: Vec::new(),
            begun: false,
            done: false,
            origin: String::new(),
            nonce: Value::Null,
            effects: Vec::new(),
        }
    }
    fn refresh(&mut self, input: &Value) {
        self.changes.take();
        if self.done {
            return;
        }
        let opening = !self.begun;
        let groups = self.catalog.native_groups(input);
        let state = self.windows.read();
        let entries: Vec<_> = groups
            .iter()
            .filter_map(|group| {
                let ids: Vec<_> = group.windows.iter().map(|w| w.id.as_str()).collect();
                let window = state.pick_window(&ids)?;
                Some(Entry {
                    key: group.key.clone(),
                    name: group.name.clone(),
                    window: window.id().into(),
                    icon: group
                        .windows
                        .iter()
                        .find(|w| w.id == window.id())
                        .unwrap()
                        .icon
                        .clone(),
                    desktop: (state.desktops().len() > 1
                        && !window.pip()
                        && window.desktop() != state.active_desktop())
                    .then(|| {
                        state
                            .desktops()
                            .iter()
                            .position(|id| *id == window.desktop())
                            .unwrap_or(0)
                            + 1
                    }),
                    members: ids.into_iter().map(str::to_owned).collect(),
                })
            })
            .collect();
        if !self.begun {
            self.origin = input["args"]["origin"].as_str().unwrap_or("").into();
            self.nonce = input["args"]["nonce"].clone();
            let focused = if self.origin.is_empty() {
                state.focused_window()
            } else {
                Some(self.origin.as_str())
            };
            let key_of = |id: &str| {
                entries
                    .iter()
                    .find(|e| e.members.iter().any(|member| member == id))
                    .map(|e| e.key.as_str())
            };
            for id in state.recent_windows().iter().rev() {
                if let Some(key) = key_of(id) {
                    self.selector.focus(key);
                }
            }
            // A fullscreen origin was minimized by the adapter to reveal the shell.
            // Its group must retain the rank it had before that temporary minimize.
            if !self.origin.is_empty() {
                if let Some(key) = key_of(&self.origin) {
                    self.selector.focus(key);
                }
            }
            let mut keys: Vec<_> = Vec::new();
            for window in state.windows().rev() {
                if let Some(key) = key_of(window.id()) {
                    if !keys.iter().any(|old| old == key) {
                        keys.push(key.to_owned());
                    }
                }
            }
            let focused = focused.and_then(key_of).unwrap_or("");
            self.selector.begin(
                &keys,
                focused,
                input["args"]["back"].as_bool().unwrap_or(false),
            );
            let mut by_key: std::collections::BTreeMap<_, _> = entries
                .into_iter()
                .map(|entry| (entry.key.clone(), entry))
                .collect();
            self.entries = keys.iter().filter_map(|key| by_key.remove(key)).collect();
            self.begun = true;
        } else {
            for entry in &mut self.entries {
                if let Some(next) = entries.iter().find(|next| next.key == entry.key) {
                    entry.name.clone_from(&next.name);
                    entry.window.clone_from(&next.window);
                    entry.icon.clone_from(&next.icon);
                    entry.desktop = next.desktop;
                    entry.members.clone_from(&next.members);
                } else {
                    self.selector.remove(&entry.key);
                }
            }
        }
        let empty = self.selector.items.is_empty();
        let missing_origin = !self.origin.is_empty() && state.window(&self.origin).is_none();
        drop(state);
        if empty || (opening && missing_origin) {
            self.finish(false);
        }
    }
    fn selected(&self) -> Option<&Entry> {
        self.selector
            .items
            .get(self.selector.selected)
            .and_then(|(_, index)| self.entries.get(*index))
    }
    fn finish(&mut self, commit: bool) {
        if self.done {
            return;
        }
        let state = self.windows.read();
        let target = commit
            .then(|| self.selected())
            .flatten()
            .filter(|entry| state.window(&entry.window).is_some())
            .map(|entry| entry.window.clone())
            .or_else(|| {
                state
                    .window(&self.origin)
                    .map(|window| window.id().to_owned())
            });
        self.done = true;
        self.effects
            .push(json!({"kind":"switcher","nonce":self.nonce,"focus":target}));
    }
    fn tree(&self, input: &Value) -> Value {
        if self.done {
            return json!({"kind":"view","style":{"pointerEvents":"none"}});
        }
        let width = input["context"]["width"].as_f64().unwrap_or(1000.);
        let per_row = (((width - 96.) / 104.).floor() as usize).max(1);
        let start = self
            .selector
            .selected
            .saturating_sub(per_row / 2)
            .min(self.selector.items.len().saturating_sub(per_row));
        let theme = self.appearance.as_deref().unwrap_or(&input["context"]["theme"]);
        let surface = if self.appearance.is_some() { &self.surface } else { &input["args"]["surface"] };
        let radius = if self.appearance.is_some() { theme["groupRadius"].as_f64().unwrap_or(10.) + 10. }
            else { input["args"]["radius"].as_f64().unwrap_or(20.) };
        let cells: Vec<_> = self.selector.items.iter().enumerate().skip(start).take(per_row).map(|(index, (key, source))| {
            let entry = &self.entries[*source];
            json!({"kind":"view","key":key,"events":{"onClick":["choose",key]},
                "style":{"width":104,"height":104,"borderRadius":16,"alignItems":"center","justifyContent":"center",
                    "backgroundColor":if index == self.selector.selected {format!("{}55",theme["accent"].as_str().unwrap_or("#3584e4"))} else {"transparent".into()}},
                "children":[super::icons::from_descriptor(&entry.icon,72.,&entry.name)]})
        }).collect();
        let mut label = Vec::new();
        if let Some(entry) = self.selected() {
            let style = json!({"fontSize":15,"fontWeight":600,"color":theme["text"]});
            label.push(json!({"kind":"text","label":entry.name,"style":style}));
            if let Some(desktop) = entry.desktop {
                label.push(json!({"kind":"text","label":" · ","style":style}));
                label.push(json!({"kind":"text","label":{"key":"shell.desktops.name","params":{"n":desktop}},"style":style}));
            }
        }
        json!({"kind":"view","style":{"flex":1,"alignItems":"center","justifyContent":"center","pointerEvents":"box-none"},"children":[
            {"kind":"view","style":[surface,{"alignItems":"center","padding":16,"borderRadius":radius,
                "maxWidth":width-48.,"boxShadow":"0 18px 48px rgba(0,0,0,0.45)"}],"children":[
                {"kind":"view","style":{"flexDirection":"row","direction":"ltr"},"children":cells},
                {"kind":"view","style":{"flexDirection":"row","marginTop":10,"justifyContent":"center"},"children":label}]}]})
    }
}
impl NativeApp for Switcher {
    fn uses_native_appearance(&self) -> bool { true }
    fn uses_shell_appearance(&self) -> bool { true }
    fn set_native_appearance(&mut self, theme: std::rc::Rc<Value>) {
        if self.appearance.as_ref().is_some_and(|old| std::rc::Rc::ptr_eq(old, &theme)) { return; }
        self.surface = crate::native_style::shell_theme::dock_surface(&theme);
        self.appearance = Some(theme);
    }
    fn invalidated(&self) -> bool {
        !self.done && (self.changes.pending() || self.catalog.invalidated())
    }
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                self.refresh(input);
                json!({"tree":self.tree(input),"effects":std::mem::take(&mut self.effects)})
            }
            "move" if !self.done => {
                self.selector
                    .step(input["key"].as_i64().unwrap_or(1) as i32);
                Value::Null
            }
            "choose" if !self.done => {
                if let Some(index) = self
                    .selector
                    .items
                    .iter()
                    .position(|(key, _)| Some(key.as_str()) == input["key"].as_str())
                {
                    self.selector.selected = index;
                    self.finish(true);
                }
                Value::Null
            }
            "commit" => {
                self.finish(true);
                Value::Null
            }
            "cancel" => {
                self.finish(false);
                Value::Null
            }
            _ => Value::Null,
        }
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::*;
    #[test]
    fn native_material_keeps_selection_and_ignores_old_surface_facts() {
        let mut session = crate::winmodel::Session::default();
        for id in ["files","notes","calculator"] {
            session.call("open", &[json!({}),json!({"app":id})]).unwrap();
        }
        let mut view = Switcher::new(session.reader());
        let input = json!({"cmd":"sync","registry":[{"id":"files","name":"Files"},{"id":"notes","name":"Notes"},{"id":"calculator","name":"Calculator"}],
            "context":{"width":640,"theme":{"text":"#ff0000"}},"args":{"nonce":7,"radius":999,"surface":{"backgroundColor":"#ff0000"}}});
        view.request(&input);
        view.request(&json!({"cmd":"move","key":1}));
        let selected = view.selected().unwrap().window.clone();
        let origin = view.origin.clone();
        for mode in ["light","dark"] { for effects in [false,true] {
            let theme = Rc::new(crate::native_style::shell_theme::tokens(&json!({"mode":mode,"effects":effects,"accent":"verde"})));
            view.set_native_appearance(theme.clone());
            let tree = view.request(&input)["tree"].clone();
            assert_eq!(tree["children"][0]["style"][0], crate::native_style::shell_theme::dock_surface(&theme));
            assert_eq!(tree["children"][0]["style"][1]["borderRadius"], 20.);
            assert_eq!(view.selected().unwrap().window, selected);
            assert_eq!(view.origin, origin);
            assert!(Rc::ptr_eq(view.appearance.as_ref().unwrap(), &theme));
            assert!(!view.done);
        } }
        view.request(&json!({"cmd":"commit"}));
        let effects = view.request(&input)["effects"].clone();
        assert_eq!(effects[0]["focus"], selected);
        assert_eq!(effects[0]["nonce"], 7);
    }
}
