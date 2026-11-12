use super::*;
fn app() -> App {
    let mut app = App::default();
    app.resources(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets")
            .to_string_lossy(),
        None,
    );
    app.request(&json!({"cmd":"sync","lang":"en","settings":{},"context":{"theme":{"text":"#fff","textDim":"#aaa","accent":"#0a84ff","mode":"dark","control":"#333","controlRadius":7,"controlRadiusSmall":6,"sidebarBg":"#222","separator":"#444"}}}));
    app
}
#[test]
fn shared_unicode_data_and_skin_tones_preserve_sequences() {
    let source = data();
    assert_eq!(source.groups.len(), 9);
    let mut seen = std::collections::HashSet::new();
    for (_, group) in &source.groups {
        for ch in group {
            assert!(seen.insert(ch));
        }
    }
    assert!(seen.len() > 1800);
    assert_eq!(with_tone("✋", 3), "✋🏽");
    assert_eq!(with_tone("✌️", 3), "✌🏽");
    assert_eq!(with_tone("👩‍⚕️", 5), "👩🏿‍⚕️");
    assert_eq!(with_tone("😀", 3), "😀");
    assert_eq!(with_tone("✋", 255), "✋");
    assert_eq!(key("👩🏿‍⚕️"), "1f469_200d_2695");
}
#[test]
fn multilingual_search_folds_accents_and_keeps_unicode_order() {
    let mut app = app();
    app.query = "coração".into();
    let accented = app.list();
    app.query = "coracao".into();
    assert_eq!(app.list(), accented);
    assert!(accented.contains(&"❤️".into()));
    app.query = "cat".into();
    assert!(app.list().contains(&"🐈".into()));
    app.query = "gato".into();
    assert!(app.list().contains(&"🐈".into()));
    app.query = "smiling face".into();
    let results = app.list();
    assert!(results.contains(&"😊".into()));
    assert_eq!(app.list(), results);
    app.query = "no-emoji-matches-this-axia-query".into();
    assert!(app.list().is_empty());
    app.lang = "pt-BR".into();
    assert_eq!(app.name("❤️"), "coração vermelho");
    app.lang = "unknown".into();
    assert_eq!(app.name("❤️"), "red heart");
}
#[test]
fn tabs_blocks_picks_and_clipboard_are_rust_owned() {
    let mut app = app();
    assert!(app.request(&json!({"cmd":"copy"})).is_null());
    app.request(&json!({"cmd":"tab","key":"symbols"}));
    assert_eq!(app.list().len(), 0x21ff - 0x2190 + 1);
    app.request(&json!({"cmd":"pick","key":"←"}));
    assert_eq!(app.pick, "←");
    assert_eq!(
        app.request(&json!({"cmd":"copy"}))["effect"],
        json!({"host":true,"kind":"copy","text":"←"})
    );
    assert!(app.recent.is_empty());
    app.request(&json!({"cmd":"block","key":"999"}));
    assert_eq!(app.block, 0);
    app.request(&json!({"cmd":"pick","key":"untrusted"}));
    assert_eq!(app.pick, "←");
    for (i, (_, a, b)) in BLOCKS.iter().enumerate() {
        app.request(&json!({"cmd":"block","key":i.to_string()}));
        let list = app.list();
        assert_eq!(
            list.first().unwrap(),
            &char::from_u32(*a).unwrap().to_string()
        );
        assert_eq!(
            list.last().unwrap(),
            &char::from_u32(*b).unwrap().to_string()
        );
    }
    app.request(&json!({"cmd":"tab","key":"emoji"}));
    assert!(app.pick.is_empty());
    app.request(&json!({"cmd":"tab","key":"invalid"}));
    assert_eq!(app.tab, "emoji");
}
#[test]
fn recents_tone_persistence_and_independent_instances() {
    let mut app = App::default();
    app.request(&json!({"cmd":"sync","settings":{"emojiRecent":["✋","😀"],"emojiTone":3}}));
    assert_eq!(app.group, "recent");
    assert_eq!(app.list()[0], "✋🏽");
    let effect = app.request(&json!({"cmd":"pick","key":"✋🏽"}));
    assert_eq!(app.pick, "✋🏽");
    assert_eq!(
        effect["effect"]["patch"]["emojiRecent"],
        json!(["✋", "😀"])
    );
    assert_eq!(
        app.request(&json!({"cmd":"tone","key":"5"}))["effect"]["patch"]["emojiTone"],
        5
    );
    app.request(&json!({"cmd":"tone","key":"6"}));
    assert_eq!(app.tone, 5);
    app.request(&json!({"cmd":"sync","settings":{"emojiTone":0}}));
    assert_eq!(app.tone, 5);
    app.request(&json!({"cmd":"query","value":"missing"}));
    app.request(&json!({"cmd":"group","key":"people"}));
    assert!(app.query.is_empty());
    let mut other = App::default();
    other.request(&json!({"cmd":"sync"}));
    assert_eq!(other.tone, 0);
    assert!(other.pick.is_empty());
    assert!(other.recent.is_empty());
}
#[test]
fn native_tree_contains_tabs_preview_and_copy_action_without_app_wrappers() {
    let mut app = app();
    app.request(&json!({"cmd":"tab","key":"symbols"}));
    app.request(&json!({"cmd":"pick","key":"←"}));
    let tree = ui::tree(&mut app).to_string();
    for expected in [
        "characters.tab.emoji",
        "characters.tab.symbols",
        "characters.block.braille",
        "U+2190",
        "characters.copy",
        "\"copy\"",
    ] {
        assert!(tree.contains(expected), "{expected}");
    }
    assert!(!tree.contains("appIcon"));
    let icon = app.request(&json!({"cmd":"icon","size":64}));
    assert_eq!(icon["style"]["width"], 64.);
    assert!(icon.to_string().contains("Ω"));
}
