//! A widget tree that the shell already has (Rust made it, JS carries it) drawn by the native
//! host: `shell:tree`. The Spotlight panel is the first: its session lives in JS (search sources,
//! polling), its tree comes from Rust, and React only drew it. Here the facts are
//! `args = {"tree": <the tree>}`; what the user does goes back as
//! `{"kind":"tree","action":[cmd, key],"extra":{…pointer, modifiers, value…}}` for the shell to
//! hand to the session. A search field passes the arrows, Enter and Tab on and closes on Escape.
use super::NativeApp;
use serde_json::{json, Map, Value};

/// The keys a search field hands to the app instead of editing with them.
const KEYS: [&str; 4] = ["ArrowDown", "ArrowUp", "Enter", "Tab"];

#[derive(Default)]
pub struct Tree {
    last: Value,
}

fn magnifier(size: f64, color: &Value) -> Value {
    let r = size * 0.62;
    let line = 1.4_f64.max(size / 10.);
    let abs = |style: Value| {
        let mut s = style;
        s["position"] = json!("absolute");
        json!({"kind":"view","style":s})
    };
    json!({"kind":"view","style":{"width":size,"height":size},"children":[
        abs(json!({"left":0,"top":0,"width":r,"height":r,"borderRadius":r/2.,"border":format!("{line}px solid {}",color.as_str().unwrap_or("#fff"))})),
        abs(json!({"left":r*0.82,"top":r*0.98,"width":size*0.42,"height":1.6_f64.max(size/9.),"borderRadius":1,
            "backgroundColor":color,"transform":"rotate(45deg)","transformOrigin":"left top"}))]})
}

fn convert(node: &Value) -> Value {
    match node["kind"].as_str().unwrap_or("") {
        "icon" => json!({"kind":"appIcon","app":node["app"],"size":node["size"]}),
        // an icon that a JS source draws with a component: not something Rust can see
        "providerIcon" => {
            let size = node["size"].as_f64().unwrap_or(22.);
            json!({"kind":"view","style":{"width":size,"height":size,"borderRadius":size*0.225,"backgroundColor":"rgba(128,128,128,0.35)"}})
        }
        "searchGlyph" => magnifier(node["size"].as_f64().unwrap_or(24.), &node["color"]),
        "input" => {
            let mut out = node.clone();
            if let Some(size) = node["size"].as_f64() {
                if node["style"]["fontSize"].is_null() {
                    out["style"]["fontSize"] = json!(size);
                }
            }
            out["keys"] = json!(KEYS);
            out["onKey"] = json!(["key"]);
            out["cancel"] = json!(["close"]);
            out
        }
        _ => {
            let mut out = node.clone();
            if let Some(children) = node["children"].as_array() {
                out["children"] = Value::Array(children.iter().map(convert).collect());
            }
            out
        }
    }
}

impl NativeApp for Tree {
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                let tree = if input["args"]["tree"].is_object() { convert(&input["args"]["tree"]) } else { Value::Null };
                if tree == self.last {
                    return json!({"same":true});
                }
                self.last = tree.clone();
                json!({"tree":tree})
            }
            // the rows of a native menu and the host's own bookkeeping: nothing for the session
            "menuHover" | "menuSubHover" | "next" => json!({}),
            cmd => {
                let mut extra: Map<String, Value> = input.as_object().cloned().unwrap_or_default();
                extra.remove("cmd");
                extra.remove("key");
                let action = if input["key"].is_null() { json!([cmd]) } else { json!([cmd, input["key"]]) };
                json!({"effects":[{"kind":"tree","action":action,"extra":extra}]})
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spotlight_like() -> Value {
        json!({"kind":"view","events":{"onClick":["close"]},"children":[
            {"kind":"view","children":[
                {"kind":"searchGlyph","size":24,"color":"#999999"},
                {"kind":"input","value":"ab","size":20,"autoFocus":true,"style":{"flex":1}}]},
            {"kind":"icon","app":"files","size":22},
            {"kind":"providerIcon","source":"s","callback":"k","size":22}]})
    }
    fn sync(app: &mut Tree, tree: Value) -> Value {
        app.request(&json!({"cmd":"sync","args":{"tree":tree}}))
    }

    #[test]
    fn draws_the_tree_without_nodes_only_react_knew() {
        let mut app = Tree::default();
        let tree = sync(&mut app, spotlight_like())["tree"].clone();
        let text = tree.to_string();
        assert!(!text.contains("searchGlyph") && !text.contains("providerIcon") && !text.contains("\"icon\""));
        assert_eq!(tree["children"][1]["kind"], "appIcon");
        assert_eq!(tree["children"][0]["children"][0]["style"]["width"].as_f64(), Some(24.));
        assert_eq!(tree["children"][0]["children"][0]["children"].as_array().unwrap().len(), 2, "a ring and a handle");
        assert_eq!(tree["events"]["onClick"], json!(["close"]), "events and styles pass through");
    }
    #[test]
    fn the_search_field_passes_its_keys_and_closes_on_escape() {
        let mut app = Tree::default();
        let tree = sync(&mut app, spotlight_like())["tree"].clone();
        let input = &tree["children"][0]["children"][1];
        assert_eq!(input["keys"], json!(["ArrowDown", "ArrowUp", "Enter", "Tab"]));
        assert_eq!(input["onKey"], json!(["key"]));
        assert_eq!(input["cancel"], json!(["close"]));
        assert_eq!(input["style"]["fontSize"].as_f64(), Some(20.));
        assert_eq!(input["style"]["flex"], 1, "the field's own style stays");
    }
    #[test]
    fn an_unchanged_tree_sends_nothing_and_a_missing_one_clears() {
        let mut app = Tree::default();
        sync(&mut app, spotlight_like());
        assert_eq!(sync(&mut app, spotlight_like())["same"], true);
        assert!(sync(&mut app, Value::Null)["tree"].is_null());
    }
    #[test]
    fn what_the_user_does_goes_back_to_the_session() {
        let mut app = Tree::default();
        let reply = app.request(&json!({"cmd":"key","key":"ArrowDown","meta":true,"shift":false}));
        assert_eq!(reply["effects"][0]["kind"], "tree");
        assert_eq!(reply["effects"][0]["action"], json!(["key", "ArrowDown"]));
        assert_eq!(reply["effects"][0]["extra"]["meta"], true);
        let typed = app.request(&json!({"cmd":"query","key":"hello"}));
        assert_eq!(typed["effects"][0]["action"], json!(["query", "hello"]));
        let close = app.request(&json!({"cmd":"close"}));
        assert_eq!(close["effects"][0]["action"], json!(["close"]));
        assert_eq!(app.request(&json!({"cmd":"menuHover","key":1})), json!({}));
    }
}
