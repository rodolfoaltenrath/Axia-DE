//! Antialiased rasters for native views: line icons and simple vector artwork.
//! Shapes are signed-distance tests per pixel; colours are straight RGBA.
pub type Rgba = [u8; 4];
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}
#[derive(Clone, Debug)]
pub enum Shape {
    /// Segment with round ends: from, to, width.
    Line((f64, f64), (f64, f64), f64),
    /// Circle outline: centre, radius, stroke.
    Circle((f64, f64), f64, f64),
    Dot((f64, f64), f64),
    /// Rounded rectangle outline: x, y, w, h, corner, stroke.
    Rect(f64, f64, f64, f64, f64, f64),
    FillRect(f64, f64, f64, f64, f64),
    Polygon(Vec<(f64, f64)>),
}
fn segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let len = vx * vx + vy * vy;
    let t = if len > 0. {
        (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / len).clamp(0., 1.)
    } else {
        0.
    };
    (p.0 - a.0 - vx * t).hypot(p.1 - a.1 - vy * t)
}
fn round_box(p: (f64, f64), x: f64, y: f64, w: f64, h: f64, r: f64) -> f64 {
    let r = r.min(w / 2.).min(h / 2.).max(0.);
    let qx = (p.0 - x - w / 2.).abs() - (w / 2. - r);
    let qy = (p.1 - y - h / 2.).abs() - (h / 2. - r);
    qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - r
}
fn polygon(p: (f64, f64), pts: &[(f64, f64)]) -> f64 {
    let mut d = f64::MAX;
    let mut inside = false;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        d = d.min(segment(p, a, b));
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    if inside {
        -d
    } else {
        d
    }
}
impl Shape {
    /// Signed distance (negative inside the painted area), in the shape's units.
    pub fn distance(&self, p: (f64, f64)) -> f64 {
        match self {
            Shape::Line(a, b, w) => segment(p, *a, *b) - w / 2.,
            Shape::Circle(c, r, w) => ((p.0 - c.0).hypot(p.1 - c.1) - r).abs() - w / 2.,
            Shape::Dot(c, r) => (p.0 - c.0).hypot(p.1 - c.1) - r,
            Shape::Rect(x, y, w, h, r, s) => round_box(p, *x, *y, *w, *h, *r).abs() - s / 2.,
            Shape::FillRect(x, y, w, h, r) => round_box(p, *x, *y, *w, *h, *r),
            Shape::Polygon(pts) => polygon(p, pts),
        }
    }
    /// Bounding box with a margin: (x0, y0, x1, y1).
    fn bounds(&self) -> (f64, f64, f64, f64) {
        let pad = |(x0, y0, x1, y1): (f64, f64, f64, f64), m: f64| (x0 - m, y0 - m, x1 + m, y1 + m);
        match self {
            Shape::Line(a, b, w) => pad(
                (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)),
                w / 2. + 1.,
            ),
            Shape::Circle(c, r, w) => pad((c.0, c.1, c.0, c.1), r + w / 2. + 1.),
            Shape::Dot(c, r) => pad((c.0, c.1, c.0, c.1), r + 1.),
            Shape::Rect(x, y, w, h, _, s) => pad((*x, *y, x + w, y + h), s / 2. + 1.),
            Shape::FillRect(x, y, w, h, _) => pad((*x, *y, x + w, y + h), 1.),
            Shape::Polygon(pts) => pad(
                pts.iter()
                    .fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, p| {
                        (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1))
                    }),
                1.,
            ),
        }
    }
}
fn cover(d: f64) -> f64 {
    (0.5 - d).clamp(0., 1.)
}
impl Image {
    pub fn new(width: usize, height: usize) -> Self {
        let (width, height) = (width.clamp(1, 4096), height.clamp(1, 4096));
        Self {
            width,
            height,
            data: vec![0; width * height * 4],
        }
    }
    pub fn fill(&mut self, c: Rgba) {
        for px in self.data.chunks_exact_mut(4) {
            px.copy_from_slice(&c);
        }
    }
    /// Straight-alpha "over" with a coverage in 0..1.
    pub fn blend(&mut self, x: usize, y: usize, c: Rgba, coverage: f64) {
        let a = c[3] as f64 / 255. * coverage.clamp(0., 1.);
        if a <= 0. || x >= self.width || y >= self.height {
            return;
        }
        let at = (y * self.width + x) * 4;
        let d = &mut self.data[at..at + 4];
        let da = d[3] as f64 / 255.;
        let out = a + da * (1. - a);
        for i in 0..3 {
            let v = (c[i] as f64 * a + d[i] as f64 * da * (1. - a)) / out;
            d[i] = v.round().clamp(0., 255.) as u8;
        }
        d[3] = (out * 255.).round() as u8;
    }
    /// One shape in pixels, over what is already drawn (only its bounding box is visited).
    pub fn layer(&mut self, shape: &Shape, color: Rgba) {
        let (x0, y0, x1, y1) = shape.bounds();
        let (x0, y0) = (x0.floor().max(0.) as usize, y0.floor().max(0.) as usize);
        let x1 = (x1.ceil().max(0.) as usize).min(self.width);
        let y1 = (y1.ceil().max(0.) as usize).min(self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                let d = shape.distance((x as f64 + 0.5, y as f64 + 0.5));
                if d < 0.5 {
                    self.blend(x, y, color, cover(d));
                }
            }
        }
    }
}
/// A line icon drawn on a 20-unit grid at `size` pixels, in one colour. The shapes
/// are a union: overlapping strokes do not darken each other's edges.
pub fn icon(shapes: &[Shape], size: usize, color: Rgba) -> Image {
    let mut out = Image::new(size, size);
    let scale = 20. / out.width as f64;
    for y in 0..out.height {
        for x in 0..out.width {
            let p = ((x as f64 + 0.5) * scale, (y as f64 + 0.5) * scale);
            let d = shapes
                .iter()
                .map(|s| s.distance(p))
                .fold(f64::MAX, f64::min);
            out.blend(x, y, color, cover(d / scale));
        }
    }
    out
}
/// `#rrggbb` or `#rrggbbaa` → RGBA (opaque white when malformed).
pub fn rgba(hex: &str) -> Rgba {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255);
    [
        byte(0),
        byte(2),
        byte(4),
        if h.len() >= 8 { byte(6) } else { 255 },
    ]
}
