//! Box shadows of the software canvas (server/src/canvas.c `canvas_shadow`).
//!
//! A shadow is a blurred rounded rectangle drawn around a box and never under it. Its coverage mask
//! is computed once per size (an LRU of 32, like the C it replaces) and blended into the canvas.
//!
//! The profile of a desktop with app windows open showed a fifth of the compositor's main thread in
//! that blend: the mask covers the whole window plus the blur, but under the window it is zero, and
//! every one of those zero bytes was read for each repainted row. A maximized window is two million
//! of them. Each mask row now keeps where its coverage is (`Spans`: the part before the hole under the
//! box and the part after it), so the blend only walks the ring. The pixels are the same.
use std::cell::RefCell;

const CACHE: usize = 32;

/// The mask as C sees it (server/src/shadow_ffi.h). `spans` has four numbers per row:
/// coverage only in [a0, a1) and [b0, b1) (columns of the mask; a1 <= b0).
#[repr(C)]
pub struct ShadowMask {
    pub x0: i32,
    pub y0: i32,
    pub mw: i32,
    pub mh: i32,
    pub a: *const u8,
    pub spans: *const i32,
}

struct Entry {
    key: (i32, i32, i32, i32, i32, i32),
    x0: i32,
    y0: i32,
    mw: i32,
    mh: i32,
    a: Vec<u8>,
    spans: Vec<i32>,
    used: u64,
    view: ShadowMask,
}

#[derive(Default)]
struct Cache {
    entries: Vec<Entry>,
    stamp: u64,
}

thread_local! {
    static SHADOWS: RefCell<Cache> = RefCell::new(Cache::default());
}

// ---- the C geometry, operation for operation in f32 (canvas.c) ----

pub(crate) fn rr_sdf(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let ox = if qx > 0. { qx } else { 0. };
    let oy = if qy > 0. { qy } else { 0. };
    let inn = if qx > qy { qx } else { qy };
    (if ox != 0. || oy != 0. { (ox * ox + oy * oy).sqrt() } else { 0. }) + (if inn < 0. { inn } else { 0. }) - r
}

pub(crate) fn cov_of(d: f32) -> f32 {
    let c = 0.5 - d;
    if c <= 0. {
        0.
    } else if c >= 1. {
        1.
    } else {
        c
    }
}

pub(crate) fn fix_radius(r: f32, w: f32, h: f32) -> f32 {
    let m = (if w < h { w } else { h }) * 0.5;
    if r < 0. {
        0.
    } else if r > m {
        m
    } else {
        r
    }
}

/// The coverage mask of a w×h box with radius r, blurred by `blur` and offset by (dx, dy), in a
/// mw×mh grid whose (0, 0) is (x0, y0) from the box's corner.
fn build(w: i32, h: i32, r: i32, blur: i32, dx: i32, dy: i32) -> (i32, i32, i32, i32, Vec<u8>) {
    let x0 = (if dx < 0 { dx } else { 0 }) - blur;
    let y0 = (if dy < 0 { dy } else { 0 }) - blur;
    let mw = w + dx.abs() + 2 * blur;
    let mh = h + dy.abs() + 2 * blur;
    let mut a = vec![0u8; mw.max(0) as usize * mh.max(0) as usize];
    let (wf, hf) = (w as f32, h as f32);
    let fr = fix_radius(r as f32, wf, hf);
    let fb = blur as f32;
    let (bcx, bcy) = (wf * 0.5, hf * 0.5);
    let (scx, scy) = (bcx + dx as f32, bcy + dy as f32);
    let (hw, hh) = (wf * 0.5, hf * 0.5);
    for j in 0..mh {
        let py = (y0 + j) as f32 + 0.5;
        for i in 0..mw {
            let px = (x0 + i) as f32 + 0.5;
            let inside = cov_of(rr_sdf(px, py, bcx, bcy, hw, hh, fr));
            if inside >= 1. {
                continue;
            }
            let mut t = (rr_sdf(px, py, scx, scy, hw, hh, fr) + fb) / (2. * fb);
            if t >= 1. {
                continue;
            }
            if t < 0. {
                t = 0.;
            }
            let f = (1. - t * t * (3. - 2. * t)) * (1. - inside);
            a[(j * mw + i) as usize] = (f * 255. + 0.5) as u8;
        }
    }
    (x0, y0, mw, mh, a)
}

