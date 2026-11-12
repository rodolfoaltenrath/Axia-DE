//! Shared app artwork for Rust views, including icons of apps still being migrated.
//! Compiled apps draw their own icons; package and desktop files use descriptors.
use serde_json::{json, Value};
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}

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
        "calendar" => {
            let now = unsafe { libc::time(std::ptr::null_mut()) };
            let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
            let day = if unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
                1
            } else {
                tm.tm_mday
            };
            let dots=(0..3).map(|_|view(json!({"width":s*0.07,"height":s*0.07,"borderRadius":s*0.035,"backgroundColor":"rgba(255,255,255,0.85)"}),vec![])).collect();
            let mut node = base(
                s,
                "#ffffff",
                vec![
                    view(
                        json!({"height":s*0.27,"backgroundColor":"#f2463d","alignItems":"center","justifyContent":"center"}),
                        vec![view(json!({"flexDirection":"row","gap":s*0.05}), dots)],
                    ),
                    view(
                        json!({"flex":1,"alignItems":"center","justifyContent":"center"}),
                        vec![text(
                            json!(day.to_string()),
                            json!({"fontSize":s*0.46,"fontWeight":300,"color":"#1c1c1e"}),
                        )],
                    ),
                ],
            );
            node["style"]["border"] = json!("1px solid rgba(0,0,0,0.08)");
            node
        }
        "contacts" => {
            let card = view(
                json!({"position":"absolute","left":s*0.24,"top":s*0.14,"width":s*0.62,"height":s*0.72,"borderRadius":s*0.06,"backgroundColor":"#f4f1ec","overflow":"hidden","alignItems":"center"}),
                vec![
                    view(
                        json!({"marginTop":s*0.12,"width":s*0.22,"height":s*0.22,"borderRadius":s*0.11,"backgroundColor":"#9b9b9f"}),
                        vec![],
                    ),
                    view(
                        json!({"marginTop":s*0.04,"width":s*0.42,"height":s*0.42,"borderRadius":s*0.21,"backgroundColor":"#9b9b9f"}),
                        vec![],
                    ),
                ],
            );
            let mut children = vec![
                view(
                    json!({"position":"absolute","left":0,"top":0,"bottom":0,"width":s*0.16,"backgroundColor":"#6f5843"}),
                    vec![],
                ),
                card,
            ];
            children.extend(
                [0.3, 0.5, 0.7]
                    .into_iter()
                    .map(|y| rect(0.86, y, 0.06, 0.1, 0.02, "#e0a33a")),
            );
            base(s, "linear-gradient(180deg, #b49a7e, #8c7259)", children)
        }
        "mail" => {
            let mut children = vec![shadow(
                rect(0.16, 0.27, 0.68, 0.46, 0.06, "#ffffff"),
                s,
                0.02,
                0.05,
                0.25,
            )];
            for i in 0..5 {
                let mut band = rect(
                    0.2 + i as f64 * 0.06,
                    0.31 + i as f64 * 0.045,
                    0.6 - i as f64 * 0.12,
                    0.04,
                    0.02,
                    if i == 4 { "#0a5fd0" } else { "#cfe6ff" },
                );
                band["style"]["height"] = json!((s * 0.04).max(1.));
                children.push(band);
            }
            base(s, "linear-gradient(165deg, #3fa9ff, #0a5fd0)", children)
        }
        "passwords" => {
            let shackle = view(
                json!({"position":"absolute","left":s*0.3,"top":s*0.14,"width":s*0.4,"height":s*0.4,"borderRadius":s*0.2,"border":format!("{}px solid #ffffff",(s*0.075).max(2.))}),
                vec![],
            );
            let dots=(0..3).map(|_|view(json!({"width":s*0.09,"height":s*0.09,"borderRadius":s*0.045,"backgroundColor":"#1f6fd1"}),vec![])).collect();
            let body = shadow(
                view(
                    json!({"position":"absolute","left":s*0.19,"top":s*0.38,"width":s*0.62,"height":s*0.44,"borderRadius":s*0.1,"backgroundColor":"#ffffff","flexDirection":"row","alignItems":"center","justifyContent":"center","gap":s*0.05}),
                    dots,
                ),
                s,
                0.02,
                0.05,
                0.25,
            );
            base(
                s,
                "linear-gradient(160deg, #3fd2c7, #1f6fd1)",
                vec![shackle, body],
            )
        }
        "sysinfo" | "sysprobe" => {
            let chip = view(
                json!({"width":s*0.5,"height":s*0.5,"borderRadius":s*0.08,"border":format!("{}px solid rgba(255,255,255,0.95)",(s*0.04).max(1.)),"alignItems":"center","justifyContent":"center"}),
                vec![view(
                    json!({"width":s*0.22,"height":s*0.22,"borderRadius":s*0.03,"backgroundColor":"#FFD60A"}),
                    vec![],
                )],
            );
            let mut children = vec![chip];
            for p in [0.32, 0.5, 0.68] {
                for edge in ["top", "bottom"] {
                    children.push(view(json!({"position":"absolute","left":s*p-s*0.02,edge:s*0.16,"width":s*0.04,"height":s*0.08,"backgroundColor":"rgba(255,255,255,0.8)"}),vec![]));
                }
            }
            view(
                json!({"width":s,"height":s,"borderRadius":s*0.22,"alignItems":"center","justifyContent":"center","background":"linear-gradient(135deg, #B46CFF, #4F46E5)"}),
                children,
            )
        }
        "reminders" => {
            let children=["#007aff","#ff3b30","#ff9500"].into_iter().enumerate().map(|(i,c)|view(json!({"flexDirection":"row","alignItems":"center","gap":s*0.08}),vec![view(json!({"width":s*0.13,"height":s*0.13,"borderRadius":s*0.065,"backgroundColor":c}),vec![]),view(json!({"width":s*if i==2{0.3}else{0.42},"height":(s*0.035).max(1.),"borderRadius":s*0.02,"backgroundColor":"#c7c7cc"}),vec![])])).collect();
            let mut node = base(s, "#ffffff", children);
            node["style"]["border"] = json!("1px solid rgba(0,0,0,0.08)");
            node["style"]["paddingHorizontal"] = json!(s * 0.17);
            node["style"]["justifyContent"] = json!("center");
            node["style"]["gap"] = json!(s * 0.09);
            node
        }
        _ => return None,
    })
}

