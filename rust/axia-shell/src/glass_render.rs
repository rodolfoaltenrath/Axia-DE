//! Pixel-identical port of the backdrop renderer. C supplies only the canvas and
//! its existing synchronous worker pool. All glass math and pixel loops live here.
use crate::glass::{
    axia_glass_commit, axia_glass_prepare, axia_glass_reuse, axia_glass_scratch, GlassBuffers,
};
use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};
#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Canvas {
    px: *mut u32,
    w: i32,
    h: i32,
    stride: i32,
    clip: Rect,
}
type Kernel = unsafe extern "C" fn(*mut c_void, i32, i32);
type Parallel = unsafe extern "C" fn(i32, u64, Kernel, *mut c_void);
struct Blur {
    c: Canvas,
    x0: i32,
    y0: i32,
    x1: i32,
    bx0: i32,
    by0: i32,
    bx1: i32,
    by1: i32,
    f: i32,
    sw: i32,
    sh: i32,
    rad: i32,
    cw: i32,
    in0: i32,
    in1: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
    inv: f32,
    buf: *mut u32,
    src: *mut u32,
    dt: *const u8,
    sum: *mut i32,
    cx0: *mut i32,
    cx1: *mut i32,
    cfx: *mut i32,
    failed: AtomicBool,
    backdrop: *const u32,
}
unsafe fn phase(b: &Blur, n: i32, work: u64, run: Kernel, parallel: Option<Parallel>) {
    let ctx = b as *const Blur as *mut c_void;
    if let Some(p) = parallel {
        p(n, work, run, ctx);
    } else {
        run(ctx, 0, n);
    }
}
// The kernels below are compiled twice. The x86-64 baseline (SSE2) has no 32-bit vector multiply,
// so their loops ran one value at a time; with AVX2, detected at run time, the same loops vectorize.
// Integer arithmetic, and float expressions Rust never fuses (no mul+add contraction): the same pixels.
macro_rules! kernel {
    ($name:ident, $body:ident, $avx2:ident) => {
        #[cfg(target_arch = "x86_64")]
        #[target_feature(enable = "avx2")]
        unsafe fn $avx2(ctx: *mut c_void, a: i32, b: i32) {
            $body(ctx, a, b)
        }
        unsafe extern "C" fn $name(ctx: *mut c_void, a: i32, b: i32) {
            #[cfg(target_arch = "x86_64")]
            if std::is_x86_feature_detected!("avx2") {
                return $avx2(ctx, a, b);
            }
            $body(ctx, a, b)
        }
    };
}
kernel!(down, down_body, down_avx2);
kernel!(rows, rows_body, rows_avx2);
kernel!(cols, cols_body, cols_avx2);
kernel!(up, up_body, up_avx2);
// Kernels share an immutable job. Only disjoint rows/columns of raw pixel arrays
// are written by different workers. No &mut Blur/Canvas aliases are constructed.
#[inline(always)]
unsafe fn down_body(ctx: *mut c_void, j0: i32, j1: i32) {
    let b = &*ctx.cast::<Blur>();
    let f = b.f;
    let sw = b.sw;
    if f == 1 {
        for j in j0..j1 {
            ptr::copy_nonoverlapping(
                b.c.px.offset(((b.by0 + j) * b.c.stride + b.bx0) as isize),
                b.buf.offset((j * sw) as isize),
                sw as usize,
            );
        }
        return;
    }
    let acc = axia_glass_scratch(sw as usize * 2);
    if acc.is_null() {
        b.failed.store(true, Ordering::Relaxed);
        return;
    }
    let span = b.bx1 - b.bx0;
    let full = span / f;
    // Slices: the compiler knows the row and the sums don't overlap and vectorizes the blocks.
    let sums = std::slice::from_raw_parts_mut(acc, sw as usize * 2);
    for j in j0..j1 {
        sums.fill(0);
        let rows = (b.by1 - (b.by0 + j * f)).min(f);
        for v in 0..rows {
            let row = std::slice::from_raw_parts(
                b.c.px.offset(((b.by0 + j * f + v) * b.c.stride + b.bx0) as isize),
                span as usize,
            );
            let (whole, rest) = row.split_at((full * f) as usize);
            for (block, sum) in whole.chunks_exact(f as usize).zip(sums.chunks_exact_mut(2)) {
                let (mut rb, mut ag) = (0u32, 0u32);
                for &p in block {
                    rb += p & 0x00ff00ff;
                    ag += (p >> 8) & 0x00ff00ff;
                }
                sum[0] += rb;
                sum[1] += ag;
            }
            for &p in rest {
                sums[(full * 2) as usize] += p & 0x00ff00ff;
                sums[(full * 2 + 1) as usize] += (p >> 8) & 0x00ff00ff;
            }
        }
        for i in 0..sw {
            let n = ((b.bx1 - (b.bx0 + i * f)).min(f) * rows) as u32;
            let rb = sums[(i * 2) as usize];
            let ag = sums[(i * 2 + 1) as usize];
            *b.buf.offset((j * sw + i) as isize) = if n.is_power_of_two() {
                // every whole block (f × f = 1, 4 or 16 pixels): an unsigned division by a power of
                // two is exactly a shift, without the four divisions per pixel
                let s = n.trailing_zeros();
                (ag >> 16 >> s) << 24 | (rb >> 16 >> s) << 16 | ((ag & 0xffff) >> s) << 8 | (rb & 0xffff) >> s
            } else if n > 0 {
                ((ag >> 16) / n) << 24
                    | ((rb >> 16) / n) << 16
                    | ((ag & 0xffff) / n) << 8
                    | (rb & 0xffff) / n
            } else {
                0
            };
        }
    }
}
// Horizontal output goes straight to the second buffer. The original C path
// copied each row, blurred in place, then copied the full intermediate again.
// Separate read/write buffers remove both copies without changing any rounding.
#[inline(always)]
unsafe fn rows_body(ctx: *mut c_void, j0: i32, j1: i32) {
    let b = &*ctx.cast::<Blur>();
    let w = b.sw;
    let rad = b.rad;
    for j in j0..j1 {
        let input = b.buf.offset((j * w) as isize);
        let output = b.src.offset((j * w) as isize);
        let (mut sa, mut sr, mut sg, mut sb) = (0i32, 0i32, 0i32, 0i32);
        for k in -rad..=rad {
            let p = *input.offset(k.clamp(0, w - 1) as isize);
            sa += (p >> 24) as i32;
            sr += ((p >> 16) & 255) as i32;
            sg += ((p >> 8) & 255) as i32;
            sb += (p & 255) as i32;
        }
        for i in 0..w {
            *output.offset(i as isize) = (*b.dt.offset(sa as isize) as u32) << 24
                | (*b.dt.offset(sr as isize) as u32) << 16
                | (*b.dt.offset(sg as isize) as u32) << 8
                | *b.dt.offset(sb as isize) as u32;
            let a = *input.offset((i + rad + 1).min(w - 1) as isize);
            let d = *input.offset((i - rad).max(0) as isize);
            sa += (a >> 24) as i32 - (d >> 24) as i32;
            sr += ((a >> 16) & 255) as i32 - ((d >> 16) & 255) as i32;
            sg += ((a >> 8) & 255) as i32 - ((d >> 8) & 255) as i32;
            sb += (a & 255) as i32 - (d & 255) as i32;
        }
    }
}
#[inline(always)]
unsafe fn cols_body(ctx: *mut c_void, i0: i32, i1: i32) {
    let b = &*ctx.cast::<Blur>();
    let (w, h, rad) = (b.sw, b.sh, b.rad);
    ptr::write_bytes(b.sum.offset((i0 * 4) as isize), 0, ((i1 - i0) * 4) as usize);
    for k in -rad..=rad {
        let row = b.src.offset((k.clamp(0, h - 1) * w) as isize);
        for i in i0..i1 {
            let p = *row.offset(i as isize);
            let s = b.sum.offset((i * 4) as isize);
            *s += (p >> 24) as i32;
            *s.add(1) += ((p >> 16) & 255) as i32;
            *s.add(2) += ((p >> 8) & 255) as i32;
            *s.add(3) += (p & 255) as i32;
        }
    }
    for j in 0..h {
        let out = b.buf.offset((j * w) as isize);
        let add = b.src.offset(((j + rad + 1).min(h - 1) * w) as isize);
        let sub = b.src.offset(((j - rad).max(0) * w) as isize);
        for i in i0..i1 {
            let s = b.sum.offset((i * 4) as isize);
            let [sa, sr, sg, sb] = *s.cast::<[i32; 4]>();
            let a = *add.offset(i as isize);
            let d = *sub.offset(i as isize);
            *out.offset(i as isize) = (*b.dt.offset(sa as isize) as u32) << 24
                | (*b.dt.offset(sr as isize) as u32) << 16
                | (*b.dt.offset(sg as isize) as u32) << 8
                | *b.dt.offset(sb as isize) as u32;
            *s.cast::<[i32; 4]>() = [
                sa + (a >> 24) as i32 - (d >> 24) as i32,
                sr + ((a >> 16) & 255) as i32 - ((d >> 16) & 255) as i32,
                sg + ((a >> 8) & 255) as i32 - ((d >> 8) & 255) as i32,
                sb + (a & 255) as i32 - (d & 255) as i32,
            ];
        }
    }
}
#[inline]
fn over_alpha(a: u32, d: u32) -> u32 {
    a + (((d * (255 - a) + 128) * 257) >> 16)
}
#[inline]
fn over(p: u32, d: u32, a: u32) -> u32 {
    let (pa, prb, pg) = if a >= 255 {
        (p >> 24, p & 0xff00ff, p & 0xff00)
    } else {
        (
            ((p >> 24) * a + 127) / 255,
            (((p & 0xff00ff) * a) / 255) & 0xff00ff,
            (((p & 0xff00) * a) / 255) & 0xff00,
        )
    };
    let ia = 255 - pa;
    let rb = (((d & 0xff00ff) * ia) >> 8) & 0xff00ff;
    let g = (((d & 0xff00) * ia) >> 8) & 0xff00;
    (over_alpha(pa, d >> 24) << 24) | (prb + rb) | (pg + g)
}

