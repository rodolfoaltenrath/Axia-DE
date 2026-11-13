use super::*;

fn call(model: &mut Model, op: &str, fields: Value) -> Value {
    let mut input = fields;
    input["cmd"] = json!("viewer");
    input["op"] = json!(op);
    let reply = model.request(&input);
    assert!(reply.get("error").is_none(), "{input}: {reply}");
    reply
}
fn rect() -> Value {
    json!([1, 2, 3, 4])
}

#[test]
fn navigation_matches_frozen_javascript_positions_hits_and_zoom_policy() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../rust/axia-shell/tests/fixtures/pdf_viewer.json"
    ))
    .unwrap();
    let mut model = Model::default();
    for case in cases.as_array().unwrap() {
        let reply = model.request(&case["input"]);
        assert!(
            tests::numeric_equal(&reply, &case["expected"]),
            "{}: {reply}, expected {}",
            case["input"],
            case["expected"]
        );
    }
    let cases: Value = serde_json::from_str(include_str!(
        "../../rust/axia-shell/tests/fixtures/pdf_navigation.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        model.request(&case["layout"]);
        let mut input = case["viewport"].clone();
        input["cmd"] = json!("view");
        input["op"] = json!("viewport");
        let reply = model.request(&input);
        assert!(
            tests::numeric_equal(&reply, &case["expected"]),
            "{input}: {reply}, expected {}",
            case["expected"]
        );
        for op in ["goto", "hit"] {
            let mut input = case[op].clone();
            input["cmd"] = json!("view");
            input["op"] = json!(op);
            let reply = model.request(&input);
            assert!(
                tests::numeric_equal(&reply, &case[op]["expected"]),
                "{input}: {reply}"
            );
        }
    }
}

#[test]
fn repeating_the_same_query_cannot_resurrect_an_old_search() {
    let mut model = Model::default();
    let a = call(&mut model, "findStart", json!({"query":" same "}));
    let b = call(&mut model, "findStart", json!({"query":" same "}));
    assert_ne!(a["token"], b["token"]);
    assert_eq!(a["query"], "same");
    assert!(call(
        &mut model,
        "findAppend",
        json!({"token":a["token"],"page":0,"rects":[rect()]})
    )
    .is_null());
    assert!(call(&mut model, "findFinish", json!({"token":a["token"]})).is_null());
    let ready = call(
        &mut model,
        "findAppend",
        json!({"token":b["token"],"page":1,"rects":[rect()]}),
    );
    assert_eq!(ready["go"]["page"], 1);
    assert_eq!(ready["hits"]["list"].as_array().unwrap().len(), 1);
    assert_eq!(
        call(&mut model, "findFinish", json!({"token":b["token"]}))["done"],
        true
    );
}

#[test]
fn empty_parts_do_not_navigate_and_next_previous_wrap_around() {
    let mut model = Model::default();
    let token = call(&mut model, "findStart", json!({"query":"x"}))["token"].clone();
    let empty = call(
        &mut model,
        "findAppend",
        json!({"token":token,"page":0,"rects":[]}),
    );
    assert!(empty["go"].is_null());
    for p in 1..3 {
        call(
            &mut model,
            "findAppend",
            json!({"token":token,"page":p,"rects":[rect()]}),
        );
    }
    assert_eq!(
        call(&mut model, "findNext", json!({"delta":-1}))["go"]["page"],
        2
    );
    assert_eq!(
        call(&mut model, "findNext", json!({"delta":1}))["go"]["page"],
        1
    );
    call(&mut model, "findStart", json!({"query":"  "}));
    assert!(call(
        &mut model,
        "findAppend",
        json!({"token":token,"page":0,"rects":[rect()]})
    )
    .is_null());
    assert!(call(&mut model, "findNext", json!({"delta":1})).is_null());
}

#[test]
fn clearing_selection_or_switching_documents_rejects_pending_replies() {
    let mut model = Model::default();
    let a = call(&mut model, "selectionStart", json!({}));
    let b = call(&mut model, "selectionStart", json!({}));
    let result = json!({"text":"selected","rects":[rect()]});
    assert!(call(
        &mut model,
        "selectionReply",
        json!({"token":a,"page":0,"result":result})
    )
    .is_null());
    assert_eq!(
        call(
            &mut model,
            "selectionReply",
            json!({"token":b,"page":0,"result":result})
        )["text"],
        "selected"
    );
    call(&mut model, "selectionClear", json!({}));
    assert!(call(
        &mut model,
        "selectionReply",
        json!({"token":b,"page":0,"result":result})
    )
    .is_null());
    let pending = call(&mut model, "selectionStart", json!({}));
    let search = call(&mut model, "findStart", json!({"query":"x"}));
    call(&mut model, "reset", json!({}));
    assert!(call(
        &mut model,
        "selectionReply",
        json!({"token":pending,"page":0,"result":result})
    )
    .is_null());
    assert!(call(&mut model, "findFinish", json!({"token":search["token"]})).is_null());
}

