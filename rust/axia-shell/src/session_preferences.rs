//! Revisioned compatibility publications belong to the context, not a scene host.
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::time::Instant;
#[path = "session_appearance.rs"]
mod appearance;
pub struct Service {
    assets: String,
    home: String,
    reader: Reader<Value>,
    changes: Subscription,
    sent: Option<u64>,
    locale: Reader<crate::locale::State>,
    locale_changes: Subscription,
    locale_sent: Option<u64>,
    appearance: appearance::Service,
}
impl Service {
    pub fn new(home: &str, assets: &str) -> Self {
        let home = crate::settings::profile(home);
        let reader = crate::settings::with(&home, |s| {
            s.rearm(Instant::now());
            s.reader()
        });
        let changes = reader.subscribe();
        let locale = crate::locale::reader(&home, assets);
        let locale_changes = locale.subscribe();
        Self {
            assets: assets.into(),
            home,
            reader,
            changes,
            sent: None,
            locale,
            locale_changes,
            locale_sent: None,
            appearance: appearance::Service::default(),
        }
    }
    pub(crate) fn matches(&self, home: &str, assets: &str) -> bool {
        self.home == crate::settings::profile(home) && self.assets == assets
    }
    pub(crate) fn invalidated(&self) -> bool {
        self.changes.pending() || self.locale_changes.pending()
    }
    pub(crate) fn tick(&mut self) -> Vec<Value> {
        self.changes.take();
        let revision = self.reader.revision();
        self.locale_changes.take();
        let mut effects = Vec::new();
        if self.sent != Some(revision) {
            let plan = appearance::Plan::from_profile(&self.reader.read());
            self.appearance.sync(plan);
            self.sent = Some(revision);
            let reply = crate::settings::with(&self.home, |s| s.snapshot());
            effects.push(json!({"kind":"settingsState","reply":reply}));
        }
        if self.locale_sent != Some(self.locale.revision()) {
            if let Some(reply) =
                crate::locale::snapshot_if_active(&self.home, &self.assets)
            {
                self.locale_sent = Some(self.locale.revision());
                effects.push(json!({"kind":"localeState","reply":reply}));
            }
        }
        effects
    }
}
