use super::{
    host::Host,
    i18n::I18n,
    style::{diff, normalize},
    NativeApp,
};
use crate::css_style::color;
use crate::runtime::native::{self as scene, Prop};
use serde_json::{json, Map, Value};
use std::{cell::RefCell, rc::Rc};

#[test]
fn styles_expand_shorthands_and_colors_like_style_js() {
    assert_eq!(color(&json!("#0a84ff")), 0xff0a84ff);
    assert_eq!(color(&json!("#0000001f")), 0x1f000000);
    assert_eq!(color(&json!("#fff")), 0xffffffff);
    assert_eq!(color(&json!("rgba(255, 0, 0, 0.5)")), 0x80ff0000);
    assert_eq!(color(&json!("transparent")), 0);
    let p = normalize(&json!([
        {"padding":"4 8","flex":1,"border":"1px solid #ff0000","boxShadow":"0 2px 6px rgba(0,0,0,0.5)"},
        {"paddingHorizontal":10,"fontWeight":"bold","width":"50%","height":"auto","opacity":"0.5"}
    ]));
    let n = |k: &str| p.get(k).cloned();
    assert_eq!(n("paddingTop"), Some(Prop::Num(4.)));
    assert_eq!(n("paddingLeft"), Some(Prop::Num(10.)), "later objects win");
    assert_eq!(n("paddingRight"), Some(Prop::Num(10.)));
    assert_eq!(n("flexGrow"), Some(Prop::Num(1.)));
    assert_eq!(n("flexBasis"), Some(Prop::Num(0.)));
    assert_eq!(n("borderWidth"), Some(Prop::Num(1.)));
    assert_eq!(n("borderColor"), Some(Prop::Num(f64::from(0xffff0000u32))));
    assert_eq!(n("shadowBlur"), Some(Prop::Num(6.)));
    assert_eq!(n("shadowOffsetY"), Some(Prop::Num(2.)));
    assert_eq!(n("shadowColor"), Some(Prop::Num(f64::from(0x80000000u32))));
    assert_eq!(n("fontWeight"), Some(Prop::Num(700.)));
    assert_eq!(n("width"), Some(Prop::Str("50%".into())));
    assert_eq!(n("height"), Some(Prop::Str("auto".into())));
    assert_eq!(n("opacity"), Some(Prop::Num(0.5)));
    assert!(matches!(
        normalize(&json!({"background":"linear-gradient(to right, #000, #fff)"})).get("gradient"),
        Some(Prop::Gradient(a, from, to)) if *a == 90. && *from == f64::from(0xff000000u32) && *to == f64::from(0xffffffffu32)
    ));
    // Only removed and changed properties are sent.
    let before = normalize(&json!({"opacity":1,"gap":4}));
    let after = normalize(&json!({"opacity":0.5}));
    let d = diff(&before, &after);
    assert!(d.contains(&("gap", None)));
    assert!(d.contains(&("opacity", Some(&Prop::Num(0.5)))));
    assert_eq!(d.len(), 2);
}

struct Locales(std::path::PathBuf);
impl Drop for Locales {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn locales() -> Locales {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "axia-native-view-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    for (path, text) in [
        ("locales/en/meta.json", r#"{"plural":"one-other"}"#),
        ("locales/pt/meta.json", r#"{"plural":"zero-one-other"}"#),
        (
            "locales/en/demo.json",
            r#"{"hello":"Hello {name}","items":{"one":"{count} item","other":"{count} items"},"only":"English"}"#,
        ),
        (
            "locales/pt/demo.json",
            r#"{"hello":"Olá {name}","items":{"one":"{count} item","other":"{count} itens"}}"#,
        ),
        (
            "locales/pt/errors.json",
            r#"{"demo.failed":"Falhou: {reason}","io.lost":"perdido"}"#,
        ),
    ] {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    Locales(root)
}

#[test]
fn captions_translate_with_plurals_fallback_parameters_and_errors() {
    let dir = locales();
    let mut i = I18n::new(dir.0.to_str().unwrap(), None);
    i.lang = "pt".into();
    assert_eq!(
        i.caption(&json!({"key":"demo.hello","params":{"name":"Ana"}})),
        "Olá Ana"
    );
    // Portuguese counts zero as singular.
    assert_eq!(
        i.caption(&json!({"key":"demo.items","params":{"count":0}})),
        "0 item"
    );
    assert_eq!(
        i.caption(&json!({"key":"demo.items","params":{"count":3}})),
        "3 itens"
    );
    assert_eq!(
        i.caption(&json!({"key":"demo.only"})),
        "English",
        "English fallback"
    );
    assert_eq!(i.caption(&json!({"key":"demo.missing"})), "demo.missing");
    assert_eq!(
        i.caption(&json!({"key":"demo.hello","params":{"name":{"key":"demo.only"}}})),
        "Olá English",
        "nested captions"
    );
    assert_eq!(
        i.caption(&json!({"error":"demo.failed reason=io.lost"})),
        "Falhou: perdido"
    );
    assert_eq!(i.caption(&json!({"error":"not a code"})), "not a code");
    assert_eq!(i.caption(&json!(42)), "42");
    i.lang = "en".into();
    assert_eq!(
        i.caption(&json!({"key":"demo.items","params":{"count":0}})),
        "0 items"
    );
    // Ids cannot leave the catalogs.
    i.lang = "../x".into();
    assert_eq!(i.caption(&json!({"key":"demo.only"})), "English");
}

/// A compiled app that shows a counter, a button, a field and a scroll view.
#[derive(Default)]
struct Demo {
    log: Rc<RefCell<Vec<Value>>>,
    count: i32,
    query: String,
}
impl NativeApp for Demo {
    fn request(&mut self, input: &Value) -> Value {
        self.log.borrow_mut().push(input.clone());
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => json!({"title":"Demo","tree":{"kind":"view","style":{"flex":1},"children":[
                {"kind":"text","label":format!("count {}", self.count)},
                {"kind":"button","label":"More","action":["more",""]},
                {"kind":"input","value":self.query,"placeholder":"Search"},
                {"kind":"scroll","children":[{"kind":"view","key":"row","events":{"onClick":["open","row"]},
                    "children":[{"kind":"text","label":"Row"}]}]},
                {"kind":"appIcon","app":"x"}
            ]}}),
            "more" => {
                self.count += 1;
                json!({"effect":{"kind":"openApp","app":"files"}})
            }
            "query" => {
                self.query = input["key"].as_str().unwrap_or("").into();
                json!({})
            }
            _ => json!({}),
        }
    }
    fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
        None
    }
}
fn texts(host: &Host) -> String {
    host.text()
}
/// The deepest node under `ancestor` whose own text is `label`.
fn node_with_text(root: i32, label: &str) -> i32 {
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if scene::gather_text(n) == label && n != root {
            let mut c = n;
            // descend to the text node itself
            loop {
                let kids = children(c);
                match kids.iter().find(|k| scene::gather_text(**k) == label) {
                    Some(k) => c = *k,
                    None => return c,
                }
            }
        }
        stack.extend(children(n));
    }
    0
}
fn children(node: i32) -> Vec<i32> {
    // Read the same borrowed sibling links as the renderer. Avoid scanning
    // thousands of unused IDs for every text lookup (especially under Miri).
    let mut out = Vec::new();
    unsafe {
        let parent = crate::runtime::node_get(node);
        if parent.is_null() {
            return out;
        }
        let mut child = (*parent).first;
        while !child.is_null() {
            out.push((*child).id);
            child = (*child).next;
        }
    }
    out
}

