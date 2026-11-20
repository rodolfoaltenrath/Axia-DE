//! Notification defaults and per-app overrides are resolved in the native owner.
use serde_json::{json, Value};

fn merge(mut base: Value, patch: &Value) -> Value {
    if let Some(fields) = patch.as_object() {
        for (key, value) in fields {
            base[key] = value.clone();
        }
    }
    base
}
pub(super) fn normalize(raw: &Value) -> Value {
    merge(
        json!({"dnd":false,"schedule":{"on":false,"from":1320,"to":420},
        "grouping":"app","apps":{},"seen":[]}),
        raw,
    )
}
pub(super) fn app(raw: &Value, id: &str, permission: &Value) -> Value {
    let mut result = merge(
        json!({"allow":true,"style":"banner","sound":true,"lock":true}),
        &raw["apps"][id],
    );
    // Package permission comes from the host permission API. Built-ins use preferences.
    if let Some(permission) = permission.as_str() {
        result["allow"] = json!(permission != "deny");
    }
    result
}
pub(super) fn patch(raw: &Value, id: &str, patch: &Value) -> Value {
    let mut prefs = normalize(raw);
    if !prefs["apps"].is_object() {
        prefs["apps"] = json!({});
    }
    let current = merge(json!({}), &prefs["apps"][id]);
    prefs["apps"][id] = merge(current, patch);
    prefs
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_shallow_overrides_preserve_unknown_preferences() {
        let defaults = normalize(&Value::Null);
        assert_eq!(
            defaults["schedule"],
            json!({"on":false,"from":1320,"to":420})
        );
        assert_eq!(
            app(&defaults, "clock", &Value::Null),
            json!({"allow":true,"style":"banner","sound":true,"lock":true})
        );
        let prefs = normalize(
            &json!({"schedule":{"on":true},"extra":3,"apps":{"clock":{"sound":false,"future":"value"}}}),
        );
        assert_eq!(prefs["schedule"], json!({"on":true}));
        assert_eq!(prefs["extra"], 3);
        assert_eq!(app(&prefs, "clock", &Value::Null)["future"], "value");
        assert_eq!(app(&prefs, "clock", &Value::Null)["sound"], false);
    }
    #[test]
    fn package_permission_overrides_stored_allow_without_changing_other_options() {
        let prefs = json!({"apps":{"package":{"allow":false,"style":"alert","sound":false}}});
        for permission in ["ask", "allow"] {
            assert_eq!(app(&prefs, "package", &json!(permission))["allow"], true);
        }
        assert_eq!(app(&prefs, "package", &json!("deny"))["allow"], false);
        assert_eq!(app(&prefs, "package", &Value::Null)["allow"], false);
        assert_eq!(app(&prefs, "package", &json!("allow"))["sound"], false);
    }
    #[test]
    fn updating_one_app_keeps_other_apps_and_unrelated_fields() {
        let prefs = json!({"apps":{"clock":{"sound":false,"custom":2},"notes":{"allow":false}},"extra":7,"seen":["notes"]});
        let updated = patch(&prefs, "clock", &json!({"style":"none"}));
        assert_eq!(
            updated["apps"]["clock"],
            json!({"sound":false,"custom":2,"style":"none"})
        );
        assert_eq!(updated["apps"]["notes"], prefs["apps"]["notes"]);
        assert_eq!(updated["seen"], prefs["seen"]);
        assert_eq!(updated["extra"], 7);
        assert_eq!(prefs["apps"]["clock"].get("style"), None);
    }
}
