//! Short-lived capture windows. Rust owns their scene, input and deadlines;
//! the transitional transport only executes effects and lends media handles.
use axia_native::NativeApp;
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Thumbnail,
    Countdown,
    Recording,
}
impl Kind {
    fn read(v: &Value) -> Option<Self> {
        Some(match v.as_str()? {
            "thumbnail" => Self::Thumbnail,
            "countdown" => Self::Countdown,
            "recording" => Self::Recording,
            _ => return None,
        })
    }
}
struct Auxiliary {
    kind: Kind,
    args: Value,
    identity: Value,
    deadline: Option<i64>,
    hovered: bool,
    terminal: bool,
    retryable: bool,
}
impl Auxiliary {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            args: Value::Null,
            identity: Value::Null,
            deadline: None,
            hovered: false,
            terminal: false,
            retryable: false,
        }
    }
    fn sync(&mut self, args: &Value, now: i64) {
        let identity = match self.kind {
            Kind::Thumbnail => args["handle"].clone(),
            Kind::Countdown => json!([args["start"], args["secs"]]),
            Kind::Recording => args["recording"]["id"].clone(),
        };
        if self.args.is_null() || self.identity != identity {
            self.identity = identity;
            self.terminal = false;
            self.retryable = false;
            self.deadline = match self.kind {
                Kind::Thumbnail if !self.hovered => Some(now.saturating_add(8000)),
                Kind::Countdown => Some(
                    args["start"]
                        .as_i64()
                        .unwrap_or(now)
                        .saturating_add(args["secs"].as_i64().unwrap_or(0).clamp(0, 3600) * 1000),
                ),
                _ => None,
            };
        }
        self.args = args.clone();
    }
    fn effect(&mut self, op: &str) -> Value {
        if self.terminal {
            return Value::Null;
        }
        self.terminal = true;
        self.retryable = matches!(op, "edit" | "stop");
        self.deadline = None;
        json!({"kind":"captureAuxiliary", "view":match self.kind {
            Kind::Thumbnail=>"thumbnail", Kind::Countdown=>"countdown", Kind::Recording=>"recording"
        }, "op":op, "source":match self.kind {
            Kind::Thumbnail=>self.args["handle"].clone(),
            Kind::Countdown=>self.args["start"].clone(),
            Kind::Recording=>self.args["recording"]["id"].clone(),
        }})
    }
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        if input["cmd"] == "sync" {
            self.sync(&input["args"], now);
        }
        let mut effects = vec![];
        let command = input["cmd"].as_str().unwrap_or("");
        if command == "retry" && self.retryable && input["key"] == self.identity {
            self.terminal = false;
            self.retryable = false;
            if self.kind == Kind::Thumbnail && !self.hovered {
                self.deadline = Some(now.saturating_add(8000));
            }
        }
        // Keep tracking the pointer after a terminal effect: a replacement may
        // arrive before its delayed close, after the pointer has already left.
        if self.kind == Kind::Thumbnail {
            match command {
                "enter" => {
                    self.hovered = true;
                    self.deadline = None;
                }
                "leave" => {
                    self.hovered = false;
                    if !self.terminal {
                        self.deadline = Some(now.saturating_add(8000));
                    }
                }
                _ => {}
            }
        }
        if !self.terminal {
            let op = match (self.kind, command) {
                (Kind::Thumbnail, "edit") => Some("edit"),
                (Kind::Countdown, "cancel") => Some("cancel"),
                (Kind::Recording, "stop") if !self.args["recording"]["id"].is_null() => {
                    Some("stop")
                }
                _ => None,
            };
            if let Some(op) = op {
                effects.push(self.effect(op));
            } else if self.deadline.is_some_and(|due| now >= due) {
                effects.push(self.effect(if self.kind == Kind::Thumbnail {
                    "expire"
                } else {
                    "complete"
                }));
            }
        }
        json!({"tree":self.tree(now),"effects":effects})
    }
    fn refresh_at(&self, now: i64) -> Option<f64> {
        if self.terminal {
            return None;
        }
        match self.kind {
            Kind::Thumbnail => self
                .deadline
                .map(|due| due.saturating_sub(now).max(1) as f64),
            Kind::Countdown => self.deadline.map(|due| {
                let remaining = due.saturating_sub(now).max(1);
                ((remaining - 1) % 1000 + 1) as f64
            }),
            Kind::Recording if !self.args["recording"]["id"].is_null() => {
                let elapsed = now
                    .saturating_sub(self.args["recording"]["start"].as_i64().unwrap_or(now))
                    .max(0);
                Some((1000 - elapsed % 1000) as f64)
            }
            _ => None,
        }
    }
    fn tree(&self, now: i64) -> Value {
        match self.kind {
            Kind::Thumbnail => {
                json!({"kind":"view","key":"thumbnail","events":{"onClick":["edit"],"onMouseEnter":["enter"],"onMouseLeave":["leave"]},"style":{"flex":1},"children":[
                    {"kind":"media","key":"image","media":self.args["handle"],"style":{"flex":1,"pointerEvents":"none"}},
                    {"kind":"view","key":"caption","style":{"position":"absolute","left":0,"right":0,"bottom":0,"paddingHorizontal":10,"paddingVertical":5,"backgroundColor":"rgba(0,0,0,0.55)","pointerEvents":"none"},"children":[
                        {"kind":"text","label":if self.args["path"].as_str().is_some_and(|s| !s.is_empty()) {json!({"key":"screenshots.savedTo","params":{"folder":{"key":"screenshots.folder"}}})} else {json!({"key":"screenshots.copied"})},"style":{"fontSize":11,"color":"#fff"}}
                    ]}
                ]})
            }
            Kind::Countdown => {
                let remaining = self.deadline.unwrap_or(now).saturating_sub(now).max(0);
                json!({"kind":"view","key":"countdown","style":{"flex":1,"alignItems":"center","justifyContent":"center","backgroundColor":"rgba(20,20,24,0.85)"},"children":[{"kind":"text","label":((remaining+999)/1000).to_string(),"style":{"fontSize":56,"fontWeight":600,"color":"#fff"}}]})
            }
            Kind::Recording => {
                let seconds = now
                    .saturating_sub(self.args["recording"]["start"].as_i64().unwrap_or(now))
                    .max(0)
                    / 1000;
                json!({"kind":"view","key":"recording","style":{"flex":1,"flexDirection":"row","alignItems":"center","gap":10,"paddingHorizontal":12,"backgroundColor":"rgba(30,30,34,0.92)"},"children":[
                    {"kind":"view","key":"dot","style":{"width":9,"height":9,"borderRadius":5,"backgroundColor":"#ff3b30"}},
                    {"kind":"text","key":"elapsed","label":format!("{}:{:02}",seconds/60,seconds%60),"style":{"fontSize":13,"fontWeight":500,"color":"#fff","flex":1,"direction":"ltr"}},
                    {"kind":"view","key":"stop","events":{"onClick":["stop"]},"style":{"paddingHorizontal":12,"height":24,"borderRadius":6,"justifyContent":"center","backgroundColor":"#ff3b30","opacity":if self.terminal {0.5}else{1.}},"children":[{"kind":"text","label":{"key":"screenshots.stop"},"style":{"fontSize":12,"fontWeight":600,"color":"#fff","pointerEvents":"none"}}]}
                ]})
            }
        }
    }
}

