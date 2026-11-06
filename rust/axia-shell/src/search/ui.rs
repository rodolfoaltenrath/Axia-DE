//! The existing Spotlight geometry and materials, described by the Rust owner.
use super::{key, s, Client};
use serde_json::{json, Value};
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn event(mut node: Value, name: &str, action: Value) -> Value {
    node["events"] = json!({name:action});
    node
}
fn color(t: &Value, k: &str) -> Value {
    t[k].clone()
}
fn title(label: Value, t: &Value) -> Value {
    text(
        label,
        json!({"fontSize":14,"fontWeight":700,"color":t["text"]}),
    )
}
fn dim(label: Value, t: &Value) -> Value {
    text(
        label,
        json!({"fontSize":11,"color":t["textDim"],"lineHeight":1.35}),
    )
}
fn preview(c: &Client, it: &Value, t: &Value) -> Value {
    let p = &it["preview"];
    let mut children = Vec::new();
    if !it["big"].is_null() {
        children.push(text(
            it["big"].clone(),
            json!({"fontSize":30,"fontWeight":700,"color":t["text"]}),
        ));
        children.push(dim(it["subtitle"].clone(), t));
        children.push(dim(it["hint"].clone(), t));
    } else {
        match s(p, "kind") {
            "app" => {
                children.push(view(
                    json!({"alignItems":"center","gap":8,"marginTop":10}),
                    vec![
                        json!({"kind":"icon","app":p["id"],"size":72}),
                        title(p["name"].clone(), t),
                    ],
                ));
                children.push(dim(p["comment"].clone(), t));
                children.push(dim(key(&format!("store.kind.{}", s(p, "appKind"))), t));
            }
            "file" => {
                let f = &p["file"];
                let extra = &c.preview;
                if super::worker::image_path(s(f, "name")) {
                    children.push(if s(extra,"kind")=="image"{json!({"kind":"image","src":extra["path"],"style":{"width":232,"height":160,"objectFit":"contain"}})}else{view(json!({"width":232,"height":160}),vec![])});
                }
                children.push(title(f["name"].clone(), t));
                children.push(dim(it["subtitle"].clone(), t));
                children.push(dim(
                    if f["dir"] == true {
                        key("shell.spotlight.folder")
                    } else {
                        json!({"parts":[{"size":f["size"]}," · ",{"date":f["mtime"]}]})
                    },
                    t,
                ));
                if s(extra, "kind") == "text" {
                    children.push(text(extra["text"].clone(),json!({"fontSize":10,"fontFamily":"monospace","color":t["text"],"maxHeight":220,"overflow":"hidden","lineHeight":1.3})));
                }
                if let Some(items) = extra["items"].as_array() {
                    children.push(view(
                        json!({"gap":2}),
                        items
                            .iter()
                            .map(|it| {
                                text(
                                    json!(format!(
                                        "{} {}",
                                        if it["dir"] == true { "📁" } else { "📄" },
                                        s(it, "name")
                                    )),
                                    json!({"fontSize":11,"color":t["text"]}),
                                )
                            })
                            .collect(),
                    ));
                }
            }
            "note" | "text" => {
                if s(p, "kind") == "note" {
                    children.push(title(p["title"].clone(), t));
                }
                children.push(text(p["text"].clone(),json!({"fontSize":11,"color":t["text"],"maxHeight":300,"overflow":"hidden","lineHeight":1.35})));
            }
            _ => {
                children.push(title(it["title"].clone(), t));
                children.push(dim(it["subtitle"].clone(), t));
            }
        }
    }
    view(
        json!({"flexDirection":"row"}),
        vec![
            view(json!({"width":1,"backgroundColor":t["separator"]}), vec![]),
            view(json!({"width":259,"padding":14,"gap":8}), children),
        ],
    )
}
fn rows(c: &Client) -> Vec<(Option<String>, Option<&Value>, f64)> {
    let mut rows = Vec::new();
    for sec in &c.sections {
        rows.push((Some(s(sec, "id").to_owned()), None, 22.));
        if let Some(items) = sec["items"].as_array() {
            for it in items {
                rows.push((None, Some(it), 34.));
            }
        }
    }
    let selected = c.current().map(|it| s(it, "key"));
    let at = rows
        .iter()
        .position(|(_, it, _)| it.is_some_and(|it| Some(s(it, "key")) == selected))
        .unwrap_or(0);
    let mut used = rows.iter().take(at + 1).map(|r| r.2).sum::<f64>();
    let mut start = 0;
    while used > 420. - 34. && start < at {
        used -= rows[start].2;
        start += 1;
    }
    if start > 0 && rows[start - 1].0.is_some() {
        start -= 1;
    }
    let mut out = vec![];
    let mut used = 0.;
    for r in rows.into_iter().skip(start) {
        if used + r.2 > 420. {
            break;
        }
        used += r.2;
        out.push(r);
    }
    out
}
pub(super) fn render(c: &Client) -> Value {
    let t = &c.context["theme"];
    let area = &c.context["area"];
    let width = area["width"].as_f64().unwrap_or(1000.).clamp(1., 16384.);
    let height = area["height"].as_f64().unwrap_or(700.).clamp(1., 16384.);
    let w = 720_f64.min(width - 40.).max(40.);
    let has_query = !super::trimmed(&c.query).is_empty();
    let cur = c.current();
    let list_w = if cur.is_some() && has_query {
        w - 260.
    } else {
        w
    };
    let input = json!({"kind":"input","value":c.query,"size":20,"autoFocus":true,"placeholder":{"key":"shell.spotlight.placeholder"},"style":{"flex":1,"backgroundColor":"transparent","border":"0 solid transparent"}});
    let mut body = vec![view(
        json!({"flexDirection":"row","alignItems":"center","gap":10,"paddingHorizontal":14,"paddingVertical":10}),
        vec![
            json!({"kind":"searchGlyph","size":24,"color":t["textDim"]}),
            input,
        ],
    )];
    let separator = || view(json!({"height":1,"backgroundColor":t["separator"]}), vec![]);
    if has_query {
        body.push(separator());
        let mut result_rows = vec![];
        if c.flat().is_empty() {
            result_rows.push(text(
                key("shell.spotlight.none"),
                json!({"fontSize":13,"color":t["textDim"],"padding":14}),
            ));
        }
        for (head, it, _) in rows(c) {
            if let Some(head) = head {
                let mut node = text(
                    json!({"upper":{"key":format!("shell.spotlight.section.{head}")}}),
                    json!({"fontSize":10,"fontWeight":700,"color":t["textDim"],"paddingHorizontal":14,"paddingTop":6,"paddingBottom":2}),
                );
                node["key"] = json!(format!("h:{head}"));
                result_rows.push(node);
            } else if let Some(it) = it {
                let selected = cur.is_some_and(|current| current["key"] == it["key"]);
                let index = c
                    .flat()
                    .iter()
                    .position(|v| v["key"] == it["key"])
                    .unwrap_or(0);
                let image = if it["app"].is_string() {
                    json!({"kind":"icon","app":it["app"],"size":22})
                } else if it["providerIcon"] == true {
                    json!({"kind":"providerIcon","source":it["action"]["source"],"callback":it["action"]["key"],"size":22})
                } else if it["image"].is_string() {
                    json!({"kind":"image","src":it["image"],"style":{"width":22,"height":22,"objectFit":"contain"}})
                } else {
                    text(
                        json!(if s(it, "glyph").is_empty() {
                            "•"
                        } else {
                            s(it, "glyph")
                        }),
                        json!({"width":22,"fontSize":15,"textAlign":"center","color":if selected{json!("#fff")}else{color(t,"text")}}),
                    )
                };
                let mut row = view(
                    json!({"flexDirection":"row","alignItems":"center","gap":10,"marginHorizontal":6,"paddingHorizontal":8,"height":34,"borderRadius":7,"backgroundColor":if selected{color(t,"accent")}else{json!("transparent")}}),
                    vec![
                        image,
                        text(
                            it["title"].clone(),
                            json!({"fontSize":13,"color":if selected{json!("#fff")}else{color(t,"text")},"whiteSpace":"nowrap","overflow":"hidden","maxWidth":list_w*0.55}),
                        ),
                        text(
                            it["subtitle"].clone(),
                            json!({"flex":1,"fontSize":11,"color":if selected{json!("rgba(255,255,255,0.8)")}else{color(t,"textDim")},"whiteSpace":"nowrap","overflow":"hidden"}),
                        ),
                    ],
                );
                row["key"] = it["key"].clone();
                row["events"] =
                    json!({"onClick":["run",it["key"]],"onMouseEnter":["select",index]});
                result_rows.push(row);
            }
        }
        let mut results = vec![view(
            json!({"width":list_w,"paddingVertical":6,"maxHeight":440,"overflow":"hidden"}),
            result_rows,
        )];
        if let Some(cur) = cur {
            results.push(preview(c, cur, t));
        }
        body.push(view(
            json!({"flexDirection":"row","maxHeight":440}),
            results,
        ));
    }
    body.push(separator());
    body.push(view(
        json!({"flexDirection":"row","gap":14,"paddingHorizontal":14,"paddingVertical":6}),
        vec![text(
            key("shell.spotlight.keys"),
            json!({"fontSize":10,"color":t["textDim"]}),
        )],
    ));
    let mut style = c.context["glass"].clone();
    if !style.is_object() {
        style = json!({});
    }
    for(k,v)in json!({"position":"absolute","left":((width-w)/2.).round(),"top":(height*0.16).round(),"width":w,"borderRadius":16,"boxShadow":"0 24px 60px rgba(0,0,0,0.5)","overflow":"hidden"}).as_object().unwrap(){style[k]=v.clone();}
    let panel = event(view(style, body), "onClick", json!(["noop"]));
    let mut style = area.clone();
    if !style.is_object() {
        style = json!({});
    }
    style["position"] = json!("absolute");
    event(view(style, vec![panel]), "onClick", json!(["close"]))
}
