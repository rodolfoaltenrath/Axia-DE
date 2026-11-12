use super::*;
fn app() -> App {
    let mut app = App::default();
    app.resources(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets")
            .to_string_lossy(),
        None,
    );
    sync(&mut app, "en", json!({"article":"welcome"}));
    app
}
fn sync(app: &mut App, lang: &str, args: Value) -> Value {
    app.request(&json!({"cmd":"sync","lang":lang,"args":args,"registry":[
        {"id":"help","name":"Help","icon":true},{"id":"clock","name":"Clock","icon":true},
        {"id":"files","name":"Files","icon":true},{"id":"terminal","name":"Terminal","icon":true},
        {"id":"settings","name":"Settings","icon":true},{"id":"monitor","name":"Monitor","icon":true},
        {"id":"store","name":"Store","icon":true}],
        "context":{"width":860,"theme":{"text":"#fff","textDim":"#aaa","accent":"#0a84ff","mode":"dark","control":"#333","controlRadius":7,"controlRadiusSmall":6,"sidebarBg":"#222","separator":"#444","hover":"#444"}}}))
}
fn walk(v: &Value, f: &mut impl FnMut(&Value)) {
    f(v);
    if let Some(children) = v["children"].as_array() {
        for child in children {
            walk(child, f);
        }
    }
}
#[test]
fn article_links_stay_in_the_window_and_app_links_keep_their_arguments() {
    let mut a = app();
    assert!(a
        .request(&json!({"cmd":"link","key":"help:clock"}))
        .is_null());
    assert_eq!(a.article, "clock");
    assert!(!a.tour);
    let reply = a.request(&json!({"cmd":"link","key":"settings:network"}));
    assert_eq!(
        reply["effect"],
        json!({"kind":"openApp","app":"settings","args":{"section":"network"}})
    );
    assert_eq!(
        a.request(&json!({"cmd":"open","key":"clock"}))["effect"]["app"],
        "clock"
    );
    for link in ["app:missing", "https://example.org", "javascript:alert(1)"] {
        assert!(a.request(&json!({"cmd":"link","key":link})).is_null());
    }
}
#[test]
fn search_submit_cancel_locale_change_and_missing_articles_keep_their_states() {
    let mut a = app();
    a.request(&json!({"cmd":"query","value":"clock"}));
    assert!(a
        .results
        .as_ref()
        .unwrap()
        .iter()
        .any(|r| r["id"] == "clock"));
    a.request(&json!({"cmd":"submit"}));
    assert_eq!(a.article, "clock");
    a.request(&json!({"cmd":"query","key":""}));
    assert!(a.results.is_none());
    sync(&mut a, "pt", json!({"article":"clock"}));
    assert_eq!(a.body.as_ref().unwrap().meta.title, "Relógio");
    let r = sync(&mut a, "zz", json!({"article":"clock"}));
    assert!(!r["tree"].is_null());
    assert_eq!(a.body.as_ref().unwrap().meta.title, "Clock");
    a.set_article("missing");
    assert!(a.body.is_none());
    let tree = ui::tree(&mut a);
    assert!(tree.to_string().contains("help.notFound"));
    // Re-syncing the same args must not undo in-window navigation.
    sync(&mut a, "zz", json!({"article":"clock"}));
    assert_eq!(a.article, "missing");
}
#[test]
fn all_six_tour_steps_back_dots_skip_and_finish_are_rust_owned() {
    let mut a = app();
    sync(&mut a, "en", json!({"tour":true,"nonce":1}));
    assert!(a.tour);
    assert_eq!(a.step, 0);
    for step in 0..6 {
        assert_eq!(a.step, step);
        let tree = ui::tree(&mut a);
        assert!(tree
            .to_string()
            .contains(&format!("help.tour.{step}.title")));
        if step < 5 {
            a.request(&json!({"cmd":"next"}));
        }
    }
    a.request(&json!({"cmd":"back"}));
    assert_eq!(a.step, 4);
    a.request(&json!({"cmd":"step","key":"3"}));
    assert!(ui::tree(&mut a).to_string().contains("settings:network"));
    a.request(&json!({"cmd":"step","key":"5"}));
    let done = a.request(&json!({"cmd":"next"}));
    assert_eq!(done["effect"]["patch"]["welcomeShown"], true);
    assert!(!a.tour);
    assert_eq!(a.article, "welcome");
    a.request(&json!({"cmd":"tour"}));
    assert_eq!(a.step, 0);
    assert_eq!(
        a.request(&json!({"cmd":"finish"}))["effect"]["patch"]["welcomeShown"],
        true
    );
}
#[test]
fn selection_theme_reading_width_and_empty_catalog_are_preserved() {
    let mut a = app();
    a.width = 640.;
    a.theme["selection"] = json!("indicator");
    let tree = ui::tree(&mut a);
    let mut indicator = false;
    let mut column = false;
    walk(&tree, &mut |n| {
        indicator |= n["style"]["width"] == 3 && n["style"]["height"] == 12;
        column |= n["style"]["width"] == 336.;
    });
    assert!(indicator);
    assert!(column);
    let mut empty = App::default();
    let reply = sync(&mut empty, "en", Value::Null);
    assert!(reply["tree"].to_string().contains("help.unavailable"));
}
#[test]
fn every_shipped_app_article_and_tour_icon_is_a_native_tree() {
    let mut a = app();
    let list = a.list.clone();
    for meta in list {
        if meta.app.is_empty() || meta.app == "installer" {
            continue;
        }
        for size in [18., 48., 84.] {
            let icon = icons::app_icon(&meta.app, size).unwrap_or_else(|| panic!("{}", meta.app));
            assert_eq!(icon["kind"], "view");
            assert_eq!(icon["style"]["width"], size);
        }
        a.set_article(&meta.id);
        assert!(!a.document.is_empty());
    }
}
