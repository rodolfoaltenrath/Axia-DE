//! Menus and context menus as native-view trees: the `"kind": "menu"` node of host.rs, drawn like
//! `sdk/menu.jsx` (which stays for the apps that are still React).
//!
//! The app describes only what it knows:
//! `{"kind":"menu","x","y","items":[…],"active":n,"sub":n,"layer":"popover","absolute":false,
//!   "width":220,"above":false,"centered":false,"side":"left"|"right","events":{"onDismiss":[…]}}`
//! where an item is `"-"`, `{"header": caption}`, or `{"label": caption, "action": […], "hint",
//! "checked", "disabled", "danger", "submenu": […]}`. `x` and `y` are the window's (unless `absolute`).
//! The host has the rest: the resolved captions, the measured text, the screen and the theme, and
//! this turns it into layered views: the panel in the shell layer, clamped inside the screen, scrolling
//! when taller than it, rows that light up under the pointer by themselves (`hoverStyle`), and the open
//! submenu beside its row. A row's `onMouseEnter` is `["menuHover", index]`, for the app to track the
//! keyboard's row and the submenu; the keys themselves (arrows, Enter) are the app's.
use serde_json::{json, Map, Value};

const ROW: f64 = 24.;
const SEP_H: f64 = 9.;
const HEAD_H: f64 = 22.;

/// What the host knows that the app doesn't.
pub struct Env<'a> {
    /// A caption (a string or a `{"key", "params"}` object) as text.
    pub caption: &'a dyn Fn(&Value) -> String,
    /// A token of the theme.
    pub theme: &'a dyn Fn(&str) -> Option<Value>,
    /// The width of a text at a size and weight.
    pub measure: &'a dyn Fn(&str, f64, f64) -> f64,
    /// The screen.
    pub screen: (f64, f64),
    /// The window's origin on the screen (the menu's `x`, `y` are relative to it).
    pub origin: (f64, f64),
    pub rtl: bool,
    pub dark: bool,
}

impl Env<'_> {
    fn tok(&self, key: &str, fallback: &str) -> Value {
        (self.theme)(key).unwrap_or_else(|| json!(fallback))
    }
    fn first(&self, keys: &[&str], fallback: &str) -> Value {
        keys.iter().find_map(|k| (self.theme)(k).filter(|v| !v.is_null())).unwrap_or_else(|| json!(fallback))
    }
}

fn is_sep(item: &Value) -> bool {
    item == "-"
}
fn row_h(item: &Value) -> f64 {
    if is_sep(item) {
        SEP_H
    } else if item["header"].is_string() || item["header"].is_object() {
        HEAD_H
    } else {
        ROW
    }
}
fn selectable(item: &Value) -> bool {
    item.is_object() && item["header"].is_null() && item["disabled"] != true
}

/// The width a menu needs for its longest row (translations change each one's length).
fn fit_width(items: &[Value], min: f64, env: &Env) -> f64 {
    let mut w = min;
    for item in items {
        if is_sep(item) || item["label"].is_null() {
            continue;
        }
        let label = (env.measure)(&(env.caption)(&item["label"]), 13., 400.);
        let hint = if item["hint"].is_null() { 0. } else { 18. + (env.measure)(&(env.caption)(&item["hint"]), 12., 400.) };
        let need = 10. + 16. + label + hint + 10. + 10. + if item["submenu"].is_array() { 14. } else { 0. };
        w = w.max(need);
    }
    w.ceil().min(env.screen.0 - 8.)
}

struct Placed {
    x: f64,
    y: f64,
    width: f64,
}