/// Per row, [a0, a1) and [b0, b1) hold every nonzero byte: the longest run of zeros between the
/// first and the last nonzero one is left out (under the box). A row of zeros is four zeros.
fn spans(a: &[u8], mw: usize, mh: usize) -> Vec<i32> {
    let mut out = Vec::with_capacity(mh * 4);
    for row in a.chunks(mw.max(1)).take(mh) {
        let Some(first) = row.iter().position(|&v| v != 0) else {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        };
        let last = row.iter().rposition(|&v| v != 0).unwrap() + 1;
        let (mut hole, mut best, mut run) = ((last, last), 0, None);
        for (i, &v) in row.iter().enumerate().take(last).skip(first) {
            match (v, run) {
                (0, None) => run = Some(i),
                (0, Some(_)) => {}
                (_, Some(s)) => {
                    if i - s > best {
                        best = i - s;
                        hole = (s, i);
                    }
                    run = None;
                }
                (_, None) => {}
            }
        }
        out.extend_from_slice(&[first as i32, hole.0 as i32, hole.1 as i32, last as i32]);
    }
    out
}

fn mask(w: i32, h: i32, r: i32, blur: i32, dx: i32, dy: i32) -> *const ShadowMask {
    let key = (w, h, r, blur, dx, dy);
    SHADOWS.with(|cell| {
        let mut c = cell.borrow_mut();
        c.stamp += 1;
        let stamp = c.stamp;
        let i = match c.entries.iter().position(|e| e.key == key) {
            Some(i) => i,
            None => {
                let (x0, y0, mw, mh, a) = build(w, h, r, blur, dx, dy);
                let spans = spans(&a, mw.max(0) as usize, mh.max(0) as usize);
                let view = ShadowMask { x0, y0, mw, mh, a: a.as_ptr(), spans: spans.as_ptr() };
                let e = Entry { key, x0, y0, mw, mh, a, spans, used: 0, view };
                if c.entries.len() < CACHE {
                    c.entries.push(e);
                    c.entries.len() - 1
                } else {
                    let lru = (0..c.entries.len()).min_by_key(|&k| c.entries[k].used).unwrap_or(0);
                    c.entries[lru] = e;
                    lru
                }
            }
        };
        let e = &mut c.entries[i];
        e.used = stamp;
        // the Vecs never move inside an entry: the pointers stay valid until it is evicted
        e.view = ShadowMask { x0: e.x0, y0: e.y0, mw: e.mw, mh: e.mh, a: e.a.as_ptr(), spans: e.spans.as_ptr() };
        &e.view as *const ShadowMask
    })
}

// ---- blending, as canvas.c's put()/blend() ----

#[inline]
fn over_alpha(sa: u32, da: u32) -> u32 {
    sa + (((da * (255 - sa) + 128) * 257) >> 16)
}

/// canvas.c's blend(): `src` over `dst` with weight a (0..255), alpha as "over" leaves it.
#[inline]
pub(crate) fn blend_px(dst: u32, src: u32, a: u32) -> u32 {
    let ia = 255 - a;
    let rb = (((src & 0x00ff_00ff) * a + (dst & 0x00ff_00ff) * ia) >> 8) & 0x00ff_00ff;
    let g = (((src & 0x0000_ff00) * a + (dst & 0x0000_ff00) * ia) >> 8) & 0x0000_ff00;
    over_alpha(a, dst >> 24) << 24 | rb | g
}

#[inline]
pub(crate) fn put(p: &mut u32, argb: u32, a: u32) {
    if a >= 255 {
        *p = argb | 0xff00_0000;
    } else if a != 0 {
        *p = blend_px(*p, argb, a);
    }
}

/// Blends `m` with its corner at (x, y) into the rows of `px` inside the clip.
///
/// # Safety
/// `px` addresses at least `clip_y1` rows of `stride` pixels, the clip lies inside them, and `m`
/// came from `axia_shadow_mask` with no call to it since.
#[allow(clippy::too_many_arguments)]
unsafe fn blend(px: *mut u32, stride: i32, clip: [i32; 4], x: i32, y: i32, m: &ShadowMask, argb: u32) {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        return blend_avx2(px, stride, clip, x, y, m, argb);
    }
    blend_rows(px, stride, clip, x, y, m, argb)
}

// The same rows compiled for AVX2 (integer arithmetic only: the same pixels), picked at run time.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn blend_avx2(px: *mut u32, stride: i32, clip: [i32; 4], x: i32, y: i32, m: &ShadowMask, argb: u32) {
    blend_rows(px, stride, clip, x, y, m, argb)
}

