//! Preference changes reach compatibility consumers without polling.
use axia_native::NativeApp;
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
pub struct Observer {
    home: String,
    reader: Reader<Value>,
    changes: Subscription,
    sent: Option<u64>,
}
impl Observer {
    pub fn new(home: &str) -> Self {
        let home = crate::settings::profile(home);
        let reader = crate::settings::with(&home, |s| s.reader());
        let changes = reader.subscribe();
        Self {
            home,
            reader,
            changes,
            sent: None,
        }
    }
}
impl NativeApp for Observer {
    fn invalidated(&self) -> bool {
        self.changes.pending()
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] != "sync" {
            return json!({});
        }
        self.changes.take();
        let revision = self.reader.revision();
        let effects = if self.sent == Some(revision) {
            json!([])
        } else {
            self.sent = Some(revision);
            let reply = crate::settings::with(&self.home, |s| s.snapshot());
            json!([{"kind":"settingsState","reply":reply}])
        };
        json!({"tree":{"kind":"view","style":{"pointerEvents":"none"}},"effects":effects})
    }
}
