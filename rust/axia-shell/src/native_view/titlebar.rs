//! A window's title bar (title, close/minimize/zoom lights) as a native view: the shell mounts one
//! per framed window (`shell:titlebar`, sdk/nativetitlebar.jsx) in place of the React header of
//! themes/default/components/Window.jsx. The facts are `args = {"id","title","focused","height",
//! "controls":[{"key","color","glyph"}],"tokens":{"titleBg","text","textDim","lightOff"},
//! "chrome": null | {"header","controls","button","buttonHover","glyph","title"}}`, the `chrome`
//! plan being the one Rust already made for the window's appearance. The effects go to the shell,
//! which owns the window manager: `{"kind":"winDrag"|"winMax"|"winAct","id","key","x","y"}`.
use super::NativeApp;
use serde_json::{json, Value};

const TITLE_H: f64 = 40.;

#[derive(Default)]
pub struct TitleBar {
    /// The light under the pointer, "*" over the group between them, "" elsewhere.
    hot: String,
    last: Value,
    /// The window's id, from the last sync (an event carries only its action and pointer).
    id: Value,
}

fn merge(base: Value, over: &Value) -> Value {
    let mut out = base;
    if let (Some(a), Some(b)) = (out.as_object_mut(), over.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
    out
}

impl TitleBar {
    fn tree(&self, args: &Value) -> Value {
        let tokens = &args["tokens"];
        let chrome = args["chrome"].is_object().then(|| &args["chrome"]);
        let focused = args["focused"] == true;
        let height = args["height"].as_f64().unwrap_or(TITLE_H);
        let header = chrome.map(|c| c["header"].clone()).filter(Value::is_object).unwrap_or_else(|| {
            json!({"height":TITLE_H,"flexDirection":"row","alignItems":"center","paddingLeft":14,"backgroundColor":tokens["titleBg"]})
        });
        let title = merge(
            json!({"position":"absolute","left":0,"right":0,"top":0,"lineHeight":height,"textAlign":"center","fontSize":13,
                "fontWeight":600,"color":if focused {&tokens["text"]} else {&tokens["textDim"]},
                "pointerEvents":"none","whiteSpace":"nowrap","paddingHorizontal":80}),
            &chrome.map_or(Value::Null, |c| c["title"].clone()),
        );
        let group_hot = !self.hot.is_empty();
        let lights: Vec<Value> = args["controls"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|c| {
                let key = c["key"].as_str().unwrap_or("");
                let (style, glyph) = match chrome {
                    Some(plan) => {
                        let style = if self.hot == key { merge(plan["button"].clone(), &plan["buttonHover"]) } else { plan["button"].clone() };
                        let glyph = merge(plan["glyph"].clone(), &json!({"color":if focused {&tokens["text"]} else {&tokens["textDim"]}}));
                        (style, Some(glyph))
                    }
                    None => (
                        json!({"width":13,"height":13,"borderRadius":7,"alignItems":"center","justifyContent":"center",
                            "backgroundColor":if focused || group_hot {c["color"].clone()} else {tokens["lightOff"].clone()}}),
                        group_hot.then(|| json!({"fontSize":11,"fontWeight":700,"lineHeight":13,"color":"rgba(0,0,0,0.6)"})),
                    ),
                };
                json!({"kind":"view","key":key,"style":style,
                    // a press on a light is not the start of a window drag
                    "events":{"onMouseDown":["noop"],"onClick":["act",key],"onMouseEnter":["hot",key],"onMouseLeave":["hot",""]},
                    "children":glyph.map(|g| vec![json!({"kind":"text","label":c["glyph"],"style":g})]).unwrap_or_default()})
            })
            .collect();
        let controls_style = chrome
            .map(|c| c["controls"].clone())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({"flexDirection":"row","gap":8,"paddingVertical":6}));
        json!({"kind":"view","key":"titlebar","style":header,
            // onClick only so the host sends the press (it goes to the nearest view that takes clicks)
            "events":{"onClick":["noop"],"onMouseDown":["drag"],"onDoubleClick":["max"]},
            "children":[
                {"kind":"text","key":"title","label":args["title"],"style":title},
                {"kind":"view","key":"lights","style":controls_style,
                    "events":{"onMouseEnter":["hot","*"],"onMouseLeave":["hot",""]},"children":lights}]})
    }
}

