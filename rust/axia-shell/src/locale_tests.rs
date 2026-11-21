use super::*;
use serde_json::json;

#[test]
fn sdk_translation_uses_current_owner_shared_catalogs_and_fractional_plurals() {
    let root = std::env::temp_dir().join(format!("axia-locale-translate-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let assets = root.join("assets");
    let home = crate::settings::profile(root.to_str().unwrap());
    let assets = assets.to_str().unwrap();
    let pack = root.join(".local/share/axia/locales");
    for (lang, rule) in [
        ("en", "one-other"),
        ("pt", "zero-one-other"),
        ("ar", "arabic"),
        ("ru", "east-slavic"),
        ("pl", "polish"),
        ("cs", "czech-slovak"),
        ("ja", "none"),
    ] {
        let folder = pack.join(lang);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("meta.json"), json!({"plural":rule}).to_string()).unwrap();
        std::fs::write(folder.join("test.json"), json!({
            "items":{"zero":"zero:{count}","one":"one:{count}","two":"two:{count}","few":"few:{count}","many":"many:{count}","other":"other:{count}"},
            "greeting":format!("{lang}:{{name}}/{{absent}}")
        }).to_string()).unwrap();
    }
    std::fs::write(pack.join("en/apps.json"), r#"{"sample":"Native app"}"#).unwrap();
    std::fs::write(pack.join("en/fallback.json"), r#"{"hello":"Hello {name}"}"#).unwrap();
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.set("language", &json!("en"));
    });
    request(&home, assets, &json!({"cmd":"initialize"}));
    for (lang, count, expected) in [
        ("en", "1", "one"),
        ("en", "1.5", "other"),
        ("pt", "0", "one"),
        ("pt", "0.5", "other"),
        ("ar", "0", "zero"),
        ("ar", "2", "two"),
        ("ar", "2.5", "other"),
        ("ar", "7", "few"),
        ("ar", "11", "many"),
        ("ru", "21", "one"),
        ("ru", "22", "few"),
        ("ru", "12", "many"),
        ("pl", "21", "many"),
        ("cs", "2.5", "few"),
        ("ja", "1", "other"),
        ("ar", "NaN", "other"),
        ("ar", "Infinity", "other"),
    ] {
        crate::settings::with(&home, |s| {
            s.set("language", &json!(lang));
        });
        let reply = request(
            &home,
            assets,
            &json!({"cmd":"translate","key":"test.items","count":count,"vars":{"count":"01"}}),
        );
        assert_eq!(reply["value"], format!("{expected}:01"), "{lang}/{count}");
    }
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"translate","key":"fallback.hello","vars":{"name":"[object Object]"}})
        )["value"],
        "Hello [object Object]"
    );
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"translate","key":"missing.{name}","vars":{"name":"Ada"}})
        )["value"],
        "missing.Ada"
    );
    assert!(request(
        &home,
        assets,
        &json!({"cmd":"optional","key":"missing.key"})
    )["value"]
        .is_null());
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"appName","key":"apps.sample","fallback":"Manifest"})
        )["value"],
        "Native app"
    );
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"appName","key":"apps.unknown","fallback":"Manifest"})
        )["value"],
        "Manifest"
    );
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"translate","key":"../outside.secret"})
        )["value"],
        "../outside.secret"
    );
    let before = request(&home, assets, &json!({"cmd":"snapshot"}));
    request(
        &home,
        assets,
        &json!({"cmd":"translate","key":"test.greeting","vars":{"name":"Ada"}}),
    );
    assert_eq!(request(&home, assets, &json!({"cmd":"snapshot"})), before);
    std::fs::write(
        pack.join("ar/test.json"),
        r#"{"greeting":"Reloaded {name}"}"#,
    )
    .unwrap();
    request(&home, assets, &json!({"cmd":"reload"}));
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"optional","key":"test.greeting","vars":{"name":"Ada"}})
        )["value"],
        "Reloaded Ada"
    );
    std::fs::remove_dir_all(root).unwrap();
}

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

