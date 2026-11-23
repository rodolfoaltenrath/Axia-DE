//! Screenshot annotation policy. Image resources belong to the transport owner;
//! this model retains coordinates and operations only.
use crate::geometry::{Fit, Point, Rect};
use serde_json::{json, Value};

pub const COLORS: [&str; 6] = [
    "#ff3b30", "#ffcc00", "#34c759", "#0a84ff", "#ffffff", "#1c1c1e",
];
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    Arrow,
    Box,
    Text,
    Blur,
    Line,
}
impl Tool {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "arrow" => Self::Arrow,
            "box" => Self::Box,
            "text" => Self::Text,
            "blur" => Self::Blur,
            "line" => Self::Line,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Arrow => "arrow",
            Self::Box => "box",
            Self::Text => "text",
            Self::Blur => "blur",
            Self::Line => "line",
        }
    }
}
struct Text {
    point: Point,
    value: String,
}
pub struct Editor {
    image: (f64, f64),
    tool: Tool,
    color: usize,
    ops: Vec<Value>,
    draft: Option<Value>,
    text: Option<Text>,
    revision: u64,
    drawing_revision: u64,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            image: (1., 1.),
            tool: Tool::Arrow,
            color: 0,
            ops: vec![],
            draft: None,
            text: None,
            revision: 0,
            drawing_revision: 0,
        }
    }
}
fn js_space(c: char) -> bool {
    matches!(c,'\u{0009}'..='\u{000d}'|'\u{0020}'|'\u{00a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}')
}
impl Editor {
    fn changed(&mut self, drawing: bool) {
        self.revision = self.revision.wrapping_add(1);
        if drawing {
            self.drawing_revision = self.drawing_revision.wrapping_add(1);
        }
    }
    fn width(&self) -> f64 {
        (self.image.0.min(self.image.1) / 220.).round().max(3.)
    }
    fn point(&self, input: &Value) -> Option<Point> {
        let viewport = Rect::read(&input["viewport"])?;
        let point = Point::read(&input["point"])?;
        Some(Fit::contain(viewport, self.image.0, self.image.1)?.image(point))
    }
    pub fn snapshot(&self) -> Value {
        json!({"tool":self.tool.name(),"color":COLORS[self.color],"ops":self.ops,"draft":self.draft,
            "text":self.text.as_ref().map(|t|json!({"x":t.point.x,"y":t.point.y,"value":t.value})),
            "dragging":self.draft.is_some(),"revision":self.revision,"drawingRevision":self.drawing_revision})
    }
    pub fn request(&mut self, input: &Value) -> Value {
        match input["op"].as_str().unwrap_or("") {
            "open" => {
                if let Some((width, height)) = input["image"]["width"]
                    .as_f64()
                    .zip(input["image"]["height"].as_f64())
                {
                    if width.is_finite() && height.is_finite() && width > 0. && height > 0. {
                        *self = Self {
                            image: (width, height),
                            revision: self.revision,
                            drawing_revision: self.drawing_revision,
                            ..Default::default()
                        };
                        self.changed(true);
                    }
                }
            }
            "tool" => {
                if let Some(tool) = input["value"].as_str().and_then(Tool::parse) {
                    if tool != self.tool {
                        self.tool = tool;
                        self.changed(false);
                    }
                }
            }
            "color" => {
                if let Some(color) = COLORS
                    .iter()
                    .position(|c| Some(*c) == input["value"].as_str())
                {
                    if color != self.color {
                        self.color = color;
                        self.changed(false);
                    }
                }
            }
            "down" => {
                if let Some(p) = self.point(input) {
                    if self.tool == Tool::Text {
                        self.text = Some(Text {
                            point: p,
                            value: String::new(),
                        });
                        self.changed(false);
                    } else {
                        self.draft = Some(
                            json!({"type":self.tool.name(),"x1":p.x,"y1":p.y,"x2":p.x,"y2":p.y,
                            "color":COLORS[self.color],"width":if self.tool==Tool::Blur{6.}else{self.width()}}),
                        );
                        self.changed(true);
                    }
                }
            }
            "move" if self.draft.is_some() => {
                if let Some(p) = self.point(input) {
                    let draft = self.draft.as_mut().unwrap();
                    if draft["x2"] != p.x || draft["y2"] != p.y {
                        draft["x2"] = json!(p.x);
                        draft["y2"] = json!(p.y);
                        self.changed(true);
                    }
                }
            }
            "up" => {
                if let Some(draft) = self.draft.take() {
                    let x = draft["x2"].as_f64().unwrap() - draft["x1"].as_f64().unwrap();
                    let y = draft["y2"].as_f64().unwrap() - draft["y1"].as_f64().unwrap();
                    if x.hypot(y) > 3. {
                        self.ops.push(draft);
                    }
                    self.changed(true);
                }
            }
            "cancelDrag" => {
                if self.draft.take().is_some() {
                    self.changed(true);
                }
            }
            "undo" => {
                if self.ops.pop().is_some() {
                    self.changed(true);
                }
            }
            "text" => {
                if let (Some(text), Some(value)) = (&mut self.text, input["value"].as_str()) {
                    if text.value != value {
                        text.value = value.to_owned();
                        self.changed(false);
                    }
                }
            }
            "textSubmit" => {
                if let Some(text) = self.text.take() {
                    let drawing = !text.value.trim_matches(js_space).is_empty();
                    if drawing {
                        self.ops.push(json!({"type":"text","x1":text.point.x,"y1":text.point.y,"text":text.value,
                        "color":COLORS[self.color],"size":(self.width()*7.).max(18.)}));
                    }
                    self.changed(drawing);
                }
            }
            "textCancel" => {
                if self.text.take().is_some() {
                    self.changed(false);
                }
            }
            _ => {}
        }
        self.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor() -> Editor {
        let mut e = Editor::default();
        e.request(&json!({"op":"open","image":{"width":1000,"height":500}}));
        e
    }
    fn point(op: &str, x: f64, y: f64) -> Value {
        json!({"op":op,"point":{"x":x,"y":y},"viewport":{"x":40,"y":80,"w":500,"h":500}})
    }
    #[test]
    fn preview_is_separate_from_history_and_commits_once_in_image_coordinates() {
        let mut e = editor();
        e.request(&point("down", 90., 255.));
        let draft = e.request(&point("move", 190., 305.));
        assert!(draft["ops"].as_array().unwrap().is_empty());
        assert_eq!(
            draft["draft"],
            json!({"type":"arrow","x1":100.,"y1":100.,"x2":300.,"y2":200.,"color":"#ff3b30","width":3.})
        );
        let done = e.request(&json!({"op":"up"}));
        assert_eq!(done["ops"].as_array().unwrap().len(), 1);
        assert!(done["draft"].is_null());
        assert_eq!(done, e.request(&json!({"op":"up"})));
        assert!(e.request(&json!({"op":"undo"}))["ops"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    #[test]
    fn tiny_strokes_cancel_and_idle_pointer_movement_does_not_change_revisions() {
        let mut e = editor();
        let before = e.snapshot();
        assert_eq!(e.request(&point("move", 90., 255.)), before);
        e.request(&point("down", 90., 255.));
        e.request(&point("move", 91., 255.));
        assert!(e.request(&json!({"op":"up"}))["ops"]
            .as_array()
            .unwrap()
            .is_empty());
        e.request(&point("down", 90., 255.));
        assert!(e.request(&json!({"op":"cancelDrag"}))["draft"].is_null());
    }
    #[test]
    fn text_uses_shared_mapping_preserves_whitespace_and_honors_js_empty_text_rules() {
        let mut e = editor();
        e.request(&json!({"op":"tool","value":"text"}));
        e.request(&point("down", 90., 255.));
        e.request(&json!({"op":"text","value":"\u{feff} \n"}));
        assert!(e.request(&json!({"op":"textSubmit"}))["ops"]
            .as_array()
            .unwrap()
            .is_empty());
        e.request(&point("down", 90., 255.));
        e.request(&json!({"op":"text","value":" Olá 世界 "}));
        assert_eq!(
            e.request(&json!({"op":"textSubmit"}))["ops"][0],
            json!({"type":"text","x1":100.,"y1":100.,"text":" Olá 世界 ","color":"#ff3b30","size":21.})
        );
        e.request(&point("down", 90., 255.));
        assert!(e.request(&json!({"op":"textCancel"}))["text"].is_null());
    }
    #[test]
    fn colors_and_tools_are_closed_and_do_not_redraw_existing_annotations() {
        let mut e = editor();
        let before = e.snapshot();
        assert_eq!(before, e.request(&json!({"op":"tool","value":"unknown"})));
        assert_eq!(before, e.request(&json!({"op":"color","value":"invalid"})));
        for tool in ["arrow", "box", "line", "blur"] {
            e.request(&json!({"op":"tool","value":tool}));
            e.request(&json!({"op":"color","value":"#0a84ff"}));
            assert_eq!(e.snapshot()["drawingRevision"], before["drawingRevision"]);
        }
        assert_eq!(e.request(&point("down", 90., 255.))["draft"]["width"], 6.);
        assert_eq!(e.snapshot()["draft"]["color"], "#0a84ff");
    }
    #[test]
    fn models_are_independent_and_reopening_resets_history() {
        let mut a = editor();
        let b = editor();
        a.request(&point("down", 90., 255.));
        a.request(&point("move", 190., 305.));
        a.request(&json!({"op":"up"}));
        assert!(b.snapshot()["ops"].as_array().unwrap().is_empty());
        assert!(
            a.request(&json!({"op":"open","image":{"width":640,"height":480}}))["ops"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
