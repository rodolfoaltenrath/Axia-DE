//! Geometry is independent of widgets and decoders. Coordinates are in the
//! oriented image, so crop selection and anchored zoom remain exact at any scale.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub zoom: f64,
    pub cx: f64,
    pub cy: f64,
    pub rotation: u32,
    pub brightness: i32,
    pub contrast: i32,
    pub quality: bool,
}
impl Default for View {
    fn default() -> Self {
        Self {
            width: 800,
            height: 500,
            zoom: 0.,
            cx: 0.5,
            cy: 0.5,
            rotation: 0,
            brightness: 0,
            contrast: 0,
            quality: true,
        }
    }
}
impl View {
    pub fn size(&self, w: u32, h: u32) -> (u32, u32) {
        if self.rotation % 2 == 0 {
            (w, h)
        } else {
            (h, w)
        }
    }
    pub fn scale(&self, w: u32, h: u32) -> f64 {
        let (rw, rh) = self.size(w, h);
        if self.zoom > 0. {
            self.zoom
        } else {
            (self.width as f64 / rw.max(1) as f64).min(self.height as f64 / rh.max(1) as f64)
        }
    }
    pub fn origin(&self, w: u32, h: u32) -> (f64, f64) {
        let (rw, rh) = self.size(w, h);
        let s = self.scale(w, h);
        let axis = |boxw: f64, image: f64, c: f64| {
            if image <= boxw {
                (boxw - image) * 0.5
            } else {
                (boxw * 0.5 - c * image).clamp(boxw - image, 0.)
            }
        };
        (
            axis(self.width as f64, rw as f64 * s, self.cx),
            axis(self.height as f64, rh as f64 * s, self.cy),
        )
    }
    pub fn point(&self, w: u32, h: u32, x: f64, y: f64) -> (f64, f64) {
        let (o, p) = self.origin(w, h);
        let s = self.scale(w, h);
        let (rw, rh) = self.size(w, h);
        (
            ((x - o) / s).clamp(0., rw as f64),
            ((y - p) / s).clamp(0., rh as f64),
        )
    }
    pub fn zoom_at(&mut self, w: u32, h: u32, factor: f64, x: f64, y: f64) {
        if !factor.is_finite() || factor <= 0. || !x.is_finite() || !y.is_finite() {
            return;
        }
        let (px, py) = self.point(w, h, x, y);
        let old = self.scale(w, h);
        let fit = {
            let mut v = *self;
            v.zoom = 0.;
            v.scale(w, h)
        };
        let scale = (old * factor).clamp(fit, 32f64.max(fit * 4.));
        self.zoom = if scale <= fit * 1.001 { 0. } else { scale };
        let (rw, rh) = self.size(w, h);
        self.cx = (px / rw.max(1) as f64 + (self.width as f64 * 0.5 - x) / (rw as f64 * scale))
            .clamp(0., 1.);
        self.cy = (py / rh.max(1) as f64 + (self.height as f64 * 0.5 - y) / (rh as f64 * scale))
            .clamp(0., 1.);
        if self.zoom == 0. {
            self.cx = 0.5;
            self.cy = 0.5
        }
    }
    pub fn crop(&self, w: u32, h: u32, start: (f64, f64), end: (f64, f64)) -> Option<[u32; 4]> {
        let (a, b) = self.point(w, h, start.0, start.1);
        let (c, d) = self.point(w, h, end.0, end.1);
        let (x, y) = (a.min(c).floor() as u32, b.min(d).floor() as u32);
        let (rw, rh) = self.size(w, h);
        let (r, b) = (
            a.max(c).ceil().min(rw as f64) as u32,
            b.max(d).ceil().min(rh as f64) as u32,
        );
        (r > x && b > y).then_some([x, y, r - x, b - y])
    }
}
pub fn source_xy(rotation: u32, w: u32, h: u32, x: u32, y: u32) -> (u32, u32) {
    match rotation % 4 {
        1 => (y, h - 1 - x),
        2 => (w - 1 - x, h - 1 - y),
        3 => (w - 1 - y, x),
        _ => (x, y),
    }
}
pub fn tone(brightness: i32, contrast: i32) -> [u8; 256] {
    let k = ((100 + contrast.clamp(-100, 100)) as f64 / 100.).powi(2);
    std::array::from_fn(|i| {
        (((i as f64 - 128.) * k + 128. + brightness.clamp(-100, 100) as f64 * 1.28)
            .round()
            .clamp(0., 255.)) as u8
    })
}
