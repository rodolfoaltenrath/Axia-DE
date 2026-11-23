//! Native capture selector. The transport supplies a frozen desktop image and
//! current window/display facts; only confirmation publishes a crop rectangle.
use crate::geometry::{Point, Rect};
use axia_native::NativeApp;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Area,
    Window,
    Monitor,
    Desktop,
}
impl Mode {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "area" => Self::Area,
            "window" => Self::Window,
            "monitor" => Self::Monitor,
            "desktop" => Self::Desktop,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Area => "area",
            Self::Window => "window",
            Self::Monitor => "monitor",
            Self::Desktop => "desktop",
        }
    }
}
fn clip(r: Rect, b: Rect) -> Option<Rect> {
    let x = r.x.max(b.x);
    let y = r.y.max(b.y);
    let w = (r.x + r.w).min(b.x + b.w) - x;
    let h = (r.y + r.h).min(b.y + b.h) - y;
    (w > 0. && h > 0.).then_some(Rect { x, y, w, h })
}
pub struct App {
    image: i64,
    bounds: Rect,
    mode: Mode,
    area: Option<Rect>,
    anchor: Option<Point>,
    target: Option<(Rect, String)>,
    locked: bool,
    pointer: Option<Point>,
    windows: Value,
    displays: Value,
    desktop: Value,
    terminal: bool,
    initialized: bool,
    effects: Vec<Value>,
}
impl Default for App {
    fn default() -> Self {
        Self {
            image: 0,
            bounds: Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: 1.,
            },
            mode: Mode::Area,
            area: None,
            anchor: None,
            target: None,
            locked: false,
            pointer: None,
            windows: Value::Null,
            displays: Value::Null,
            desktop: Value::Null,
            terminal: false,
            initialized: false,
            effects: vec![],
        }
    }
}
impl App {
    fn hover(&mut self, p: Point) {
        self.pointer = Some(p);
        if let Some(a) = self.anchor {
            self.area = clip(Rect::between(a, p), self.bounds);
            return;
        }
        if self.locked {
            return;
        }
        self.target = match self.mode {
            Mode::Window => self
                .windows
                .as_array()
                .into_iter()
                .flatten()
                .rev()
                .find_map(|w| {
                    if w["app"] == "screenshots"
                        || w["minimized"] == true
                        || w["hidden"] == true
                        || w["visible"] == false
                        || (!w["desktop"].is_null()
                            && !self.desktop.is_null()
                            && w["desktop"] != self.desktop
                            && w["sticky"] != true
                            && w["pip"] != true)
                    {
                        return None;
                    }
                    let r = Rect::read(&w["frame"])?;
                    (r.contains(p))
                        .then(|| {
                            clip(r, self.bounds)
                                .map(|r| (r, w["title"].as_str().unwrap_or("").into()))
                        })
                        .flatten()
                }),
            Mode::Monitor => self
                .displays
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|d| {
                    let r = Rect::read(d)?;
                    r.contains(p)
                        .then(|| {
                            clip(r, self.bounds)
                                .map(|r| (r, d["name"].as_str().unwrap_or("").into()))
                        })
                        .flatten()
                }),
            Mode::Desktop => Some((self.bounds, String::new())),
            Mode::Area => None,
        };
    }
    fn selected(&self) -> Option<(Rect, String)> {
        if self.mode == Mode::Area {
            self.area.map(|r| (r, String::new()))
        } else if self.mode == Mode::Desktop {
            Some((self.bounds, String::new()))
        } else {
            self.target.clone()
        }
    }
    fn mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.anchor = None;
        self.locked = false;
        self.target = None;
        if let Some(p) = self.pointer {
            self.hover(p);
        }
    }
    fn finish(&mut self, cancel: bool) {
        let selected = self.selected().filter(|(r, _)| r.capturable());
        if self.terminal || !cancel && (selected.is_none() || self.anchor.is_some()) {
            return;
        }
        self.terminal = true;
        self.anchor = None;
        let rect = selected.map(|v| v.0);
        self.effects.push(json!({"kind":"captureSelection","source":self.image,"rect":if cancel{Value::Null}else{rect.unwrap().value()},"thumbnail":if cancel{Value::Null}else{self.thumbnail(rect.unwrap())}}));
    }
    fn main_display(&self) -> Rect {
        self.displays
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["main"] == true)
            .or_else(|| self.displays.as_array().and_then(|d| d.first()))
            .and_then(Rect::read)
            .and_then(|r| clip(r, self.bounds))
            .unwrap_or(self.bounds)
    }
    fn thumbnail(&self, selection: Rect) -> Value {
        let mut display = self.main_display();
        let mut area = clip(selection, display).map_or(0., |r| r.w * r.h);
        if self.mode != Mode::Desktop {
            for d in self
                .displays
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Rect::read)
            {
                if let Some(r) = clip(selection, d) {
                    if r.w * r.h > area {
                        area = r.w * r.h;
                        display = d;
                    }
                }
            }
        }
        let w = (display.w - 40.).clamp(1., 240.);
        let h = (display.h - 40.).clamp(1., 150.);
        let bottom = 90f64.min((display.h - h - 20.).max(0.));
        json!({"x":display.x+display.w-w-20.,"y":display.y+display.h-h-bottom,"w":w,"h":h})
    }
    fn action(&mut self, input: &Value) {
        if self.terminal {
            return;
        }
        match input["cmd"].as_str().unwrap_or("") {
            "mode" => {
                if let Some(mode) = input["key"].as_str().and_then(Mode::parse) {
                    self.mode(mode);
                }
            }
            "down" => {
                if input["button"].as_i64().unwrap_or(0) == 0 {
                    if let Some(p) = Point::read(input) {
                        if self.mode == Mode::Area {
                            self.anchor = Some(p);
                            self.area = None;
                        } else {
                            self.locked = false;
                            self.hover(p);
                            self.locked = self.target.is_some();
                        }
                    }
                }
            }
            "move" | "pointerMove" => {
                if let Some(p) = Point::read(input) {
                    self.hover(p);
                }
            }
            "up" | "dragEnd" => {
                if self.anchor.is_some() {
                    if let Some(p) = Point::read(input) {
                        self.hover(p);
                    }
                    self.anchor = None;
                }
            }
            "confirm" => self.finish(false),
            "cancel" => self.finish(true),
            _ => {}
        }
    }
    fn button(&self, mode: &str, label: &str) -> Value {
        control(
            mode,
            label,
            json!(["mode", mode]),
            self.mode.name() == mode,
            false,
        )
    }
    fn tree(&self) -> Value {
        let main = self.main_display();
        let mut children = vec![
            json!({"kind":"media","key":"snapshot","media":self.image,"style":{"position":"absolute","left":0,"top":0,"width":self.bounds.w,"height":self.bounds.h,"pointerEvents":"none"}}),
        ];
        let dim = "rgba(0,0,0,0.42)";
        if let Some((r, title)) = self.selected() {
            for (id, x, y, w, h) in [
                ("top", 0., 0., self.bounds.w, r.y),
                (
                    "bottom",
                    0.,
                    r.y + r.h,
                    self.bounds.w,
                    (self.bounds.h - r.y - r.h).max(0.),
                ),
                ("left", 0., r.y, r.x, r.h),
                (
                    "right",
                    r.x + r.w,
                    r.y,
                    (self.bounds.w - r.x - r.w).max(0.),
                    r.h,
                ),
            ] {
                children.push(json!({"kind":"view","key":id,"style":{"position":"absolute","left":x,"top":y,"width":w,"height":h,"backgroundColor":dim,"pointerEvents":"none"}}));
            }
            children.push(json!({"kind":"view","key":"selection","style":{"position":"absolute","left":r.x,"top":r.y,"width":r.w,"height":r.h,"borderWidth":2,"borderColor":"#fff","pointerEvents":"none"}}));
            let caption = if title.is_empty() {
                format!("{:.0} × {:.0}", r.w, r.h)
            } else {
                title
            };
            if self.mode != Mode::Desktop {
                let mut top = (r.y + 6.).min((self.bounds.h - 100.).max(0.));
                if r.x < main.x + main.w
                    && r.x + r.w > main.x
                    && top >= main.y
                    && top < main.y + 80.
                {
                    top = (main.y + 80.).min((self.bounds.h - 40.).max(0.));
                }
                children.push(json!({"kind":"view","key":"dimensions","style":{"position":"absolute","left":(r.x+6.).min((self.bounds.w-220.).max(0.)),"top":top,"maxWidth":220,"padding":6,"backgroundColor":"rgba(0,0,0,0.8)","borderRadius":5,"pointerEvents":"none"},"children":[{"kind":"text","label":caption,"style":{"color":"#fff","fontSize":12}}]}));
            }
        } else {
            children.push(json!({"kind":"view","key":"dim","style":{"position":"absolute","left":0,"top":0,"width":self.bounds.w,"height":self.bounds.h,"backgroundColor":dim,"pointerEvents":"none"}}));
        }
        children.push(json!({"kind":"view","key":"hint","style":{"position":"absolute","left":main.x+8.,"right":self.bounds.w-main.x-main.w+8.,"top":main.y+18.,"alignItems":"center","pointerEvents":"none"},"children":[{"kind":"text","label":{"key":format!("screenshots.selector.hint.{}",self.mode.name())},"style":{"maxWidth":(main.w-16.).max(1.),"color":"#fff","fontSize":13,"backgroundColor":"rgba(0,0,0,0.8)","padding":8,"borderRadius":8}}]}));
        let mut controls = vec![
            self.button("area", "mode.area"),
            self.button("window", "mode.window"),
            self.button("monitor", "mode.monitor"),
            self.button("desktop", "mode.desktop"),
        ];
        controls.push(control(
            "capture",
            "selector.capture",
            json!(["confirm"]),
            true,
            self.selected().is_none_or(|(r, _)| !r.capturable()) || self.anchor.is_some(),
        ));
        controls.push(control(
            "cancel",
            "selector.cancel",
            json!(["cancel"]),
            false,
            false,
        ));
        children.push(json!({"kind":"view","key":"toolbar-position","style":{"position":"absolute","left":main.x+8.,"right":self.bounds.w-main.x-main.w+8.,"bottom":self.bounds.h-main.y-main.h+24.,"alignItems":"center"},"children":[{"kind":"view","key":"toolbar","events":{"onMouseDown":["noop"],"onMouseMove":["noop"],"onMouseUp":["noop"]},"style":{"flexDirection":"row","flexWrap":"wrap","justifyContent":"center","gap":6,"padding":10,"maxWidth":600f64.min((main.w-16.).max(1.)),"backgroundColor":"#29292eea","borderRadius":12},"children":controls}]}));
        json!({"kind":"view","key":"capture-selector","events":{"onClick":["noop"],"onMouseDown":["down"],"onMouseMove":["move"],"onMouseUp":["up"]},"style":{"flex":1,"direction":"ltr","overflow":"hidden"},"children":children})
    }
    fn reply(&mut self) -> Value {
        json!({"tree":self.tree(),"capture":self.anchor.is_some(),"effects":std::mem::take(&mut self.effects)})
    }
}
fn control(id: &str, label: &str, action: Value, selected: bool, disabled: bool) -> Value {
    json!({"kind":"view","key":id,"events":{"onClick":if disabled{json!(["noop"])}else{action},"onMouseDown":["noop"],"onMouseUp":["noop"],"onMouseMove":["noop"]},
        "hover":if disabled{json!({})}else{json!({"opacity":0.85})},
        "style":{"height":34,"paddingHorizontal":12,"alignItems":"center","justifyContent":"center","backgroundColor":if selected{"#0a84ff"}else{"#45454a"},"borderRadius":7,"opacity":if disabled{0.4}else{1.}},
        "children":[{"kind":"text","label":{"key":format!("screenshots.{label}")},"style":{"color":"#fff","fontSize":13,"fontWeight":500,"whiteSpace":"nowrap","pointerEvents":"none"}}]})
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool {
        true
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] == "icon" {
            return icon(input["size"].as_f64().unwrap_or(48.));
        }
        if input["cmd"] == "sync" {
            let a = &input["args"];
            if !self.initialized || a["handle"].as_i64().unwrap_or(0) != self.image {
                *self = Self::default();
                self.image = a["handle"].as_i64().unwrap_or(0);
                self.initialized = true;
                self.mode = Mode::parse(a["mode"].as_str().unwrap_or("area")).unwrap_or(Mode::Area);
            }
            self.bounds = Rect {
                x: 0.,
                y: 0.,
                w: a["screen"]["width"].as_f64().unwrap_or(1.).max(1.),
                h: a["screen"]["height"].as_f64().unwrap_or(1.).max(1.),
            };
            self.windows = a["windows"].clone();
            self.displays = a["displays"].clone();
            self.desktop = a["desktop"].clone();
            if let Some(p) = self.pointer {
                self.hover(p);
            }
        } else {
            self.action(input);
        }
        self.reply()
    }
    fn key(&mut self, key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        match key {
            "Escape" => self.finish(true),
            "Enter" if self.anchor.is_none() => self.finish(false),
            " " | "Space" => self.mode(if self.mode == Mode::Area {
                Mode::Window
            } else {
                Mode::Area
            }),
            _ => return None,
        }
        Some(self.reply())
    }
}