fn row(i: usize, item: &Value, active: bool, env: &Env) -> Value {
    if is_sep(item) {
        return json!({"kind":"view","key":format!("item{i}"),"style":{"height":1,"marginVertical":4,"marginHorizontal":8,
            "backgroundColor":env.tok("separator","rgba(128,128,128,0.3)")}});
    }
    let text = env.tok("text", if env.dark { "#ffffff" } else { "#1d1d1f" });
    let dim = env.tok("textDim", "#8e8e93");
    if !item["header"].is_null() {
        return json!({"kind":"text","key":format!("item{i}"),"label":item["header"],
            "style":{"height":HEAD_H,"paddingHorizontal":15,"paddingTop":5,"fontSize":11,"fontWeight":600,"color":dim}});
    }
    let off = item["disabled"] == true;
    let accent = env.tok("accent", "#0a84ff");
    let lit = active && !off;
    let white = json!("#ffffff");
    let colored = |base: Value, hover: bool| -> Value {
        let mut t = Map::new();
        t.insert("color".into(), if lit { white.clone() } else { base });
        if hover {
            t.insert("hoverStyle".into(), json!({"color":"#ffffff"}));
        }
        Value::Object(t)
    };
    let label_color = if off { dim.clone() } else if item["danger"] == true { json!("#ff453a") } else { text.clone() };
    let mut children = vec![
        json!({"kind":"text","label":if item["checked"] == true { "✓" } else { "" },"style":{"width":16,"fontSize":12,"color":if lit { white.clone() } else { text.clone() }},
            "hoverStyle":if off { Value::Null } else { json!({"color":"#ffffff"}) }}),
    ];
    let mut label = json!({"kind":"text","label":item["label"],"style":{"flex":1,"fontSize":13,"whiteSpace":"nowrap"}});
    merge(&mut label["style"], colored(label_color, !off));
    if !off {
        label["hoverStyle"] = json!({"color":"#ffffff"});
    }
    children.push(label);
    for (key, extra) in [("hint", item["hint"].clone()), ("submenu", if item["submenu"].is_array() { json!("›") } else { Value::Null })] {
        if extra.is_null() {
            continue;
        }
        let label = if key == "hint" { extra } else { json!("›") };
        let mut t = json!({"kind":"text","label":label,"style":{"fontSize":12}});
        merge(&mut t["style"], colored(dim.clone(), !off));
        if !off {
            t["hoverStyle"] = json!({"color":"#ffffff"});
        }
        children.push(t);
    }
    let mut events = Map::new();
    if selectable(item) && !item["submenu"].is_array() && item["action"].is_array() {
        events.insert("onClick".into(), item["action"].clone());
    }
    events.insert("onMouseEnter".into(), json!(["menuHover", i]));
    let mut view = json!({"kind":"view","key":format!("item{i}"),
        "style":{"height":ROW,"paddingHorizontal":10,"marginHorizontal":5,"flexDirection":"row","alignItems":"center",
            "borderRadius":env.tok("contextMenuItemRadius", "5"),"backgroundColor":if lit { accent.clone() } else { json!("transparent") }},
        "events":events,"children":children});
    if selectable(item) {
        view["hoverStyle"] = json!({"backgroundColor":accent});
    }
    view
}

fn merge(into: &mut Value, from: Value) {
    if let (Value::Object(a), Value::Object(b)) = (into, from) {
        a.extend(b);
    }
}

struct Opts<'a> {
    layer: &'a str,
    min: f64,
    above: bool,
    centered: bool,
    side: Option<&'a str>,
}

/// One panel (a menu or a submenu) with its rows; `at` is the screen point it opens at.
fn panel(items: &[Value], at: (f64, f64), active: i64, opts: &Opts, dismiss: Option<Value>, env: &Env) -> (Value, Placed) {
    let width = fit_width(items, opts.min, env);
    let (mut x, mut y) = at;
    // In Arabic and Hebrew x is the right edge: the menu opens to the left.
    if opts.side.map_or(env.rtl, |s| s == "left") {
        x -= width;
    }
    let full: f64 = items.iter().map(row_h).sum::<f64>() + 10.;
    let height = full.min(env.screen.1 - 8.);
    if opts.above {
        y -= height;
    } else if opts.centered {
        y -= height / 2.;
    }
    x = x.min(env.screen.0 - width - 4.).max(4.);
    y = y.min(env.screen.1 - height - 4.).max(4.);
    let scrolls = full > height;
    let rows: Vec<Value> = items.iter().enumerate().map(|(i, it)| row(i, it, i as i64 == active, env)).collect();
    let mut style = json!({"position":"absolute","left":x,"top":y,"width":width,
        "borderRadius":env.tok("contextMenuRadius", "9"),
        "backgroundColor":env.first(&["panelBg", "popupBg"], if env.dark { "rgba(35,35,40,0.88)" } else { "rgba(248,248,250,0.90)" }),
        "backdropFilter":format!("blur({}px)", if (env.theme)("effects") == Some(json!(false)) { 0. } else { (env.theme)("blur").and_then(|b| b.as_f64()).unwrap_or(24.) }),
        "border":format!("1px solid {}", env.first(&["windowBorder", "separator"], "rgba(128,128,128,0.3)").as_str().unwrap_or("rgba(128,128,128,0.3)")),
        "boxShadow":"0 12px 32px rgba(0,0,0,0.35)","overflow":"hidden"});
    let body = if scrolls {
        style["height"] = json!(height);
        vec![json!({"kind":"scroll","style":{"height":height},"children":[{"kind":"view","style":{"paddingVertical":5},"children":rows}]})]
    } else {
        style["paddingVertical"] = json!(5);
        rows
    };
    let mut node = json!({"kind":"view","layer":opts.layer,"style":style,"children":body});
    if let Some(d) = dismiss {
        node["events"] = json!({"onDismiss":d});
    }
    (node, Placed { x, y, width })
}

