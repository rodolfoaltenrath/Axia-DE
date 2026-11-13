use super::Model;
use serde_json::{json, Value};

fn request(m: &mut Model, op: &str, mut facts: Value) -> Value {
    facts["cmd"] = json!("sidebar");
    facts["op"] = json!(op);
    m.request(&facts)
}
fn configure(m: &mut Model) {
    assert_eq!(
        request(
            m,
            "configure",
            json!({"sizes":[[116,200],[116,100],[116,300]],"rot":0})
        ),
        Value::Null
    );
}
#[test]
fn thumbnail_frames_follow_then_allow_manual_scroll() {
    let mut m = Model::default();
    configure(&mut m);
    let first = request(&mut m, "viewport", json!({"height":250,"page":0}));
    assert_eq!(
        first,
        json!({"offset":0.,"total":700.,"width":116.,"frames":[
            {"page":0,"top":8.,"height":200.,"selected":true},
            {"page":1,"top":236.,"height":100.,"selected":false}
        ]})
    );
    let last = request(&mut m, "viewport", json!({"height":250,"page":2}));
    assert_eq!(last["offset"], 450.);
    assert_eq!(last["frames"][0]["page"], 2);
    request(&mut m, "scroll", json!({"delta":-1e6}));
    let unchanged = request(&mut m, "viewport", json!({"height":250,"page":2}));
    assert_eq!(unchanged["offset"], 0.);
    assert_eq!(
        request(&mut m, "scroll", json!({"delta":1e6}))["offset"],
        450.
    );
}
#[test]
fn viewport_resize_clamps_offset_and_empty_documents_are_valid() {
    let mut m = Model::default();
    configure(&mut m);
    request(&mut m, "viewport", json!({"height":100,"page":2}));
    assert_eq!(
        request(&mut m, "viewport", json!({"height":800,"page":2}))["offset"],
        0.
    );
    request(&mut m, "configure", json!({"sizes":[],"rot":0}));
    assert_eq!(
        request(&mut m, "viewport", json!({"height":800,"page":0}))["frames"],
        json!([])
    );
}
#[test]
fn rotated_thumbnail_rounding_and_owner_isolation() {
    let mut a = Model::default();
    let mut b = Model::default();
    configure(&mut a);
    request(&mut b, "configure", json!({"sizes":[[100,200]],"rot":1}));
    assert_eq!(
        request(&mut b, "viewport", json!({"height":500,"page":0}))["frames"][0]["height"],
        58.
    );
    assert_eq!(
        request(&mut a, "viewport", json!({"height":500,"page":0}))["frames"][0]["height"],
        200.
    );
}
#[test]
fn malformed_layout_and_viewport_preserve_last_valid_state() {
    let mut m = Model::default();
    configure(&mut m);
    let valid = request(&mut m, "viewport", json!({"height":250,"page":0}));
    for sizes in [
        json!([[0, 200]]),
        json!([[100, -1]]),
        json!([[1, 1e308]]),
        json!([[116, 200], [1, 0]]),
    ] {
        assert!(request(&mut m, "configure", json!({"sizes":sizes,"rot":0}))["error"].is_string());
        assert_eq!(
            request(&mut m, "viewport", json!({"height":250,"page":0})),
            valid
        );
    }
    assert!(request(&mut m, "viewport", json!({"height":-1,"page":2}))["error"].is_string());
    assert_eq!(request(&mut m, "scroll", json!({"delta":0})), valid);
}
fn outline() -> Value {
    json!({"items":[
        {"title":"Root","page":0,"open":true,"kids":[
            {"title":"Branch","page":-1,"kids":[{"title":"Leaf","page":5}]},
            {"title":"Next","page":3}
        ]}, {"title":"End","page":8}
    ]})
}
#[test]
fn outline_preserves_descendant_choices_across_parent_collapse() {
    let mut m = Model::default();
    let initial = request(&mut m, "outline", outline());
    assert_eq!(initial["rows"].as_array().unwrap().len(), 4);
    let opened = request(&mut m, "toggle", json!({"key":"0.0"}));
    assert_eq!(opened["rows"][2]["key"], "0.0.0");
    assert_eq!(opened["rows"][2]["depth"], 2);
    assert_eq!(
        request(&mut m, "toggle", json!({"key":"0"}))["rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(request(&mut m, "toggle", json!({"key":"0"})), opened);
    assert_eq!(request(&mut m, "pick", json!({"key":"0.0"})), Value::Null);
    assert_eq!(request(&mut m, "pick", json!({"key":"0.0.0"})), 5);
    assert_eq!(request(&mut m, "outline", outline()), initial);
}
#[test]
fn malformed_outline_does_not_replace_bookmarks() {
    let mut m = Model::default();
    let initial = request(&mut m, "outline", outline());
    assert!(request(
        &mut m,
        "outline",
        json!({"items":[{"title":"Bad","kids":"invalid"}]})
    )["error"]
        .is_string());
    assert_eq!(request(&mut m, "pick", json!({"key":"1"})), 8);
    assert!(request(&mut m, "toggle", json!({"key":"absent"}))["error"].is_string());
    // Clicking a leaf's disclosure area must neither hide rows nor navigate.
    assert_eq!(request(&mut m, "toggle", json!({"key":"1"})), initial);
}
#[test]
fn long_documents_only_return_visible_thumbnails() {
    let mut m = Model::default();
    request(
        &mut m,
        "configure",
        json!({"sizes":vec![[116,200];100_000],"rot":0}),
    );
    let view = request(&mut m, "viewport", json!({"height":500,"page":99_999}));
    assert!(view["frames"].as_array().unwrap().len() <= 4);
    assert_eq!(
        view["frames"].as_array().unwrap().last().unwrap()["page"],
        99_999
    );
}
