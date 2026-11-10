//! React-style objects to the runtime's properties: the port of `runtime/style.js`
//! for trees that Rust mounts directly. Colors become ARGB numbers and shorthands
//! (padding, border, boxShadow, flex…) become the engine's simple properties.
use crate::runtime::native::Prop;
use serde_json::Value;
use std::collections::BTreeMap;
pub type Props = BTreeMap<String, Prop>;
const NAMED: [(&str, u32); 12] = [
    ("transparent", 0x0000_0000),
    ("white", 0xffff_ffff),
    ("black", 0xff00_0000),
    ("red", 0xffff_0000),
    ("green", 0xff00_8000),
    ("blue", 0xff00_00ff),
    ("gray", 0xff80_8080),
    ("grey", 0xff80_8080),
    ("yellow", 0xffff_ff00),
    ("orange", 0xffff_a500),
    ("purple", 0xff80_0080),
    ("none", 0),
];
/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()` or a name → ARGB.
pub fn color(v: &Value) -> u32 {
    if let Some(n) = v.as_f64() {
        return n as i64 as u32;
    }
    let s = v.as_str().unwrap_or("").trim().to_ascii_lowercase();
    if let Some((_, c)) = NAMED.iter().find(|(n, _)| *n == s) {
        return *c;
    }
    if let Some(h) = s.strip_prefix('#') {
        let h: String = if h.len() == 3 || h.len() == 4 {
            h.chars().flat_map(|c| [c, c]).collect()
        } else {
            h.into()
        };
        let rgb = u32::from_str_radix(h.get(..6).unwrap_or(""), 16).unwrap_or(0);
        let a = if h.len() == 8 {
            u32::from_str_radix(&h[6..8], 16).unwrap_or(255)
        } else {
            255
        };
        return a << 24 | rgb;
    }
    let inner = s
        .strip_prefix("rgba(")
        .or_else(|| s.strip_prefix("rgb("))
        .and_then(|r| r.strip_suffix(')'));
    if let Some(inner) = inner {
        let p: Vec<f64> = inner
            .split([',', ' ', '/'])
            .filter(|t| !t.is_empty())
            .filter_map(|t| t.parse().ok())
            .collect();
        let c = |v: f64| v.round().clamp(0., 255.) as u32;
        let a = if p.len() > 3 { c(p[3] * 255.) } else { 255 };
        return a << 24
            | c(*p.first().unwrap_or(&0.)) << 16
            | c(*p.get(1).unwrap_or(&0.)) << 8
            | c(*p.get(2).unwrap_or(&0.));
    }
    0
}
/// Splits on whitespace outside parentheses: "0 8px 24px rgba(0,0,0,.4)".
fn tokens(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && ch.is_whitespace() {
            if !cur.trim().is_empty() {
                out.push(cur.trim().to_owned());
            }
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_owned());
    }
    out
}
fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.),
        Value::String(s) => leading_float(s),
        Value::Bool(b) => f64::from(u8::from(*b)),
        _ => 0.,
    }
}
fn leading_float(s: &str) -> f64 {
    let s = s.trim();
    let end = s
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+'))))
        .map_or(s.len(), |(i, _)| i);
    s[..end].parse().unwrap_or(0.)
}
fn numeric(t: &str) -> bool {
    t.starts_with(|c: char| c.is_ascii_digit() || c == '.' || c == '-')
}
/// Width/height-like values keep `auto` and percentages as strings.
fn dim(v: &Value) -> Prop {
    match v {
        Value::String(s) if s.trim() == "auto" || s.trim().ends_with('%') => {
            Prop::Str(s.trim().into())
        }
        _ => Prop::Num(num(v)),
    }
}
fn quad(v: &Value) -> [f64; 4] {
    if let Some(n) = v.as_f64() {
        return [n; 4];
    }
    let p: Vec<f64> = tokens(v.as_str().unwrap_or(""))
        .iter()
        .map(|t| leading_float(t))
        .collect();
    match p.len() {
        0 => [0.; 4],
        1 => [p[0]; 4],
        2 => [p[0], p[1], p[0], p[1]],
        3 => [p[0], p[1], p[2], p[1]],
        _ => [p[0], p[1], p[2], p[3]],
    }
}
fn gradient(v: &str) -> Option<Prop> {
    let inner = v
        .trim()
        .strip_prefix("linear-gradient(")?
        .strip_suffix(')')?;
    // Commas outside parentheses separate the angle and the stops.
    let (mut parts, mut cur, mut depth) = (Vec::new(), String::new(), 0);
    for ch in inner.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(cur.trim().to_owned());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    parts.push(cur.trim().to_owned());
    let angles = [
        ("to top", 0.),
        ("to right", 90.),
        ("to bottom", 180.),
        ("to left", 270.),
        ("to top right", 45.),
        ("to bottom right", 135.),
        ("to bottom left", 225.),
        ("to top left", 315.),
    ];
    let mut angle = 180.;
    if parts.first().is_some_and(|p| p.ends_with("deg")) {
        angle = leading_float(&parts.remove(0));
    } else if let Some((_, a)) = angles
        .iter()
        .find(|(n, _)| parts.first().map(String::as_str) == Some(n))
    {
        angle = *a;
        parts.remove(0);
    }
    let stop = |p: &str| {
        let t = tokens(p);
        f64::from(color(&Value::from(t.first().cloned().unwrap_or_default())))
    };
    Some(Prop::Gradient(
        angle,
        stop(parts.first()?),
        stop(parts.last()?),
    ))
}
const COLOR_KEYS: [&str; 10] = [
    "backgroundColor",
    "borderColor",
    "color",
    "shadowColor",
    "borderLeftColor",
    "borderTopColor",
    "borderRightColor",
    "borderBottomColor",
    "borderStartColor",
    "borderEndColor",
];
const DIM_KEYS: [&str; 13] = [
    "width",
    "height",
    "minWidth",
    "minHeight",
    "maxWidth",
    "maxHeight",
    "left",
    "top",
    "right",
    "bottom",
    "flexBasis",
    "start",
    "end",
];
fn alias(k: &str) -> &str {
    match k {
        "marginInlineStart" => "marginStart",
        "marginInlineEnd" => "marginEnd",
        "paddingInlineStart" => "paddingStart",
        "paddingInlineEnd" => "paddingEnd",
        "insetInlineStart" => "start",
        "insetInlineEnd" => "end",
        "borderInlineStartWidth" => "borderStartWidth",
        "borderInlineEndWidth" => "borderEndWidth",
        "borderInlineStartColor" => "borderStartColor",
        "borderInlineEndColor" => "borderEndColor",
        "lineClamp" | "WebkitLineClamp" => "numberOfLines",
        _ => k,
    }
}
fn put(out: &mut Props, k: &str, v: Prop) {
    out.insert(k.into(), v);
}
fn expand(k: &str, v: &Value, out: &mut Props) {
    let k = alias(k);
    match k {
        "margin" | "padding" => {
            let [t, r, b, l] = quad(v);
            for (side, n) in [("Top", t), ("Right", r), ("Bottom", b), ("Left", l)] {
                put(out, &format!("{k}{side}"), Prop::Num(n));
            }
        }
        "marginHorizontal" | "paddingHorizontal" => {
            let base = k.trim_end_matches("Horizontal");
            put(out, &format!("{base}Left"), Prop::Num(num(v)));
            put(out, &format!("{base}Right"), Prop::Num(num(v)));
        }
        "marginVertical" | "paddingVertical" => {
            let base = k.trim_end_matches("Vertical");
            put(out, &format!("{base}Top"), Prop::Num(num(v)));
            put(out, &format!("{base}Bottom"), Prop::Num(num(v)));
        }
        "inset" => {
            let [t, r, b, l] = quad(v);
            for (side, n) in [("top", t), ("right", r), ("bottom", b), ("left", l)] {
                put(out, side, Prop::Num(n));
            }
        }
        "flex" => {
            // React Native: flex > 0 grows and shrinks from a zero basis.
            let f = num(v);
            let (grow, shrink, basis) = if f > 0. {
                (f, 1., Prop::Num(0.))
            } else if f == 0. {
                (0., 0., Prop::Str("auto".into()))
            } else {
                (0., -f, Prop::Str("auto".into()))
            };
            put(out, "flexGrow", Prop::Num(grow));
            put(out, "flexShrink", Prop::Num(shrink));
            put(out, "flexBasis", basis);
        }
        "borderTop" | "borderRight" | "borderBottom" | "borderLeft" | "borderStart"
        | "borderEnd" => {
            let (w, c) = border(v);
            put(out, &format!("{k}Width"), Prop::Num(w));
            if let Some(c) = c {
                put(out, &format!("{k}Color"), Prop::Num(f64::from(c)));
            }
        }
        "border" => {
            let (w, c) = border(v);
            put(out, "borderWidth", Prop::Num(w));
            put(out, "borderColor", Prop::Num(f64::from(c.unwrap_or(0))));
        }
        "boxShadow" => {
            if v.as_str() == Some("none") {
                put(out, "shadowColor", Prop::Num(0.));
                return;
            }
            let t = tokens(v.as_str().unwrap_or(""));
            let nums: Vec<f64> = t
                .iter()
                .filter(|t| numeric(t))
                .map(|t| leading_float(t))
                .collect();
            let c = t.iter().find(|t| !numeric(t));
            put(
                out,
                "shadowOffsetX",
                Prop::Num(*nums.first().unwrap_or(&0.)),
            );
            put(out, "shadowOffsetY", Prop::Num(*nums.get(1).unwrap_or(&0.)));
            put(out, "shadowBlur", Prop::Num(*nums.get(2).unwrap_or(&0.)));
            let c = c.map_or(0x8000_0000, |c| color(&Value::from(c.as_str())));
            put(out, "shadowColor", Prop::Num(f64::from(c)));
        }
        "background" | "backgroundImage" => match v.as_str().and_then(gradient) {
            Some(g) => put(out, "gradient", g),
            None => put(out, "backgroundColor", Prop::Num(f64::from(color(v)))),
        },
        "fontWeight" => {
            let w = match v.as_str() {
                Some("normal") => 400.,
                Some("medium") => 500.,
                Some("semibold") => 600.,
                Some("bold") => 700.,
                _ if num(v) > 0. => num(v),
                _ => 400.,
            };
            put(out, "fontWeight", Prop::Num(w));
        }
        "fontFamily" => {
            let f = v.as_str().unwrap_or("").to_ascii_lowercase();
            let family = if ["mono", "courier", "consol"].iter().any(|m| f.contains(m)) {
                "monospace"
            } else if f.contains("brand") || f.contains("jakarta") {
                "brand"
            } else {
                "sans-serif"
            };
            put(out, "fontFamily", Prop::Str(family.into()));
        }
        "textOverflow" => put(
            out,
            k,
            Prop::Str(
                if v.as_str() == Some("ellipsis") {
                    "ellipsis"
                } else {
                    "clip"
                }
                .into(),
            ),
        ),
        _ if COLOR_KEYS.contains(&k) => put(out, k, Prop::Num(f64::from(color(v)))),
        _ if DIM_KEYS.contains(&k) => put(out, k, dim(v)),
        _ => match v {
            Value::Number(_) | Value::Bool(_) => put(out, k, Prop::Num(num(v))),
            Value::String(s) if numeric(s) && s.trim_end_matches("px").parse::<f64>().is_ok() => {
                put(out, k, Prop::Num(leading_float(s)))
            }
            Value::String(s) => put(out, k, Prop::Str(s.clone())),
            _ => {}
        },
    }
}
/// "1px solid #ccc" → (width, color); "none" → (0, transparent).
fn border(v: &Value) -> (f64, Option<u32>) {
    if let Some(n) = v.as_f64() {
        return (n, None);
    }
    let s = v.as_str().unwrap_or("");
    if s == "none" {
        return (0., Some(0));
    }
    let t = tokens(s);
    let w = t
        .iter()
        .find(|t| numeric(t))
        .map_or(1., |t| leading_float(t));
    let c = t
        .iter()
        .find(|t| {
            !numeric(t) && !["solid", "dashed", "dotted", "double", "none"].contains(&t.as_str())
        })
        .map(|c| color(&Value::from(c.as_str())));
    (w, c)
}
/// A style object (or array of them) → runtime properties.
pub fn normalize(style: &Value) -> Props {
    let mut out = Props::new();
    match style {
        Value::Array(list) => {
            for s in list {
                out.extend(normalize(s));
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                if !v.is_null() && v != &Value::Bool(false) {
                    expand(k, v, &mut out);
                }
            }
        }
        _ => {}
    }
    out
}
/// The properties to send: removed keys reset (`None`), changed ones set.
pub fn diff<'a>(prev: &'a Props, next: &'a Props) -> Vec<(&'a str, Option<&'a Prop>)> {
    let mut out: Vec<_> = prev
        .keys()
        .filter(|k| !next.contains_key(*k))
        .map(|k| (k.as_str(), None))
        .collect();
    out.extend(
        next.iter()
            .filter(|(k, v)| prev.get(*k) != Some(v))
            .map(|(k, v)| (k.as_str(), Some(v))),
    );
    out
}
