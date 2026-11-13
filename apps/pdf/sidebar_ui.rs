//! Native PDF sidebar scene. The generic native tree host mounts these nodes;
//! React supplies only its mount, resource handles and helper replies.
use serde_json::{json, Value};

pub(super) const HEADER_HEIGHT: f64 = 38.;
fn view(key: Value, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn click(mut node: Value, action: Value) -> Value {
    node["events"]["onClick"] = action;
    node
}
pub(super) fn render(thumbnails: &Value, outline: &Value, facts: &Value) -> Value {
    let t = &facts["theme"];
    let side = facts["side"].as_str().unwrap_or("thumbs");
    let tabs = ["thumbs", "outline"].into_iter().map(|tab| {
        let selected = tab == side;
        click(view(json!(tab), json!({"flexGrow":1,"paddingHorizontal":10,"height":18,"borderRadius":t["controlRadiusSmall"].as_f64().map(|r| (r-1.).max(2.)).unwrap_or(5.),"alignItems":"center","justifyContent":"center",
            "backgroundColor":if selected {json!(if t["mode"]=="dark" {"rgba(255,255,255,0.18)"} else {"#ffffff"})} else {json!("transparent")},
            "boxShadow":if selected {"0 1px 2px rgba(0,0,0,0.18)"} else {"none"}}), vec![text(
                json!({"key":format!("pdf.side.{tab}Short")}),
                json!({"fontSize":11,"fontWeight":if selected {600} else {500},"whiteSpace":"nowrap","color":t["text"]})
            )]), json!(["side",tab]))
    }).collect();
    let header = view(
        json!("header"),
        json!({"height":HEADER_HEIGHT,"flexShrink":0,"paddingHorizontal":12,"paddingTop":10,"paddingBottom":6}),
        vec![view(
            json!("tabs"),
            json!({"height":22,"padding":2,"flexDirection":"row","borderRadius":t["controlRadiusSmall"],"backgroundColor":t["control"]}),
            tabs,
        )],
    );
    let body = if side == "outline" {
        if facts["loading"] == true || outline["rows"].as_array().is_none_or(|a| a.is_empty()) {
            view(
                json!("outline-status"),
                json!({"flex":1}),
                vec![text(
                    json!({"key":if facts["loading"] == true {"pdf.loading"} else {"pdf.noOutline"}}),
                    json!({"margin":12,"fontSize":12,"color":t["textDim"]}),
                )],
            )
        } else {
            let rows = outline["rows"].as_array().unwrap().iter().map(|row| {
                let depth = row["depth"].as_u64().unwrap_or(0);
                let page = row["page"].as_i64().unwrap_or(-1);
                let disclosure = if row["hasKids"] == true {
                    click(view(json!("disclosure"), json!({"width":14,"height":26,"alignItems":"center","justifyContent":"center","flexShrink":0}), vec![text(
                        json!(if row["open"] == true {"▼"} else {"▶"}), json!({"fontSize":8,"color":t["textDim"]})
                    )]), json!(["toggle",row["key"]]))
                } else {view(json!("disclosure"),json!({"width":14,"flexShrink":0}),vec![])};
                let mut children = vec![disclosure, text(row["title"].clone(),json!({"flex":1,"fontSize":12,"lineHeight":16,"fontWeight":if depth==0 {500} else {400},"color":t["text"]}))];
                if page >= 0 { children.push(text(json!((page as u64+1).to_string()),json!({"fontSize":10,"color":t["textDim"]}))); }
                let node = view(row["key"].clone(),json!({"flexDirection":"row","alignItems":"center","minHeight":26,"paddingRight":8,"paddingLeft":4+depth*12,"borderRadius":6}),children);
                if page >= 0 { click(node,json!(["pick",row["key"]])) } else { node }
            }).collect();
            json!({"kind":"scroll","key":"outline","style":{"flex":1},"children":[
                view(json!("outline-rows"),json!({"paddingHorizontal":8,"paddingVertical":4}),rows)
            ]})
        }
    } else {
        let width = thumbnails["width"].as_f64().unwrap_or(116.);
        let rows = thumbnails["frames"].as_array().into_iter().flatten().map(|frame| {
            let page = frame["page"].as_u64().unwrap_or(0);
            let height = frame["height"].as_f64().unwrap_or(0.);
            let selected = frame["selected"] == true;
            let image = facts["images"][page.to_string()].as_i64().filter(|id| *id > 0 && *id <= i32::MAX as i64);
            let raster = image.map(|id| json!({"kind":"documentImage","key":"image","image":id,"style":{"width":width,"height":height,"objectFit":"fill"}})).into_iter().collect();
            click(view(json!(page),json!({"position":"absolute","left":0,"right":0,"top":frame["top"],"alignItems":"center","gap":4}),vec![
                view(json!("frame"),json!({"width":width+8.,"height":height+8.,"borderRadius":5,"alignItems":"center","justifyContent":"center","borderWidth":3,"borderColor":if selected {t["accent"].clone()} else {json!("transparent")}}),vec![
                    view(json!("paper"),json!({"width":width,"height":height,"backgroundColor":if facts["invert"]==true {"#1b1b1f"} else {"#ffffff"},"boxShadow":"0 1px 4px rgba(0,0,0,0.35)"}),raster)
                ]),
                text(json!((page+1).to_string()),json!({"fontSize":11,"fontWeight":if selected {600} else {400},"color":if selected {&t["text"]} else {&t["textDim"]}}))
            ]),json!(["page",page]))
        }).collect();
        let mut content = view(
            json!("thumbs"),
            json!({"flex":1,"overflow":"hidden","direction":"ltr"}),
            rows,
        );
        content["events"]["onWheel"] = json!(["scroll"]);
        content
    };
    view(
        json!(format!(
            "pdf-sidebar:{}",
            facts["document"].as_u64().unwrap_or(0)
        )),
        json!({"flex":1,"backgroundColor":t["sidebarBg"]}),
        vec![
            header,
            body,
            view(
                json!("separator"),
                json!({"position":"absolute","right":0,"top":0,"bottom":0,"width":1,"backgroundColor":t["separator"],"pointerEvents":"none"}),
                vec![],
            ),
        ],
    )
}
