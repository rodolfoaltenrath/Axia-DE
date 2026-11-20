//! Preference changes reach compatibility consumers without polling.
use axia_native::NativeApp;
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::time::Instant;
pub struct Observer {
    home: String,
    reader: Reader<Value>,
    changes: Subscription,
    sent: Option<u64>,
    locale: Reader<crate::locale::State>,
    locale_changes: Subscription,
    locale_sent: Option<u64>,
}
impl Observer {
    pub fn new(home: &str) -> Self {
        let home = crate::settings::profile(home);
        let reader = crate::settings::with(&home, |s| {
            s.rearm(Instant::now());
            s.reader()
        });
        let changes = reader.subscribe();
        let locale = crate::locale::reader(&home, &super::resource_roots().0);
        let locale_changes = locale.subscribe();
        Self {
            home,
            reader,
            changes,
            sent: None,
            locale,
            locale_changes,
            locale_sent: None,
        }
    }
}
impl NativeApp for Observer {
    fn invalidated(&self) -> bool {
        self.changes.pending() || self.locale_changes.pending()
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] != "sync" {
            return json!({});
        }
        self.changes.take();
        let revision = self.reader.revision();
        self.locale_changes.take();
        let mut effects = Vec::new();
        if self.sent != Some(revision) {
            self.sent = Some(revision);
            let reply = crate::settings::with(&self.home, |s| s.snapshot());
            effects.push(json!({"kind":"settingsState","reply":reply}));
        }
        if self.locale_sent != Some(self.locale.revision()) {
            self.locale_sent = Some(self.locale.revision());
            if let Some(reply) =
                crate::locale::snapshot_if_active(&self.home, &super::resource_roots().0)
            {
                effects.push(json!({"kind":"localeState","reply":reply}));
            }
        }
        json!({"tree":{"kind":"view","style":{"pointerEvents":"none"}},"effects":effects})
    }
}
