//! A widget tree that the shell already has (Rust made it, JS carries it) drawn by the native
//! host: `shell:tree`. The Spotlight panel is the first: its session lives in JS (search sources,
//! polling), its tree comes from Rust, and React only drew it. Here the facts are
//! `args = {"tree": <the tree>, "owner": <who gets the events>}`; what the user does goes back as
//! `{"kind":"tree","owner","action":[cmd, key],"extra":{…pointer, modifiers, value…}}` for the shell to
//! hand to the session. A search field passes the arrows, Enter and Tab on and closes on Escape.
use super::{
    menubar::{advance, selectable},
    NativeApp,
};
use serde_json::{json, Map, Value};

/// The keys a search field hands to the app instead of editing with them.
const KEYS: [&str; 4] = ["ArrowDown", "ArrowUp", "Enter", "Tab"];

pub struct Tree {
    last: Value,
    /// Who the shell gave the view to (several can be mounted): sent back with every effect.
    owner: Value,
    /// The items of the menu node the tree has (a context menu), and the row under the pointer or
    /// the keyboard, the row whose submenu is open and the row inside it.
    items: Vec<Value>,
    row: i64,
    sub: i64,
    sub_row: i64,
}
impl Default for Tree {
    fn default() -> Self {
        Self { last: Value::Null, owner: Value::Null, items: Vec::new(), row: -1, sub: -1, sub_row: -1 }
    }
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

fn convert(node: &Value, menu: (i64, i64, i64)) -> Value {
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
        "menu" => {
            let mut out = node.clone();
            out["active"] = json!(menu.0);
            out["sub"] = json!(menu.1);
            out["subActive"] = json!(menu.2);
            out
        }
        _ => {
            let mut out = node.clone();
            if let Some(children) = node["children"].as_array() {
                out["children"] = Value::Array(children.iter().map(|c| convert(c, menu)).collect());
            }
            out
        }
    }
}
fn menu_items(node: &Value) -> Option<Vec<Value>> {
    if node["kind"] == "menu" {
        return Some(node["items"].as_array().cloned().unwrap_or_default());
    }
    node["children"].as_array()?.iter().find_map(menu_items)
}

