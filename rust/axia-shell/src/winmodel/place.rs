//! Where an app's window goes when it opens again (docs/WINDOWS.md, "Remembered places"). A place is
//! kept relative to its display, and a display is known by what it is, not by its index: the name
//! from its EDID plus its connector.
use super::{get, js_round, num, number, truthy, Rect};
use serde_json::{json, Value};

pub const MAX_PLACES: usize = 200;

fn text(v: &Value) -> String {
    if !truthy(v) {
        return String::new();
    }
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

pub fn display_key(d: &Value) -> String {
    format!("{}|{}", text(get(d, "name")), text(get(d, "connector")))
}

/// The same monitor on the same port; then the same monitor on another; that port with another
/// monitor is NOT it. Two alike monitors on other ports can't be told apart.
pub fn find_display(key: &str, displays: &[Value]) -> Option<usize> {
    if key.is_empty() {
        return None;
    }
    if let Some(i) = displays.iter().position(|d| display_key(d) == key) {
        return Some(i);
    }
    let name = match key.rfind('|') {
        Some(i) => &key[..i],
        None => &key[..key.len() - key.chars().last().map_or(0, char::len_utf8)],
    };
    if name.is_empty() {
        return None;
    }
    let same: Vec<usize> = (0..displays.len())
        .filter(|&i| get(&displays[i], "name").as_str() == Some(name))
        .collect();
    (same.len() == 1).then(|| same[0])
}

/// What a window leaves: its normal frame (not the snapped or keyboard-lifted one), and whether it
/// was maximized. Null for picture-in-picture, fullscreen or a frame without a size.
pub fn place_of(w: &Value, display: &Value) -> Value {
    if truthy(get(w, "pip")) || truthy(get(w, "fullscreen")) {
        return Value::Null;
    }
    let osk = Some(get(w, "oskFrame")).filter(|v| truthy(v));
    let restore = Some(get(w, "restore")).filter(|v| truthy(get(w, "snapped")) && truthy(v));
    let f = osk.or(restore).unwrap_or(w);
    let (fw, fh) = (number(get(f, "w")), number(get(f, "h")));
    if !(fw > 0.0 && fh > 0.0) {
        return Value::Null;
    }
    let max = truthy(get(osk.unwrap_or(w), "maximized"));
    json!({
        "d": display_key(display),
        "x": num(js_round(number(get(f, "x")) - number(get(display, "x")))),
        "y": num(js_round(number(get(f, "y")) - number(get(display, "y")))),
        "w": num(js_round(fw)),
        "h": num(js_round(fh)),
        "max": max,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub d: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub max: bool,
}

/// A saved value is settings data: anything odd is no place at all.
pub fn clean_place(p: &Value) -> Option<Place> {
    let o = p.as_object()?;
    let d = o.get("d")?.as_str()?;
    if d.encode_utf16().count() > 300 {
        return None;
    }
    let int = |k: &str| o.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).map(js_round);
    let size = |k: &str| int(k).filter(|v| (40.0..=32768.0).contains(v));
    Some(Place {
        d: d.to_owned(),
        x: int("x")?,
        y: int("y")?,
        w: size("w")?,
        h: size("h")?,
        max: o.get("max") == Some(&Value::Bool(true)),
    })
}

/// A frame on a display that exists, fully usable: the title bar reachable and at least part of the
/// window in the work area. `areas[i]` is the work area of `displays[i]`. opts: { main, assigned }
/// indices into `displays`; fixed: the app's size can't change (`size`), so only the position gives
/// way; shift: px along for windows of this app already open.
pub fn restore_frame(p: &Value, displays: &Value, areas: &Value, opts: &Value) -> Value {
    let (Some(c), Some(ds), Some(areas)) = (clean_place(p), displays.as_array(), areas.as_array()) else {
        return Value::Null;
    };
    if ds.is_empty() || areas.len() != ds.len() {
        return Value::Null;
    }
    let idx = |k: &str| get(opts, k).as_u64().map(|i| i as usize).filter(|&i| i < ds.len());
    let known = find_display(&c.d, ds);
    let d = idx("assigned").or(known).or(idx("main")).unwrap_or(0);
    let Some(a) = Rect::from_value(&areas[d]) else { return Value::Null };
    let fixed = truthy(get(opts, "fixed"));
    let size = get(opts, "size").as_array().and_then(|s| Some((s.first()?.as_f64()?, s.get(1)?.as_f64()?)));
    let (w, h) = match size {
        Some(s) if fixed => s,
        _ => (c.w.min(a.w), c.h.min(a.h)),
    };
    let (wx, wy) = if known == Some(d) {
        (number(get(&ds[d], "x")) + c.x, number(get(&ds[d], "y")) + c.y)
    } else {
        (a.x + js_round((a.w - w) / 2.0), a.y + js_round((a.h - h) / 2.0))
    };
    let shift = get(opts, "shift").as_f64().unwrap_or(0.0);
    let (x, y) = (wx + shift, wy + shift);
    json!({
        "x": num(js_round(a.x.max(x.min(a.x + (a.w - w).max(0.0))))),
        "y": num(js_round(a.y.max(y.min(a.y + (a.h - h).max(0.0))))),
        "w": num(w),
        "h": num(h),
        "maximized": c.max && !fixed,
    })
}

/// `all` as [key, place] entries, oldest first: `key` goes to the end with `p`, and at most
/// MAX_PLACES stay (an app not opened in a long time gives its place up).
pub fn put_place(all: &Value, key: &Value, p: &Value) -> Value {
    let key = key.as_str().unwrap_or_default();
    let mut out: Vec<Value> = all
        .as_array()
        .map(|l| {
            l.iter()
                .filter(|e| e.get(0).and_then(Value::as_str).is_some_and(|k| k != key))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    out.push(json!([key, p]));
    let extra = out.len().saturating_sub(MAX_PLACES);
    out.drain(..extra);
    Value::Array(out)
}