#[test]
fn a_mounted_view_reconciles_in_place_handles_clicks_keys_and_unmounts() {
    let mount = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(Vec::new()));
    let app = Demo {
        log: log.clone(),
        ..Default::default()
    };
    let mut host = Host::new(mount, Box::new(app), "", None);
    host.facts(
        json!({"args":{},"context":{"theme":{"mode":"dark","accent":"#0a84ff"}},"refreshMs":1000}),
    );
    assert!(texts(&host).contains("count 0"), "{}", texts(&host));
    assert!(texts(&host).contains("More"));
    assert!(texts(&host).contains("Search"), "placeholder");
    assert!(texts(&host).contains("Row"));
    let effects = host.take().unwrap();
    assert_eq!(
        effects[0]["kind"], "title",
        "the window title is the shell's"
    );
    let counter = node_with_text(mount, "count 0");
    assert_ne!(counter, 0);

    // Press and release on the button: the app runs the command and the tree updates in place.
    let button = node_with_text(mount, "More");
    assert_eq!(host.pointer("down", button, 0., 0., 0), Some(mount));
    assert_eq!(host.pointer("up", button, 0., 0., 0), Some(mount));
    assert!(log.borrow().iter().any(|v| v["cmd"] == "more"));
    assert!(texts(&host).contains("count 1"));
    assert_eq!(
        node_with_text(mount, "count 1"),
        counter,
        "nodes are reused, not recreated"
    );
    let effects = host.take().unwrap();
    assert!(
        effects
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "openApp"),
        "{effects}"
    );
    assert!(!host.pending());

    // A press that ends elsewhere is not a click; events outside go to JS unchanged.
    let outside = scene::create(scene::VIEW);
    host.pointer("down", button, 0., 0., 0);
    assert_eq!(host.pointer("up", outside, 0., 0., 0), None);
    assert!(texts(&host).contains("count 1"));
    // A clickable view (the row) runs its onClick action.
    let row = node_with_text(mount, "Row");
    host.pointer("down", row, 0., 0., 0);
    host.pointer("up", row, 0., 0., 0);
    assert!(log
        .borrow()
        .iter()
        .any(|v| v["cmd"] == "open" && v["key"] == "row"));

    // Keys reach the app only while its field has focus and the window is focused.
    assert!(!host.key("a", "a", 0));
    let field = node_with_text(mount, "Search");
    host.pointer("down", field, 0., 0., 0);
    assert!(host.key("a", "a", 0));
    assert!(host.key("b", "b", 0));
    assert!(texts(&host).contains("ab"));
    assert!(host.key("Backspace", "", 0));
    assert!(texts(&host).contains('a') && !texts(&host).contains("ab"));
    assert!(!host.key("t", "t", 2), "Ctrl shortcuts pass to the shell");
    assert!(host.key("v", "", 2), "paste is asked of the shell");
    assert_eq!(host.take().unwrap()[0]["kind"], "paste");
    host.paste("x\ny");
    assert!(texts(&host).contains("ax y"));
    host.window_focus(false);
    assert!(
        !host.key("c", "c", 0),
        "an unfocused window keeps no keyboard"
    );
    host.window_focus(true);
    host.pointer("down", field, 0., 0., 0);
    host.pointer("down", outside, 0., 0., 0);
    assert!(!host.key("c", "c", 0), "a press elsewhere blurs the field");

    // Unmounting closes the app and removes its nodes, not the mount node.
    drop(host);
    assert!(log.borrow().last().unwrap()["cmd"] == "close");
    assert!(children(mount).is_empty());
    assert!(scene::exists(mount));
    scene::destroy(outside);
    scene::destroy(mount);
}

#[test]
fn icons_render_once_without_timers_or_input() {
    let mount = scene::create(scene::VIEW);
    struct Icon;
    impl NativeApp for Icon {
        fn request(&mut self, input: &Value) -> Value {
            if input["cmd"] == "close" {
                return Value::Null;
            }
            assert_eq!(input["cmd"], "icon");
            json!({"kind":"view","style":{"width":input["size"],"height":input["size"],"backgroundColor":"#30a448"}})
        }
        fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
            None
        }
    }
    let mut host = Host::new(mount, Box::new(Icon), "", None);
    host.facts(json!({"icon":32}));
    assert_eq!(children(mount).len(), 1);
    assert!(host.deadline().is_none(), "an icon never wakes the loop");
    assert_eq!(
        host.pointer("down", children(mount)[0], 0., 0., 0),
        Some(mount)
    );
    drop(host);
    scene::destroy(mount);
    let _ = Map::<String, Value>::new();
}