impl NativeApp for Tree {
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                self.owner = input["args"]["owner"].clone();
                let items = menu_items(&input["args"]["tree"]).unwrap_or_default();
                if items != self.items {
                    self.items = items;
                    (self.row, self.sub, self.sub_row) = (-1, -1, -1);
                }
                let tree = if input["args"]["tree"].is_object() { convert(&input["args"]["tree"], (self.row, self.sub, self.sub_row)) } else { Value::Null };
                if tree == self.last {
                    return json!({"same":true});
                }
                self.last = tree.clone();
                json!({"tree":tree})
            }
            "menuHover" => {
                let i = input["key"].as_i64().unwrap_or(-1);
                let has_sub = usize::try_from(i).ok().and_then(|i| self.items.get(i)).is_some_and(|it| it["submenu"].is_array());
                let row = if selectable(&self.items, i) { i } else { -1 };
                let sub = if has_sub { i } else { -1 };
                if (row, sub) != (self.row, self.sub) {
                    (self.row, self.sub, self.sub_row) = (row, sub, -1);
                }
                json!({})
            }
            "menuSubHover" => {
                let i = input["key"].as_i64().unwrap_or(-1);
                let sub = usize::try_from(self.sub).ok().and_then(|s| self.items.get(s)).and_then(|it| it["submenu"].as_array());
                self.sub_row = if sub.is_some_and(|s| selectable(s, i)) { i } else { -1 };
                json!({})
            }
            "next" => json!({}),
            cmd => {
                let mut extra: Map<String, Value> = input.as_object().cloned().unwrap_or_default();
                extra.remove("cmd");
                extra.remove("key");
                let action = if input["key"].is_null() { json!([cmd]) } else { json!([cmd, input["key"]]) };
                json!({"effects":[{"kind":"tree","owner":self.owner,"action":action,"extra":extra}]})
            }
        }
    }
    /// Arrows, Enter and Escape move through an open menu; None while the tree has none.
    fn key(&mut self, key: &str, _chr: &str, _flags: i32) -> Option<Value> {
        if self.items.is_empty() {
            return None;
        }
        let in_sub = self.sub >= 0;
        let list: Vec<Value> = if in_sub {
            self.items.get(self.sub as usize).and_then(|it| it["submenu"].as_array()).cloned().unwrap_or_default()
        } else {
            self.items.clone()
        };
        let active = if in_sub { self.sub_row } else { self.row };
        let effect = |action: Value| json!({"effects":[{"kind":"tree","owner":self.owner,"action":action,"extra":{}}]});
        match key {
            "ArrowDown" | "ArrowUp" | "Home" | "End" => {
                let dir = if matches!(key, "ArrowUp" | "End") { -1 } else { 1 };
                let from = if matches!(key, "Home" | "End") || active < 0 { if dir > 0 { -1 } else { 0 } } else { active };
                if let Some(next) = advance(&list, from, dir) {
                    if in_sub {
                        self.sub_row = next;
                    } else {
                        (self.row, self.sub) = (next, -1);
                    }
                }
            }
            "ArrowRight" if !in_sub => {
                if let Some(sub) = usize::try_from(self.row).ok().and_then(|i| list.get(i)).and_then(|it| it["submenu"].as_array()) {
                    self.sub = self.row;
                    self.sub_row = advance(sub, -1, 1).unwrap_or(-1);
                }
            }
            "ArrowLeft" if in_sub => (self.sub, self.sub_row) = (-1, -1),
            "ArrowLeft" => {}
            "Escape" => return Some(effect(json!(["dismiss"]))),
            "Enter" | " " | "Space" => {
                if selectable(&list, active) {
                    let item = list[active as usize].clone();
                    if !in_sub && item["submenu"].is_array() {
                        self.sub = self.row;
                        self.sub_row = advance(item["submenu"].as_array().unwrap(), -1, 1).unwrap_or(-1);
                    } else if item["action"].is_array() {
                        return Some(effect(item["action"].clone()));
                    }
                }
            }
            _ => return None,
        }
        Some(json!({}))
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
        app.request(&json!({"cmd":"sync","args":{"tree":tree,"owner":"spotlight"}}))
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
        sync(&mut app, json!({"kind":"view"}));
        let reply = app.request(&json!({"cmd":"key","key":"ArrowDown","meta":true,"shift":false}));
        assert_eq!(reply["effects"][0]["kind"], "tree");
        assert_eq!(reply["effects"][0]["owner"], "spotlight", "the effect says whose view it was");
        assert_eq!(reply["effects"][0]["action"], json!(["key", "ArrowDown"]));
        assert_eq!(reply["effects"][0]["extra"]["meta"], true);
        let typed = app.request(&json!({"cmd":"query","key":"hello"}));
        assert_eq!(typed["effects"][0]["action"], json!(["query", "hello"]));
        let close = app.request(&json!({"cmd":"close"}));
        assert_eq!(close["effects"][0]["action"], json!(["close"]));
        assert_eq!(app.request(&json!({"cmd":"menuHover","key":1})), json!({}));
    }

    fn menu_tree() -> Value {
        json!({"kind":"view","children":[{"kind":"menu","absolute":true,"x":50,"y":60,"events":{"onDismiss":["dismiss"]},
            "items":[{"label":"Open","action":["run","a"]},"-",{"label":"More","submenu":[{"label":"Sub","action":["run","s"]}]},{"label":"Quit","action":["run","q"]}]}]})
    }
    #[test]
    fn a_menu_node_keeps_its_rows_and_moves_with_the_keyboard() {
        let mut app = Tree::default();
        assert!(app.key("ArrowDown", "", 0).is_none(), "no menu: the shell keeps its keys");
        let tree = sync(&mut app, menu_tree())["tree"].clone();
        assert_eq!(tree["children"][0]["active"], -1);
        app.key("ArrowDown", "", 0);
        app.key("ArrowDown", "", 0);
        let tree = sync(&mut app, menu_tree())["tree"].clone();
        assert_eq!(tree["children"][0]["active"], 2, "the separator is skipped");
        app.key("ArrowRight", "", 0);
        let tree = sync(&mut app, menu_tree())["tree"].clone();
        assert_eq!((tree["children"][0]["sub"].clone(), tree["children"][0]["subActive"].clone()), (json!(2), json!(0)));
        let run = app.key("Enter", "", 0).unwrap();
        assert_eq!(run["effects"][0]["action"], json!(["run", "s"]));
        assert_eq!(run["effects"][0]["owner"], "spotlight");
        let esc = app.key("Escape", "", 0).unwrap();
        assert_eq!(esc["effects"][0]["action"], json!(["dismiss"]));
    }
    #[test]
    fn hovering_rows_lights_them_and_a_new_menu_starts_clean() {
        let mut app = Tree::default();
        sync(&mut app, menu_tree());
        app.request(&json!({"cmd":"menuHover","key":2}));
        app.request(&json!({"cmd":"menuSubHover","key":0}));
        let tree = sync(&mut app, menu_tree())["tree"].clone();
        let menu = &tree["children"][0];
        assert_eq!((menu["active"].clone(), menu["sub"].clone(), menu["subActive"].clone()), (json!(2), json!(2), json!(0)));
        app.request(&json!({"cmd":"menuHover","key":1}));
        let tree = sync(&mut app, menu_tree())["tree"].clone();
        assert_eq!(tree["children"][0]["active"], -1, "a separator lights nothing");
        let mut other = menu_tree();
        other["children"][0]["items"] = json!([{"label":"Only","action":["run","o"]}]);
        app.request(&json!({"cmd":"menuHover","key":0}));
        let tree = sync(&mut app, other)["tree"].clone();
        assert_eq!(tree["children"][0]["active"], -1, "other items, other rows");
    }
}
