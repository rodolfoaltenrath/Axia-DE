//! Native notification captions and timestamps share the alarm formatter.
use axia_native::{date_format, i18n::I18n};
use serde_json::Value;

#[derive(Default)]
pub(super) struct Presentation {
    roots: Option<(String, Option<String>)>,
    pub(super) i18n: I18n,
    region: String,
    hours: String,
    revision: Value,
}
impl Presentation {
    pub(super) fn configure(
        &mut self,
        input: &Value,
        config: &Value,
        settings: Option<(&str, &Value)>,
    ) {
        let mut roots = crate::native_view::resource_roots();
        if let Some((home, _)) = settings {
            roots.1 = Some(home.into());
        }
        if self.roots.as_ref() != Some(&roots) {
            self.i18n = I18n::new(&roots.0, roots.1.as_deref());
            self.roots = Some(roots);
        }
        if let Some((home, saved)) = settings {
            let locale = crate::locale::reader(home, &self.roots.as_ref().unwrap().0);
            let locale = locale.read();
            let revision = Value::from(locale.revision);
            if self.revision != revision {
                self.i18n.invalidate();
                self.revision = revision;
            }
            self.i18n.lang = locale.lang.clone();
            self.region = locale.region.clone();
            self.hours = saved["clock"]["hour24"].as_str().unwrap_or("auto").into();
            return;
        }
        if let Some(revision) = input
            .get("localeRevision")
            .or_else(|| input.get("revision"))
        {
            if self.revision != *revision {
                self.i18n.invalidate();
                self.revision = revision.clone();
            }
        }
        if let Some(lang) = input["lang"].as_str() {
            self.i18n.lang = lang.into();
        }
        if let Some(region) = input["region"].as_str() {
            self.region = region.into();
        }
        if config.is_object() {
            self.hours = config["hour24"].as_str().unwrap_or("auto").into();
        }
    }
    pub(super) fn text(&mut self, key: &str) -> String {
        self.i18n.t(key, &Default::default())
    }
    pub(super) fn time(&mut self, wall: i64) -> String {
        date_format::time(
            &mut self.i18n,
            &self.region,
            &self.hours,
            &axia_clock::local(wall),
            false,
        )
    }
    pub(super) fn app_name(
        &mut self,
        app: &str,
        entry: Option<&Value>,
        fallback: &str,
    ) -> (String, bool) {
        let Some(entry) = entry else {
            return (fallback.into(), false);
        };
        let lang = self.i18n.lang.clone();
        let translated = self
            .i18n
            .entry(&lang, "apps", app)
            .or_else(|| self.i18n.entry("en", "apps", app));
        (
            translated
                .as_ref()
                .and_then(Value::as_str)
                .or_else(|| entry["name"].as_str())
                .unwrap_or("")
                .into(),
            true,
        )
    }
}
#[cfg(test)]
#[path = "presentation_tests.rs"]
mod tests;
