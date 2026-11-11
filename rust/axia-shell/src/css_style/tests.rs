use super::*;

fn css(pairs: Value) -> Map<String, Value> {
    normalize(pairs.as_array().unwrap())
}

#[test]
fn colours_named_hex_rgb_and_numbers() {
    let c = |v: Value| color(&v);
    assert_eq!(c(json!("#fff")), 0xffff_ffff);
    assert_eq!(c(json!("#1234")), 0x4411_2233);
    assert_eq!(c(json!("#11223344")), 0x4411_2233);
    assert_eq!(c(json!(" White ")), 0xffff_ffff);
    assert_eq!(c(json!("rgba(10, 20, 30, 0.5)")), 0x800a_141e);
    assert_eq!(c(json!("rgba(1 2 3 / .2)")), 0x3301_0203);
    assert_eq!(c(json!("rgba(300,-5,2.5,2)")), 0xffff_0003, "clamped and rounded half up");
    assert_eq!(c(json!(0xff00_ff00u32)), 0xff00_ff00);
    assert_eq!(c(json!(-1)), 0xffff_ffff, "wraps like >>> 0");
    for bad in ["#zzz", "rgb(a,b,c)", "rgb(1,2)", "hsl(1,2,3)", "none"] {
        assert_eq!(c(json!(bad)), 0, "{bad}");
    }
}

#[test]
fn shorthands_expand_in_order_and_later_keys_win() {
    let s = css(json!([["padding", "4 8"], ["paddingTop", 1]]));
    assert_eq!((s["paddingTop"].clone(), s["paddingRight"].clone(), s["paddingBottom"].clone()), (json!(1), json!(8.0), json!(4.0)));
    // the order of the pairs matters: { borderColor, border } and { border, borderColor } differ
    assert_eq!(css(json!([["borderColor", "red"], ["border", "1px solid #00f"]]))["borderColor"], json!(0xff00_00ffu32));
    assert_eq!(css(json!([["border", "1px solid #00f"], ["borderColor", "red"]]))["borderColor"], json!(0xffff_0000u32));
    let f = css(json!([["flex", 1]]));
    assert_eq!((f["flexGrow"].clone(), f["flexShrink"].clone(), f["flexBasis"].clone()), (json!(1.0), json!(1.0), json!(0)));
    assert_eq!(css(json!([["flex", 0]]))["flexBasis"], "auto");
    assert_eq!(css(json!([["marginInline", "4 8"]]))["marginEnd"], json!(8.0));
    assert_eq!(css(json!([["inset", 0]]))["left"], json!(0));
    assert_eq!(css(json!([["width", "10%"], ["height", " 7 "]]))["width"], "10%");
    assert!(css(json!([["color", null], ["opacity", false]])).is_empty(), "null and false are skipped");
}

#[test]
fn borders_shadows_gradients_and_filters() {
    let b = css(json!([["borderTop", "2px dashed red"], ["borderStart", 3], ["borderInlineEnd", "none"]]));
    assert_eq!((b["borderTopWidth"].clone(), b["borderTopColor"].clone()), (json!(2.0), json!(0xffff_0000u32)));
    assert_eq!(b["borderStartWidth"], json!(3.0));
    assert!(!b.contains_key("borderStartColor"), "a number sets only the width");
    assert_eq!(b["borderEndColor"], json!(0));
    let s = css(json!([["boxShadow", "0 8px 24px rgba(0,0,0,.4)"]]));
    assert_eq!((s["shadowOffsetY"].clone(), s["shadowBlur"].clone(), s["shadowColor"].clone()), (json!(8.0), json!(24.0), json!(0x6600_0000u32)));
    assert_eq!(css(json!([["boxShadow", "2px 3px"]]))["shadowColor"], json!(0x8000_0000u32));
    assert_eq!(css(json!([["background", "linear-gradient(to right, red 0%, #000 100%)"]]))["gradient"], json!([90.0, 0xffff_0000u32, 0xff00_0000u32]));
    assert_eq!(css(json!([["background", "#123"]]))["backgroundColor"], json!(0xff11_2233u32));
    assert_eq!(css(json!([["backdropFilter", "blur( 3.5px) saturate(2)"]]))["backdropBlur"], json!(3.5));
}

