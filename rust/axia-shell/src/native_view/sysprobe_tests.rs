#[cfg(test)]
mod tests {
    use axia_app_sysprobe::*;
    use serde_json::json;
    use crate::{native_view::Host, runtime::native as scene};
    #[test]
    fn original_fallback_and_icon_mount_without_react() {
        let mount=scene::create(1);
        let assets=concat!(env!("CARGO_MANIFEST_DIR"),"/../../assets");
        let mut host=Host::new(mount,Box::new(App),assets,None);
        host.facts(json!({"lang":"en","context":{"theme":{"textDim":"#aabbcc"}}}));
        let mut i18n=crate::native_view::i18n::I18n::new(assets,None);
        assert_eq!(scene::gather_text(mount),i18n.caption(&json!({"key":"sysinfo.probe.inProcess"})));
        host.facts(json!({"icon":48}));
        assert!(scene::gather_text(mount).is_empty());
        let tree=icon(48.);
        assert_eq!(tree["children"].as_array().unwrap().len(),7);
        assert_eq!(tree["children"][0]["style"]["width"],24.);
        assert_eq!(tree["children"][1]["style"]["top"],48.*0.16);
        drop(host);
        scene::destroy(mount);
    }
}
