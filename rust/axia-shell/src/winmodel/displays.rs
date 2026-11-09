//! The arrangement math of Settings → Displays (docs/DISPLAYS.md, "Numbers, identify and snapping"):
//! numbering, the magnetic position while a display is dragged, and the drop that always touches
//! another display edge to edge and overlaps none (the rule axia-server checks too: out_fits in
//! outputs.c). Rects are { x, y, w, h } in the desktop's logical px; `threshold` in the same units.
use super::{get, js_round, num, truthy, Rect};
use serde_json::{Map, Value};

/// At most this many displays are arranged.
const MAX_RECTS: usize = 16;

pub fn rects(v: &Value) -> Result<Vec<Rect>, String> {
    let list = v.as_array().ok_or("display rects: not a list")?;
    if list.is_empty() || list.len() > MAX_RECTS {
        return Err(format!("display rects: 1 to {MAX_RECTS} displays"));
    }
    list.iter()
        .map(|r| {
            Rect::from_value(r)
                .filter(|r| [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w > 0.0 && r.h > 0.0)
                .ok_or_else(|| "display rects: each one is { x, y, w, h }".to_string())
        })
        .collect()
}

pub fn index(v: &Value, len: usize) -> Result<usize, String> {
    v.as_u64().map(|i| i as usize).filter(|&i| i < len).ok_or_else(|| "display index out of range".to_string())
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn shared(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    a1.min(b1) - a0.max(b0)
}

/// Two displays touch when an edge of one lies on an edge of the other along at least one pixel (a
/// corner alone is not enough: the pointer could not cross there).
pub fn touches(a: &Rect, b: &Rect) -> bool {
    ((a.x + a.w == b.x || b.x + b.w == a.x) && shared(a.y, a.y + a.h, b.y, b.y + b.h) > 0.0)
        || ((a.y + a.h == b.y || b.y + b.h == a.y) && shared(a.x, a.x + a.w, b.x, b.x + b.w) > 0.0)
}

/// rects[i] at (x, y): overlaps no other display and touches at least one (always fine when alone).
pub fn placement_ok(rects: &[Rect], i: usize, x: f64, y: f64) -> bool {
    let m = Rect::new(x, y, rects[i].w, rects[i].h);
    let mut touching = rects.len() < 2;
    for (j, o) in rects.iter().enumerate() {
        if j == i {
            continue;
        }
        if overlaps(&m, o) {
            return false;
        }
        touching |= touches(&m, o);
    }
    touching
}

/// Numbers like macOS's "Identify": the main display is 1, the others follow in the server's order.
/// Only active displays with a size get one. { [id]: n }.
pub fn numbers(outputs: &Value) -> Value {
    let act: Vec<&Value> = outputs
        .as_array()
        .map(|l| l.iter().filter(|o| truthy(o) && truthy(get(o, "active")) && truthy(get(o, "w"))).collect())
        .unwrap_or_default();
    let main = act.iter().position(|o| truthy(get(o, "main")));
    let order = main.into_iter().chain((0..act.len()).filter(|&k| Some(k) != main));
    let mut out = Map::new();
    for (k, i) in order.enumerate() {
        let id = match get(act[i], "id") {
            Value::String(s) => s.clone(),
            v => v.to_string(),
        };
        out.insert(id, num(k as f64 + 1.0));
    }
    Value::Object(out)
}

/// The targets one axis can snap to against another display: edge to edge (both sides), then starts
/// aligned, ends aligned and centers aligned.
fn targets(len: f64, op: f64, olen: f64) -> [f64; 5] {
    [op + olen, op - len, op, op + olen - len, js_round(op + (olen - len) / 2.0)]
}

/// While dragging: each axis independently jumps to the nearest target within `threshold`, taken from
/// the displays near on the other axis (no further than `threshold`), so a display far below another
/// doesn't stick to its side.
pub fn snap_display(rects: &[Rect], i: usize, x: f64, y: f64, threshold: f64) -> (f64, f64) {
    let (w, h) = (rects[i].w, rects[i].h);
    let (mut bx, mut by) = (js_round(x), js_round(y));
    let (mut dx, mut dy) = (threshold + 1.0, threshold + 1.0);
    for (j, o) in rects.iter().enumerate() {
        if j == i {
            continue;
        }
        let gap_y = (o.y - (y + h)).max(0.0).max(y - (o.y + o.h));
        let gap_x = (o.x - (x + w)).max(0.0).max(x - (o.x + o.w));
        if gap_y <= threshold {
            for t in targets(w, o.x, o.w) {
                let d = (x - t).abs();
                if d <= threshold && d < dx {
                    dx = d;
                    bx = t;
                }
            }
        }
        if gap_x <= threshold {
            for t in targets(h, o.y, o.h) {
                let d = (y - t).abs();
                if d <= threshold && d < dy {
                    dy = d;
                    by = t;
                }
            }
        }
    }
    (bx, by)
}

/// Along an edge of a display at `op` (length `olen`), the coordinate `v` (length `len`) is kept so
/// the edges share at least one pixel, and pulled to its start, end or center within `threshold`.
fn along_edge(v: f64, len: f64, op: f64, olen: f64, threshold: f64) -> [f64; 2] {
    let (lo, hi) = (op - len + 1.0, op + olen - 1.0);
    let c = if v < lo { lo } else if v > hi { hi } else { v };
    let (mut best, mut bd) = (c, threshold + 1.0);
    for t in [op, op + olen - len, js_round(op + (olen - len) / 2.0)] {
        let d = (c - t).abs();
        if d <= threshold && d < bd && t >= lo && t <= hi {
            bd = d;
            best = t;
        }
    }
    [best, c]
}

/// On drop: the snapped position if it is a valid one; otherwise the nearest position that touches a
/// display edge to edge without overlapping any (beside it, aligned when close, or slid along its
/// edge past a display in the way).
pub fn resolve_drop(rects: &[Rect], i: usize, x: f64, y: f64, threshold: f64) -> (f64, f64) {
    if rects.len() < 2 {
        return (js_round(x), js_round(y));
    }
    let s = snap_display(rects, i, x, y, threshold);
    if placement_ok(rects, i, s.0, s.1) {
        return s;
    }
    let (w, h) = (rects[i].w, rects[i].h);
    let mut best: Option<(f64, f64)> = None;
    let mut bd = f64::INFINITY;
    let mut consider = |cx: f64, cy: f64| {
        if !placement_ok(rects, i, cx, cy) {
            return;
        }
        let d = (cx - s.0).powi(2) + (cy - s.1).powi(2);
        if d < bd {
            bd = d;
            best = Some((cx, cy));
        }
    };
    for (j, o) in rects.iter().enumerate() {
        if j == i {
            continue;
        }
        // along an edge: where it was dropped, plus just past every other display (to slide around one in the way)
        let (mut ys, mut xs) = (vec![s.1], vec![s.0]);
        for (q, r) in rects.iter().enumerate() {
            if q == i || q == j {
                continue;
            }
            ys.extend([r.y + r.h, r.y - h]);
            xs.extend([r.x + r.w, r.x - w]);
        }
        for sx in [o.x + o.w, o.x - w] {
            for &v in &ys {
                for yy in along_edge(v, h, o.y, o.h, threshold) {
                    consider(sx, yy);
                }
            }
        }
        for sy in [o.y + o.h, o.y - h] {
            for &v in &xs {
                for xx in along_edge(v, w, o.x, o.w, threshold) {
                    consider(xx, sy);
                }
            }
        }
    }
    if let Some(b) = best {
        return b;
    }
    // nothing free next to anyone (can't happen with a few displays): right of the rightmost one
    let r = rects
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, r)| r)
        .fold(None::<&Rect>, |acc, r| match acc {
            Some(a) if a.x + a.w >= r.x + r.w => Some(a),
            _ => Some(r),
        })
        .expect("two or more displays");
    (r.x + r.w, r.y)
}
