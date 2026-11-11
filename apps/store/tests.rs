use super::*;
use crate::applications::DesktopFacts;
use serde_json::json;
fn desktop() -> Vec<DesktopFacts> {
    vec![
        DesktopFacts {
            id: "org.gimp.GIMP".into(),
            categories: "Graphics;2DGraphics;".into(),
            icon: "/usr/share/icons/gimp.png".into(),
        },
        DesktopFacts {
            id: "code".into(),
            categories: "Development;IDE;".into(),
            icon: String::new(),
        },
    ]
}
fn store() -> App {
    App {
        desktop,
        generation: || 1,
        home: None,
        ..Default::default()
    }
}
fn catalog() -> Value {
    json!([
        {"id":"desktop:org.gimp.GIMP","name":"GIMP","comment":"Edit images","kind":"linux"},
        {"id":"desktop:code","name":"Code","genericName":"Text Editor","kind":"linux"},
        {"id":"monitor","name":"Monitor","description":"System monitor","system":true,"category":"System"},
        {"id":"notes","name":"Notas","system":true,"category":"Office"},
        {"id":"fractal","name":"Fractal","package":true,"category":"Game"}
    ])
}
/// Empty answers: no packages installed.
fn nothing(_: &Value) -> Value {
    json!({"value":[]})
}
/// A sync whose effects run like the shell runs them (in order, each answered).
fn sync_with(app: &mut App, args: Value, answer: impl Fn(&Value) -> Value) -> (Value, Vec<Value>) {
    let reply = app.request(&json!({"cmd":"sync","args":args,"catalog":catalog(),
        "context":{"theme":{"mode":"dark"},"width":1080}}));
    let effects = run(app, reply["effect"].clone(), answer);
    (reply["tree"].clone(), effects)
}
fn sync(app: &mut App) -> Value {
    sync_with(app, Value::Null, nothing).0
}
/// Runs effects in order the way the shell does, answering each with `answer`.
fn run(app: &mut App, first: Value, answer: impl Fn(&Value) -> Value) -> Vec<Value> {
    let mut seen = Vec::new();
    let mut effect = first;
    while !effect.is_null() && seen.len() < 64 {
        seen.push(effect.clone());
        effect = if effect["host"] == true {
            app.request(&json!({"cmd":"noop"}))["effect"].clone()
        } else {
            app.request(&json!({"cmd":"next","result":answer(&effect)}))["effect"].clone()
        };
    }
    seen
}
fn texts(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if let Some(Value::String(k)) = o.get("key") {
                out.push(k.clone());
            }
            if let Some(Value::String(l)) = o.get("label") {
                out.push(l.clone());
            }
            o.values().for_each(|v| texts(v, out));
        }
        Value::Array(a) => a.iter().for_each(|v| texts(v, out)),
        _ => {}
    }
}
fn has(tree: &Value, s: &str) -> bool {
    let mut out = Vec::new();
    texts(tree, &mut out);
    out.iter().any(|t| t == s)
}

#[test]
fn the_catalog_merges_desktop_categories_icons_and_sources() {
    let mut app = store();
    sync(&mut app);
    let gimp = app.entry("desktop:org.gimp.GIMP").unwrap();
    assert_eq!(
        gimp.icon,
        catalog::Icon::File("/usr/share/icons/gimp.png".into())
    );
    assert!(gimp.in_group(Group::Create));
    assert!(gimp.in_category("graphics"));
    let code = app.entry("desktop:code").unwrap();
    assert_eq!(code.summary, "Text Editor");
    assert_eq!(code.icon, catalog::Icon::Letter);
    assert!(code.in_group(Group::Develop));
    let monitor = app.entry("monitor").unwrap();
    assert_eq!(monitor.icon, catalog::Icon::Compiled("monitor".into()));
    assert_eq!(monitor.source, catalog::Source::System);
    assert_eq!(
        app.entry("fractal").unwrap().source,
        catalog::Source::Package
    );
    assert!(app.entry("fractal").unwrap().in_group(Group::Play));
    let names: Vec<_> = app.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        ["Code", "Fractal", "GIMP", "Monitor", "Notas"],
        "sorted by name"
    );
}

