//! Native markup scene and image-job protocol. Pixel handles are lent by the
//! transport; tools, coordinates, result messages and deadlines stay here.
use crate::{geometry, markup};
use axia_native::NativeApp;
use serde_json::{json, Value};
use std::rc::Rc;

pub struct App {
    model: markup::Editor,
    source: i64,
    preview: i64,
    image: Value,
    theme: Rc<Value>,
    width: f64,
    height: f64,
    drawing: Option<u64>,
    text_serial: u64,
    token: u64,
    pending: Option<u64>,
    message: Option<(&'static str, i64)>,
    closed: bool,
}
impl Default for App {
    fn default() -> Self {
        Self {
            model: Default::default(),
            source: 0,
            preview: 0,
            image: Value::Null,
            theme: Rc::new(
                json!({"mode":"light","text":"#222","accent":"#0a84ff","separator":"#ccc","toolbar":"#eee"}),
            ),
            width: 1000.,
            height: 680.,
            drawing: None,
            text_serial: 0,
            token: 0,
            pending: None,
            message: None,
            closed: false,
        }
    }
}
impl App {
    fn toolbar_height(&self) -> f64 {
        if self.width < 700. {
            72.
        } else {
            44.
        }
    }
    fn viewport(&self) -> Value {
        json!({"x":0,"y":self.toolbar_height(),"w":self.width,"h":(self.height-self.toolbar_height()).max(1.)})
    }
    fn update(&mut self, op: &str, value: Value) {
        self.model.request(&json!({"op":op,"value":value}));
    }
    fn notice(&mut self, key: &'static str, now: i64) {
        self.message = Some((key, now.saturating_add(2500)));
    }
    pub fn request_at(&mut self, input: &Value, now: i64) -> Value {
        let mut effects = vec![];
        let cmd = input["cmd"].as_str().unwrap_or("");
        if cmd == "sync" {
            self.width = input["context"]["width"]
                .as_f64()
                .unwrap_or(self.width)
                .max(1.);
            self.height = input["context"]["height"]
                .as_f64()
                .unwrap_or(self.height)
                .max(1.);
            let source = input["args"]["handle"].as_i64().unwrap_or(0);
            if source != self.source {
                self.source = source;
                self.preview = 0;
                self.image = input["args"]["info"].clone();
                self.model.request(&json!({"op":"open","image":self.image}));
                self.drawing = None;
                self.pending = None;
                self.message = None;
                self.closed = false;
                self.text_serial = self.text_serial.wrapping_add(1);
            }
        } else if !self.closed {
            match cmd {
                "tool" | "color" => self.update(cmd, input["key"].clone()),
                "text" => self.update(cmd, input["value"].clone()),
                "undo" | "textSubmit" | "textCancel" => self.update(cmd, Value::Null),
                "down" if input["button"].as_i64().unwrap_or(0) == 0 => {
                    if self.model.snapshot()["tool"] == "text" {
                        self.text_serial = self.text_serial.wrapping_add(1);
                    }
                    self.model.request(&json!({"op":"down","point":{"x":input["x"],"y":input["y"]},"viewport":self.viewport()}));
                }
                "pointerMove" => {
                    self.model.request(&json!({"op":"move","point":{"x":input["x"],"y":input["y"]},"viewport":self.viewport()}));
                }
                "dragEnd" => self.update("up", Value::Null),
                "save" | "copy" if self.pending.is_none() && self.source > 0 => {
                    self.token = self.token.wrapping_add(1);
                    self.pending = Some(self.token);
                    effects.push(json!({"kind":"captureEditor","op":cmd,"source":self.source,"image":if self.preview>0{self.preview}else{self.source},"token":self.token}));
                }
                "done" => {
                    self.closed = true;
                    effects.push(json!({"kind":"captureEditor","op":"close","source":self.source}));
                }
                "editorResult" => {
                    let r = &input["key"];
                    if r["source"] == self.source {
                        if r["op"] == "annotate" && r["revision"].as_u64() == self.drawing {
                            if let Some(handle) = r["handle"].as_i64().filter(|h| *h > 0) {
                                self.preview = handle;
                            } else {
                                self.notice("screenshots.saveError", now);
                            }
                        } else if r["token"].as_u64() == self.pending && self.pending.is_some() {
                            self.pending = None;
                            self.notice(
                                if r["ok"] != true {
                                    "screenshots.saveError"
                                } else if r["op"] == "copy" {
                                    "screenshots.copied"
                                } else {
                                    "screenshots.savedTo"
                                },
                                now,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        if self.message.is_some_and(|(_, due)| now >= due) {
            self.message = None;
        }
        let state = self.model.snapshot();
        let revision = state["drawingRevision"].as_u64().unwrap_or(0);
        if !self.closed && self.source > 0 && self.drawing != Some(revision) {
            self.drawing = Some(revision);
            let mut ops = state["ops"].as_array().cloned().unwrap_or_default();
            if !state["draft"].is_null() {
                ops.push(state["draft"].clone());
            }
            effects.push(json!({"kind":"captureEditor","op":"annotate","source":self.source,"revision":revision,"ops":ops}));
        }
        json!({"tree":self.tree(&state),"capture":state["dragging"],"effects":effects})
    }
    fn control(&self, id: &str, glyph: &str, label: &str, active: bool, disabled: bool) -> Value {
        json!({"kind":"view","key":id,"tooltip":{"key":label},"events":{"onClick":if disabled{json!(["noop"])}else{json!([id])}},"hover":{"opacity":0.8},"style":{"height":28,"minWidth":28,"paddingHorizontal":4,"alignItems":"center","justifyContent":"center","borderRadius":5,"backgroundColor":if active {self.theme["accent"].clone()}else{json!("transparent")},"opacity":if disabled{0.4}else{1.}},"children":[{"kind":"text","label":if ["undo","copy","done","save"].contains(&id){json!({"key":label})}else{json!(glyph)},"style":{"fontSize":if ["undo","copy","done","save"].contains(&id){12}else{16},"color":if active{json!("#fff")}else{self.theme["text"].clone()},"pointerEvents":"none"}}]})
    }
    fn tree(&self, state: &Value) -> Value {
        let mut tools = vec![];
        for (tool, glyph) in [
            ("arrow", "↗"),
            ("box", "▢"),
            ("text", "T"),
            ("blur", "#"),
            ("line", "/"),
        ] {
            let mut button = self.control(
                tool,
                glyph,
                &format!("screenshots.tool.{tool}"),
                state["tool"] == tool,
                false,
            );
            button["events"]["onClick"] = json!(["tool", tool]);
            tools.push(button);
        }
        let colors=markup::COLORS.iter().map(|color| json!({"kind":"view","key":color,"tooltip":color,"events":{"onClick":["color",color]},"style":{"width":22,"height":22,"borderRadius":11,"alignItems":"center","justifyContent":"center","borderWidth":2,"borderColor":if state["color"]==*color {self.theme["accent"].clone()}else{json!("transparent")}},"children":[{"kind":"view","style":{"width":16,"height":16,"borderRadius":8,"backgroundColor":color,"borderWidth":1,"borderColor":self.theme["separator"],"pointerEvents":"none"}}]})).collect::<Vec<_>>();
        let left = json!({"kind":"view","key":"tools-colors","style":{"height":28,"flexDirection":"row","alignItems":"center","gap":8},"children":[{"kind":"view","style":{"flexDirection":"row","gap":2},"children":tools},{"kind":"view","style":{"flexDirection":"row","gap":4,"alignItems":"center"},"children":colors}]});
        let actions = vec![
            self.control(
                "undo",
                "⟲",
                "screenshots.undo",
                false,
                state["ops"].as_array().is_none_or(|a| a.is_empty()),
            ),
            self.control(
                "copy",
                "⧉",
                "screenshots.copy",
                false,
                self.pending.is_some(),
            ),
            self.control("done", "", "screenshots.done", false, false),
            self.control("save", "", "screenshots.save", true, self.pending.is_some()),
        ];
        let toolbar = json!({"kind":"view","key":"toolbar","style":{"height":self.toolbar_height(),"flexShrink":0,"paddingHorizontal":6,"paddingVertical":6,"flexDirection":if self.width<700.{"column"}else{"row"},"alignItems":if self.width<700.{"stretch"}else{"center"},"justifyContent":"space-between","gap":4,"backgroundColor":self.theme["toolbar"]},"children":[left,{"kind":"view","key":"actions","style":{"height":28,"flexDirection":"row","alignItems":"center","gap":6},"children":actions}]});
        let mut canvas = vec![
            json!({"kind":"media","key":"preview","media":if self.preview>0{self.preview}else{self.source},"style":{"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"pointerEvents":"none"}}),
        ];
        if !state["text"].is_null() {
            let viewport = self.viewport();
            let anchor = geometry::request(
                &json!({"cmd":"anchor","point":state["text"],"image":self.image,"viewport":{"x":0,"y":0,"w":viewport["w"],"h":viewport["h"]}}),
            );
            canvas.push(json!({"kind":"view","key":"text-position","style":{"position":"absolute","left":anchor["left"].as_f64().unwrap_or(0.).clamp(0.,(self.width-260.).max(0.)),"top":anchor["top"].as_f64().unwrap_or(0.).clamp(0.,(self.height-self.toolbar_height()-28.).max(0.)),"width":260f64.min(self.width)},"children":[{"kind":"input","key":format!("markup-text-{}",self.text_serial),"value":state["text"]["value"],"autoFocus":true,"placeholder":{"key":"screenshots.textHint"},"change":["text"],"submit":["textSubmit"],"cancel":["textCancel"],"style":{"minHeight":28,"width":"100%"}}]}));
        }
        if let Some((key, _)) = self.message {
            canvas.push(json!({"kind":"view","key":"result","style":{"position":"absolute","left":0,"right":0,"bottom":20,"alignItems":"center","pointerEvents":"none"},"children":[{"kind":"text","label":{"key":key,"params":{"folder":{"key":"screenshots.folder"}}},"style":{"paddingHorizontal":14,"paddingVertical":8,"borderRadius":15,"backgroundColor":"rgba(0,0,0,0.75)","color":"#fff","fontSize":12}}]}));
        }
        json!({"kind":"view","key":"editor","style":{"flex":1},"children":[toolbar,{"kind":"view","key":"canvas","events":{"onClick":["noop"],"onMouseDown":["down"]},"style":{"flex":1,"overflow":"hidden","direction":"ltr","backgroundColor":if self.theme["mode"]=="dark"{"#111"}else{"#ddd"}},"children":canvas}]})
    }
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool {
        true
    }
    fn set_native_appearance(&mut self, theme: Rc<Value>) {
        self.theme = theme;
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, axia_native::clock::now())
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.message
            .map(|(_, due)| due.saturating_sub(axia_native::clock::now()).max(1) as f64)
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        self.refresh_ms()
    }
    fn blur(&mut self) {
        self.update("cancelDrag", Value::Null);
    }
    fn key(&mut self, key: &str, _chr: &str, flags: i32) -> Option<Value> {
        (flags & (2 | 16) != 0 && key.eq_ignore_ascii_case("z"))
            .then(|| self.request(&json!({"cmd":"undo"})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sync(handle: i64) -> Value {
        json!({"cmd":"sync","context":{"width":380,"height":360},"args":{"handle":handle,"info":{"width":1000,"height":500}}})
    }
    #[test]
    fn scene_owns_coordinates_and_only_drawing_changes_emit_rasters() {
        let mut app = App::default();
        assert_eq!(app.request_at(&sync(8), 0)["effects"][0]["op"], "annotate");
        assert_eq!(app.request_at(&sync(8), 1)["effects"], json!([]));
        app.request_at(&json!({"cmd":"color","key":"#0a84ff"}), 2);
        let r = app.request_at(&json!({"cmd":"down","x":38,"y":180}), 3);
        assert_eq!(r["capture"], true);
        app.request_at(&json!({"cmd":"pointerMove","x":190,"y":220}), 4);
        let r = app.request_at(&json!({"cmd":"dragEnd"}), 5);
        assert_eq!(r["effects"][0]["ops"][0]["color"], "#0a84ff");
        assert_eq!(app.request_at(&sync(8), 6)["effects"], json!([]));
        app.blur();
        assert_eq!(app.model.snapshot()["dragging"], false);
    }
    #[test]
    fn replacement_and_latest_job_guard_messages_and_preview() {
        let mut app = App::default();
        app.request_at(&sync(8), 0);
        let save = app.request_at(&json!({"cmd":"save"}), 1)["effects"][0].clone();
        assert_eq!(
            app.request_at(&json!({"cmd":"copy"}), 2)["effects"],
            json!([])
        );
        app.request_at(&sync(9), 3);
        app.request_at(&json!({"cmd":"editorResult","key":{"source":8,"token":save["token"],"ok":true,"op":"save"}}),4);
        assert!(app.message.is_none());
        let copy = app.request_at(&json!({"cmd":"copy"}), 5)["effects"][0].clone();
        app.request_at(&json!({"cmd":"editorResult","key":{"source":9,"token":copy["token"],"ok":true,"op":"copy"}}),6);
        assert_eq!(app.message.unwrap().0, "screenshots.copied");
        app.request_at(&sync(9), 2506);
        assert!(app.message.is_none());
        app.request_at(&json!({"cmd":"done"}), 2507);
        assert_eq!(
            app.request_at(&json!({"cmd":"save"}), 2508)["effects"],
            json!([])
        );
    }
}