/// The fallback icon colours for an app without a PNG, as `LetterIcon` (sdk/apps.js):
/// a hue from the name, light to dark.
pub fn letter_colors(name: &str) -> (String, String) {
    let hue = name
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32))
        % 360;
    let hsl = |l: f64| {
        let a = 0.55 * l.min(1. - l);
        let f = |n: f64| {
            let k = (n + hue as f64 / 30.) % 12.;
            l - a * (k - 3.).min(9. - k).min(1.).max(-1.)
        };
        format!(
            "#{:02x}{:02x}{:02x}",
            (f(0.) * 255.).round() as u8,
            (f(8.) * 255.).round() as u8,
            (f(4.) * 255.).round() as u8
        )
    };
    (hsl(0.52), hsl(0.36))
}

/// Serializable icon facts carry file/letter resources; compiled artwork stays in Rust.
pub fn from_descriptor(facts: &Value, size: f64, fallback: &str) -> Value {
    let size = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    let name = facts["name"].as_str().unwrap_or(fallback);
    match facts["kind"].as_str().unwrap_or("") {
        "image" => {
            if let Some(path) = facts["path"]
                .as_str()
                .filter(|p| !p.is_empty() && p.len() <= 4096 && !p.contains('\0'))
            {
                return json!({"kind":"image","src":path,"style":{"width":size,"height":size,"objectFit":"contain"}});
            }
        }
        "app" => {
            if let Some(icon) = facts["id"].as_str().and_then(|id| app_icon(id, size)) {
                return icon;
            }
        }
        _ => {}
    }
    let (light, dark) = letter_colors(name);
    let letter = name
        .rsplit('.')
        .next()
        .unwrap_or(name)
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".into());
    view(
        json!({"width":size,"height":size,"borderRadius":size*0.225,"alignItems":"center","justifyContent":"center","background":format!("linear-gradient(180deg, {light}, {dark})")}),
        vec![text(
            json!(letter),
            json!({"fontSize":size*0.5,"fontWeight":700,"color":"#fff"}),
        )],
    )
}

