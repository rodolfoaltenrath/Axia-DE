//! Capture panel. Preferences are borrowed from the native settings source;
//! the adapter supplies raw session/window facts and executes closed effects.
use axia_native::NativeApp;
use serde_json::{json, Value};
use std::rc::Rc;

pub struct App {
    theme: Rc<Value>,
    options: Value,
    facts: Value,
    session: Value,
    rtl: bool,
}
impl Default for App {
    fn default() -> Self {
        Self {
            theme: Rc::new(
                json!({"mode":"light","windowBg":"#fff","text":"#222","textDim":"#666","accent":"#0a84ff","separator":"#ddd","control":"#ddd"}),
            ),
            options: json!({"timer":0,"clipboard":false,"mic":false}),
            facts: Value::Null,
            session: Value::Null,
            rtl: false,
        }
    }
}
fn key(k: &str) -> Value {
    json!({"key":format!("screenshots.{k}")})
}
fn view(id: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":id,"style":style,"children":children})
}
fn text(label: Value, size: i64, color: &Value) -> Value {
    json!({"kind":"text","label":label,"style":{"fontSize":size,"color":color,"textAlign":"start","lineHeight":1.4}})
}
fn protection(facts: &Value) -> Option<(&str, bool)> {
    facts["windows"].as_array()?.iter().rev().find_map(|w| {
        let current = w["desktop"].is_null()
            || w["desktop"] == facts["desktop"]
            || w["sticky"] == true
            || w["pip"] == true;
        (w["app"] != "screenshots"
            && w["minimized"] != true
            && w["hidden"] != true
            && w["visible"] != false
            && current)
            .then(|| w["id"].as_str().map(|id| (id, w["protected"] != true)))
            .flatten()
    })
}
impl App {
    fn preferences(&mut self, prefs: &Value) {
        self.options = json!({"timer":0,"clipboard":false,"mic":false});
        if let Some(options) = prefs["screenshots"].as_object() {
            self.options
                .as_object_mut()
                .unwrap()
                .extend(options.clone());
        }
    }
    fn request_with(&mut self, input: &Value, prefs: Option<&Value>) -> Value {
        let mut effects = vec![];
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                if let Some(prefs) = prefs.or_else(|| input.get("settings")) {
                    self.preferences(prefs);
                }
                self.session = input["args"]["session"].clone();
                self.facts = input["args"]["facts"].clone();
                self.rtl = input["context"]["direction"] == "rtl";
            }
            "timer" => {
                if input["key"]
                    .as_i64()
                    .is_some_and(|v| [0, 5, 10].contains(&v))
                {
                    self.options["timer"] = input["key"].clone();
                    effects.push(json!({"kind":"settings","patch":{"screenshots":self.options}}));
                }
            }
            "toggle" => {
                if let Some(k) = input["key"]
                    .as_str()
                    .filter(|k| ["clipboard", "mic"].contains(k))
                {
                    self.options[k] = json!(self.options[k] != true);
                    effects.push(json!({"kind":"settings","patch":{"screenshots":self.options}}));
                }
            }
            "capture" => {
                if let Some(mode) = input["key"]
                    .as_str()
                    .filter(|m| ["full", "window", "area", "record"].contains(m))
                {
                    effects.push(json!({"kind":"capturePanel","op":"capture","mode":mode}));
                }
            }
            "recent" => {
                if let Some(item) = self.session["recent"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|r| {
                        r["path"] == input["key"]
                            && r["path"].as_str().is_some_and(|p| !p.is_empty())
                    })
                {
                    effects.push(json!({"kind":"capturePanel","op":"recent","app":if item["video"]==true{"player"}else{"photos"},"path":item["path"]}));
                }
            }
            "protectFacts" => {
                // The menu adapter sends fresh facts at dispatch, rather than
                // trusting the last rendered window order or desktop.
                self.facts = input["facts"].clone();
                if let Some((id, on)) = protection(&self.facts) {
                    effects.push(json!({"kind":"capturePanel","op":"protect","id":id,"on":on}));
                }
            }
            _ => {}
        }
        json!({"tree":self.tree(),"effects":effects,"capture":false})
    }
    fn icon(&self, mode: &str) -> Value {
        let color = if mode == "record" {
            json!("#ff3b30")
        } else {
            self.theme["text"].clone()
        };
        let shape = |x: f64, y: f64, w: f64, h: f64| {
            view(
                "",
                json!({"position":"absolute","left":x,"top":y,"width":w,"height":h,"backgroundColor":color,"pointerEvents":"none"}),
                vec![],
            )
        };
        let children = match mode {
            "record" => vec![view(
                "record-dot",
                json!({"width":14,"height":14,"borderRadius":if self.session["recording"].is_object()||self.session["pendingRecord"]==true {2}else{7},"backgroundColor":color}),
                vec![],
            )],
            "area" => vec![
                shape(0., 0., 7., 2.),
                shape(0., 0., 2., 7.),
                shape(13., 0., 7., 2.),
                shape(18., 0., 2., 7.),
                shape(0., 13., 2., 7.),
                shape(0., 18., 7., 2.),
                shape(13., 18., 7., 2.),
                shape(18., 13., 2., 7.),
            ],
            _ => vec![
                view(
                    "frame",
                    json!({"position":"absolute","left":if mode=="window"{4}else{0},"top":if mode=="window"{5}else{3},"width":18,"height":13,"borderWidth":2,"borderColor":color,"borderRadius":2}),
                    vec![],
                ),
                if mode == "window" {
                    view(
                        "back-frame",
                        json!({"position":"absolute","left":0,"top":1,"width":18,"height":13,"borderTopWidth":2,"borderStartWidth":2,"borderColor":color,"borderRadius":2}),
                        vec![],
                    )
                } else {
                    shape(6., 18., 8., 2.)
                },
            ],
        };
        view(
            "mode-icon",
            json!({"width":22,"height":22,"alignItems":"center","justifyContent":"center","pointerEvents":"none"}),
            children,
        )
    }
    fn mode(&self, mode: &str) -> Value {
        let label = if mode == "record" && self.session["recording"].is_object() {
            "mode.stop"
        } else if mode == "record" && self.session["pendingRecord"] == true {
            "selector.cancel"
        } else {
            match mode {
                "full" => "mode.full",
                "window" => "mode.window",
                "area" => "mode.area",
                _ => "mode.record",
            }
        };
        let mut node = view(
            mode,
            json!({"flex":1,"minWidth":0,"height":82,"borderRadius":10,"alignItems":"center","gap":6,"paddingHorizontal":4,"paddingTop":16,"backgroundColor":if mode=="record"{"rgba(255,59,48,0.14)"}else if self.theme["mode"]=="dark"{"rgba(255,255,255,0.06)"}else{"rgba(0,0,0,0.04)"},"borderWidth":1,"borderColor":self.theme["separator"]}),
            vec![self.icon(mode), text(key(label), 11, &self.theme["text"])],
        );
        node["events"] = json!({"onClick":["capture",mode]});
        node
    }
    fn toggle(&self, k: &str) -> Value {
        let on = self.options[k] == true;
        let mut node = view(
            k,
            json!({"width":32,"height":18,"padding":2,"borderRadius":9,"flexDirection":"row","alignItems":"center","justifyContent":if on{"flex-end"}else{"flex-start"},"backgroundColor":if on{self.theme["accent"].clone()}else{self.theme["control"].clone()}}),
            vec![view(
                "knob",
                json!({"width":14,"height":14,"borderRadius":7,"backgroundColor":"#fff","pointerEvents":"none"}),
                vec![],
            )],
        );
        node["events"] = json!({"onClick":["toggle",k]});
        node
    }
    fn row(&self, id: &str, label: &str, control: Value) -> Value {
        let mut caption = text(key(label), 13, &self.theme["text"]);
        caption["style"]["flex"] = json!(1);
        view(
            id,
            json!({"minHeight":42,"flexDirection":"row","alignItems":"center","gap":12}),
            vec![caption, control],
        )
    }
    fn tree(&self) -> Value {
        let timers=[0,5,10].into_iter().map(|secs| {
            let selected=self.options["timer"].as_i64().unwrap_or(0)==secs;
            let mut node=view(&format!("timer-{secs}"),json!({"height":24,"minWidth":40,"paddingHorizontal":6,"borderRadius":5,"alignItems":"center","justifyContent":"center","backgroundColor":if selected{self.theme["accent"].clone()}else{self.theme["control"].clone()}}),vec![text(if secs==0{key("timerNone")}else{json!(format!("{secs} s"))},12,&if selected{json!("#fff")}else{self.theme["text"].clone()})]);
            node["events"]=json!({"onClick":["timer",secs]});node
        }).collect();
        let mut content = vec![
            view(
                "modes",
                json!({"flexDirection":"row","gap":8}),
                ["full", "window", "area", "record"]
                    .map(|m| self.mode(m))
                    .into(),
            ),
            view(
                "options",
                json!({"paddingHorizontal":12,"borderWidth":1,"borderColor":self.theme["separator"],"borderRadius":10}),
                vec![
                    self.row(
                        "timer-row",
                        "timer",
                        view("timer", json!({"flexDirection":"row","gap":3}), timers),
                    ),
                    self.row("clipboard-row", "toClipboard", self.toggle("clipboard")),
                    self.row("mic-row", "mic", self.toggle("mic")),
                ],
            ),
            text(key("shortcuts"), 11, &self.theme["textDim"]),
        ];
        let count = self.facts["windows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|w| w["protected"] == true)
            .count();
        if count > 0 {
            content.push(text(
                json!({"key":"screenshots.protectedNote","params":{"count":count}}),
                11,
                &self.theme["textDim"],
            ));
        }
        let entries = self.session["recent"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["path"].as_str().is_some_and(|p| !p.is_empty()))
            .take(5)
            .map(|r| {
                let path = r["path"].as_str().unwrap();
                let mut node = text(
                    json!(path.rsplit('/').next().unwrap_or(path)),
                    13,
                    &self.theme["text"],
                );
                node["style"]["whiteSpace"] = json!("nowrap");
                node["style"]["overflow"] = json!("hidden");
                node["style"]["minWidth"] = json!(0);
                let mut row = view(
                    path,
                    json!({"height":30,"justifyContent":"center"}),
                    vec![node],
                );
                row["events"] = json!({"onClick":["recent",path]});
                row
            })
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            let mut rows = vec![text(key("recent"), 12, &self.theme["textDim"])];
            rows.extend(entries);
            content.push(view("recent",json!({"padding":12,"borderWidth":1,"borderColor":self.theme["separator"],"borderRadius":10}),rows));
        }
        json!({"kind":"scroll","key":"panel-scroll","style":{"flex":1,"backgroundColor":self.theme["windowBg"],"direction":if self.rtl{"rtl"}else{"ltr"}},"children":[view("panel",json!({"padding":20,"gap":16,"minHeight":"100%"}),content)]})
    }
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool {
        true
    }
    fn uses_preferences(&self) -> bool {
        true
    }
    fn set_native_appearance(&mut self, theme: Rc<Value>) {
        self.theme = theme;
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_with(input, None)
    }
    fn sync_with_preferences(&mut self, input: &Value, prefs: &Value) -> Value {
        self.request_with(input, Some(prefs))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preference_patches_preserve_other_keys_and_reject_unknown_controls() {
        let mut app = App::default();
        app.sync_with_preferences(
            &json!({"cmd":"sync"}),
            &json!({"screenshots":{"timer":10,"clipboard":false,"future":7}}),
        );
        let r = app.request(&json!({"cmd":"toggle","key":"mic"}));
        assert_eq!(r["effects"][0]["patch"]["screenshots"]["future"], 7);
        assert_eq!(r["effects"][0]["patch"]["screenshots"]["mic"], true);
        assert_eq!(
            app.request(&json!({"cmd":"timer","key":3}))["effects"],
            json!([])
        );
        assert_eq!(
            app.request(&json!({"cmd":"toggle","key":"future"}))["effects"],
            json!([])
        );
    }
    #[test]
    fn privacy_uses_fresh_visible_window_order_including_sticky_and_pip() {
        let mut app = App::default();
        let r=app.request(&json!({"cmd":"protectFacts","facts":{"desktop":1,"windows":[{"id":"first","app":"notes","desktop":1},{"id":"pip","app":"player","desktop":2,"pip":true,"protected":true},{"id":"hidden","app":"notes","hidden":true},{"id":"self","app":"screenshots"}]}}));
        assert_eq!(r["effects"][0]["id"], "pip");
        assert_eq!(r["effects"][0]["on"], false);
        assert_eq!(
            app.request(&json!({"cmd":"protectFacts","facts":{"windows":[]}}))["effects"],
            json!([])
        );
    }
    #[test]
    fn recent_actions_revalidate_the_path_and_choose_the_native_viewer() {
        let mut app = App::default();
        app.request(&json!({"cmd":"sync","args":{"session":{"recent":[{"path":"/clip.mp4","video":true}]}}}));
        assert_eq!(
            app.request(&json!({"cmd":"recent","key":"/old.png"}))["effects"],
            json!([])
        );
        assert_eq!(
            app.request(&json!({"cmd":"recent","key":"/clip.mp4"}))["effects"][0]["app"],
            "player"
        );
    }
}
