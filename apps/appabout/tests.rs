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
    assert_eq!(data["children"][0]["kind"], "view");
    assert!(data.to_string().contains("<script>"));
    let icon = app.request(&json!({"cmd":"icon","size":-5}));
    assert_eq!(icon["style"]["width"], 1.);
}

#[test]
fn file_and_compiled_icons_mount_directly_with_the_original_text_sizes() {
    let mut app = App;
    let tree=app.request(&json!({"cmd":"sync","args":{"key":"desktop:one.desktop"},
        "catalog":[{"id":"desktop:one.desktop","name":"One","icon":true,"version":"1.2","kind":"linux","iconData":{"kind":"image","path":"/tmp/one.png","name":"One"}}]}))["tree"].clone();
    assert_eq!(tree["children"][0]["kind"], "image");
    assert_eq!(tree["children"][0]["src"], "/tmp/one.png");
    assert_eq!(tree["children"][1]["style"]["fontSize"], 17);
    assert_eq!(tree["children"][2]["style"]["fontSize"], 12);
    assert_eq!(tree["children"][3]["style"]["fontSize"], 11);
    let tree=app.request(&json!({"cmd":"sync","args":{"key":"calculator"},"registry":[{"id":"calculator","icon":true}]}))["tree"].clone();
    assert_eq!(
        tree["children"][0],
        crate::native_view::icon_tree("calculator", 64.).unwrap()
    );
    assert!(!tree.to_string().contains("appIcon"));
}
