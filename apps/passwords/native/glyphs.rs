//! The icons of Passwords, drawn with views (no color emoji, docs/DESIGN.md): the same
//! shapes as apps/passwords/glyphs.jsx, node for node, so the native window looks the
//! same. A category stores the name of its glyph ("globe", "key"…); categories saved
//! by older versions stored an emoji, which `icon_kind` maps to the closest glyph.
use serde_json::{json, Value};

/// The glyphs a user category can pick (Settings → New category), in picker order.
pub const GLYPH_KINDS: [&str; 22] = [
    "folder", "globe", "mail", "bank", "card", "network", "wifi", "terminal", "database", "apps", "key", "braces", "id", "coin", "shield",
    "note", "lock", "star", "person", "briefcase", "clock", "drive",
];

const LEGACY: [(&str, &str); 44] = [
    ("🗂", "folder"), ("📁", "folder"), ("📂", "folder"), ("🏠", "folder"), ("🌐", "globe"), ("✉", "mail"), ("📧", "mail"),
    ("🏦", "bank"), ("💰", "bank"), ("💳", "card"), ("🖧", "network"), ("📡", "network"), ("📶", "wifi"), ("🖥", "terminal"),
    ("💻", "terminal"), ("🗄", "database"), ("📱", "apps"), ("🎮", "apps"), ("🔑", "key"), ("🗝", "key"), ("🧩", "braces"),
    ("🪪", "id"), ("🪙", "coin"), ("🛡", "shield"), ("📝", "note"), ("📄", "note"), ("🔒", "lock"), ("🔐", "lock"), ("⭐", "star"),
    ("👤", "person"), ("🐧", "key"), ("💼", "briefcase"), ("🎓", "briefcase"), ("🛒", "briefcase"), ("✈", "briefcase"),
    ("🏥", "shield"), ("⚙", "apps"), ("📷", "apps"), ("🎵", "apps"), ("📦", "drive"), ("💾", "drive"), ("🗃", "folder"),
    ("🔏", "lock"), ("💡", "apps"),
];

/// The glyph of a stored icon value (a glyph name, or an emoji from an older version).
pub fn icon_kind(icon: &str, fallback: &'static str) -> String {
    if GLYPH_KINDS.contains(&icon) {
        return icon.into();
    }
    let bare: String = icon.chars().filter(|&c| c != '\u{fe0f}').collect();
    LEGACY.iter().find(|(e, _)| *e == bare).map_or(fallback, |(_, k)| k).into()
}

fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(a), Some(b)) = (a.as_object_mut(), b.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
    a
}

fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}

fn text(label: &str, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}

