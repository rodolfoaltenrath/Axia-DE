//! The complete Force Quit view and icon, using the shared native widget vocabulary.
use super::App;
use serde_json::{json, Value};
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
pub fn icon(size: f64) -> Value {
    let size = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    view(
        json!({"width":size,"height":size,"borderRadius":size*0.22,"backgroundColor":"#ff453a","alignItems":"center","justifyContent":"center"}),
        vec![text(
            json!("!"),
            json!({"fontSize":size*0.55,"fontWeight":700,"color":"#fff"}),
        )],
    )
}
pub fn tree(app: &App, context: &Value) -> Value {
    let t = &context["theme"];
    let mut rows = Vec::new();
    for g in &app.groups {
        let on = app.selected.as_ref() == Some(&g.key);
        let mut children = vec![
            crate::native_view::icons::from_descriptor(&g.windows[0].icon, 20., &g.name),
            text(
                json!(g.name),
                json!({"flex":1,"fontSize":13,"color":if on {json!("#fff")} else {t["text"].clone()},"textAlign":"start","whiteSpace":"nowrap"}),
            ),
        ];
        if g.windows.len() > 1 {
            children.push(text(
                json!({"key":"forcequit.windows","params":{"count":g.windows.len()}}),
                json!({"fontSize":11,"color":if on {json!("#fff")} else {t["textDim"].clone()}}),
            ));
        }
        let mut row = view(
            json!({"flexDirection":"row","alignItems":"center","gap":10,"height":34,"paddingHorizontal":10,"backgroundColor":if on {t["accent"].clone()} else {json!("transparent")}}),
            children,
        );
        row["key"] = json!(g.key);
        row["events"] = json!({"onClick":["select",g.key],"onDoubleClick":["select",g.key]});
        rows.push(row);
    }
    if rows.is_empty() {
        rows.push(text(
            key("forcequit.none"),
            json!({"padding":12,"fontSize":13,"color":t["textDim"]}),
        ));
    }
    let mut children = vec![
        text(
            key("forcequit.intro"),
            json!({"fontSize":12,"color":t["text"],"textAlign":"start","lineHeight":1.4}),
        ),
        view(
            json!({"flex":1,"borderRadius":8,"border":format!("1px solid {}",t["separator"].as_str().unwrap_or("transparent")),"overflow":"hidden"}),
            vec![json!({"kind":"scroll","style":{"flex":1},"children":rows})],
        ),
    ];
    if !app.error.is_empty() {
        children.push(text(
            json!({"error":app.error}),
            json!({"fontSize":11,"color":"#ff9f0a","textAlign":"start"}),
        ));
    }
    children.push(view(json!({"flexDirection":"row","alignItems":"center"}),vec![text(key("forcequit.shortcut"),json!({"flex":1,"fontSize":11,"color":t["textDim"],"textAlign":"start"})),json!({"kind":"button","destructive":true,"label":key("forcequit.quit"),"disabled":app.selected.is_none(),"action":["quit"]})]));
    view(json!({"flex":1,"padding":16,"gap":10}), children)
}
