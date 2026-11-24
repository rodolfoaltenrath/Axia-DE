//! The same bindings feed the temporary SDK key transport and native policy.
use serde_json::{json, Value};
pub fn bindings() -> Value {
    json!([
        {"id":"capture.cancel","keys":["Escape"]},
        {"id":"capture.full","keys":["Meta+Shift+3"],"label":"shell.shortcuts.captureFull","group":"system"},
        {"id":"capture.selector","keys":["PrintScreen"],"label":"shell.shortcuts.captureSelector","group":"system"},
        {"id":"capture.area","keys":["Meta+Shift+4"],"label":"shell.shortcuts.captureArea","group":"system"},
        {"id":"capture.panel","keys":["Meta+Shift+5"],"label":"shell.shortcuts.capturePanel","group":"system"}
    ])
}
pub fn external(value: &Value) -> Option<String> {
    let value = value.as_str()?;
    let stem = value.strip_suffix(".desktop")?;
    if stem.is_empty()
        || !stem
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.+-".contains(&c))
    {
        return None;
    }
    if stem.strip_prefix("lordos-").is_some_and(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c))
    }) {
        return None;
    }
    Some(format!("desktop:{stem}"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chosen_desktop_ids_and_direct_selector_bindings_follow_existing_contracts() {
        for value in [
            Value::Null,
            json!(""),
            json!(".desktop"),
            json!("../../capture.desktop"),
            json!("capture desktop"),
            json!("lordos-screenshots.desktop"),
            json!("lordos-photos.desktop"),
        ] {
            assert!(external(&value).is_none());
        }
        assert_eq!(
            external(&json!("org.kde.spectacle.desktop")).as_deref(),
            Some("desktop:org.kde.spectacle")
        );
        assert_eq!(
            external(&json!("lordos-Capture.desktop")).as_deref(),
            Some("desktop:lordos-Capture")
        );
        let bindings = bindings();
        let bindings = bindings.as_array().unwrap();
        assert_eq!(
            bindings
                .iter()
                .find(|b| b["id"] == "capture.selector")
                .unwrap()["keys"],
            json!(["PrintScreen"])
        );
        assert_ne!(
            bindings.iter().find(|b| b["id"] == "capture.full").unwrap()["keys"],
            json!(["PrintScreen"])
        );
    }
}