unsafe extern "C" fn backdrop_over(ctx: *mut c_void, j0: i32, j1: i32) {
    let b = &*ctx.cast::<Blur>();
    for i in j0 * b.sw..j1 * b.sw {
        let p = b.buf.offset(i as isize);
        if *p >> 24 < 255 {
            *p = over(*p, *b.backdrop.offset(i as isize), 255);
        }
    }
}
#[inline]
fn coverage(b: &Blur, i: i32, j: i32) -> f32 {
    let qx = (i as f32 + 0.5 - b.cx).abs() - (b.hw - b.r);
    let qy = (j as f32 + 0.5 - b.cy).abs() - (b.hh - b.r);
    let (ox, oy) = (qx.max(0.0), qy.max(0.0));
    let inside = qx.max(qy);
    let distance = if ox != 0.0 || oy != 0.0 {
        (ox * ox + oy * oy).sqrt()
    } else {
        0.0
    } + inside.min(0.0)
        - b.r;
    (0.5 - distance).clamp(0.0, 1.0)
}
#[inline(always)]
unsafe fn up_body(ctx: *mut c_void, j0: i32, j1: i32) {
    let b = &*ctx.cast::<Blur>();
    let cw = b.cw;
    let hr = axia_glass_scratch(cw as usize * 9).cast::<i32>();
    if hr.is_null() {
        b.failed.store(true, Ordering::Relaxed);
        return;
    }
    let vrow = hr.offset((cw * 8) as isize).cast::<u32>();
    let vrow8 = vrow.cast::<u8>();
    let hrow = [hr, hr.offset((cw * 4) as isize)];
    let mut hidx = [-1i32, -1i32];
    for j in b.y0 + j0..b.y0 + j1 {
        let row = b.c.px.offset((j * b.c.stride) as isize);
        let v = ((j - b.by0) as f32 + 0.5) * b.inv - 0.5;
        let v = v.clamp(0.0, (b.sh - 1) as f32);
        let ry0 = v as i32;
        let ry1 = (ry0 + 1).min(b.sh - 1);
        let fy = ((v - ry0 as f32) * 256.0) as i32;
        let (mut top, mut bot): (*const i32, *const i32) = (ptr::null(), ptr::null());
        for want in 0..2 {
            let ry = if want == 1 { ry1 } else { ry0 };
            let mut k: i32 = if hidx[0] == ry {
                0
            } else if hidx[1] == ry {
                1
            } else {
                -1
            };
            if k < 0 {
                let keep = if want == 1 {
                    if top == hrow[0] {
                        0
                    } else {
                        1
                    }
                } else if hidx[0] == ry1 {
                    0
                } else if hidx[1] == ry1 {
                    1
                } else {
                    -1
                };
                k = if keep == 0 { 1 } else { 0 };
                // Slices, not raw pointers: the compiler then knows these don't overlap (the
                // rows and vrow share one scratch buffer) and vectorizes. Same arithmetic.
                let br = std::slice::from_raw_parts(b.buf.offset((ry * b.sw) as isize), b.sw as usize);
                let hh = std::slice::from_raw_parts_mut(hrow[k as usize], cw as usize * 4);
                let (c0, c1, fx) = (
                    std::slice::from_raw_parts(b.cx0, cw as usize),
                    std::slice::from_raw_parts(b.cx1, cw as usize),
                    std::slice::from_raw_parts(b.cfx, cw as usize),
                );
                for (i, out) in hh.chunks_exact_mut(4).enumerate() {
                    let (p0, p1, f) = (br[c0[i] as usize], br[c1[i] as usize], fx[i]);
                    for (s, o) in out.iter_mut().enumerate() {
                        let a = ((p0 >> (s * 8)) & 255) as i32;
                        let d = ((p1 >> (s * 8)) & 255) as i32;
                        *o = a * 256 + (d - a) * f;
                    }
                }
                hidx[k as usize] = ry;
            }
            if want == 1 {
                bot = hrow[k as usize];
            } else {
                top = hrow[k as usize];
            }
        }
        {
            let n = cw as usize * 4;
            let (t, bo) = (std::slice::from_raw_parts(top, n), std::slice::from_raw_parts(bot, n));
            let v = std::slice::from_raw_parts_mut(vrow8, n);
            for ((v, &t), &bo) in v.iter_mut().zip(t).zip(bo) {
                *v = (((t << 8) + (bo - t) * fy) >> 16) as u8;
            }
        }
        let corner = (j as f32) < b.y + b.r || j as f32 + 1.0 > b.y + b.h - b.r;
        let m0 = if corner { b.x1 } else { b.in0 };
        let m1 = if corner { b.x1 } else { b.in1 };
        // A corner row computed `coverage` (a square root) for every pixel. Where qx <= 0 and
        // qx <= qy, the middle of the row, it depends on qy alone: one value for the whole span,
        // taken from the first pixel there. Same expressions as `coverage`, so the same bits.
        let qy = (j as f32 + 0.5 - b.cy).abs() - (b.hh - b.r);
        let mut span: Option<f32> = None;
        let mut i = b.x0;
        while i < b.x1 {
            if i == m0 {
                while i < m1 {
                    let p = *vrow.offset((i - b.x0) as isize);
                    let dst = row.offset(i as isize);
                    *dst = if p >> 24 >= 255 {
                        p
                    } else {
                        over(p, *dst, 255)
                    };
                    i += 1;
                }
                if i >= b.x1 {
                    break;
                }
            }
            let a = if corner || (i as f32) < b.x + 1.0 || i as f32 + 1.0 > b.x + b.w - 1.0 {
                let c = if corner {
                    let qx = (i as f32 + 0.5 - b.cx).abs() - (b.hw - b.r);
                    if qx <= 0.0 && qx <= qy {
                        *span.get_or_insert_with(|| coverage(b, i, j))
                    } else {
                        coverage(b, i, j)
                    }
                } else {
                    coverage(b, i, j)
                };
                if c <= 0.0 {
                    i += 1;
                    continue;
                }
                (c * 255.0 + 0.5) as u32
            } else {
                255
            };
            let p = *vrow.offset((i - b.x0) as isize);
            let dst = row.offset(i as isize);
            *dst = if a >= 255 && p >> 24 >= 255 {
                p
            } else {
                over(p, *dst, a)
            };
            i += 1;
        }
    }
}
// Canvas owns stride*height live u32 pixels; clip is within its dimensions. The
// optional pool callback must synchronously run disjoint ranges and join workers.
#[no_mangle]
pub unsafe extern "C" fn axia_glass_render(
    canvas: *mut c_void,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    radius: i32,
    parallel: Option<Parallel>,
) {
    if canvas.is_null()
        || ![x, y, w, h, r].iter().all(|v| v.is_finite())
        || w <= 0.0
        || h <= 0.0
        || !(1..=4096).contains(&radius)
    {
        return;
    }
    let c = *canvas.cast::<Canvas>();
    if c.px.is_null()
        || c.w <= 0
        || c.h <= 0
        || c.stride < c.w
        || c.stride.checked_mul(c.h).is_none()
        || c.clip.x0 < 0
        || c.clip.y0 < 0
        || c.clip.x1 > c.w
        || c.clip.y1 > c.h
        || c.clip.x0 > c.clip.x1
        || c.clip.y0 > c.clip.y1
    {
        return;
    }
    let x0 = (x.floor() as i32).clamp(c.clip.x0, c.clip.x1);
    let y0 = (y.floor() as i32).clamp(c.clip.y0, c.clip.y1);
    let x1 = ((x + w).ceil() as i32).clamp(c.clip.x0, c.clip.x1);
    let y1 = ((y + h).ceil() as i32).clamp(c.clip.y0, c.clip.y1);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let m = radius * 2;
    let bx0 = (x0 - m).max(0);
    let by0 = (y0 - m).max(0);
    let bx1 = (x1.saturating_add(m)).min(c.w);
    let by1 = (y1.saturating_add(m)).min(c.h);
    let f = if radius >= 12 {
        4
    } else if radius >= 6 {
        2
    } else {
        1
    };
    let sw = (bx1 - bx0 + f - 1) / f;
    let sh = (by1 - by0 + f - 1) / f;
    let rad = (radius / (2 * f)).max(1);
    let cw = x1 - x0;
    let mut storage = std::mem::MaybeUninit::<GlassBuffers>::uninit();
    if !axia_glass_prepare(
        sw as usize,
        sh as usize,
        rad as usize,
        cw as usize,
        storage.as_mut_ptr(),
    ) {
        return;
    }
    let s = storage.assume_init();
    let r = r.clamp(0.0, w.min(h) * 0.5);
    let mut b = Blur {
        c,
        x0,
        y0,
        x1,
        bx0,
        by0,
        bx1,
        by1,
        f,
        sw,
        sh,
        rad,
        cw,
        in0: x0,
        in1: x1,
        x,
        y,
        w,
        h,
        r,
        cx: x + w * 0.5,
        cy: y + h * 0.5,
        hw: w * 0.5,
        hh: h * 0.5,
        inv: 1.0 / f as f32,
        buf: s.pixels,
        src: s.source,
        dt: s.division,
        sum: s.sums,
        cx0: s.columns,
        cx1: s.columns.offset(cw as isize),
        cfx: s.columns.offset((cw * 2) as isize),
        failed: AtomicBool::new(false),
        backdrop: ptr::null(),
    };
    phase(
        &b,
        sh,
        (bx1 - bx0) as u64 * (by1 - by0) as u64,
        down,
        parallel,
    );
    if b.failed.load(Ordering::Relaxed) {
        return;
    }
    // The GPU reader runs before workers and only when the CPU image contains
    // holes. Mix the small underlay into blur input, leaving canvas alpha and
    // rounded output untouched. Screenshots use their complete CPU image.
    if std::slice::from_raw_parts(b.buf, (sw * sh) as usize)
        .iter()
        .any(|p| p >> 24 < 255)
    {
        b.backdrop = crate::backdrop::read([bx0, by0, bx1, by1, f, sw, sh]);
        if !b.backdrop.is_null() {
            phase(&b, sh, sw as u64 * sh as u64, backdrop_over, parallel);
        }
    }
    if !axia_glass_reuse() {
        for _ in 0..3 {
            phase(&b, sh, sw as u64 * sh as u64, rows, parallel);
            if b.failed.load(Ordering::Relaxed) {
                return;
            }
            phase(&b, sw, sw as u64 * sh as u64, cols, parallel);
        }
        axia_glass_commit();
    }
    for i in x0..x1 {
        let u = (((i - bx0) as f32 + 0.5) * b.inv - 0.5).clamp(0.0, (sw - 1) as f32);
        let a = u as i32;
        *b.cx0.offset((i - x0) as isize) = a;
        *b.cx1.offset((i - x0) as isize) = (a + 1).min(sw - 1);
        *b.cfx.offset((i - x0) as isize) = ((u - a as f32) * 256.0) as i32;
    }
    while b.in0 < x1 && (b.in0 as f32) < x + 1.0 {
        b.in0 += 1;
    }
    while b.in1 > b.in0 && b.in1 as f32 - 1.0 + 1.0 > x + w - 1.0 {
        b.in1 -= 1;
    }
    if b.in0 >= b.in1 {
        b.in0 = x1;
        b.in1 = x1;
    }
    phase(&b, y1 - y0, cw as u64 * (y1 - y0) as u64, up, parallel);
}
