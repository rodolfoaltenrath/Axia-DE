//! Dock symbols rendered directly in the native scene, shared by all themes.
use super::NativeApp;
use serde_json::{json, Value};
pub enum Icon {
    Launcher,
    Trash,
}
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
impl NativeApp for Icon {
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] != "icon" {
            return Value::Null;
        }
        let size = input["size"]
            .as_f64()
            .filter(|s| s.is_finite())
            .unwrap_or(32.)
            .clamp(1., 512.);
        match self {
            Self::Launcher => launcher(size),
            Self::Trash => trash(
                size,
                input["args"]["full"] == true,
                input["args"]["dark"] == true,
            ),
        }
    }
}
fn launcher(s: f64) -> Value {
    let colors = [
        "#ff5f57", "#febc2e", "#28c840", "#0a84ff", "#bf5af2", "#ff9f0a", "#30d158", "#ff375f",
        "#64d2ff",
    ];
    let (d, g) = (s * 0.14, s * 0.08);
    let o = (s - 3. * d - 2. * g) / 2.;
    view(
        json!({"width":s,"height":s,"borderRadius":s*0.225,
        "background":"linear-gradient(180deg, #5b5b66, #2b2b33)",
        "boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12)}),
        colors
            .iter()
            .enumerate()
            .map(|(i, color)| {
                view(
                    json!({"position":"absolute",
            "left":o+(i%3) as f64*(d+g),"top":o+(i/3) as f64*(d+g),"width":d,"height":d,
            "borderRadius":d*0.3,"backgroundColor":color}),
                    vec![],
                )
            })
            .collect(),
    )
}
fn trash(s: f64, full: bool, dark: bool) -> Value {
    let body = if dark {
        "linear-gradient(180deg, rgba(235,238,245,0.30), rgba(200,205,215,0.16))"
    } else {
        "linear-gradient(180deg, rgba(255,255,255,0.85), rgba(215,220,230,0.65))"
    };
    let line = if dark {
        "rgba(255,255,255,0.55)"
    } else {
        "rgba(60,66,80,0.45)"
    };
    let (w, h, y) = (s * 0.62, s * 0.64, s * 0.3);
    let x = (s - w) / 2.;
    let mut children = Vec::new();
    if full {
        children.push(view(
            json!({"position":"absolute","left":x+w*0.02,"top":y-s*0.24,
            "width":w*0.52,"height":s*0.32,"borderRadius":s*0.13,"backgroundColor":"#ffffff",
            "border":"1px solid rgba(60,66,80,0.35)","rotate":-14}),
            vec![],
        ));
        children.push(view(
            json!({"position":"absolute","left":x+w*0.4,"top":y-s*0.28,
            "width":w*0.56,"height":s*0.34,"borderRadius":s*0.08,"backgroundColor":"#e9f1ff",
            "border":"1px solid rgba(60,66,80,0.35)","rotate":12}),
            [0.3, 0.5, 0.7]
                .iter()
                .map(|k| {
                    view(
                        json!({"position":"absolute","left":w*0.08,
                "right":w*0.1,"top":s*0.34*k,"height":1,"backgroundColor":"rgba(60,90,160,0.45)"}),
                        vec![],
                    )
                })
                .collect(),
        ));
    }
    let mut wires: Vec<Value> = [0.2, 0.4, 0.6, 0.8]
        .iter()
        .map(|k| {
            view(
                json!({"position":"absolute",
        "top":0,"bottom":0,"left":w*k-0.5,"width":1,"backgroundColor":line}),
                vec![],
            )
        })
        .collect();
    wires.push(view(json!({"position":"absolute","left":0,"right":0,"top":h*0.5,"height":1,"backgroundColor":line}),vec![]));
    children.push(view(
        json!({"position":"absolute","left":x,"top":y,"width":w,"height":h,
        "borderBottomLeftRadius":s*0.1,"borderBottomRightRadius":s*0.1,"borderTopLeftRadius":s*0.04,
        "borderTopRightRadius":s*0.04,"background":body,"border":format!("1px solid {line}"),
        "overflow":"hidden","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.25)",s*0.03,s*0.1)}),
        wires,
    ));
    children.push(view(
        json!({"position":"absolute","left":x-s*0.03,"top":y-1.,"width":w+s*0.06,
        "height":s*0.05,"borderRadius":s*0.03,
        "backgroundColor":if dark {"rgba(235,238,245,0.6)"} else {"rgba(120,126,140,0.7)"}}),
        vec![],
    ));
    view(json!({"width":s,"height":s}), children)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_dock_symbols_match_the_original_jsx_geometry() {
        use crate::runtime::native::Prop;
        fn compare(expected: &Value, actual: &Value) {
            assert_eq!(expected["kind"], actual["kind"]);
            let mut reference = expected["style"].clone();
            // The authored JSX used CSS transform strings. The native tree
            // emits the equivalent engine property directly.
            if let Some(transform) = reference
                .as_object_mut()
                .and_then(|o| o.remove("transform"))
            {
                let angle: f64 = transform
                    .as_str()
                    .unwrap()
                    .trim_start_matches("rotate(")
                    .trim_end_matches("deg)")
                    .parse()
                    .unwrap();
                reference["rotate"] = json!(angle);
            }
            let a = super::super::style::normalize(&reference);
            let b = super::super::style::normalize(&actual["style"]);
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
            for (key, left) in a {
                match (&left, &b[&key]) {
                    (Prop::Num(x), Prop::Num(y)) => {
                        assert!((x - y).abs() < 1e-9, "{key}: {x} != {y}")
                    }
                    _ => assert_eq!(left, b[&key], "{key}"),
                }
            }
            let a = expected["children"].as_array().unwrap();
            let b = actual["children"].as_array().unwrap();
            assert_eq!(a.len(), b.len());
            for (expected, actual) in a.iter().zip(b) {
                compare(expected, actual);
            }
        }
        let cases: Value =
            serde_json::from_str(include_str!("../../tests/data/dock-icons.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let size = case["size"].as_f64().unwrap();
            let tree = if case["symbol"] == "launcher" {
                launcher(size)
            } else {
                trash(size, case["full"] == true, case["dark"] == true)
            };
            compare(&case["tree"], &tree);
        }
    }
    #[test]
    fn dock_symbols_preserve_geometry_and_trash_state() {
        for size in [20., 32., 54., 86., 128.] {
            let icon = launcher(size);
            assert_eq!(icon["children"].as_array().unwrap().len(), 9);
            assert_eq!(icon["style"]["width"], size);
            for dark in [false, true] {
                let empty = trash(size, false, dark);
                let full = trash(size, true, dark);
                assert_eq!(empty["children"].as_array().unwrap().len(), 2);
                assert_eq!(full["children"].as_array().unwrap().len(), 4);
                assert_eq!(full["children"][0]["style"]["rotate"], -14);
                assert_eq!(full["children"][1]["style"]["rotate"], 12);
                assert_eq!(full["children"][2], empty["children"][0]);
            }
        }
    }
}