#[test]
fn calculator_mounts_keys_and_pointer_feedback_directly_and_releases_its_nodes() {
    let dir = locales();
    std::fs::write(
        dir.0.join("locales/en/calculator.json"),
        r#"{"error":"Error"}"#,
    )
    .unwrap();
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(
        mount,
        super::app("calculator").unwrap(),
        dir.0.to_str().unwrap(),
        None,
    );
    host.facts(json!({"lang":"en","context":{"decimal":".","theme":{"mode":"dark","text":"#fff","textDim":"#aaa","accent":"#3584e4"}}}));
    let key = node_with_text(mount, "7");
    assert_ne!(key, 0);
    let button = scene::parent(key);
    let opacity = || unsafe { (*crate::runtime::node_get(button)).s.opacity };
    assert_eq!(opacity(), 1.);
    host.pointer("move", key, 0., 0., 0);
    assert_eq!(opacity(), 0.88);
    host.pointer("down", key, 0., 0., 0);
    assert_eq!(opacity(), 0.7);
    host.pointer("up", key, 0., 0., 0);
    assert_eq!(opacity(), 0.88);
    assert!(texts(&host).lines().any(|t| t == "7"));
    // The key's nodes survive updates, hover and result changes.
    assert!(scene::exists(key));
    assert_eq!(scene::parent(key), button);
    for (name, chr) in [("+", "+"), ("3", "3"), ("Enter", "")] {
        assert!(host.key(name, chr, 0));
    }
    assert!(texts(&host).lines().any(|t| t == "10"));
    assert!(!host.key("1", "1", 2), "Ctrl shortcuts pass through");
    assert!(!host.key("1", "1", 16), "Super shortcuts pass through");
    assert!(!host.key("x", "x", 0));
    host.pointer("down", key, 0., 0., 0);
    host.window_focus(false);
    assert_eq!(opacity(), 1., "losing focus clears hover and pressure");
    assert!(!host.key("Escape", "", 0));
    host.pointer("up", key, 0., 0., 0);
    assert!(
        texts(&host).lines().any(|t| t == "10"),
        "focus loss cancels the pending click"
    );
    host.window_focus(true);
    host.key("Escape", "", 0);
    let outside = scene::create(scene::VIEW);
    host.pointer("down", key, 0., 0., 0);
    host.pointer("move", outside, 0., 0., 0);
    host.pointer("up", outside, 0., 0., 0);
    assert_eq!(opacity(), 1.);
    host.key("Enter", "", 0);
    assert!(
        texts(&host).contains("Error"),
        "cancelled click left the expression empty"
    );
    host.key("Escape", "", 0);
    for (name, chr) in [
        ("1", "1"),
        (",", ","),
        ("5", "5"),
        ("*", "*"),
        ("2", "2"),
        ("Enter", ""),
    ] {
        assert!(host.key(name, chr, 0));
    }
    assert!(texts(&host).lines().any(|t| t == "3"));
    assert!(
        host.deadline().is_none(),
        "calculator does not poll or blink at idle"
    );
    drop(host);
    assert!(children(mount).is_empty());
    scene::destroy(outside);
    scene::destroy(mount);
}

#[test]
fn calculator_icons_and_unfocused_windows_never_take_arithmetic_keys() {
    let a = scene::create(scene::VIEW);
    let b = scene::create(scene::VIEW);
    let icon = scene::create(scene::VIEW);
    let mut first = Host::new(a, super::app("calculator").unwrap(), "", None);
    let mut second = Host::new(b, super::app("calculator").unwrap(), "", None);
    let mut picture = Host::new(icon, super::app("calculator").unwrap(), "", None);
    first.facts(json!({"focused":true,"context":{"decimal":","}}));
    second.facts(json!({"focused":false,"context":{"decimal":"."}}));
    picture.facts(json!({"icon":32}));
    assert!(!picture.key("1", "1", 0));
    assert!(!second.key("1", "1", 0));
    for key in ["1", ".", "5", "Enter"] {
        assert!(first.key(key, key, 0));
    }
    assert!(texts(&first).contains("1,5"));
    assert!(!texts(&second).contains("1.5"));
    first.facts(json!({"focused":false,"context":{"decimal":","}}));
    second.window_focus(true);
    assert!(!first.key("Escape", "", 0));
    assert!(second.key("9", "9", 0));
    assert!(second.key("Enter", "", 0));
    assert!(texts(&second).lines().any(|t| t == "9"));
    assert!(texts(&first).contains("1,5"));
    assert!(!children(icon).is_empty());
    assert!(picture.deadline().is_none());
    drop((first, second, picture));
    for mount in [a, b, icon] {
        assert!(children(mount).is_empty());
        scene::destroy(mount);
    }
}

struct CountedApp {
    app: Box<dyn NativeApp>,
    syncs: Rc<std::cell::Cell<usize>>,
}
impl NativeApp for CountedApp {
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] == "sync" || input["cmd"] == "icon" {
            self.syncs.set(self.syncs.get() + 1);
        }
        self.app.request(input)
    }
    fn pixels(&self, id: &str) -> Option<(usize, usize, &[u8])> {
        self.app.pixels(id)
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        self.app.key(key, chr, flags)
    }
}
fn counted_calculator() -> (Host, i32, Rc<std::cell::Cell<usize>>) {
    let mount = scene::create(scene::VIEW);
    let syncs = Rc::new(std::cell::Cell::new(0));
    let app = CountedApp {
        app: super::app("calculator").unwrap(),
        syncs: syncs.clone(),
    };
    let mut host = Host::new(mount, Box::new(app), "", None);
    host.facts(json!({"focused":true,"context":{"theme":{"mode":"dark"}}}));
    (host, mount, syncs)
}

#[test]
fn calculator_keeps_the_primary_press_when_another_mouse_button_is_used() {
    let (mut host, mount, syncs) = counted_calculator();
    let seven = node_with_text(mount, "7");
    let button = scene::parent(seven);
    let opacity = || unsafe { (*crate::runtime::node_get(button)).s.opacity };
    host.pointer("move", seven, 0., 0., 0);
    host.pointer("down", seven, 0., 0., 0);
    assert_eq!(opacity(), 0.7);
    let before = syncs.get();
    // A second button's release (or press) cannot end the held left click.
    host.pointer("up", seven, 0., 0., 2);
    host.pointer("down", seven, 0., 0., 2);
    host.pointer("up", seven, 0., 0., 2);
    assert_eq!(opacity(), 0.7);
    assert_eq!(syncs.get(), before);
    host.pointer("up", seven, 0., 0., 0);
    host.key("Enter", "", 0);
    assert_eq!(
        texts(&host).lines().filter(|text| *text == "7").count(),
        3,
        "expression, result and key label all show 7"
    );
    assert!(!texts(&host).lines().any(|text| text == "77"));
    drop(host);
    assert!(children(mount).is_empty());
    scene::destroy(mount);
}

