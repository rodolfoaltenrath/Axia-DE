//! The resident clock borrows profile preferences; only isolated hosts use JS config.
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};

struct Preferences {
    home: String,
    reader: Reader<Value>,
}
impl axia_clock::Preferences for Preferences {
    fn read(&self, visit: &mut dyn FnMut(&Value)) {
        let settings = self.reader.read();
        if settings["clock"].is_object() {
            visit(&settings["clock"]);
        } else {
            visit(&json!({}));
        }
    }
    fn write(&self, config: &Value) {
        crate::settings::with(&self.home, |s| {
            s.set("clock", config);
        });
    }
}
thread_local! {
    static SOURCE: RefCell<Option<(Rc<Preferences>,Subscription)>> = const { RefCell::new(None) };
}
pub(crate) fn bind(home: &str) {
    let home = crate::settings::profile(home);
    SOURCE.with(|source| {
        let mut source = source.borrow_mut();
        if source.as_ref().is_some_and(|(p, _)| p.home == home) {
            return;
        }
        let reader = crate::settings::with(&home, |s| s.reader());
        let changes = reader.subscribe();
        let preferences = Rc::new(Preferences { home, reader });
        axia_clock::with(|engine| engine.bind_preferences(preferences.clone()));
        *source = Some((preferences, changes));
    });
}
pub(crate) fn pending() -> bool {
    SOURCE.with(|s| {
        s.borrow()
            .as_ref()
            .is_some_and(|(_, changes)| changes.pending())
    })
}
pub(crate) fn take() {
    SOURCE.with(|s| {
        if let Some((_, changes)) = s.borrow().as_ref() {
            changes.take();
        }
    });
}
#[cfg(test)]
#[path = "clock_settings_tests.rs"]
mod tests;
