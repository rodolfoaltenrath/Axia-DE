//! Single metadata source for apps compiled into the compositor; no app JSON/JSX sources.
#[path = "../apps/about/manifest.rs"]
pub mod about;
#[path = "../apps/appabout/manifest.rs"]
pub mod appabout;
#[path = "../apps/calculator/manifest.rs"]
pub mod calculator;
#[path = "../apps/characters/manifest.rs"]
pub mod characters;
#[path = "../apps/clock/manifest.rs"]
pub mod clock;
#[path = "../apps/files/manifest.rs"]
pub mod files;
#[path = "../apps/forcequit/manifest.rs"]
pub mod forcequit;
#[path = "../apps/help/manifest.rs"]
pub mod help;
#[path = "../apps/monitor/manifest.rs"]
pub mod monitor;
#[path = "../apps/store/manifest.rs"]
pub mod store;
pub fn manifests() -> serde_json::Value {
    serde_json::json!([
        about::manifest(),
        appabout::manifest(),
        calculator::manifest(),
        characters::manifest(),
        clock::manifest(),
        files::manifest(),
        forcequit::manifest(),
        help::manifest(),
        monitor::manifest(),
        store::manifest()
    ])
}
#[cfg(test)]
mod tests {
    #[test]
    fn compiled_apps_have_unique_metadata_and_native_hosts() {
        let catalog = super::manifests();
        let apps = catalog.as_array().unwrap();
        let mut ids = std::collections::BTreeSet::new();
        for app in apps {
            assert!(ids.insert(app["id"].as_str().unwrap()));
            assert!(matches!(
                app["nativeView"].as_str(),
                Some("files" | "rust")
            ));
            assert!(app["size"][0].as_u64().unwrap() >= app["minSize"][0].as_u64().unwrap());
            assert!(app["size"][1].as_u64().unwrap() >= app["minSize"][1].as_u64().unwrap());
        }
    }
    #[test]
    fn compiled_app_sources_have_no_legacy_manifest_or_jsx_entrypoint() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        for app in super::manifests().as_array().unwrap() {
            let id = app["id"].as_str().unwrap();
            let folder = root.join("apps").join(id);
            for entry in std::fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                assert_eq!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("rs"),
                    "{}",
                    path.display()
                );
            }
        }
    }
}