#[test]
fn keyboard_modifier_precedence_and_editing_guards_are_preserved() {
    let mut model = Model::default();
    for (event, expected) in [
        (json!({"ctrl":true,"alt":true,"base":"g"}), json!("goto")),
        (
            json!({"ctrl":true,"shift":true,"base":"g"}),
            json!("previousHit"),
        ),
        (json!({"meta":true,"base":"F"}), json!("find")),
        (
            json!({"meta":true,"shift":true,"base":"r"}),
            json!("rotateLeft"),
        ),
        (json!({"key":" ","shift":true}), json!("pageUp")),
        (json!({"key":"Escape","ctrl":true}), json!("escape")),
        (json!({"key":"Q"}), Value::Null),
    ] {
        assert_eq!(
            call(
                &mut model,
                "key",
                json!({"event":event,"hasDoc":true,"editing":false})
            ),
            expected
        );
        assert!(call(
            &mut model,
            "key",
            json!({"event":event,"hasDoc":true,"editing":true})
        )
        .is_null());
    }
    assert!(call(
        &mut model,
        "key",
        json!({"event":{"ctrl":true,"base":"a"},"hasDoc":false})
    )
    .is_null());
}

#[test]
fn wheel_gating_and_scroll_limits_use_the_original_thresholds() {
    let mut model = Model::default();
    let e = json!({"deltaY":1});
    assert_eq!(
        call(
            &mut model,
            "wheel",
            json!({"event":e,"single":true,"now":1000})
        )["kind"],
        "page"
    );
    assert!(call(
        &mut model,
        "wheel",
        json!({"event":e,"single":true,"now":1249})
    )
    .is_null());
    assert_eq!(
        call(
            &mut model,
            "wheel",
            json!({"event":e,"single":true,"now":1250})
        )["delta"],
        1
    );
    assert!(call(
        &mut model,
        "wheel",
        json!({"event":{"deltaY":-1,"ctrl":true,"precise":true},"single":false,"now":1300})
    )
    .is_null());
    assert_eq!(
        call(
            &mut model,
            "wheel",
            json!({"event":{"deltaY":-1,"ctrl":true,"precise":true,"notches":true},"single":false,"now":1300})
        )["step"],
        1
    );
    assert_eq!(
        call(
            &mut model,
            "keyScroll",
            json!({"action":"pageDown","value":100,"max":500,"height":600})
        ),
        json!(500.)
    );
    assert_eq!(
        call(
            &mut model,
            "keyScroll",
            json!({"action":"up","value":20,"max":500,"height":600})
        ),
        json!(0.)
    );
}

#[test]
fn empty_documents_and_outside_hits_do_not_manufacture_page_zero() {
    let mut model = Model::default();
    model.request(&json!({"cmd":"layout","sizes":[],"rot":0,"scale":1,"gap":14}));
    let v=model.request(&json!({"cmd":"view","op":"viewport","view":{"w":800,"h":600},"mode":"single","page":0,"offset":0,"offsetX":0}));
    assert_eq!(v["last"], -1);
    assert_eq!(v["frames"], json!([]));
    assert!(model
        .request(&json!({"cmd":"view","op":"goto","page":0}))
        .is_null());
    assert!(model
        .request(&json!({"cmd":"view","op":"hit","x":20,"y":20}))
        .is_null());
}

#[test]
fn rejected_viewports_preserve_the_last_valid_geometry_and_scales_stay_finite() {
    let mut model = Model::default();
    model.request(&json!({"cmd":"layout","sizes":[[100,200]],"rot":0,"scale":1,"gap":14}));
    let mut viewport = json!({"cmd":"view","op":"viewport","view":{"w":800,"h":600},"mode":"single","page":0,"offset":0,"offsetX":0});
    let valid = model.request(&viewport);
    assert!(valid.get("error").is_none());
    viewport["view"]["w"] = json!(1e308);
    viewport["offsetX"] = json!(-1.7e308);
    assert!(model.request(&viewport)["error"].is_string());
    assert_eq!(
        model.request(&json!({"cmd":"view","op":"frame","page":0})),
        valid["frames"][0]
    );
    assert!(model.request(
        &json!({"cmd":"viewer","op":"scale","zoom":1e308,"size":[100,200],"view":{"w":800,"h":600}})
    )["error"]
        .is_string());
}
