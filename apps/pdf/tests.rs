use super::*;

pub(super) fn numeric_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            (a - b).abs() <= 1e-10 * a.abs().max(b.abs()).max(1.)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| numeric_equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|b| numeric_equal(v, b)))
        }
        _ => a == b,
    }
}

#[test]
fn geometry_matches_frozen_javascript_reference() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../rust/axia-shell/tests/fixtures/pdf_geometry.json"
    ))
    .unwrap();
    let mut model = Model::default();
    for (i, case) in cases.as_array().unwrap().iter().enumerate() {
        let actual = model.request(&case["input"]);
        assert!(
            numeric_equal(&actual, &case["expected"]),
            "case {i}: {} => {actual}, expected {}",
            case["input"],
            case["expected"]
        );
    }
}

fn cache(model: &mut Model, op: &str, fields: Value) -> Value {
    let mut input = fields;
    input["cmd"] = json!("cache");
    input["op"] = json!(op);
    let result = model.request(&input);
    assert!(result.get("error").is_none(), "{input}: {result}");
    result
}
fn facts(page: u32, width: u32) -> Value {
    json!({"page":page,"width":width,"rot":0,"inv":false})
}
fn begin(model: &mut Model, page: u32, width: u32) -> u64 {
    cache(model, "begin", facts(page, width))["value"]["token"]
        .as_u64()
        .unwrap()
}
fn complete(model: &mut Model, token: u64, w: u32, h: u32) -> Value {
    cache(
        model,
        "complete",
        json!({"token":token,"w":w,"h":h,"now":1000,"ms":10}),
    )
}

#[test]
fn quantization_and_duplicate_render_admission() {
    let mut model = Model::default();
    assert_eq!(
        cache(&mut model, "key", facts(0, 47))["value"],
        "0|32|0|0|0"
    );
    assert_eq!(
        cache(&mut model, "key", facts(0, 48))["value"],
        "0|64|0|0|0"
    );
    let token = begin(&mut model, 0, 48);
    assert!(cache(&mut model, "begin", facts(0, 63))["value"].is_null());
    complete(&mut model, token, 64, 100);
    assert!(cache(&mut model, "begin", facts(0, 64))["value"].is_null());
}

#[test]
fn lru_reaches_eighty_percent_and_preserves_newest_even_when_oversized() {
    let mut model = Model::default();
    cache(&mut model, "configure", json!({"budget":1200}));
    for page in 0..3 {
        let token = begin(&mut model, page, 32);
        complete(&mut model, token, 10, 10);
    }
    cache(&mut model, "get", facts(0, 32));
    let token = begin(&mut model, 3, 32);
    let result = complete(&mut model, token, 10, 10);
    assert_eq!(result["release"], json!(["1|32|0|0|0", "2|32|0|0|0"]));
    assert_eq!(result["stats"]["bytes"], 800);
    let token = begin(&mut model, 4, 32);
    let result = complete(&mut model, token, 100, 100);
    assert_eq!(result["stats"]["bytes"], 40000);
    assert_eq!(result["release"].as_array().unwrap().len(), 2);
}

#[test]
fn invalidation_keeps_old_preview_until_replacement_and_rejects_late_results() {
    let mut model = Model::default();
    let original = begin(&mut model, 0, 64);
    complete(&mut model, original, 64, 100);
    let late = begin(&mut model, 0, 96);
    cache(&mut model, "invalidate", json!({"page":0}));
    assert!(cache(&mut model, "get", facts(0, 64))["value"].is_null());
    assert_eq!(cache(&mut model, "best", facts(0, 64))["value"]["gen"], 0);
    assert!(complete(&mut model, late, 96, 100)["value"].is_null());
    let fresh = begin(&mut model, 0, 64);
    let result = complete(&mut model, fresh, 64, 100);
    assert_eq!(result["release"], json!(["0|64|0|0|0"]));
    assert_eq!(result["value"]["gen"], 1);
    assert_eq!(result["stats"]["bytes"], 25600);
}

