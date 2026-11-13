//! Files' widget tree as the tree of a native view (rust/axia-shell/src/native_view).
//!
//! The Files interface is authored in `ui.rs` with its own vocabulary (`type`, `text`, `tool`,
//! `fileTile`, `contextMenu`…) that `sdk/rustfiles.js` binds to React widgets one by one. A native
//! view takes `kind` trees and draws them straight into the scene; this is the translation, so the
//! React layer (QuickJS, one render per change of the window) is not needed to show Files:
//!
//! | Files                | native view                                                        |
//! | -------------------- | ------------------------------------------------------------------ |
//! | view, scroll, text   | view, scroll, text (`text` becomes `label`)                         |
//! | filename, fileTile   | text and view with a `tooltip`                                      |
//! | button, input, image | button, input (`change` is its action), image                       |
//! | icon                 | the vector icon's own views                                         |
//! | tool, toolGroup      | the toolbar buttons of sdk/controls.jsx: hover, active, tooltip     |
//! | toolbar              | the 44 px row with its hairline                                     |
//! | contextMenu          | a `menu` node (native_view/menu.rs)                                 |
//! | media, documentImage | a placeholder: these two still need the compositor's own nodes      |
//!
//! Events already come in the host's form (`events: {onClick: [action]}`), and captions are left for
//! the host to resolve, except dates, which it has no format for.
use serde_json::{json, Map, Value};

/// What the translation reads from outside the tree.
pub struct Env<'a> {
    /// The theme tokens of the window (control, hover, accent, text, textDim, separator, mode…).
    pub theme: &'a Value,
    /// The interface language (dates are written the way it does).
    pub lang: &'a str,
    /// The context menu's keyboard row and open submenu (-1: none), which the app keeps.
    pub menu: (i64, i64),
}

impl Env<'_> {
    fn tok(&self, key: &str, fallback: &str) -> Value {
        match &self.theme[key] {
            Value::Null => json!(fallback),
            v => v.clone(),
        }
    }
    fn dark(&self) -> bool {
        self.theme["mode"] != "light"
    }
}

/// A caption as the host reads it; a date, which it has no format for, as text.
fn caption(env: &Env, v: &Value) -> Value {
    if v["format"] == "date" {
        return Value::String(date(v["value"].as_i64().unwrap_or(0), env.lang));
    }
    v.clone()
}

/// A time as a short local date and time: month first with the clock in English, day first otherwise.
fn date(secs: i64, lang: &str) -> String {
    if secs <= 0 {
        return String::new();
    }
    let t = secs as libc::time_t;
    // SAFETY: localtime_r writes a plain struct from a time_t.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return String::new();
        }
        tm
    };
    let (y, mo, d, h, mi) = (tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min);
    if lang.starts_with("en") {
        format!("{mo}/{d}/{y} {}:{mi:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
    } else {
        format!("{d:02}/{mo:02}/{y} {h:02}:{mi:02}")
    }
}

fn copy(from: &Map<String, Value>, to: &mut Map<String, Value>, keys: &[&str]) {
    for k in keys {
        if let Some(v) = from.get(*k) {
            to.insert((*k).into(), v.clone());
        }
    }
}

