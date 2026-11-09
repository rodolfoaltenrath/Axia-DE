//! Antialiased rasters for the dashboard: progress rings and line icons. Shapes are
//! signed-distance tests per pixel, drawn once per data change (rings) or theme (icons).
use super::charts::Raster;
use std::f64::consts::TAU;
/// Straight-alpha "over" with a coverage in 0..1.
fn blend(r: &mut Raster, x: usize, y: usize, c: [u8; 4], coverage: f64) {
    let a = c[3] as f64 / 255. * coverage.clamp(0., 1.);
    if a <= 0. {
        return;
    }
    let at = (y * r.width + x) * 4;
    let d = &mut r.data[at..at + 4];
    let da = d[3] as f64 / 255.;
    let out = a + da * (1. - a);
    for i in 0..3 {
        let v = (c[i] as f64 * a + d[i] as f64 * da * (1. - a)) / out;
        d[i] = v.round().clamp(0., 255.) as u8;
    }
    d[3] = (out * 255.).round() as u8;
}
fn cover(d: f64) -> f64 {
    (0.5 - d).clamp(0., 1.)
}
/// One arc from `start` to `end` (fractions of a turn, clockwise from 12 o'clock),
/// with round caps. Later arcs are drawn over earlier ones.
pub struct Ring {
    pub radius: f64,
    pub width: f64,
    pub start: f64,
    pub end: f64,
    pub color: [u8; 4],
}
/// `tracks` are the full circles (radius, width) drawn under the arcs.
pub fn rings(
    size: usize,
    tracks: &[(f64, f64)],
    arcs: &[Ring],
    track: [u8; 4],
    generation: u64,
) -> Raster {
    let mut out = Raster::new(size, size, generation);
    let c = size as f64 / 2.;
    for y in 0..out.height {
        for x in 0..out.width {
            let (dx, dy) = (x as f64 + 0.5 - c, y as f64 + 0.5 - c);
            let dist = dx.hypot(dy);
            for &(radius, width) in tracks {
                blend(
                    &mut out,
                    x,
                    y,
                    track,
                    cover((dist - radius).abs() - width / 2.),
                );
            }
            // Clockwise angle from the top, 0..1.
            let angle = (dx.atan2(-dy) / TAU).rem_euclid(1.);
            for ring in arcs {
                let (start, end) = (ring.start.clamp(0., 1.), ring.end.clamp(0., 1.));
                if end - start <= 0.001 {
                    continue;
                }
                let base = cover((dist - ring.radius).abs() - ring.width / 2.);
                let arc = if angle >= start && angle <= end {
                    base
                } else {
                    0.
                };
                let cap = |t: f64| {
                    let (s, k) = ((t * TAU).sin(), (t * TAU).cos());
                    cover((dx - s * ring.radius).hypot(dy + k * ring.radius) - ring.width / 2.)
                };
                let v = arc.max(cap(start)).max(cap(end));
                if v > 0. {
                    blend(&mut out, x, y, ring.color, v);
                }
            }
        }
    }
    out
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Dashboard,
    Processes,
    Cpu,
    Memory,
    Storage,
    Network,
    Cooling,
    Voltage,
    Settings,
    Laptop,
    Router,
}
impl Icon {
    pub fn id(self) -> &'static str {
        match self {
            Self::Dashboard => "dashboard",
            Self::Processes => "processes",
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Storage => "storage",
            Self::Network => "network",
            Self::Cooling => "cooling",
            Self::Voltage => "voltage",
            Self::Settings => "settings",
            Self::Laptop => "laptop",
            Self::Router => "router",
        }
    }
}
enum Shape {
    /// Segment with round ends.
    Line((f64, f64), (f64, f64), f64),
    /// Circle outline: centre, radius, stroke.
    Circle((f64, f64), f64, f64),
    Dot((f64, f64), f64),
    /// Rounded rectangle outline: x, y, w, h, corner, stroke.
    Rect(f64, f64, f64, f64, f64, f64),
    FillRect(f64, f64, f64, f64, f64),
    /// Ellipse outline: centre, radii, stroke.
    Ellipse((f64, f64), f64, f64, f64),
    Polygon(&'static [(f64, f64)]),
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
    let (cx, cy) = (x + w / 2., y + h / 2.);
    let qx = (p.0 - cx).abs() - (w / 2. - r);
    let qy = (p.1 - cy).abs() - (h / 2. - r);
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
    /// Signed distance in design units (negative inside the painted area).
    fn distance(&self, p: (f64, f64)) -> f64 {
        match *self {
            Shape::Line(a, b, w) => segment(p, a, b) - w / 2.,
            Shape::Circle(c, r, w) => ((p.0 - c.0).hypot(p.1 - c.1) - r).abs() - w / 2.,
            Shape::Dot(c, r) => (p.0 - c.0).hypot(p.1 - c.1) - r,
            Shape::Rect(x, y, w, h, r, s) => round_box(p, x, y, w, h, r).abs() - s / 2.,
            Shape::FillRect(x, y, w, h, r) => round_box(p, x, y, w, h, r),
            Shape::Ellipse(c, rx, ry, w) => {
                let (nx, ny) = ((p.0 - c.0) / rx, (p.1 - c.1) / ry);
                ((nx.hypot(ny) - 1.) * rx.min(ry)).abs() - w / 2.
            }
            Shape::Polygon(pts) => polygon(p, pts),
        }
    }
}
const S: f64 = 1.6;
const BOLT: &[(f64, f64)] = &[
    (11.5, 1.8),
    (4.0, 11.4),
    (9.3, 11.4),
    (8.3, 18.2),
    (16.0, 8.4),
    (10.6, 8.4),
];
fn shapes(icon: Icon) -> Vec<Shape> {
    use Shape::*;
    match icon {
        Icon::Dashboard => [(3., 3.), (11., 3.), (3., 11.), (11., 11.)]
            .into_iter()
            .map(|(x, y)| Rect(x, y, 6., 6., 1.6, S))
            .collect(),
        Icon::Processes => {
            let mut v = Vec::new();
            for y in [5., 10., 15.] {
                v.push(Dot((4., y), 1.3));
                v.push(Line((8., y), (17., y), S));
            }
            v
        }
        Icon::Cpu => {
            let mut v = vec![Rect(5., 5., 10., 10., 2., S), FillRect(8., 8., 4., 4., 0.8)];
            for k in [7.5, 10., 12.5] {
                v.push(Line((k, 2.2), (k, 4.2), 1.3));
                v.push(Line((k, 15.8), (k, 17.8), 1.3));
                v.push(Line((2.2, k), (4.2, k), 1.3));
                v.push(Line((15.8, k), (17.8, k), 1.3));
            }
            v
        }
        Icon::Memory => {
            let mut v = vec![Rect(2.5, 5.5, 15., 8., 1.5, S)];
            for x in [6.5, 10., 13.5] {
                v.push(FillRect(x - 1.1, 7.8, 2.2, 3.4, 0.5));
                v.push(Line((x, 14.5), (x, 16.5), 1.3));
            }
            v
        }
        Icon::Storage => vec![
            Rect(2.5, 5., 15., 10., 2., S),
            Line((2.8, 11.), (17.2, 11.), S * 0.8),
            Dot((14., 13.), 0.9),
        ],
        Icon::Network => vec![
            Circle((10., 10.), 7.4, S),
            Ellipse((10., 10.), 3.4, 7.4, S * 0.85),
            Line((2.8, 10.), (17.2, 10.), S * 0.85),
        ],
        Icon::Cooling => vec![
            Rect(8., 2.5, 4., 11., 2., S),
            Circle((10., 15.), 3., S),
            Line((10., 8.), (10., 15.), S),
            Dot((10., 15.), 1.6),
        ],
        Icon::Voltage => vec![Polygon(BOLT)],
        Icon::Settings => {
            let mut v = vec![Circle((10., 10.), 5., S), Circle((10., 10.), 2., S)];
            for k in 0..8 {
                let a = k as f64 * TAU / 8.;
                let (s, c) = (a.sin(), a.cos());
                v.push(Line(
                    (10. + s * 5.6, 10. - c * 5.6),
                    (10. + s * 7.6, 10. - c * 7.6),
                    2.3,
                ));
            }
            v
        }
        Icon::Laptop => vec![
            Rect(3.5, 4., 13., 9., 1.5, S),
            Line((1.5, 15.8), (18.5, 15.8), S),
        ],
        Icon::Router => vec![
            Rect(2.5, 10., 15., 6.5, 1.6, S),
            Line((6., 10.), (4.8, 4.5), S * 0.9),
            Line((14., 10.), (15.2, 4.5), S * 0.9),
            Dot((6., 13.25), 0.9),
            Dot((9., 13.25), 0.9),
        ],
    }
}
/// A 20-unit line icon drawn at `size` pixels in one colour.
pub fn icon(icon: Icon, size: usize, color: [u8; 4]) -> Raster {
    let mut out = Raster::new(size, size, 1);
    let scale = 20. / size as f64;
    let shapes = shapes(icon);
    for y in 0..out.height {
        for x in 0..out.width {
            let p = ((x as f64 + 0.5) * scale, (y as f64 + 0.5) * scale);
            let d = shapes
                .iter()
                .map(|s| s.distance(p))
                .fold(f64::MAX, f64::min);
            // Distances are in design units: convert to pixels before covering.
            blend(&mut out, x, y, color, cover(d / scale));
        }
    }
    out
}
