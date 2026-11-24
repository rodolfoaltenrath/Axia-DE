//! Real scene/host tests: Rust model -> generic native tree -> input effects.
use super::{children, node_with_text, scene, Host};
use serde_json::{json, Value};

struct Model(std::cell::RefCell<axia_app_pdf::Model>);
impl Model {
    fn new() -> Self {
        Self(Default::default())
    }
    fn request(&self, op: &str, mut facts: Value) -> Value {
        facts["cmd"] = json!("sidebar");
        facts["op"] = json!(op);
        let reply = self.0.borrow_mut().request(&facts);
        assert!(reply["error"].is_null(), "{reply}");
        reply
    }
}
fn facts(tree: Value) -> Value {
    json!({"args":{"owner":"pdf-test","tree":tree},"lang":"pt"})
}
fn click(host: &mut Host, mount: i32, label: &str) -> Value {
    let node = node_with_text(mount, label);
    assert_ne!(node, 0, "missing {label}");
    assert_eq!(host.pointer("down", node, 0., 0., 0), Some(mount));
    assert_eq!(host.pointer("up", node, 0., 0., 0), Some(mount));
    let effects = host.take().expect("native input effect");
    assert_eq!(
        effects.as_array().unwrap().len(),
        1,
        "a disclosure must not also activate its parent"
    );
    assert_eq!(effects[0]["owner"], "pdf-test");
    effects[0].clone()
}
fn host(mount: i32) -> Host {
    Host::new(
        mount,
        crate::native_view::app("shell:tree").unwrap(),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        None,
    )
}
#[test]
fn thumbnails_mount_localize_and_deliver_page_tab_and_wheel_actions() {
    let model = Model::new();
    model.request("configure", json!({"sizes":[[116,200],[116,200]],"rot":0}));
    model.request("viewport", json!({"height":300,"page":0}));
    let tree = model.request("tree", json!({"side":"thumbs"}));
    let mount = scene::create(scene::VIEW);
    let mut host = host(mount);
    host.facts(facts(tree));
    assert!(host.text().contains("Páginas"));
    assert!(host.text().contains("Sumário"));
    assert_eq!(click(&mut host, mount, "1")["action"], json!(["page", 0]));
    assert_eq!(
        click(&mut host, mount, "Sumário")["action"],
        json!(["side", "outline"])
    );
    let node = node_with_text(mount, "1");
    host.pointer("wheel", node, 0., 0., -1);
    let effect = host.take().unwrap();
    assert_eq!(effect[0]["action"], json!(["scroll"]));
    assert!(effect[0]["extra"]["deltaY"].as_f64().unwrap() > 0.);
    drop(host);
    assert!(
        children(mount).is_empty(),
        "the native sidebar releases its subtree"
    );
    scene::destroy(mount);
}
#[test]
fn outline_disclosures_do_not_navigate_and_new_documents_replace_the_subtree() {
    let model = Model::new();
    model.request(
        "outline",
        json!({"items":[{"title":"Chapter","page":0,"kids":[{"title":"Leaf","page":2}]}]}),
    );
    let mount = scene::create(scene::VIEW);
    let mut host = host(mount);
    host.facts(facts(
        model.request("tree", json!({"side":"outline","document":1})),
    ));
    assert!(!host.text().contains("Leaf"));
    assert_eq!(
        click(&mut host, mount, "▶")["action"],
        json!(["toggle", "0"])
    );
    model.request("toggle", json!({"key":"0"}));
    host.facts(facts(
        model.request("tree", json!({"side":"outline","document":1})),
    ));
    assert!(host.text().contains("Leaf"));
    assert_eq!(
        click(&mut host, mount, "Leaf")["action"],
        json!(["pick", "0.0"])
    );
    assert_eq!(model.request("pick", json!({"key":"0.0"})), 2);
    let old = children(mount)[0];
    host.facts(facts(
        model.request("tree", json!({"side":"outline","document":2})),
    ));
    assert_ne!(
        children(mount)[0],
        old,
        "a document change clears native scroll state"
    );
    assert!(!scene::exists(old));
    drop(host);
    scene::destroy(mount);
}
