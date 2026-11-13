//! The launcher (the grid of apps, "flow" mode, and the start menu, "classic") as a native view: `shell:launcher`, mounted by the
//! shell over the display's work area in place of the React widget of sdk/rustlauncher.jsx. The
//! state, the layout and the decisions are the launcher's own (rust/axia-shell/src/launcher): this
//! only asks it for the tree and sends it the events, as `lord.launcher.request` does for React,
//! and turns its widget tree into what the host draws: app icons are `appIcon` nodes (the host has
//! the catalog), the search field is a native input, the scroll regions come whole (the rows the
//! launcher plans for the first screens of pixels), a menu is a `"kind":"menu"` node.
//!
//! Facts: `args = {"mode":"flow"|"classic","info":{"fullName","user"},"session":{"managed","canPower"},
//! "dock":{<app id>:{"id","pinned","removable"}}}` and the `settings` and `catalog` facts. Every
//! effect of the launcher goes to the shell as `{"kind":"launcher","effect":{…}}` (close, open an
//! app, start a drag to the Dock, pin, settings, session; sdk/nativelauncher.js runs them).
use super::NativeApp;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Rows planned per scroll region: the launcher caps a window at this many pixels.
const SCROLL_PX: f64 = 16384.;

type Call = Box<dyn FnMut(&Value) -> Result<Value, &'static str>>;

pub struct LauncherView {
    call: Call,
    /// The launcher's handle for this menu (0 while it is not open).
    id: u64,
    context: Value,
    classic: bool,
    /// The launcher's last widget tree and what the host was given for it.
    raw: Value,
    last: Value,
    /// Effects that came with a reply and wait for the next answer to the host.
    pending: Vec<Value>,
    /// Search results by key, for the icons of their rows.
    results: HashMap<String, Value>,
    dock: Value,
}

impl Default for LauncherView {
    fn default() -> Self {
        Self::with(Box::new(crate::launcher::shell_request))
    }
}

fn rtl(lang: &str) -> bool {
    matches!(lang, "ar" | "he" | "fa" | "ur")
}

