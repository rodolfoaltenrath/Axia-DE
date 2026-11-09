//! Affine math shared by Rust layout and the remaining C paint/hit-testing code.
use super::types::{Affine, Dim, Rect, Style};

impl Affine {
    pub fn identity() -> Self {
        Self {
            a: 1.,
            d: 1.,
            ..Self::default()
        }
    }
    pub fn compose(self, n: Self) -> Self {
        let m = self;
        Self {
            a: m.a * n.a + m.c * n.b,
            b: m.b * n.a + m.d * n.b,
            c: m.a * n.c + m.c * n.d,
            d: m.b * n.c + m.d * n.d,
            e: m.a * n.e + m.c * n.f + m.e,
            f: m.b * n.e + m.d * n.f + m.f,
        }
    }
    pub fn inverse(self) -> Option<Self> {
        let m = self;
        let det = m.a * m.d - m.b * m.c;
        if det.abs() < 1e-8 {
            return None;
        }
        let i = 1. / det;
        Some(Self {
            a: m.d * i,
            b: -m.b * i,
            c: -m.c * i,
            d: m.a * i,
            e: (m.c * m.f - m.d * m.e) * i,
            f: (m.b * m.e - m.a * m.f) * i,
        })
    }
    pub fn point(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
    pub fn bounds(self, b: Rect) -> Rect {
        let (x, y) = self.point(b.x0, b.y0);
        let mut r = Rect {
            x0: x,
            y0: y,
            x1: x,
            y1: y,
        };
        for (x, y) in [(b.x1, b.y0), (b.x1, b.y1), (b.x0, b.y1)] {
            let (x, y) = self.point(x, y);
            r.x0 = r.x0.min(x);
            r.y0 = r.y0.min(y);
            r.x1 = r.x1.max(x);
            r.y1 = r.y1.max(y);
        }
        r
    }
}
fn origin(d: Dim, size: f32) -> f32 {
    match d.unit {
        2 => size * d.v / 100.,
        1 => d.v,
        _ => size / 2.,
    }
}
impl Style {
    pub fn has_transform(&self) -> bool {
        // Extensions are live owner allocations throughout this synchronous call.
        unsafe { self.ext.as_ref() }.is_some_and(|x| {
            x.scale_x != 1. || x.scale_y != 1. || (x.rotate != 0. && x.rotate % 360. != 0.)
        })
    }
    pub fn transform(&self, x: f32, y: f32, w: f32, h: f32) -> Affine {
        let Some(s) = (unsafe { self.ext.as_ref() }) else {
            return Affine::identity();
        };
        let ox = x + origin(s.origin_x, w);
        let oy = y + origin(s.origin_y, h);
        // Preserve the renderer's f32 operation order (no fused multiply-add).
        let r = s.rotate * std::f32::consts::PI / 180.;
        let co = r.cos();
        let si = r.sin();
        let mut m = Affine {
            a: co * s.scale_x,
            b: si * s.scale_x,
            c: -si * s.scale_y,
            d: co * s.scale_y,
            e: 0.,
            f: 0.,
        };
        m.e = ox - (m.a * ox + m.c * oy);
        m.f = oy - (m.b * ox + m.d * oy);
        m
    }
}
#[no_mangle]
pub extern "C" fn affine_identity() -> Affine {
    Affine::identity()
}
#[no_mangle]
pub extern "C" fn affine_mul(m: Affine, n: Affine) -> Affine {
    m.compose(n)
}
#[no_mangle]
pub unsafe extern "C" fn affine_invert(m: Affine, out: *mut Affine) -> i32 {
    match m.inverse() {
        Some(value) if !out.is_null() => {
            out.write(value);
            1
        }
        _ => 0,
    }
}
#[no_mangle]
pub unsafe extern "C" fn affine_apply(m: Affine, x: f32, y: f32, ox: *mut f32, oy: *mut f32) {
    let (x, y) = m.point(x, y);
    if !ox.is_null() {
        ox.write(x);
    }
    if !oy.is_null() {
        oy.write(y);
    }
}
#[no_mangle]
pub extern "C" fn affine_box(m: Affine, b: Rect) -> Rect {
    m.bounds(b)
}
#[no_mangle]
pub extern "C" fn style_rotates(deg: f32) -> i32 {
    i32::from(deg % 360. != 0.)
}
#[no_mangle]
pub unsafe extern "C" fn style_has_transform(s: *const Style) -> i32 {
    i32::from(s.as_ref().is_some_and(Style::has_transform))
}
#[no_mangle]
pub unsafe extern "C" fn style_transform(
    s: *const Style,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> Affine {
    s.as_ref()
        .map_or_else(Affine::identity, |s| s.transform(x, y, w, h))
}