#[test]
fn calculator_reconciles_once_per_pointer_event_and_focus_fact_update() {
    let (mut host, mount, syncs) = counted_calculator();
    let seven = node_with_text(mount, "7");
    let eight = node_with_text(mount, "8");
    assert_eq!(syncs.get(), 1);
    for (kind, node) in [
        ("move", seven),
        ("move", eight),
        ("down", eight),
        ("up", eight),
    ] {
        let before = syncs.get();
        host.pointer(kind, node, 0., 0., 0);
        assert_eq!(syncs.get(), before + 1, "one tree for {kind}");
    }
    host.pointer("move", seven, 0., 0., 0);
    host.pointer("down", seven, 0., 0., 0);
    let before = syncs.get();
    host.facts(json!({"focused":false,"context":{"theme":{"mode":"dark"}}}));
    assert_eq!(
        syncs.get(),
        before + 1,
        "focus cleanup and new facts share a tree"
    );
    let button = scene::parent(seven);
    assert_eq!(unsafe { (*crate::runtime::node_get(button)).s.opacity }, 1.);
    host.pointer("up", seven, 0., 0., 0);
    assert_eq!(syncs.get(), before + 1, "cancelled click does not rebuild");
    host.window_focus(true);
    host.key("Enter", "", 0);
    assert_eq!(
        texts(&host).lines().filter(|text| *text == "8").count(),
        3,
        "expression, result and key label all show 8"
    );
    assert!(!texts(&host).lines().any(|text| text == "87"));
    drop(host);
    scene::destroy(mount);
}

#[test]
fn calculator_icon_is_not_rebuilt_on_each_frame_after_layout() {
    let mount = scene::create(scene::VIEW);
    let calls = Rc::new(std::cell::Cell::new(0));
    let app = CountedApp {
        app: super::app("calculator").unwrap(),
        syncs: calls.clone(),
    };
    let mut host = Host::new(mount, Box::new(app), "", None);
    host.facts(json!({"icon":32,"refreshMs":250}));
    assert_eq!(calls.get(), 1);
    unsafe {
        // The compositor lays out the shell's mount after the initial tree.
        let node = crate::runtime::node_get(mount);
        (*node).w = 32.;
        (*node).h = 32.;
    }
    for now in [0., 16., 32., 1000.] {
        host.tick(now);
    }
    assert_eq!(
        calls.get(),
        1,
        "an unchanged icon does not rebuild per frame"
    );
    assert!(host.deadline().is_none());
    host.facts(json!({"icon":48}));
    assert_eq!(calls.get(), 2, "a new explicit icon size is rendered");
    drop(host);
    assert!(children(mount).is_empty());
    scene::destroy(mount);
}

#[test]
fn about_report_button_uses_native_input_and_small_control_metrics() {
    let dir = locales();
    std::fs::write(
        dir.0.join("locales/en/about.json"),
        r#"{"systemReport":"System Report…"}"#,
    )
    .unwrap();
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(
        mount,
        super::app("about").unwrap(),
        dir.0.to_str().unwrap(),
        None,
    );
    host.facts(json!({"context":{"theme":{"text":"#fff","control":"#333","controlRadiusSmall":4,"buttonBorder":true,"separator":"#555"}}}));
    let label = node_with_text(mount, "System Report…");
    assert_ne!(label, 0);
    let button = scene::parent(label);
    assert_eq!(
        unsafe { (*crate::runtime::node_get(button)).s.height.v },
        22.
    );
    host.take();
    host.pointer("down", button, 0., 0., 0);
    host.pointer("up", button, 0., 0., 0);
    assert!(host
        .take()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|effect| effect == &json!({"kind":"openApp","app":"sysinfo"})));
    drop(host);
    assert!(children(mount).is_empty());
    scene::destroy(mount);
}

#[test]
fn characters_selects_previews_and_copies_through_native_pointer_input() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(
        mount,
        super::app("characters").unwrap(),
        &assets.to_string_lossy(),
        None,
    );
    host.facts(json!({"lang":"en","settings":{"emojiRecent":["😺"],"emojiTone":0},"context":{"theme":{"mode":"dark","text":"#fff","textDim":"#aaa","accent":"#0a84ff","controlRadius":7,"controlRadiusSmall":6}}}));
    let cat = node_with_text(mount, "😺");
    assert_ne!(cat, 0);
    host.pointer("down", cat, 0., 0., 0);
    host.pointer("up", cat, 0., 0., 0);
    assert!(host.text().contains("U+1F63A"));
    assert!(host
        .take()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "settings"));
    let copy = node_with_text(mount, "Copy");
    assert_ne!(copy, 0);
    host.pointer("down", copy, 0., 0., 0);
    host.pointer("up", copy, 0., 0., 0);
    assert!(host
        .take()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e == &json!({"host":true,"kind":"copy","text":"😺"})));
    let search = node_with_text(mount, "Search emoji");
    assert_ne!(search, 0);
    host.pointer("down", search, 0., 0., 0);
    host.pointer("up", search, 0., 0., 0);
    assert!(host.key("x", "x", 0));
    assert_ne!(node_with_text(mount, "x"), 0);
    assert!(host.key("Escape", "", 0));
    assert_eq!(node_with_text(mount, "x"), 0);
    drop(host);
    assert!(children(mount).is_empty());
    scene::destroy(mount);
}

#[test]
fn clock_native_input_starts_pauses_and_saves_alarm_with_enter() {
    crate::clock::with(|s| *s = Default::default());
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(
        mount,
        super::app("clock").unwrap(),
        &assets.to_string_lossy(),
        None,
    );
    let mut facts = json!({"lang":"en","args":{"tab":"timer"},"settings":{"clock":{"hour24":"24"}},"context":{"theme":{"mode":"dark","text":"#fff","textDim":"#aaa","accent":"#0a84ff","control":"#333","controlRadius":7,"controlRadiusSmall":6}}});
    host.facts(facts.clone());
    let preset = node_with_text(mount, "3 min");
    assert_ne!(preset, 0);
    host.pointer("down", preset, 0., 0., 0);
    host.pointer("up", preset, 0., 0., 0);
    assert_eq!(crate::clock::with(|s| s.timer["duration"].clone()), 180000);
    assert!(host.text().contains("Pause"));
    assert!(host.deadline().is_some());
    let pause = node_with_text(mount, "Pause");
    host.pointer("down", pause, 0., 0., 0);
    host.pointer("up", pause, 0., 0., 0);
    assert_eq!(crate::clock::with(|s| s.timer["state"].clone()), "paused");
    assert!(host.deadline().is_none());
    facts["args"] = json!({"tab":"alarms"});
    host.facts(facts);
    let add = node_with_text(mount, "+");
    assert_ne!(add, 0);
    host.pointer("down", add, 0., 0., 0);
    host.pointer("up", add, 0., 0., 0);
    let label = node_with_text(mount, "Label (optional)");
    assert_ne!(label, 0);
    host.pointer("down", label, 0., 0., 0);
    host.pointer("up", label, 0., 0., 0);
    assert!(host.key("N", "Native alarm", 0));
    assert!(host.key("Enter", "", 0));
    assert_eq!(
        crate::clock::with(|s| s.config["alarms"][0]["label"].clone()),
        "Native alarm"
    );
    drop(host);
    assert!(children(mount).is_empty());
    scene::destroy(mount);
}

