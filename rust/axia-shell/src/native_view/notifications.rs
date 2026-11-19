//! Banners read their context's notification state, never a JS list or tree.
use crate::notifications::{Service, State};
use axia_state::{Reader, Subscription};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc, time::Instant};

pub struct Banners {
    service: Rc<RefCell<Service>>,
    state: Reader<State>,
    changes: Subscription,
    last: Value,
}
impl Banners {
    pub fn new(service: Rc<RefCell<Service>>) -> Self {
        let state = service.borrow().reader();
        let changes = state.subscribe();
        Self {
            service,
            state,
            changes,
            last: Value::Null,
        }
    }
    fn tree(&self, input: &Value) -> Value {
        let th = &input["context"]["theme"];
        let light = th["mode"] == "light";
        let color = |key: &str, fallback: &str| th[key].as_str().unwrap_or(fallback).to_owned();
        let bg = if light {
            "#f6f6f8".to_owned()
        } else {
            color("windowBg", "#252529")
        };
        let fg = if light {
            "#1d1d1f".to_owned()
        } else {
            color("text", "#fff")
        };
        let dim = if light {
            "rgba(60,60,67,0.62)".to_owned()
        } else {
            color("textDim", "#aaa")
        };
        let separator = if light {
            "rgba(0,0,0,0.12)".to_owned()
        } else {
            color("separator", "#555")
        };
        let control = if light {
            "#e2e2e7".to_owned()
        } else {
            color("control", "#444")
        };
        let accent = color("accent", "#3489bd");
        let state = self.state.read();
        let children:Vec<_>=state.grouped().into_iter().map(|(notice,more)| {
            let n=&notice.data;let id=notice.id();
            let text=|label:Value,style:Value|json!({"kind":"text","label":label,"style":style});
            let actions:Vec<_>=n["actions"].as_array().into_iter().flatten().enumerate().map(|(i,a)|{
                let primary=a["primary"]==true;
                json!({"kind":"view","key":format!("a{i}"),"style":{"height":26,"paddingHorizontal":12,
                    "borderRadius":7,"justifyContent":"center","backgroundColor":if primary {&accent} else {&control}},
                    "children":[text(a["label"].clone(),json!({"fontSize":12,"fontWeight":600,"color":if primary {"#fff"} else {&fg}}))],
                    "events":{"onClick":["act",format!("{id}:{i}")]}})
            }).collect();
            let icon=if n["hasApp"]==true {json!({"kind":"appIcon","app":n["app"],"size":34})}
                else {json!({"kind":"view","key":"icon","style":{"width":34,"height":34,"borderRadius":8,"backgroundColor":accent}})};
            let name=n["displayLabel"].as_str().unwrap_or("");
            let name=if more>0 {format!("{name} · +{more}")} else {name.to_owned()};
            let mut body=vec![json!({"kind":"view","key":"line","style":{"flexDirection":"row","gap":6},"children":[
                text(json!(name),json!({"flex":1,"fontSize":11,"color":dim,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start"})),
                text(n["timeLabel"].clone(),json!({"fontSize":11,"color":dim}))]}),
                text(n["title"].clone(),json!({"fontSize":13,"fontWeight":700,"color":fg,"textAlign":"start"}))];
            if n["body"].as_str().is_some_and(|s|!s.is_empty()) {
                body.push(text(n["body"].clone(),json!({"fontSize":12,"color":fg,"lineHeight":1.35,"textAlign":"start"})));
            }
            let mut children=vec![json!({"kind":"view","key":"head","style":{"flexDirection":"row","gap":10,"alignItems":"flex-start"},"children":[
                icon,json!({"kind":"view","key":"body","style":{"flex":1,"gap":2},"children":body}),
                json!({"kind":"view","key":"close","style":{"width":18,"height":18,"borderRadius":9,"alignItems":"center","justifyContent":"center","backgroundColor":"transparent"},
                    "hoverStyle":{"backgroundColor":control},"children":[text(json!("×"),json!({"fontSize":11,"color":dim}))],"events":{"onClick":["dismiss",id.to_string()]}})
            ]})];
            let has_actions=!actions.is_empty();
            if has_actions { children.push(json!({"kind":"view","key":"actions","style":{"flexDirection":"row","gap":8,"justifyContent":"flex-end"},"children":actions})); }
            json!({"kind":"view","key":id.to_string(),"style":{"width":340,"padding":12,"gap":8,"borderRadius":14,
                "backgroundColor":bg,"border":format!("1px solid {separator}"),"boxShadow":"0 10px 30px rgba(0,0,0,0.4)"},"children":children,
                "events":{"onClick":if has_actions {json!(["act",format!("{id}:0")])} else {json!(["dismiss",id.to_string()])}}})
        }).collect();
        json!({"kind":"view","key":"banners","style":{"width":340,"gap":10},"children":children})
    }
}
impl super::NativeApp for Banners {
    fn invalidated(&self) -> bool {
        self.changes.pending()
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.service.borrow().wait_ms(Instant::now())
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        self.refresh_ms()
    }
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str() {
            Some("sync") => {
                let mut service = self.service.borrow_mut();
                service.configure(input["args"]["ungrouped"] == true);
                service.expire(Instant::now());
                let effects = service.effects();
                drop(service);
                self.changes.take();
                let tree = self.tree(input);
                if tree == self.last {
                    json!({"same":true,"effects":effects})
                } else {
                    self.last = tree.clone();
                    json!({"tree":tree,"effects":effects})
                }
            }
            Some(cmd @ ("act" | "dismiss")) => {
                let key = input["key"].as_str().unwrap_or("");
                let (id, index) = key.split_once(':').unwrap_or((key, "0"));
                let (Ok(id), Ok(index)) = (id.parse::<u64>(), index.parse::<u64>()) else {
                    return json!({});
                };
                self.service
                    .borrow_mut()
                    .command(&json!({"action":cmd,"id":id,"index":index}), Instant::now())
            }
            _ => json!({}),
        }
    }
}
