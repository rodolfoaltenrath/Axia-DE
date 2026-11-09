//! The window manager's pure rules (docs/WINDOWS.md, docs/DESKTOPS.md, docs/DISPLAYS.md): where an
//! app's window reopens, the list of desktops, the Overview's and Snap Assist's layouts, and the
//! arrangement math of Settings → Displays. The session's window list stays in sdk/windows.js; these
//! rules take its facts as JSON (`lord.winmodel(op, args)`, server/src/ui/js_winmodel.c) and give
//! JSON back. The per-window visibility predicates (sdk/desktopmodel.js) stay with the list.
mod desktops;
mod displays;
mod ffi;
mod layout;
mod place;
#[cfg(test)]
mod tests;

pub use layout::{grid, ASSIST_GRID, OVERVIEW_GRID};

use serde_json::Value;

/// A rectangle in screen (or logical) pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    /// `{ x, y, w, h }` with four numbers.
    pub fn from_value(v: &Value) -> Option<Rect> {
        let f = |k: &str| v.get(k).and_then(Value::as_f64);
        Some(Rect::new(f("x")?, f("y")?, f("w")?, f("h")?))
    }
    pub fn to_value(self) -> Value {
        let mut o = serde_json::Map::new();
        o.insert("x".into(), num(self.x));
        o.insert("y".into(), num(self.y));
        o.insert("w".into(), num(self.w));
        o.insert("h".into(), num(self.h));
        Value::Object(o)
    }
}

static NULL: Value = Value::Null;

/// A property, or null (JavaScript's undefined).
pub(crate) fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&NULL)
}

/// JavaScript truthiness.
pub(crate) fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// A number, or NaN (what arithmetic on anything else gives).
pub(crate) fn number(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

/// JavaScript's Math.round: halves go up.
pub(crate) fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// A number as JSON: whole numbers as integers, NaN and infinities as null (as JSON.stringify does).
pub(crate) fn num(v: f64) -> Value {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 9_007_199_254_740_992.0 {
        Value::from(v as i64)
    } else {
        serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
    }
}

/// JavaScript's === between JSON values (numbers by value).
pub(crate) fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Runs `op` on `args` (a JSON array): the result, or why it can't run.
pub fn call(op: &str, args: &[Value]) -> Result<Value, String> {
    let a = |i: usize| args.get(i).unwrap_or(&NULL);
    let rect = |i: usize| Rect::from_value(a(i)).ok_or_else(|| format!("{op}: argument {i} is not a rectangle"));
    match op {
        // where an app's window reopens (sdk/windowmemory.js)
        "place.of" => Ok(place::place_of(a(0), a(1))),
        "place.restore" => Ok(place::restore_frame(a(0), a(1), a(2), a(3))),
        "place.put" => Ok(place::put_place(a(0), a(1), a(2))),
        // desktops (sdk/desktops.js, sdk/windows.js)
        "desk.add" => Ok(desktops::add(a(0))),
        "desk.remove" => Ok(desktops::remove(a(0), a(1))),
        "desk.neighbor" => Ok(desktops::neighbor(a(0), a(1), number(a(2)))),
        "desk.arrow" => Ok(num(desktops::arrow_step(a(0), truthy(a(1))))),
        "desk.swipe" => Ok(num(-desktops::arrow_step(a(0), truthy(a(1))))),
        "desk.count" => Ok(num(desktops::clean_count(a(0)))),
        "desk.index" => Ok(num(desktops::clean_index(a(0)))),
        // the Overview's and Snap Assist's layouts (sdk/overview.jsx)
        "layout.grid" => {
            let spec = match a(2).as_str() {
                Some("assist") => ASSIST_GRID,
                _ => OVERVIEW_GRID,
            };
            let frames: Vec<(f64, f64)> = a(0)
                .as_array()
                .map(|l| l.iter().take(layout::GRID_MAX).map(|f| (number(get(f, "w")), number(get(f, "h")))).collect())
                .unwrap_or_default();
            Ok(Value::Array(grid(&frames, rect(1)?, &spec).into_iter().map(|p| p.to_value()).collect()))
        }
        "layout.strip" => Ok(layout::strip(a(0), rect(1)?, a(2))),
        // Settings → Displays' arrangement (sdk/displaysui.jsx, apps/settings/Displays.jsx)
        "display.numbers" => Ok(displays::numbers(a(0))),
        "display.ok" => {
            let rs = displays::rects(a(0))?;
            let i = displays::index(a(1), rs.len())?;
            Ok(Value::Bool(displays::placement_ok(&rs, i, number(a(2)), number(a(3)))))
        }
        "display.snap" | "display.drop" => {
            let rs = displays::rects(a(0))?;
            let i = displays::index(a(1), rs.len())?;
            let (x, y, t) = (number(a(2)), number(a(3)), number(a(4)));
            if ![x, y, t].iter().all(|v| v.is_finite()) {
                return Err(format!("{op}: x, y and the threshold must be numbers"));
            }
            let (px, py) = if op == "display.snap" {
                displays::snap_display(&rs, i, x, y, t)
            } else {
                displays::resolve_drop(&rs, i, x, y, t)
            };
            Ok(serde_json::json!({ "x": num(px), "y": num(py) }))
        }
        _ => Err(format!("unknown winmodel op '{op}'")),
    }
}