#[inline(always)]
unsafe fn blend_rows(px: *mut u32, stride: i32, clip: [i32; 4], x: i32, y: i32, m: &ShadowMask, argb: u32) {
    let ca = argb >> 24;
    let [cx0, cy0, cx1, cy1] = clip;
    let (j0, j1) = (y.max(cy0), (y + m.mh).min(cy1));
    for j in j0..j1 {
        let mj = (j - y) as usize;
        let s = std::slice::from_raw_parts(m.spans.add(mj * 4), 4);
        let row = px.offset(j as isize * stride as isize);
        let mrow = m.a.add(mj * m.mw as usize);
        for (a0, a1) in [(s[0], s[1]), (s[2], s[3])] {
            let (i0, i1) = ((x + a0).max(cx0), (x + a1).min(cx1));
            if i0 >= i1 {
                continue;
            }
            let n = (i1 - i0) as usize;
            let dst = std::slice::from_raw_parts_mut(row.offset(i0 as isize), n);
            let cov = std::slice::from_raw_parts(mrow.add((i0 - x) as usize), n);
            blend_span(dst, cov, argb, ca);
        }
    }
}

/// put() over a span, without branches so it vectorizes: a weight of 0 keeps the pixel (a zero
/// byte of the mask gives 0, as the skip it replaces), 255 writes the opaque color, the rest blend.
#[inline(always)]
fn blend_span(dst: &mut [u32], cov: &[u8], argb: u32, ca: u32) {
    let full = argb | 0xff00_0000;
    for (p, &v) in dst.iter_mut().zip(cov) {
        let a = (v as u32 * ca + 127) / 255;
        let d = *p;
        let ia = 255 - a.min(255);
        let rb = (((argb & 0x00ff_00ff) * a + (d & 0x00ff_00ff) * ia) >> 8) & 0x00ff_00ff;
        let g = (((argb & 0x0000_ff00) * a + (d & 0x0000_ff00) * ia) >> 8) & 0x0000_ff00;
        let blended = over_alpha(a.min(255), d >> 24) << 24 | rb | g;
        *p = if a >= 255 {
            full
        } else if a == 0 {
            d
        } else {
            blended
        };
    }
}

// ---- C ABI (server/src/shadow_ffi.h) ----

/// The cached mask of a shadow (box size, radius, blur ≥ 1 and offset in whole pixels); NULL for an
/// empty box. Valid until the next call. UI thread only.
#[no_mangle]
pub extern "C" fn axia_shadow_mask(w: i32, h: i32, r: i32, blur: i32, dx: i32, dy: i32) -> *const ShadowMask {
    if w <= 0 || h <= 0 || blur < 1 || w > 16384 || h > 16384 || blur > 1024 || dx.abs() > 4096 || dy.abs() > 4096 {
        return std::ptr::null();
    }
    mask(w, h, r, blur, dx, dy)
}

