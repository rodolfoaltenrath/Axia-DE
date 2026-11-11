//! Shared native app/window icons. Compiled apps supply their own geometry;
//! desktop entries supply an image path. Missing icons use the shell's existing
//! initial-on-gradient convention, without evaluating application JavaScript.
use serde_json::{json, Value};

pub fn tree(spec: &Value, facts: &Value) -> Value {
    let size = spec["size"]
        .as_f64()
        .filter(|s| s.is_finite())
        .unwrap_or(32.)
        .clamp(1., 512.);
    let window = spec["window"]
        .as_str()
        .and_then(|id| facts["windows"].as_array()?.iter().find(|w| w["id"] == id));
    let id = window
        .and_then(|w| w["iconApp"].as_str().or_else(|| w["key"].as_str()))
        .or_else(|| spec["app"].as_str())
        .unwrap_or("");
    let id = id.strip_prefix("app:").unwrap_or(id);
    let entry = facts["catalog"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(facts["registry"].as_array().into_iter().flatten())
        .find(|a| a["id"] == id);
    let path = window
        .and_then(|w| w["iconPath"].as_str())
        .or_else(|| entry.and_then(|a| a["iconPath"].as_str()))
        .unwrap_or("");
    if !path.is_empty() {
        return json!({"kind":"image","src":path,"style":{"width":size,"height":size,"objectFit":"contain"}});
    }
    if id == crate::native_apps::files::ID {
        return crate::files::native_icon(size);
    }
    if let Some(mut app) = super::app(id) {
        return app.request(&json!({"cmd":"icon","size":size}));
    }
    let name = window
        .and_then(|w| w["name"].as_str())
        .or_else(|| entry.and_then(|a| a["name"].as_str()))
        .unwrap_or(id);
    letter(name, size)
}
fn letter(name: &str, size: f64) -> Value {
    let hash = name
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let hue = (hash % 360) as f64;
    let hex = |light: f64| {
        let a = 0.55 * light.min(1. - light);
        let f = |n: f64| {
            let k = (n + hue / 30.) % 12.;
            ((light - a * (-1f64).max((k - 3.).min(9. - k).min(1.))) * 255.).round() as u8
        };
        format!("#{:02x}{:02x}{:02x}", f(0.), f(8.), f(4.))
    };
    let initial = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".into());
    json!({"kind":"view","style":{"width":size,"height":size,"borderRadius":size*0.225,
        "alignItems":"center","justifyContent":"center",
        "background":format!("linear-gradient(180deg, {}, {})",hex(0.52),hex(0.36))},
        "children":[{"kind":"text","label":initial,"style":{"fontSize":size*0.5,"fontWeight":700,"color":"#fff"}}]})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_geometry_paths_window_identity_and_unicode_fallback() {
        let facts = json!({"catalog":[{"id":"desktop:browser","iconPath":"/usr/share/icons/browser.png","name":"Browser"}],
            "windows":[{"id":"linux#1","key":"desktop:browser","name":"Browser","iconPath":"/usr/share/icons/browser.png"}]});
        let icon = tree(&json!({"window":"linux#1","size":20}), &facts);
        assert_eq!(icon["src"], "/usr/share/icons/browser.png");
        assert_eq!(icon["style"]["width"], 20.);
        let calculator = tree(&json!({"app":"calculator","size":64}), &facts);
        assert!(calculator.to_string().contains("#9be7b0"));
        let files = tree(&json!({"app":"files","size":32}), &facts);
        assert_eq!(files["kind"], "view");
        assert_eq!(files["children"][0]["kind"], "view");
        let fallback = tree(&json!({"app":"org.example.área","size":-1}), &facts);
        assert_eq!(fallback["children"][0]["label"], "Á");
        assert_eq!(fallback["style"]["width"], 1.);
        assert_eq!(
            letter("Demo", 32.)["style"]["background"],
            "linear-gradient(180deg, #41c8c6, #298e8d)"
        );
    }
}
