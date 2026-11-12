//! Owned, immutable cursor images rasterized from signed-distance primitives.
//! Pixel references in tests were recorded from the removed C renderer.
use std::sync::OnceLock;
const COUNT: usize = 23;
pub(crate) const CSS_NAMES: [&str; COUNT] = [
    "auto",
    "default",
    "pointer",
    "text",
    "move",
    "grab",
    "grabbing",
    "not-allowed",
    "wait",
    "progress",
    "crosshair",
    "n-resize",
    "s-resize",
    "e-resize",
    "w-resize",
    "ne-resize",
    "nw-resize",
    "se-resize",
    "sw-resize",
    "ew-resize",
    "ns-resize",
    "nesw-resize",
    "nwse-resize",
];
pub(crate) const MOVE: i32 = 4;
#[repr(C)]
pub struct CursorImage {
    px: *const u32,
    w: i32,
    h: i32,
    hx: i32,
    hy: i32,
}
struct Cached {
    image: CursorImage,
    _pixels: Box<[u32]>,
}
// The raw view points only into _pixels, which is immutable and kept alive in
// the static OnceLock. Moving Cached never moves its heap pixel storage.
unsafe impl Send for Cached {}
unsafe impl Sync for Cached {}
static CACHE: [[OnceLock<Cached>; COUNT]; 5] = [const { [const { OnceLock::new() }; COUNT] }; 5];
enum Primitive {
    Polygon(Vec<[f32; 2]>),
    Capsule {
        a: [f32; 2],
        b: [f32; 2],
        radius: f32,
    },
    Ring {
        center: [f32; 2],
        radius: f32,
        half: f32,
    },
}
#[derive(Default)]
struct Shape {
    primitives: Vec<Primitive>,
    hotspot: [f32; 2],
}
fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let vx = b[0] - a[0];
    let vy = b[1] - a[1];
    let wx = p[0] - a[0];
    let wy = p[1] - a[1];
    let len = vx * vx + vy * vy;
    let mut t = if len > 0.0 {
        (wx * vx + wy * vy) / len
    } else {
        0.0
    };
    t = if t < 0.0 {
        0.0
    } else if t > 1.0 {
        1.0
    } else {
        t
    };
    let dx = wx - vx * t;
    let dy = wy - vy * t;
    (dx * dx + dy * dy).sqrt()
}
impl Primitive {
    fn distance(&self, x: f32, y: f32) -> f32 {
        match self {
            Self::Capsule { a, b, radius } => segment_distance([x, y], *a, *b) - radius,
            Self::Ring {
                center,
                radius,
                half,
            } => ((x - center[0]).hypot(y - center[1]) - radius).abs() - half,
            Self::Polygon(points) => {
                let mut distance = 1e9f32;
                let mut inside = false;
                let mut previous = points.len() - 1;
                for current in 0..points.len() {
                    let [ax, ay] = points[previous];
                    let [bx, by] = points[current];
                    let value = segment_distance([x, y], [ax, ay], [bx, by]);
                    if value < distance {
                        distance = value;
                    }
                    if (by > y) != (ay > y) && x < (ax - bx) * (y - by) / (ay - by) + bx {
                        inside = !inside;
                    }
                    previous = current;
                }
                if inside {
                    -distance
                } else {
                    distance
                }
            }
        }
    }
}
impl Shape {
    fn polygon(&mut self, points: &[[f32; 2]]) {
        self.primitives.push(Primitive::Polygon(points.to_vec()));
    }
    fn capsule(&mut self, ax: f32, ay: f32, bx: f32, by: f32, radius: f32) {
        self.primitives.push(Primitive::Capsule {
            a: [ax, ay],
            b: [bx, by],
            radius,
        });
    }
    fn ring(&mut self, x: f32, y: f32, radius: f32, half: f32) {
        self.primitives.push(Primitive::Ring {
            center: [x, y],
            radius,
            half,
        });
    }
    fn distance(&self, x: f32, y: f32) -> f32 {
        let mut distance = 1e9f32;
        for primitive in &self.primitives {
            let value = primitive.distance(x, y);
            if value < distance {
                distance = value;
            }
        }
        distance
    }
    fn double_arrow(&mut self, degrees: f32) {
        let angle = degrees * std::f32::consts::PI / 180.0;
        let c = angle.cos();
        let s = angle.sin();
        for triangle in [
            [[-10.0, 0.0], [-4.5, -5.0], [-4.5, 5.0]],
            [[10.0, 0.0], [4.5, -5.0], [4.5, 5.0]],
        ] {
            let points = triangle.map(|[x, y]| [16.0 + x * c - y * s, 16.0 + x * s + y * c]);
            self.polygon(&points);
        }
        self.capsule(
            16.0 - 6.0 * c,
            16.0 - 6.0 * s,
            16.0 + 6.0 * c,
            16.0 + 6.0 * s,
            1.4,
        );
        self.hotspot = [16.0, 16.0];
    }
    fn arrow(&mut self, x: f32, y: f32) {
        let points = [
            [0.0, 0.0],
            [0.0, 17.0],
            [4.0, 13.4],
            [7.0, 20.0],
            [9.6, 18.9],
            [6.7, 12.5],
            [12.0, 12.5],
        ]
        .map(|[a, b]| [a + x, b + y]);
        self.polygon(&points);
        self.hotspot = [x, y];
    }
    fn hand(&mut self, kind: i32) {
        self.polygon(&[
            [8.0, 15.0],
            [21.0, 15.0],
            [22.0, 22.0],
            [19.0, 28.0],
            [11.0, 28.0],
            [7.5, 21.0],
        ]);
        let fingers = match kind {
            0 => [
                [10.0, 5.0, 10.0, 17.0, 1.9],
                [13.6, 12.0, 13.6, 17.0, 1.8],
                [17.0, 12.6, 17.0, 17.0, 1.8],
                [20.2, 13.6, 20.2, 18.0, 1.7],
                [4.6, 16.5, 8.5, 21.0, 1.8],
            ],
            1 => [
                [10.0, 8.0, 10.0, 17.0, 1.8],
                [13.6, 6.0, 13.6, 17.0, 1.8],
                [17.0, 6.6, 17.0, 17.0, 1.8],
                [20.2, 9.0, 20.2, 18.0, 1.7],
                [4.0, 14.5, 8.5, 21.0, 1.8],
            ],
            _ => [
                [10.0, 13.2, 10.0, 17.0, 1.8],
                [13.6, 12.4, 13.6, 17.0, 1.8],
                [17.0, 12.8, 17.0, 17.0, 1.8],
                [20.2, 13.8, 20.2, 18.0, 1.7],
                [6.0, 17.0, 8.5, 21.0, 1.8],
            ],
        };
        for [ax, ay, bx, by, radius] in fingers {
            self.capsule(ax, ay, bx, by, radius);
        }
        self.hotspot = match kind {
            0 => [10.0, 3.0],
            1 => [14.0, 14.0],
            _ => [14.0, 16.0],
        };
    }
    fn build(id: i32) -> Self {
        let mut shape = Self::default();
        match id {
            2 => shape.hand(0),
            5 => shape.hand(1),
            6 => shape.hand(2),
            3 => {
                for p in [
                    [16.0, 7.5, 16.0, 24.5, 0.9],
                    [12.5, 6.5, 15.0, 7.6, 0.9],
                    [17.0, 7.6, 19.5, 6.5, 0.9],
                    [12.5, 25.5, 15.0, 24.4, 0.9],
                    [17.0, 24.4, 19.5, 25.5, 0.9],
                ] {
                    shape.capsule(p[0], p[1], p[2], p[3], p[4]);
                }
                shape.hotspot = [16.0, 16.0];
            }
            10 => {
                for [ax, ay, bx, by] in [
                    [16.0, 5.0, 16.0, 13.0],
                    [16.0, 19.0, 16.0, 27.0],
                    [5.0, 16.0, 13.0, 16.0],
                    [19.0, 16.0, 27.0, 16.0],
                ] {
                    shape.capsule(ax, ay, bx, by, 0.9);
                }
                shape.hotspot = [16.0, 16.0];
            }
            4 => {
                shape.capsule(16.0, 8.0, 16.0, 24.0, 1.3);
                shape.capsule(8.0, 16.0, 24.0, 16.0, 1.3);
                for triangle in [
                    [[16.0, 3.0], [11.5, 8.5], [20.5, 8.5]],
                    [[16.0, 29.0], [11.5, 23.5], [20.5, 23.5]],
                    [[3.0, 16.0], [8.5, 11.5], [8.5, 20.5]],
                    [[29.0, 16.0], [23.5, 11.5], [23.5, 20.5]],
                ] {
                    shape.polygon(&triangle);
                }
                shape.hotspot = [16.0, 16.0];
            }
            7 => {
                shape.ring(16.0, 16.0, 8.5, 1.7);
                shape.capsule(10.2, 10.2, 21.8, 21.8, 1.7);
                shape.hotspot = [16.0, 16.0];
            }
            8 => {
                shape.ring(16.0, 16.0, 7.5, 2.2);
                shape.capsule(16.0, 16.0, 16.0, 11.0, 1.1);
                shape.capsule(16.0, 16.0, 19.5, 16.0, 1.1);
                shape.hotspot = [16.0, 16.0];
            }
            9 => {
                shape.arrow(3.0, 3.0);
                shape.ring(22.0, 23.0, 4.5, 1.4);
            }
            13 | 14 | 19 => shape.double_arrow(0.0),
            11 | 12 | 20 => shape.double_arrow(90.0),
            16 | 17 | 22 => shape.double_arrow(45.0),
            15 | 18 | 21 => shape.double_arrow(-45.0),
            _ => shape.arrow(3.0, 3.0),
        }
        shape
    }
}
fn pack(mut alpha: f32, luminance: f32) -> u32 {
    if alpha <= 0.0 {
        return 0;
    }
    if alpha > 1.0 {
        alpha = 1.0;
    }
    let a = (alpha * 255.0 + 0.5) as u32;
    let c = ((luminance * alpha * 255.0 + 0.5) as u32).min(a);
    a << 24 | c << 16 | c << 8 | c
}
fn rasterize(id: i32, pct: i32) -> Cached {
    let scale = pct as f32 / 100.0;
    let shape = Shape::build(id);
    let size = (32.0 * scale + 0.5) as i32;
    let mut pixels = vec![0u32; (size * size) as usize];
    let inverse = 1.0 / scale;
    for y in 0..size {
        for x in 0..size {
            let (mut alpha, mut luminance) = (0.0f32, 0.0f32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let ux = (x as f32 + (sx as f32 + 0.5) / 4.0) * inverse;
                    let uy = (y as f32 + (sy as f32 + 0.5) / 4.0) * inverse;
                    let distance = shape.distance(ux, uy);
                    if distance < 0.0 {
                        alpha += 1.0;
                    } else if distance < 1.25 {
                        alpha += 1.0;
                        luminance += 1.0;
                    } else {
                        let shadow = shape.distance(ux - 0.4, uy - 1.0) - 1.25;
                        if shadow < 2.5 {
                            alpha += 0.28 * (1.0 - if shadow < 0.0 { 0.0 } else { shadow } / 2.5);
                        }
                    }
                }
            }
            alpha /= 16.0;
            luminance /= 16.0;
            pixels[(y * size + x) as usize] =
                pack(alpha, if alpha > 0.0 { luminance / alpha } else { 0.0 });
        }
    }
    let pixels = pixels.into_boxed_slice();
    let image = CursorImage {
        px: pixels.as_ptr(),
        w: size,
        h: size,
        hx: (shape.hotspot[0] * scale + 0.5) as i32,
        hy: (shape.hotspot[1] * scale + 0.5) as i32,
    };
    Cached {
        image,
        _pixels: pixels,
    }
}
#[no_mangle]
pub extern "C" fn cursor_image_pct(mut shape: i32, pct: i32) -> *const CursorImage {
    if shape <= 0 || shape >= COUNT as i32 {
        shape = 1;
    }
    let pct = (pct.clamp(100, 200) + 12) / 25 * 25;
    &CACHE[((pct - 100) / 25) as usize][shape as usize]
        .get_or_init(|| rasterize(shape, pct))
        .image
}
#[no_mangle]
pub extern "C" fn cursor_image(shape: i32, scale: i32) -> *const CursorImage {
    cursor_image_pct(shape, if scale >= 2 { 200 } else { 100 })
}
#[no_mangle]
pub extern "C" fn cursor_scale_for(width: i32, height: i32) -> i32 {
    if width >= 2560 && height >= 1440 {
        2
    } else {
        1
    }
}
#[no_mangle]
pub extern "C" fn cursor_for_edges(edges: i32) -> i32 {
    match edges & 15 {
        1 => 11,
        2 => 12,
        4 => 14,
        8 => 13,
        5 => 16,
        9 => 15,
        6 => 18,
        10 => 17,
        _ => MOVE,
    }
}
#[no_mangle]
pub extern "C" fn cursor_from_wayland_shape(shape: u32) -> i32 {
    const MAP: [i32; 35] = [
        0, 1, 1, 1, 2, 9, 8, 10, 10, 3, 3, 1, 1, 4, 7, 7, 5, 6, 13, 11, 15, 16, 12, 17, 18, 14, 19,
        20, 21, 22, 19, 20, 4, 1, 1,
    ];
    MAP.get(shape as usize).copied().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_shapes_and_scales_match_frozen_c_pixels() {
        for line in include_str!("../tests/data/cursor_legacy.csv").lines() {
            let parts: Vec<_> = line.split(',').collect();
            let values: Vec<i32> = parts[..6]
                .iter()
                .map(|value| value.parse().unwrap())
                .collect();
            let expected = u64::from_str_radix(parts[6], 16).unwrap();
            let image = unsafe { &*cursor_image_pct(values[0], values[1]) };
            assert_eq!([image.w, image.h, image.hx, image.hy], values[2..6]);
            let pixels =
                unsafe { std::slice::from_raw_parts(image.px, (image.w * image.h) as usize) };
            let actual = pixels.iter().fold(1469598103934665603u64, |hash, &pixel| {
                (hash ^ pixel as u64).wrapping_mul(1099511628211)
            });
            assert_eq!(actual, expected, "shape={} scale={}", values[0], values[1]);
        }
    }
    #[test]
    fn normalization_cache_lifetime_and_premultiplication() {
        assert_eq!(cursor_image_pct(-1, -1), cursor_image_pct(1, 100));
        assert_eq!(cursor_image_pct(999, 999), cursor_image_pct(1, 200));
        assert_eq!(cursor_image_pct(2, 112), cursor_image_pct(2, 100));
        assert_eq!(cursor_image_pct(2, 113), cursor_image_pct(2, 125));
        let image = cursor_image(3, 2);
        let threads: Vec<_> = (0..4)
            .map(|_| std::thread::spawn(|| cursor_image_pct(3, 200) as usize))
            .collect();
        for thread in threads {
            assert_eq!(thread.join().unwrap(), image as usize);
        }
        let image = unsafe { &*image };
        for &pixel in unsafe { std::slice::from_raw_parts(image.px, (image.w * image.h) as usize) }
        {
            let alpha = pixel >> 24;
            assert!(
                (pixel & 255) <= alpha
                    && ((pixel >> 8) & 255) <= alpha
                    && ((pixel >> 16) & 255) <= alpha
            );
        }
    }
}
