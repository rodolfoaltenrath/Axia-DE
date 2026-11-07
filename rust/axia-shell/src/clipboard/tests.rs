use super::*;
use std::fs;
use std::path::Path;
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "axia-clipboard-{}-{}",
            std::process::id(),
            next_id()
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn config() -> Config {
    Config::parse(&json!({}))
}
fn entry(text: &str, time: f64, pinned: bool) -> Entry {
    Entry {
        id: next_id(),
        kind: "text".into(),
        category: classify(text, false).into(),
        text: Some(text.into()),
        image: None,
        origin: "terminal".into(),
        time,
        pinned,
        bytes: 0,
        src: None,
        revision: 0,
        summary: String::new(),
        characters: 0,
    }
}
fn wait(s: &Service) -> Value {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(v) = s.take() {
            return serde_json::from_str(&v).unwrap();
        }
        assert!(Instant::now() < until, "worker result");
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn classifier_preserves_all_filters_and_unicode() {
    for (text, kind) in [
        ("olá 👋", "text"),
        ("https://example.com/ação", "link"),
        ("www.axia.dev", "link"),
        ("mailto:a@b.c", "link"),
        ("#a0B1c2", "color"),
        ("rgba(1,2,3,0.5)", "color"),
        ("~/Imagens/férias.png", "file"),
        ("file:///tmp/a", "file"),
        ("fn main() {\n println!(\"Olá\");\n}", "code"),
        ("linha\n  dois\n  três", "code"),
    ] {
        assert_eq!(classify(text, false), kind, "{text}")
    }
    assert_eq!(classify("anything", true), "image");
    assert_eq!(classify("not #fff", false), "text");
}
#[test]
fn dedup_pin_order_expiration_and_hard_text_budget() {
    let mut m = Model::new(vec![]);
    let mut c = config();
    c.max = 2;
    m.insert(entry("primeiro", now(), false));
    let first = m.items[0].id.clone();
    m.act("pin", &first, true);
    m.insert(entry("segundo", now(), false));
    m.insert(entry("terceiro", now(), false));
    m.insert(entry("quarto", now(), false));
    m.prune(&c, now());
    assert_eq!(m.items.len(), 3);
    assert!(m.items.iter().any(|e| e.id == first && e.pinned));
    m.insert(entry("primeiro", now(), false));
    assert_eq!(m.items[0].id, first);
    assert!(m.items[0].pinned);
    let original = m.items[0].time;
    m.act("use", &first, true);
    assert!(m.items[0].time >= original);
    m.items.push(entry("velho", 0., false));
    m.prune(&c, now());
    assert!(!m.items.iter().any(|e| e.text.as_deref() == Some("velho")));
    c.max = 2000;
    for _ in 0..40 {
        m.insert(entry(&"x".repeat(ITEM_BYTES), now(), false));
    } // repeated text deduplicates
    for n in 0..25 {
        m.insert(entry(
            &format!("{n}{}", "x".repeat(ITEM_BYTES - 3)),
            now(),
            false,
        ));
    }
    m.prune(&c, now());
    assert!(
        m.items
            .iter()
            .filter_map(|e| e.text.as_ref())
            .map(String::len)
            .sum::<usize>()
            <= TEXT_BYTES
    );
    m.act("clear", "", true);
    assert_eq!(m.items.len(), 1);
    assert_eq!(m.items[0].id, first);
    m.act("clear", "", false);
    assert!(m.items.is_empty());
}
#[test]
fn legacy_history_keeps_ids_pins_unicode_and_is_atomically_private() {
    let t = Temp::new();
    let mut store = store::Store::new(&t.0, None);
    fs::create_dir_all(&store.dir).unwrap();
    let legacy = json!({"version":1,"items":[{"id":"legacy-1","kind":"text","type":"text","text":"Olá, Fedora 👋","origin":"terminal","time":now(),"pinned":true,"src":"/run/user/1000/axia/clip-1.png"}]});
    fs::write(store.dir.join("history.json"), legacy.to_string()).unwrap();
    let items = store.load().unwrap();
    assert_eq!(items[0].id, "legacy-1");
    assert!(
        items[0].src.is_none(),
        "preview names must not deduplicate across sessions"
    );
    assert!(items[0].pinned);
    assert_eq!(items[0].text.as_deref(), Some("Olá, Fedora 👋"));
    store.save(&items).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(store.dir.join("history.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(store.load().unwrap()[0].id, "legacy-1");
}
#[test]
fn corrupt_oversized_links_and_arbitrary_image_paths_are_preserved_or_rejected() {
    use std::os::unix::fs::symlink;
    let t = Temp::new();
    let mut store = store::Store::new(&t.0, None);
    fs::create_dir_all(&store.dir).unwrap();
    let p = store.dir.join("history.json");
    fs::write(&p, "corrupt").unwrap();
    assert!(store.load().is_err());
    assert!(store.save(&[]).is_err());
    assert_eq!(fs::read_to_string(&p).unwrap(), "corrupt");
    fs::remove_file(&p).unwrap();
    let other = t.0.join("other");
    fs::write(&other, "untouched").unwrap();
    symlink(&other, &p).unwrap();
    assert!(store.load().is_err());
    assert!(store.save(&[]).is_err());
    assert_eq!(fs::read_to_string(&other).unwrap(), "untouched");
    fs::remove_file(&p).unwrap();
    let large = File::create(&p).unwrap();
    large.set_len(33 * 1024 * 1024).unwrap();
    drop(large);
    assert!(store.load().is_err());
    assert!(store.save(&[]).is_err());
    assert_eq!(fs::metadata(&p).unwrap().len(), 33 * 1024 * 1024);
    assert!(!store.owned(&other));
    store.remove(other.to_str().unwrap());
    assert!(other.exists());
    assert!(!store.owned(&store.dir.join("../img-a.png")));
    assert!(!store.owned(&store.dir.join("img-../../other.png")));
}
#[test]
fn packet_coalescing_and_theme_resubscribe_deliver_complete_projection() {
    let t = Temp::new();
    let s = Service::new(t.0.clone(), None, config()).unwrap();
    wait(&s);
    s.enqueue(Command::Text("A".into(), "terminal".into()));
    s.enqueue(Command::Text("B".into(), "terminal".into()));
    assert!(s.flush(false));
    let p = wait(&s);
    assert_eq!(p["order"].as_array().unwrap().len(), 2);
    assert_eq!(p["upsert"].as_array().unwrap().len(), 2);
    s.enqueue(Command::Text("C".into(), "terminal".into()));
    assert!(s.flush(false));
    let p = wait(&s);
    assert_eq!(p["upsert"].as_array().unwrap().len(), 1);
    assert_eq!(p["order"].as_array().unwrap().len(), 3);
    s.enqueue(Command::Configure(config(), true));
    assert!(s.flush(false));
    let p = wait(&s);
    assert_eq!(p["upsert"].as_array().unwrap().len(), 3);
}
#[test]
fn ignored_apps_save_disable_and_logout_pin_policy() {
    let t = Temp::new();
    let mut c = config();
    c.ignore = vec!["secret-app".into()];
    c.clear_logout = true;
    let s = Service::new(t.0.clone(), None, c.clone()).unwrap();
    wait(&s);
    s.enqueue(Command::Text("secret".into(), "secret-app".into()));
    s.enqueue(Command::Text("keep".into(), "terminal".into()));
    assert!(s.flush(false));
    let p = wait(&s);
    assert_eq!(p["order"].as_array().unwrap().len(), 1);
    let id = p["order"][0].as_str().unwrap().to_string();
    s.enqueue(Command::Act("pin".into(), id, true));
    s.enqueue(Command::Text("temporary".into(), "terminal".into()));
    assert!(s.flush(true));
    let p = wait(&s);
    assert_eq!(p["order"].as_array().unwrap().len(), 1);
    c.save = false;
    s.enqueue(Command::Configure(c, false));
    assert!(s.flush(false));
    let path = t.0.join(".local/share/axia/clipboard/history.json");
    let disk: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(disk["items"].as_array().unwrap().is_empty());
    assert_eq!(wait(&s)["order"].as_array().unwrap().len(), 1);
}
#[test]
fn image_stream_copy_survives_source_unlink_and_managed_deletion_is_confined() {
    let t = Temp::new();
    let rt = t.0.join("run");
    fs::create_dir(&rt).unwrap();
    let store = store::Store::new(&t.0, Some(&rt));
    let src = t.0.join("source.png");
    fs::write(&src, b"\x89PNG\r\n\x1a\ncompressed fixture").unwrap();
    let f = File::open(&src).unwrap();
    fs::remove_file(&src).unwrap();
    let (p, n) = store.image(f, true).unwrap();
    assert_eq!(n, 26);
    assert!(store.image_exists(Path::new(&p)));
    assert_eq!(
        fs::read(&p).unwrap(),
        b"\x89PNG\r\n\x1a\ncompressed fixture"
    );
    let mut e = entry("", now(), false);
    e.text = None;
    e.image = Some(p.clone());
    store.transient(&mut e);
    assert!(!Path::new(&p).exists());
    assert!(e.image.as_ref().unwrap().contains("clip-rust-"));
    let runtime = e.image.clone().unwrap();
    store.persistent(&mut e);
    assert!(!Path::new(&runtime).exists());
    store.remove(e.image.as_ref().unwrap());
    assert!(!Path::new(e.image.as_ref().unwrap()).exists());
}
#[test]
fn blocked_worker_never_blocks_capture_or_grows_queue_without_bound() {
    let t = Temp::new();
    let s = Service::new(t.0.clone(), None, config()).unwrap();
    wait(&s);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    s.enqueue(Command::Barrier(entered_tx, release_rx));
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    for n in 0..32 {
        assert!(s.enqueue(Command::Text(
            format!("{n}{}", "x".repeat(ITEM_BYTES - 3)),
            "terminal".into()
        )));
    }
    {
        let q = s.state.shared.lock().unwrap();
        assert!(q.bytes <= QUEUE_BYTES);
        assert!(q.queue.len() <= QUEUE_ITEMS);
    }
    assert!(s.enqueue(Command::Act("clear".into(), "".into(), false)));
    assert!(s.enqueue(Command::Text("latest copy".into(), "terminal".into())));
    release_tx.send(()).unwrap();
    assert!(s.flush(false));
    let p = wait(&s);
    assert_eq!(p["order"].as_array().unwrap().len(), 1);
    assert_eq!(p["upsert"][0]["text"], "latest copy");
}
#[test]
fn worker_wakes_eventfd_once_and_becomes_idle_after_delivery() {
    let t = Temp::new();
    let s = Service::new(t.0.clone(), None, config()).unwrap();
    wait(&s);
    let mut fd = libc::pollfd {
        fd: s.state.event.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(unsafe { libc::poll(&mut fd, 1, 0) }, 0);
    assert!(s.enqueue(Command::Text("wake the session".into(), "terminal".into())));
    assert_eq!(unsafe { libc::poll(&mut fd, 1, 1000) }, 1);
    assert_eq!(wait(&s)["upsert"][0]["text"], "wake the session");
    assert_eq!(unsafe { libc::poll(&mut fd, 1, 0) }, 0);
}

#[test]
fn worker_query_preserves_pin_order_filters_counts_unicode_and_cancellation() {
    let mut m = Model::new(vec![]);
    m.insert(entry("FÉRIAS no Fedora", now(), true));
    let pin = m.items[0].id.clone();
    m.insert(entry("https://axia.example/férias", now(), false));
    let link = m.items[0].id.clone();
    m.insert(entry("#aabbcc", now(), false));
    let version = m.version;
    let generation = AtomicU64::new(1);
    assert!(m.query("férias", "all", 12, 1, &generation));
    let v = m.view.as_ref().unwrap();
    assert_eq!(v["ids"], json!([pin, link]));
    assert_eq!(v["counts"]["all"], 3);
    assert_eq!(v["counts"]["pinned"], 1);
    assert!(m.query("", "link", 2048, 1, &generation));
    assert_eq!(m.view.as_ref().unwrap()["ids"], json!([link]));
    let previous = m.view.clone();
    generation.store(2, Ordering::Relaxed);
    assert!(!m.query("discard", "all", 12, 1, &generation));
    assert_eq!(m.view, previous);
    assert_eq!(m.version, version);
}

#[test]
fn previews_and_character_counts_are_cached_off_the_ui_thread() {
    let mut m = Model::new(vec![]);
    let text = format!("  Olá\n\n🙂\t mundo{}", " text".repeat(100000));
    m.insert(entry(&text, now(), false));
    let e = &m.items[0];
    assert!(e.summary.starts_with(" Olá 🙂 mundo"));
    assert!(e.summary.encode_utf16().count() <= 160);
    assert_eq!(e.characters, text.chars().count());
    let packet: Value = serde_json::from_str(&m.packet(&HashMap::new(), &None)).unwrap();
    assert_eq!(packet["upsert"][0]["preview"], e.summary);
    assert_eq!(packet["upsert"][0]["characters"], e.characters);
    let original = e.summary.clone();
    let id = e.id.clone();
    m.act("pin", &id, true);
    assert_eq!(m.items[0].summary, original);
}
#[test]
fn pin_use_and_repeat_copy_do_not_resend_immutable_text() {
    let mut m = Model::new(vec![]);
    m.insert(entry(&"x".repeat(ITEM_BYTES), now(), false));
    let id = m.items[0].id.clone();
    let baseline = HashMap::from([(id.clone(), m.items[0].revision)]);
    m.act("pin", &id, true);
    let packet = m.packet(&baseline, &None);
    assert!(packet.len() < 2048);
    let value: Value = serde_json::from_str(&packet).unwrap();
    assert!(value["upsert"][0].get("text").is_none());
    assert_eq!(value["upsert"][0]["pinned"], true);
    m.act("use", &id, true);
    assert!(m.packet(&baseline, &None).len() < 2048);
    m.insert(entry(&"x".repeat(ITEM_BYTES), now(), false));
    assert_eq!(m.items[0].id, id);
    assert!(m.packet(&baseline, &None).len() < 2048);
}

#[test]
fn configured_five_thousand_items_and_pins_fit_within_safety_bounds() {
    let c = Config::parse(&json!({"max": 5000, "expireDays": 0}));
    assert_eq!(c.max, 5000);
    let mut items: Vec<_> = (0..5001)
        .map(|i| entry(&format!("copy {i}"), now(), false))
        .collect();
    items.push(entry("pinned", now(), true));
    let mut m = Model::new(items);
    m.prune(&c, now());
    assert_eq!(m.items.len(), 5001);
    assert!(m.items.iter().any(|e| e.pinned));
    let generation = AtomicU64::new(1);
    assert!(m.query("", "all", MAX_ITEMS, 1, &generation));
    assert_eq!(
        m.view.as_ref().unwrap()["ids"].as_array().unwrap().len(),
        5001
    );
}

#[test]
fn escaped_text_cannot_replace_a_readable_index_with_an_oversized_one() {
    let t = Temp::new();
    let store = store::Store::new(&t.0, None);
    store.save(&[entry("saved", now(), false)]).unwrap();
    let path = store.dir.join("history.json");
    let before = fs::read(&path).unwrap();
    let expanded: Vec<_> = (0..6)
        .map(|i| entry(&format!("{i}{}", "\0".repeat(ITEM_BYTES - 1)), now(), false))
        .collect();
    assert!(store.save(&expanded).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(fs::read_dir(&store.dir).unwrap().count(), 1);
}
