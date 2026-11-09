//! Premultiplied ARGB pixels and the renderer's original sampling arithmetic.
use super::memory;

const MAX_PIXELS: usize = 64 * 1024 * 1024;
pub(super) fn count(w: i32, h: i32) -> Option<usize> {
    let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
    let count = w.checked_mul(h)?;
    (w != 0 && h != 0 && w <= 32768 && h <= 32768 && count <= MAX_PIXELS).then_some(count)
}

#[derive(Default)]
pub(super) struct Pixels {
    pub w: i32,
    pub h: i32,
    pub data: Vec<u32>,
    pub opaque: bool,
}
impl Pixels {
    pub fn empty(w: i32, h: i32) -> Option<Self> {
        let n = count(w, h)?;
        let mut data = Vec::new();
        if !memory::try_reserve_exact(&mut data, n) {
            return None;
        }
        data.resize(n, 0);
        Some(Self {
            w,
            h,
            data,
            opaque: false,
        })
    }
    pub fn rgba(&mut self, rgba: &[u8]) {
        debug_assert_eq!(rgba.len(), self.data.len() * 4);
        self.opaque = true;
        for (out, c) in self.data.iter_mut().zip(rgba.as_chunks::<4>().0) {
            let [r, g, b, a] = [c[0], c[1], c[2], c[3]].map(u32::from);
            self.opaque &= a == 255;
            *out = a << 24
                | ((r * a + 127) / 255) << 16
                | ((g * a + 127) / 255) << 8
                | ((b * a + 127) / 255);
        }
    }
    pub fn argb(&mut self, argb: &[u32]) {
        self.data.copy_from_slice(argb);
        self.opaque = argb.iter().all(|&p| p >> 24 == 255);
    }
    fn bilinear(&self, u: f32, v: f32) -> u32 {
        let (u, v) = (
            u.max(0.).min((self.w - 1) as f32),
            v.max(0.).min((self.h - 1) as f32),
        );
        let (x0, y0) = (u as usize, v as usize);
        let (x1, y1) = (
            (x0 + 1).min(self.w as usize - 1),
            (y0 + 1).min(self.h as usize - 1),
        );
        let (fx, fy) = (u - x0 as f32, v - y0 as f32);
        let at = |x, y| self.data[y * self.w as usize + x];
        let p = [at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1)];
        let weights = [
            (1. - fx) * (1. - fy),
            fx * (1. - fy),
            (1. - fx) * fy,
            fx * fy,
        ];
        let mut out = 0;
        for shift in (0..32).step_by(8) {
            let mut acc = 0.;
            for k in 0..4 {
                acc += ((p[k] >> shift) & 255) as f32 * weights[k];
            }
            out |= ((acc + 0.5) as u32) << shift;
        }
        out
    }
    fn area(&self, (x0, x1): (usize, usize), (y0, y1): (usize, usize)) -> u32 {
        // A source up to 64 megapixels can overflow the old u32 accumulator.
        let (mut acc, mut n) = ([0u64; 4], 0u64);
        for y in y0..y1 {
            for &p in &self.data[y * self.w as usize + x0..y * self.w as usize + x1] {
                for (k, channel) in acc.iter_mut().enumerate() {
                    *channel += u64::from((p >> (k * 8)) & 255);
                }
                n += 1;
            }
        }
        if n == 0 {
            return 0;
        }
        acc.into_iter()
            .enumerate()
            .fold(0, |out, (k, a)| out | ((a / n) as u32) << (k * 8))
    }
    /// Reuse an evicted buffer when it is not grossly oversized. Allocation
    /// failure leaves its contents/dimensions unchanged, so it may be restored.
    pub fn rescale(&self, mut dst: Self, w: i32, h: i32, fit: i32) -> Result<Self, Self> {
        let Some(n) = count(w, h) else {
            return Err(dst);
        };
        let additional = n.saturating_sub(dst.data.len());
        if !memory::try_reserve_exact(&mut dst.data, additional) {
            return Err(dst);
        }
        // Only initialize new elements once. Existing samples are overwritten
        // below, including the transparent margins of object-fit: contain.
        dst.data.resize(n, 0);
        dst.w = w;
        dst.h = h;
        dst.opaque = self.opaque && (fit == 0 || fit == 1);
        let (mut sx, mut sy) = (w as f32 / self.w as f32, h as f32 / self.h as f32);
        let (mut ox, mut oy) = (0., 0.);
        if fit != 0 {
            let scale = if fit == 1 { sx.max(sy) } else { sx.min(sy) };
            sx = scale;
            sy = scale;
            ox = (w as f32 - self.w as f32 * scale) / 2.;
            oy = (h as f32 - self.h as f32 * scale) / 2.;
        }
        let area = sx < 0.75 || sy < 0.75;
        let bounds = |a: f32, b: f32, limit: i32| {
            (
                a.floor().max(0.) as usize,
                b.ceil().min(limit as f32) as usize,
            )
        };
        let mut previous_y = None;
        for j in 0..h {
            let row = j as usize * w as usize;
            let (v0, v1) = ((j as f32 - oy) / sy, ((j + 1) as f32 - oy) / sy);
            if v1 <= 0. || v0 >= self.h as f32 {
                dst.data[row..row + w as usize].fill(0);
                continue;
            }
            let y_bounds = bounds(v0, v1, self.h);
            if area && previous_y == Some(y_bounds) {
                // An enlarged axis can repeat an identical source footprint
                // thousands of times. Reuse its exact integer average.
                dst.data.copy_within(row - w as usize..row, row);
                continue;
            }
            previous_y = Some(y_bounds);
            let mut previous_x = None;
            let mut sample = 0;
            for i in 0..w {
                let (u0, u1) = ((i as f32 - ox) / sx, ((i + 1) as f32 - ox) / sx);
                let out = row + i as usize;
                if u1 <= 0. || u0 >= self.w as f32 {
                    dst.data[out] = 0;
                    continue;
                }
                dst.data[out] = if area {
                    let x_bounds = bounds(u0, u1, self.w);
                    if previous_x != Some(x_bounds) {
                        sample = self.area(x_bounds, y_bounds);
                        previous_x = Some(x_bounds);
                    }
                    sample
                } else {
                    self.bilinear((u0 + u1) / 2. - 0.5, (v0 + v1) / 2. - 0.5)
                };
            }
        }
        Ok(dst)
    }
    pub fn bytes(&self) -> u64 {
        (self.data.capacity() as u64).saturating_mul(4)
    }
}