#[derive(Default)]
pub struct App {
    selector: crate::selector::App,
    auxiliary: Option<Auxiliary>,
    editor: Option<crate::editor_view::App>,
    theme: std::rc::Rc<Value>,
}
impl App {
    fn request_at(&mut self, input: &Value, now: i64) -> Value {
        if input["cmd"] == "icon" {
            return crate::selector::icon(input["size"].as_f64().unwrap_or(48.));
        }
        if input["cmd"] == "sync" {
            if input["args"]["view"] == "editor" {
                let editor = self.editor.get_or_insert_with(Default::default);
                if !self.theme.is_null() {
                    editor.set_native_appearance(self.theme.clone());
                }
                return editor.request_at(input, now);
            }
            self.editor = None;
            let kind = Kind::read(&input["args"]["view"]);
            if kind != self.auxiliary.as_ref().map(|a| a.kind) {
                self.auxiliary = kind.map(Auxiliary::new);
                self.selector = Default::default();
            }
        }
        if let Some(editor) = &mut self.editor {
            return editor.request_at(input, now);
        }
        if let Some(auxiliary) = &mut self.auxiliary {
            auxiliary.request_at(input, now)
        } else {
            self.selector.request(input)
        }
    }
}
impl NativeApp for App {
    fn uses_native_appearance(&self) -> bool {
        true
    }
    fn set_native_appearance(&mut self, theme: std::rc::Rc<Value>) {
        self.theme = theme.clone();
        if let Some(editor) = &mut self.editor {
            editor.set_native_appearance(theme);
        }
    }
    fn request(&mut self, input: &Value) -> Value {
        self.request_at(input, axia_native::clock::now())
    }
    fn refresh_ms(&self) -> Option<f64> {
        if let Some(editor) = &self.editor {
            return editor.refresh_ms();
        }
        self.auxiliary
            .as_ref()
            .and_then(|a| a.refresh_at(axia_native::clock::now()))
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        self.refresh_ms()
    }
    fn key(&mut self, key: &str, chr: &str, flags: i32) -> Option<Value> {
        if let Some(editor) = &mut self.editor {
            return editor.key(key, chr, flags);
        }
        if let Some(a) = &self.auxiliary {
            if a.kind == Kind::Countdown && key == "Escape" {
                return Some(self.request(&json!({"cmd":"cancel"})));
            }
            None
        } else {
            self.selector.key(key, chr, flags)
        }
    }
    fn blur(&mut self) {
        if let Some(editor) = &mut self.editor {
            editor.blur();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumbnail_hover_replacement_and_terminal_delivery() {
        let mut app = App::default();
        let sync = json!({"cmd":"sync","args":{"view":"thumbnail","handle":3,"path":null}});
        app.request_at(&sync, 1000);
        assert_eq!(
            app.auxiliary.as_ref().unwrap().refresh_at(1000),
            Some(8000.)
        );
        app.request_at(&json!({"cmd":"enter"}), 8999);
        assert_eq!(app.auxiliary.as_ref().unwrap().refresh_at(12000), None);
        assert_eq!(app.request_at(&sync, 12000)["effects"], json!([]));
        app.request_at(&json!({"cmd":"leave"}), 12000);
        assert_eq!(app.request_at(&sync, 19999)["effects"], json!([]));
        let done = app.request_at(&sync, 20000);
        assert_eq!(done["effects"][0]["op"], "expire");
        assert_eq!(
            app.request_at(&json!({"cmd":"edit"}), 20000)["effects"],
            json!([])
        );
        let new = json!({"cmd":"sync","args":{"view":"thumbnail","handle":4,"path":"/saved.png"}});
        app.request_at(&new, 21000);
        app.request_at(&json!({"cmd":"enter"}), 21500);
        let edit = app.request_at(&json!({"cmd":"edit"}), 22000);
        assert_eq!(edit["effects"][0]["source"], 4);
        assert_eq!(edit["effects"][0]["op"], "edit");
        assert_eq!(app.auxiliary.as_ref().unwrap().refresh_at(30000), None);
        app.request_at(&json!({"cmd":"leave"}), 31000);
        app.request_at(
            &json!({"cmd":"sync","args":{"view":"thumbnail","handle":5}}),
            32000,
        );
        assert_eq!(
            app.auxiliary.as_ref().unwrap().refresh_at(32000),
            Some(8000.)
        );
    }
    #[test]
    fn countdown_boundaries_cancel_and_completion_without_focus() {
        let mut app = App::default();
        let sync = json!({"cmd":"sync","args":{"view":"countdown","start":1000,"secs":5}});
        let r = app.request_at(&sync, 1501);
        assert_eq!(r["tree"]["children"][0]["label"], "5");
        assert_eq!(app.auxiliary.as_ref().unwrap().refresh_at(1501), Some(499.));
        assert_eq!(
            app.request_at(&sync, 2000)["tree"]["children"][0]["label"],
            "4"
        );
        assert_eq!(app.request_at(&sync, 6000)["effects"][0]["op"], "complete");
        assert_eq!(app.request_at(&sync, 7000)["effects"], json!([]));
        app.request_at(
            &json!({"cmd":"sync","args":{"view":"countdown","start":8000,"secs":10}}),
            8000,
        );
        assert_eq!(
            app.request_at(&json!({"cmd":"cancel"}), 8500)["effects"][0]["op"],
            "cancel"
        );
        assert_eq!(app.auxiliary.as_ref().unwrap().refresh_at(9000), None);
    }
    #[test]
    fn recording_ticks_at_second_boundaries_and_stops_once() {
        let mut app = App::default();
        let sync =
            json!({"cmd":"sync","args":{"view":"recording","recording":{"id":9,"start":1000}}});
        assert_eq!(
            app.request_at(&sync, 62600)["tree"]["children"][1]["label"],
            "1:01"
        );
        assert_eq!(
            app.auxiliary.as_ref().unwrap().refresh_at(62600),
            Some(400.)
        );
        assert_eq!(
            app.request_at(&json!({"cmd":"stop"}), 62700)["effects"][0]["source"],
            9
        );
        assert_eq!(
            app.request_at(&json!({"cmd":"stop"}), 62800)["effects"],
            json!([])
        );
        assert_eq!(app.auxiliary.as_ref().unwrap().refresh_at(63000), None);
        app.request_at(&json!({"cmd":"retry","key":8}), 63000);
        assert_eq!(app.auxiliary.as_ref().unwrap().refresh_at(63000), None);
        app.request_at(&json!({"cmd":"retry","key":9}), 63000);
        assert_eq!(
            app.auxiliary.as_ref().unwrap().refresh_at(63000),
            Some(1000.)
        );
        assert_eq!(
            app.request_at(&json!({"cmd":"stop"}), 63100)["effects"][0]["op"],
            "stop"
        );
        app.request_at(
            &json!({"cmd":"sync","args":{"view":"recording","recording":null}}),
            64000,
        );
        assert_eq!(app.auxiliary.as_ref().unwrap().refresh_at(64000), None);
        assert_eq!(
            app.request_at(&json!({"cmd":"stop"}), 64000)["effects"],
            json!([])
        );
    }
}