#[test]
fn hover_only_parent_keeps_hover_above_a_clickable_child() {
    struct Hover {
        log: Rc<RefCell<Vec<String>>>,
    }
    impl NativeApp for Hover {
        fn request(&mut self, input: &Value) -> Value {
            let cmd = input["cmd"].as_str().unwrap_or("");
            if cmd == "sync" {
                return json!({"tree":{"kind":"view","events":{"onMouseEnter":["enter",""],"onMouseLeave":["leave",""]},"children":[{"kind":"text","label":"Row"},{"kind":"view","events":{"onClick":["remove",""]},"children":[{"kind":"text","label":"Remove"}]}]}});
            }
            self.log.borrow_mut().push(cmd.into());
            Value::Null
        }
        fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
            None
        }
    }
    let mount = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(vec![]));
    let mut host = Host::new(mount, Box::new(Hover { log: log.clone() }), "", None);
    host.facts(json!({"context":{}}));
    host.pointer("move", node_with_text(mount, "Row"), 0., 0., 0);
    host.pointer("move", node_with_text(mount, "Remove"), 0., 0., 0);
    assert_eq!(*log.borrow(), vec!["enter"]);
    let remove = node_with_text(mount, "Remove");
    host.pointer("down", remove, 0., 0., 0);
    host.pointer("up", remove, 0., 0., 0);
    assert!(log.borrow().contains(&"remove".into()));
    drop(host);
    scene::destroy(mount);
}

#[test]
fn help_native_search_enter_escape_tour_and_open_app_use_scene_input() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(mount, super::app("help").unwrap(), assets.to_str().unwrap(), None);
    let mut facts = json!({"lang":"en","focused":true,"args":{"article":"welcome"},"width":860,
        "registry":[{"id":"help","name":"Help","icon":true},{"id":"clock","name":"Clock","icon":true}],
        "context":{"theme":{"text":"#fff","textDim":"#aaa","accent":"#3584e4","control":"#333","sidebarBg":"#222","separator":"#444","controlRadius":6,"controlRadiusSmall":4}}});
    host.facts(facts.clone());
    let click = |host: &mut Host, label: &str| {
        let node = node_with_text(mount, label);
        assert_ne!(node, 0, "missing {label}");
        host.pointer("down", node, 0., 0., 0);
        host.pointer("up", node, 0., 0., 0);
    };
    let field = scene::parent(node_with_text(mount,"Search help"));
    click(&mut host,"Search help");
    assert!(host.key("c","clock",0));
    assert!(host.key("Enter","",0));
    assert!(host.text().contains("Open Clock"));
    host.take();click(&mut host,"Open Clock");
    assert!(host.take().unwrap().as_array().unwrap().iter().any(|e| e["kind"]=="openApp"&&e["app"]=="clock"));
    host.pointer("down",field,0.,0.,0);host.pointer("up",field,0.,0.,0);assert!(host.key("Escape","",0));
    assert!(host.text().contains("Getting started"));
    facts["args"]=json!({"tour":true,"nonce":1});host.facts(facts);
    for _ in 0..5 { click(&mut host,"Next"); }
    assert!(host.text().contains("Always at hand"));
    host.take();click(&mut host,"Get started");
    assert!(host.take().unwrap().as_array().unwrap().iter().any(|e|e["kind"]=="settings"&&e["patch"]["welcomeShown"]==true));
    assert!(!host.text().contains("Get started"));
    drop(host);assert!(children(mount).is_empty());scene::destroy(mount);
}

#[test]
fn force_quit_native_rows_disabled_button_and_error_result_use_scene_input() {
    let assets=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let mount=scene::create(scene::VIEW);
    let mut host=Host::new(mount,super::app("forcequit").unwrap(),assets.to_str().unwrap(),None);
    let mut facts=json!({"lang":"en","focused":true,"windows":[
        {"id":"notes#1","key":"notes","name":"Notes","iconData":{"kind":"app","id":"notes"}},
        {"id":"notes#2","key":"notes","name":"Notes","iconData":{"kind":"app","id":"notes"}}],
        "context":{"theme":{"text":"#fff","textDim":"#aaa","accent":"#3584e4","control":"#333","separator":"#444","controlRadius":6}}});
    host.facts(facts.clone());
    let click=|host:&mut Host,label:&str| {
        let node=host.find_text(label);assert_ne!(node,0,"missing {label}");
        host.pointer("down",node,0.,0.,0);host.pointer("up",node,0.,0.,0);
    };
    click(&mut host,"Force Quit");assert!(host.take().is_none());
    assert!(host.text().contains("2 windows"));
    click(&mut host,"Notes");click(&mut host,"Force Quit");
    assert_eq!(host.take(),Some(json!([{"kind":"forceQuit","key":"notes"}])));
    host.result(json!("app.forceQuitX11"));assert!(host.text().contains("X11"));
    facts["windows"]=json!([]);host.facts(facts);
    assert_eq!(host.find_text("Notes"),0);click(&mut host,"Force Quit");assert!(host.take().is_none());
    assert_eq!(host.find_text(""),0);
    drop(host);assert!(children(mount).is_empty());scene::destroy(mount);
}

#[test]
fn app_about_native_metadata_title_and_target_changes_do_not_retain_old_content() {
    let assets=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let mount=scene::create(scene::VIEW);
    let mut host=Host::new(mount,super::app("appabout").unwrap(),assets.to_str().unwrap(),None);
    let mut facts=json!({"lang":"en","args":{"key":"calculator"},
        "catalog":[{"id":"calculator","name":"Calculator","icon":true}],
        "registry":[{"id":"calculator","version":"0.9.0","system":true}],
        "context":{"theme":{"text":"#fff","textDim":"#aaa"}}});
    host.facts(facts.clone());
    assert_ne!(host.find_text("Calculator"),0);assert_ne!(host.find_text("Version 0.9.0"),0);
    assert!(host.take().unwrap().as_array().unwrap().iter().any(|e|e["title"]=="About Calculator"));
    facts["args"]=json!({"key":"missing"});host.facts(facts);
    assert_eq!(host.find_text("Calculator"),0);assert_eq!(host.find_text("Version 0.9.0"),0);
    assert!(host.take().unwrap().as_array().unwrap().iter().any(|e|e["title"]=="About missing"));
    drop(host);assert!(children(mount).is_empty());scene::destroy(mount);
}