/// `kind` drawn `size` px wide in `color`.
pub fn glyph(kind: &str, size: f64, color: &str) -> Value {
    let (s, c) = (size, color);
    let w = ((s / 11. * 2.).round() / 2.).max(1.5); // stroke
    let bw = format!("{w}px solid {c}");
    let bw13 = format!("{}px solid {c}", w * 1.3);
    let boxed = |children: Vec<Value>, extra: Value| {
        view(merge(json!({"width":s,"height":s,"alignItems":"center","justifyContent":"center"}), extra), children)
    };
    let b = |children: Vec<Value>| boxed(children, json!({}));
    let line = |style: Value| view(merge(json!({"position":"absolute","backgroundColor":c}), style), vec![]);
    match kind {
        "lock" => b(vec![
            view(json!({"width":s*0.5,"height":s*0.42,"marginBottom":-w,"borderTopLeftRadius":s*0.25,"borderTopRightRadius":s*0.25,
                "borderTopWidth":w,"borderLeftWidth":w,"borderRightWidth":w,"borderColor":c}), vec![]),
            view(json!({"width":s*0.74,"height":s*0.48,"borderRadius":s*0.1,"backgroundColor":c}), vec![]),
        ]),
        "star" => b(vec![text("★", json!({"fontSize":s*0.92,"lineHeight":s,"color":c}))]),
        "archive" => b(vec![
            view(json!({"width":s*0.9,"height":s*0.2,"borderRadius":1.5,"backgroundColor":c,"marginBottom":1}), vec![]),
            view(json!({"width":s*0.76,"height":s*0.48,"borderBottomLeftRadius":s*0.1,"borderBottomRightRadius":s*0.1,"border":bw,"alignItems":"center"}),
                vec![view(json!({"marginTop":s*0.1,"width":s*0.28,"height":w,"backgroundColor":c}), vec![])]),
        ]),
        "trash" => b(vec![
            view(json!({"width":s*0.3,"height":s*0.1,"borderTopLeftRadius":1.5,"borderTopRightRadius":1.5,"backgroundColor":c}), vec![]),
            view(json!({"width":s*0.8,"height":w,"backgroundColor":c,"marginBottom":1}), vec![]),
            view(json!({"width":s*0.6,"height":s*0.54,"borderBottomLeftRadius":s*0.1,"borderBottomRightRadius":s*0.1,"border":bw}), vec![]),
        ]),
        "folder" => b(vec![view(json!({"width":s*0.86,"height":s*0.66}), vec![
            view(json!({"width":s*0.36,"height":s*0.16,"borderTopLeftRadius":s*0.08,"borderTopRightRadius":s*0.08,"backgroundColor":c}), vec![]),
            view(json!({"flex":1,"borderRadius":s*0.1,"borderTopLeftRadius":0,"border":bw}), vec![]),
        ])]),
        "clock" => b(vec![view(json!({"width":s*0.84,"height":s*0.84,"borderRadius":s*0.42,"border":bw}), vec![
            line(json!({"left":s*0.42-w*1.5,"top":s*0.16,"width":w,"height":s*0.28})),
            line(json!({"left":s*0.42-w*1.5,"top":s*0.42-w*1.5,"width":s*0.22,"height":w})),
        ])]),
        "wifi" => b(vec![view(json!({"flexDirection":"row","alignItems":"flex-end","gap":s*0.1,"height":s*0.76,"direction":"ltr"}),
            [0.32, 0.54, 0.76].iter().map(|h| view(json!({"width":s*0.2,"height":s*h,"borderRadius":1.5,"backgroundColor":c}), vec![])).collect())]),
        "note" => b(vec![view(json!({"width":s*0.7,"height":s*0.86,"borderRadius":s*0.1,"border":bw,"paddingTop":s*0.14,"paddingHorizontal":s*0.1,"gap":s*0.1}),
            [1., 0.8, 0.55].iter().map(|f| view(json!({"width":format!("{}%", f*100.),"height":w,"backgroundColor":c}), vec![])).collect())]),
        "card" => b(vec![view(json!({"width":s*0.9,"height":s*0.64,"borderRadius":s*0.1,"border":bw,"overflow":"hidden"}),
            vec![view(json!({"marginTop":s*0.1,"height":s*0.14,"backgroundColor":c}), vec![])])]),
        "shield" => b(vec![view(json!({"width":s*0.7,"height":s*0.84,"borderTopLeftRadius":s*0.08,"borderTopRightRadius":s*0.08,
            "borderBottomLeftRadius":s*0.35,"borderBottomRightRadius":s*0.35,"border":bw}), vec![])]),
        "person" => b(vec![
            view(json!({"width":s*0.4,"height":s*0.4,"borderRadius":s*0.2,"backgroundColor":c,"marginBottom":s*0.06}), vec![]),
            view(json!({"width":s*0.76,"height":s*0.34,"borderTopLeftRadius":s*0.38,"borderTopRightRadius":s*0.38,"backgroundColor":c}), vec![]),
        ]),
        "globe" => b(vec![view(json!({"width":s*0.86,"height":s*0.86,"borderRadius":s*0.43,"border":bw,"alignItems":"center","justifyContent":"center"}), vec![
            view(json!({"width":s*0.36,"height":s*0.86-w*2.,"borderRadius":s*0.18,"border":bw}), vec![]),
            line(json!({"left":0,"right":0,"top":s*0.43-w*1.5,"height":w})),
        ])]),
        "mail" => b(vec![view(json!({"width":s*0.9,"height":s*0.64,"borderRadius":s*0.08,"border":bw,"overflow":"hidden","alignItems":"center"}),
            vec![view(json!({"position":"absolute","top":-s*0.34,"width":s*0.6,"height":s*0.6,"borderRadius":s*0.06,"border":bw,"transform":"rotate(45deg)"}), vec![])])]),
        "bank" => b(vec![
            view(json!({"width":s*0.84,"height":s*0.16,"borderTopLeftRadius":s*0.3,"borderTopRightRadius":s*0.3,"backgroundColor":c}), vec![]),
            view(json!({"flexDirection":"row","gap":s*0.12,"height":s*0.4,"marginVertical":s*0.04}),
                (0..3).map(|_| view(json!({"width":s*0.12,"backgroundColor":c}), vec![])).collect()),
            view(json!({"width":s*0.84,"height":s*0.12,"backgroundColor":c}), vec![]),
        ]),
        "network" => b(vec![
            view(json!({"width":s*0.36,"height":s*0.26,"borderRadius":s*0.05,"backgroundColor":c}), vec![]),
            view(json!({"width":w,"height":s*0.12,"backgroundColor":c}), vec![]),
            view(json!({"width":s*0.66,"height":w,"backgroundColor":c}), vec![]),
            view(json!({"flexDirection":"row","width":s*0.9,"justifyContent":"space-between"}), (0..2).map(|_| view(json!({"alignItems":"center"}), vec![
                view(json!({"width":w,"height":s*0.1,"backgroundColor":c}), vec![]),
                view(json!({"width":s*0.32,"height":s*0.22,"borderRadius":s*0.05,"backgroundColor":c}), vec![]),
            ])).collect()),
        ]),
        "terminal" => b(vec![view(json!({"width":s*0.9,"height":s*0.72,"borderRadius":s*0.1,"border":bw,"justifyContent":"center","paddingInlineStart":s*0.08,"direction":"ltr"}),
            vec![text(">_", json!({"fontSize":s*0.42,"lineHeight":s*0.5,"fontWeight":700,"color":c}))])]),
        "database" => b(vec![view(json!({"gap":s*0.04}),
            (0..3).map(|_| view(json!({"width":s*0.72,"height":s*0.24,"borderRadius":s*0.12,"border":bw}), vec![])).collect())]),
        "apps" => b(vec![view(json!({"width":s*0.78,"flexDirection":"row","flexWrap":"wrap","gap":s*0.1}),
            (0..4).map(|_| view(json!({"width":s*0.34,"height":s*0.34,"borderRadius":s*0.08,"backgroundColor":c}), vec![])).collect())]),
        "key" => b(vec![view(json!({"flexDirection":"row","alignItems":"center","direction":"ltr"}), vec![
            view(json!({"width":s*0.4,"height":s*0.4,"borderRadius":s*0.2,"border":bw13}), vec![]),
            view(json!({"width":s*0.46,"height":w*1.3,"backgroundColor":c}), vec![
                view(json!({"position":"absolute","right":0,"top":0,"width":w*1.3,"height":s*0.2,"backgroundColor":c}), vec![]),
                view(json!({"position":"absolute","right":s*0.12,"top":0,"width":w*1.3,"height":s*0.14,"backgroundColor":c}), vec![]),
            ]),
        ])]),
        "braces" => b(vec![text("{ }", json!({"fontSize":s*0.62,"lineHeight":s*0.8,"fontWeight":700,"color":c,"direction":"ltr"}))]),
        "id" => b(vec![view(json!({"width":s*0.9,"height":s*0.64,"borderRadius":s*0.1,"border":bw,"flexDirection":"row","alignItems":"center",
            "paddingHorizontal":s*0.08,"gap":s*0.08,"direction":"ltr"}), vec![
            view(json!({"width":s*0.22,"height":s*0.22,"borderRadius":s*0.11,"backgroundColor":c}), vec![]),
            view(json!({"flex":1,"gap":s*0.08}), vec![
                view(json!({"height":w,"backgroundColor":c}), vec![]),
                view(json!({"width":"70%","height":w,"backgroundColor":c}), vec![]),
            ]),
        ])]),
        "coin" => b(vec![view(json!({"width":s*0.84,"height":s*0.84,"borderRadius":s*0.42,"border":bw,"alignItems":"center","justifyContent":"center"}),
            vec![view(json!({"width":s*0.44,"height":s*0.44,"borderRadius":s*0.22,"border":bw}), vec![])])]),
        "briefcase" => b(vec![
            view(json!({"width":s*0.34,"height":s*0.16,"borderTopLeftRadius":s*0.06,"borderTopRightRadius":s*0.06,"borderTopWidth":w,"borderLeftWidth":w,"borderRightWidth":w,"borderColor":c}), vec![]),
            view(json!({"width":s*0.86,"height":s*0.56,"borderRadius":s*0.1,"backgroundColor":c}), vec![]),
        ]),
        "drive" => b(vec![
            view(json!({"width":s*0.3,"height":s*0.2,"borderTopLeftRadius":s*0.04,"borderTopRightRadius":s*0.04,"border":bw,"marginBottom":-w}), vec![]),
            view(json!({"width":s*0.5,"height":s*0.62,"borderRadius":s*0.1,"backgroundColor":c}), vec![]),
        ]),
        "eye" | "eyeOff" => {
            let mut kids = vec![view(json!({"width":s*0.9,"height":s*0.56,"borderRadius":s*0.28,"border":bw,"alignItems":"center","justifyContent":"center"}),
                vec![view(json!({"width":s*0.26,"height":s*0.26,"borderRadius":s*0.13,"backgroundColor":c}), vec![])])];
            if kind == "eyeOff" {
                kids.push(view(json!({"position":"absolute","width":s,"height":w,"backgroundColor":c,"transform":"rotate(-40deg)"}), vec![]));
            }
            b(kids)
        }
        "clip" => b(vec![view(json!({"width":s*0.34,"height":s*0.86,"borderRadius":s*0.17,"border":bw,"transform":"rotate(40deg)"}), vec![])]),
        "image" => b(vec![view(json!({"width":s*0.9,"height":s*0.72,"borderRadius":s*0.1,"border":bw,"overflow":"hidden","direction":"ltr"}), vec![
            view(json!({"position":"absolute","left":s*0.14,"top":s*0.12,"width":s*0.18,"height":s*0.18,"borderRadius":s*0.09,"backgroundColor":c}), vec![]),
            view(json!({"position":"absolute","left":s*0.2,"top":s*0.36,"width":s*0.5,"height":s*0.5,"backgroundColor":c,"transform":"rotate(45deg)"}), vec![]),
        ])]),
        _ => b(vec![]),
    }
}

