//! Dock geometry independent of React: ordering, separators, drag gaps,
//! magnification, display-relative placement and stable drop positions.
use serde_json::{json, Value};
const PAD: f64 = 9.;
const GAP: f64 = 8.;
const SEP: f64 = 9.;
const MARGIN: f64 = 6.;
#[derive(Clone, Copy)]
enum Token {
    Item(usize),
    Trash,
    Separator,
    Gap,
}
fn number(v: &Value, fallback: f64) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(fallback)
}
fn tokens(items: &[Value], own: &str, gap: f64, flip: bool) -> Vec<Token> {
    let mut out = Vec::new();
    let (mut pinned, mut separator, mut inserted) = (0., false, gap < 0.);
    for (index, item) in items.iter().enumerate() {
        if !own.is_empty() && item["id"] == own {
            continue;
        }
        let pin = item["pinned"] == true && item["recent"] != true;
        if !inserted && (!pin || pinned == gap) {
            out.push(Token::Gap);
            inserted = true;
        }
        if pin {
            pinned += 1.;
        }
        if item["recent"] == true && !separator {
            out.push(Token::Separator);
            separator = true;
        }
        out.push(Token::Item(index));
    }
    if !inserted {
        out.push(Token::Gap);
    }
    out.extend([Token::Separator, Token::Trash]);
    if flip {
        out.reverse();
    }
    out
}
fn layout(tokens: &[Token], base: f64, sizes: Option<&[f64]>) -> (Vec<f64>, f64) {
    let mut offset = PAD;
    let mut centers = Vec::new();
    for token in tokens {
        match token {
            Token::Separator => offset += SEP,
            Token::Gap => offset += base + GAP,
            _ => {
                let size = sizes.map_or(base, |s| s[centers.len()]);
                centers.push(offset + size / 2.);
                offset += size + GAP;
            }
        }
    }
    (centers, offset - GAP + PAD)
}
pub fn request(v: &Value) -> Value {
    let items = v["items"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if items.len() > 1024 {
        return json!({"error":"dock.tooManyItems"});
    }
    let position = v["position"].as_str().unwrap_or("bottom");
    let vert = position != "bottom";
    let flip = !vert && v["rtl"] == true;
    let own = v["own"].as_str().unwrap_or("");
    let dragging = v["dragging"] == true;
    let count = items
        .iter()
        .filter(|item| item["pinned"] == true && item["recent"] != true && item["id"] != own)
        .count();
    let gap = v["gapAt"]
        .as_f64()
        .filter(|n| n.is_finite())
        .map_or(-1., |n| n.max(0.).min(count as f64));
    let vis = tokens(items, own, gap, flip);
    let n = vis
        .iter()
        .filter(|t| matches!(t, Token::Item(_) | Token::Trash))
        .count();
    let separators = vis.iter().filter(|t| matches!(t, Token::Separator)).count();
    let gaps = vis.iter().filter(|t| matches!(t, Token::Gap)).count();
    let area = &v["area"];
    let (x, y) = (number(&area["x"], 0.), number(&area["y"], 0.));
    let (w, h) = (
        number(&area["w"], 1280.).max(0.),
        number(&area["h"], 720.).max(0.),
    );
    let top = number(&v["top"], 0.);
    let (origin, available) = if vert { (y + top, h - top) } else { (x, w) };
    let size = number(&v["size"], 54.).clamp(1., 512.);
    let maximum = number(&v["maxSize"], 86.).clamp(1., 1024.);
    let magnify = v["magnify"] == true;
    let grow = if magnify {
        2.6 * (maximum / size - 1.).max(0.)
    } else {
        0.
    };
    let fit =
        ((available - 2. * PAD - 16. - (n + gaps - 1) as f64 * GAP - separators as f64 * SEP)
            / ((n + gaps).max(1) as f64 + grow))
            .floor();
    let base = size.min(fit).max(20.);
    let max = if magnify && !dragging {
        (maximum * base / size).round().max(base)
    } else {
        base
    };
    let (base_centers, base_len) = layout(&vis, base, None);
    let base_start = origin + (available - base_len) / 2.;
    let centers: Vec<f64> = base_centers.into_iter().map(|c| base_start + c).collect();
    let pointer = v["pointer"].as_f64().filter(|n| n.is_finite());
    let sizes: Vec<f64> = centers
        .iter()
        .map(|center| {
            let Some(pointer) = pointer else {
                return base;
            };
            let distance = (pointer - center).abs() / (base * 3.);
            if max != base && distance < 1. {
                (base + (max - base) * (distance * std::f64::consts::PI / 2.).cos()).round()
            } else {
                base
            }
        })
        .collect();
    let hovered = if dragging {
        None
    } else {
        pointer.and_then(|p| {
            centers
                .iter()
                .position(|c| (p - c).abs() <= (base + GAP) / 2.)
        })
    };
    let (offsets, len) = layout(&vis, base, Some(&sizes));
    let start = origin.max(origin + (available - len) / 2.);
    let bar = base + PAD * 2.;
    let rect = match position {
        "bottom" => json!({"x0":start,"x1":start+len,"y0":y+h-MARGIN-bar,"y1":y+h}),
        "left" => json!({"x0":x,"x1":x+MARGIN+bar,"y0":start,"y1":start+len}),
        _ => json!({"x0":x+w-MARGIN-bar,"x1":x+w,"y0":start,"y1":start+len}),
    };
    // Dropping is based on the fixed-size layout without the dragged icon or
    // the placeholder, so inserting a gap never moves the target under a drag.
    let rest_tokens = tokens(items, own, -1., flip);
    let (rest_centers, rest_len) = layout(&rest_tokens, base, None);
    let rest_start = origin + (available - rest_len) / 2.;
    let mut rest = Vec::new();
    let mut index = 0;
    for token in &rest_tokens {
        match token {
            Token::Item(i) => {
                if items[*i]["pinned"] == true && items[*i]["recent"] != true {
                    rest.push(rest_start + rest_centers[index]);
                }
                index += 1;
            }
            Token::Trash => index += 1,
            _ => {}
        }
    }
    let trash_index = vis
        .iter()
        .filter(|t| matches!(t, Token::Item(_) | Token::Trash))
        .position(|t| matches!(t, Token::Trash))
        .unwrap();
    let encoded: Vec<Value> = vis
        .iter()
        .map(|token| match token {
            Token::Item(i) => json!({"item":i}),
            Token::Trash => json!({"item":-1}),
            Token::Separator => json!({"sep":true}),
            Token::Gap => json!({"gap":true}),
        })
        .collect();
    json!({"tokens":encoded,"flip":flip,"vert":vert,"baseSize":base,"maxSize":max,
        "centers":centers,"sizes":sizes,"hovered":hovered.map_or(-1,|i|i as i64),
        "offsets":offsets,"len":len,"start":start,"bar":bar,"rect":rect,
        "drop":{"rest":rest,"trashC":start+offsets[trash_index]}})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn compare(expected: &Value, actual: &Value) {
        match (expected, actual) {
            (Value::Number(a), Value::Number(b)) => {
                let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
                assert!((a - b).abs() < 1e-8, "{a} != {b}");
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) {
                    compare(a, b);
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
                for (key, a) in a {
                    compare(a, &b[key]);
                }
            }
            _ => assert_eq!(expected, actual),
        }
    }
    #[test]
    fn geometry_matches_original_jsx_across_displays_rtl_dragging_and_magnification() {
        let cases: Value =
            serde_json::from_str(include_str!("../tests/data/dock-layout.json")).unwrap();
        for case in cases.as_array().unwrap() {
            compare(&case["expected"], &request(&case["input"]));
        }
    }
    #[test]
    fn empty_small_and_invalid_inputs_remain_finite_and_bounded() {
        for input in [
            Value::Null,
            json!({"area":{"w":0,"h":0},"size":0}),
            json!({"size":-1,"maxSize":1e300}),
        ] {
            let out = request(&input);
            assert!(out["baseSize"].as_f64().unwrap().is_finite());
            assert_eq!(out["tokens"][1]["item"], -1);
        }
        let too_many = json!({"items":vec![json!({"id":"x"});1025]});
        assert_eq!(request(&too_many)["error"], "dock.tooManyItems");
    }
    #[test]
    fn placeholder_does_not_move_drop_targets_and_dragging_disables_hover() {
        let mut input = json!({"items":[{"id":"a","pinned":true},{"id":"b","pinned":true}],
            "area":{"w":1280,"h":720},"dragging":true,"magnify":true,"pointer":640});
        let before = request(&input);
        input["gapAt"] = json!(1);
        let gap = request(&input);
        assert_eq!(before["drop"]["rest"], gap["drop"]["rest"]);
        assert_eq!(gap["baseSize"], gap["maxSize"]);
        assert_eq!(gap["hovered"], -1);
    }
}