#[test]
fn pages_show_their_sections_and_navigation_keeps_history() {
    let mut app = store();
    let tree = sync(&mut app);
    for k in [
        "store.nav.discover",
        "store.story.welcome.title",
        "store.section.essentials",
        "store.section.create",
        "store.section.develop",
        "store.pill.open",
    ] {
        assert!(has(&tree, k), "{k}");
    }
    assert!(has(&tree, "GIMP"));
    // A sidebar page has no back link; an app page does.
    app.request(&json!({"cmd":"page","key":"play"}));
    let tree = sync(&mut app);
    assert!(has(&tree, "store.nav.play") && has(&tree, "Fractal") && !has(&tree, "GIMP"));
    assert!(!has(&tree, "store.back"));
    app.request(&json!({"cmd":"app","key":"fractal"}));
    let tree = sync(&mut app);
    assert!(has(&tree, "store.back") && has(&tree, "store.fact.source"));
    app.request(&json!({"cmd":"back"}));
    assert_eq!(app.page, Page::Group(Group::Play));
    // Unknown apps open nothing.
    app.request(&json!({"cmd":"app","key":"missing"}));
    assert_eq!(app.page, Page::Group(Group::Play));
    // Categories count their apps.
    app.request(&json!({"cmd":"page","key":"categories"}));
    let tree = sync(&mut app);
    assert!(has(&tree, "store.category.graphics"));
    app.request(&json!({"cmd":"category","key":"development"}));
    let tree = sync(&mut app);
    assert!(has(&tree, "Code") && !has(&tree, "GIMP"));
}

#[test]
fn search_follows_the_query_and_clearing_it_returns() {
    let mut app = store();
    sync(&mut app);
    app.request(&json!({"cmd":"page","key":"work"}));
    app.request(&json!({"cmd":"query","key":"edit"}));
    assert_eq!(app.page, Page::Search);
    let tree = sync(&mut app);
    assert!(has(&tree, "GIMP"), "matches the summary");
    assert!(!has(&tree, "Monitor"));
    app.request(&json!({"cmd":"query","key":"zzz"}));
    assert!(has(&sync(&mut app), "store.empty.search"));
    app.request(&json!({"cmd":"query","key":""}));
    assert_eq!(app.page, Page::Group(Group::Work));
}

#[test]
fn opening_an_app_asks_the_shell_without_waiting_for_an_answer() {
    let mut app = store();
    sync(&mut app);
    let reply = app.request(&json!({"cmd":"open","key":"desktop:org.gimp.GIMP"}));
    // The first sync read the packages; the launch follows it.
    let effects = run(&mut app, reply["effect"].clone(), |_| json!({"value":[]}));
    assert!(effects.iter().any(|e| e["kind"] == "launchApp"
        && e["app"] == "desktop:org.gimp.GIMP"
        && e["host"] == true));
    assert!(app.request(&json!({"cmd":"open","key":"desktop:nope"}))["effect"].is_null());
}

#[test]
fn packages_and_their_signatures_are_read_then_managed() {
    let mut app = store();
    sync(&mut app);
    let reply = app.request(&json!({"cmd":"page","key":"installed"}));
    let effects = run(&mut app, reply["effect"].clone(), |e| {
        match (e["service"].as_str(), e["method"].as_str()) {
            (Some("apps"), Some("packages")) => json!({"method":"packages","value":[
            {"id":"fractal","dir":"/x","manifest":"{\"name\":\"Fractal\",\"version\":\"1.0.0\"}"}]}),
            (Some("sign"), Some("status")) => json!({"method":"status","value":
            {"installed":true,"hasOriginal":true,"modified":true,"version":"1.0.0","trust":"verified","signer":"Ana","changedCount":2}}),
            _ => json!({}),
        }
    });
    assert!(effects
        .iter()
        .any(|e| e["method"] == "status" && e["args"] == json!(["fractal"])));
    assert_eq!(
        app.packages[0]["manifest"]["version"], "1.0.0",
        "manifest text is parsed"
    );
    let tree = sync(&mut app);
    for k in [
        "store.section.packages",
        "store.manage.modified",
        "store.manage.restore",
        "store.trust.verified",
    ] {
        assert!(has(&tree, k), "{k}");
    }
    // Removing calls the shell and, once done, refreshes its list and reads again.
    let reply = app.request(&json!({"cmd":"remove","key":"fractal"}));
    let effects = run(&mut app, reply["effect"].clone(), |e| {
        if e["method"] == "removePackage" {
            json!(true)
        } else {
            json!({"value":[]})
        }
    });
    assert_eq!(effects[0]["method"], "removePackage");
    assert!(effects.iter().any(|e| e["kind"] == "refreshPackages"));
    assert_eq!(app.message.as_ref().unwrap().0, true);
    assert!(app.packages.is_empty());
    // Only listed packages can be removed.
    assert!(app.request(&json!({"cmd":"remove","key":"other"}))["effect"].is_null());
}

