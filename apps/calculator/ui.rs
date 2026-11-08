use super::{App, KEYS};
use serde_json::{json, Value};
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
pub fn tree(app: &App, context: &Value) -> Value {
    let t = &context["theme"];
    let dark = t["mode"] == "dark";
    let traditional = t["nativeStyle"] == "adwaita";
    let decimal = context["decimal"].as_str().unwrap_or(".");
    let show = |s: &str| s.replace('.', decimal).replace('-', "−");
    let shown = show(&app.result);
    let length = shown.chars().count();
    // "Error" is short in every language: it is never shrunk for the previous, longer result
    let big = if app.error || length <= 8 {
        44
    } else if length <= 11 {
        34
    } else {
        26
    };
    let result = if app.error {
        json!({"key":"calculator.error"})
    } else {
        json!(shown)
    };
    let mut children = vec![view(
        json!({"height":92,"justifyContent":"flex-end","alignItems":"flex-end","paddingHorizontal":6,"marginBottom":4,"overflow":"hidden"}),
        vec![
            text(
                json!(if app.expression.is_empty() {
                    " ".into()
                } else {
                    show(&app.expression)
                }),
                json!({"fontSize":13,"color":t["textDim"],"whiteSpace":"nowrap"}),
            ),
            text(
                result,
                json!({"fontSize":big,"fontWeight":400,"color":t["text"],"whiteSpace":"nowrap"}),
            ),
        ],
    )];
    for row in KEYS {
        let keys=row.into_iter().map(|key|{
            let op=["÷","×","-","+"].contains(&key);let function=["C","(",")","⌫"].contains(&key);
            let bg=if key=="="{t["accent"].clone()}else if traditional{if function{t["inputBg"].clone()}else{t["control"].clone()}}
                else if op{json!("#ff9f0a")}else if function{json!(if dark{"#4a4a50"}else{"#d4d4d9"})}else{json!(if dark{"#333338"}else{"#ffffff"})};
            let label=if key=="."{decimal}else if key=="-"{"−"}else{key};
            let mut style=json!({"flex":1,"borderRadius":if traditional{t["controlRadius"].clone()}else{json!(12)},"alignItems":"center","justifyContent":"center","backgroundColor":bg,
                "opacity":if app.down.as_deref()==Some(key){0.7}else if app.hover.as_deref()==Some(key){0.88}else{1.},"boxShadow":if dark{"none"}else{"0 1px 1px rgba(0,0,0,0.08)"}});
            if traditional{style["border"]=json!(format!("1px solid {}",t["separator"].as_str().unwrap_or("transparent")));}
            let mut node=view(style,vec![text(json!(label),json!({"fontSize":if op||key=="="{24}else{20},"fontWeight":if op||key=="="{500}else{400},"color":if key=="="||(op&&!traditional){json!("#fff")}else{t["text"].clone()}}))]);
            node["key"]=json!(key);node["events"]=json!({"onClick":["press",key],"onMouseDown":["down",key],"onMouseUp":["up",key],"onMouseEnter":["hover",key],"onMouseLeave":["leave",key]});node
        }).collect();
        children.push(view(json!({"flexDirection":"row","gap":8,"flex":1}), keys));
    }
    view(
        json!({"flex":1,"padding":16,"gap":8,"direction":"ltr","backgroundColor":if traditional{t["windowBg"].clone()}else{json!(if dark{"#1c1c1e"}else{"#f5f5f7"})}}),
        children,
    )
}
pub fn icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    let mut children = vec![view(
        json!({"position":"absolute","left":s*0.16,"right":s*0.16,"top":s*0.12,"height":s*0.15,"borderRadius":s*0.04,"backgroundColor":"#9be7b0"}),
        vec![],
    )];
    for y in [0.34, 0.53, 0.72] {
        for (x, color) in [(0.16, "#71717a"), (0.425, "#71717a"), (0.69, "#ff9f0a")] {
            children.push(view(json!({"position":"absolute","left":s*x,"top":s*y,"width":s*0.15,"height":s*0.15,"borderRadius":s*0.075,"backgroundColor":color}),vec![]));
        }
    }
    view(
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(180deg, #3a3a40, #18181b)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"border":"1px solid rgba(255,255,255,0.12)"}),
        children,
    )
}