/// Blends a mask from `axia_shadow_mask` (any thread, while it is valid) at (x, y) into the canvas
/// rows inside [cx0, cx1) × [cy0, cy1).
///
/// # Safety
/// Every pixel `px + j * stride + i` inside the clip is addressable (a canvas.c canvas and its clip).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn axia_shadow_blend(
    px: *mut u32,
    stride: i32,
    cx0: i32,
    cy0: i32,
    cx1: i32,
    cy1: i32,
    x: i32,
    y: i32,
    m: *const ShadowMask,
    argb: u32,
) {
    let Some(m) = m.as_ref() else { return };
    // No bounds of its own beyond the clip: canvas.c also gives canvases whose `px` starts before
    // their buffer and whose width passes the stride (the dragged window's layer, ui/paint.c), and
    // their clip holds only addressable pixels, as for every other primitive.
    if px.is_null() || stride <= 0 || cx0 >= cx1 || cy0 >= cy1 {
        return;
    }
    blend(px, stride, [cx0, cy0, cx1, cy1], x, y, m, argb);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The former blend: every byte of the mask, zero or not.
    fn reference(px: &mut [u32], stride: i32, clip: [i32; 4], x: i32, y: i32, a: &[u8], mw: i32, mh: i32, argb: u32) {
        let ca = argb >> 24;
        // canvas.c's clampi: no panic when the clip is empty (lo > hi)
        let clampi = |v: i32, lo: i32, hi: i32| if v < lo { lo } else if v > hi { hi } else { v };
        let (x0, x1) = (clampi(x, clip[0], clip[2]), clampi(x + mw, clip[0], clip[2]));
        let (y0, y1) = (clampi(y, clip[1], clip[3]), clampi(y + mh, clip[1], clip[3]));
        for j in y0..y1 {
            for i in x0..x1 {
                let v = a[((j - y) * mw + (i - x)) as usize] as u32;
                if v != 0 {
                    put(&mut px[(j * stride + i) as usize], argb, (v * ca + 127) / 255);
                }
            }
        }
    }

    fn canvas(w: i32, h: i32, seed: u32) -> Vec<u32> {
        let mut s = seed;
        (0..w * h)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                if s & 1 == 0 { s | 0xff00_0000 } else { s & 0x7f7f_7f7f }
            })
            .collect()
    }

    #[test]
    fn spans_skip_only_zeros_and_blend_the_same_pixels() {
        let mut seed = 7u32;
        let mut next = |n: i32| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 8) as i32 % n
        };
        for case in 0..300 {
            let (w, h) = (1 + next(220), 1 + next(160));
            let (r, blur) = (next(40) - 2, 1 + next(30));
            let (dx, dy) = (next(21) - 10, next(21) - 10);
            let (x0, y0, mw, mh, a) = build(w, h, r, blur, dx, dy);
            let sp = spans(&a, mw as usize, mh as usize);
            for j in 0..mh as usize {
                let s = &sp[j * 4..j * 4 + 4];
                assert!(s[0] <= s[1] && s[1] <= s[2] && s[2] <= s[3], "case {case} row {j}: {s:?}");
                for i in 0..mw as usize {
                    let covered = (s[0] as usize..s[1] as usize).contains(&i) || (s[2] as usize..s[3] as usize).contains(&i);
                    assert!(covered || a[j * mw as usize + i] == 0, "case {case}: a nonzero byte outside the spans");
                }
            }
            let (cw, ch) = (mw + 40, mh + 40);
            let clip = [next(30), next(30), cw - next(30), ch - next(30)];
            let argb = (next(256) as u32) << 24 | 0x0012_3456 ^ ((case as u32).wrapping_mul(0x0101_0101) & 0x00ff_ffff);
            let (x, y) = (next(60) - 10, next(60) - 10);
            let mut want = canvas(cw, ch, case);
            let mut got = want.clone();
            reference(&mut want, cw, clip, x, y, &a, mw, mh, argb);
            let view = ShadowMask { x0, y0, mw, mh, a: a.as_ptr(), spans: sp.as_ptr() };
            unsafe { blend(got.as_mut_ptr(), cw, clip, x, y, &view, argb) };
            assert_eq!(got, want, "case {case}");
        }
    }

    #[test]
    fn a_window_shadow_walks_only_its_ring() {
        let (_, _, mw, mh, a) = build(1920, 1000, 12, 24, 0, 8);
        let sp = spans(&a, mw as usize, mh as usize);
        let walked: i64 = sp.chunks(4).map(|s| (s[1] - s[0] + s[3] - s[2]) as i64).sum();
        let whole = mw as i64 * mh as i64;
        // 6.5 %: 135 500 of 2 078 208 bytes
        assert!(walked * 10 < whole, "{walked} of {whole} bytes");
    }

    #[test]
    fn the_cache_reuses_and_evicts() {
        let p = axia_shadow_mask(100, 80, 8, 10, 0, 4);
        assert!(!p.is_null());
        assert_eq!(axia_shadow_mask(100, 80, 8, 10, 0, 4), p, "same key, same entry");
        for k in 0..CACHE as i32 {
            axia_shadow_mask(10 + k, 10, 0, 2, 0, 0);
        }
        let m = unsafe { &*axia_shadow_mask(100, 80, 8, 10, 0, 4) };
        assert_eq!((m.x0, m.y0, m.mw, m.mh), (-10, -10, 120, 104));
        assert!(axia_shadow_mask(0, 10, 0, 2, 0, 0).is_null());
        assert!(axia_shadow_mask(10, 10, 0, 0, 0, 0).is_null());
        unsafe { axia_shadow_blend(std::ptr::null_mut(), 10, 0, 0, 1, 1, 0, 0, m, 0xff00_0000) };
    }
}