impl NativeApp for TitleBar {
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "sync" => {
                self.id = input["args"]["id"].clone();
                let tree = self.tree(&input["args"]);
                if tree == self.last {
                    return json!({"same":true});
                }
                self.last = tree.clone();
                json!({"tree":tree})
            }
            "hot" => {
                self.hot = input["key"].as_str().unwrap_or("").to_owned();
                json!({})
            }
            "drag" => json!({"effects":[{"kind":"winDrag","id":self.id,"x":input["x"],"y":input["y"]}]}),
            "max" => json!({"effects":[{"kind":"winMax","id":self.id}]}),
            "act" => json!({"effects":[{"kind":"winAct","id":self.id,"key":input["key"]}]}),
            _ => json!({}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(chrome: Value, focused: bool) -> Value {
        json!({"id":"files","title":"Files","focused":focused,"height":40,"chrome":chrome,
            "controls":[{"key":"close","color":"#ff5f57","glyph":"×"},{"key":"min","color":"#febc2e","glyph":"−"}],
            "tokens":{"titleBg":"#222","text":"#fff","textDim":"#888","lightOff":"#555"}})
    }
    fn sync(app: &mut TitleBar, a: Value) -> Value {
        app.request(&json!({"cmd":"sync","args":a}))
    }
    fn act(app: &mut TitleBar, cmd: &str, key: Value) -> Value {
        app.request(&json!({"cmd":cmd,"key":key,"x":12.,"y":8.}))
    }

    #[test]
    fn draws_the_default_header_with_the_title_and_lights() {
        let mut app = TitleBar::default();
        let tree = sync(&mut app, args(Value::Null, true))["tree"].clone();
        assert_eq!(tree["style"]["height"].as_f64(), Some(40.));
        assert_eq!(tree["style"]["backgroundColor"], "#222");
        let title = &tree["children"][0];
        assert_eq!(title["label"], "Files");
        assert_eq!(title["style"]["color"], "#fff");
        let lights = tree["children"][1]["children"].as_array().unwrap();
        assert_eq!(lights.len(), 2);
        assert_eq!(lights[0]["style"]["backgroundColor"], "#ff5f57", "a focused window shows its colors");
        assert!(lights[0]["children"].as_array().unwrap().is_empty(), "and no glyph until the pointer is over them");
        assert_eq!(sync(&mut app, args(Value::Null, true))["same"], true);
    }
    #[test]
    fn an_unfocused_window_dims_its_lights_and_title_until_hovered() {
        let mut app = TitleBar::default();
        let tree = sync(&mut app, args(Value::Null, false))["tree"].clone();
        assert_eq!(tree["children"][0]["style"]["color"], "#888");
        assert_eq!(tree["children"][1]["children"][0]["style"]["backgroundColor"], "#555");
        act(&mut app, "hot", json!("*"));
        let tree = sync(&mut app, args(Value::Null, false))["tree"].clone();
        let light = &tree["children"][1]["children"][0];
        assert_eq!(light["style"]["backgroundColor"], "#ff5f57", "hovering the group lights them");
        assert_eq!(light["children"][0]["label"], "×");
        act(&mut app, "hot", json!(""));
        let tree = sync(&mut app, args(Value::Null, false))["tree"].clone();
        assert!(tree["children"][1]["children"][0]["children"].as_array().unwrap().is_empty());
    }
    #[test]
    fn a_chrome_plan_styles_the_header_and_hovers_one_button() {
        let chrome = json!({"header":{"height":36,"flexDirection":"row"},"controls":{"flexDirection":"row","gap":4},
            "button":{"width":24},"buttonHover":{"backgroundColor":"#333"},"glyph":{"fontSize":12},"title":{"left":40}});
        let mut app = TitleBar::default();
        let tree = sync(&mut app, args(chrome.clone(), true))["tree"].clone();
        assert_eq!(tree["style"]["height"], 36);
        assert_eq!(tree["children"][0]["style"]["left"], 40, "the plan overrides the title's style");
        let first = &tree["children"][1]["children"][0];
        assert_eq!(first["style"], json!({"width":24}));
        assert_eq!(first["children"][0]["style"]["fontSize"], 12, "with a plan the glyph is always there");
        act(&mut app, "hot", json!("close"));
        let tree = sync(&mut app, args(chrome, true))["tree"].clone();
        assert_eq!(tree["children"][1]["children"][0]["style"]["backgroundColor"], "#333");
        assert_eq!(tree["children"][1]["children"][1]["style"], json!({"width":24}), "only the hovered one");
    }
    #[test]
    fn the_effects_carry_the_window_and_the_pointer() {
        let mut app = TitleBar::default();
        sync(&mut app, args(Value::Null, true));
        assert_eq!(act(&mut app, "drag", Value::Null)["effects"][0], json!({"kind":"winDrag","id":"files","x":12.,"y":8.}));
        assert_eq!(act(&mut app, "max", Value::Null)["effects"][0], json!({"kind":"winMax","id":"files"}));
        assert_eq!(act(&mut app, "act", json!("min"))["effects"][0], json!({"kind":"winAct","id":"files","key":"min"}));
        assert_eq!(act(&mut app, "noop", Value::Null), json!({}));
    }
    #[test]
    fn a_light_stops_the_press_from_starting_a_drag() {
        let mut app = TitleBar::default();
        let tree = sync(&mut app, args(Value::Null, true))["tree"].clone();
        let light = &tree["children"][1]["children"][0];
        assert_eq!(light["events"]["onMouseDown"], json!(["noop"]));
        assert_eq!(tree["events"]["onMouseDown"], json!(["drag"]));
    }
}
