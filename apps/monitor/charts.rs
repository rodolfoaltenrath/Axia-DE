//! Native RGBA chart/map rasterization. Buffers cross the shared host as ArrayBuffers,
//! never as per-pixel JavaScript or JSON arrays.
use std::collections::HashMap;
#[derive(Clone, Debug)]
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
    pub generation: u64,
}
impl Raster {
    pub fn new(w: usize, h: usize, generation: u64) -> Self {
        let width = w.clamp(1, 2048);
        let height = h.clamp(1, 1024);
        Self {
            width,
            height,
            data: vec![0; width * height * 4],
            generation,
        }
    }
    fn pixel(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let at = (y as usize * self.width + x as usize) * 4;
        self.data[at..at + 4].copy_from_slice(&color);
    }
    pub fn line(&mut self, a: (f64, f64), b: (f64, f64), c: [u8; 4]) {
        if ![a.0, a.1, b.0, b.1].iter().all(|n| n.is_finite()) {
            return;
        }
        let steps = ((a.0 - b.0).abs().max((a.1 - b.1).abs()).ceil() as usize)
            .min(8192)
            .max(1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            self.pixel(
                (a.0 + (b.0 - a.0) * t).round() as i32,
                (a.1 + (b.1 - a.1) * t).round() as i32,
                c,
            )
        }
    }
    pub fn circle(&mut self, c: (f64, f64), r: i32, color: [u8; 4]) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    self.pixel(c.0 as i32 + dx, c.1 as i32 + dy, color)
                }
            }
        }
    }
}
pub fn area(
    w: usize,
    h: usize,
    series: &[(Vec<f64>, [u8; 4])],
    top: f64,
    capacity: usize,
    generation: u64,
) -> Raster {
    let mut out = Raster::new(w, h, generation);
    let top = if top.is_finite() && top > 0. { top } else { 1. };
    let capacity = capacity.max(2);
    for (values, color) in series {
        let values = &values[values.len().saturating_sub(capacity)..];
        if values.len() < 2 {
            continue;
        }
        let offset = capacity - values.len();
        let mut previous = None;
        for x in 0..out.width {
            let f =
                x as f64 / (out.width - 1).max(1) as f64 * (capacity - 1) as f64 - offset as f64;
            if f < 0. {
                continue;
            }
            let i = (f.floor() as usize).min(values.len() - 1);
            let v = if i + 1 < values.len() {
                values[i] * (1. - f.fract()) + values[i + 1] * f.fract()
            } else {
                values[i]
            };
            if !v.is_finite() {
                previous = None;
                continue;
            }
            let y = out.height as f64
                - 1.
                - v.clamp(0., top) / top * out.height.saturating_sub(3) as f64;
            for yy in y.ceil().max(0.) as usize..out.height {
                let mut c = *color;
                c[3] = (82. - 77. * yy as f64 / out.height as f64) as u8;
                out.pixel(x as i32, yy as i32, c)
            }
            if let Some(p) = previous {
                out.line(p, (x as f64, y), *color)
            }
            previous = Some((x as f64, y));
        }
    }
    out
}
pub fn world(
    w: usize,
    h: usize,
    rings: &[Vec<(f64, f64)>],
    places: &[(f64, f64)],
    home: Option<(f64, f64)>,
    lon: f64,
    lat: f64,
    zoom: f64,
    generation: u64,
) -> Raster {
    let mut out = Raster::new(w, h, generation);
    let scale = out.width as f64 / 360. * zoom.clamp(1., 16.);
    let point = |p: (f64, f64)| ((p.0 - lon) * scale, (lat - p.1) * scale);
    for ring in rings {
        for pair in ring.windows(2) {
            let a = point(pair[0]);
            let b = point(pair[1]);
            if (a.0 - b.0).abs() < out.width as f64 {
                out.line(a, b, [127, 141, 156, 160]);
            }
        }
    }
    for place in places.iter().take(256) {
        let end = point(*place);
        if let Some(home) = home {
            let start = point(home);
            let mut prev = start;
            for i in 1..=48 {
                let t = i as f64 / 48.;
                let p = (
                    start.0 + (end.0 - start.0) * t,
                    start.1 + (end.1 - start.1) * t
                        - (end.0 - start.0).abs() * 0.2 * (std::f64::consts::PI * t).sin(),
                );
                out.line(prev, p, [60, 150, 255, 140]);
                prev = p;
            }
            out.circle(start, 4, [50, 210, 130, 255]);
        }
        out.circle(end, 4, [60, 150, 255, 255]);
    }
    out
}
pub type Images = HashMap<String, std::sync::Arc<Raster>>;
#[derive(Clone)]
pub struct MapView {
    pub width: usize,
    pub places: Vec<(f64, f64)>,
    pub home: Option<(f64, f64)>,
    pub lon: f64,
    pub lat: f64,
    pub zoom: f64,
    pub generation: u64,
}