impl LauncherView {
    pub fn with(call: Call) -> Self {
        Self { call, id: 0, context: Value::Null, classic: false, raw: Value::Null, last: Value::Null, pending: Vec::new(), results: HashMap::new(), dock: Value::Null }
    }
    fn context(input: &Value) -> Value {
        let mut theme = input["context"]["theme"].clone();
        if let Some(t) = theme.as_object_mut() {
            t.remove("wallpapers");
            t.remove("accents");
        }
        json!({"theme":theme,
            // the flow mode reads x, y, w and h; the classic one, left, top, width and height
            "area":{"x":0,"y":0,"w":input["context"]["width"],"h":input["context"]["height"],
                "left":0,"top":0,"width":input["context"]["width"],"height":input["context"]["height"]},
            "settings":{"startPinned":input["settings"]["startPinned"],"dockRecentApps":input["settings"]["dockRecentApps"]},
            "info":input["args"]["info"],"rtl":rtl(input["lang"].as_str().unwrap_or("")),"session":input["args"]["session"]})
    }
    fn take(&mut self, reply: Result<Value, &'static str>) {
        let Ok(reply) = reply else { return };
        if !reply["tree"].is_null() {
            self.raw = reply["tree"].clone();
        }
        for effect in reply["effects"].as_array().into_iter().flatten() {
            self.pending.push(json!({"kind":"launcher","effect":effect}));
        }
    }
    /// `hover` carries its on/off in the action's third element, which the host does not send.
    fn actions(events: &Value) -> Value {
        let Some(map) = events.as_object() else { return Value::Null };
        let mut out = Map::new();
        for (name, action) in map {
            let mut action = action.clone();
            if action[0] == "hover" {
                action = json!([if action[2] == true { "hoverOn" } else { "hoverOff" }, action[1]]);
            }
            out.insert(name.clone(), action);
        }
        Value::Object(out)
    }
    fn convert(&mut self, node: &Value) -> Value {
        match node["kind"].as_str().unwrap_or("") {
            "icon" => json!({"kind":"appIcon","app":node["app"],"size":node["size"]}),
            "resultIcon" => self.result_icon(node),
            "input" => {
                let mut out = json!({"kind":"input","key":node["key"],"style":node["style"],"value":node["value"],
                    "placeholder":node["placeholder"],"autoFocus":node["autoFocus"],
                    // no "change": the host then sends ["query", text], what the launcher takes
                    "submit":["submit"],"cancel":["cancel",node["value"]]});
                // the start menu moves through its results with the arrows and runs one with Enter
                if self.classic {
                    out["keys"] = json!(["ArrowDown", "ArrowUp", "Enter"]);
                    out["onKey"] = json!(["key"]);
                }
                out
            }
            "scroll" => self.scroll(node),
            "menu" => {
                let mut out = node.clone();
                out["layer"] = json!("popover");
                out["events"] = json!({"onDismiss":node["close"]});
                out
            }
            "text" => {
                let mut out = node.clone();
                if node["events"].is_object() {
                    out["events"] = Self::actions(&node["events"]);
                }
                out
            }
            _ => {
                let mut out = node.clone();
                if node["events"].is_object() {
                    out["events"] = Self::actions(&node["events"]);
                }
                if let Some(children) = node["children"].as_array() {
                    out["children"] = Value::Array(children.iter().map(|c| self.convert(c)).collect());
                }
                out
            }
        }
    }
    /// A scroll region whole: the content the launcher plans from the top, in the host's scroll view.
    fn scroll(&mut self, node: &Value) -> Value {
        let mut out = node.clone();
        let content = (self.call)(&json!({"op":"scroll","id":self.id,"key":node["region"],"offset":0,"height":SCROLL_PX}))
            .ok()
            .map(|reply| reply["tree"].clone())
            .filter(|tree| !tree.is_null());
        out["children"] = match content {
            Some(tree) => json!([self.convert(&tree)]),
            None => json!([]),
        };
        out
    }
    fn result_icon(&self, node: &Value) -> Value {
        let size = node["size"].as_f64().unwrap_or(28.);
        let item = self.results.get(node["result"].as_str().unwrap_or(""));
        if let Some(app) = item.and_then(|i| i["app"].as_str()) {
            return json!({"kind":"appIcon","app":app,"size":size});
        }
        let glyph = item.and_then(|i| i["glyph"].as_str()).unwrap_or("");
        json!({"kind":"view","style":{"width":size,"height":size,"borderRadius":size*0.225,"alignItems":"center","justifyContent":"center",
            "backgroundColor":"rgba(128,128,128,0.35)"},
            "children":[{"kind":"text","label":glyph,"style":{"fontSize":size*0.5,"color":"#ffffff"}}]})
    }
    fn dock_of(&self, app: &str) -> Value {
        self.dock[app].clone()
    }
    fn event(&mut self, action: Value, input: &Value) {
        if self.id == 0 {
            return;
        }
        let mut args = json!({"op":"event","id":self.id,"action":action,"dock":Value::Null,"dragJustEnded":false,"insidePopup":false});
        for k in ["x", "y", "button", "query", "sections"] {
            if !input[k].is_null() {
                args[k] = input[k].clone();
            }
        }
        if let Some(app) = args["action"][1].as_str() {
            args["dock"] = self.dock_of(app);
        }
        if let Some(sections) = args["sections"].as_array() {
            self.results = sections
                .iter()
                .flat_map(|s| s["items"].as_array().into_iter().flatten())
                .filter_map(|i| Some((i["key"].as_str()?.to_owned(), i.clone())))
                .collect();
        }
        let reply = (self.call)(&args);
        self.take(reply);
    }
    fn answer(&mut self) -> Value {
        let tree = if self.raw.is_null() { Value::Null } else { self.convert(&self.raw.clone()) };
        let effects = std::mem::take(&mut self.pending);
        if tree == self.last || tree.is_null() {
            return if effects.is_empty() { json!({"same":true}) } else { json!({"same":true,"effects":effects}) };
        }
        self.last = tree.clone();
        json!({"tree":tree,"effects":effects})
    }
}

