//! Article/tour artwork for apps which still expose JSX icons. Native apps use
//! their own icon factory; these remaining drawings keep Help independent of React.
use super::ui::{text, view};
use serde_json::{json, Value};
fn base(s: f64, bg: &str, children: Vec<Value>) -> Value {
    let mut style = json!({"width":s,"height":s,"borderRadius":s*0.225,"boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden"});
    style[if bg.starts_with("linear-") {
        "background"
    } else {
        "backgroundColor"
    }] = json!(bg);
    view(style, children)
}
fn shape(s: f64, x: f64, y: f64, w: f64, h: f64, r: f64, color: &str) -> Value {
    let mut style = json!({"position":"absolute","left":s*x,"top":s*y,"width":s*w,"height":s*h,"borderRadius":s*r});
    style[if color.starts_with("linear-") {
        "background"
    } else {
        "backgroundColor"
    }] = json!(color);
    view(style, vec![])
}
fn shadow(mut node: Value, s: f64, y: f64, blur: f64, alpha: f64) -> Value {
    node["style"]["boxShadow"] = json!(format!("0 {}px {}px rgba(0,0,0,{alpha})", s * y, s * blur));
    node
}
pub fn app_icon(id: &str, size: f64) -> Option<Value> {
    if let Some(icon) = crate::native_view::icon_tree(id, size) {
        return Some(icon);
    }
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    let rect = |x, y, w, h, r, c| shape(s, x, y, w, h, r, c);
    Some(match id {
        "files" => base(
            s,
            "linear-gradient(180deg, #74c7ff, #1f6fe8)",
            vec![
                rect(0.2, 0.27, 0.27, 0.12, 0.03, "rgba(255,255,255,0.75)"),
                rect(0.2, 0.33, 0.6, 0.42, 0.06, "rgba(255,255,255,0.95)"),
            ],
        ),
        "terminal" => {
            let mut icon = base(
                s,
                "linear-gradient(180deg, #2c2c31, #111113)",
                vec![text(
                    json!(">_"),
                    json!({"position":"absolute","left":s*0.17,"top":s*0.16,"fontSize":s*0.32,"fontWeight":700,"color":"#4ade80"}),
                )],
            );
            icon["style"]["border"] = json!("1px solid rgba(255,255,255,0.15)");
            icon
        }
        "settings" => base(
            s,
            "linear-gradient(180deg, #c4c8d0, #6c717c)",
            vec![
                rect(0.44, 0.14, 0.12, 0.72, 0.04, "#3b3e46"),
                rect(0.14, 0.44, 0.72, 0.12, 0.04, "#3b3e46"),
                rect(0.21, 0.21, 0.58, 0.58, 0.29, "#3b3e46"),
                rect(
                    0.36,
                    0.36,
                    0.28,
                    0.28,
                    0.14,
                    "linear-gradient(180deg, #e5e7eb, #9ca3af)",
                ),
            ],
        ),
        "editor" => {
            let mut children = vec![shadow(
                rect(0.2, 0.16, 0.6, 0.7, 0.06, "#fbfaff"),
                s,
                0.02,
                0.06,
                0.3,
            )];
            for (w, c, top) in [
                (0.36, "#fc5fa3", 0.27),
                (0.26, "#5dd8ff", 0.37),
                (0.4, "#8b5cf6", 0.47),
                (0.2, "#d0bf69", 0.57),
                (0.32, "#67b7a4", 0.67),
            ] {
                children.push(rect(0.3, top, w, 0.055, 0.03, c));
            }
            children.push(rect(0.22, 0.255, 0.03, 0.48, 0., "#c4b5fd"));
            base(s, "linear-gradient(160deg, #8b5cf6, #3b1f8f)", children)
        }
        "logs" => {
            let lights=["#ff5f57","#febc2e","#28c840"].into_iter().map(|c|view(json!({"width":s*0.08,"height":s*0.08,"borderRadius":s*0.04,"backgroundColor":c}),vec![])).collect();
            let mut children = vec![view(
                json!({"flexDirection":"row","gap":s*0.05,"marginBottom":s*0.02}),
                lights,
            )];
            for (i, (c, w)) in [
                ("#ff453a", 0.42),
                ("#d0d0d6", 0.62),
                ("#ffd60a", 0.5),
                ("#d0d0d6", 0.68),
                ("#30d158", 0.36),
            ]
            .into_iter()
            .enumerate()
            {
                children.push(view(json!({"flexDirection":"row","gap":s*0.05,"alignItems":"center"}),vec![view(json!({"width":s*0.06,"height":s*0.06,"borderRadius":s*0.03,"backgroundColor":c}),vec![]),view(json!({"width":s*w,"height":(s*0.045).max(1.),"borderRadius":s*0.02,"backgroundColor":if i%2==1{"rgba(255,255,255,0.45)"}else{"rgba(255,255,255,0.7)"}}),vec![])]));
            }
            let mut node = base(s, "#2b2d33", children);
            node["style"]["padding"] = json!(s * 0.16);
            node["style"]["gap"] = json!(s * 0.075);
            node["style"]["direction"] = json!("ltr");
            node
        }
        "screenshots" => {
            let mut children = vec![];
            for (x, y, w, h) in [
                (0.18, 0.2, 0.18, 0.05),
                (0.18, 0.2, 0.05, 0.18),
                (0.64, 0.2, 0.18, 0.05),
                (0.77, 0.2, 0.05, 0.18),
                (0.18, 0.75, 0.18, 0.05),
                (0.18, 0.62, 0.05, 0.18),
                (0.64, 0.75, 0.18, 0.05),
                (0.77, 0.62, 0.05, 0.18),
            ] {
                children.push(rect(x, y, w, h, 0.02, "#fff"));
            }
            children.extend([
                rect(0.36, 0.38, 0.28, 0.24, 0.05, "rgba(255,255,255,0.9)"),
                rect(0.44, 0.43, 0.12, 0.12, 0.06, "#2c333d"),
                rect(0.47, 0.46, 0.06, 0.06, 0.03, "#ff5f57"),
            ]);
            base(s, "linear-gradient(160deg, #5e6b7a, #2c333d)", children)
        }
        "camera" => base(
            s,
            "linear-gradient(160deg, #3a3f47, #15171b)",
            vec![
                rect(0.16, 0.3, 0.68, 0.46, 0.08, "#d9dde3"),
                rect(0.24, 0.23, 0.18, 0.09, 0.03, "#d9dde3"),
                rect(0.32, 0.33, 0.36, 0.36, 0.18, "#23262c"),
                rect(
                    0.38,
                    0.39,
                    0.24,
                    0.24,
                    0.12,
                    "linear-gradient(135deg, #5b8cff, #1d2a66)",
                ),
                rect(0.44, 0.43, 0.06, 0.06, 0.03, "rgba(255,255,255,0.8)"),
                rect(0.72, 0.35, 0.06, 0.06, 0.03, "#30d158"),
            ],
        ),
        "player" => {
            let mut children = vec![rect(0.14, 0.22, 0.72, 0.56, 0.08, "rgba(20,10,30,0.82)")];
            for y in [0.25, 0.7] {
                for i in 0..4 {
                    children.push(rect(
                        0.2 + i as f64 * 0.17,
                        y,
                        0.08,
                        0.05,
                        0.015,
                        "rgba(255,255,255,0.35)",
                    ));
                }
            }
            let steps = (s / 5.).round().max(6.) as usize;
            for i in 0..steps {
                let f = 1. - ((i as f64 + 0.5) / steps as f64 - 0.5).abs() * 2.;
                children.push(view(json!({"position":"absolute","left":s*0.42,"top":s*0.36+s*0.28*i as f64/steps as f64,"width":(s*0.24*f).max(1.),"height":s*0.28/steps as f64+0.6,"backgroundColor":"#fff"}),vec![]));
            }
            base(s, "linear-gradient(160deg, #ff7a59, #d6246e)", children)
        }
        "pdf" => {
            let mut children = vec![
                rect(0.3, 0.12, 0.5, 0.64, 0.04, "rgba(255,255,255,0.55)"),
                shadow(
                    rect(0.2, 0.2, 0.52, 0.66, 0.045, "#ffffff"),
                    s,
                    0.02,
                    0.06,
                    0.3,
                ),
            ];
            for (i, y) in [0.32, 0.42, 0.52, 0.62, 0.72].into_iter().enumerate() {
                let mut line = rect(
                    0.28,
                    y,
                    if i == 4 { 0.22 } else { 0.36 },
                    0.04,
                    0.02,
                    if i == 2 { "#ffd60a" } else { "#d4d4dc" },
                );
                line["style"]["height"] = json!((s * 0.04).max(1.));
                children.push(line);
            }
            children.push(text(json!("PDF"),json!({"position":"absolute","left":s*0.48,"top":s*0.66,"fontSize":s*0.13,"fontWeight":700,"color":"#c8102e"})));
            base(s, "linear-gradient(160deg, #ff7a59, #c8102e)", children)
        }
        "scanner" => base(
            s,
            "linear-gradient(160deg, #66d4cf, #0f7c78)",
            vec![
                rect(0.12, 0.46, 0.76, 0.3, 0.06, "#e9f7f6"),
                rect(0.12, 0.3, 0.76, 0.12, 0.05, "#cfe9e7"),
                rect(0.18, 0.54, 0.64, 0.05, 0., "#7cf5a0"),
                rect(0.18, 0.64, 0.3, 0.04, 0., "#9fbfbd"),
            ],
        ),
        "notes" => {
            let mut children = vec![shadow(
                rect(0.2, 0.18, 0.6, 0.64, 0.05, "#fffbe0"),
                s,
                0.02,
                0.05,
                0.25,
            )];
            for i in 0..4 {
                let mut line = rect(
                    0.28,
                    0.32 + i as f64 * 0.11,
                    if i == 3 { 0.26 } else { 0.44 },
                    0.035,
                    0.02,
                    if i == 0 { "#e0a800" } else { "#c9b98a" },
                );
                line["style"]["height"] = json!((s * 0.035).max(1.));
                children.push(line);
            }
            children.push(rect(0.66, 0.68, 0.14, 0.14, 0.03, "#f3d77a"));
            base(s, "linear-gradient(160deg, #ffe066, #f7b733)", children)
        }
        "photos" => {
            let mut children = vec![];
            for (c, x, y) in [
                ("#ffb340", 0.35, 0.16),
                ("#ff5f57", 0.55, 0.27),
                ("#c86dd7", 0.55, 0.47),
                ("#4c8dff", 0.35, 0.58),
                ("#33c7a8", 0.15, 0.47),
                ("#9bd44a", 0.15, 0.27),
            ] {
                let mut p = rect(x, y, 0.3, 0.3, 0.15, c);
                p["style"]["opacity"] = json!(0.88);
                children.push(p);
            }
            children.push(rect(0.42, 0.4, 0.16, 0.16, 0.08, "#fff"));
            base(s, "#fbfbfd", children)
        }
        "fonts" => {
            let mut children = vec![
                text(
                    json!("A"),
                    json!({"position":"absolute","left":s*0.12,"top":s*0.06,"fontSize":s*0.62,"fontWeight":700,"color":"#1c1c1e"}),
                ),
                text(
                    json!("a"),
                    json!({"position":"absolute","left":s*0.52,"top":s*0.36,"fontSize":s*0.44,"fontWeight":400,"color":"#d70015"}),
                ),
            ];
            children.push(view(json!({"position":"absolute","left":s*0.12,"right":s*0.12,"bottom":s*0.16,"height":(s*0.03).max(1.),"backgroundColor":"#8e8e93"}),vec![]));
            base(s, "linear-gradient(160deg, #f5f5f7, #c7c7cc)", children)
        }
        "recorder" => base(
            s,
            "linear-gradient(160deg, #2b2b30, #0e0e10)",
            [0.18, 0.34, 0.52, 0.3, 0.6, 0.42, 0.24, 0.12]
                .into_iter()
                .enumerate()
                .map(|(i, h)| {
                    rect(
                        0.16 + i as f64 * 0.09,
                        0.5 - h / 2.,
                        0.05,
                        h,
                        0.025,
                        if i == 4 { "#fff" } else { "#ff453a" },
                    )
                })
                .collect(),
        ),
        "disks" => {
            let platter = view(
                json!({"width":s*0.4,"height":s*0.4,"borderRadius":s*0.2,"background":"linear-gradient(140deg, #ffffff, #8f9aa8)","border":format!("{}px solid #7d8794",(s*0.02).max(1.)),"alignItems":"center","justifyContent":"center"}),
                vec![view(
                    json!({"width":s*0.1,"height":s*0.1,"borderRadius":s*0.05,"backgroundColor":"#5b6573"}),
                    vec![],
                )],
            );
            let case = shadow(
                view(
                    json!({"width":s*0.7,"height":s*0.56,"borderRadius":s*0.08,"background":"linear-gradient(180deg, #f2f4f7, #b9c0ca)","alignItems":"center","justifyContent":"center"}),
                    vec![
                        platter,
                        view(
                            json!({"position":"absolute","right":s*0.08,"top":s*0.08,"width":s*0.05,"height":s*0.26,"borderRadius":s*0.025,"backgroundColor":"#4b5563"}),
                            vec![],
                        ),
                        view(
                            json!({"position":"absolute","left":s*0.06,"bottom":s*0.05,"width":s*0.06,"height":s*0.06,"borderRadius":s*0.03,"backgroundColor":"#30d158"}),
                            vec![],
                        ),
                    ],
                ),
                s,
                0.02,
                0.05,
                0.4,
            );
            let mut node = base(s, "linear-gradient(160deg, #5e6b7d, #2b323d)", vec![case]);
            node["style"]["alignItems"] = json!("center");
            node["style"]["justifyContent"] = json!("center");
            node
        }
        "backup" => {
            let ring = s * 0.62;
            let bw = (s * 0.06).max(1.5);
            let circle = view(
                json!({"width":ring,"height":ring,"borderRadius":ring/2.,"border":format!("{bw}px solid #fff"),"alignItems":"center","justifyContent":"center"}),
                vec![
                    view(
                        json!({"position":"absolute","left":ring/2.-bw/2.-bw,"top":ring*0.16,"width":bw,"height":ring*0.36,"backgroundColor":"#fff","borderRadius":bw/2.}),
                        vec![],
                    ),
                    view(
                        json!({"position":"absolute","left":ring/2.-bw,"top":ring/2.-bw*1.5,"width":ring*0.3,"height":bw,"backgroundColor":"#fff","borderRadius":bw/2.}),
                        vec![],
                    ),
                ],
            );
            let mut node = view(
                json!({"width":s,"height":s,"borderRadius":s*0.225,"alignItems":"center","justifyContent":"center","background":"linear-gradient(180deg, #2e8b57 0%, #1d5c3a 100%)","direction":"ltr"}),
                vec![
                    circle,
                    rect(0.13, 0.17, 0.2, 0.2, 0.04, "#25744a"),
                    view(
                        json!({"position":"absolute","left":s*0.16,"top":s*0.3,"width":s*0.17,"height":bw*1.4,"backgroundColor":"#fff","borderRadius":bw}),
                        vec![],
                    ),
                    view(
                        json!({"position":"absolute","left":s*0.26,"top":s*0.2,"width":bw*1.4,"height":s*0.13,"backgroundColor":"#fff","borderRadius":bw}),
                        vec![],
                    ),
                    view(
                        json!({"position":"absolute","left":s*0.25,"bottom":s*0.08,"width":s*0.5,"height":s*0.07,"borderRadius":s*0.03,"backgroundColor":"rgba(255,255,255,0.55)"}),
                        vec![],
                    ),
                ],
            );
            // No shadow in the original backup icon.
            node["style"]["direction"] = json!("ltr");
            node
        }
        _ => return None,
    })
}