pub fn icon(s: f64) -> Value {
    let mut children = vec![];
    for (x, y, w, h) in [
        (0.18, 0.2, 0.18, 0.05),
        (0.18, 0.2, 0.05, 0.18),
        (0.64, 0.2, 0.18, 0.05),
        (0.77, 0.2, 0.05, 0.18),
        (0.18, 0.75, 0.18, 0.05),
        (0.18, 0.62, 0.05, 0.18),
        (0.64, 0.75, 0.18, 0.05),
        (0.77, 0.62, 0.05, 0.18),
    ] {
        children.push(json!({"kind":"view","style":{"position":"absolute","left":s*x,"top":s*y,"width":s*w,"height":s*h,"backgroundColor":"#fff","borderRadius":s*0.02}}));
    }
    for (x, y, w, h, r, color) in [
        (0.36, 0.38, 0.28, 0.24, 0.05, "#ffffffe6"),
        (0.44, 0.43, 0.12, 0.12, 0.06, "#2c333d"),
        (0.47, 0.46, 0.06, 0.06, 0.03, "#ff5f57"),
    ] {
        children.push(json!({"kind":"view","style":{"position":"absolute","left":s*x,"top":s*y,"width":s*w,"height":s*h,"backgroundColor":color,"borderRadius":s*r}}));
    }
    json!({"kind":"view","style":{"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #5e6b7a, #2c333d)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden"},"children":children})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let mut app = App::default();
        app.request(&json!({"cmd":"sync","args":{"handle":1,"screen":{"width":1800,"height":1000},"desktop":1,
            "windows":[{"app":"notes","desktop":1,"title":"Note","frame":{"x":100,"y":80,"w":600,"h":500}},{"app":"editor","desktop":2,"frame":{"x":100,"y":80,"w":600,"h":500}}],
            "displays":[{"x":0,"y":0,"w":1000,"h":1000,"name":"Main","scale":150},{"x":1000,"y":100,"w":800,"h":600,"name":"Second","scale":200}]}}));
        app
    }
    #[test]
    fn reverse_drag_waits_for_confirmation_and_publishes_only_once() {
        let mut a = app();
        a.request(&json!({"cmd":"down","x":500,"y":400}));
        a.request(&json!({"cmd":"move","x":100,"y":80}));
        let up = a.request(&json!({"cmd":"up","x":100,"y":80}));
        assert!(up["effects"].as_array().unwrap().is_empty());
        let done = a.key("Enter", "", 0).unwrap();
        assert_eq!(
            done["effects"][0]["rect"],
            json!({"x":100.,"y":80.,"w":400.,"h":320.})
        );
        assert!(a.key("Enter", "", 0).unwrap()["effects"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    #[test]
    fn window_filters_other_desktops_and_monitor_uses_logical_geometry() {
        let mut a = app();
        a.request(&json!({"cmd":"mode","key":"window"}));
        a.request(&json!({"cmd":"move","x":200,"y":200}));
        assert_eq!(a.selected().unwrap().1, "Note");
        a.request(&json!({"cmd":"mode","key":"monitor"}));
        a.request(&json!({"cmd":"move","x":1500,"y":300}));
        assert_eq!(
            a.selected().unwrap().0,
            Rect {
                x: 1000.,
                y: 100.,
                w: 800.,
                h: 600.
            }
        );
        a.request(&json!({"cmd":"move","x":1500,"y":900}));
        assert!(a.selected().is_none());
    }
    #[test]
    fn tiny_or_unfinished_area_cannot_confirm_and_escape_always_cancels() {
        let mut a = app();
        assert!(a.key("Enter", "", 0).unwrap()["effects"]
            .as_array()
            .unwrap()
            .is_empty());
        a.request(&json!({"cmd":"down","x":1,"y":1}));
        assert!(a.key("Enter", "", 0).is_none());
        a.request(&json!({"cmd":"up","x":2,"y":2}));
        assert!(a.key("Enter", "", 0).unwrap()["effects"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(a.key("Escape", "", 0).unwrap()["effects"][0]["rect"].is_null());
    }
    #[test]
    fn picture_in_picture_remains_selectable_across_desktops() {
        let mut a = app();
        a.windows.as_array_mut().unwrap().push(json!({"app":"player","desktop":2,"pip":true,"title":"Video","frame":{"x":150,"y":150,"w":240,"h":150}}));
        a.mode(Mode::Window);
        a.hover(Point { x: 200., y: 200. });
        assert_eq!(a.selected().unwrap().1, "Video");
        let done = a.key("Enter", "", 0).unwrap();
        assert_eq!(
            done["effects"][0]["rect"],
            json!({"x":150.,"y":150.,"w":240.,"h":150.})
        );
    }
}
