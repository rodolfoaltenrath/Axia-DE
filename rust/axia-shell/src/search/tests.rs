use super::*;
#[test]
fn calculator_and_units_preserve_search_examples() {
    for (text, value) in [
        ("2*(3+4)", 14.),
        ("15% de 2400", 360.),
        ("15% of 2400", 360.),
        ("sqrt(2)^2", 2.),
        ("2^3^2", 512.),
        ("-2^2", -4.),
        ("2**3", 8.),
        ("round(-1.5)", -1.),
        ("1,5+2", 3.5),
    ] {
        assert!((math::calc(text).unwrap() - value).abs() < 1e-10, "{text}");
    }
    for text in ["hello", "42", "1/0", "sqrt(-1)", "2+", "()"] {
        assert!(math::calc(text).is_none(), "{text}");
    }
    assert!(math::calc(&"(".repeat(5000)).is_none());
    for (text, value) in [
        ("10 km to mi", 6.2137119223733395),
        ("72 f to c", 22.22222222222223),
        ("1,5 gb to mb", 1500.),
    ] {
        assert!((math::convert(text).unwrap()["value"].as_f64().unwrap() - value).abs() < 1e-9);
    }
    assert!(math::convert("10 kg to km").is_none());
}
#[test]
fn matching_keeps_accents_word_prefixes_and_fuzzy_weights() {
    assert_eq!(fold("ÁRVORE İ ΟΣ"), "arvore i ος");
    for (a, q, f, value) in [
        ("files", "files", true, 100.),
        ("files", "fil", true, 80.),
        ("my files", "files", true, 60.),
        ("profile", "file", true, 40.),
        ("calculator", "cltr", true, 15.),
        ("calculator", "cltr", false, 0.),
    ] {
        assert_eq!(score(a, q, f), value);
    }
}
fn engine() -> Search {
    let mut search = Search::default();
    let path = std::env::temp_dir().join(format!("axia-search-empty-{}", std::process::id()));
    search.config = Some(Config {
        home: path.clone(),
        assets: path.clone(),
        cache: path.join("cache"),
    });
    search.apps(
        [
            json!({"id":"files","name":"Files","kind":"lordos"}),
            json!({"id":"editor","name":"Editor","kind":"lordos"}),
        ]
        .into_iter(),
    );
    search
}
#[test]
fn context_capability_and_client_lifetimes_are_bounded() {
    assert!(Search::default().request(&json!({"cmd":"open"})).is_err());
    let mut search = engine();
    let mut ids = vec![];
    for _ in 0..8 {
        ids.push(
            search.request(&json!({"cmd":"open"})).unwrap()["id"]
                .as_u64()
                .unwrap(),
        );
    }
    assert!(search.request(&json!({"cmd":"open"})).is_err());
    for id in ids {
        search.request(&json!({"cmd":"close","id":id})).unwrap();
    }
    let stats = search.request(&json!({"cmd":"stats"})).unwrap();
    assert_eq!(stats["clients"], 0);
    assert_eq!(stats["activeBytes"], 0);
}
#[test]
fn panel_geometry_and_empty_actions_remain_stable() {
    let mut search = engine();
    let r=search.request(&json!({"cmd":"open","panel":true,"context":{"area":{"width":1000,"height":700},"theme":{},"glass":{}}})).unwrap();
    assert_eq!(r["tree"]["children"][0]["style"]["left"], 140.);
    assert_eq!(r["tree"]["children"][0]["style"]["top"], 112.);
    let id = r["id"].as_u64().unwrap();
    let empty = search
        .request(&json!({"cmd":"event","id":id,"action":["key","Enter"]}))
        .unwrap();
    assert!(empty["effects"].as_array().unwrap().is_empty());
    search.request(&json!({"cmd":"close","id":id})).unwrap();
}

struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "axia-search-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn engine(&self) -> Search {
        let mut search = engine();
        search.config = Some(Config {
            home: self.0.clone(),
            assets: self.0.join("assets"),
            cache: self.0.join(".cache/axia/search"),
        });
        search
    }
    fn write(&self, name: &str, text: &str) {
        let p = self.0.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn open(search: &mut Search, panel: bool) -> u64 {
    search.request(&json!({"cmd":"open","panel":panel,"context":{"area":{"width":1000,"height":700},"theme":{},"glass":{}}})).unwrap()["id"].as_u64().unwrap()
}
fn settle(search: &mut Search, id: u64) {
    let start = std::time::Instant::now();
    loop {
        search.request(&json!({"cmd":"poll"})).unwrap();
        let c = &search.clients[&id];
        if !c.pending && !c.preview_pending {
            return;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(8),
            "worker did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn keys(search: &Search, id: u64) -> Vec<String> {
    search.clients[&id]
        .flat()
        .iter()
        .map(|it| s(it, "key").to_owned())
        .collect()
}

#[test]
fn blocked_io_does_not_block_input_and_cancelled_results_do_not_resurface() {
    let home = Home::new();
    let mut search = home.engine();
    search.start_worker();
    let (started, ready) = std::sync::mpsc::sync_channel(1);
    let (release, wait) = std::sync::mpsc::channel();
    search.worker.as_ref().unwrap().submit(Job {
        id: 0,
        generation: 0,
        token: Arc::new(AtomicU64::new(0)),
        work: Work::Gate(started, wait),
    });
    ready
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let id = open(&mut search, true);
    let first = search
        .request(&json!({"cmd":"query","id":id,"query":"Files"}))
        .unwrap();
    assert_eq!(first["sections"][0]["items"][0]["key"], "app:files");
    assert_eq!(first["pending"], true);
    search
        .request(&json!({"cmd":"query","id":id,"query":"Editor"}))
        .unwrap();
    release.send(()).unwrap();
    settle(&mut search, id);
    assert_eq!(keys(&search, id), ["app:editor"]);
    search.request(&json!({"cmd":"close","id":id})).unwrap();
    assert_eq!(
        search.request(&json!({"cmd":"stats"})).unwrap()["activeBytes"],
        0
    );
}

#[test]
fn files_notes_help_and_text_previews_run_off_thread() {
    let home = Home::new();
    home.write("Documents/budget.txt", "annual budget\nsecond line");
    home.write(
        "Notes/entry/nota.md",
        "---\ntitle: Budget note\n---\nAnnual budget discussion",
    );
    home.write(
        "assets/help/en/budget.md",
        "---\ntitle: Budget help\nkeywords: finance\n---\nHow to plan a budget",
    );
    let mut search = home.engine();
    search
        .request(&json!({"cmd":"facts","lang":"en","notes":true,"help":true}))
        .unwrap();
    let id = open(&mut search, true);
    search
        .request(&json!({"cmd":"query","id":id,"query":"budget"}))
        .unwrap();
    settle(&mut search, id);
    let result = keys(&search, id);
    assert!(result.iter().any(|s| s.starts_with("file:")));
    assert!(result.contains(&"note:entry".into()));
    assert!(result.contains(&"help:budget".into()));
    let at = search.clients[&id]
        .flat()
        .iter()
        .position(|it| s(it, "key").ends_with("budget.txt"))
        .unwrap();
    search
        .request(&json!({"cmd":"event","id":id,"action":["select",at]}))
        .unwrap();
    settle(&mut search, id);
    assert_eq!(search.clients[&id].preview["kind"], "text");
    assert!(s(&search.clients[&id].preview, "text").starts_with("annual budget"));
    let index = home.0.join(".cache/axia/search/files.jsonl");
    assert!(std::fs::metadata(index).unwrap().len() <= worker::INDEX_BYTES);
    search.request(&json!({"cmd":"close","id":id})).unwrap();
}

#[test]
fn filesystem_changes_invalidate_the_disk_index_and_hidden_files_stay_hidden() {
    let home = Home::new();
    home.write("report-first.txt", "one");
    home.write(".report-secret", "secret");
    let mut search = home.engine();
    let id = open(&mut search, false);
    search
        .request(&json!({"cmd":"query","id":id,"query":"report"}))
        .unwrap();
    settle(&mut search, id);
    assert_eq!(keys(&search, id).len(), 1);
    home.write("report-added.txt", "two");
    search
        .request(&json!({"cmd":"query","id":id,"query":"report-"}))
        .unwrap();
    settle(&mut search, id);
    assert_eq!(keys(&search, id).len(), 2);
    std::fs::remove_file(home.0.join("report-first.txt")).unwrap();
    search
        .request(&json!({"cmd":"query","id":id,"query":"report"}))
        .unwrap();
    settle(&mut search, id);
    assert_eq!(keys(&search, id).len(), 1);
    search.request(&json!({"cmd":"close","id":id})).unwrap();
}

#[test]
fn preview_rejects_symlink_escapes_and_special_files() {
    use std::os::unix::fs::symlink;
    let home = Home::new();
    let outside = Home::new();
    outside.write("secret.txt", "outside secret");
    symlink(&outside.0, home.0.join("escape")).unwrap();
    let fifo = home.0.join("pipe.txt");
    let path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut search = home.engine();
    let id = open(&mut search, true);
    let c = search.clients.get_mut(&id).unwrap();
    c.sections = vec![section(
        "files",
        vec![{
            let mut it = item(
                "file:escape",
                80.,
                json!("secret.txt"),
                json!(""),
                "📄",
                json!({}),
            );
            it["preview"] = json!({"kind":"file","file":{"name":"secret.txt","path":home.0.join("escape/secret.txt")}});
            it
        }],
        None,
    )];
    let mut c = search.clients.remove(&id).unwrap();
    search.start_worker();
    search.reply(&mut c, vec![]);
    search.clients.insert(id, c);
    settle(&mut search, id);
    assert_eq!(search.clients[&id].preview["kind"], "info");
    let mut c = search.clients.remove(&id).unwrap();
    c.sections[0]["items"][0]["key"] = json!("file:pipe");
    c.sections[0]["items"][0]["preview"]["file"] = json!({"name":"pipe.txt","path":fifo});
    search.reply(&mut c, vec![]);
    search.clients.insert(id, c);
    settle(&mut search, id);
    assert_eq!(search.clients[&id].preview["kind"], "info");
    search.request(&json!({"cmd":"close","id":id})).unwrap();
}

#[test]
fn keyboard_selection_sections_execution_and_preview_cleanup_are_owned_by_rust() {
    let home = Home::new();
    let mut search = home.engine();
    let id = open(&mut search, true);
    search.request(&json!({"cmd":"facts","settings":[{"id":"appearance","title":"File settings","keywords":""}],"clipboard":[{"id":"clip","time":1,"text":"file sample"}]})).unwrap();
    search
        .request(&json!({"cmd":"query","id":id,"query":"file"}))
        .unwrap();
    assert_eq!(keys(&search, id)[0], "app:files");
    let tab = search
        .request(&json!({"cmd":"event","id":id,"action":["key","Tab"]}))
        .unwrap();
    assert_eq!(tab["selected"], 1);
    let run = search
        .request(&json!({"cmd":"event","id":id,"action":["key","Enter"]}))
        .unwrap();
    assert_eq!(run["effects"][1]["action"]["type"], "settings");
    assert_eq!(run["effects"][0]["type"], "close");
    search.request(&json!({"cmd":"close","id":id})).unwrap();
    let stats = search.request(&json!({"cmd":"stats"})).unwrap();
    assert_eq!(stats["clipboardBytes"], 0);
    assert_eq!(stats["activeBytes"], 0);
}

#[test]
fn bounded_images_are_thumbnail_sized_and_removed_after_close() {
    let home = Home::new();
    let file = home.0.join("photo.png");
    image::RgbaImage::from_pixel(800, 600, image::Rgba([1, 2, 3, 255]))
        .save(&file)
        .unwrap();
    let mut search = home.engine();
    let id = open(&mut search, true);
    search
        .request(&json!({"cmd":"query","id":id,"query":"photo"}))
        .unwrap();
    settle(&mut search, id);
    let path = PathBuf::from(s(&search.clients[&id].preview, "path"));
    assert!(path.is_file());
    let size = image::image_dimensions(&path).unwrap();
    assert!(size.0 <= 232 && size.1 <= 160);
    // Saturate the control queue while I/O is blocked: dropped release jobs must
    // not keep thumbnails alive after their client's cancellation token changes.
    let (started, ready) = std::sync::mpsc::sync_channel(1);
    let (release, wait) = std::sync::mpsc::channel();
    search.worker.as_ref().unwrap().submit(Job {
        id: 0,
        generation: 0,
        token: Arc::new(AtomicU64::new(0)),
        work: Work::Gate(started, wait),
    });
    ready
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    search.request(&json!({"cmd":"close","id":id})).unwrap();
    for _ in 0..100 {
        let noise = open(&mut search, false);
        search.request(&json!({"cmd":"close","id":noise})).unwrap();
    }
    assert!(search.worker.as_ref().unwrap().pending() <= worker::MAX_JOBS);
    release.send(()).unwrap();
    let start = std::time::Instant::now();
    while path.exists() {
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn non_utf8_filenames_do_not_stop_search_and_usage_keys_have_a_byte_budget() {
    use std::os::unix::ffi::OsStringExt;
    let home = Home::new();
    home.write("report-valid.txt", "valid");
    std::fs::write(
        home.0
            .join(std::ffi::OsString::from_vec(b"report-\xff.txt".to_vec())),
        "invalid UTF-8 name",
    )
    .unwrap();
    let mut search = home.engine();
    let id = open(&mut search, false);
    search
        .request(&json!({"cmd":"query","id":id,"query":"report"}))
        .unwrap();
    settle(&mut search, id);
    assert_eq!(keys(&search, id).len(), 1);
    search.request(&json!({"cmd":"close","id":id})).unwrap();
    let mut usage = std::collections::BTreeMap::new();
    for n in 0..300 {
        usage.insert(format!("{n:03}{}", "x".repeat(4090)), (1, n as f64));
    }
    worker::trim_usage(&mut usage);
    assert!(usage.keys().map(String::len).sum::<usize>() <= 65536);
    assert!(usage.values().all(|(_, at)| *at >= 284.));
}

#[test]
fn client_isolation_and_stale_external_results_are_preserved() {
    let home = Home::new();
    let mut search = home.engine();
    let a = open(&mut search, false);
    let b = open(&mut search, false);
    search
        .request(&json!({"cmd":"query","id":a,"query":"Files"}))
        .unwrap();
    search
        .request(&json!({"cmd":"query","id":b,"query":"Editor"}))
        .unwrap();
    search.request(&json!({"cmd":"external","id":a,"query":"old","sections":[{"id":"test","items":[{"key":"stale","score":900}]}]})).unwrap();
    assert!(!keys(&search, a).contains(&"stale".into()));
    assert_eq!(keys(&search, b), ["app:editor"]);
    search.request(&json!({"cmd":"close","id":a})).unwrap();
    settle(&mut search, b);
    assert_eq!(keys(&search, b), ["app:editor"]);
    search.request(&json!({"cmd":"close","id":b})).unwrap();
}
