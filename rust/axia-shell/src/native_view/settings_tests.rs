use serde_json::json;
#[test]
fn account_passwords_are_masked_in_the_real_native_scene() {
    use crate::{native_view::Host, runtime::native as scene};
    let root = scene::create(scene::VIEW);
    let (app, theme) = axia_app_settings::host_test_fixture();
    let mut host = Host::new(
        root,
        Box::new(app),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"),
        Some("/home/test"),
    );
    host.facts(json!({"args":{"section":"users"},"lang":"en","width":880,"keyboard":true,"context":{"theme":theme}}));
    let visible = host.text();
    assert!(visible.contains("Test User"));
    assert!(!visible.contains("private-old-password"));
    assert!(!visible.contains("private-new-password"));
    host.action(&json!({"action":["do",json!({"op":"userSelect","value":"other"}).to_string()]}));
    assert!(host.text().contains("Other User"));
    drop(host);
    scene::destroy(root);
}