impl NativeApp for LauncherView {
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                let context = Self::context(input);
                self.dock = input["args"]["dock"].clone();
                if self.id == 0 {
                    self.classic = input["args"]["mode"] == "classic";
                    let reply = (self.call)(&json!({"op":"open","mode":if self.classic {"classic"} else {"flow"},"context":context}));
                    if let Ok(r) = &reply {
                        self.id = r["id"].as_u64().unwrap_or(0);
                    }
                    self.take(reply);
                    self.context = context;
                } else if context != self.context {
                    let reply = (self.call)(&json!({"op":"context","id":self.id,"context":context}));
                    self.take(reply);
                    self.context = context;
                }
                self.answer()
            }
            "close" => {
                if self.id != 0 {
                    let _ = (self.call)(&json!({"op":"close","id":self.id}));
                    self.id = 0;
                }
                json!({})
            }
            "hoverOn" => {
                self.event(json!(["hover", input["key"], true]), input);
                json!({"effects":std::mem::take(&mut self.pending)})
            }
            "hoverOff" => {
                self.event(json!(["hover", input["key"], false]), input);
                json!({"effects":std::mem::take(&mut self.pending)})
            }
            // the rows of a native menu light up by themselves; the launcher has no keyboard row
            "menuHover" | "menuSubHover" | "next" => json!({}),
            cmd => {
                let action = if input["key"].is_null() { json!([cmd]) } else { json!([cmd, input["key"]]) };
                self.event(action, input);
                json!({"effects":std::mem::take(&mut self.pending)})
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::AxiaLauncher;
    use std::{cell::RefCell, rc::Rc};

    fn view() -> (LauncherView, Rc<RefCell<AxiaLauncher>>) {
        let launcher = Rc::new(RefCell::new(AxiaLauncher::default()));
        let apps: Vec<Value> = ["Calculator", "Browser", "Notes"]
            .iter()
            .map(|n| json!({"id":n.to_lowercase(),"name":n,"categories":"Utility","kind":"lordos"}))
            .collect();
        launcher.borrow_mut().replace(&json!(apps)).unwrap();
        let shared = launcher.clone();
        (LauncherView::with(Box::new(move |args| shared.borrow_mut().request(args))), launcher)
    }
    fn sync(width: f64) -> Value {
        json!({"cmd":"sync","lang":"en","settings":{},
            "context":{"width":width,"height":700.,"theme":{"text":"#ffffff","textDim":"#999999","accent":"#0a84ff","mode":"dark","wallpapers":[1,2]}},
            "args":{"mode":"flow","info":{"fullName":"Ada Lovelace","user":"ada"},"session":{"managed":false,"canPower":false},"dock":{}}})
    }
    fn find<'a>(tree: &'a Value, test: &dyn Fn(&Value) -> bool) -> Option<&'a Value> {
        if test(tree) {
            return Some(tree);
        }
        tree["children"].as_array()?.iter().find_map(|c| find(c, test))
    }

    #[test]
    fn the_first_sync_opens_the_menu_and_draws_a_tree_the_host_understands() {
        let (mut v, _) = view();
        let reply = v.request(&sync(1000.));
        let tree = &reply["tree"];
        assert!(tree.is_object());
        assert!(find(tree, &|n| n["kind"] == "scroll").is_some());
        assert!(find(tree, &|n| n["kind"] == "scroll" && n["children"].as_array().is_some_and(|c| !c.is_empty())).is_some(),
            "a scroll region comes with its content");
        assert!(find(tree, &|n| n["kind"] == "icon" || n["kind"] == "resultIcon").is_none(), "no node left for React to draw");
        let icon = find(tree, &|n| n["kind"] == "appIcon" && n["app"] == "calculator").expect("an app icon node");
        assert!(icon["size"].as_f64().unwrap() > 0.);
        let input = find(tree, &|n| n["kind"] == "input").expect("the search field");
        assert!(input["change"].is_null(), "the host's default is [\"query\", text]");
        assert_eq!(input["submit"], json!(["submit"]));
        assert_eq!(input["cancel"][0], "cancel");
        assert_eq!(v.request(&sync(1000.))["same"], true, "an unchanged context sends no tree");
    }
    #[test]
    fn a_new_area_is_a_new_context() {
        let (mut v, _) = view();
        v.request(&sync(1000.));
        let reply = v.request(&sync(640.));
        assert!(reply["tree"].is_object(), "the launcher re-lays itself out in the new width");
    }
    #[test]
    fn hover_actions_keep_their_on_off_through_the_host() {
        let events = LauncherView::actions(&json!({"onMouseEnter":["hover","a",true],"onMouseLeave":["hover","a",false],"onClick":["open","a"]}));
        assert_eq!(events["onMouseEnter"], json!(["hoverOn", "a"]));
        assert_eq!(events["onMouseLeave"], json!(["hoverOff", "a"]));
        assert_eq!(events["onClick"], json!(["open", "a"]));
        let (mut v, _) = view();
        v.request(&sync(1000.));
        assert_eq!(v.request(&json!({"cmd":"hoverOn","key":"calculator"})), json!({"effects":[]}));
        assert_eq!(v.request(&json!({"cmd":"hoverOff","key":"calculator"})), json!({"effects":[]}));
    }
    #[test]
    fn opening_an_app_is_an_effect_for_the_shell() {
        let (mut v, _) = view();
        let tree = v.request(&sync(1000.))["tree"].clone();
        let tile = find(&tree, &|n| n["events"]["onClick"][0] == "open").expect("a tile that opens an app");
        let action = tile["events"]["onClick"].clone();
        let reply = v.request(&json!({"cmd":action[0],"key":action[1],"x":10.,"y":10.,"button":0}));
        let effects = reply["effects"].as_array().unwrap();
        assert!(effects.iter().all(|e| e["kind"] == "launcher"));
        assert!(effects.iter().any(|e| e["effect"]["type"] == "open" && e["effect"]["id"] == action[1]), "{effects:?}");
    }
    #[test]
    fn typing_filters_and_submit_opens_the_first_match() {
        let (mut v, _) = view();
        v.request(&sync(1000.));
        v.request(&json!({"cmd":"query","key":"brow"}));
        let reply = v.request(&json!({"cmd":"submit"}));
        assert!(reply["effects"].as_array().unwrap().iter().any(|e| e["effect"]["type"] == "open" && e["effect"]["id"] == "browser"), "{reply}");
    }
    #[test]
    fn the_dock_facts_reach_the_context_menu_and_a_pin_becomes_an_effect() {
        let (mut v, _) = view();
        let mut input = sync(1000.);
        input["args"]["dock"] = json!({"notes":{"id":"app:notes","pinned":false,"removable":true}});
        v.request(&input);
        v.request(&json!({"cmd":"context","key":"notes","x":100.,"y":100.}));
        let reply = v.request(&json!({"cmd":"dock","key":"notes"}));
        assert!(reply["effects"].as_array().unwrap().iter().any(|e| e["effect"] == json!({"type":"dock","id":"notes","pin":true})), "{reply}");
    }
    #[test]
    fn closing_frees_the_menu_and_stops_answering() {
        let (mut v, launcher) = view();
        v.request(&sync(1000.));
        assert_eq!(launcher.borrow().menu_count(), 1);
        v.request(&json!({"cmd":"close"}));
        assert_eq!(launcher.borrow().menu_count(), 0);
        assert_eq!(v.request(&json!({"cmd":"query","key":"x"})), json!({"effects":[]}));
    }
    #[test]
    fn a_menu_node_dismisses_through_the_host() {
        let (mut v, _) = view();
        v.request(&sync(1000.));
        v.request(&json!({"cmd":"powerMenu","x":300.,"y":300.}));
        let tree = v.request(&sync(1000.))["tree"].clone();
        let menu = find(&tree, &|n| n["kind"] == "menu").expect("the power menu");
        assert_eq!(menu["layer"], "popover");
        assert_eq!(menu["events"]["onDismiss"], json!(["closePower"]));
    }

    fn classic_sync() -> Value {
        let mut input = sync(400.);
        input["args"]["mode"] = json!("classic");
        input
    }
    #[test]
    fn the_start_menu_opens_for_a_search_and_its_field_passes_the_arrows() {
        let (mut v, _) = view();
        let reply = v.request(&classic_sync());
        assert!(reply["effects"].as_array().unwrap().iter().any(|e| e["effect"]["type"] == "prepareSearch"), "{reply}");
        let input = find(&reply["tree"], &|n| n["kind"] == "input").expect("the search field");
        assert_eq!(input["keys"], json!(["ArrowDown", "ArrowUp", "Enter"]));
        assert_eq!(input["onKey"], json!(["key"]));
        let flow = LauncherView::with(Box::new(|_| Err("unused")));
        assert!(!flow.classic);
    }
    #[test]
    fn typing_in_the_start_menu_asks_the_shell_to_search_and_its_results_come_back() {
        let (mut v, _) = view();
        v.request(&classic_sync());
        let reply = v.request(&json!({"cmd":"query","key":"calc"}));
        let search = reply["effects"].as_array().unwrap().iter().find(|e| e["effect"]["type"] == "search").expect("a search effect");
        assert_eq!(search["effect"]["query"], "calc");
        let sections = json!([{"id":"apps","title":"Apps","items":[{"key":"app:calculator","title":"Calculator","subtitle":"","glyph":"","hasIcon":true,"hasImage":false,"app":"calculator"}]}]);
        v.request(&json!({"cmd":"results","query":"calc","sections":sections}));
        let tree = v.request(&classic_sync())["tree"].clone();
        let text = tree.to_string();
        assert!(text.contains("Calculator"), "the result is listed");
        assert!(find(&tree, &|n| n["kind"] == "appIcon" && n["app"] == "calculator" && n["size"].as_f64() == Some(28.)).is_some(), "with its icon: {text}");
        let enter = v.request(&json!({"cmd":"key","key":"Enter"}));
        assert!(enter["effects"].as_array().unwrap().iter().any(|e| e["effect"]["type"] == "runSearch"), "{enter}");
    }
}
