//! The launcher (the grid of apps of Lord Flow) as a native view: `shell:launcher`, mounted by the
//! shell over the display's work area in place of the React widget of sdk/rustlauncher.jsx. The
//! state, the layout and the decisions are the launcher's own (rust/axia-shell/src/launcher): this
//! only asks it for the tree and sends it the events, as `lord.launcher.request` does for React,
//! and turns its widget tree into what the host draws: app icons are `appIcon` nodes (the host has
//! the catalog), the search field is a native input, scroll regions mount only the visible rows
//! plus overscan, and a menu is a `"kind":"menu"` node.
//!
//! Facts: `args = {"mode":"flow","dock":{<app id>:{"id","pinned","removable"}}}` and the `catalog`
//! fact. Every effect of the launcher goes to the shell as `{"kind":"launcher","effect":{…}}`
//! (close, open an app, start a drag to the Dock, pin; sdk/nativelauncher.js runs them).
use super::NativeApp;
use serde_json::{json, Value};
use super::menubar::{advance, selectable};

type Call = Box<dyn FnMut(&Value, Option<std::rc::Rc<Value>>) -> Result<Value, &'static str>>;

pub struct LauncherView {
    call: Call,
    /// The launcher's handle for this menu (0 while it is not open).
    id: u64,
    context: Value,
    appearance: Option<std::rc::Rc<Value>>,
    sent_appearance: Option<std::rc::Rc<Value>>,
    /// The launcher's last widget tree and what the host was given for it.
    raw: Value,
    last: Value,
    /// Effects that came with a reply and wait for the next answer to the host.
    pending: Vec<Value>,
    dock: Value,
    catalog: Value,
    scroll_key: Value,
    scroll_offset: f64,
    scroll_height: f64,
    menu_items: Vec<Value>,
    menu_row: i64,
}

impl Default for LauncherView {
    fn default() -> Self {
        Self::with(Box::new(crate::launcher::shell_menu_request))
    }
}

fn rtl(lang: &str) -> bool {
    matches!(lang, "ar" | "he" | "fa" | "ur")
}

