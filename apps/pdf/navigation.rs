//! Geometry retained per layout; viewport queries never transfer all pages back
//! into Rust. React receives only visible frames and render scheduling facts.
use super::geometry::{shown, to_page, to_view, visible};
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct Layout {
    pub tops: Vec<f64>,
    pub sizes: Vec<[f64; 2]>,
    pub rot: i64,
    pub scale: f64,
    pub gap: f64,
    pub width: f64,
    pub height: f64,
    viewport: Viewport,
}
#[derive(Default)]
struct Viewport {
    w: f64,
    h: f64,
    offset: f64,
    x: f64,
    page: usize,
    single: bool,
}
fn number(v: &Value) -> Result<f64, &'static str> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or("pdf.invalidNavigation")
}
impl Layout {
    fn frame(&self, page: usize) -> Option<Value> {
        let [w, h] = shown(*self.sizes.get(page)?, self.rot);
        let (w, h) = (w * self.scale, h * self.scale);
        let v = &self.viewport;
        let top = if v.single {
            self.gap.max((v.h - h) / 2.)
        } else {
            self.tops[page] - v.offset
        };
        let left = self.gap.max((v.w - w) / 2.) - v.x;
        if ![left, top, w, h].iter().all(|n| n.is_finite()) {
            return None;
        }
        Some(json!({"page":page,"left":left,"top":top,"width":w,"height":h}))
    }
    fn range(&self) -> (usize, Option<usize>) {
        if self.sizes.is_empty() {
            return (0, None);
        }
        if self.viewport.single {
            return (self.viewport.page, Some(self.viewport.page));
        }
        let r = visible(
            &self.tops,
            self.viewport.offset,
            self.viewport.offset + self.viewport.h,
        );
        (
            r[0].as_u64().unwrap() as usize,
            Some(r[1].as_u64().unwrap() as usize),
        )
    }
    fn max_offset(&self) -> f64 {
        if self.viewport.single {
            0.
        } else {
            (self.height - self.viewport.h).max(0.)
        }
    }
    pub(super) fn request(&mut self, input: &Value) -> Result<Value, &'static str> {
        match input["op"].as_str().unwrap_or("") {
            "viewport" => {
                let (w, h) = (number(&input["view"]["w"])?, number(&input["view"]["h"])?);
                if w < 0. || h < 0. {
                    return Err("pdf.invalidNavigation");
                }
                let page = input["page"].as_u64().ok_or("pdf.invalidNavigation")? as usize;
                let single = match input["mode"].as_str() {
                    Some("single") => true,
                    Some("continuous") => false,
                    _ => return Err("pdf.invalidNavigation"),
                };
                let viewport = Viewport {
                    w,
                    h,
                    offset: number(&input["offset"])?,
                    x: number(&input["offsetX"])?,
                    page: page.min(self.sizes.len().saturating_sub(1)),
                    single,
                };
                let previous = std::mem::replace(&mut self.viewport, viewport);
                let reply = (|| {
                    let (first, last) = self.range();
                    let mut frames = Vec::new();
                    let mut renders = Vec::new();
                    let mut current = first;
                    if let Some(last) = last {
                        for p in first..=last {
                            let frame = self.frame(p).ok_or("pdf.invalidNavigation")?;
                            renders.push(json!({"page":p,"width":frame["width"],"priority":4}));
                            frames.push(frame);
                        }
                        for p in [last as i64 + 1, first as i64 - 1, last as i64 + 2] {
                            if p >= 0 && (p as usize) < self.sizes.len() {
                                renders.push(json!({"page":p,"width":shown(self.sizes[p as usize], self.rot)[0]*self.scale,"priority":2}));
                            }
                        }
                        if !single {
                            let mid = self.viewport.offset + h / 3.;
                            while current < last && self.tops[current + 1] <= mid {
                                current += 1;
                            }
                        }
                    }
                    let max_x = (self.width + 2. * self.gap - w).max(0.);
                    if !max_x.is_finite() {
                        return Err("pdf.invalidNavigation");
                    }
                    Ok(
                        json!({"first":first,"last":last.map(|p| p as i64).unwrap_or(-1),"currentPage":current,
                    "maxOffset":self.max_offset(),"maxX":max_x,"frames":frames,"renders":renders}),
                    )
                })();
                if reply.is_err() {
                    self.viewport = previous;
                }
                reply
            }
            "frame" => Ok(input["page"]
                .as_u64()
                .and_then(|p| self.frame(p as usize))
                .unwrap_or(Value::Null)),
            "goto" => {
                if self.sizes.is_empty() {
                    return Ok(Value::Null);
                }
                let p = number(&input["page"])?;
                let p = p.max(0.).min((self.sizes.len() - 1) as f64) as usize;
                if self.viewport.single {
                    return Ok(json!({"page":p}));
                }
                let y = if let Some(rect) = input.get("rect").filter(|v| !v.is_null()) {
                    let x = number(&rect[0])?;
                    let y = number(&rect[1])?;
                    let w = number(&rect[2])?;
                    let h = number(&rect[3])?;
                    let a = to_view([x, y], self.sizes[p], self.rot, self.scale);
                    let b = to_view([x + w, y + h], self.sizes[p], self.rot, self.scale);
                    self.tops[p] + a[1].min(b[1]) - self.viewport.h / 3.
                } else {
                    self.tops[p] - self.gap
                };
                if !y.is_finite() {
                    return Err("pdf.invalidNavigation");
                }
                Ok(json!({"page":p,"offset":y.min(self.max_offset()).max(0.)}))
            }
            "hit" => {
                let (x, y) = (number(&input["x"])?, number(&input["y"])?);
                let (first, last) = self.range();
                if let Some(last) = last {
                    for p in first..=last {
                        let frame = self.frame(p).ok_or("pdf.invalidNavigation")?;
                        let (l, t, w, h) = (
                            frame["left"].as_f64().unwrap(),
                            frame["top"].as_f64().unwrap(),
                            frame["width"].as_f64().unwrap(),
                            frame["height"].as_f64().unwrap(),
                        );
                        if x >= l && x <= l + w && y >= t && y <= t + h {
                            let [x, y] =
                                to_page([x - l, y - t], self.sizes[p], self.rot, self.scale);
                            if !x.is_finite() || !y.is_finite() {
                                return Err("pdf.invalidNavigation");
                            }
                            return Ok(json!({"page":p,"x":x,"y":y}));
                        }
                    }
                }
                Ok(Value::Null)
            }
            _ => Err("pdf.invalidNavigation"),
        }
    }
}
