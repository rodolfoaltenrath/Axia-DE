use super::*;
use axia_native::NativeApp;
use std::{cell::RefCell, rc::Rc};

pub struct View {
    session: Session,
    service: Rc<RefCell<Service>>,
    seen: u64,
    last: Value,
}
impl View {
    pub fn new(session: Session) -> Self {
        let service = session.restore_service();
        Self {
            session,
            service,
            seen: 0,
            last: Value::Null,
        }
    }
    fn tree(service: &Service, input: &Value) -> Value {
        let Some(question) = &service.question else {
            return json!({"kind":"view","key":"restore","style":{"pointerEvents":"none"}});
        };
        let theme = &input["context"]["theme"];
        let display = &input["args"]["display"];
        let mut names = Vec::new();
        for entry in &question.list {
            let Some((kind, key)) = crate::launcher::dock_model::parse_id(&entry["id"]) else {
                continue;
            };
            let app = if kind == "desktop" {
                rows(&service.metadata["catalog"])
                    .iter()
                    .find(|a| a["desktopId"] == key)
            } else {
                rows(&service.metadata["registry"])
                    .iter()
                    .find(|a| a["id"] == key)
            };
            let name = app.and_then(|a| {
                a[if a["kind"] == "linux" {
                    "shortName"
                } else {
                    "name"
                }]
                .as_str()
            });
            if let Some(name) = name.filter(|n| !n.is_empty()) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        let text = |label: Value| json!({"kind":"text","label":label,"style":{"fontSize":13,"color":theme["textDim"],"lineHeight":1.4,"textAlign":"start"}});
        let mut children = vec![
            json!({"kind":"text","label":{"key":"shell.restore.title"},"style":{"fontSize":17,"fontWeight":600,"color":theme["text"],"textAlign":"start"}}),
        ];
        if question.unclean {
            children.push(text(json!({"key":"shell.restore.unclean"})));
        }
        if !names.is_empty() {
            children.push(text(
                json!({"key":"shell.restore.apps","params":{"list":names.join(", ")}}),
            ));
        }
        let buttons:Vec<_>=[false,true].iter().map(|yes|json!({"kind":"view","key":if *yes{"yes"}else{"no"},
            "events":{"onClick":["answer",yes]},"style":{"height":30,"paddingHorizontal":14,"borderRadius":theme["controlRadius"].as_f64().unwrap_or(6.),
                "justifyContent":"center","alignItems":"center","backgroundColor":if *yes{&theme["accent"]}else{&theme["control"]}},
            "children":[{"kind":"text","label":{"key":if *yes{"shell.restore.yes"}else{"shell.restore.no"}},
                "style":{"fontSize":13,"fontWeight":500,"color":if *yes{json!("#fff")}else{theme["text"].clone()},"whiteSpace":"nowrap"}}]})).collect();
        children.push(json!({"kind":"view","style":{"flexDirection":"row","justifyContent":"flex-end","gap":8,"marginTop":12},"children":buttons}));
        json!({"kind":"view","key":"restore","style":{"position":"absolute","left":display["x"],"top":display["y"],"width":display["w"],"height":display["h"],
            "backgroundColor":"rgba(0,0,0,0.3)","alignItems":"center","justifyContent":"center"},"children":[{
            "kind":"view","key":"question","style":{"width":400_f64.min(display["w"].as_f64().unwrap_or(640.)-24.),"padding":24,"gap":8,
                "borderRadius":theme["groupRadius"].as_f64().unwrap_or(10.)+2.,"backgroundColor":theme["windowBg"],
                "border":format!("1px solid {}",theme["separator"].as_str().unwrap_or("#777")),"boxShadow":"0 14px 36px rgba(0,0,0,0.45)"},"children":children}]})
    }
}
impl NativeApp for View {
    fn invalidated(&self) -> bool {
        let state = self.service.borrow();
        state.changes.pending() || self.seen != state.revision || !state.effects.is_empty()
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.service
            .borrow()
            .deadline()
            .map(|d| d.saturating_duration_since(Instant::now()).as_secs_f64() * 1000. + 0.01)
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        self.refresh_ms()
    }
    fn key(&mut self, key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        if self.service.borrow().question.is_none() {
            return None;
        }
        match key {
            "Escape" | "Enter" => Some(self.request(&json!({"cmd":"answer","key":key=="Enter"}))),
            _ => Some(json!({})),
        }
    }
    fn uses_preferences(&self) -> bool { true }
    fn sync_with_preferences(&mut self, input: &Value, preferences: &Value) -> Value {
        self.request_with_preferences(input, preferences)
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_with_preferences(input, &input["settings"])
    }
}
impl View {
    fn request_with_preferences(&mut self, input: &Value, preferences: &Value) -> Value {
        let mut state = self.service.borrow_mut();
        let now = Instant::now();
        match input["cmd"].as_str() {
            Some("sync") => {
                state.configure_preferences(input, input, preferences, now);
                state.tick(&mut self.session, now);
                let tree = Self::tree(&state, input);
                let effects = std::mem::take(&mut state.effects);
                self.seen = state.revision;
                if self.last == tree {
                    return json!({"same":true,"effects":effects});
                }
                self.last = tree.clone();
                json!({"tree":tree,"effects":effects})
            }
            Some("answer") => {
                state.answer(&mut self.session, input["key"] == true, now);
                json!({"effects":std::mem::take(&mut state.effects)})
            }
            _ => json!({}),
        }
    }
}