#[test]
fn a_layered_view_mounts_in_the_shell_layer_and_receives_pointer_events() {
    struct Pop(Rc<RefCell<Vec<Value>>>);
    impl NativeApp for Pop {
        fn request(&mut self, input: &Value) -> Value {
            if input["cmd"] == "sync" {
                return json!({"tree":{"kind":"view","children":[
                    {"kind":"text","label":"Window"},
                    {"kind":"view","layer":"popover","style":{"position":"absolute"},"children":[
                        {"kind":"view","events":{"onClick":["pick"]},"children":[{"kind":"text","label":"Item"}]}]}]}});
            }
            self.0.borrow_mut().push(input.clone());
            json!({})
        }
        fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
            None
        }
    }
    let mount = scene::create(scene::VIEW);
    let layer = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = Host::new(mount, Box::new(Pop(log.clone())), "", None);
    host.facts(json!({"layers":{"popover":layer}}));
    let item = node_with_text(layer, "Item");
    assert_ne!(item, 0, "the view is in the layer");
    assert!(scene::within(item, layer) && !scene::within(item, mount));
    host.pointer("move", item, 1., 1., 0);
    host.pointer("down", item, 1., 1., 0);
    host.pointer("up", item, 1., 1., 0);
    assert!(log.borrow().iter().any(|v| v["cmd"] == "pick"), "a click in the layer reaches the app");
    drop(host);
    assert!(!scene::exists(item), "its layered nodes go with the view");
    // without a layer named, the same view stays in the window
    let mount2 = scene::create(scene::VIEW);
    let mut inline = Host::new(mount2, Box::new(Pop(log.clone())), "", None);
    inline.facts(json!({}));
    let item2 = node_with_text(mount2, "Item");
    assert_ne!(item2, 0);
    assert!(scene::within(item2, mount2));
    drop(inline);
    scene::destroy(layer);
    scene::destroy(mount);
    scene::destroy(mount2);
}

#[test]
fn a_view_takes_its_hover_style_and_a_layer_is_dismissed_by_a_press_outside() {
    struct Menu(Rc<RefCell<Vec<Value>>>);
    impl NativeApp for Menu {
        fn request(&mut self, input: &Value) -> Value {
            if input["cmd"] == "sync" {
                return json!({"tree":{"kind":"view","children":[
                    {"kind":"text","label":"Window"},
                    {"kind":"view","layer":"popover","anchor":"mount","style":{"position":"absolute","left":10,"top":20},
                     "events":{"onDismiss":["close"]},"children":[
                        {"kind":"view","style":{"backgroundColor":"#00ff00"},"hoverStyle":{"backgroundColor":"#ff0000"},
                         "events":{"onClick":["pick"]},"children":[{"kind":"text","label":"Item"}]}]}]}});
            }
            self.0.borrow_mut().push(input.clone());
            json!({})
        }
        fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
            None
        }
    }
    let bg = |n: i32| unsafe { (*crate::runtime::node_get(n)).s.bg };
    let mount = scene::create(scene::VIEW);
    let layer = scene::create(scene::VIEW);
    let outside = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = Host::new(mount, Box::new(Menu(log.clone())), "", None);
    host.facts(json!({"layers":{"popover":layer}}));
    let row = scene::parent(node_with_text(layer, "Item"));
    assert_eq!(bg(row), 0xff00_ff00, "at rest");
    host.pointer("move", row, 1., 1., 0);
    assert_eq!(bg(row), 0xffff_0000, "hovered");
    host.pointer("move", outside, 1., 1., 0);
    assert_eq!(bg(row), 0xff00_ff00, "and back");
    host.pointer("down", row, 1., 1., 0);
    assert!(!log.borrow().iter().any(|v| v["cmd"] == "close"), "a press inside the layer keeps it");
    host.pointer("down", outside, 1., 1., 0);
    assert!(log.borrow().iter().any(|v| v["cmd"] == "close"), "a press outside asks the app to close it");
    drop(host);
    scene::destroy(layer);
    scene::destroy(mount);
    scene::destroy(outside);
}

#[test]
fn a_menu_node_is_drawn_in_the_layer_acts_on_its_rows_and_is_dismissed() {
    struct Pop(Rc<RefCell<Vec<Value>>>);
    impl NativeApp for Pop {
        fn request(&mut self, input: &Value) -> Value {
            if input["cmd"] == "sync" {
                return json!({"tree":{"kind":"view","children":[
                    {"kind":"text","label":"Window"},
                    {"kind":"menu","x":10,"y":20,"events":{"onDismiss":["closeContext"]},
                     "items":[{"label":"Open","action":["open"]},"-",{"label":"Trash","action":["trash"],"disabled":true}]}]}});
            }
            self.0.borrow_mut().push(input.clone());
            json!({})
        }
        fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
            None
        }
    }
    let bg = |n: i32| unsafe { (*crate::runtime::node_get(n)).s.bg };
    let mount = scene::create(scene::VIEW);
    let layer = scene::create(scene::VIEW);
    let outside = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = Host::new(mount, Box::new(Pop(log.clone())), "", None);
    host.facts(json!({"layers":{"popover":layer}}));
    let open = node_with_text(layer, "Open");
    assert_ne!(open, 0, "the menu's rows are in the layer");
    let row = scene::parent(open);
    host.pointer("move", row, 1., 1., 0);
    assert_eq!(bg(row), 0xff0a_84ff, "an enabled row lights up with the accent");
    host.pointer("down", row, 1., 1., 0);
    host.pointer("up", row, 1., 1., 0);
    assert!(log.borrow().iter().any(|v| v["cmd"] == "open"), "a click on a row runs its action");
    assert!(log.borrow().iter().any(|v| v["cmd"] == "menuHover"), "and the app hears about the hover");
    let trash = scene::parent(node_with_text(layer, "Trash"));
    host.pointer("move", trash, 1., 1., 0);
    host.pointer("down", trash, 1., 1., 0);
    host.pointer("up", trash, 1., 1., 0);
    assert!(!log.borrow().iter().any(|v| v["cmd"] == "trash"), "a disabled row does nothing");
    host.pointer("down", outside, 1., 1., 0);
    assert!(log.borrow().iter().any(|v| v["cmd"] == "closeContext"), "a press outside closes it");
    drop(host);
    scene::destroy(layer);
    scene::destroy(mount);
    scene::destroy(outside);
}

