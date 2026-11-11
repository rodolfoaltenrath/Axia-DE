//! CSS/React-style styles → axia-server's native style properties (formerly runtime/style.js).
//!
//! Colours become ARGB numbers; shorthands (margin, padding, inset, border, boxShadow, background,
//! transform, flex…) become the engine's simple properties (server/src/ui/style.c). The renderer
//! (runtime/index.js) sends a node's flattened style as `[key, value]` pairs, in order, because a later
//! key wins over an earlier one ({ borderColor, border } and { border, borderColor } differ); it gets back
//! the native properties and only sends the server the ones that changed.
//!
//! The rules follow the JS version exactly, including its number parsing (a leading number, like
//! parseFloat) and rounding (half up, like Math.round), so styles render the same as before.
use serde_json::{json, Map, Value};
use std::ffi::{c_char, CStr, CString};

/// parseFloat: the longest leading decimal number, or None.
fn parse_float(s: &str) -> Option<f64> {
    let s = s.trim_start();
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    if s[i..].starts_with("Infinity") {
        let v = f64::INFINITY;
        return Some(if b[0] == b'-' { -v } else { v });
    }
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start
    };
    let mut n = digits(&mut i);
    if i < b.len() && b[i] == b'.' {
        i += 1;
        n += digits(&mut i);
    }
    if n == 0 {
        return None;
    }
    let mut end = i;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if digits(&mut j) > 0 {
            end = j;
        }
    }
    s[..end].parse().ok()
}

/// String(v) for the values styles carry.
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        _ => String::new(),
    }
}

/// num(): a number as is, else parseFloat or 0.
fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.),
        _ => parse_float(&text(v)).filter(|n| !n.is_nan()).unwrap_or(0.),
    }
}

/// dim(): a number, 'auto', a percentage (as text), or parseFloat or 0.
fn dim(v: &Value) -> Value {
    if v.is_number() {
        return v.clone();
    }
    let s = text(v);
    let s = s.trim();
    if s == "auto" || s.ends_with('%') {
        return json!(s);
    }
    json!(parse_float(s).filter(|n| !n.is_nan()).unwrap_or(0.))
}

fn round(n: f64) -> f64 {
    (n + 0.5).floor()
}
fn clamp255(n: f64) -> u32 {
    if n.is_nan() {
        0
    } else {
        round(n).clamp(0., 255.) as u32
    }
}

const NAMED: [(&str, u32); 11] = [
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
];

/// parseInt(s, 16): the leading hex digits.
fn parse_hex(s: &str) -> Option<u32> {
    let end = s.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(s.len());
    u32::from_str_radix(&s[..end], 16).ok()
}

/// A colour as 0xAARRGGBB; 0 for anything it can't read.
pub fn color(v: &Value) -> u32 {
    if let Value::Number(n) = v {
        return n.as_f64().map_or(0, |f| f as i64 as u32);
    }
    let s = text(v).trim().to_lowercase();
    if let Some((_, c)) = NAMED.iter().find(|(name, _)| *name == s) {
        return *c;
    }
    if let Some(h) = s.strip_prefix('#') {
        let h: String = if h.len() == 3 || h.len() == 4 { h.chars().flat_map(|c| [c, c]).collect() } else { h.into() };
        let Some(rgb) = parse_hex(&h[..h.len().min(6)]) else { return 0 };
        let a = if h.len() == 8 {
            let Some(a) = parse_hex(&h[6..8]) else { return 0 };   // NaN in the JS: no colour
            a
        } else {
            255
        };
        return (a << 24).wrapping_add(rgb);
    }
    let body = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')'));
    if let Some(body) = body.filter(|b| !b.contains(')')) {
        let p: Vec<f64> = body
            .split(|c: char| c.is_whitespace() || c == ',' || c == '/')
            .filter(|t| !t.is_empty())
            .map(|t| parse_float(t).unwrap_or(f64::NAN))
            .collect();
        if p.len() < 3 || p.iter().take(4).any(|n| n.is_nan()) {
            return 0;                                         // NaN in the JS: no colour
        }
        let a = if p.len() > 3 { clamp255(p[3] * 255.) } else { 255 };
        return (a << 24) + (clamp255(p[0]) << 16) + (clamp255(p[1]) << 8) + clamp255(p[2]);
    }
    0
}

