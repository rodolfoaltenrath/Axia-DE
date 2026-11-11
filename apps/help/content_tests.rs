use super::*;
use std::{
    os::unix::fs::symlink,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "axia-help-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn put(&self, lang: &str, id: &str, text: &str) {
        let dir = self.0.join("help").join(lang);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{id}.md")), text).unwrap();
    }
    fn content(&self) -> Content {
        Content::new(&self.0)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn translations_replace_metadata_keep_english_fallback_and_include_extra_articles() {
    let f = Fixture::new();
    f.put("en","clock","---\ntitle: Clock\napp: clock\nsection: apps\norder: 20\nkeywords: alarm, timer\n---\n# Clock\nBody");
    f.put(
        "en",
        "welcome",
        "---\ntitle: Welcome\norder: 1\n---\nEnglish body",
    );
    f.put("pt","clock","---\ntitle: Relógio\napp: clock\nsection: apps\norder: 20\nkeywords: alarme\n---\nConteúdo português");
    f.put(
        "pt",
        "extra",
        "---\ntitle: Extra\norder: 30\n---\nOnly Portuguese",
    );
    let mut c = f.content();
    let list = c.list("pt");
    assert_eq!(
        list.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        ["welcome", "clock", "extra"]
    );
    assert_eq!(list[1].title, "Relógio");
    assert_eq!(list[1].keywords, "alarme");
    assert_eq!(c.read("pt", "clock").unwrap().body, "Conteúdo português");
    assert_eq!(c.read("pt", "welcome").unwrap().body, "English body");
    assert_eq!(c.list("ja").len(), 2);
    assert!(c.read("ja", "absent").is_none());
    assert_eq!(c.raw_list("ja").len(), 0);
    assert!(c.raw_read("ja", "welcome").is_none());
}

#[test]
fn front_matter_preserves_body_and_only_accepts_a_complete_header() {
    let raw = "---\n title : Título: teste \napp: clock\nignored\n: invalid\n---\n\n# Hello\n";
    let a = parse("test", raw);
    assert_eq!(a.meta.title, "Título: teste");
    assert_eq!(a.meta.app, "clock");
    assert_eq!(a.body, "\n# Hello\n");
    for raw in [
        "# No header\nBody",
        "---\ntitle: Unfinished",
        "---\ntitle: False\n---oops\nbody",
    ] {
        let a = parse("test", raw);
        assert_eq!(a.body, raw);
        assert_eq!(a.meta.title, "test");
    }
    assert_eq!(
        parse("test", "---\ntitle: \norder: bogus\n---").meta.order,
        0
    );
}

#[test]
fn ranking_uses_title_then_keywords_then_body_and_keeps_order_for_ties() {
    let f = Fixture::new();
    for (id, title, order, keywords, body) in [
        ("prefix", "Árvore guia", 8, "", "Body"),
        ("substring", "Minha árvore", 7, "", "Body"),
        ("keywords", "Manual", 6, "árvore", "Body"),
        ("body", "Referência", 5, "", "Texto sobre árvore."),
        ("tie", "Outro guia", 1, "árvore", "Body"),
        ("absent", "Nada", 0, "", "Nothing"),
    ] {
        f.put(
            "en",
            id,
            &format!(
                "---\ntitle: {title}\norder: {order}\nkeywords: {keywords}\n---\n# {title}\n{body}"
            ),
        );
    }
    let mut c = f.content();
    let hits = c.search("en", " ÁRVORE ");
    assert_eq!(
        hits.iter()
            .map(|h| h["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["prefix", "substring", "tie", "keywords", "body"]
    );
    assert_eq!(
        hits.iter()
            .map(|h| h["score"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [90, 70, 55, 55, 30]
    );
    assert!(c.search("en", "a").is_empty());
    assert!(c.search("en", "").is_empty());
    assert!(c.search("en", &"a".repeat(4097)).is_empty());
}

#[test]
fn snippets_keep_unicode_whole_words_and_the_original_fallback() {
    let body = format!(
        "# Heading\n{} **ação** com café e 日本語. {}",
        "Uma frase anterior. ".repeat(15),
        "fim ".repeat(80)
    );
    let snippet = snippet(&body, "acao", true);
    assert!(snippet.starts_with('…'));
    assert!(snippet.contains("ação com café e 日本語."));
    assert!(!snippet.contains("**"));
    assert!(!snippet.contains("Heading"));
    assert_eq!(
        super::snippet("# Heading\nA body with **bold**.", "keywords", false),
        "A body with **bold**."
    );
    assert_eq!(
        super::snippet("# 日本語\n日本語の説明です。", "日本語", true),
        "日本語 日本語の説明です。"
    );
    let decomposed = format!("{}needle and context", "e\u{301} ".repeat(300));
    assert!(super::snippet(&decomposed, "needle", true).contains("needle and context"));
}

#[test]
fn equal_order_titles_use_the_same_nfc_code_point_comparison_as_quickjs() {
    let f = Fixture::new();
    f.put("en", "a", "---\ntitle: e\u{301}\n---\nbody");
    f.put("en", "b", "---\ntitle: é\n---\nbody");
    f.put("en", "c", "---\ntitle: Zebra\n---\nbody");
    let ids: Vec<_> = f.content().list("en").into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["c", "a", "b"]);
}

#[test]
fn invalid_paths_symlinks_directories_fifos_and_oversized_articles_are_not_read() {
    let f = Fixture::new();
    f.put("en", "safe", "hello");
    let c = f.content();
    for lang in [
        "",
        "../en",
        "en/../../",
        "-en",
        "EN",
        "abcdefghijklmn",
        "en\0",
    ] {
        assert!(c.raw_read(lang, "safe").is_none(), "{lang:?}");
    }
    for id in [
        "", "../safe", "safe.md", "-safe", "SAFE", "safe/foo", "safe\0",
    ] {
        assert!(c.raw_read("en", id).is_none(), "{id:?}");
    }
    f.put("en", "large", &"x".repeat(MAX_ARTICLE + 1));
    fs::create_dir(f.0.join("help/en/directory.md")).unwrap();
    symlink(f.0.join("help/en/safe.md"), f.0.join("help/en/link.md")).unwrap();
    let fifo = CString::new(f.0.join("help/en/pipe.md").as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    for id in ["large", "directory", "link", "pipe"] {
        assert!(c.raw_read("en", id).is_none(), "{id}");
    }
    assert_eq!(f.content().raw_list("en").len(), 1);
    assert!(Content::default().raw_read("en", "safe").is_none());
    assert!(f.content().list("../en").is_empty());
}

#[test]
fn cached_content_is_refreshed_on_clear_and_remains_bounded() {
    let f = Fixture::new();
    f.put("en", "safe", "---\ntitle: Old\n---\nold");
    let mut c = f.content();
    assert_eq!(c.list("en")[0].title, "Old");
    assert_eq!(c.read("en", "safe").unwrap().body, "old");
    f.put("en", "safe", "---\ntitle: New\n---\nnew");
    assert_eq!(c.read("en", "safe").unwrap().body, "old");
    c.clear();
    assert_eq!(c.list("en")[0].title, "New");
    assert_eq!(c.read("en", "safe").unwrap().body, "new");
    for n in 0..MAX_BODIES + 5 {
        let id = format!("a{n}");
        f.put("en", &id, "body");
        c.read("en", &id).unwrap();
    }
    assert_eq!(c.bodies.len(), MAX_BODIES);
    for n in 0..MAX_LANGUAGES + 5 {
        c.raw_list(&format!("l{n}"));
    }
    assert!(c.lists.len() <= MAX_LANGUAGES);
}

#[test]
fn links_only_classify_the_existing_internal_schemes() {
    assert_eq!(
        link("settings:network"),
        json!({"app":"settings","args":{"section":"network"}})
    );
    assert_eq!(link("app:remote-desk"), json!({"app":"remote-desk"}));
    assert_eq!(
        link("help:clock"),
        json!({"app":"help","args":{"article":"clock"}})
    );
    for url in [
        "https://example.org",
        "file:/etc/passwd",
        "javascript:alert(1)",
        "settings:a-b",
        "app:../files",
        "help:",
        "app:Clock",
        "help:a:b",
    ] {
        assert!(link(url).is_null(), "{url}");
    }
}

#[test]
fn bridge_preserves_raw_calls_and_parsed_requests_and_rejects_bad_json() {
    let f = Fixture::new();
    f.put("en", "safe", "---\ntitle: Safe\n---\nHello");
    let root = CString::new(f.0.as_os_str().as_encoded_bytes()).unwrap();
    let call = |request: &str| unsafe {
        let request = CString::new(request).unwrap();
        let out = axia_help_request(root.as_ptr(), request.as_ptr());
        if out.is_null() {
            return None;
        }
        let out = CString::from_raw(out);
        Some(serde_json::from_slice::<Value>(out.as_bytes()).unwrap())
    };
    assert_eq!(
        call(r#"{"cmd":"rawRead","lang":"en","id":"safe"}"#).unwrap(),
        "---\ntitle: Safe\n---\nHello"
    );
    assert_eq!(
        call(r#"{"cmd":"read","lang":"pt","id":"safe"}"#).unwrap()["body"],
        "Hello"
    );
    assert_eq!(
        call(r#"{"cmd":"search","lang":"en","query":"safe"}"#).unwrap()[0]["score"],
        90
    );
    assert!(call("{").is_none());
    assert!(call(r#"{"cmd":"invalid"}"#).unwrap().is_null());
}

#[test]
fn bundled_articles_are_readable_in_all_shipped_languages() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let mut c = Content::new(assets);
    let english = c.list("en");
    assert!(english.len() > 20);
    for (lang, query) in [("en", "clock"), ("pt", "relógio"), ("es", "reloj")] {
        let list = c.list(lang);
        assert!(list.len() >= english.len());
        for item in list {
            assert!(
                !c.read(lang, &item.id).unwrap().body.is_empty(),
                "{lang}/{}",
                item.id
            );
        }
        assert!(c.search(lang, query).iter().any(|hit| hit["id"] == "clock"));
    }
}
