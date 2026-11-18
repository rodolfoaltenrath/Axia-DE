#[cfg(test)]
mod tests {
    use crate::native_view::icons::*;
    use serde_json::{json, Value};
    #[test]
    fn every_help_article_uses_a_native_app_or_fallback_icon() {
        crate::native_view::install_services();
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for meta in axia_app_help::content::Content::new(assets).list("en") {
            if meta.app.is_empty() || meta.app == "installer" { continue; }
            for size in [18., 48., 84.] {
                let icon = app_icon(&meta.app, size).unwrap_or_else(|| panic!("{}", meta.app));
                assert_eq!(icon["kind"], "view");
                assert_eq!(icon["style"]["width"], size);
            }
        }
    }
    #[test]
    fn every_shipped_application_has_rust_artwork() {
        crate::native_view::install_services();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps");
        let mut ids: Vec<String> = crate::native_apps::manifests()
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap().into())
            .collect();
        for entry in std::fs::read_dir(root).unwrap().flatten() {
            if let Ok(raw) = std::fs::read_to_string(entry.path().join("manifest.json")) {
                let manifest: Value = serde_json::from_str(&raw).unwrap();
                ids.push(manifest["id"].as_str().unwrap().into());
            }
        }
        for id in ids {
            for size in [20., 64.] {
                let icon =
                    app_icon(&id, size).unwrap_or_else(|| panic!("missing native artwork: {id}"));
                assert_eq!(icon["kind"], "view");
                assert_eq!(icon["style"]["width"], size);
            }
        }
    }
    #[test]
    fn image_resources_and_fallback_letters_keep_the_sdk_contract() {
        let image = from_descriptor(&json!({"kind":"image","path":"/tmp/icon.png"}), 64., "App");
        assert_eq!(image["kind"], "image");
        assert_eq!(image["style"]["objectFit"], "contain");
        let fallback = from_descriptor(
            &json!({"kind":"letter","name":"org.example.Terminal"}),
            20.,
            "ignored",
        );
        assert_eq!(fallback["children"][0]["label"], "T");
        let (light, dark) = letter_colors("org.example.Terminal");
        assert_eq!(
            fallback["style"]["background"],
            format!("linear-gradient(180deg, {light}, {dark})")
        );
        let unknown = from_descriptor(
            &json!({"kind":"app","id":"unregistered","name":"ß"}),
            64.,
            "ignored",
        );
        assert_eq!(unknown["children"][0]["label"], "SS");
        assert_eq!(
            from_descriptor(&json!({"kind":"image","path":"\0bad"}), 20., "?")["kind"],
            "view"
        );
    }
}

#[cfg(test)]
mod tree_tests {
    use crate::native_view::icons::*;
    use serde_json::json;
    #[test]
    fn native_geometry_paths_window_identity_and_unicode_fallback() {
        crate::native_view::install_services();
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
            from_descriptor(&json!({"kind":"letter","name":"Demo"}), 32., "Demo")["style"]["background"],
            "linear-gradient(180deg, #41c8c6, #298e8d)"
        );
    }
}