#[test]
fn installing_a_file_inspects_it_first_and_reports_errors() {
    let mut app = store();
    sync(&mut app);
    let (_, effects) = sync_with(
        &mut app,
        json!({"install":"/home/a/Downloads/x.lpk"}),
        |e| match e["method"].as_str() {
            Some("inspectPackage") => json!({"method":"inspectPackage","value":{
                "manifest":{"id":"x","name":"X","version":"2.0.0","permissions":["network"]},
                "files":3,"size":2048,"signature":{"trust":"unsigned"}}}),
            _ => json!({"value":[]}),
        },
    );
    assert_eq!(app.page, Page::File("/home/a/Downloads/x.lpk".into()));
    assert!(effects.iter().any(|e| e["method"] == "inspectPackage"));
    let tree = sync(&mut app);
    assert!(has(&tree, "store.file.permissions") && has(&tree, "store.install"));
    // Install only the file on screen; a failure keeps the page with the shell's error.
    assert!(app.request(&json!({"cmd":"install","key":"/other.lpk"}))["effect"].is_null());
    let reply = app.request(&json!({"cmd":"install","key":"/home/a/Downloads/x.lpk"}));
    run(&mut app, reply["effect"].clone(), |e| {
        if e["method"] == "installPackage" {
            json!({"error":"lpk.badSignature"})
        } else {
            json!({"value":[]})
        }
    });
    assert_eq!(
        app.message,
        Some((false, json!({"error":"lpk.badSignature"})))
    );
    assert_eq!(app.page, Page::File("/home/a/Downloads/x.lpk".into()));
}

#[test]
fn glyphs_and_artwork_are_stable_and_antialiased() {
    for g in art::Glyph::NAV.into_iter().chain([art::Glyph::Search]) {
        let img = art::glyph(g, 36, [10, 132, 255, 255]);
        let alpha: Vec<u8> = img.data.chunks_exact(4).map(|p| p[3]).collect();
        assert_eq!(alpha[0], 0, "{g:?}");
        assert!(alpha.iter().any(|&a| a == 255), "{g:?}");
        assert!(alpha.iter().any(|&a| a > 0 && a < 255), "{g:?}");
    }
    let a = art::hero(art::hash("welcome"), 120, 68);
    assert_eq!(
        a,
        art::hero(art::hash("welcome"), 120, 68),
        "same story, same art"
    );
    assert_ne!(a, art::hero(art::hash("monitor"), 120, 68));
    assert!(
        a.data.chunks_exact(4).all(|p| p[3] == 255),
        "opaque card art"
    );
    let (light, dark) = art::letter_colors("Code");
    assert!(light.starts_with('#') && light.len() == 7 && light != dark);
}

#[test]
fn rasters_are_kept_and_served_by_id() {
    use crate::native_view::NativeApp;
    let mut app = store();
    let tree = sync(&mut app).to_string();
    let id = app
        .images
        .keys()
        .find(|k| k.starts_with("hero-"))
        .cloned()
        .unwrap();
    assert!(tree.contains(&id));
    let (w, h, data) = NativeApp::pixels(&app, &id).unwrap();
    assert_eq!(data.len(), w * h * 4);
    let before = app.images.len();
    sync(&mut app);
    assert_eq!(app.images.len(), before, "nothing is redrawn");
}

#[test]
fn localized_package_strings_follow_the_session_language() {
    let v = json!({"en":"Paint","pt":"Pintura","pt-BR":"Pintura BR"});
    assert_eq!(catalog_text(&v, "pt-BR").as_deref(), Some("Pintura BR"));
    assert_eq!(catalog_text(&v, "pt_PT").as_deref(), Some("Pintura"), "base language");
    assert_eq!(catalog_text(&v, "fr").as_deref(), Some("Paint"), "English fallback");
    assert_eq!(catalog_text(&json!(""), "en"), None);
    let mut app = store();
    app.request(&json!({"cmd":"sync","lang":"en","context":{},
        "catalog":[{"id":"x","name":{"en":"Paint","pt":"Pintura"}}]}));
    assert_eq!(app.entry("x").unwrap().name, "Paint");
    app.request(&json!({"cmd":"sync","lang":"pt","context":{},
        "catalog":[{"id":"x","name":{"en":"Paint","pt":"Pintura"}}]}));
    assert_eq!(app.entry("x").unwrap().name, "Pintura", "a new language rebuilds the list");
}

#[test]
fn the_list_is_rebuilt_only_when_the_catalog_or_desktop_files_change() {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    static READS: AtomicUsize = AtomicUsize::new(0);
    static GENERATION: AtomicU64 = AtomicU64::new(1);
    let mut app = App {
        desktop: || {
            READS.fetch_add(1, Ordering::SeqCst);
            desktop()
        },
        generation: || GENERATION.load(Ordering::SeqCst),
        home: None,
        ..Default::default()
    };
    sync(&mut app);
    app.request(&json!({"cmd":"page","key":"play"}));
    sync(&mut app);
    sync(&mut app);
    assert_eq!(READS.load(Ordering::SeqCst), 1, "clicks and answers reuse the list");
    GENERATION.store(2, Ordering::SeqCst);
    sync(&mut app);
    assert_eq!(READS.load(Ordering::SeqCst), 2, "new .desktop files are read");
}
