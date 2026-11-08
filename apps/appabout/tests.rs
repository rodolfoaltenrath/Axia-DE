use super::*;
#[test]
fn system_package_linux_and_unknown_metadata_use_the_right_fields() {
    let mut app = App;
    let system=app.request(&json!({"cmd":"sync","args":{"key":"calculator"},"catalog":[{"id":"calculator","name":"Calculadora","version":"old"}],"registry":[{"id":"calculator","version":"0.9.0","system":true,"description":"Rust"}]}));
    assert_eq!(system["title"]["params"]["app"], "Calculadora");
    let tree = system["tree"].to_string();
    assert!(tree.contains("0.9.0"));
    assert!(tree.contains("appabout.system"));
    assert!(tree.contains("Rust"));
    assert!(!tree.contains("old"));
    let package=app.request(&json!({"cmd":"sync","args":{"key":"pkg"},"catalog":[{"id":"pkg","name":"Package","package":true}],"registry":[{"id":"pkg","package":true}]}))["tree"].to_string();
    assert!(package.contains("appabout.package"));
    assert!(package.contains("appabout.isolated"));
    let linux=app.request(&json!({"cmd":"sync","args":{"key":"desktop:editor.desktop"},"catalog":[{"id":"desktop:editor.desktop","kind":"linux","name":"Editor","comment":"Editor description","genericName":"Fallback"}]}))["tree"].to_string();
    assert!(linux.contains("appabout.linux"));
    assert!(linux.contains("Editor description"));
    assert!(!linux.contains("Fallback"));
    assert!(!linux.contains("desktop:editor.desktop"));
    let unknown = app.request(&json!({"cmd":"sync","args":{"key":"missing"}}));
    assert_eq!(unknown["title"]["params"]["app"], "missing");
    assert!(!unknown["tree"].to_string().contains("appabout.system"));
}
#[test]
fn empty_and_changing_targets_do_not_reuse_previous_data() {
    let mut app = App;
    app.request(&json!({"cmd":"sync","args":{"key":"one"},"registry":[{"id":"one","name":"One","version":"1.0.0"}]}));
    let empty = app.request(&json!({"cmd":"sync"}));
    assert_eq!(empty["title"]["params"]["app"], "");
    assert!(!empty["tree"].to_string().contains("1.0.0"));
}
#[test]
fn icons_and_untrusted_descriptions_are_widget_data_not_code() {
    let mut app = App;
    let data=app.request(&json!({"cmd":"sync","args":{"key":"one"},"catalog":[{"id":"one","icon":true,"comment":"<script>alert(1)</script>"}]}))["tree"].clone();
    assert_eq!(data["children"][0]["kind"], "appIcon");
    assert!(data.to_string().contains("<script>"));
    let icon = app.request(&json!({"cmd":"icon","size":-5}));
    assert_eq!(icon["style"]["width"], 1.);
}
