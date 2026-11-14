//! Widget descriptions for the launcher grid (Lord Flow). No React implementation of
//! these layouts or policies is retained; the adapter renders this vocabulary.
use super::{session::Menu, *};

fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn key(key: &str) -> Value {
    json!({"key":key})
}
fn events(mut node: Value, events: Value) -> Value {
    node["events"] = events;
    node
}
fn icon(id: &str, size: f64) -> Value {
    json!({"kind":"icon","app":id,"size":size})
}
fn popup_item(label: &str, action: Value) -> Value {
    json!({"label":key(label),"action":action})
}

fn scroll_node(m: &Menu, model: &AxiaLauncher, region: &str, style: Value) -> Value {
    json!({"kind":"scroll","region":region,"key":format!("{region}:{}:{}",m.scroll_generation,model.generation),"style":style})
}

fn cell(a: &Value) -> Value {
    let id = string(a, "id");
    let label_style = json!({"fontSize":13,"fontWeight":500,"color":"#fff","whiteSpace":"nowrap","maxWidth":120,"overflow":"hidden"});
    let handlers = json!({"onClick":["open",id],"onMouseDown":["drag",id],"onContextMenu":["context",id]});
    let label = a.get("displayName").unwrap_or(&a["name"]);
    let mut node = events(
        view(
            json!({"width":130,"alignItems":"center","gap":10}),
            vec![icon(id, 72.), text(label.clone(), label_style)],
        ),
        handlers,
    );
    node["key"] = json!(id);
    node["app"] = json!(id);
    node
}

pub(super) fn scroll(m: &mut Menu, model: &mut AxiaLauncher, region: &str) -> Value {
    let fallback = number(&m.context["area"], "h", 700., 1., 16384.) - 190.;
    let (offset, height) = m
        .offsets
        .get(region)
        .copied()
        .unwrap_or((0., fallback.max(1.)));
    let plan = model.view(&json!({"layout":"grid","query":m.query.as_str(),"category":m.category.as_str(),"offset":offset,"height":height})).unwrap();
    let mut children = Vec::new();
    for r in plan["items"].as_array().unwrap() {
        let kids = r["indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| cell(&model.entries[i.as_u64().unwrap() as usize].metadata))
            .collect();
        let mut node = view(
            json!({"position":"absolute","top":r["top"],"left":0,"width":"100%","height":98,"flexDirection":"row"}),
            kids,
        );
        node["key"] = r["row"].clone();
        children.push(node);
    }
    if plan["total"] == 0 {
        children.push(text(key("shell.launchpad.empty"),json!({"marginTop":16,"fontSize":14,"color":"rgba(255,255,255,0.7)","textAlign":"center"})));
    }
    let tree = view(json!({"height":plan["height"]}), children);
    m.plan = plan;
    tree
}

pub(super) fn render(m: &mut Menu, model: &mut AxiaLauncher) -> Value {
    let t = m.context["theme"].clone();
    let area = m.context["area"].clone();
    let q = unicode::query(&m.query);
    m.plan = model.view(&json!({"query":q.as_str(),"category":m.category.as_str(),"height":1})).unwrap();
    let mut children = Vec::new();
    let mut input = json!({"kind":"input","search":false,"value":m.query,"placeholder":key("shell.launchpad.search"),"autoFocus":true});
    input["style"] = json!({"backgroundColor":"rgba(255,255,255,0.14)","border":"1px solid rgba(255,255,255,0.22)"});
    children.push(events(view(json!({"marginTop":48,"width":260}), vec![input]), json!({"onClick":["noop"]})));
    if q.is_empty() {
        let mut cats = vec![String::new()];
        cats.extend(model.categories.iter().map(|s| s.to_string()));
        children.push(events(view(json!({"marginTop":22,"flexDirection":"row","gap":6}),cats.iter().map(|c|events(view(json!({"paddingHorizontal":12,"paddingVertical":5,"borderRadius":14,"backgroundColor":if c==&m.category {"rgba(255,255,255,0.32)"}else{"rgba(255,255,255,0.1)"}}),vec![text(key(&if c.is_empty(){"shell.category.all".into()}else{format!("shell.category.{c}")}),json!({"fontSize":12,"fontWeight":600,"color":"#fff"}))]),json!({"onClick":["category",c]}))).collect()),json!({"onClick":["noop"]})));
    }
    children.push(scroll_node(m,model,"grid",json!({"marginTop":if q.is_empty(){14}else{40},"height":number(&area,"h",700.,1.,16384.)-190.,"width":780_f64.min(number(&area,"w",1000.,1.,16384.)-20.)})));
    if let Some(p) = &m.popup {
        let id = string(p, "id");
        let d = &p["dock"];
        let mut items = vec![popup_item("shell.dock.open", json!(["open", id]))];
        if truthy(&d["id"]) {
            let mut it = popup_item("shell.dock.keep", json!(["dock", id]));
            it["disabled"] = json!(truthy(&d["pinned"]) && !truthy(&d["removable"]));
            it["checked"] = d["pinned"].clone();
            items.push(it);
        }
        children.push(json!({"kind":"menu","key":"context","x":p["x"],"y":p["y"],"items":items,"close":["closeContext"]}));
    }
    let style = json!({"position":"absolute","left":area["x"],"top":area["y"],"width":area["w"],"height":area["h"],"alignItems":"center","backgroundColor":if truthy(&t["effects"]){"rgba(10,10,16,0.35)"}else{"rgba(18,18,24,0.94)"},"backdropFilter":if truthy(&t["effects"]){"blur(32px)"}else{"blur(0px)"}});
    let mut root = view(style, children);
    root["root"] = json!(true);
    root["events"] = json!({"onClick":["close"]});
    root
}
