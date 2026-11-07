//! Clipboard panel controller and widget descriptions. The host only resolves
//! existing theme/locale tokens, text-field focus and native paste effects.
use super::MAX_ITEMS;
use serde_json::{json, Value};
use std::cell::RefCell;
const FILTERS: [&str; 8] = [
    "all", "text", "link", "image", "code", "color", "file", "pinned",
];
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn action(mut node: Value, events: Value) -> Value {
    node["events"] = events;
    node
}
fn glyph(category: &str) -> &str {
    match category {
        "link" => "🔗",
        "image" => "🖼",
        "code" => "‹›",
        "color" => "●",
        "file" => "📁",
        _ => "¶",
    }
}
fn swatch(color: &str, size: usize) -> Value {
    view(
        json!({"width":size,"height":size,"borderRadius":3,"backgroundColor":color,"border":"1px solid rgba(128,128,128,0.5)"}),
        vec![],
    )
}
fn image(src: &Value, w: usize, h: usize) -> Value {
    json!({"kind":"image","src":src,"style":{"width":w,"height":h,"objectFit":"contain"}})
}
fn origin(name: &str) -> Value {
    match name {
        "" | "lordos" => json!("Axia DE"),
        "terminal" => key("shell.clipboard.fromTerminal"),
        "linux" => json!("Linux"),
        _ => json!(name.rsplit('.').next().unwrap_or(name)),
    }
}
fn button(label: Value, event: Value, t: &Value) -> Value {
    action(
        view(
            json!({"paddingHorizontal":10,"paddingVertical":5,"borderRadius":6,"backgroundColor":t["control"]}),
            vec![text(
                label,
                json!({"fontSize":11,"fontWeight":600,"color":t["text"]}),
            )],
        ),
        json!({"onClick":event}),
    )
}
fn relative(time: &Value) -> Value {
    let secs = ((super::now() - time.as_f64().unwrap_or(0.)) / 1000.).round();
    if secs < 60. {
        key("shell.clipboard.now")
    } else if secs < 3600. {
        json!({"key":"shell.clipboard.minutes","params":{"count":(secs/60.).floor()}})
    } else if secs < 86400. {
        json!({"key":"shell.clipboard.hours","params":{"count":(secs/3600.).floor()}})
    } else {
        json!({"date":time})
    }
}
fn query_text(text: &str) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 128
        })
        .collect()
}
fn toast(v: &Value) -> Value {
    let t = &v["context"]["theme"];
    let area = &v["context"]["area"];
    let number = |k: &str| area[k].as_f64().unwrap_or(0.);
    json!({"tree":view(json!({"position":"absolute","left":(number("left")+number("width")/2.-170.).round(),"width":340,"top":number("top")+number("height")-150.,"alignItems":"center","pointerEvents":"none"}),vec![view(json!({"paddingHorizontal":14,"paddingVertical":8,"borderRadius":10,"backgroundColor":t["windowBg"],"border":format!("1px solid {}",s(t,"separator")),"boxShadow":"0 8px 24px rgba(0,0,0,0.35)"}),vec![text(v["message"].clone(),json!({"fontSize":12,"color":t["text"]}))])])})
}
#[derive(Default)]
struct Panel {
    mode: String,
    query: String,
    filter: String,
    selected: usize,
    confirm: bool,
    generation: u64,
    ids: Vec<String>,
    counts: Value,
    context: Value,
    rows: Value,
    save: bool,
}
thread_local! {static PANEL:RefCell<Panel>=RefCell::new(Panel::default());}
pub fn reset() {
    PANEL.with(|p| *p.borrow_mut() = Panel::default());
}
impl Panel {
    fn query_effect(&mut self) -> Value {
        self.generation = self.generation.wrapping_add(1);
        json!({"type":"query","query":self.query,"filter":self.filter,"limit":if self.mode=="full"{MAX_ITEMS}else{12},"generation":self.generation})
    }
    fn selected(&self) -> Option<&String> {
        self.ids
            .get(self.selected.min(self.ids.len().saturating_sub(1)))
    }
    fn visible(&self) -> &[String] {
        if self.mode != "full" {
            return &self.ids[..self.ids.len().min(12)];
        }
        let start = self
            .selected
            .saturating_sub(8)
            .min(self.ids.len().saturating_sub(16));
        let end = 16.max(self.selected + 8).min(self.ids.len());
        &self.ids[start..end.max(start)]
    }
    fn row(&self, id: &str) -> &Value {
        self.rows.get(id).unwrap_or(&Value::Null)
    }
    fn row_tree(&self, id: &str) -> Value {
        let item = self.row(id);
        let t = &self.context["theme"];
        let active = self.selected().map(String::as_str) == Some(id);
        let fg = if active {
            json!("#fff")
        } else {
            t["text"].clone()
        };
        let dim = if active {
            json!("rgba(255,255,255,0.8)")
        } else {
            t["textDim"].clone()
        };
        let mut children = vec![if s(item, "type") == "color" {
            swatch(s(item, "color"), 14)
        } else {
            text(
                json!(glyph(s(item, "type"))),
                json!({"width":16,"fontSize":11,"color":dim,"textAlign":"center"}),
            )
        }];
        if item["image"].is_string() {
            children.push(image(&item["image"], 64, 36));
            children.push(view(json!({"flex":1}), vec![]));
        } else {
            let mut style = json!({"flex":1,"fontSize":12,"color":fg,"whiteSpace":"nowrap","overflow":"hidden"});
            if s(item, "type") == "code" {
                style["fontFamily"] = json!("monospace");
            }
            children.push(text(item["preview"].clone(), style));
        }
        if item["pinned"] == true {
            children.push(text(json!("📌"), json!({"fontSize":11,"color":fg})));
        }
        children.push(text(
            relative(&item["time"]),
            json!({"fontSize":10,"color":dim}),
        ));
        let mut node = action(
            view(
                json!({"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":8,"paddingVertical":if self.mode=="full"{7}else{5},"borderRadius":7,"backgroundColor":if active{t["accent"].clone()}else{json!("transparent")}}),
                children,
            ),
            json!({"onClick":["pick",id],"onMouseEnter":["select",id]}),
        );
        node["key"] = json!(id);
        node
    }
    fn tree(&self) -> Value {
        if self.mode.is_empty() || self.context["theme"].is_null() {
            return Value::Null;
        }
        let t = &self.context["theme"];
        let area = &self.context["area"];
        let width = area["width"].as_f64().unwrap_or(1000.);
        let height = area["height"].as_f64().unwrap_or(700.);
        let full = self.mode == "full";
        let w = if full { 860_f64.min(width - 40.) } else { 420. };
        let h = 540_f64.min(height - 120.);
        let mut head = vec![
            text(
                key("shell.clipboard.title"),
                json!({"fontSize":if full{15}else{13},"fontWeight":700,"color":t["text"]}),
            ),
            json!({"kind":"input","value":self.query,"autoFocus":true,"placeholder":key("shell.clipboard.search"),"style":{"flex":1}}),
        ];
        if full {
            head.push(button(
                key(if self.confirm {
                    "shell.clipboard.confirmClear"
                } else {
                    "shell.clipboard.clear"
                }),
                json!(["clear"]),
                t,
            ));
            head.push(button(json!("✕"), json!(["close"]), t));
        } else {
            head.push(action(view(json!({"paddingHorizontal":8,"paddingVertical":4,"borderRadius":6,"backgroundColor":t["control"]}),vec![text(key("shell.clipboard.more"),json!({"fontSize":11,"color":t["text"]}))]),json!({"onClick":["full"]})));
        }
        let mut header_style =
            json!({"flexDirection":"row","alignItems":"center","gap":if full{10}else{8}});
        if full {
            header_style["padding"] = json!(12);
        } else {
            header_style["marginBottom"] = json!(4);
        }
        let mut contents = vec![view(header_style, head)];
        if full {
            contents.push(view(
                json!({"height":1,"backgroundColor":t["separator"]}),
                vec![],
            ));
            let filters=FILTERS.iter().map(|f|{let active=self.filter==*f;let mut n=action(view(json!({"flexDirection":"row","alignItems":"center","paddingHorizontal":8,"height":28,"borderRadius":6,"backgroundColor":if active{t["accent"].clone()}else{json!("transparent")}}),vec![text(key(&format!("shell.clipboard.filter.{f}")),json!({"flex":1,"fontSize":12,"color":if active{json!("#fff")}else{t["text"].clone()}})),text(json!(self.counts[*f].as_u64().unwrap_or(0).to_string()),json!({"fontSize":10,"color":if active{json!("#fff")}else{t["textDim"].clone()}}))]),json!({"onClick":["filter",f]}));n["key"]=json!(f);n}).collect();
            let mut rows: Vec<_> = self.visible().iter().map(|id| self.row_tree(id)).collect();
            if rows.is_empty() {
                rows.push(text(
                    key("shell.clipboard.empty"),
                    json!({"fontSize":12,"color":t["textDim"],"padding":10}),
                ));
            }
            let mut preview = Vec::new();
            if let Some(id) = self.selected() {
                let item = self.row(id);
                let category = s(item, "type");
                preview.push(if item["image"].is_string(){image(&item["image"],256,200)}else if category=="color"{view(json!({"flexDirection":"row","alignItems":"center","gap":10}),vec![swatch(s(item,"color"),48),text(item["color"].clone(),json!({"fontSize":16,"fontWeight":700,"color":t["text"],"fontFamily":"monospace"}))])}else{let mut style=json!({"fontSize":11,"color":t["text"],"maxHeight":280,"overflow":"hidden","lineHeight":1.35});if category=="code"{style["fontFamily"]=json!("monospace");}text(item["text"].clone(),style)});
                preview.push(text(json!({"key":"shell.clipboard.meta","params":{"type":key(&format!("shell.clipboard.filter.{category}")),"origin":origin(s(item,"origin")),"when":relative(&item["time"]),"size":if item["text"].as_str().is_some_and(|s|!s.is_empty()){json!({"key":"shell.clipboard.chars","params":{"count":item["characters"]}})}else{json!("")}}}),json!({"fontSize":11,"color":t["textDim"],"lineHeight":1.4})));
                preview.push(view(
                    json!({"flexDirection":"row","flexWrap":"wrap","gap":6}),
                    vec![
                        button(key("shell.clipboard.paste"), json!(["pick", id]), t),
                        button(
                            key(if item["pinned"] == true {
                                "shell.clipboard.unpin"
                            } else {
                                "shell.clipboard.pin"
                            }),
                            json!(["pin", id]),
                            t,
                        ),
                        button(key("shell.clipboard.delete"), json!(["remove", id]), t),
                    ],
                ));
            } else {
                preview.push(text(
                    key("shell.clipboard.noSelection"),
                    json!({"fontSize":12,"color":t["textDim"]}),
                ));
            }
            contents.push(view(
                json!({"flex":1,"flexDirection":"row"}),
                vec![
                    view(
                        json!({"width":150,"padding":8,"gap":2,"backgroundColor":t["sidebarBg"]}),
                        filters,
                    ),
                    view(
                        json!({"flex":1,"padding":6,"gap":1,"overflow":"hidden"}),
                        rows,
                    ),
                    view(json!({"width":1,"backgroundColor":t["separator"]}), vec![]),
                    view(json!({"width":280,"padding":12,"gap":8}), preview),
                ],
            ));
            contents.push(view(
                json!({"height":1,"backgroundColor":t["separator"]}),
                vec![],
            ));
            contents.push(text(key(if self.save{"shell.clipboard.fullHint"}else{"shell.clipboard.notSaved"}),json!({"fontSize":10,"color":t["textDim"],"paddingHorizontal":12,"paddingVertical":6})));
        } else {
            if self.ids.is_empty() {
                contents.push(text(
                    key("shell.clipboard.empty"),
                    json!({"fontSize":12,"color":t["textDim"],"padding":8}),
                ));
            }
            contents.extend(self.visible().iter().map(|id| self.row_tree(id)));
            contents.push(text(
                key("shell.clipboard.hint"),
                json!({"fontSize":10,"color":t["textDim"],"paddingHorizontal":4,"marginTop":4}),
            ));
        }
        let mut style = json!({"position":"absolute","left":((width-w)/2.).round(),"top":if full{((height-h)/2.).round()}else{70.},"width":w,"backgroundColor":t["windowBg"],"border":format!("1px solid {}",s(t,"separator")),"boxShadow":if full{"0 24px 60px rgba(0,0,0,0.5)"}else{"0 16px 40px rgba(0,0,0,0.45)"},"borderRadius":if full{14}else{12}});
        if full {
            style["height"] = json!(h);
            style["overflow"] = json!("hidden");
        } else {
            style["padding"] = json!(10);
            style["gap"] = json!(4);
        }
        let panel = action(view(style, contents), json!({"onClick":["stop"]}));
        let mut root = area.clone();
        root["position"] = json!("absolute");
        if full {
            root["backgroundColor"] = json!("rgba(0,0,0,0.25)");
        }
        action(view(root, vec![panel]), json!({"onClick":["close"]}))
    }
    fn reply(&self, effects: Vec<Value>, handled: bool, tree: bool) -> Value {
        json!({"mode":self.mode,"visible":self.visible(),"selected":self.selected(),"textLimit":3000,"tree":if tree{self.tree()}else{Value::Null},"effects":effects,"handled":handled})
    }
}
pub fn request(v: &Value) -> Value {
    if s(v, "cmd") == "toast" {
        return toast(v);
    }
    PANEL.with(|p| {
        let mut p = p.borrow_mut();
        let mut effects = Vec::new();
        let mut handled = true;
        match s(v, "cmd") {
            "open" => {
                p.mode = if s(v, "mode") == "full" {
                    "full"
                } else {
                    "quick"
                }
                .into();
                p.query.clear();
                p.filter = "all".into();
                p.selected = 0;
                p.confirm = false;
                p.ids.clear();
                p.rows = Value::Null;
                effects.push(p.query_effect());
            }
            "close" => {
                p.mode.clear();
                p.ids.clear();
                p.rows = Value::Null;
            }
            "context" => {
                p.context = v["context"].clone();
                p.save = v["save"] != false;
            }
            "refresh" => {
                if !p.mode.is_empty() {
                    effects.push(p.query_effect());
                }
            }
            "view" => {
                let view = &v["view"];
                if view["generation"] == p.generation
                    && s(view, "query") == p.query
                    && s(view, "filter") == p.filter
                {
                    p.ids = view["ids"]
                        .as_array()
                        .map(|ids| {
                            ids.iter()
                                .take(MAX_ITEMS)
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    p.counts = view["counts"].clone();
                    p.selected = p.selected.min(p.ids.len().saturating_sub(1));
                }
            }
            "render" => {
                p.rows = v["rows"].clone();
            }
            "event" => {
                let a = &v["action"];
                let name = a[0].as_str().unwrap_or("");
                let id = a[1].as_str().unwrap_or("");
                match name {
                    "query" => {
                        p.query = query_text(id);
                        p.selected = 0;
                        p.ids.clear();
                        effects.push(p.query_effect());
                    }
                    "filter" if FILTERS.contains(&id) => {
                        p.filter = id.into();
                        p.selected = 0;
                        p.ids.clear();
                        effects.push(p.query_effect());
                    }
                    "select" => {
                        if let Some(i) = p.ids.iter().position(|x| x == id) {
                            p.selected = i;
                        }
                    }
                    "pick" | "pin" | "remove" => {
                        if p.ids.iter().any(|x| x == id) {
                            effects.push(json!({"type":name,"id":id}));
                        }
                    }
                    "clear" => {
                        if p.confirm {
                            effects.push(json!({"type":"clear"}));
                            p.confirm = false;
                        } else {
                            p.confirm = true;
                        }
                    }
                    "full" => effects.push(json!({"type":"full"})),
                    "close" => effects.push(json!({"type":"close"})),
                    "stop" => (),
                    "key" => match s(v, "event") {
                        "ArrowDown" => {
                            p.selected = (p.selected + 1).min(p.ids.len().saturating_sub(1))
                        }
                        "ArrowUp" => p.selected = p.selected.saturating_sub(1),
                        "Enter" => {
                            if let Some(id) = p.selected() {
                                effects.push(json!({"type":"pick","id":id}));
                            }
                        }
                        "Delete" if p.mode == "full" => {
                            if let Some(id) = p.selected() {
                                effects.push(json!({"type":"remove","id":id}));
                            }
                        }
                        _ => {
                            let ch = s(v, "char");
                            if p.mode == "quick"
                                && p.query.is_empty()
                                && ch.len() == 1
                                && matches!(ch.as_bytes()[0], b'1'..=b'9')
                            {
                                if let Some(id) = p.ids.get((ch.as_bytes()[0] - b'1') as usize) {
                                    effects.push(json!({"type":"pick","id":id}));
                                }
                            } else {
                                handled = false;
                            }
                        }
                    },
                    _ => handled = false,
                }
            }
            _ => handled = false,
        }
        p.reply(effects, handled, s(v, "cmd") == "render")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(mode: &str) -> Value {
        reset();
        request(
            &json!({"cmd":"context","context":{"theme":{"text":"#fff","textDim":"#aaa","accent":"#08f","windowBg":"rgba(20,20,20,0.8)","separator":"#333","control":"#444"},"area":{"left":0,"top":28,"width":1000,"height":672}},"save":true}),
        );
        let open = request(&json!({"cmd":"open","mode":mode}));
        let query = &open["effects"][0];
        request(
            &json!({"cmd":"view","view":{"generation":query["generation"],"query":"","filter":"all","ids":["a","b"],"counts":{"all":2,"text":2}}}),
        );
        request(
            &json!({"cmd":"render","rows":{"a":{"type":"text","text":"first","preview":"first","time":super::super::now(),"characters":5},"b":{"type":"text","text":"second","preview":"second","time":super::super::now(),"characters":6}}}),
        )
    }
    #[test]
    fn quick_geometry_numeric_keys_and_hover_keep_existing_contract() {
        let quick = setup("quick");
        assert_eq!(quick["tree"]["children"][0]["style"]["width"], 420.);
        assert_eq!(quick["tree"]["children"][0]["style"]["left"], 290.);
        assert_eq!(quick["tree"]["children"][0]["style"]["top"], 70.);
        assert_eq!(quick["selected"], "a");
        assert_eq!(
            request(&json!({"cmd":"event","action":["key"],"event":"ArrowDown"}))["selected"],
            "b"
        );
        let pick = request(&json!({"cmd":"event","action":["key"],"event":"2","char":"2"}));
        assert_eq!(pick["effects"][0], json!({"type":"pick","id":"b"}));
        let hover = request(&json!({"cmd":"event","action":["select","a"]}));
        assert_eq!(hover["selected"], "a");
        assert!(hover["effects"].as_array().unwrap().is_empty());
    }
    #[test]
    fn full_geometry_filter_stale_reply_confirmation_and_delete() {
        let full = setup("full");
        let style = &full["tree"]["children"][0]["style"];
        assert_eq!(style["width"], 860.);
        assert_eq!(style["height"], 540.);
        assert_eq!(style["top"], 66.);
        assert_eq!(style["borderRadius"], 14);
        let confirm = request(&json!({"cmd":"event","action":["clear"]}));
        assert!(confirm["effects"].as_array().unwrap().is_empty());
        assert_eq!(
            request(&json!({"cmd":"event","action":["clear"]}))["effects"][0]["type"],
            "clear"
        );
        assert_eq!(
            request(&json!({"cmd":"event","action":["key"],"event":"Delete"}))["effects"][0],
            json!({"type":"remove","id":"a"})
        );
        let filter = request(&json!({"cmd":"event","action":["filter","image"]}));
        assert_eq!(filter["effects"][0]["filter"], "image");
        assert!(filter["visible"].as_array().unwrap().is_empty());
        let stale = request(
            &json!({"cmd":"view","view":{"generation":0,"query":"","filter":"all","ids":["stale"]}}),
        );
        assert!(stale["visible"].as_array().unwrap().is_empty());
    }
    #[test]
    fn queries_are_utf16_bounded_and_row_requests_are_windowed() {
        setup("full");
        let q = request(&json!({"cmd":"event","action":["query","👋".repeat(100)]}));
        assert_eq!(
            q["effects"][0]["query"].as_str().unwrap().chars().count(),
            64
        );
        let panel = Panel {
            mode: "full".into(),
            ids: (0..8192).map(|i| i.to_string()).collect(),
            selected: 4000,
            ..Panel::default()
        };
        assert_eq!(panel.visible().len(), 16);
        assert_eq!(panel.selected(), Some(&"4000".to_string()));
        assert_eq!(panel.reply(vec![], true, false)["textLimit"], 3000);
    }
}
