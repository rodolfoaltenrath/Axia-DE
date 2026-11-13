//! Primitives of the software canvas moving from server/src/canvas.c, one at a time, each with the
//! same pixels as the C it replaces.
//!
//! `canvas_clear` filled its clip one 32-bit store at a time: at -O2 for the x86-64 baseline GCC
//! neither vectorized that loop nor turned it into memset, and it runs for every damaged area and for
//! every app window's hole in the UI layer (4.8 % of the main thread in a desktop profile). Here a row
//! is one `fill`, or a `memset` when the value is 0 (the holes).
//!
//! `canvas_rrect_gradient` worked out everything per pixel: the rounded-rectangle distance, the
//! gradient position from the pixel's column and row, the color between both ends and the coverage
//! weight. Here the column's part and the row's part of the position are computed once each (the
//! same float expressions, so the same bits), the color is looked up among the 257 the position can
//! give, and so is the weight where the coverage is whole.
use crate::canvas_shadow::{blend_px, cov_of, fix_radius, put, rr_sdf};

/// Fills [x0, x1) × [y0, y1) of a canvas with `value`.
///
/// # Safety
/// Every pixel `px + y * stride + x` inside the rectangle is addressable (a canvas.c canvas and its
/// clip: `px` may start before its buffer, as the drag layer's does).
#[no_mangle]
pub unsafe extern "C" fn axia_canvas_fill(px: *mut u32, stride: i32, x0: i32, y0: i32, x1: i32, y1: i32, value: u32) {
    if px.is_null() || stride <= 0 || x0 >= x1 || y0 >= y1 {
        return;
    }
    let n = (x1 - x0) as usize;
    for y in y0..y1 {
        let row = px.offset(y as isize * stride as isize + x0 as isize);
        if value == 0 {
            std::ptr::write_bytes(row, 0, n);
        } else {
            std::slice::from_raw_parts_mut(row, n).fill(value);
        }
    }
}

/// canvas.c's lerp_color for ti = (int)(t * 256): each channel from a to b in 1/256 steps.
#[inline]
fn lerp_steps(a: u32, b: u32, ti: i32) -> u32 {
    let mut out = 0u32;
    for s in (0..32).step_by(8) {
        let (ca, cb) = (((a >> s) & 255) as i32, ((b >> s) & 255) as i32);
        out |= ((ca + (((cb - ca) * ti) >> 8)) as u32) << s;
    }
    out
}

fn clampi(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// canvas_rrect_gradient: a rounded rectangle filled with a linear gradient from `from` to `to` at
/// `angle_deg` (CSS: 180 is top to bottom), times `opacity`, inside the clip [cx0, cx1) × [cy0, cy1).
///
/// # Safety
/// Every pixel `px + y * stride + x` inside the clip is addressable (a canvas.c canvas and its clip).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn axia_canvas_rrect_gradient(
    px: *mut u32,
    stride: i32,
    cx0: i32,
    cy0: i32,
    cx1: i32,
    cy1: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    from: u32,
    to: u32,
    angle_deg: f32,
    opacity: f32,
) {
    if px.is_null() || stride <= 0 || w <= 0. || h <= 0. {
        return;
    }
    let x0 = clampi(x.floor() as i32, cx0, cx1);
    let y0 = clampi(y.floor() as i32, cy0, cy1);
    let x1 = clampi((x + w).ceil() as i32, cx0, cx1);
    let y1 = clampi((y + h).ceil() as i32, cy0, cy1);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let r = fix_radius(r, w, h);
    let (cx, cy, hw, hh) = (x + w * 0.5, y + h * 0.5, w * 0.5, h * 0.5);
    let rad = angle_deg * 3.14159265 / 180.0;
    let (dx, dy) = (rad.sin(), -rad.cos());
    let mut len = (w * dx).abs() + (h * dy).abs();
    if len <= 0. {
        len = 1.;
    }
    // the 257 colors and, for whole coverage, their weights ((a * 1) * opacity: the same product)
    let colors: Vec<u32> = (0..=256).map(|ti| lerp_steps(from, to, ti)).collect();
    let whole: Vec<u32> = colors.iter().map(|c| ((c >> 24) as f32 * 1.0 * opacity + 0.5) as u32).collect();
    let columns: Vec<f32> = (x0..x1).map(|i| (i as f32 + 0.5 - cx) * dx).collect();
    let g = Gradient { cx, cy, dy, hw, hh, r, len, opacity, colors: &colors, whole: &whole, columns: &columns };
    for j in y0..y1 {
        let row = std::slice::from_raw_parts_mut(px.offset(j as isize * stride as isize + x0 as isize), (x1 - x0) as usize);
        gradient_row(&g, row, x0, j);
    }
}

