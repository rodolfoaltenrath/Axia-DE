//! The Overview's layouts (docs/DESKTOPS.md): the windows as miniatures in a grid, and the strip of
//! desktops along the top. Snap Assist (docs/WINDOWS.md) uses the same grid inside a free zone.
use super::{get, js_round, num, Rect};
use serde_json::{json, Value};

/// At most this many windows are laid out in one grid.
pub const GRID_MAX: usize = 512;

/// Margins of a grid, the space between miniatures and the title row above each one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridSpec {
    pub pad_x: f64,
    pub pad_top: f64,
    pub pad_bottom: f64,
    pub gap: f64,
    pub label: f64,
}

/// The Overview: under the strip of desktops, which leaves its own room at the top.
pub const OVERVIEW_GRID: GridSpec = GridSpec { pad_x: 48.0, pad_top: 0.0, pad_bottom: 48.0, gap: 28.0, label: 30.0 };
/// Snap Assist: inside one zone of the work area.
pub const ASSIST_GRID: GridSpec = GridSpec { pad_x: 24.0, pad_top: 24.0, pad_bottom: 24.0, gap: 20.0, label: 30.0 };

/// A miniature: its frame scaled by `scale` at (x, y), and its title row (ly, lh) above it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridPlace {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
    pub ly: f64,
    pub lh: f64,
}

impl GridPlace {
    pub fn to_value(self) -> Value {
        json!({
            "x": num(self.x), "y": num(self.y), "w": self.w, "h": self.h,
            "scale": self.scale, "ly": num(self.ly), "lh": num(self.lh),
        })
    }
}

/// `frames` (each window's drawn width and height: a maximized one takes the work area) in a grid in
/// `a`, the last row centered, never larger than 85 %.
pub fn grid(frames: &[(f64, f64)], a: Rect, spec: &GridSpec) -> Vec<GridPlace> {
    let n = frames.len().min(GRID_MAX);
    if n == 0 {
        return Vec::new();
    }
    let cols = ((n as f64 * (a.w / a.h.max(1.0))) / 1.6).sqrt().ceil();
    let c = if cols.is_finite() { (cols as usize).clamp(1, n) } else { 1 };
    let r = n.div_ceil(c);
    let cw = ((a.w - 2.0 * spec.pad_x - spec.gap * (c as f64 - 1.0)) / c as f64).max(1.0);
    let ch = ((a.h - spec.pad_top - spec.pad_bottom - spec.gap * (r as f64 - 1.0)) / r as f64).max(spec.label + 1.0);
    frames[..n]
        .iter()
        .enumerate()
        .map(|(i, &(fw, fh))| {
            let (col, row) = (i % c, i / c);
            let in_row = if row == r - 1 { n - row * c } else { c };
            let row_left = a.x + spec.pad_x + ((c - in_row) as f64 * (cw + spec.gap)) / 2.0;
            let (fw, fh) = (if fw > 0.0 { fw } else { 1.0 }, if fh > 0.0 { fh } else { 1.0 });
            let scale = (cw / fw).min((ch - spec.label) / fh).min(0.85).max(0.05);
            let (sw, sh) = (fw * scale, fh * scale);
            let y = js_round(a.y + spec.pad_top + row as f64 * (ch + spec.gap) + spec.label + (ch - spec.label - sh) / 2.0);
            GridPlace {
                x: js_round(row_left + col as f64 * (cw + spec.gap) + (cw - sw) / 2.0),
                y,
                w: sw,
                h: sh,
                scale,
                ly: y - spec.label,
                lh: spec.label - 4.0,
            }
        })
        .collect()
}

const STRIP_PAD: f64 = 48.0;

/// The strip of desktops, centered along the top of `a`: one card per desktop and the add button
/// (cards in the list's order; RTL mirrors them). opts: { aspect = 1.6, canAdd = true, rtl = false }.
/// Returns { cards: [{ id, x, y, w, h }], add, h }.
pub fn strip(list: &Value, a: Rect, opts: &Value) -> Value {
    let ids: &[Value] = list.as_array().map_or(&[], Vec::as_slice);
    let aspect = get(opts, "aspect").as_f64().filter(|v| *v > 0.0).unwrap_or(1.6);
    let can_add = get(opts, "canAdd").as_bool().unwrap_or(true);
    let rtl = get(opts, "rtl").as_bool().unwrap_or(false);
    let (gap, add_w, top) = (12.0, 44.0, 14.0);
    let n = ids.len() as f64;
    let add_room = if can_add { add_w + gap } else { 0.0 };
    let room = a.w - 2.0 * STRIP_PAD - add_room - gap * (n - 1.0).max(0.0);
    let w = js_round((room / n.max(1.0)).min(168.0).max(72.0));
    let h = js_round(w / aspect);
    let total = n * w + gap * (n - 1.0).max(0.0) + add_room;
    let left = js_round(a.x + (a.w - total) / 2.0);
    let at = |slot: f64, width: f64| if rtl { left + total - slot - width } else { left + slot };
    let cards: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| json!({ "id": id, "x": num(at(i as f64 * (w + gap), w)), "y": num(a.y + top), "w": num(w), "h": num(h) }))
        .collect();
    let add = if can_add {
        Rect::new(at(n * (w + gap), add_w), a.y + top, add_w, h).to_value()
    } else {
        Value::Null
    };
    json!({ "cards": cards, "add": add, "h": num(top + h + 30.0) })
}
