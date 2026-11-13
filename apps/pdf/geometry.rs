//! Coordinates use page points and view pixels, never integer truncation.
use super::navigation::Layout;
use serde_json::{json, Value};

fn number(v: &Value) -> Result<f64, &'static str> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or("pdf.invalidGeometry")
}
fn pair(v: &Value) -> Result<[f64; 2], &'static str> {
    Ok([number(&v[0])?, number(&v[1])?])
}
fn point_json(point: [f64; 2]) -> Result<Value, &'static str> {
    if point.iter().all(|n| n.is_finite()) {
        Ok(json!(point))
    } else {
        Err("pdf.invalidGeometry")
    }
}
pub(super) fn shown([w, h]: [f64; 2], rot: i64) -> [f64; 2] {
    if rot % 2 != 0 {
        [h, w]
    } else {
        [w, h]
    }
}
pub(super) fn to_view([x, y]: [f64; 2], [w, h]: [f64; 2], rot: i64, s: f64) -> [f64; 2] {
    match rot {
        1 => [(h - y) * s, x * s],
        2 => [(w - x) * s, (h - y) * s],
        3 => [y * s, (w - x) * s],
        _ => [x * s, y * s],
    }
}
pub(super) fn to_page([x, y]: [f64; 2], [w, h]: [f64; 2], rot: i64, s: f64) -> [f64; 2] {
    let (a, b) = (x / s, y / s);
    match rot {
        1 => [b, h - a],
        2 => [w - a, h - b],
        3 => [w - b, a],
        _ => [a, b],
    }
}
fn tops(v: &Value) -> Result<Vec<f64>, &'static str> {
    let values = v.as_array().ok_or("pdf.invalidGeometry")?;
    if values.len() > 100_000 {
        return Err("pdf.invalidGeometry");
    }
    let out = values.iter().map(number).collect::<Result<Vec<_>, _>>()?;
    if out.windows(2).any(|p| p[0] > p[1]) {
        return Err("pdf.invalidGeometry");
    }
    Ok(out)
}
pub(super) fn visible(tops: &[f64], y0: f64, y1: f64) -> Value {
    if tops.is_empty() {
        return json!([0, -1]);
    }
    let (mut lo, mut hi) = (0, tops.len() - 1);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if tops[mid] <= y0 {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut last = lo;
    while last + 1 < tops.len() && tops[last + 1] < y1 {
        last += 1;
    }
    json!([lo, last])
}

pub(super) fn request(input: &Value, layout: &mut Layout) -> Result<Value, &'static str> {
    let op = input["cmd"].as_str().unwrap_or("");
    if op == "visible" {
        let supplied;
        let positions = if input.get("tops").is_some() {
            supplied = tops(&input["tops"])?;
            &supplied
        } else {
            &layout.tops
        };
        return Ok(visible(
            positions,
            number(&input["y0"])?,
            number(&input["y1"])?,
        ));
    }
    let rot = input["rot"].as_i64().ok_or("pdf.invalidGeometry")?;
    if op == "shown" {
        return Ok(json!(shown(pair(&input["size"])?, rot)));
    }
    if op == "shownMany" {
        let sizes = input["sizes"]
            .as_array()
            .filter(|a| a.len() <= 100_000)
            .ok_or("pdf.invalidGeometry")?;
        return Ok(json!(sizes
            .iter()
            .map(|size| pair(size).map(|size| shown(size, rot)))
            .collect::<Result<Vec<_>, _>>()?));
    }
    let scale = number(&input["scale"])?;
    if scale <= 0. {
        return Err("pdf.invalidGeometry");
    }
    match op {
        "toView" => point_json(to_view(
            pair(&input["point"])?,
            pair(&input["size"])?,
            rot,
            scale,
        )),
        "toPage" => point_json(to_page(
            pair(&input["point"])?,
            pair(&input["size"])?,
            rot,
            scale,
        )),
        "rect" => {
            let rect = &input["rect"];
            let (x, y, w, h) = (
                number(&rect[0])?,
                number(&rect[1])?,
                number(&rect[2])?,
                number(&rect[3])?,
            );
            let size = pair(&input["size"])?;
            let a = to_view([x, y], size, rot, scale);
            let b = to_view([x + w, y + h], size, rot, scale);
            if !a.iter().chain(&b).all(|n| n.is_finite())
                || !(a[0] - b[0]).is_finite()
                || !(a[1] - b[1]).is_finite()
            {
                return Err("pdf.invalidGeometry");
            }
            Ok(
                json!({"left":a[0].min(b[0]),"top":a[1].min(b[1]),"width":(a[0]-b[0]).abs(),"height":(a[1]-b[1]).abs()}),
            )
        }
        "layout" => {
            let sizes = input["sizes"]
                .as_array()
                .filter(|a| a.len() <= 100_000)
                .ok_or("pdf.invalidGeometry")?;
            let gap = number(&input["gap"])?;
            if gap < 0. {
                return Err("pdf.invalidGeometry");
            }
            let (mut y, mut width) = (gap, 0_f64);
            let mut positions = Vec::with_capacity(sizes.len());
            let mut page_sizes = Vec::with_capacity(sizes.len());
            for size in sizes {
                let size = pair(size)?;
                let [w, h] = shown(size, rot);
                if w < 0. || h < 0. {
                    return Err("pdf.invalidGeometry");
                }
                positions.push(y);
                page_sizes.push(size);
                y += h * scale + gap;
                width = width.max(w * scale);
                if !y.is_finite() || !width.is_finite() {
                    return Err("pdf.invalidGeometry");
                }
            }
            let result = json!({"tops":positions,"height":y,"width":width});
            layout.tops = positions;
            layout.sizes = page_sizes;
            layout.rot = rot;
            layout.scale = scale;
            layout.gap = gap;
            layout.width = width;
            layout.height = y;
            Ok(result)
        }
        _ => Err("pdf.invalidGeometry"),
    }
}
