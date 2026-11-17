//! Viewport-only rasterization. Coordinate tables are O(viewport width+height),
//! mipmaps bound minification and packed integer interpolation filters RGBA.
use super::{
    decode::{Level, Photo},
    model::View,
};
use std::sync::atomic::{AtomicU64, Ordering};
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}
#[derive(Clone, Copy)]
struct Axis {
    a: usize,
    b: usize,
    f: u64,
}
fn axis(position: f64, size: u32) -> Axis {
    let p = position.clamp(0., size.saturating_sub(1) as f64);
    let a = p as usize;
    Axis {
        a,
        b: (a + 1).min(size as usize - 1),
        f: ((p - a as f64) * 256.) as u64,
    }
}
// Two channels in 32-bit lanes. 8-bit channels times 16-bit weights cannot
// carry into the neighboring lane. Round only once after both dimensions.
#[inline(always)]
fn pair(p: u32, shift: u32) -> u64 {
    let p = p >> shift;
    (p & 255) as u64 | (((p >> 16) & 255) as u64) << 32
}
#[inline(always)]
fn sample(l: &Level, x: Axis, y: Axis, quality: bool) -> u32 {
    let stride = l.width as usize;
    let p0 = l.pixels[y.a * stride + x.a];
    if !quality {
        return p0;
    }
    let p1 = l.pixels[y.a * stride + x.b];
    let p2 = l.pixels[y.b * stride + x.a];
    let p3 = l.pixels[y.b * stride + x.b];
    let w = [
        (256 - x.f) * (256 - y.f),
        x.f * (256 - y.f),
        (256 - x.f) * y.f,
        x.f * y.f,
    ];
    let mix = |shift| {
        let v = pair(p0, shift) * w[0]
            + pair(p1, shift) * w[1]
            + pair(p2, shift) * w[2]
            + pair(p3, shift) * w[3]
            + 0x0000800000008000;
        (v >> 16 & 255) as u32 | ((v >> 48 & 255) as u32) << 16
    };
    mix(0) | mix(8) << 8
}
pub fn render(
    photo: &Photo,
    view: View,
    frame: usize,
    serial: u64,
    current: &AtomicU64,
) -> Option<Raster> {
    if photo.width == 0 || photo.height == 0 || photo.frames.is_empty() {
        return None;
    }
    let (w, h) = (view.width.clamp(1, 4096), view.height.clamp(1, 4096));
    if w as usize * h as usize > 8_500_000 {
        return None;
    }
    let scale = view.scale(photo.width, photo.height);
    if !scale.is_finite() || scale <= 0. {
        return None;
    }
    let f = &photo.frames[frame % photo.frames.len()];
    let level = if scale < 1. {
        ((-scale.log2()).floor() as usize).min(f.levels.len() - 1)
    } else {
        0
    };
    let src = &f.levels[level];
    let factor = (1u32 << level) as f64;
    let (ox, oy) = view.origin(photo.width, photo.height);
    let (rw, rh) = view.size(photo.width, photo.height);
    let rotation = view.rotation % 4;
    let swap = rotation % 2 == 1;
    let xs: Vec<Axis> = (0..w)
        .map(|x| {
            let p = (x as f64 + 0.5 - ox) / scale - 0.5;
            if swap {
                let p = if rotation == 1 {
                    photo.height as f64 - 1. - p
                } else {
                    p
                };
                axis((p + 0.5) / factor - 0.5, src.height)
            } else {
                let p = if rotation == 2 {
                    photo.width as f64 - 1. - p
                } else {
                    p
                };
                axis((p + 0.5) / factor - 0.5, src.width)
            }
        })
        .collect();
    let ys: Vec<Axis> = (0..h)
        .map(|y| {
            let p = (y as f64 + 0.5 - oy) / scale - 0.5;
            if swap {
                let p = if rotation == 3 {
                    photo.width as f64 - 1. - p
                } else {
                    p
                };
                axis((p + 0.5) / factor - 0.5, src.width)
            } else {
                let p = if rotation == 2 {
                    photo.height as f64 - 1. - p
                } else {
                    p
                };
                axis((p + 0.5) / factor - 0.5, src.height)
            }
        })
        .collect();
    let table = super::model::tone(view.brightness, view.contrast);
    let tone = view.brightness != 0 || view.contrast != 0;
    let mut pixels = vec![0xff141416; w as usize * h as usize];
    let x0 = (ox - 0.5).ceil().clamp(0., w as f64) as usize;
    let x1 = (ox + rw as f64 * scale - 0.5).ceil().clamp(0., w as f64) as usize;
    for y in 0..h as usize {
        if y % 8 == 0 && current.load(Ordering::Relaxed) != serial {
            return None;
        }
        let iy = (y as f64 + 0.5 - oy) / scale - 0.5;
        if iy < -0.5 || iy >= rh as f64 - 0.5 {
            continue;
        }
        let row = &mut pixels[y * w as usize..(y + 1) * w as usize];
        if f.opaque && !tone && !view.quality {
            if swap {
                for (out, x) in row[x0..x1].iter_mut().zip(&xs[x0..x1]) {
                    *out = src.pixels[x.a * src.width as usize + ys[y].a];
                }
            } else {
                let source =
                    &src.pixels[ys[y].a * src.width as usize..(ys[y].a + 1) * src.width as usize];
                for (out, x) in row[x0..x1].iter_mut().zip(&xs[x0..x1]) {
                    *out = source[x.a];
                }
            }
            continue;
        }
        for x in x0..x1 {
            let (sx, sy) = if swap { (ys[y], xs[x]) } else { (xs[x], ys[y]) };
            let mut p = sample(src, sx, sy, view.quality);
            if f.opaque {
                row[x] = if tone {
                    0xff000000
                        | (table[(p >> 16 & 255) as usize] as u32) << 16
                        | (table[(p >> 8 & 255) as usize] as u32) << 8
                        | table[(p & 255) as usize] as u32
                } else {
                    p
                };
                continue;
            }
            let a = p >> 24;
            if tone && a > 0 {
                let mut q = a << 24;
                for sh in [0, 8, 16] {
                    let c = ((p >> sh & 255) * 255 / a).min(255);
                    q |= ((table[c as usize] as u32 * a + 127) / 255) << sh
                }
                p = q
            }
            let inv = 255 - a;
            let bg = 0xff141416u32;
            let mut out = 0xff000000;
            for sh in [0, 8, 16] {
                out |= ((p >> sh & 255) + ((bg >> sh & 255) * inv + 127) / 255).min(255) << sh
            }
            row[x] = out;
        }
    }
    Some(Raster {
        width: w as usize,
        height: h as usize,
        pixels,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_interpolation_matches_channel_reference() {
        let l = Level {
            width: 2,
            height: 2,
            pixels: vec![0x00112233, 0x44556677, 0x8899aabb, 0xccddeeff],
        };
        for fx in (0..=256).step_by(7) {
            for fy in (0..=256).step_by(9) {
                let x = Axis { a: 0, b: 1, f: fx };
                let y = Axis { a: 0, b: 1, f: fy };
                let p = sample(&l, x, y, true);
                let mut expected = 0u32;
                for sh in [0, 8, 16, 24] {
                    let a = ((l.pixels[0] >> sh & 255) as u64 * (256 - fx)
                        + (l.pixels[1] >> sh & 255) as u64 * fx)
                        * (256 - fy);
                    let b = ((l.pixels[2] >> sh & 255) as u64 * (256 - fx)
                        + (l.pixels[3] >> sh & 255) as u64 * fx)
                        * fy;
                    expected |= (((a + b + 32768) >> 16) as u32) << sh;
                }
                assert_eq!(p, expected)
            }
        }
    }
}