/// What every row of one gradient shares (see `axia_canvas_rrect_gradient`).
struct Gradient<'a> {
    cx: f32,
    cy: f32,
    dy: f32,
    hw: f32,
    hh: f32,
    r: f32,
    len: f32,
    opacity: f32,
    colors: &'a [u32],
    whole: &'a [u32],
    columns: &'a [f32],
}

/// The color step (0..=256) of column k in a row whose row part is `rowterm`: canvas.c's expression.
#[inline(always)]
fn step(g: &Gradient, k: usize, rowterm: f32) -> usize {
    let mut t = 0.5 + (g.columns[k] + rowterm) / g.len;
    t = if t < 0. { 0. } else if t > 1. { 1. } else { t };
    (t * 256.) as i32 as usize
}

#[inline(always)]
fn gradient_row_body(g: &Gradient, row: &mut [u32], x0: i32, j: i32) {
    let py = j as f32 + 0.5;
    let rowterm = (py - g.cy) * g.dy;
    let n = row.len();
    let cov = |k: usize| cov_of(rr_sdf((x0 + k as i32) as f32 + 0.5, py, g.cx, g.cy, g.hw, g.hh, g.r));
    // Where the coverage is whole, [full0, full1): moving toward the center the distance only falls
    // (left of it) and only rises (right of it), by the same argument as `step`, so the run where
    // `cov` is exactly 1 is found by a binary search on each side, and inside it there is no
    // distance to work out per pixel.
    let m = (0..n).find(|&k| (x0 + k as i32) as f32 + 0.5 >= g.cx).unwrap_or(n);
    let full0 = {
        // first column of [0, m) with cov == 1 (non-decreasing there), else m
        let (mut lo, mut hi) = (0, m);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if cov(mid) == 1. {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        lo
    };
    let full1 = {
        // one past the last column of [m, n) with cov == 1 (non-increasing there), else m
        let (mut lo, mut hi) = (m, n);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if cov(mid) == 1. {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    };
    // [full0, m) and [m, full1) are whole each, and they meet at m: one run (either may be empty)
    // `step` is monotone in k: every IEEE operation in it has one varying operand (the column's
    // position, monotone in k) and rounds monotonically, and so do the clamp and the truncation. So a
    // run of columns between two with the same step has that step: each run's end is found by a
    // binary search on the exact expression, and the division is done a few times per run instead of
    // once per pixel (one run for a whole row of a vertical gradient).
    let mut k = 0;
    while k < n {
        let ti = step(g, k, rowterm);
        let (mut lo, mut hi) = (k, n - 1);
        if step(g, hi, rowterm) != ti {
            // the last column with this step lies in [lo, hi)
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if step(g, mid, rowterm) == ti {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
        } else {
            lo = hi;
        }
        let (col, whole) = (g.colors[ti], g.whole[ti]);
        let (f0, f1) = (full0.clamp(k, lo + 1), full1.clamp(k, lo + 1));
        for i in (k..f0).chain(f1..lo + 1) {
            let c = cov(i);
            if c <= 0. {
                continue;
            }
            let weight = if c == 1. { whole } else { ((col >> 24) as f32 * c * g.opacity + 0.5) as u32 };
            put(&mut row[i], col, weight);
        }
        if f0 < f1 {
            if whole >= 255 {
                row[f0..f1].fill(col | 0xff00_0000); // put()'s opaque case
            } else {
                for p in &mut row[f0..f1] {
                    put(p, col, whole);
                }
            }
        }
        k = lo + 1;
    }
}

// The same row compiled for AVX2 as well, picked at run time (as glass_render.rs): IEEE f32
// arithmetic, never fused, so the same pixels.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn gradient_row_avx2(g: &Gradient, row: &mut [u32], x0: i32, j: i32) {
    gradient_row_body(g, row, x0, j)
}

fn gradient_row(g: &Gradient, row: &mut [u32], x0: i32, j: i32) {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        return unsafe { gradient_row_avx2(g, row, x0, j) };
    }
    gradient_row_body(g, row, x0, j)
}

/// The columns [s0, s1) of a rounded rectangle's row whose pixel centers lie between the corners
/// (canvas.c's mid_span): there the distance doesn't depend on the column. (x1, x1) if there are none.
fn mid_span(cx: f32, lim: f32, x0: i32, x1: i32) -> (i32, i32) {
    let mut a = (cx - lim - 0.5).ceil() as i32;
    let mut b = (cx + lim - 0.5).floor() as i32 + 1;
    a = a.max(x0);
    b = b.min(x1);
    // the same float test as rr_sdf, so the span's edges agree exactly
    while a < b && (a as f32 + 0.5 - cx).abs() - lim > 0. {
        a += 1;
    }
    while b > a && (b as f32 - 0.5 - cx).abs() - lim > 0. {
        b -= 1;
    }
    if a < b { (a, b) } else { (x1, x1) }
}

/// A translucent run: `argb` over each pixel with weight `a` (canvas.c's blend), with AVX2 if there is
/// one (integer arithmetic only: the same pixels).
fn blend_run(run: &mut [u32], argb: u32, a: u32) {
    #[inline(always)]
    fn body(run: &mut [u32], argb: u32, a: u32) {
        for p in run {
            *p = blend_px(*p, argb, a);
        }
    }
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn avx2(run: &mut [u32], argb: u32, a: u32) {
        body(run, argb, a)
    }
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        return unsafe { avx2(run, argb, a) };
    }
    body(run, argb, a)
}