/// A `"kind": "menu"` spec as the views that draw it (see the module's header).
pub fn expand(spec: &Value, env: &Env) -> Value {
    let items = spec["items"].as_array().cloned().unwrap_or_default();
    let opts = Opts {
        layer: spec["layer"].as_str().unwrap_or("popover"),
        min: spec["width"].as_f64().unwrap_or(220.),
        above: spec["above"] == true,
        centered: spec["centered"] == true,
        side: spec["side"].as_str(),
    };
    let (ox, oy) = if spec["absolute"] == true { (0., 0.) } else { env.origin };
    let at = (spec["x"].as_f64().unwrap_or(0.) + ox, spec["y"].as_f64().unwrap_or(0.) + oy);
    let dismiss = spec["events"]["onDismiss"].is_array().then(|| spec["events"]["onDismiss"].clone());
    let (mut main, placed) = panel(&items, at, spec["active"].as_i64().unwrap_or(-1), &opts, dismiss, env);
    if let Some(i) = spec["sub"].as_i64().filter(|i| *i >= 0) {
        if let Some(sub) = items.get(i as usize).and_then(|it| it["submenu"].as_array()) {
            let top: f64 = items.iter().take(i as usize).map(row_h).sum::<f64>() + 5.;
            // beside its row, in the same layer (no side of its own: the language decides)
            let sub_opts = Opts { side: None, above: false, centered: false, ..opts };
            let (mut node, _) = panel(sub, (placed.x + placed.width - 4., placed.y + top - 5.), -1, &sub_opts, None, env);
            node["key"] = json!("sub");
            main["children"].as_array_mut().map(|c| c.push(node));
        }
    }
    main["key"] = spec.get("key").cloned().unwrap_or_else(|| json!("menu"));
    // An empty, zero-size anchor in the window; the panels hang from the shell layer.
    json!({"kind":"view","key":main["key"],"style":{"position":"absolute","left":0,"top":0,"width":0,"height":0},"children":[main]})
}