#[test]
fn a_tooltip_shows_after_the_pointer_rests_and_goes_with_a_press_or_leaving() {
    struct Tip;
    impl NativeApp for Tip {
        fn request(&mut self, input: &Value) -> Value {
            if input["cmd"] == "sync" {
                return json!({"tree":{"kind":"view","children":[
                    {"kind":"view","tooltip":"A long file name.txt","children":[{"kind":"text","label":"short"}]},
                    {"kind":"text","label":"plain"}]}});
            }
            json!({})
        }
        fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
            None
        }
    }
    let mount = scene::create(scene::VIEW);
    let layer = scene::create(scene::VIEW);
    let outside = scene::create(scene::VIEW);
    let mut host = Host::new(mount, Box::new(Tip), "", None);
    host.facts(json!({"layers":{"tooltip":layer}}));
    let short = node_with_text(mount, "short");
    let tile = scene::parent(short);
    host.pointer("move", short, 1., 1., 0);
    assert_eq!(node_with_text(layer, "A long file name.txt"), 0, "not at once");
    assert!(host.deadline().is_some(), "the host asks to be woken for it");
    host.tick(1e18);
    let bubble = node_with_text(layer, "A long file name.txt");
    assert_ne!(bubble, 0, "after the delay it is in the tooltip layer");
    host.pointer("down", tile, 1., 1., 0);
    assert_eq!(node_with_text(layer, "A long file name.txt"), 0, "a press hides it");
    host.pointer("up", tile, 1., 1., 0);
    host.pointer("move", outside, 1., 1., 0);
    host.pointer("move", short, 1., 1., 0);
    host.tick(1e18);
    assert_ne!(node_with_text(layer, "A long file name.txt"), 0);
    host.pointer("move", outside, 1., 1., 0);
    assert_eq!(node_with_text(layer, "A long file name.txt"), 0, "leaving hides it");
    host.pointer("move", short, 1., 1., 0);
    host.tick(1e18);
    drop(host);
    assert_eq!(node_with_text(layer, "A long file name.txt"), 0, "and the host takes it away with itself");
    scene::destroy(layer);
    scene::destroy(mount);
    scene::destroy(outside);
}

#[test]
fn a_native_field_edits_at_the_caret_and_starts_with_the_selection_it_asks_for() {
    struct Form(Rc<RefCell<String>>, Option<Value>);
    impl NativeApp for Form {
        fn request(&mut self, input: &Value) -> Value {
            match input["cmd"].as_str().unwrap_or("") {
                "sync" => {
                    let mut field = json!({"kind":"input","value":self.0.borrow().clone(),"autoFocus":true,"submit":["ok"]});
                    if let Some(sel) = &self.1 {
                        field["select"] = sel.clone();
                    }
                    json!({"tree":{"kind":"view","children":[field]}})
                }
                "query" => {
                    *self.0.borrow_mut() = input["key"].as_str().unwrap_or("").into();
                    json!({})
                }
                _ => json!({}),
            }
        }
        fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
            None
        }
    }
    let run = |initial: &'static str, select: Option<Value>, keys: &[(&str, &str, i32)]| -> String {
        let value = Rc::new(RefCell::new(initial.to_owned()));
        let mount = scene::create(scene::VIEW);
        let mut host = Host::new(mount, Box::new(Form(value.clone(), select)), "", None);
        host.facts(json!({}));
        for (key, chr, flags) in keys {
            assert!(host.key(key, chr, *flags), "{key} is the field's");
        }
        let out = value.borrow().clone();
        drop(host);
        scene::destroy(mount);
        out
    };
    // the caret starts at the end: type, move left, type in the middle, Delete and Backspace
    assert_eq!(run("abc", None, &[("d", "d", 0), ("ArrowLeft", "", 0), ("ArrowLeft", "", 0), ("x", "x", 0)]), "abxcd");
    assert_eq!(run("abc", None, &[("Home", "", 0), ("Delete", "", 0)]), "bc");
    assert_eq!(run("abc", None, &[("ArrowLeft", "", 0), ("Backspace", "", 0)]), "ac");
    // a rename: the name without its extension selected, typing replaces only that
    assert_eq!(run("report.final.pdf", Some(json!([0, 12])), &[("n", "n", 0), ("e", "e", 0)]), "ne.pdf");
    // Ctrl+A selects it all; Shift+Home selects to the start
    assert_eq!(run("abc", None, &[("a", "a", 2), ("z", "z", 0)]), "z");
    assert_eq!(run("abc", None, &[("Home", "", 1), ("z", "z", 0)]), "z");
    // the app's own shortcuts are not the field's
    let mount = scene::create(scene::VIEW);
    let mut host = Host::new(mount, Box::new(Form(Rc::new(RefCell::new("a".into())), None)), "", None);
    host.facts(json!({}));
    assert!(!host.key("s", "s", 2), "Ctrl+S passes");
    assert!(!host.key("f", "f", 4), "Alt+F passes");
    assert!(!host.key("F5", "", 0), "so does a key it has no use for");
    drop(host);
    scene::destroy(mount);
}