#[test]
fn transforms_origins_and_fonts() {
    let t = css(json!([["transform", "translate(4px, 5px) scale(1.2) rotate(0.5turn)"]]));
    assert_eq!((t["translateX"].clone(), t["translateY"].clone(), t["scaleY"].clone(), t["rotate"].clone()), (json!(4.0), json!(5.0), json!(1.2), json!(180.0)));
    let a = css(json!([["transform", [{"translateX": 3}, {"scale": "2"}, {"rotate": "1rad"}]]]));
    assert_eq!(a["scaleX"], json!(2.0));
    assert!((a["rotate"].as_f64().unwrap() - 57.29577951308232).abs() < 1e-12);
    let o = css(json!([["transformOrigin", "top left"]]));
    assert_eq!((o["transformOriginX"].clone(), o["transformOriginY"].clone()), (json!("0%"), json!("0%")));
    assert_eq!(css(json!([["transformOrigin", "bottom"]]))["transformOriginY"], "100%");
    assert_eq!(css(json!([["fontWeight", "semibold"]]))["fontWeight"], json!(600.0));
    assert_eq!(css(json!([["fontWeight", "0"]]))["fontWeight"], json!(400.0));
    assert_eq!(css(json!([["fontFamily", "JetBrains Mono"]]))["fontFamily"], "monospace");
    assert_eq!(css(json!([["WebkitLineClamp", 3]]))["numberOfLines"], json!(3));
    assert_eq!(css(json!([["borderRadius", "8px"], ["zIndex", "auto"]]))["borderRadius"], json!(8.0));
}

/// The JS renderer's own results on real and random styles: AXIA_CSS_CORPUS is a file of
/// {"pairs": …, "expected": …} lines made from runtime/style.js before it was removed.
#[test]
#[ignore = "needs a corpus made with the former runtime/style.js"]
fn matches_the_former_js_renderer_on_a_corpus() {
    let path = std::env::var("AXIA_CSS_CORPUS").expect("AXIA_CSS_CORPUS");
    let same = |a: &Value, b: &Value| -> bool {
        fn eq(a: &Value, b: &Value) -> bool {
            match (a, b) {
                (Value::Number(x), Value::Number(y)) => {
                    let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
                    (x - y).abs() <= 1e-9 * x.abs().max(1.)
                }
                (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| eq(a, b)),
                _ => a == b,
            }
        }
        eq(a, b)
    };
    let (mut cases, mut wrong) = (0, Vec::new());
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let case: Value = serde_json::from_str(line).unwrap();
        let got = normalize(case["pairs"].as_array().unwrap());
        let want = case["expected"].as_object().unwrap();
        cases += 1;
        // a missing key in the JS output was undefined (NaN became null): both reset the property
        let keys: std::collections::BTreeSet<_> = got.keys().chain(want.keys()).collect();
        for k in keys {
            let (g, w) = (got.get(k).unwrap_or(&Value::Null), want.get(k).unwrap_or(&Value::Null));
            // the JS sent NaN for what it couldn't read; the engine stored that as 0 (colours) or garbage:
            // Rust sends 0 or nothing (a reset) instead
            let nan_ok = w == "NaN" && (g.is_null() || g.as_f64() == Some(0.));
            if !nan_ok && !same(g, w) {
                wrong.push(format!("{}: {k} rust {g} js {w}", case["pairs"]));
            }
        }
    }
    assert!(wrong.is_empty(), "{} of {cases} cases differ:\n{}", wrong.len(), wrong.iter().take(25).cloned().collect::<Vec<_>>().join("\n"));
    println!("css: {cases} styles converted exactly as the former JS renderer did");
}

#[test]
fn a_style_object_keeps_its_key_order_over_the_ffi() {
    let call = |json: &str| -> Value {
        let input = CString::new(json).unwrap();
        unsafe {
            let out = axia_css_normalize(input.as_ptr());
            let v = serde_json::from_str(CStr::from_ptr(out).to_str().unwrap()).unwrap();
            axia_css_free(out);
            v
        }
    };
    // a later key wins, in the object's own order (not alphabetical)
    assert_eq!(call(r#"{"borderColor":"red","border":"1px solid #00f"}"#)["borderColor"], json!(0xff00_00ffu32));
    assert_eq!(call(r#"{"border":"1px solid #00f","borderColor":"red"}"#)["borderColor"], json!(0xffff_0000u32));
    assert_eq!(call(r#"[["padding",4],["paddingTop",1]]"#)["paddingTop"], json!(1));
    assert_eq!(call("not json"), json!({}));
}