#[test]
fn best_preserves_insertion_order_on_ties_and_separates_rotation_and_inversion() {
    let mut model = Model::default();
    for width in [64, 128] {
        let token = begin(&mut model, 0, width);
        complete(&mut model, token, width, 100);
    }
    assert_eq!(
        cache(&mut model, "best", facts(0, 96))["value"]["width"],
        64
    );
    assert!(cache(
        &mut model,
        "best",
        json!({"page":0,"width":96,"rot":1,"inv":false})
    )["value"]
        .is_null());
    assert!(cache(
        &mut model,
        "best",
        json!({"page":0,"width":96,"rot":0,"inv":true})
    )["value"]
        .is_null());
}

#[test]
fn failures_drop_and_close_preserve_ticket_and_resource_ownership() {
    let mut model = Model::default();
    let a = begin(&mut model, 0, 64);
    let b = begin(&mut model, 1, 64);
    assert_eq!(
        cache(&mut model, "drop", json!({"keep":[0]}))["value"],
        json!([b])
    );
    cache(&mut model, "fail", json!({"token":b}));
    let retry = begin(&mut model, 1, 64);
    cache(&mut model, "fail", json!({"token":b}));
    assert!(cache(&mut model, "begin", facts(1, 64))["value"].is_null());
    complete(&mut model, a, 64, 100);
    let result = cache(&mut model, "close", json!({}));
    assert_eq!(result["release"], json!(["0|64|0|0|0"]));
    assert_eq!(result["stats"]["bytes"], 0);
    assert!(complete(&mut model, retry, 64, 100)["value"].is_null());
    assert!(cache(&mut model, "begin", facts(2, 64))["value"].is_null());
    assert_eq!(cache(&mut model, "close", json!({}))["release"], json!([]));
}

#[test]
fn clear_preserves_inflight_requests_like_the_previous_cache() {
    let mut model = Model::default();
    let token = begin(&mut model, 0, 64);
    cache(&mut model, "clear", json!({}));
    assert!(!complete(&mut model, token, 64, 100)["value"].is_null());
}

#[test]
fn layouts_are_owned_per_model_and_invalid_updates_preserve_previous_positions() {
    let mut a = Model::default();
    let mut b = Model::default();
    a.request(&json!({"cmd":"layout","sizes":[[100,200],[100,200]],"rot":0,"scale":1,"gap":10}));
    assert_eq!(
        a.request(&json!({"cmd":"visible","y0":220,"y1":300})),
        json!([1, 1])
    );
    assert_eq!(
        b.request(&json!({"cmd":"visible","y0":220,"y1":300})),
        json!([0, -1])
    );
    assert!(
        a.request(&json!({"cmd":"layout","sizes":[[1,-1]],"rot":0,"scale":1,"gap":10}))["error"]
            .is_string()
    );
    assert_eq!(
        a.request(&json!({"cmd":"visible","y0":220,"y1":300})),
        json!([1, 1])
    );
}

#[test]
fn malformed_cache_requests_do_not_consume_live_render_tickets() {
    let mut model = Model::default();
    let token = begin(&mut model, 0, 32);
    assert!(model.request(
        &json!({"cmd":"cache","op":"complete","token":token,"w":-1,"h":10,"now":1,"ms":0})
    )["error"]
        .is_string());
    assert!(model.request(
        &json!({"cmd":"cache","op":"complete","token":token,"w":32769,"h":10,"now":1,"ms":0})
    )["error"]
        .is_string());
    assert!(!complete(&mut model, token, 32, 10)["value"].is_null());
}

#[test]
fn coordinate_overflow_is_rejected_instead_of_serialized_as_null() {
    let mut model = Model::default();
    for input in [
        json!({"cmd":"toView","point":[1e308,1],"size":[100,200],"rot":0,"scale":1e308}),
        json!({"cmd":"toPage","point":[1e308,1],"size":[100,200],"rot":0,"scale":1e-308}),
        json!({"cmd":"rect","rect":[1e308,1,1e308,10],"size":[100,200],"rot":0,"scale":1}),
    ] {
        assert_eq!(
            model.request(&input),
            json!({"error":"pdf.invalidGeometry"})
        );
    }
}
