//! A session view borrows locale state; app-host views keep their local facts.
use axia_native::i18n::I18n;
use axia_state::{Reader, Subscription};
use serde_json::{json, Map, Value};
pub(super) struct Source {
    reader: Reader<crate::locale::State>,
    profile: (String, String),
    changes: Subscription,
    revision: Option<u64>,
    generation: Option<u64>,
    lang: String,
    context: Map<String, Value>,
}
impl Source {
    pub fn new(home: &str, assets: &str) -> Self {
        let reader = crate::locale::reader(home, assets);
        let changes = reader.subscribe();
        Self {
            reader,
            profile: (home.into(), assets.into()),
            changes,
            revision: None,
            generation: None,
            lang: String::new(),
            context: Map::new(),
        }
    }
    pub fn pending(&self) -> bool {
        self.changes.pending()
    }
    pub fn apply(&mut self, i18n: &mut I18n, facts: &mut Value) {
        self.changes.take();
        if self.revision != Some(self.reader.revision()) {
            let (decimal, group) = crate::locale::separators(&self.profile.0, &self.profile.1);
            let locale = self.reader.read();
            if self.generation != Some(locale.revision) {
                i18n.invalidate();
            }
            self.generation = Some(locale.revision);
            self.revision = Some(self.reader.revision());
            self.lang = locale.lang.clone();
            i18n.lang = self.lang.clone();
            i18n.decimal = decimal;
            i18n.group = group;
            self.context = json!({"region":locale.region,"direction":locale.dir,
                "localeRevision":locale.revision,"decimal":i18n.decimal,"group":i18n.group})
            .as_object()
            .unwrap()
            .clone();
        }
        if !facts.is_object() {
            *facts = json!({});
        }
        facts["lang"] = json!(self.lang);
        if !facts["context"].is_object() {
            facts["context"] = json!({});
        }
        facts["context"]
            .as_object_mut()
            .unwrap()
            .extend(self.context.clone());
    }
}