/// A color moved toward white (amt > 0) or black (amt < 0), as the JS `tint`.
pub fn tint(hex: &str, amt: f64) -> String {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 || !h.bytes().all(|c| c.is_ascii_hexdigit()) {
        return hex.into();
    }
    let n = u32::from_str_radix(h, 16).unwrap();
    let ch = |v: u32| {
        let v = v as f64;
        let out = v + if amt < 0. { v * amt } else { (255. - v) * amt };
        out.round().clamp(0., 255.) as u32
    };
    let (r, g, b) = (ch(n >> 16), ch((n >> 8) & 255), ch(n & 255));
    format!("#{:06x}", (r << 16) | (g << 8) | b)
}

/// A category's tile: its color as a soft vertical gradient and its glyph in white.
pub fn tile(kind: &str, color: &str, size: f64, radius: Option<f64>) -> Value {
    view(
        json!({"width":size,"height":size,"borderRadius":radius.unwrap_or(size*0.26),"alignItems":"center","justifyContent":"center",
            "background":format!("linear-gradient(180deg, {}, {})", tint(color, 0.18), tint(color, -0.12))}),
        vec![glyph(kind, (size * 0.62).round(), "#ffffff")],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_kinds_and_tints() {
        assert_eq!(icon_kind("globe", "folder"), "globe");
        assert_eq!(icon_kind("🔑", "folder"), "key");
        assert_eq!(icon_kind("☕", "folder"), "folder");
        assert_eq!(tint("#808080", 0.5), "#c0c0c0");
        assert_eq!(tint("#808080", -0.5), "#404040");
        assert_eq!(tint("nope", 0.5), "nope");
        for k in GLYPH_KINDS.iter().chain(["archive", "trash", "eye", "eyeOff", "clip", "image"].iter()) {
            let g = glyph(k, 16., "#fff");
            assert_eq!(g["style"]["width"], 16.);
            if *k != "star" && *k != "braces" {
                assert!(!g["children"].as_array().unwrap().is_empty(), "{k}");
            }
        }
    }
}