/// canvas_rrect: a rounded rectangle of one color, antialiased, inside the clip.
///
/// # Safety
/// Every pixel `px + y * stride + x` inside the clip is addressable (a canvas.c canvas and its clip).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn axia_canvas_rrect(
    px: *mut u32, stride: i32, cx0: i32, cy0: i32, cx1: i32, cy1: i32,
    x: f32, y: f32, w: f32, h: f32, r: f32, argb: u32,
) {
    let a = argb >> 24;
    if px.is_null() || stride <= 0 || a == 0 || !(w > 0.) || !(h > 0.) || ![x, y, w, h, r].iter().all(|v| v.is_finite()) {
        return;
    }
    let x0 = clampi(x.floor() as i32, cx0, cx1);
    let y0 = clampi(y.floor() as i32, cy0, cy1);
    let x1 = clampi((x + w).ceil() as i32, cx0, cx1);
    let y1 = clampi((y + h).ceil() as i32, cy0, cy1);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let r = fix_radius(r, w, h);
    let (cx, cy, hw, hh) = (x + w * 0.5, y + h * 0.5, w * 0.5, h * 0.5);
    let (fx0, fx1) = (x.ceil() as i32, (x + w).floor() as i32); // fully covered columns
    let edge = |row: &mut [u32], i: i32| {
        let cov = (i as f32 + 1.0).min(x + w) - (i as f32).max(x);
        if cov > 0. {
            put(&mut row[(i - x0) as usize], argb, (a as f32 * cov + 0.5) as u32);
        }
    };
    for j in y0..y1 {
        let row = std::slice::from_raw_parts_mut(px.offset(j as isize * stride as isize + x0 as isize), (x1 - x0) as usize);
        if j as f32 >= y + r && (j + 1) as f32 <= y + h - r {
            // the straight band: only the left and right edges are partial; the middle, in one run
            let m0 = fx0.max(x0);
            let m1 = fx1.min(x1).max(m0);
            for i in x0..m0 {
                edge(row, i);
            }
            let middle = &mut row[(m0 - x0) as usize..(m1 - x0) as usize];
            if a >= 255 {
                middle.fill(argb | 0xff00_0000);
            } else {
                blend_run(middle, argb, a);
            }
            for i in m1..x1 {
                edge(row, i);
            }
        } else {
            let py = j as f32 + 0.5;
            // between the corners the distance only depends on the row: worked out once there
            let (s0, s1) = if (py - cy).abs() - (hh - r) > 0. { mid_span(cx, hw - r, x0, x1) } else { (x1, x1) };
            let mut i = x0;
            while i < x1 {
                let cov = cov_of(rr_sdf(i as f32 + 0.5, py, cx, cy, hw, hh, r));
                if i == s0 {
                    if cov > 0. {
                        let k = (a as f32 * cov + 0.5) as u32;
                        for p in &mut row[(i - x0) as usize..(s1 - x0) as usize] {
                            put(p, argb, k);
                        }
                    }
                    i = s1;
                    continue;
                }
                if cov > 0. {
                    put(&mut row[(i - x0) as usize], argb, (a as f32 * cov + 0.5) as u32);
                }
                i += 1;
            }
        }
    }
}

