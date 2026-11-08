//! About one application. The host supplies registry facts; Rust chooses all content and layout.
#[cfg(test)]
mod tests;
use serde_json::{json, Value};
#[derive(Default)]
pub struct App;
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn first<'a>(values: &[&'a str]) -> &'a str {
    values.iter().copied().find(|s| !s.is_empty()).unwrap_or("")
}
impl App {
    pub fn request(&mut self, input: &Value) -> Value {
        match s(input, "cmd") {
            "icon" => icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => {
                let id = s(&input["args"], "key");
                let context = &input["context"];
                let theme = &context["theme"];
                let scale = &context["scale"];
                let find = |field: &str| {
                    input[field]
                        .as_array()
                        .and_then(|apps| apps.iter().find(|app| s(app, "id") == id))
                        .cloned()
                        .unwrap_or(Value::Null)
                };
                let app = find("catalog");
                let own = find("registry");
                let name = first(&[s(&app, "name"), s(&own, "name"), id]);
                let version = first(&[s(&own, "version"), s(&app, "version")]);
                let description = first(&[
                    s(&app, "comment"),
                    s(&own, "description"),
                    s(&app, "genericName"),
                ]);
                let linux = s(&app, "kind") == "linux";
                let kind = if app.is_null() {
                    None
                } else if linux {
                    Some("appabout.linux")
                } else if app["package"] == true {
                    Some("appabout.package")
                } else if own["system"] == true && own["package"] != true {
                    Some("appabout.system")
                } else {
                    Some("appabout.lordos")
                };
                let icon = if app["icon"] == true || own["icon"] == true {
                    json!({"kind":"appIcon","app":id,"size":64})
                } else {
                    icon(64.)
                };
                let mut children = vec![
                    icon,
                    text(
                        json!(name),
                        json!({"fontSize":scale["title2"].as_u64().unwrap_or(17),"fontWeight":700,"color":theme["text"],"marginTop":6,"textAlign":"center"}),
                    ),
                ];
                if !version.is_empty() {
                    children.push(text(
                        json!({"key":"appabout.version","params":{"v":version}}),
                        json!({"fontSize":scale["callout"],"color":theme["textDim"]}),
                    ));
                }
                if let Some(kind) = kind {
                    children.push(text(key(kind),json!({"fontSize":scale["caption"],"color":theme["textDim"],"textAlign":"center"})));
                }
                if own["isolated"] == true || own["package"] == true {
                    children.push(text(
                        key("appabout.isolated"),
                        json!({"fontSize":scale["caption"],"color":theme["textDim"]}),
                    ));
                }
                if !description.is_empty() {
                    children.push(text(json!(description),json!({"fontSize":scale["callout"],"color":theme["text"],"marginTop":8,"textAlign":"center","lineHeight":1.4,"maxWidth":280})));
                }
                children.push(json!({"kind":"view","style":{"flex":1}}));
                children.push(text(json!(if linux{id.strip_prefix("desktop:").unwrap_or(id)}else{id}),json!({"fontSize":scale["caption2"].as_u64().unwrap_or(10),"color":theme["textDim"]})));
                json!({"title":{"key":"appabout.title","params":{"app":name}},"tree":{"kind":"view","style":{"flex":1,"alignItems":"center","paddingHorizontal":22,"paddingTop":22,"paddingBottom":18,"gap":6},"children":children}})
            }
            _ => json!({"error":"app.invalidRequest"}),
        }
    }
}
fn icon(size: f64) -> Value {
    let size = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    json!({"kind":"view","style":{"width":size,"height":size,"borderRadius":size*0.22,"backgroundColor":"#8e8e93","alignItems":"center","justifyContent":"center"},"children":[text(json!("i"),json!({"fontSize":size*0.55,"fontWeight":700,"color":"#fff"}))]})
}
