use axia_native::i18n::I18n;
use serde_json::{json, Value};
pub fn refusal(i18n: &mut I18n, error: &Value) -> Value {
    let body = if error.is_null() {
        i18n.caption(&json!({"key":"errors.media.denied.screen","params":{"app":{"key":"apps.screenshots"}}}))
    } else if let Some(code) = error["code"].as_str() {
        let mut params = error["params"].as_object().cloned().unwrap_or_default();
        if let Some(errno) = params.get("errno").and_then(Value::as_str) {
            let key = format!("errors.errno.{errno}");
            let translated = i18n.t(&key, &Default::default());
            if translated != key {
                params.insert("errno".into(), json!(translated));
            }
        }
        if let Some(reason) = params.get("reason").and_then(Value::as_str) {
            let text = i18n.error(reason);
            params.insert("reason".into(), json!(text));
        }
        let key = format!("errors.{code}");
        let translated = i18n.t(&key, &params);
        if translated != key {
            translated
        } else {
            i18n.error(error["message"].as_str().unwrap_or(code))
        }
    } else {
        i18n.error(
            error
                .as_str()
                .or_else(|| error["message"].as_str())
                .unwrap_or(""),
        )
    };
    json!({"app":"screenshots","label":i18n.t("apps.screenshots",&Default::default()),"title":i18n.t("screenshots.refused",&Default::default()),"body":body,"sound":true,"timeout":6000,"actions":[{"id":"privacy","label":i18n.t("screenshots.refused.open",&Default::default())}]})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refusal_captions_share_native_locale_and_structured_error_parameters() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("assets");
        let mut i18n = I18n::new(assets.to_str().unwrap(), None);
        i18n.lang = "pt".into();
        let default = refusal(&mut i18n, &Value::Null);
        assert_eq!(default["actions"][0]["id"], "privacy");
        assert!(!default["body"].as_str().unwrap().starts_with("errors."));
        assert_ne!(default["title"], "Couldn't capture the screen");
        i18n.lang = "en".into();
        let message = refusal(
            &mut i18n,
            &json!({"code":"media.denied.screen","params":{"app":"Capture test"},"message":"ignored translation"}),
        );
        assert!(message["body"].as_str().unwrap().contains("Capture test"));
        assert_eq!(
            refusal(
                &mut i18n,
                &json!({"code":"unknown.code","message":"Backend diagnostic"})
            )["body"],
            "Backend diagnostic"
        );
    }
}