/// canvas_rrect_stroke: the outline of a rounded rectangle, `bw` wide inward, inside the clip.
///
/// # Safety
/// As `axia_canvas_rrect`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn axia_canvas_rrect_stroke(
    px: *mut u32, stride: i32, cx0: i32, cy0: i32, cx1: i32, cy1: i32,
    x: f32, y: f32, w: f32, h: f32, r: f32, bw: f32, argb: u32,
) {
    let a = argb >> 24;
    if px.is_null() || stride <= 0 || a == 0 || !(bw > 0.) || !(w > 0.) || !(h > 0.) || ![x, y, w, h, r, bw].iter().all(|v| v.is_finite()) {
        return;
    }
    let x0 = clampi(x.floor() as i32, cx0, cx1);
    let y0 = clampi(y.floor() as i32, cy0, cy1);
    let x1 = clampi((x + w).ceil() as i32, cx0, cx1);
    let y1 = clampi((y + h).ceil() as i32, cy0, cy1);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let r = fix_radius(r, w, h);
    let ir = if r - bw > 0. { r - bw } else { 0. };
    let (cx, cy, hw, hh) = (x + w * 0.5, y + h * 0.5, w * 0.5, h * 0.5);
    let edge = (if r > bw { r } else { bw }) + 1.;
    for j in y0..y1 {
        let row = std::slice::from_raw_parts_mut(px.offset(j as isize * stride as isize + x0 as isize), (x1 - x0) as usize);
        let py = j as f32 + 0.5;
        let middle = py > y + edge && py < y + h - edge;
        // the span where both distances only depend on the row (rows outside both straight bands)
        let (mut s0, mut s1) = (x1, x1);
        if !middle && (py - cy).abs() - (hh - r) > 0. && (py - cy).abs() - ((hh - bw) - ir) > 0. {
            let (a0, a1) = mid_span(cx, hw - r, x0, x1);
            let (b0, b1) = mid_span(cx, (hw - bw) - ir, x0, x1);
            if a0 < x1 && b0 < x1 {
                s0 = a0.max(b0);
                s1 = a1.min(b1);
                if s0 >= s1 {
                    s0 = x1;
                    s1 = x1;
                }
            }
        }
        let mut i = x0;
        while i < x1 {
            let pxc = i as f32 + 0.5;
            if middle && pxc > x + bw + 1. && pxc < x + w - bw - 1. {
                // skips the middle, up to the last pixel whose center is before the right edge
                let last = (x + w - bw - 1. - 0.5).ceil() as i32 - 1;
                if last > i {
                    i = last;
                }
                i += 1;
                continue;
            }
            if i == s0 {
                let cov = cov_of(rr_sdf(pxc, py, cx, cy, hw, hh, r)) - cov_of(rr_sdf(pxc, py, cx, cy, hw - bw, hh - bw, ir));
                if cov > 0. {
                    let k = (a as f32 * cov + 0.5) as u32;
                    for p in &mut row[(i - x0) as usize..(s1 - x0) as usize] {
                        put(p, argb, k);
                    }
                }
                i = s1;
                continue;
            }
            let outer = cov_of(rr_sdf(pxc, py, cx, cy, hw, hh, r));
            let inner = cov_of(rr_sdf(pxc, py, cx, cy, hw - bw, hh - bw, ir));
            let cov = outer - inner;
            if cov > 0. {
                put(&mut row[(i - x0) as usize], argb, (a as f32 * cov + 0.5) as u32);
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_exactly_the_rectangle() {
        for value in [0u32, 0xff12_3456, 0x0101_0101] {
            let (w, h) = (37, 23);
            let mut px: Vec<u32> = (0..w * h).map(|i| (i as u32).wrapping_mul(2_654_435_761)).collect();
            let before = px.clone();
            let (x0, y0, x1, y1) = (3, 4, 30, 20);
            unsafe { axia_canvas_fill(px.as_mut_ptr(), w, x0, y0, x1, y1, value) };
            for y in 0..h {
                for x in 0..w {
                    let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
                    let i = (y * w + x) as usize;
                    assert_eq!(px[i], if inside { value } else { before[i] }, "({x}, {y})");
                }
            }
        }
        let mut one = [7u32];
        unsafe { axia_canvas_fill(one.as_mut_ptr(), 1, 0, 0, 0, 1, 0) };
        assert_eq!(one, [7], "an empty rectangle");
    }
}
