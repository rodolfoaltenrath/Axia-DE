use super::*;
use serde_json::json;

#[test]
fn native_locale_session_owns_runtime_overrides_fonts_and_quiet_revisions() {
    let root = std::env::temp_dir().join(format!("axia-locale-session-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let arabic_pack = root.join(".local/share/axia/locales/ar");
    std::fs::create_dir_all(&arabic_pack).unwrap();
    std::fs::write(arabic_pack.join("meta.json"), r#"{"direction":"rtl"}"#).unwrap();
    let home = crate::settings::profile(root.to_str().unwrap());
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let assets = assets.to_str().unwrap();
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.set("language", &json!("en"));
    });
    reader(&home, assets);
    assert!(host::applied().is_empty());
    let initial = request(&home, assets, &json!({"cmd":"initialize"}));
    assert_eq!(initial["state"]["lang"], "en");
    assert!(initial["state"].get("dir").is_none());
    assert_eq!(host::applied(), vec![("en".into(), false)]);
    let arabic = request(&home, assets, &json!({"cmd":"patch","value":{"lang":"ar"}}));
    assert_eq!(arabic["direction"], "rtl");
    assert!(arabic["state"].get("dir").is_none());
    let quiet = request(&home, assets, &json!({"cmd":"patch","value":{"lang":"ar"}}));
    assert_eq!(quiet, arabic);
    assert_eq!(host::applied().last(), Some(&("ar".into(), true)));
    request(
        &home,
        assets,
        &json!({"cmd":"patch","value":{"dir":"ltr","revision":u64::MAX}}),
    );
    let before = request(&home, assets, &json!({"cmd":"snapshot"}));
    assert_eq!(before["state"]["revision"], 0);
    assert_eq!(before["direction"], "ltr");
    crate::settings::with(&home, |s| {
        s.set("language", &json!("pt"));
        s.set("region", &json!("pt_BR"));
    });
    assert!(pending());
    tick();
    assert!(!pending());
    let saved = request(&home, assets, &json!({"cmd":"snapshot"}));
    assert_eq!(saved["state"]["lang"], "pt");
    assert_eq!(saved["state"]["region"], "pt_BR");
    assert_eq!(saved["state"]["dir"], "ltr");
    let bad = request(&home, assets, &json!({"cmd":"replace","value":null}));
    assert_eq!(bad["error"], "locale.invalidValue");
    assert_eq!(request(&home, assets, &json!({"cmd":"snapshot"})), saved);
    let reloaded = request(&home, assets, &json!({"cmd":"reload"}));
    assert_eq!(reloaded["state"]["revision"], 1);
    let new_session = request(&home, assets, &json!({"cmd":"initialize"}));
    assert!(new_session["state"].get("dir").is_none());
    assert_eq!(new_session["state"]["lang"], "pt");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn only_the_active_locale_session_applies_global_text_state() {
    let root = std::env::temp_dir().join(format!("axia-locale-active-{}", std::process::id()));
    let a = root.join("a");
    let b = root.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let a = crate::settings::profile(a.to_str().unwrap());
    let b = crate::settings::profile(b.to_str().unwrap());
    for (home, lang) in [(&a, "en"), (&b, "fr")] {
        crate::settings::with(home, |s| {
            s.command(&json!({"cmd":"initialize"}));
            s.set("language", &json!(lang));
        });
        request(home, "", &json!({"cmd":"initialize"}));
    }
    let count = host::applied().len();
    crate::settings::with(&a, |s| {
        s.set("language", &json!("ar"));
    });
    tick();
    assert_eq!(host::applied().len(), count);
    assert_eq!(
        request(&b, "", &json!({"cmd":"snapshot"}))["state"]["lang"],
        "fr"
    );
    std::fs::remove_dir_all(root).unwrap();
}

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
