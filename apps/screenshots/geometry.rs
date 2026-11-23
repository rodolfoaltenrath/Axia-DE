//! Selection and image-space geometry shared by the transitional host and native view.
//! Coordinates are logical pixels; image coordinates may lie outside the image
//! while a stroke is dragged through a letterbox. The rasterizer clips strokes.
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
impl Point {
    pub fn read(value: &Value) -> Option<Self> {
        let p = Self {
            x: value["x"].as_f64()?,
            y: value["y"].as_f64()?,
        };
        (p.x.is_finite() && p.y.is_finite()).then_some(p)
    }
}
impl Rect {
    pub fn read(value: &Value) -> Option<Self> {
        let p = Point::read(value)?;
        let r = Self {
            x: p.x,
            y: p.y,
            w: value["w"].as_f64()?,
            h: value["h"].as_f64()?,
        };
        (r.w.is_finite() && r.h.is_finite() && r.w > 0. && r.h > 0.).then_some(r)
    }
    pub fn between(a: Point, b: Point) -> Self {
        Self {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            w: (b.x - a.x).abs(),
            h: (b.y - a.y).abs(),
        }
    }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.w && p.y < self.y + self.h
    }
    pub fn capturable(self) -> bool {
        self.w >= 3. && self.h >= 3.
    }
    pub fn value(self) -> Value {
        json!({"x":self.x,"y":self.y,"w":self.w,"h":self.h})
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Fit {
    pub scale: f64,
    pub origin: Point,
}
impl Fit {
    pub fn contain(viewport: Rect, width: f64, height: f64) -> Option<Self> {
        if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
            return None;
        }
        let scale = (viewport.w / width).min(viewport.h / height);
        if !scale.is_finite() || scale <= 0. {
            return None;
        }
        Some(Self {
            scale,
            origin: Point {
                x: viewport.x + (viewport.w - width * scale) / 2.,
                y: viewport.y + (viewport.h - height * scale) / 2.,
            },
        })
    }
    pub fn image(self, point: Point) -> Point {
        Point {
            x: (point.x - self.origin.x) / self.scale,
            y: (point.y - self.origin.y) / self.scale,
        }
    }
    pub fn screen(self, point: Point) -> Point {
        Point {
            x: self.origin.x + point.x * self.scale,
            y: self.origin.y + point.y * self.scale,
        }
    }
}

/// Narrow request interface retained only while the JSX presentation migrates.
pub fn request(input: &Value) -> Value {
    match input["cmd"].as_str().unwrap_or("") {
        "selection" => {
            let Some((a, b)) = Point::read(&input["start"]).zip(Point::read(&input["end"])) else {
                return Value::Null;
            };
            Rect::between(a, b).value()
        }
        "map" | "anchor" => {
            let Some((viewport, point)) =
                Rect::read(&input["viewport"]).zip(Point::read(&input["point"]))
            else {
                return Value::Null;
            };
            let Some(fit) = input["image"]["width"]
                .as_f64()
                .zip(input["image"]["height"].as_f64())
                .and_then(|(w, h)| Fit::contain(viewport, w, h))
            else {
                return Value::Null;
            };
            if input["cmd"] == "map" {
                let p = fit.image(point);
                json!({"x":p.x,"y":p.y,"s":fit.scale})
            } else {
                let p = fit.screen(point);
                json!({"left":p.x-viewport.x,"top":p.y-viewport.y})
            }
        }
        "window" => {
            let Some(p) = Point::read(&input["point"]) else {
                return Value::Null;
            };
            input["windows"]
                .as_array()
                .into_iter()
                .flatten()
                .rev()
                .find(|w| {
                    w["minimized"] != true
                        && w["app"] != "screenshots"
                        && Rect::read(&w["frame"]).is_some_and(|r| r.contains(p))
                })
                .map(|w| {
                    let mut frame = w["frame"].clone();
                    frame["title"] = w["title"].clone();
                    frame
                })
                .unwrap_or(Value::Null)
        }
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reverse_drag_and_zero_area_preserve_the_selection() {
        assert_eq!(
            request(&json!({"cmd":"selection","start":{"x":120,"y":80},"end":{"x":10,"y":20}})),
            json!({"x":10.,"y":20.,"w":110.,"h":60.})
        );
        assert!(!Rect::between(Point { x: 1., y: 1. }, Point { x: 1., y: 9. }).capturable());
        assert!(Rect::between(Point { x: 0., y: 0. }, Point { x: 3., y: 3. }).capturable());
    }
    #[test]
    fn image_mapping_and_text_anchor_share_letterbox_and_mount_origin() {
        for (vw, vh, iw, ih) in [
            (800., 600., 1920., 1080.),
            (600., 800., 1080., 1920.),
            (200., 100., 10., 10.),
        ] {
            let viewport = Rect {
                x: 53.,
                y: 91.,
                w: vw,
                h: vh,
            };
            let fit = Fit::contain(viewport, iw, ih).unwrap();
            for p in [
                Point { x: 0., y: 0. },
                Point { x: iw, y: ih },
                Point {
                    x: -10.,
                    y: ih + 10.,
                },
            ] {
                let screen = fit.screen(p);
                let back = fit.image(screen);
                assert!((back.x - p.x).abs() < 1e-9 && (back.y - p.y).abs() < 1e-9);
                let input = json!({"viewport":viewport.value(),"point":{"x":p.x,"y":p.y},"image":{"width":iw,"height":ih},"cmd":"anchor"});
                let anchor = request(&input);
                assert!((anchor["left"].as_f64().unwrap() + viewport.x - screen.x).abs() < 1e-9);
                assert!((anchor["top"].as_f64().unwrap() + viewport.y - screen.y).abs() < 1e-9);
            }
        }
    }
    #[test]
    fn topmost_window_uses_exclusive_edges_and_ignores_capture_and_minimized_windows() {
        let mut input = json!({"cmd":"window","point":{"x":20,"y":20},"windows":[
            {"app":"notes","title":"bottom","frame":{"x":0,"y":0,"w":100,"h":100}},
            {"app":"editor","title":"top","frame":{"x":20,"y":20,"w":60,"h":60}},
            {"app":"photos","title":"hidden","minimized":true,"frame":{"x":0,"y":0,"w":100,"h":100}},
            {"app":"screenshots","title":"overlay","frame":{"x":0,"y":0,"w":100,"h":100}}
        ]});
        assert_eq!(request(&input)["title"], "top");
        input["point"] = json!({"x":80,"y":80});
        assert_eq!(request(&input)["title"], "bottom");
        input["point"] = json!({"x":100,"y":100});
        assert!(request(&input).is_null());
    }
    #[test]
    fn invalid_geometry_never_reaches_media_calls() {
        for viewport in [
            json!({"x":0,"y":0,"w":0,"h":1}),
            json!({"x":0,"y":0,"w":1,"h":-1}),
            Value::Null,
        ] {
            assert!(request(&json!({"cmd":"map","point":{"x":0,"y":0},"viewport":viewport,"image":{"width":100,"height":100}})).is_null());
        }
        assert!(Fit::contain(
            Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: 1.
            },
            0.,
            1.
        )
        .is_none());
        assert!(Fit::contain(
            Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: 1.
            },
            f64::INFINITY,
            1.
        )
        .is_none());
        assert!(request(&json!({"cmd":"selection","start":{"x":0},"end":{"x":1,"y":1}})).is_null());
    }
}
