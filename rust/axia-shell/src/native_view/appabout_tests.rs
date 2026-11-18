use serde_json::json;
#[test]
fn file_and_compiled_icons_mount_directly_with_the_original_text_sizes() {
    let mut app = crate::native_view::app("appabout").unwrap();
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
