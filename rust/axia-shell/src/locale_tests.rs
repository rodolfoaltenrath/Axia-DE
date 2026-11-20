use super::*;
use serde_json::json;

#[test]
fn locale_conf_parser_preserves_host_precedence_and_validation() {
    assert_eq!(
        lang_value("LANG=fr_FR.UTF-8\nLANG=\"pt_BR.UTF-8\" # note\n"),
        Some("pt_BR.UTF-8".into())
    );
    for invalid in [
        " LANG=pt",
        "LANG=pt # note",
        "LANG=../../pt",
        "LANG=pt\r",
        "LANG=pt:en",
    ] {
        assert_eq!(lang_value(invalid), None, "{invalid}");
    }
    assert_eq!(lang_value("LANG=pt\0LANG=fr"), Some("pt".into()));
    assert_eq!(lang_value(&format!("LANG={}", "x".repeat(65))), None);
    assert_eq!(
        configured(
            Some("pt_BR.UTF-8".into()),
            Some("fr_FR.UTF-8".into()),
            Some("de_DE.UTF-8".into())
        ),
        Some("pt".into())
    );
    assert_eq!(
        configured(Some("".into()), Some("fr".into()), Some("de".into())),
        Some("de".into())
    );
    assert_eq!(configured(None, Some("fr".into()), Some("".into())), None);
    assert_eq!(configured(None, Some("C.UTF-8".into()), None), None);
}

#[test]
fn native_locale_tracks_profile_and_pack_changes_without_a_js_context() {
    let root = std::env::temp_dir().join(format!("axia-locale-source-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".config")).unwrap();
    let home = crate::settings::profile(root.to_str().unwrap());
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let assets = assets.to_str().unwrap();
    std::fs::write(root.join(".config/locale.conf"), "LANG=pt_BR.UTF-8\n").unwrap();
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize"}));
    });
    let state = reader(&home, assets);
    assert_eq!(state.read().lang, "pt");
    let changed = state.subscribe();
    crate::settings::with(&home, |s| {
        s.set("language", &json!("en"));
        s.set("region", &json!("en_US"));
    });
    reader(&home, assets);
    assert!(changed.take().is_some());
    assert_eq!(state.read().lang, "en");
    assert_eq!(state.read().region, "en_US");
    crate::settings::with(&home, |s| {
        s.set("accent", &json!("green"));
    });
    reader(&home, assets);
    assert!(!changed.pending());
    crate::settings::with(&home, |s| {
        s.set("language", &Value::Null);
    });
    reader(&home, assets);
    assert_eq!(state.read().lang, "pt");
    let pack = root.join(".local/share/axia/locales/xx");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("shell.json"), "{}").unwrap();
    std::fs::write(pack.join("meta.json"), r#"{"direction":"rtl"}"#).unwrap();
    std::fs::write(root.join(".config/locale.conf"), "LANG=xx_ZZ.UTF-8\n").unwrap();
    let c_home = std::ffi::CString::new(home.as_str()).unwrap();
    unsafe {
        axia_locale_changed(c_home.as_ptr(), 1);
    }
    assert_eq!(state.read().lang, "xx");
    assert_eq!(state.read().dir, "rtl");
    let revision = state.read().revision;
    std::fs::write(pack.join("meta.json"), r#"{"direction":"ltr"}"#).unwrap();
    unsafe {
        axia_locale_changed(c_home.as_ptr(), 0);
    }
    assert_eq!(state.read().dir, "ltr");
    assert_eq!(state.read().revision, revision + 1);
    std::fs::write(root.join(".config/locale.conf"), "LANG=qzx.UTF-8\n").unwrap();
    unsafe {
        axia_locale_changed(c_home.as_ptr(), 1);
    }
    assert_eq!(state.read().lang, "en");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn locale_conf_reads_are_bounded() {
    let path = std::env::temp_dir().join(format!("axia-locale-limit-{}", std::process::id()));
    std::fs::write(&path, "x".repeat(8193)).unwrap();
    assert!(read_lang(&path).is_none());
    std::fs::write(&path, "LANG=pt_BR.UTF-8\n").unwrap();
    assert_eq!(read_lang(&path), Some("pt_BR.UTF-8".into()));
    std::fs::remove_file(path).unwrap();
}