/// The width of `text` as the compositor will lay it out.
#[cfg(not(test))]
pub fn measure(text: &str, size: f64, weight: f64) -> f64 {
    extern "C" {
        fn text_measure(s: *const std::ffi::c_char, weight: f32, size: f32, line_h: f32, max_w: f32, nowrap: i32, w: *mut f32, h: *mut f32);
    }
    let Ok(c) = std::ffi::CString::new(text.replace('\0', "")) else { return 0. };
    let (mut w, mut h) = (0f32, 0f32);
    unsafe { text_measure(c.as_ptr(), weight as f32, size as f32, 0., 0., 1, &mut w, &mut h) };
    f64::from(w)
}
#[cfg(test)]
pub fn measure(text: &str, size: f64, _weight: f64) -> f64 {
    text.chars().count() as f64 * size * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(caption: &'a dyn Fn(&Value) -> String, theme: &'a dyn Fn(&str) -> Option<Value>, measure: &'a dyn Fn(&str, f64, f64) -> f64) -> Env<'a> {
        Env { caption, theme, measure, screen: (1000., 700.), origin: (100., 50.), rtl: false, dark: true }
    }
    fn items() -> Value {
        json!([{"label":"Open","action":["open"],"hint":"⌘O"},"-",{"header":"Share"},
            {"label":"Disabled","action":["x"],"disabled":true},
            {"label":"Open with","submenu":[{"label":"Editor","action":["with","editor"]}]},
            {"label":"Delete","action":["delete"],"danger":true,"checked":true}])
    }
    fn menu(extra: Value) -> Value {
        let mut m = json!({"kind":"menu","x":10,"y":20,"items":items()});
        merge(&mut m, extra);
        m
    }
    fn panel_of(tree: &Value) -> &Value {
        &tree["children"][0]
    }

    #[test]
    fn a_menu_opens_at_its_window_point_with_rows_that_light_up() {
        let caption = |v: &Value| v.as_str().unwrap_or("").to_owned();
        let theme = |_: &str| None;
        let measure = |t: &str, s: f64, w: f64| measure(t, s, w);
        let tree = expand(&menu(json!({})), &env(&caption, &theme, &measure));
        let p = panel_of(&tree);
        assert_eq!(p["layer"], "popover");
        assert_eq!((p["style"]["left"].clone(), p["style"]["top"].clone()), (json!(110.), json!(70.)), "window point + origin");
        let rows = p["children"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0]["events"]["onClick"], json!(["open"]));
        assert_eq!(rows[0]["events"]["onMouseEnter"], json!(["menuHover", 0]));
        assert!(rows[0]["hoverStyle"]["backgroundColor"].is_string(), "an enabled row lights up");
        assert!(rows[3]["hoverStyle"].is_null() && rows[3]["events"]["onClick"].is_null(), "a disabled row doesn't, nor act");
        assert!(rows[4]["events"]["onClick"].is_null(), "a submenu row opens it instead");
        assert_eq!(rows[1]["style"]["height"], 1, "separator");
        assert_eq!(rows[2]["style"]["height"], HEAD_H, "header");
        assert_eq!(rows[5]["children"][1]["style"]["color"], "#ff453a", "danger");
        assert_eq!(rows[5]["children"][0]["label"], "✓");
        // the label's text turns white with its row
        assert_eq!(rows[0]["children"][1]["hoverStyle"]["color"], "#ffffff");
    }

    #[test]
    fn it_fits_the_longest_row_and_stays_inside_the_screen() {
        let caption = |v: &Value| v.as_str().unwrap_or("").to_owned();
        let theme = |_: &str| None;
        let measure = |t: &str, s: f64, w: f64| measure(t, s, w);
        let e = env(&caption, &theme, &measure);
        let long = json!({"kind":"menu","x":0,"y":0,"items":[{"label":"A very very very very very very long label that needs room","hint":"Ctrl+Shift+Alt+L"}]});
        let w = expand(&long, &e)["children"][0]["style"]["width"].as_f64().unwrap();
        assert!(w > 220. && w <= 992., "{w}");
        // pushed back inside at the right and bottom edges
        let tree = expand(&menu(json!({"x":2000,"y":2000,"absolute":true})), &e);
        let s = &panel_of(&tree)["style"];
        assert!(s["left"].as_f64().unwrap() + s["width"].as_f64().unwrap() <= 996.);
        assert!(s["top"].as_f64().unwrap() < 700. - 100.);
        // above: y is the bottom edge
        let up = expand(&menu(json!({"x":100,"y":300,"absolute":true,"above":true})), &e);
        assert!(panel_of(&up)["style"]["top"].as_f64().unwrap() < 300. - 100.);
        // a menu taller than the screen scrolls
        let many: Vec<Value> = (0..40).map(|i| json!({"label":format!("Row {i}"),"action":["r",i]})).collect();
        let tall = expand(&json!({"kind":"menu","x":0,"y":0,"items":many}), &e);
        let p = panel_of(&tall);
        assert_eq!(p["children"][0]["kind"], "scroll");
        assert_eq!(p["style"]["height"], 692.);
    }

    #[test]
    fn the_open_submenu_is_beside_its_row_and_only_the_menu_is_dismissed() {
        let caption = |v: &Value| v.as_str().unwrap_or("").to_owned();
        let theme = |_: &str| None;
        let measure = |t: &str, s: f64, w: f64| measure(t, s, w);
        let e = env(&caption, &theme, &measure);
        let tree = expand(&menu(json!({"sub":4,"active":4,"events":{"onDismiss":["closeContext"]}})), &e);
        let main = panel_of(&tree);
        assert_eq!(main["events"]["onDismiss"], json!(["closeContext"]));
        let kids = main["children"].as_array().unwrap();
        let sub = kids.last().unwrap();
        assert_eq!(sub["key"], "sub");
        assert!(sub["events"].is_null(), "a press in the submenu doesn't dismiss: it is part of the menu");
        let (mx, mw) = (main["style"]["left"].as_f64().unwrap(), main["style"]["width"].as_f64().unwrap());
        assert_eq!(sub["style"]["left"].as_f64().unwrap(), mx + mw - 4.);
        // 5 + rows above it (24 + 9 + 22 + 24 = 79) - 5, from the menu's top
        assert_eq!(sub["style"]["top"].as_f64().unwrap(), main["style"]["top"].as_f64().unwrap() + 79.);
        assert_eq!(sub["children"][0]["events"]["onClick"], json!(["with", "editor"]));
        assert_eq!(kids[4]["style"]["backgroundColor"], "#0a84ff", "the keyboard's row is lit");
    }

    #[test]
    fn right_to_left_opens_to_the_left_of_the_point() {
        let caption = |v: &Value| v.as_str().unwrap_or("").to_owned();
        let theme = |_: &str| None;
        let measure = |t: &str, s: f64, w: f64| measure(t, s, w);
        let mut e = env(&caption, &theme, &measure);
        e.rtl = true;
        let tree = expand(&menu(json!({"x":500,"y":100,"absolute":true})), &e);
        let s = &panel_of(&tree)["style"];
        assert_eq!(s["left"].as_f64().unwrap() + s["width"].as_f64().unwrap(), 500.);
    }
}