impl LauncherView {
    pub fn with(call: Call) -> Self {
        Self { call, id: 0, appearance: None, sent_appearance: None, context: Value::Null, raw: Value::Null, last: Value::Null, pending: Vec::new(), dock: Value::Null,
            catalog: Value::Null, scroll_key: Value::Null, scroll_offset: 0., scroll_height: 0.,
            menu_items: Vec::new(), menu_row: -1 }
    }
    fn backend(&mut self, args: &Value) -> Result<Value, &'static str> {
        (self.call)(args, self.appearance.clone())
    }
    fn context(&self, input: &Value) -> Value {
        let mut context = json!({"area":{"x":0,"y":0,"w":input["context"]["width"],"h":input["context"]["height"]},
            "rtl":rtl(input["lang"].as_str().unwrap_or(""))});
        if self.appearance.is_some() { return context; }
        let mut theme = input["context"]["theme"].clone();
        if let Some(t) = theme.as_object_mut() {
            t.remove("wallpapers");
            t.remove("accents");
        }
        context["theme"] = theme;
        context
    }
    fn take(&mut self, reply: Result<Value, &'static str>) {
        let Ok(reply) = reply else { return };
        if !reply["tree"].is_null() {
            self.raw = reply["tree"].clone();
            fn items(node:&Value)->Option<Vec<Value>> {
                if node["kind"]=="menu" { return node["items"].as_array().cloned(); }
                node["children"].as_array()?.iter().find_map(items)
            }
            let menu=items(&self.raw).unwrap_or_default();
            if menu!=self.menu_items { self.menu_items=menu; self.menu_row=-1; }
        }
        for effect in reply["effects"].as_array().into_iter().flatten() {
            self.pending.push(json!({"kind":"launcher","effect":effect}));
        }
    }
    fn convert(&mut self, node: &Value) -> Value {
        match node["kind"].as_str().unwrap_or("") {
            "icon" => json!({"kind":"appIcon","app":node["app"],"size":node["size"]}),
            "input" => {
                // no "change": the host then sends ["query", text], what the launcher takes
                json!({"kind":"input","key":node["key"],"style":node["style"],"value":node["value"],
                    "placeholder":node["placeholder"],"autoFocus":node["autoFocus"],
                    "submit":["submit"],"cancel":["cancel",node["value"]]})
            }
            "scroll" => self.scroll(node),
            "menu" => {
                let mut out = node.clone();
                out["layer"] = json!("popover");
                out["events"] = json!({"onDismiss":node["close"]});
                out["active"] = json!(self.menu_row);
                out
            }
            "text" => node.clone(),
            _ => {
                let mut out = node.clone();
                // "close" is the NativeApp lifecycle command; a background click
                // must emit the shell close effect before unmount ends that lifecycle.
                if node["root"] == true { out["events"]["onClick"] = json!(["cancel"]); }
                if let Some(children) = node["children"].as_array() {
                    out["children"] = Value::Array(children.iter().map(|c| self.convert(c)).collect());
                }
                out
            }
        }
    }
    /// Keep the full content height for the thumb, but mount only a viewport of rows.
    fn scroll(&mut self, node: &Value) -> Value {
        let mut out = node.clone();
        if node["key"] != self.scroll_key {
            self.scroll_key = node["key"].clone();
            self.scroll_offset = 0.;
            self.scroll_height = 0.;
        }
        let height = if self.scroll_height > 0. { self.scroll_height } else { node["style"]["height"].as_f64().unwrap_or(510.).max(1.) };
        let reply = self.backend(&json!({"op":"scroll","id":self.id,"key":node["region"],"offset":self.scroll_offset,"height":height})).ok();
        if let Some(offset) = reply.as_ref().and_then(|reply| reply["offset"].as_f64()) {
            self.scroll_offset = offset;
        }
        out["offset"] = json!(self.scroll_offset);
        let content = reply.map(|reply| reply["tree"].clone()).filter(|tree| !tree.is_null());
        out["children"] = match content {
            Some(tree) => json!([self.convert(&tree)]),
            None => json!([]),
        };
        out["onScroll"] = json!(["scroll", self.scroll_key]);
        out
    }
    fn dock_of(&self, app: &str) -> Value {
        self.dock[app].clone()
    }
    fn event(&mut self, action: Value, input: &Value) {
        if self.id == 0 {
            return;
        }
        let mut args = json!({"op":"event","id":self.id,"action":action,"dock":Value::Null,"dragJustEnded":false,"insidePopup":false});
        for k in ["x", "y", "button"] {
            if !input[k].is_null() {
                args[k] = input[k].clone();
            }
        }
        if let Some(app) = args["action"][1].as_str() {
            args["dock"] = self.dock_of(app);
        }
        let reply = self.backend(&args);
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
    fn uses_native_appearance(&self) -> bool { true }
    fn uses_shell_appearance(&self) -> bool { true }
    fn set_native_appearance(&mut self, theme: std::rc::Rc<Value>) { self.appearance = Some(theme); }
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                let context = self.context(input);
                let appearance_changed = match (&self.appearance, &self.sent_appearance) {
                    (Some(a),Some(b)) => !std::rc::Rc::ptr_eq(a,b), (None,None) => false, _ => true,
                };
                let catalog = input["catalog"].clone();
                self.dock = input["args"]["dock"].clone();
                if self.id == 0 {
                    let reply = self.backend(&json!({"op":"open","mode":"flow","context":context}));
                    if let Ok(r) = &reply {
                        self.id = r["id"].as_u64().unwrap_or(0);
                    }
                    self.take(reply);
                    self.context = context;
                } else if context != self.context || catalog != self.catalog || appearance_changed {
                    if context != self.context || catalog != self.catalog { self.scroll_height = 0.; }
                    let reply = self.backend(&json!({"op":"context","id":self.id,"context":context}));
                    self.take(reply);
                    self.context = context;
                }
                self.sent_appearance = self.appearance.clone();
                self.catalog = catalog;
                self.answer()
            }
            "scroll" => {
                if input["key"] == self.scroll_key {
                    self.scroll_offset = input["offset"].as_f64().filter(|v| v.is_finite()).unwrap_or(0.).clamp(0., 1e9);
                    self.scroll_height = input["height"].as_f64().filter(|v| v.is_finite()).unwrap_or(1.).clamp(1., 16384.);
                }
                json!({})
            }
            "close" => {
                if self.id != 0 {
                    let _ = self.backend(&json!({"op":"close","id":self.id}));
                    self.id = 0;
                }
                json!({})
            }
            "menuHover" => {
                let row=input["key"].as_i64().unwrap_or(-1);
                self.menu_row=if selectable(&self.menu_items,row) {row} else {-1};
                json!({})
            }
            "menuSubHover" | "next" => json!({}),
            cmd => {
                let action = if input["key"].is_null() { json!([cmd]) } else { json!([cmd, input["key"]]) };
                self.event(action, input);
                json!({"effects":std::mem::take(&mut self.pending)})
            }
        }
    }
    fn key(&mut self,key:&str,_chr:&str,_flags:i32)->Option<Value> {
        if self.menu_items.is_empty() {
            if key!="Escape" { return None; }
            self.event(json!(["cancel"]),&json!({}));
        } else {
            match key {
                "ArrowDown"|"ArrowUp"|"Home"|"End"=>{
                    let dir=if matches!(key,"ArrowUp"|"End") {-1} else {1};
                    let from=if matches!(key,"Home"|"End")||self.menu_row<0 {if dir>0 {-1} else {0}} else {self.menu_row};
                    self.menu_row=advance(&self.menu_items,from,dir).unwrap_or(-1);
                }
                "Escape"=>self.event(json!(["closeContext"]),&json!({})),
                "Enter"|" "|"Space"=>{
                    if selectable(&self.menu_items,self.menu_row) {
                        let action=self.menu_items[self.menu_row as usize]["action"].clone();
                        self.event(action,&json!({}));
                    }
                }
                _=>return None,
            }
        }
        Some(json!({"effects":std::mem::take(&mut self.pending)}))
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
        (LauncherView::with(Box::new(move |args, appearance| shared.borrow_mut().menu_request(args, appearance))), launcher)
    }
    fn sync(width: f64) -> Value {
        json!({"cmd":"sync","lang":"en","settings":{},
            "context":{"width":width,"height":700.,"theme":{"text":"#ffffff","textDim":"#999999","accent":"#0a84ff","mode":"dark","wallpapers":[1,2]}},
            "args":{"mode":"flow","dock":{}}})
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
        assert!(find(tree, &|n| n["kind"] == "icon").is_none(), "no node left for React to draw");
        let icon = find(tree, &|n| n["kind"] == "appIcon" && n["app"] == "calculator").expect("an app icon node");
        assert!(icon["size"].as_f64().unwrap() > 0.);
        let input = find(tree, &|n| n["kind"] == "input").expect("the search field");
        assert!(input["change"].is_null(), "the host's default is [\"query\", text]");
        assert_eq!(input["submit"], json!(["submit"]));
        assert_eq!(input["cancel"][0], "cancel");
        assert_eq!(v.request(&sync(1000.))["same"], true, "an unchanged context sends no tree");
    }
    #[test]
    fn native_colors_preserve_query_popup_and_measured_scroll_viewport() {
        let (mut v, _) = view();
        let first = Rc::new(json!({"mode":"dark","accent":"#123456","effects":false}));
        v.set_native_appearance(first);
        let input = sync(1000.);
        v.request(&input);
        assert!(v.context.get("theme").is_none(), "native colors never enter the JSON context");
        v.request(&json!({"cmd":"query","key":"Notes"}));
        v.request(&json!({"cmd":"context","key":"notes","x":200.,"y":200.}));
        let before = v.request(&input)["tree"].clone();
        v.request(&json!({"cmd":"scroll","key":v.scroll_key,"offset":0.,"height":321.}));
        let key = v.scroll_key.clone();
        let next = Rc::new(json!({"mode":"light","accent":"#654321","effects":true}));
        v.set_native_appearance(next.clone());
        let tree = v.request(&input)["tree"].clone();
        assert_eq!(v.scroll_height, 321., "a color change must keep the measured viewport");
        assert_eq!(v.scroll_key, key, "a color change must not reset the query generation");
        assert_eq!(find(&tree, &|n| n["kind"] == "input").unwrap()["value"], "Notes");
        assert!(find(&before, &|n| n["kind"] == "menu").is_some());
        assert!(find(&tree, &|n| n["kind"] == "menu").is_some());
        assert!(Rc::ptr_eq(v.sent_appearance.as_ref().unwrap(), &next));
        assert!(v.context.get("theme").is_none());
        let reply = v.request(&json!({"cmd":"submit"}));
        assert!(reply["effects"].as_array().unwrap().iter().any(|e| e["effect"]["id"] == "notes"));
    }
    #[test]
    fn a_new_area_is_a_new_context() {
        let (mut v, _) = view();
        v.request(&sync(1000.));
        let reply = v.request(&sync(640.));
        assert!(reply["tree"].is_object(), "the launcher re-lays itself out in the new width");
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
        v.request(&json!({"cmd":"context","key":"notes","x":300.,"y":300.}));
        let tree = v.request(&sync(1000.))["tree"].clone();
        let menu = find(&tree, &|n| n["kind"] == "menu").expect("the app's context menu");
        assert_eq!(menu["layer"], "popover");
        assert_eq!(menu["events"]["onDismiss"], json!(["closeContext"]));
    }
    #[test]
    fn viewport_scroll_reaches_the_last_app_and_discards_old_query_offsets() {
        let (mut v, model) = view();
        model.borrow_mut().replace(&json!((0..2048).map(|i| json!({
            "id":format!("app-{i:04}"),"name":format!("App {i:04}"),"kind":"lordos"
        })).collect::<Vec<_>>())).unwrap();
        let first = v.request(&sync(1000.))["tree"].clone();
        fn icons(tree:&Value)->usize {
            usize::from(tree["kind"]=="appIcon")+tree["children"].as_array().into_iter().flatten().map(icons).sum::<usize>()
        }
        assert!(icons(&first)>0 && icons(&first)<=48);
        assert!(find(&first,&|n| n["app"]=="app-2047").is_none());
        let key = find(&first,&|n| n["kind"]=="scroll").unwrap()["key"].clone();
        v.request(&json!({"cmd":"scroll","key":key,"offset":1e9,"height":510.}));
        let end=v.request(&sync(1000.))["tree"].clone();
        assert!(find(&end,&|n| n["app"]=="app-2047").is_some());
        assert!(icons(&end)<=48);
        assert!(find(&end,&|n| n["kind"]=="scroll").unwrap()["offset"].as_f64().unwrap()>16384.);
        v.request(&json!({"cmd":"query","key":"App 0001"}));
        let filtered=v.request(&sync(1000.))["tree"].clone();
        assert_eq!(icons(&filtered),1);
        assert_eq!(find(&filtered,&|n| n["kind"]=="scroll").unwrap()["offset"],0.);
        v.request(&json!({"cmd":"scroll","key":key,"offset":1e9,"height":510.}));
        assert_eq!(v.scroll_offset,0.,"a delayed event cannot scroll a new query");
    }
    #[test]
    fn context_menu_keyboard_skips_locked_pins_and_escape_keeps_the_launcher() {
        let (mut v,_) = view();
        let mut input=sync(1000.);
        input["args"]["dock"]=json!({"notes":{"id":"app:notes","pinned":true,"removable":false}});
        v.request(&input);
        v.request(&json!({"cmd":"context","key":"notes","x":200.,"y":200.}));
        v.key("End","",0);
        assert_eq!(v.menu_row,0,"the disabled Keep in Dock item is skipped");
        let escape=v.key("Escape","",0).unwrap();
        assert_eq!(escape["effects"],json!([]));
        let tree=v.request(&input)["tree"].clone();
        assert!(find(&tree,&|n|n["kind"]=="menu").is_none());
        assert!(v.key("Escape","",0).unwrap()["effects"].as_array().unwrap().iter().any(|effect|effect["effect"]["type"]=="close"));
    }
    #[test]
    fn background_click_emits_close_before_the_host_closes_the_model() {
        let (mut v,model)=view();
        let tree=v.request(&sync(1000.))["tree"].clone();
        let action=&tree["events"]["onClick"];
        let reply=v.request(&json!({"cmd":action[0]}));
        assert!(reply["effects"].as_array().unwrap().iter().any(|effect|effect["effect"]["type"]=="close"));
        assert_eq!(model.borrow().menu_count(),1);
        v.request(&json!({"cmd":"close"}));
        assert_eq!(model.borrow().menu_count(),0);
    }
    #[test]
    fn catalog_changes_refresh_categories_and_narrow_screens_fit_four_columns() {
        let (mut v,model)=view();
        let mut input=sync(640.);
        v.request(&input);
        model.borrow_mut().replace(&json!((0..60).map(|i|json!({"id":format!("game-{i:02}"),"name":format!("Game {i:02}"),"category":"Game","kind":"lordos"})).collect::<Vec<_>>())).unwrap();
        input["catalog"]=json!([{"id":"game-00"}]);
        let tree=v.request(&input)["tree"].clone();
        assert!(find(&tree,&|n|n["label"]["key"]=="shell.category.Game").is_some());
        let row=find(&tree,&|n|n["style"]["height"]==98).unwrap();
        assert_eq!(row["children"].as_array().unwrap().len(),4);
        assert!(find(&tree,&|n|n["app"]=="game-00").is_some());
    }
}