#[test]
fn sdk_formats_read_current_native_preferences_and_reload_without_mutating_state() {
    let root = std::env::temp_dir().join(format!("axia-locale-formats-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    let home = crate::settings::profile(root.to_str().unwrap());
    crate::settings::with(&home, |s| {
        s.command(&json!({"cmd":"initialize"}));
        s.set("language", &json!("pt"));
        s.set("region", &json!("en_US"));
    });
    request(&home, assets, &json!({"cmd":"initialize"}));
    let date = |style: &str| json!({"cmd":"date","year":2026,"month":10,"day":13,"weekday":5,"style":style});
    let time = json!({"cmd":"time","hour":13,"minute":7,"second":9,"seconds":true});
    assert_eq!(
        request(&home, assets, &date("long"))["value"],
        "sexta-feira, 13 de novembro de 2026"
    );
    assert_eq!(
        request(&home, assets, &date("numeric"))["value"],
        "11/13/2026"
    );
    assert_eq!(request(&home, assets, &time)["value"], "1:07:09 PM");
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"number","value":"1234.5","digits":"2"})
        )["value"],
        "1,234.50"
    );
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"date","year":2026,"month":2147483647,"day":1,"weekday":0})
        )["error"],
        "locale.invalidDate"
    );
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"time","hour":24,"minute":0,"second":0})
        )["error"],
        "locale.invalidDate"
    );
    let before = request(&home, assets, &json!({"cmd":"snapshot"}));
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"month","index":10,"short":false})
        )["value"],
        "novembro"
    );
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"weekday","index":5,"short":true})
        )["value"],
        "sex"
    );
    assert_eq!(request(&home, assets, &json!({"cmd":"snapshot"})), before);
    crate::settings::with(&home, |s| {
        s.set("region", &json!("pt_PT"));
        s.set("clock", &json!({"hour24":"24"}));
    });
    assert_eq!(request(&home, assets, &time)["value"], "13:07:09");
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"number","value":"1234567.5","digits":"2"})
        )["value"],
        "1 234 567,50"
    );
    let pack = root.join(".local/share/axia/locales/pt");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("format.json"),r#"{"months":["{day}"],"date":{"short":"{month}/{day}"},"decimal":"!","group":"","hour12":true,"regions":{"pt_PT":{"pm":"tarde"}}}"#).unwrap();
    request(&home, assets, &json!({"cmd":"reload"}));
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"date","year":2026,"month":0,"day":13,"weekday":5,"style":"short"})
        )["value"],
        "{day}/13"
    );
    assert_eq!(
        request(
            &home,
            assets,
            &json!({"cmd":"number","value":"1234.5","digits":"2"})
        )["value"],
        "1234!50"
    );
    crate::settings::with(&home, |s| {
        s.set("clock", &json!({"hour24":"12"}));
    });
    assert_eq!(request(&home, assets, &time)["value"], "1:07:09 tarde");
    request(
        &home,
        assets,
        &json!({"cmd":"patch","value":{"lang":"unknown","region":"en_GB"}}),
    );
    assert_eq!(
        request(&home, assets, &date("long"))["value"],
        "Friday, November 13, 2026"
    );
    assert_eq!(
        request(&home, assets, &date("numeric"))["value"],
        "13/11/2026"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn language_choices_preserve_bundled_precedence_skip_bad_metadata_and_reload() {
    let root = std::env::temp_dir().join(format!("axia-locale-languages-{}", std::process::id()));
    let assets = root.join("assets");
    let user = root.join(".local/share/axia/locales");
    for (base, id, meta) in [
        (
            assets.join("locales"),
            "en",
            r#"{"name":"English","plural":"one-other","region":"en_US"}"#,
        ),
        (user.clone(), "en", r#"{"name":"User duplicate"}"#),
        (
            user.clone(),
            "zz",
            r#"{"name":"Zed","english":"Zed language"}"#,
        ),
        (user.clone(), "broken", "not JSON"),
        (user.clone(), "array", "[]"),
        (
            user.clone(),
            "huge",
            &format!("{{\"name\":\"{}\"}}", "x".repeat(4096)),
        ),
    ] {
        std::fs::create_dir_all(base.join(id)).unwrap();
        std::fs::write(base.join(id).join("meta.json"), meta).unwrap();
    }
    let home = crate::settings::profile(root.to_str().unwrap());
    let assets = assets.to_str().unwrap();
    let list = || request(&home, assets, &json!({"cmd":"languages"}))["value"].clone();
    let choices = list();
    assert_eq!(choices.as_array().unwrap().len(), 2);
    assert_eq!(
        choices[0],
        json!({"id":"en","name":"English","english":"","plural":"one-other","region":"en_US","user":false})
    );
    assert_eq!(choices[1]["user"], true);
    std::fs::create_dir_all(user.join("new")).unwrap();
    std::fs::write(user.join("new/meta.json"), r#"{"name":"A new language"}"#).unwrap();
    assert_eq!(
        list(),
        choices,
        "choices are cached until explicit catalog invalidation"
    );
    request(&home, assets, &json!({"cmd":"reload"}));
    assert_eq!(list()[0]["id"], "new");
    std::fs::remove_file(user.join("new/meta.json")).unwrap();
    request(&home, assets, &json!({"cmd":"reload"}));
    assert_eq!(list(), choices);
    for (id, name) in [
        ("bmp", "\u{e000}"),
        ("astral", "\u{10000}"),
        ("decomposed", "e\u{301}"),
        ("zebra", "zebra"),
    ] {
        std::fs::create_dir_all(user.join(id)).unwrap();
        std::fs::write(
            user.join(id).join("meta.json"),
            json!({"name":name}).to_string(),
        )
        .unwrap();
    }
    request(&home, assets, &json!({"cmd":"reload"}));
    let ids: Vec<_> = list()
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["en", "zz", "zebra", "decomposed", "bmp", "astral"]);
    std::fs::remove_dir_all(root).unwrap();
}
