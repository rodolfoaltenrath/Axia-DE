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
    // The scene has no child list API here: walk ids that name `node` as parent.
    (1..4096)
        .filter(|id| scene::exists(*id) && scene::parent(*id) == node)
        .collect()
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
