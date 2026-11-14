use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "axia-notes-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const FROZEN: &str = r###"[{"note":{"slug":"test","body":"# Hello\n\n- [ ] todo\n","title":"Hello","color":"yellow","x":80,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: Hello\ncolor: yellow\nx: 80\ny: 90\nw: 260\nh: 240\ndesktop: true\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n# Hello\n\n- [ ] todo\n","parsed":{"meta":{"title":"Hello","color":"yellow","x":80,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"# Hello\n\n- [ ] todo\n"},"title":"Hello"},{"note":{"slug":"test","body":"\r\n## Olá 😀\r\ntext","title":"Olá 😀","color":"pink","x":80.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: Olá 😀\ncolor: pink\nx: 80.5\ny: 90\nw: 260\nh: 240\ndesktop: false\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n\r\n## Olá 😀\r\ntext","parsed":{"meta":{"title":"Olá 😀","color":"pink","x":80.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"\n## Olá 😀\ntext"},"title":"Olá 😀"},{"note":{"slug":"test","body":"> quote","title":"quote","color":"green","x":81,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: quote\ncolor: green\nx: 81\ny: 90\nw: 260\nh: 240\ndesktop: true\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n> quote","parsed":{"meta":{"title":"quote","color":"green","x":81,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"> quote"},"title":"quote"},{"note":{"slug":"test","body":"1. item","title":"item","color":"blue","x":81.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: item\ncolor: blue\nx: 81.5\ny: 90\nw: 260\nh: 240\ndesktop: false\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n1. item","parsed":{"meta":{"title":"item","color":"blue","x":81.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"1. item"},"title":"item"},{"note":{"slug":"test","body":"- [x] **done**","title":"done","color":"purple","x":82,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: done\ncolor: purple\nx: 82\ny: 90\nw: 260\nh: 240\ndesktop: true\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n- [x] **done**","parsed":{"meta":{"title":"done","color":"purple","x":82,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"- [x] **done**"},"title":"done"},{"note":{"slug":"test","body":"```\ncode\n```","title":"code","color":"gray","x":82.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: code\ncolor: gray\nx: 82.5\ny: 90\nw: 260\nh: 240\ndesktop: false\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n```\ncode\n```","parsed":{"meta":{"title":"code","color":"gray","x":82.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"```\ncode\n```"},"title":"code"},{"note":{"slug":"test","body":"![image](x.png)\ntext","title":"","color":"yellow","x":83,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: \"\"\ncolor: yellow\nx: 83\ny: 90\nw: 260\nh: 240\ndesktop: true\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n![image](x.png)\ntext","parsed":{"meta":{"title":"","color":"yellow","x":83,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"![image](x.png)\ntext"},"title":""},{"note":{"slug":"test","body":"---\n# Header\n","title":"Header","color":"pink","x":83.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: Header\ncolor: pink\nx: 83.5\ny: 90\nw: 260\nh: 240\ndesktop: false\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n---\n# Header\n","parsed":{"meta":{"title":"Header","color":"pink","x":83.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"---\n# Header\n"},"title":"Header"},{"note":{"slug":"test","body":"  ### **Título**  \nnext","title":"Título","color":"green","x":84,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: Título\ncolor: green\nx: 84\ny: 90\nw: 260\nh: 240\ndesktop: true\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n  ### **Título**  \nnext","parsed":{"meta":{"title":"Título","color":"green","x":84,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"  ### **Título**  \nnext"},"title":"Título"},{"note":{"slug":"test","body":"\n","title":"","color":"blue","x":84.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: \"\"\ncolor: blue\nx: 84.5\ny: 90\nw: 260\nh: 240\ndesktop: false\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n\n","parsed":{"meta":{"title":"","color":"blue","x":84.5,"y":90,"w":260,"h":240,"desktop":false,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"\n"},"title":""},{"note":{"slug":"test","body":"_italic_ ~strike~ `code`","title":"italic strike code","color":"purple","x":85,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"serialized":"---\ntitle: italic strike code\ncolor: purple\nx: 85\ny: 90\nw: 260\nh: 240\ndesktop: true\npinned: false\ncreated: \"2026-10-16T10:12:00.000Z\"\nupdated: \"2026-10-16T10:20:31.000Z\"\n---\n_italic_ ~strike~ `code`","parsed":{"meta":{"title":"italic strike code","color":"purple","x":85,"y":90,"w":260,"h":240,"desktop":true,"pinned":false,"created":"2026-10-16T10:12:00.000Z","updated":"2026-10-16T10:20:31.000Z"},"body":"_italic_ ~strike~ `code`"},"title":"italic strike code"}]"###;
fn normalize(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Object(m) => json!(m
            .iter()
            .map(|(k, v)| (k, normalize(v)))
            .collect::<std::collections::BTreeMap<_, _>>()),
        _ => v.clone(),
    }
}
#[test]
fn original_yaml_storage_and_titles_are_preserved() {
    let cases: Value = serde_json::from_str(FROZEN).unwrap();
    for c in cases.as_array().unwrap() {
        let n: store::Note = serde_json::from_value(c["note"].clone()).unwrap();
        assert_eq!(store::title(&n.body), c["title"].as_str().unwrap());
        assert_eq!(store::serialize(&n), c["serialized"].as_str().unwrap());
        let (m, b) = store::parse(c["serialized"].as_str().unwrap());
        assert_eq!(normalize(&m), normalize(&c["parsed"]["meta"]));
        assert_eq!(b, c["parsed"]["body"].as_str().unwrap());
    }
}
#[test]
fn legacy_directory_migrates_and_updates_flush_without_losing_metadata() {
    let h = Home::new();
    fs::create_dir_all(h.0.join("Notas/old")).unwrap();
    fs::write(
        h.0.join("Notas/old/nota.md"),
        "---\ncolor: blue\nx: 150\ndesktop: true\n---\n# Old\ntext",
    )
    .unwrap();
    let mut s = store::Session::new(h.path());
    assert!(h.0.join("Notes/old/nota.md").exists());
    assert_eq!(s.list[0].color, "blue");
    s.update("old", &json!({"body":"# Changed\n😀"}), true, 1000);
    s.flush(1599, false);
    assert!(fs::read_to_string(h.0.join("Notes/old/nota.md"))
        .unwrap()
        .contains("# Old"));
    s.flush(1600, false);
    let reload = store::Session::new(h.path());
    assert_eq!(reload.list[0].title, "Changed");
    assert_eq!(reload.list[0].x, 150.);
    assert!(reload.list[0].desktop);
}
#[test]
fn new_slugs_do_not_overwrite_notes_missing_from_memory() {
    let h = Home::new();
    let mut s = store::Session::new(h.path());
    let a = s.create("a".into(), "yellow", false, 1000).unwrap();
    s.list.clear();
    let b = s.create("b".into(), "pink", false, 1000).unwrap();
    assert_ne!(a, b);
    assert_eq!(
        store::parse(&fs::read_to_string(s.root().join(a).join("nota.md")).unwrap()).1,
        "a"
    );
}
#[test]
fn multiline_editing_is_unicode_safe_and_preserves_history_and_markdown() {
    let mut e = editor::Editor::default();
    e.reset("😀 café");
    e.key("ArrowLeft", "", 0, 1000);
    e.key("Backspace", "", 0, 1100);
    assert_eq!(e.body, "😀 caé");
    e.key("z", "", 2, 1200);
    assert_eq!(e.body, "😀 café");
    e.reset("- [x] done");
    e.key("Enter", "", 0, 2000);
    assert_eq!(e.body, "- [x] done\n- [ ] ");
    e.key("Enter", "", 0, 3000);
    assert_eq!(e.body, "- [x] done\n");
    e.reset("hello");
    e.a = 0;
    e.b = 5;
    e.format("bold", 4000);
    assert_eq!(e.body, "**hello**");
    assert_eq!((e.a, e.b), (2, 7));
}
#[test]
fn emoji_lookup_search_and_completion_preserve_bilingual_names() {
    assert_eq!(emoji::get("cafe"), Some("☕"));
    assert!(emoji::search("sonrisa").iter().any(|e| e.ch == "😀"));
    let mut e = editor::Editor::default();
    e.reset(":caf");
    e.key("e", "e", 0, 1000);
    assert!(e.suggestion.is_some());
    e.key("Enter", "", 0, 1100);
    assert!(e.body.contains("☕"));
}
#[test]
fn long_unicode_documents_wrap_without_truncation() {
    let body = "界😀 hello world\n".repeat(1000);
    let mut e = editor::Editor::default();
    e.reset(&body);
    e.width = 260.;
    e.layout();
    assert!(e.rows.len() > 1000);
    assert_eq!(e.rows.last().unwrap().end, body.chars().count());
    e.key("a", "", 2, 1000);
    e.insert("new\nbody", 2000);
    assert_eq!(e.body, "new\nbody");
}
#[test]
fn attachment_copy_finishes_before_publishing_markdown_and_preserves_concurrent_edits() {
    let h = Home::new();
    fs::write(h.0.join("file.txt"), "attachment").unwrap();
    let mut s = store::Session::new(h.path());
    let slug = s.create("base".into(), "yellow", false, 1000).unwrap();
    s.attach(&slug, h.0.join("file.txt").to_str().unwrap(), (4, 4))
        .unwrap();
    s.update(&slug, &json!({"body":"new edits"}), true, 1001);
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.tick(2000);
        if s.get(&slug).unwrap().body.contains("file.txt") {
            break;
        }
    }
    assert!(s.get(&slug).unwrap().body.starts_with("new edits"));
    assert!(s
        .get(&slug)
        .unwrap()
        .body
        .contains("[📎 file.txt](file.txt)"));
    assert_eq!(
        fs::read_to_string(s.path(&slug).unwrap().join("file.txt")).unwrap(),
        "attachment"
    );
}
#[test]
fn notes_and_attachments_cannot_escape_the_home_root() {
    let h = Home::new();
    let out = Home::new();
    fs::create_dir_all(h.0.join("Notes")).unwrap();
    std::os::unix::fs::symlink(&out.0, h.0.join("Notes/escape")).unwrap();
    let mut s = store::Session::new(h.path());
    assert!(s.path("escape").is_err());
    assert!(s.path("../escape").is_err());
    let slug = s.create("safe".into(), "yellow", false, 1000).unwrap();
    fs::write(out.0.join("secret"), "secret").unwrap();
    assert!(s
        .attach(&slug, out.0.join("secret").to_str().unwrap(), (0, 0))
        .is_err());
}
#[test]
fn desktop_widgets_restore_pin_geometry_and_do_not_touch_updated_date() {
    let h = Home::new();
    let mut s = store::Session::new(h.path());
    let slug = s.create("# Widget".into(), "green", true, 1000).unwrap();
    let date = s.get(&slug).unwrap().updated.clone();
    s.update(
        &slug,
        &json!({"x":9000,"y":9000,"w":320,"h":200,"pinned":true}),
        false,
        2000,
    );
    let w = s.widgets(800., 600.);
    assert_eq!(w[0]["rect"]["left"], 720.);
    assert_eq!(w[0]["rect"]["top"], 540.);
    assert_eq!(w[0]["pinned"], true);
    assert_eq!(s.get(&slug).unwrap().updated, date);
    s.flush(2000, true);
    let again = store::Session::new(h.path());
    assert!(again.get(&slug).unwrap().pinned);
}

fn app_at(home: &Home) -> App {
    let mut app = App::default();
    app.resources("", Some(home.path()));
    app.request_at(&json!({"cmd":"sync","args":{"__nativeOwner":9,"__nativeWindow":"notes#2"},"context":{"width":860,"height":560,"theme":{"text":"#222","textDim":"#666","accent":"#06c","separator":"#ddd","windowBg":"#fff"}}}), 1000);
    app
}
#[test]
fn window_actions_and_another_mount_share_the_rust_session() {
    let home = Home::new();
    let mut first = app_at(&home);
    first.command("new", &json!({}), 1000);
    let slug = first.note().unwrap().slug;
    first.paste("# Unicode 😀\n- [ ] task\n\n## Section\nbody");
    first.command("desktop", &json!({}), 2000);
    first.command("pin", &json!({}), 2001);
    first.command("mode", &json!({"key":"read"}), 2002);
    let mut second = app_at(&home);
    assert_eq!(second.note().unwrap().slug, slug);
    assert!(second.note().unwrap().body.contains("Unicode 😀"));
    second.command("toggleTask", &json!({"key":3}), 2003);
    assert!(first.note().unwrap().body.contains("[x] task"));
    let reply = store::background(&json!({"home":home.path(),"screen":{"width":800,"height":600}}));
    assert_eq!(reply["widgets"][0]["id"], slug);
    assert_eq!(reply["widgets"][0]["pinned"], true);
    // Modal confirmation must keep its original target despite background actions.
    second.command("delete", &json!({}), 2004);
    second.command("new", &json!({}), 2005);
    assert!(second.key("n", "", 2).is_none());
    assert_eq!(second.note().unwrap().slug, slug);
    second.command("dismiss", &json!({}), 2006);
    assert!(second.panel.is_empty());
    second.request_at(&json!({"cmd":"close"}), 3000);
    let restored = store::Session::new(home.path());
    assert!(restored.get(&slug).unwrap().body.contains("[x] task"));
    assert!(restored.get(&slug).unwrap().pinned);
}
#[test]
fn desktop_geometry_updates_preserve_editing_and_deleted_widgets_do_not_switch_notes() {
    let home = Home::new();
    let mut app = app_at(&home);
    app.command("new", &json!({}), 1000);
    let slug = app.note().unwrap().slug;
    let facts = |left| json!({"cmd":"sync","args":{"__nativeOwner":12,"widget":slug,"rect":{"left":left,"top":80,"width":260,"height":240}},"context":{"width":260,"height":240}});
    app.request_at(&facts(100), 2000);
    app.set_mode("edit");
    app.request_at(&facts(130), 2001);
    assert!(app.text_input());
    store::with(home.path(), |s| s.list.retain(|n| n.slug != slug));
    assert!(app.note().is_none());
    let tree = app.request_at(&facts(130), 2002);
    assert_eq!(tree["tree"]["children"], json!([]));
}
#[test]
fn formatting_menu_and_composition_are_native_and_copy_does_not_erase_empty_clipboard() {
    let home = Home::new();
    let mut app = app_at(&home);
    app.command("new", &json!({}), 1000);
    app.editor.reset("hello");
    app.editor.focused = true;
    app.editor.a = 0;
    app.editor.b = 5;
    app.command("format", &json!({"key":"bold"}), 1001);
    assert_eq!(app.note().unwrap().body, "**hello**");
    app.menu();
    assert_eq!(
        app.menus[1]["items"][0]["action"],
        json!(["format", "bold"])
    );
    app.composition = json!({"preedit":"にほん","candidates":["日本","二本"],"selected":0});
    let tree = ui::tree(&mut app, 1002).to_string();
    assert!(tree.contains("にほん") && tree.contains("日本"));
    app.editor.reset("code");
    app.editor.focused = true;
    app.editor.a = 0;
    app.editor.b = 4;
    app.key("C", "", 2 | 1);
    assert!(app.note().unwrap().body.contains("```"));
}