/// `node` and everything under it, as a native-view tree.
pub fn convert(node: &Value, env: &Env) -> Value {
    let Some(obj) = node.as_object() else { return node.clone() };
    let kids = |env: &Env| -> Vec<Value> {
        obj.get("children").and_then(Value::as_array).map(|c| c.iter().map(|n| convert(n, env)).collect()).unwrap_or_default()
    };
    let mut out = Map::new();
    copy(obj, &mut out, &["key", "style", "events"]);
    match obj.get("type").and_then(Value::as_str).unwrap_or("view") {
        "icon" => {
            let size = obj.get("size").and_then(Value::as_f64).unwrap_or(16.);
            let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
            return convert(&super::ui::icon(name, size), env);
        }
        "text" => {
            out.insert("kind".into(), json!("text"));
            out.insert("label".into(), caption(env, &obj["text"]));
        }
        "filename" => {
            out.insert("kind".into(), json!("text"));
            let text = caption(env, &obj["text"]);
            out.insert("tooltip".into(), obj.get("tooltip").map_or_else(|| text.clone(), |t| caption(env, t)));
            out.insert("label".into(), text);
        }
        "fileTile" => {
            out.insert("kind".into(), json!("view"));
            if let Some(tip) = obj.get("tooltip") {
                out.insert("tooltip".into(), caption(env, tip));
            }
            out.insert("children".into(), Value::Array(kids(env)));
        }
        "button" => {
            out.insert("kind".into(), json!("button"));
            copy(obj, &mut out, &["action", "disabled", "primary", "small", "destructive", "label"]);
            out.insert("label".into(), caption(env, &obj["label"]));
        }
        "input" => {
            out.insert("kind".into(), json!("input"));
            copy(obj, &mut out, &["value", "placeholder", "submit", "cancel", "autoFocus", "selectAll", "select"]);
            if let Some(action) = obj.get("action") {
                out.insert("change".into(), action.clone());
            }
        }
        "image" => {
            out.insert("kind".into(), json!("image"));
            copy(obj, &mut out, &["src"]);
        }
        "scroll" => {
            out.insert("kind".into(), json!("scroll"));
            out.insert("children".into(), Value::Array(kids(env)));
        }
        "tool" => return tool(obj, out, env),
        "toolGroup" => {
            out.insert("kind".into(), json!("view"));
            let base = json!({"flexDirection":"row","alignItems":"center","gap":2,"padding":2,"flexShrink":0,
                "borderRadius":env.tok("controlRadius", "6").as_f64().unwrap_or(6.) + 1.,"backgroundColor":env.tok("control", "#3a3a3f")});
            out.insert("style".into(), json!([base, obj.get("style").cloned().unwrap_or(Value::Null)]));
            out.insert("children".into(), Value::Array(kids(env)));
        }
        "toolbar" => {
            out.insert("kind".into(), json!("view"));
            let base = json!({"height":44,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":12,"zIndex":10});
            out.insert("style".into(), json!([base, obj.get("style").cloned().unwrap_or(Value::Null)]));
            let mut children = vec![json!({"kind":"view","style":{"position":"absolute","left":0,"right":0,"bottom":0,"height":1,
                "backgroundColor":env.tok("separator", "rgba(128,128,128,0.3)")}})];
            children.extend(kids(env));
            out.insert("children".into(), Value::Array(children));
        }
        "contextMenu" => {
            let mut menu = json!({"kind":"menu","key":"context-menu","x":obj["x"],"y":obj["y"],"items":obj["items"],
                "active":env.menu.0,"sub":env.menu.1,
                "layer":obj.get("layer").cloned().unwrap_or(json!("popover")),"events":{"onDismiss":["closeContext"]}});
            if obj.get("absolute") == Some(&json!(true)) {
                menu["absolute"] = json!(true);
            }
            return menu;
        }
        "media" | "documentImage" => {
            // The video surface and the document image are nodes the compositor owns (the React
            // layer creates them); until the host can, the space they take is kept.
            out.insert("kind".into(), json!("view"));
            out.insert("children".into(), json!([{"kind":"text","label":"…","style":{"color":env.tok("textDim", "#999999")}}]));
            if let Some(Value::Object(style)) = out.get_mut("style") {
                style.insert("alignItems".into(), json!("center"));
                style.insert("justifyContent".into(), json!("center"));
            }
        }
        _ => {
            out.insert("kind".into(), json!("view"));
            out.insert("children".into(), Value::Array(kids(env)));
        }
    }
    Value::Object(out)
}

/// A toolbar button (`ToolbarButton`, sdk/controls.jsx, small): a glyph, the accent when active, dimmed
/// when disabled, lit under the pointer, its title as the tooltip and its action on click.
fn tool(obj: &Map<String, Value>, mut out: Map<String, Value>, env: &Env) -> Value {
    let disabled = obj.get("disabled") == Some(&json!(true));
    let active = obj.get("active") == Some(&json!(true));
    let color = if disabled { env.tok("textDim", "#999999") } else if active { env.tok("accent", "#0a84ff") } else { env.tok("text", "#ffffff") };
    let lit = if env.dark() { "rgba(255,255,255,0.12)" } else { "rgba(0,0,0,0.08)" };
    let base = json!({"height":22,"minWidth":22,"borderRadius":env.tok("controlRadiusSmall", "6"),"flexDirection":"row","gap":6,
        "alignItems":"center","justifyContent":"center","flexShrink":0,"opacity":if disabled { 0.45 } else { 1. },
        "backgroundColor":if active { json!(lit) } else { json!("transparent") }});
    out.insert("kind".into(), json!("view"));
    out.insert("style".into(), json!([base, obj.get("style").cloned().unwrap_or(Value::Null)]));
    if let Some(title) = obj.get("title") {
        out.insert("tooltip".into(), caption(env, title));
    }
    if !disabled {
        if let Some(action) = obj.get("action") {
            out.insert("events".into(), json!({"onClick":action}));
        }
        if !active {
            out.insert("hoverStyle".into(), json!({"backgroundColor":env.tok("hover", "rgba(255,255,255,0.08)")}));
        }
    }
    let glyph = match &obj["icon"] {
        Value::String(s) => Value::String(s.clone()),
        Value::Number(n) => Value::String(n.to_string()),
        _ => Value::Null,
    };
    out.insert("children".into(), json!([{"kind":"text","label":glyph,"style":{"fontSize":13,"color":color,"textAlign":"center"}}]));
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::tests::Home;
    use serde_json::json;

    fn env(theme: &Value) -> Env<'_> {
        Env { theme, lang: "en", menu: (-1, -1) }
    }
    fn walk(v: &Value, f: &mut dyn FnMut(&Value)) {
        f(v);
        for c in v["children"].as_array().into_iter().flatten() {
            walk(c, f);
        }
    }

    #[test]
    fn the_real_interface_becomes_a_tree_of_native_kinds_only() {
        let home = Home::new();
        home.file("report.txt", "x");
        std::fs::create_dir_all(home.path("Folder")).unwrap();
        let (session, shared) = home.session(json!({}));
        let theme = json!({"mode":"dark","accent":"#0a84ff","text":"#fff","control":"#333","nativeStyle":"adwaita"});
        let tree = convert(&session.render(&shared), &env(&theme));
        let (mut kinds, mut tooltips, mut clicks, mut stray) = (std::collections::BTreeSet::new(), 0, 0, Vec::new());
        walk(&tree, &mut |n| {
            match n["kind"].as_str() {
                Some(k) => {
                    kinds.insert(k.to_owned());
                }
                None => stray.push(n.to_string()),
            }
            tooltips += n["tooltip"].is_null() as usize ^ 1;
            clicks += n["events"]["onClick"].is_array() as usize;
            assert!(n["type"].is_null(), "no node keeps Files' own `type`: {n}");
        });
        assert!(stray.is_empty(), "{stray:?}");
        let known = ["view", "text", "image", "button", "input", "scroll", "menu"];
        assert!(kinds.iter().all(|k| known.contains(&k.as_str())), "{kinds:?}");
        assert!(kinds.contains("input") && kinds.contains("scroll"), "{kinds:?}");
        assert!(clicks > 3, "the sidebar, the toolbar and the entries keep their actions: {clicks}");
        assert!(tooltips > 0, "toolbar buttons and file names have tooltips");
    }

    #[test]
    fn a_tool_is_a_toolbar_button_and_a_context_menu_a_menu_node() {
        let theme = json!({"mode":"light","accent":"#0a84ff","text":"#111","hover":"#eee"});
        let e = env(&theme);
        let on = convert(&json!({"type":"tool","icon":"‹","title":{"key":"files.back"},"action":["back"]}), &e);
        assert_eq!(on["events"]["onClick"], json!(["back"]));
        assert_eq!(on["hoverStyle"]["backgroundColor"], "#eee");
        assert_eq!(on["tooltip"], json!({"key":"files.back"}));
        assert_eq!(on["children"][0]["label"], "‹");
        let off = convert(&json!({"type":"tool","icon":"›","action":["forward"],"disabled":true}), &e);
        assert!(off["events"].is_null() && off["hoverStyle"].is_null(), "a disabled tool does nothing");
        assert_eq!(off["style"][0]["opacity"], 0.45);
        let active = convert(&json!({"type":"tool","icon":"▦","action":["view","icons"],"active":true}), &e);
        assert!(active["hoverStyle"].is_null() && active["style"][0]["backgroundColor"] == "rgba(0,0,0,0.08)");
        let menu = convert(&json!({"type":"contextMenu","x":10,"y":20,"items":[{"label":"Open","action":["open"]}],"layer":"popover"}), &e);
        assert_eq!((menu["kind"].clone(), menu["x"].clone(), menu["layer"].clone()), (json!("menu"), json!(10), json!("popover")));
        assert_eq!(menu["events"]["onDismiss"], json!(["closeContext"]));
        let input = convert(&json!({"type":"input","value":"a","action":["dialogText"],"submit":["submit"],"autoFocus":true}), &e);
        assert_eq!((input["change"].clone(), input["submit"].clone(), input["autoFocus"].clone()), (json!(["dialogText"]), json!(["submit"]), json!(true)));
    }

    #[test]
    fn dates_are_written_for_the_language() {
        let theme = json!({});
        let en = convert(&json!({"type":"text","text":{"format":"date","value":1_700_000_000}}), &Env { theme: &theme, lang: "en", menu: (-1, -1) });
        let pt = convert(&json!({"type":"text","text":{"format":"date","value":1_700_000_000}}), &Env { theme: &theme, lang: "pt", menu: (-1, -1) });
        let (en, pt) = (en["label"].as_str().unwrap().to_owned(), pt["label"].as_str().unwrap().to_owned());
        assert!(en.ends_with("AM") || en.ends_with("PM"), "{en}");
        assert!(pt.contains('/') && pt.contains(':') && !pt.contains("AM"), "{pt}");
        // a caption that isn't a date stays for the host
        assert_eq!(convert(&json!({"type":"text","text":{"key":"files.name"}}), &env(&theme))["label"], json!({"key":"files.name"}));
    }
}