/// A window's or an app's icon for the shell's native views (the Dock, previews): an image path
/// from its desktop entry or package, the compiled app's own drawing, or its initial.
pub fn tree(spec: &Value, facts: &Value) -> Value {
    let size = spec["size"]
        .as_f64()
        .filter(|s| s.is_finite())
        .unwrap_or(32.)
        .clamp(1., 512.);
    let window = spec["window"]
        .as_str()
        .and_then(|id| facts["windows"].as_array()?.iter().find(|w| w["id"] == id));
    let id = window
        .and_then(|w| w["iconApp"].as_str().or_else(|| w["key"].as_str()))
        .or_else(|| spec["app"].as_str())
        .unwrap_or("");
    let id = id.strip_prefix("app:").unwrap_or(id);
    let entry = facts["catalog"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(facts["registry"].as_array().into_iter().flatten())
        .find(|a| a["id"] == id);
    let path = window
        .and_then(|w| w["iconPath"].as_str())
        .or_else(|| entry.and_then(|a| a["iconPath"].as_str()))
        .unwrap_or("");
    if !path.is_empty() {
        return json!({"kind":"image","src":path,"style":{"width":size,"height":size,"objectFit":"contain"}});
    }
    if id == crate::native_apps::files::ID {
        return crate::files::native_icon(size);
    }
    if let Some(mut app) = super::app(id) {
        return app.request(&json!({"cmd":"icon","size":size}));
    }
    let name = window
        .and_then(|w| w["name"].as_str())
        .or_else(|| entry.and_then(|a| a["name"].as_str()))
        .unwrap_or(id);
    letter(name, size)
}
fn letter(name: &str, size: f64) -> Value {
    let hash = name
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let hue = (hash % 360) as f64;
    let hex = |light: f64| {
        let a = 0.55 * light.min(1. - light);
        let f = |n: f64| {
            let k = (n + hue / 30.) % 12.;
            ((light - a * (-1f64).max((k - 3.).min(9. - k).min(1.))) * 255.).round() as u8
        };
        format!("#{:02x}{:02x}{:02x}", f(0.), f(8.), f(4.))
    };
    let initial = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".into());
    json!({"kind":"view","style":{"width":size,"height":size,"borderRadius":size*0.225,
        "alignItems":"center","justifyContent":"center",
        "background":format!("linear-gradient(180deg, {}, {})",hex(0.52),hex(0.36))},
        "children":[{"kind":"text","label":initial,"style":{"fontSize":size*0.5,"fontWeight":700,"color":"#fff"}}]})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_shipped_application_has_rust_artwork() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps");
        let mut ids: Vec<String> = crate::native_apps::manifests()
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap().into())
            .collect();
        for entry in std::fs::read_dir(root).unwrap().flatten() {
            if let Ok(raw) = std::fs::read_to_string(entry.path().join("manifest.json")) {
                let manifest: Value = serde_json::from_str(&raw).unwrap();
                ids.push(manifest["id"].as_str().unwrap().into());
            }
        }
        for id in ids {
            for size in [20., 64.] {
                let icon =
                    app_icon(&id, size).unwrap_or_else(|| panic!("missing native artwork: {id}"));
                assert_eq!(icon["kind"], "view");
                assert_eq!(icon["style"]["width"], size);
            }
        }
    }
    #[test]
    fn image_resources_and_fallback_letters_keep_the_sdk_contract() {
        let image = from_descriptor(&json!({"kind":"image","path":"/tmp/icon.png"}), 64., "App");
        assert_eq!(image["kind"], "image");
        assert_eq!(image["style"]["objectFit"], "contain");
        let fallback = from_descriptor(
            &json!({"kind":"letter","name":"org.example.Terminal"}),
            20.,
            "ignored",
        );
        assert_eq!(fallback["children"][0]["label"], "T");
        let (light, dark) = letter_colors("org.example.Terminal");
        assert_eq!(
            fallback["style"]["background"],
            format!("linear-gradient(180deg, {light}, {dark})")
        );
        let unknown = from_descriptor(
            &json!({"kind":"app","id":"unregistered","name":"ß"}),
            64.,
            "ignored",
        );
        assert_eq!(unknown["children"][0]["label"], "SS");
        assert_eq!(
            from_descriptor(&json!({"kind":"image","path":"\0bad"}), 20., "?")["kind"],
            "view"
        );
    }
}

#[cfg(test)]
mod tree_tests {
    use super::*;
    #[test]
    fn native_geometry_paths_window_identity_and_unicode_fallback() {
        let facts = json!({"catalog":[{"id":"desktop:browser","iconPath":"/usr/share/icons/browser.png","name":"Browser"}],
            "windows":[{"id":"linux#1","key":"desktop:browser","name":"Browser","iconPath":"/usr/share/icons/browser.png"}]});
        let icon = tree(&json!({"window":"linux#1","size":20}), &facts);
        assert_eq!(icon["src"], "/usr/share/icons/browser.png");
        assert_eq!(icon["style"]["width"], 20.);
        let calculator = tree(&json!({"app":"calculator","size":64}), &facts);
        assert!(calculator.to_string().contains("#9be7b0"));
        let files = tree(&json!({"app":"files","size":32}), &facts);
        assert_eq!(files["kind"], "view");
        assert_eq!(files["children"][0]["kind"], "view");
        let fallback = tree(&json!({"app":"org.example.área","size":-1}), &facts);
        assert_eq!(fallback["children"][0]["label"], "Á");
        assert_eq!(fallback["style"]["width"], 1.);
        assert_eq!(
            letter("Demo", 32.)["style"]["background"],
            "linear-gradient(180deg, #41c8c6, #298e8d)"
        );
    }
}