/// Splits on whitespace (or commas) outside parentheses: "0 8px 24px rgba(0,0,0,.4)".
fn tokens(s: &str, commas: bool) -> Vec<String> {
    let (mut out, mut depth, mut cur) = (Vec::new(), 0i32, String::new());
    for ch in s.chars() {
        if ch == '(' {
            depth += 1;
        }
        if ch == ')' {
            depth -= 1;
        }
        let sep = if commas { ch == ',' } else { ch.is_whitespace() };
        if depth == 0 && sep {
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

/// "a b c d" → [top, right, bottom, left], like CSS; null where the value has no part.
fn four(v: &Value) -> [Value; 4] {
    if v.is_number() {
        return [v.clone(), v.clone(), v.clone(), v.clone()];
    }
    let p: Vec<Value> = tokens(&text(v), false).iter().map(|t| json!(num(&json!(t)))).collect();
    let get = |i: usize| p.get(i).cloned().unwrap_or(Value::Null);
    match p.len() {
        1 => [get(0), get(0), get(0), get(0)],
        2 => [get(0), get(1), get(0), get(1)],
        3 => [get(0), get(1), get(2), get(1)],
        _ => [get(0), get(1), get(2), get(3)],
    }
}

fn starts_numeric(t: &str) -> bool {
    let t = t.strip_prefix('-').unwrap_or(t);
    t.starts_with(|c: char| c.is_ascii_digit() || c == '.')
}

fn gradient(v: &Value) -> Option<Value> {
    let s = text(v);
    let inner = s.strip_prefix("linear-gradient(")?.strip_suffix(')')?;
    let mut parts = tokens(inner, true);
    let mut angle = 180.;
    let named = [
        ("to top", 0.),
        ("to right", 90.),
        ("to bottom", 180.),
        ("to left", 270.),
        ("to top right", 45.),
        ("to bottom right", 135.),
        ("to bottom left", 225.),
        ("to top left", 315.),
    ];
    if parts.first().is_some_and(|p| p.ends_with("deg")) {
        angle = parse_float(&parts.remove(0)).unwrap_or(f64::NAN);
    } else if let Some((_, a)) = parts.first().and_then(|p| named.iter().find(|(n, _)| n == p)) {
        angle = *a;
        parts.remove(0);
    }
    let stop = |p: Option<&String>| p.and_then(|p| tokens(p, false).into_iter().next()).map_or(0, |t| color(&json!(t)));
    Some(json!([angle, stop(parts.first()), stop(parts.last())]))
}

/// "1px solid #ccc" → (width, colour); "none" → (0, transparent); a number → (it, no colour).
fn border_parts(v: &Value) -> (f64, Option<u32>) {
    if let Value::Number(n) = v {
        return (n.as_f64().unwrap_or(0.), None);            // checked before "none", as in the JS
    }
    let s = text(v);
    if s == "none" {
        return (0., Some(0));
    }
    let p = tokens(&s, false);
    let w = p.iter().find(|t| starts_numeric(t));
    let c = p.iter().find(|t| !starts_numeric(t) && !matches!(t.as_str(), "solid" | "dashed" | "dotted" | "double" | "none"));
    (w.map_or(1., |w| num(&json!(w))), c.map(|c| color(&json!(c))))
}

fn angle(v: &Value) -> f64 {
    if let Value::Number(n) = v {
        return n.as_f64().unwrap_or(0.);
    }
    let s = text(v);
    let s = s.trim();
    let n = parse_float(s).filter(|n| !n.is_nan()).unwrap_or(0.);
    if s.ends_with("rad") {
        n * 180. / std::f64::consts::PI
    } else if s.ends_with("turn") {
        n * 360.
    } else {
        n
    }
}

fn origin(v: &Value, out: &mut Map<String, Value>) {
    let p: Vec<Value> = match v {
        Value::Array(a) => a.clone(),
        _ => tokens(&text(v), false).into_iter().map(Value::String).collect(),
    };
    let mut x = p.first().cloned().unwrap_or(json!("50%"));
    let mut y = p.get(1).cloned().unwrap_or(json!("50%"));
    let is = |v: &Value, words: &[&str]| v.as_str().is_some_and(|s| words.contains(&s));
    if p.len() == 1 && is(&x, &["top", "bottom"]) {
        y = x;
        x = json!("50%");
    }
    if is(&x, &["top", "bottom"]) || is(&y, &["left", "right"]) {
        std::mem::swap(&mut x, &mut y);
    }
    let word = |v: Value| match v.as_str() {
        Some("left") | Some("top") => json!("0%"),
        Some("center") => json!("50%"),
        Some("right") | Some("bottom") => json!("100%"),
        _ => v,
    };
    out.insert("transformOriginX".into(), dim(&word(x)));
    out.insert("transformOriginY".into(), dim(&word(y)));
}

fn set(out: &mut Map<String, Value>, k: &str, v: Value) {
    out.insert(k.to_owned(), v);
}

const SIDES: [&str; 6] = ["Left", "Top", "Right", "Bottom", "Start", "End"];
const DIM_KEYS: [&str; 13] =
    ["width", "height", "minWidth", "minHeight", "maxWidth", "maxHeight", "left", "top", "right", "bottom", "flexBasis", "start", "end"];
const ALIASES: [(&str, &str); 15] = [
    ("marginInlineStart", "marginStart"),
    ("marginInlineEnd", "marginEnd"),
    ("paddingInlineStart", "paddingStart"),
    ("paddingInlineEnd", "paddingEnd"),
    ("insetInlineStart", "start"),
    ("insetInlineEnd", "end"),
    ("borderInlineStartWidth", "borderStartWidth"),
    ("borderInlineEndWidth", "borderEndWidth"),
    ("borderInlineStartColor", "borderStartColor"),
    ("borderInlineEndColor", "borderEndColor"),
    ("borderStartStartRadius", "borderTopStartRadius"),
    ("borderStartEndRadius", "borderTopEndRadius"),
    ("borderEndStartRadius", "borderBottomStartRadius"),
    ("borderEndEndRadius", "borderBottomEndRadius"),
    ("lineClamp", "numberOfLines"),
];

fn is_color_key(k: &str) -> bool {
    matches!(k, "backgroundColor" | "borderColor" | "color" | "shadowColor")
        || SIDES.iter().any(|s| k.strip_prefix("border").and_then(|r| r.strip_suffix("Color")) == Some(*s))
}

fn transform(v: &Value, out: &mut Map<String, Value>) {
    if let Value::Array(list) = v {
        for t in list {
            let has = |k: &str| t.get(k).is_some();
            if has("translateX") {
                set(out, "translateX", json!(num(&t["translateX"])));
            }
            if has("translateY") {
                set(out, "translateY", json!(num(&t["translateY"])));
            }
            if has("scale") {
                let s = json!(num(&t["scale"]));
                set(out, "scaleX", s.clone());
                set(out, "scaleY", s);
            }
            if has("scaleX") {
                set(out, "scaleX", json!(num(&t["scaleX"])));
            }
            if has("scaleY") {
                set(out, "scaleY", json!(num(&t["scaleY"])));
            }
            if has("rotate") {
                set(out, "rotate", json!(angle(&t["rotate"])));
            }
            if has("rotateZ") {
                set(out, "rotate", json!(angle(&t["rotateZ"])));
            }
        }
        return;
    }
    let Value::String(s) = v else { return };
    if s == "none" {
        return;
    }
    // (\w+)\(([^)]*)\)
    let mut rest = s.as_str();
    while let Some(open) = rest.find('(') {
        let name_start = rest[..open].rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
        let name = &rest[name_start..open];
        let Some(close) = rest[open..].find(')') else { break };
        let args: Vec<Value> =
            rest[open + 1..open + close].split(|c: char| c.is_whitespace() || c == ',').filter(|a| !a.is_empty()).map(|a| json!(a)).collect();
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        if !name.is_empty() {
            match name {
                "translate" => {
                    set(out, "translateX", json!(num(&a(0))));
                    set(out, "translateY", json!(if args.len() > 1 { num(&a(1)) } else { 0. }));
                }
                "translateX" => set(out, "translateX", json!(num(&a(0)))),
                "translateY" => set(out, "translateY", json!(num(&a(0)))),
                "scale" => {
                    set(out, "scaleX", json!(num(&a(0))));
                    set(out, "scaleY", json!(num(&if args.len() > 1 { a(1) } else { a(0) })));
                }
                "scaleX" => set(out, "scaleX", json!(num(&a(0)))),
                "scaleY" => set(out, "scaleY", json!(num(&a(0)))),
                "rotate" | "rotateZ" => set(out, "rotate", json!(angle(&a(0)))),
                _ => {}
            }
        }
        rest = &rest[open + close + 1..];
    }
}

fn expand(k: &str, v: &Value, out: &mut Map<String, Value>) {
    let k = ALIASES.iter().find(|(from, _)| *from == k).map_or(k, |(_, to)| to);
    let k = if k == "WebkitLineClamp" { "numberOfLines" } else { k };
    match k {
        "margin" | "padding" | "inset" => {
            let [t, r, b, l] = four(v);
            let base = if k == "inset" { "" } else { k };
            let name = |side: &str| if base.is_empty() { side.to_lowercase() } else { format!("{base}{side}") };
            set(out, &name("Top"), t);
            set(out, &name("Right"), r);
            set(out, &name("Bottom"), b);
            set(out, &name("Left"), l);
        }
        "marginHorizontal" | "paddingHorizontal" => {
            let base = k.trim_end_matches("Horizontal");
            set(out, &format!("{base}Left"), json!(num(v)));
            set(out, &format!("{base}Right"), json!(num(v)));
        }
        "marginVertical" | "paddingVertical" => {
            let base = k.trim_end_matches("Vertical");
            set(out, &format!("{base}Top"), json!(num(v)));
            set(out, &format!("{base}Bottom"), json!(num(v)));
        }
        "marginInline" | "paddingInline" => {
            let base = k.trim_end_matches("Inline");
            let p: Vec<f64> = tokens(&text(v), false).iter().map(|t| num(&json!(t))).collect();
            let first = p.first().copied().unwrap_or(0.);
            set(out, &format!("{base}Start"), json!(first));
            set(out, &format!("{base}End"), json!(if p.len() > 1 { p[1] } else { first }));
        }
        "insetInline" => {
            let p: Vec<Value> = tokens(&text(v), false).iter().map(|t| dim(&json!(t))).collect();
            let first = p.first().cloned().unwrap_or(Value::Null);
            set(out, "start", first.clone());
            set(out, "end", if p.len() > 1 { p[1].clone() } else { first });
        }
        "flex" => {
            // React Native: flex > 0 grows and shrinks from a zero basis; flex: 0 is sized by its
            // width/height and inflexible; flex < 0 only shrinks (down to minWidth/minHeight)
            let f = num(v);
            let (grow, shrink, basis) = if f > 0. {
                (f, 1., json!(0))
            } else if f == 0. {
                (0., 0., json!("auto"))
            } else {
                (0., -f, json!("auto"))
            };
            set(out, "flexGrow", json!(grow));
            set(out, "flexShrink", json!(shrink));
            set(out, "flexBasis", basis);
        }
        "borderTop" | "borderRight" | "borderBottom" | "borderLeft" | "borderStart" | "borderEnd" | "borderInlineStart" | "borderInlineEnd" => {
            let side = k.replace("Inline", "");
            let (w, c) = border_parts(v);
            set(out, &format!("{side}Width"), json!(w));
            if let Some(c) = c {
                set(out, &format!("{side}Color"), json!(c));
            }
        }
        "textOverflow" => set(out, "textOverflow", json!(if v == "ellipsis" { "ellipsis" } else { "clip" })),
        "transformOrigin" => origin(v, out),
        "border" => {
            let p = tokens(&text(v), false);
            let first = p.first().map_or(Value::Null, |t| json!(t));
            let last = p.last().map_or(Value::Null, |t| json!(t));
            set(out, "borderWidth", json!(num(&first)));
            set(out, "borderColor", json!(color(&last)));
        }
        "boxShadow" => {
            if v == "none" {
                set(out, "shadowColor", json!(0));
                return;
            }
            let p = tokens(&text(v), false);
            let nums: Vec<f64> = p.iter().filter(|t| starts_numeric(t)).map(|t| num(&json!(t))).collect();
            let c = p.iter().find(|t| !starts_numeric(t));
            set(out, "shadowOffsetX", json!(nums.first().copied().unwrap_or(0.)));
            set(out, "shadowOffsetY", json!(nums.get(1).copied().unwrap_or(0.)));
            set(out, "shadowBlur", json!(nums.get(2).copied().unwrap_or(0.)));
            set(out, "shadowColor", json!(c.map_or(0x8000_0000, |c| color(&json!(c)))));
        }
        "backdropFilter" => {
            let s = text(v);
            let blur = s.find("blur(").and_then(|i| {
                let r = s[i + 5..].trim_start();
                let end = r.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(r.len());
                (end > 0).then(|| parse_float(&r[..end]).unwrap_or(0.))
            });
            set(out, "backdropBlur", json!(blur.unwrap_or(0.)));
        }
        "background" | "backgroundImage" => match gradient(v) {
            Some(g) => set(out, "gradient", g),
            None => set(out, "backgroundColor", json!(color(v))),
        },
        "transform" => transform(v, out),
        "fontWeight" => {
            let w = match v {
                Value::Number(_) => v.clone(),
                _ => {
                    let s = text(v);
                    let named = match s.as_str() {
                        "normal" => Some(400.),
                        "medium" => Some(500.),
                        "semibold" => Some(600.),
                        "bold" => Some(700.),
                        _ => None,
                    };
                    json!(named.or_else(|| parse_float(&s).filter(|n| *n != 0. && !n.is_nan())).unwrap_or(400.))
                }
            };
            set(out, "fontWeight", w);
        }
        "fontFamily" => {
            // three families: Inter ('sans-serif'), JetBrains Mono ('monospace', or a name containing "mono")
            // and the brand face of the wordmark ('brand': Plus Jakarta Sans)
            let s = text(v).to_lowercase();
            let family = if ["mono", "courier", "consol"].iter().any(|w| s.contains(w)) {
                "monospace"
            } else if ["brand", "jakarta"].iter().any(|w| s.contains(w)) {
                "brand"
            } else {
                "sans-serif"
            };
            set(out, "fontFamily", json!(family));
        }
        _ => {
            let value = if is_color_key(k) {
                json!(color(v))
            } else if DIM_KEYS.contains(&k) {
                dim(v)
            } else if let Value::String(s) = v {
                let digits = s.strip_suffix("px").unwrap_or(s);
                let digits = digits.strip_prefix('-').unwrap_or(digits);
                if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    json!(parse_float(s).unwrap_or(f64::NAN))
                } else {
                    v.clone()
                }
            } else {
                v.clone()
            };
            set(out, k, value);
        }
    }
}

/// `[[key, value], …]` in the style's order → the native properties. Null and false values are
/// skipped, as the renderer always did.
pub fn normalize(pairs: &[Value]) -> Map<String, Value> {
    let mut out = Map::new();
    for pair in pairs {
        let (Some(k), Some(v)) = (pair.get(0).and_then(Value::as_str), pair.get(1)) else { continue };
        if v.is_null() || v == &Value::Bool(false) {
            continue;
        }
        expand(k, v, &mut out);
    }
    out
}

/// A style object's entries in the order they appear in its JSON (serde_json's own Map would sort
/// them, and a later key must win over an earlier one).
struct Ordered(Vec<Value>);
impl<'de> serde::Deserialize<'de> for Ordered {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a style object or [key, value] pairs")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Value>()? {
                    out.push(Value::Array(vec![Value::String(k), v]));
                }
                Ok(Ordered(out))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Ordered, A::Error> {
                let mut out = Vec::new();
                while let Some(v) = seq.next_element::<Value>()? {
                    out.push(v);
                }
                Ok(Ordered(out))
            }
        }
        d.deserialize_any(V)
    }
}

/// lord.css(json): a style object (its key order kept) or `[[key, value], …]` → `{ property: value }`
/// (server/src/ui/js.c).
#[no_mangle]
pub unsafe extern "C" fn axia_css_normalize(input: *const c_char) -> *mut c_char {
    let pairs = if input.is_null() { None } else { serde_json::from_slice::<Ordered>(CStr::from_ptr(input).to_bytes()).ok() };
    let reply = Value::Object(pairs.map_or_else(Map::new, |p| normalize(&p.0)));
    CString::new(reply.to_string()).unwrap_or_default().into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn axia_css_free(reply: *mut c_char) {
    if !reply.is_null() {
        drop(CString::from_raw(reply));
    }
}

#[cfg(test)]
mod tests;