#[test]
fn pointer_actions_carry_position_and_modifiers_and_the_extra_events_reach_the_app() {
    struct Ev(Rc<RefCell<Vec<Value>>>);
    impl NativeApp for Ev {
        fn request(&mut self, input: &Value) -> Value {
            if input["cmd"] == "sync" {
                return json!({"tree":{"kind":"view","children":[{"kind":"view","events":{
                    "onClick":["click"],"onDoubleClick":["dbl"],"onContextMenu":["ctx"],"onWheel":["wheel"],"onMouseMove":["mv"]},
                    "children":[{"kind":"text","label":"Tile"}]}]}});
            }
            self.0.borrow_mut().push(input.clone());
            json!({})
        }
        fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
            None
        }
    }
    let mount = scene::create(scene::VIEW);
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = Host::new(mount, Box::new(Ev(log.clone())), "", None);
    host.facts(json!({}));
    let tile = node_with_text(mount, "Tile");
    let cmds = |name: &str| log.borrow().iter().filter(|v| v["cmd"] == name).cloned().collect::<Vec<_>>();
    super::axia_view_set_modifiers(2 | 1); // Ctrl and Shift held
    host.pointer("move", tile, 5., 6., 0);
    assert_eq!(cmds("mv").len(), 1, "a move reaches onMouseMove");
    host.pointer("down", tile, 5., 6., 0);
    host.pointer("up", tile, 5., 6., 0);
    let click = &cmds("click")[0];
    assert_eq!((click["ctrl"].clone(), click["shift"].clone(), click["alt"].clone()), (json!(true), json!(true), json!(false)));
    assert_eq!((click["button"].clone(), click["x"].is_number(), click["y"].is_number()), (json!(0), true, true));
    assert!(cmds("dbl").is_empty(), "one click is not a double click");
    host.pointer("down", tile, 5., 6., 0);
    host.pointer("up", tile, 5., 6., 0);
    assert_eq!(cmds("click").len(), 2);
    assert_eq!(cmds("dbl").len(), 1, "the second one soon after is");
    host.pointer("down", tile, 5., 6., 0);
    host.pointer("up", tile, 5., 6., 0);
    assert_eq!(cmds("dbl").len(), 1, "a third click starts over");
    host.pointer("down", tile, 5., 6., 2);
    host.pointer("up", tile, 5., 6., 2);
    assert_eq!(cmds("ctx").len(), 1, "a right click is onContextMenu");
    assert_eq!(cmds("ctx")[0]["button"], 2);
    host.pointer("wheel", tile, 5., 6., 1);
    let wheel = &cmds("wheel")[0];
    assert_eq!((wheel["deltaY"].clone(), wheel["notches"].clone(), wheel["ctrl"].clone()), (json!(-48.), json!(1), json!(true)));
    super::axia_view_set_modifiers(0);
    drop(host);
    scene::destroy(mount);
}

struct SurfaceProbe {
    tree: Rc<RefCell<Value>>,
    log: Rc<RefCell<Vec<Value>>>,
}
impl NativeApp for SurfaceProbe {
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] == "sync" { json!({"tree":*self.tree.borrow()}) }
        else { self.log.borrow_mut().push(input.clone()); json!({}) }
    }
}

#[test]
fn native_drop_tokens_follow_destinations_and_reject_stale_or_external_trash_drops() {
    let mount=scene::create(scene::VIEW);
    let tree=Rc::new(RefCell::new(json!({"kind":"view","dropAction":["dropFiles","/first"],"children":[{"kind":"text","label":"target"}]})));
    let log=Rc::new(RefCell::new(Vec::new()));
    let mut host=Host::new(mount,Box::new(SurfaceProbe{tree:tree.clone(),log:log.clone()}),"",None);
    host.facts(json!({}));
    let text=host.find_text("target");
    let token=unsafe {crate::runtime::node_file_drop(crate::runtime::node_get(text))};
    assert_ne!(token & super::DROP_TOKEN_FLAG,0);
    assert!(host.file_drop(token,&json!(["/source"]),false));
    assert_eq!(log.borrow()[0]["cmd"],"dropFiles");
    assert_eq!(log.borrow()[0]["key"],"/first");
    assert_eq!(log.borrow()[0]["internalMove"],false);
    assert_eq!(unsafe {crate::runtime::node_file_drop(crate::runtime::node_get(text))},token,"unchanged target reuses its token");
    tree.borrow_mut()["dropAction"]=json!(["dropFiles","trash"]);
    tree.borrow_mut()["nativeMoveOnly"]=json!(true);
    host.facts(json!({"changed":true}));
    let trash=unsafe {crate::runtime::node_file_drop(crate::runtime::node_get(text))};
    assert_ne!(trash & 0x7fff_ffff,token);
    assert!(!host.file_drop(token,&json!(["/source"]),true),"navigation invalidates in-flight data");
    assert!(host.file_drop(trash & 0x7fff_ffff,&json!(["/source"]),false));
    assert_eq!(log.borrow().len(),1,"an external source cannot trigger trash");
    assert!(host.file_drop(trash & 0x7fff_ffff,&json!(["/source"]),true));
    assert_eq!(log.borrow().len(),2);
    assert_eq!(log.borrow()[1]["key"],"trash");
    assert!(host.file_drop(trash & 0x7fff_ffff,&json!([42]),true));
    assert_eq!(log.borrow().len(),2,"invalid paths do not reach the model");
    tree.borrow_mut().as_object_mut().unwrap().remove("dropAction");
    host.facts(json!({"changed":false}));
    assert_eq!(unsafe {crate::runtime::node_file_drop(crate::runtime::node_get(text))},0);
    assert!(!host.file_drop(trash & 0x7fff_ffff,&json!(["/source"]),true));
    drop(host);scene::destroy(mount);
}

#[test]
fn compositor_preview_nodes_bind_sources_and_copy_documents_only_when_the_handle_changes() {
    let mount=scene::create(scene::VIEW);
    scene::document_fixture(41,2,1,vec![0xffff0000,0xff00ff00]);
    let tree=Rc::new(RefCell::new(json!({"kind":"view","children":[{"kind":"media","key":"video","media":23},{"kind":"documentImage","key":"pdf","image":41}]})));
    let mut host=Host::new(mount,Box::new(SurfaceProbe{tree:tree.clone(),log:Default::default()}),"",None);
    host.facts(json!({}));
    let nodes=children(children(mount)[0]);
    unsafe {
        let video=crate::runtime::node_get(nodes[0]);let pdf=crate::runtime::node_get(nodes[1]);
        assert_eq!((*video).kind,scene::MEDIA);assert_eq!((*video).term,23);
        assert_eq!((*pdf).kind,scene::IMAGE);assert!(!(*pdf).img.is_null());
        // Replacing the service's fixture cannot change a node with the same handle:
        // a routine sync must not copy the pixels again.
        scene::document_fixture(41,3,1,vec![0xff0000ff;3]);
        let image=(*pdf).img;
        host.facts(json!({"again":true}));
        assert_eq!((*pdf).img,image);
        assert_eq!(scene::image_dimensions(nodes[1]),(2,1));
        scene::document_fixture(42,3,1,vec![0xff0000ff;3]);
        tree.borrow_mut()["children"][1]["image"]=json!(42);
        host.facts(json!({"newHandle":true}));
        assert_eq!(scene::image_dimensions(nodes[1]),(3,1));
        tree.borrow_mut()["children"][0]["media"]=json!(0);
        tree.borrow_mut()["children"][1]["image"]=json!(0);
        host.facts(json!({"again":false}));
        assert_eq!((*video).term,0);assert!((*pdf).img.is_null());
    }
    drop(host);scene::destroy(mount);
}
