//! The controls of Passwords as view trees: the SDK's Button, Segmented, ToolbarButton,
//! SearchField and EmptyState (sdk/components.jsx, sdk/controls.jsx) and the app's own small
//! pieces (apps/passwords/ui.jsx), with the same sizes, colors and spacing.
use super::glyphs::{glyph, tile, tint};
use serde_json::{json, Value};

pub const RED: &str = "#ff453a";
pub const ORANGE: &str = "#ff9f0a";
pub const GREEN: &str = "#30d158";
pub const YELLOW: &str = "#ffd60a";

/// A translated caption.
pub fn k(key: &str) -> Value {
    json!({"key":format!("passwords.{key}")})
}
/// A translated caption with parameters (which may be captions themselves).
pub fn kp(key: &str, params: Value) -> Value {
    json!({"key":format!("passwords.{key}"),"params":params})
}

pub fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(a), Some(b)) = (a.as_object_mut(), b.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
    a
}
pub fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
pub fn keyed(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
pub fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
pub fn on(mut node: Value, event: &str, action: Value) -> Value {
    node["events"][event] = action;
    node
}
pub fn click(node: Value, action: Value) -> Value {
    on(node, "onClick", action)
}
pub fn hover(mut node: Value, style: Value) -> Value {
    node["hoverStyle"] = style;
    node
}
pub fn tip(mut node: Value, caption: Value) -> Value {
    if !caption.is_null() {
        node["tooltip"] = caption;
    }
    node
}

/// The theme the window gets (context.theme), with the SDK's defaults.
pub struct Theme<'a>(pub &'a Value);
impl Theme<'_> {
    pub fn get(&self, name: &str, fallback: &str) -> Value {
        let v = &self.0[name];
        if v.is_null() {
            json!(fallback)
        } else {
            v.clone()
        }
    }
    pub fn dark(&self) -> bool {
        self.0["mode"] != "light"
    }
    pub fn text(&self) -> Value {
        self.get("text", "#ffffff")
    }
    pub fn dim(&self) -> Value {
        self.get("textDim", "#98989f")
    }
    pub fn accent(&self) -> Value {
        self.get("accent", "#0a84ff")
    }
    pub fn sep(&self) -> Value {
        self.get("separator", "rgba(255,255,255,0.1)")
    }
    pub fn control(&self) -> Value {
        self.get("control", "#3a3a3f")
    }
    pub fn hover(&self) -> Value {
        self.get("hover", "rgba(255,255,255,0.06)")
    }
    pub fn bg(&self) -> Value {
        self.get("windowBg", "#1e1e22")
    }
    pub fn input_bg(&self) -> Value {
        self.get("inputBg", "#1e1e22")
    }
    pub fn radius(&self, small: bool) -> f64 {
        let n = &self.0[if small { "controlRadiusSmall" } else { "controlRadius" }];
        n.as_f64().or_else(|| n.as_str().and_then(|s| s.parse().ok())).unwrap_or(if small { 5. } else { 6. })
    }
    pub fn border(&self) -> bool {
        self.0["buttonBorder"] == true
    }
    pub fn s(&self, v: Value) -> String {
        v.as_str().unwrap_or("").to_string()
    }
}

#[derive(Default, Clone, Copy)]
pub struct Btn {
    pub primary: bool,
    pub small: bool,
    pub plain: bool,
    pub disabled: bool,
    pub destructive: bool,
    /// A glyph before the label.
    pub icon: Option<&'static str>,
}
/// sdk/components.jsx Button.
pub fn button(t: &Theme, label: Value, action: Value, o: Btn) -> Value {
    let fg = if o.primary {
        json!("#fff")
    } else if o.destructive {
        json!(RED)
    } else {
        t.text()
    };
    let bg = if o.primary {
        t.accent()
    } else if o.plain {
        json!("transparent")
    } else {
        t.control()
    };
    let mut style = json!({"height":if o.small {22} else {28},"paddingHorizontal":if o.small {10} else {14},
        "borderRadius":t.radius(o.small),"flexDirection":"row","gap":6,"justifyContent":"center","alignItems":"center",
        "backgroundColor":bg,"opacity":if o.disabled {0.4} else {1.}});
    if t.border() && !o.plain {
        style["border"] = json!(format!("1px solid {}", t.s(if o.primary { t.accent() } else { t.sep() })));
    }
    let mut kids = vec![];
    if let Some(icon) = o.icon {
        kids.push(text(json!(icon), json!({"fontSize":if o.small {12} else {14},"color":fg})));
    }
    kids.push(text(label, json!({"fontSize":if o.small {12} else {13},"fontWeight":500,"color":fg,"whiteSpace":"nowrap"})));
    let mut node = view(style, kids);
    if !o.disabled {
        node = click(node, action);
        node = hover(node, if o.plain { json!({"backgroundColor":t.hover()}) } else { json!({"opacity":0.85}) });
    }
    node
}

/// sdk/components.jsx Segmented: `cmd` gets the picked value.
pub fn segmented(t: &Theme, cmd: &str, value: &str, options: &[(&str, Value)], neutral: bool, fill: bool, small: bool) -> Value {
    let raised = if t.dark() { "rgba(255,255,255,0.18)" } else { "#ffffff" };
    let r = t.radius(small);
    let kids = options
        .iter()
        .map(|(v, label)| {
            let on = *v == value;
            let fw = if on && neutral { 600 } else if neutral { 500 } else { 600 };
            click(
                keyed(v, json!({"paddingHorizontal":if small {10} else {14},"height":if small {18} else {24},"borderRadius":(r - 1.).max(2.),
                    "justifyContent":"center","alignItems":"center","flexGrow":if fill {1} else {0},
                    "backgroundColor":if on { if neutral { json!(raised) } else { t.accent() } } else { json!("transparent") },
                    "boxShadow":if on && neutral {"0 1px 2px rgba(0,0,0,0.18)"} else {"none"}}),
                    vec![text(label.clone(), json!({"fontSize":if small {11} else {12},"fontWeight":fw,"whiteSpace":"nowrap",
                        "color":if on && !neutral { json!("#fff") } else { t.text() }}))]),
                json!([cmd, v]),
            )
        })
        .collect();
    view(json!({"flexDirection":"row","backgroundColor":t.control(),"borderRadius":if small { r } else { r + 1. },"padding":2}), kids)
}

pub struct Tool {
    pub icon: Value,
    pub icon_size: f64,
    pub label: Value,
    pub tip: Value,
    pub small: bool,
    pub color: Option<Value>,
    pub active: bool,
    pub disabled: bool,
}
impl Default for Tool {
    fn default() -> Self {
        Self { icon: Value::Null, icon_size: 0., label: Value::Null, tip: Value::Null, small: false, color: None, active: false, disabled: false }
    }
}
/// sdk/controls.jsx ToolbarButton: `icon` is a string glyph or a node.
pub fn tool(t: &Theme, o: Tool, action: Value) -> Value {
    let h = if o.small { 22 } else { 28 };
    let fg = if o.disabled {
        t.dim()
    } else if o.active {
        t.accent()
    } else {
        o.color.clone().unwrap_or_else(|| t.text())
    };
    let mut kids = vec![];
    match &o.icon {
        Value::String(s) => {
            let size = if o.icon_size > 0. { o.icon_size } else if o.small { 13. } else { 15. };
            kids.push(text(json!(s), json!({"fontSize":size,"color":fg,"textAlign":"center"})));
        }
        Value::Null => {}
        node => kids.push(node.clone()),
    }
    if !o.label.is_null() {
        kids.push(text(o.label.clone(), json!({"fontSize":if o.small {12} else {13},"fontWeight":500,"color":fg,"whiteSpace":"nowrap"})));
    }
    let active_bg = if t.dark() { "rgba(255,255,255,0.12)" } else { "rgba(0,0,0,0.08)" };
    let mut node = view(
        json!({"height":h,"minWidth":h,"paddingHorizontal":if o.label.is_null() {0} else {8},"borderRadius":t.radius(true),"flexDirection":"row","gap":6,
            "alignItems":"center","justifyContent":"center","opacity":if o.disabled {0.45} else {1.},
            "backgroundColor":if o.active { json!(active_bg) } else { json!("transparent") }}),
        kids,
    );
    node = tip(node, o.tip);
    if !o.disabled {
        node = click(node, action);
        if !o.active {
            node = hover(node, json!({"backgroundColor":t.hover()}));
        }
    }
    node
}

/// A text field (the host's): `id` gets its value as it changes.
pub fn input(id: &str, value: &str, placeholder: Value, secure: bool, autofocus: bool, submit: Option<&str>, style: Value) -> Value {
    let mut n = json!({"kind":"input","key":id,"value":value,"placeholder":placeholder,"secure":secure,"change":[id],"style":style});
    if autofocus {
        n["autoFocus"] = json!(true);
    }
    if let Some(s) = submit {
        n["submit"] = json!([s]);
    }
    n
}

/// sdk/controls.jsx SearchField.
pub fn search(t: &Theme, id: &str, value: &str, placeholder: Value, rtl: bool) -> Value {
    let (lead, trail) = if rtl { ("right", "left") } else { ("left", "right") };
    let mut field_style = json!({"flex":1});
    field_style[if rtl { "paddingRight" } else { "paddingLeft" }] = json!(26);
    field_style[if rtl { "paddingLeft" } else { "paddingRight" }] = json!(if value.is_empty() { 10 } else { 26 });
    let mut field = input(id, value, placeholder, false, false, None, field_style);
    field["cancel"] = json!([format!("{id}Clear")]);
    let mut mag = json!({"position":"absolute","top":5,"fontSize":13,"color":t.dim(),"pointerEvents":"none"});
    mag[lead] = json!(9);
    let mut kids = vec![field, text(json!("⌕"), mag)];
    if !value.is_empty() {
        let mut s = json!({"position":"absolute","top":6,"width":16,"height":16,"borderRadius":8,"alignItems":"center","justifyContent":"center","backgroundColor":t.dim()});
        s[trail] = json!(6);
        kids.push(click(
            view(s, vec![text(json!("✕"), json!({"fontSize":10,"fontWeight":700,"color":if t.dark() {"#1f1f23"} else {"#fff"}}))]),
            json!([format!("{id}Clear")]),
        ));
    }
    view(json!({"flexDirection":"row","alignItems":"center"}), kids)
}

/// sdk/controls.jsx EmptyState.
pub fn empty(t: &Theme, icon: Value, title: Value, hint: Value, style: Value) -> Value {
    let mut kids = vec![icon];
    if !title.is_null() {
        kids.push(text(title, json!({"fontSize":15,"fontWeight":600,"color":t.text(),"textAlign":"center"})));
    }
    if !hint.is_null() {
        kids.push(text(hint, json!({"fontSize":12,"color":t.dim(),"textAlign":"center","maxWidth":360,"lineHeight":1.4})));
    }
    view(merge(json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":8,"padding":32}), style), kids)
}

/// sdk/components.jsx SectionTitle.
pub fn section(t: &Theme, label: Value) -> Value {
    text(label, json!({"fontSize":11,"fontWeight":600,"color":t.dim(),"marginLeft":10,"marginTop":10,"marginBottom":4}))
}

/// Caption over a field (11 px, 600, dim).
pub fn label(t: &Theme, caption: Value) -> Value {
    text(caption, json!({"fontSize":11,"fontWeight":600,"color":t.dim()}))
}
/// One hint line (12 px, dim).
pub fn hint(t: &Theme, caption: Value, color: Option<Value>) -> Value {
    text(caption, json!({"fontSize":12,"color":color.unwrap_or_else(|| t.dim()),"lineHeight":17}))
}
/// A calm warning box: a monochrome sign and the text, on a soft tint.
pub fn warning(t: &Theme, caption: Value, orange: bool) -> Value {
    let c = if orange { ORANGE } else { RED };
    view(
        json!({"flexDirection":"row","alignItems":"flex-start","gap":10,"paddingHorizontal":12,"paddingVertical":10,"borderRadius":10,
            "backgroundColor":if orange {"rgba(255,159,10,0.12)"} else {"rgba(255,69,58,0.10)"},
            "border":format!("1px solid {}", if orange {"rgba(255,159,10,0.35)"} else {"rgba(255,69,58,0.32)"})}),
        vec![
            text(json!("⚠"), json!({"fontSize":13,"lineHeight":17,"fontWeight":600,"color":c})),
            text(caption, json!({"flex":1,"fontSize":12,"color":if t.dark() {"#ffb3ad"} else {"#c4281c"},"lineHeight":17})),
        ],
    )
}
/// A grouped ("inset") box: rows separated by hairlines.
pub fn group(t: &Theme, rows: Vec<Value>, style: Value) -> Value {
    let rows = rows
        .into_iter()
        .filter(|r| !r.is_null())
        .enumerate()
        .map(|(i, r)| if i > 0 { view(json!({"borderTopWidth":1,"borderTopColor":t.sep()}), vec![r]) } else { view(json!({}), vec![r]) })
        .collect();
    view(
        merge(json!({"borderRadius":10,"backgroundColor":if t.dark() {"rgba(255,255,255,0.04)"} else {"rgba(0,0,0,0.025)"},
            "border":format!("1px solid {}", t.s(t.sep())),"paddingHorizontal":12}), style),
        rows,
    )
}
/// A checkbox with its label (16 px box, accent when on).
pub fn check(t: &Theme, on: bool, caption: Value, action: Value, color: Option<Value>) -> Value {
    let mut boxed = view(
        json!({"width":16,"height":16,"borderRadius":4,"border":format!("1px solid {}", if on { "transparent".into() } else { t.s(t.dim()) }),
            "backgroundColor":if on { color.unwrap_or_else(|| t.accent()) } else { t.input_bg() },"alignItems":"center","justifyContent":"center"}),
        vec![],
    );
    if on {
        boxed["children"] = json!([text(json!("✓"), json!({"fontSize":11,"fontWeight":700,"color":"#fff"}))]);
    }
    click(
        view(json!({"flexDirection":"row","gap":8,"alignItems":"center","minHeight":24}),
            vec![boxed, text(caption, json!({"flex":1,"fontSize":13,"color":t.text(),"lineHeight":18}))]),
        action,
    )
}

pub fn strength_color(score: i64) -> &'static str {
    [RED, RED, ORANGE, YELLOW, GREEN][score.clamp(0, 4) as usize]
}
pub fn strength_label(score: i64) -> Value {
    k(&format!("strength.{}", score.clamp(0, 4)))
}
pub fn strength_bar(t: &Theme, score: i64, bits: &Value) -> Value {
    let c = strength_color(score);
    let segs = (0..4)
        .map(|i| view(json!({"flex":1,"height":4,"borderRadius":2,"backgroundColor":if i < score.max(1) { json!(c) } else { t.control() }}), vec![]))
        .collect();
    let caption = if bits.is_null() { strength_label(score) } else { kp("strength.bits", json!({"label":strength_label(score),"bits":bits})) };
    view(json!({"gap":6}), vec![
        view(json!({"flexDirection":"row","gap":4,"direction":"ltr"}), segs),
        text(caption, json!({"fontSize":11,"fontWeight":500,"color":c})),
    ])
}

/// The window's toast: a dark pill at the bottom, above everything.
pub fn toast(t: &Theme, caption: Value) -> Value {
    view(
        json!({"position":"absolute","bottom":20,"left":0,"right":0,"alignItems":"center","zIndex":80,"pointerEvents":"none"}),
        vec![view(
            json!({"paddingHorizontal":14,"height":32,"justifyContent":"center","borderRadius":16,"backgroundColor":if t.dark() {"#3a3a3c"} else {"#1c1c1e"},
                "boxShadow":"0 6px 20px rgba(0,0,0,0.3)"}),
            vec![text(caption, json!({"fontSize":12,"fontWeight":500,"color":"#fff","whiteSpace":"nowrap"}))],
        )],
    )
}

/// A sheet over the window: the dimmed backdrop and the panel (title, close, body, footer).
pub fn sheet(t: &Theme, title: Value, close: Option<Value>, width: f64, body: Vec<Value>, footer: Vec<Value>) -> Value {
    let mut head = vec![text(title, json!({"flex":1,"minWidth":0,"fontSize":15,"fontWeight":600,"color":t.text(),"whiteSpace":"nowrap","textOverflow":"ellipsis"}))];
    if let Some(close) = close {
        head.push(tool(t, Tool { icon: json!("✕"), icon_size: 11., small: true, tip: k("close"), ..Tool::default() }, close));
    }
    let mut panel = vec![
        view(json!({"height":48,"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":20,"borderBottomWidth":1,"borderBottomColor":t.sep()}), head),
        json!({"kind":"scroll","key":"sheet-body","style":{"flex":-1},"children":[view(json!({"paddingHorizontal":20,"paddingVertical":16,"gap":12}), body)]}),
    ];
    if !footer.is_empty() {
        panel.push(view(json!({"flexDirection":"row","justifyContent":"flex-end","gap":8,"paddingHorizontal":20,"paddingVertical":12,"borderTopWidth":1,"borderTopColor":t.sep()}), footer));
    }
    on(
        view(
            json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":50,"alignItems":"center","justifyContent":"center","backgroundColor":"rgba(0,0,0,0.38)"}),
            vec![view(
                json!({"width":width,"maxWidth":"92%","maxHeight":"92%","borderRadius":12,"backgroundColor":t.bg(),"border":format!("1px solid {}", t.s(t.sep())),
                    "boxShadow":"0 18px 50px rgba(0,0,0,0.45)","overflow":"hidden"}),
                panel,
            )],
        ),
        "onMouseDown",
        json!(["noop"]),
    )
}

/// The app icon: a padlock with three dots.
pub fn app_icon(s: f64) -> Value {
    let dots = (0..3).map(|_| view(json!({"width":s*0.09,"height":s*0.09,"borderRadius":s*0.045,"backgroundColor":"#1f6fd1"}), vec![])).collect();
    view(
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #3fd2c7, #1f6fd1)","overflow":"hidden",
            "boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)", s*0.04, s*0.12)}),
        vec![
            view(json!({"position":"absolute","left":s*0.3,"top":s*0.14,"width":s*0.4,"height":s*0.4,"borderRadius":s*0.2,
                "border":format!("{}px solid #ffffff", (s*0.075).max(2.))}), vec![]),
            view(json!({"position":"absolute","left":s*0.19,"top":s*0.38,"width":s*0.62,"height":s*0.44,"borderRadius":s*0.1,"backgroundColor":"#ffffff",
                "flexDirection":"row","alignItems":"center","justifyContent":"center","gap":s*0.05,
                "boxShadow":format!("0 {}px {}px rgba(0,0,0,0.25)", s*0.02, s*0.05)}), dots),
        ],
    )
}

/// A category's tile: its color and its glyph.
pub fn cat_icon(cat: &Value, builtin: Option<&Value>, size: f64) -> Value {
    let color = cat["color"].as_str().or_else(|| builtin.and_then(|b| b["color"].as_str())).unwrap_or("#8e8e93");
    let icon = cat["icon"].as_str().or_else(|| builtin.and_then(|b| b["icon"].as_str())).unwrap_or("");
    tile(&super::glyphs::icon_kind(icon, "folder"), color, size, None)
}

/// The site's initial (or the title's) on the category color.
pub fn entry_icon(entry: &Value, cat: &Value, builtin: Option<&Value>, size: f64) -> Value {
    let color = cat["color"].as_str().or_else(|| builtin.and_then(|b| b["color"].as_str())).unwrap_or("#8e8e93");
    let url = entry["url"].as_str().unwrap_or("");
    let host = strip_scheme(url);
    let host = host.strip_prefix("www.").unwrap_or(host);
    let letter_cat = ["internet", "general", "email", "apps", "secrets", "keyring"].contains(&entry["cat"].as_str().unwrap_or(""));
    if host.is_empty() && !letter_cat {
        return cat_icon(cat, builtin, size);
    }
    let src = if !host.is_empty() { host } else { entry["title"].as_str().filter(|s| !s.is_empty()).unwrap_or("?") };
    let ch: String = src.chars().next().unwrap_or('?').to_uppercase().collect();
    view(
        json!({"width":size,"height":size,"borderRadius":size*0.26,"alignItems":"center","justifyContent":"center",
            "background":format!("linear-gradient(180deg, {}, {})", tint(color, 0.18), tint(color, -0.12))}),
        vec![text(json!(ch), json!({"fontSize":(size*0.46).round(),"fontWeight":600,"color":"#fff"}))],
    )
}
fn strip_scheme(url: &str) -> &str {
    if let Some(i) = url.find("://") {
        if i > 0 && url[..i].chars().all(|c| c.is_ascii_alphabetic()) {
            return &url[i + 3..];
        }
    }
    url
}

pub fn g(kind: &str, size: f64, color: &Value) -> Value {
    glyph(kind, size, color.as_str().unwrap_or("#fff"))
}

/// sdk/components.jsx Toggle (small: 36 × 20).
pub fn toggle(t: &Theme, on: bool, small: bool, action: Value) -> Value {
    let (w, h) = if small { (36., 20.) } else { (42., 24.) };
    let kn = h - 4.;
    click(
        view(json!({"width":w,"height":h,"borderRadius":h/2.,"direction":"ltr","backgroundColor":if on { t.accent() } else { t.control() }}), vec![
            view(json!({"position":"absolute","top":2,"left":if on { w - kn - 2. } else { 2. },"width":kn,"height":kn,"borderRadius":kn/2.,
                "backgroundColor":"#fff","boxShadow":"0 1px 3px rgba(0,0,0,0.35)"}), vec![]),
        ]),
        action,
    )
}

/// sdk/components.jsx Stepper: `cmd` gets the new value.
pub fn stepper(t: &Theme, cmd: &str, value: i64, min: i64, max: i64) -> Value {
    let btn = |label: &str, v: i64| {
        click(view(json!({"width":26,"height":26,"alignItems":"center","justifyContent":"center"}),
            vec![text(json!(label), json!({"fontSize":15,"fontWeight":600,"color":t.text()}))]), json!([cmd, v.clamp(min, max)]))
    };
    view(json!({"flexDirection":"row","alignItems":"center","backgroundColor":t.control(),"borderRadius":t.radius(false),"height":26}), vec![
        btn("−", value - 1),
        text(json!(value.to_string()), json!({"width":56,"textAlign":"center","fontSize":12,"fontWeight":600,"color":t.text()})),
        btn("+", value + 1),
    ])
}

/// sdk/components.jsx Slider: `cmd` gets the pointer (with the slider's frame) on press.
pub fn slider(t: &Theme, cmd: &str, value: f64, min: f64, max: f64, width: f64) -> Value {
    let f = ((value - min) / if max - min == 0. { 1. } else { max - min }).clamp(0., 1.);
    on(
        view(json!({"width":width,"height":20,"justifyContent":"center","direction":"ltr"}), vec![
            view(json!({"height":4,"borderRadius":2,"backgroundColor":t.control()}), vec![
                view(json!({"width":(f*width).max(0.),"height":4,"borderRadius":2,"backgroundColor":t.accent()}), vec![]),
            ]),
            view(json!({"position":"absolute","left":f*(width-16.),"top":2,"width":16,"height":16,"borderRadius":8,"backgroundColor":"#fff",
                "boxShadow":"0 1px 3px rgba(0,0,0,0.4)"}), vec![]),
        ]),
        "onMouseDown",
        json!([cmd]),
    )
}

/// A tag or category chip; `remove`: the ✕ that takes it away.
pub fn chip(t: &Theme, label: Value, color: &str, remove: Option<Value>) -> Value {
    let mut kids = vec![
        view(json!({"width":7,"height":7,"borderRadius":4,"backgroundColor":color}), vec![]),
        text(label, json!({"fontSize":11,"fontWeight":500,"color":t.text(),"whiteSpace":"nowrap"})),
    ];
    if let Some(r) = remove {
        kids.push(click(text(json!("✕"), json!({"fontSize":9,"color":t.dim(),"paddingInlineStart":2})), r));
    }
    view(json!({"flexDirection":"row","alignItems":"center","gap":6,"height":22,"paddingHorizontal":9,"borderRadius":11,"backgroundColor":t.control()}), kids)
}

/// ui.jsx IconButton: a small borderless button with a glyph (a string, or a drawn kind).
pub fn icon_button(t: &Theme, glyph_str: Option<&str>, drawn: Option<&str>, label: Value, tip_caption: Value, active: bool, color: Option<Value>, action: Value) -> Value {
    let icon = match (drawn, glyph_str) {
        (Some(kind), _) => g(kind, 14., &if active { t.accent() } else { color.clone().unwrap_or_else(|| t.text()) }),
        (None, Some(s)) => json!(s),
        _ => Value::Null,
    };
    tool(t, Tool { icon, icon_size: 13., label, tip: tip_caption, small: true, active, color, ..Tool::default() }, action)
}

/// ui.jsx Choice: the button; the list is drawn at the window's root (see `choice_list`).
pub fn choice(t: &Theme, id: &str, label: Value, open: bool, width: f64) -> Value {
    click(
        keyed(&format!("choice:{id}"), json!({"width":width,"height":28,"paddingHorizontal":10,"borderRadius":7,"flexDirection":"row","alignItems":"center","gap":6,
            "backgroundColor":t.control(),"border":format!("1px solid {}", if open { t.s(t.accent()) } else { "transparent".into() })}), vec![
            text(label, json!({"flex":1,"minWidth":0,"fontSize":13,"color":t.text(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
            text(json!("▼"), json!({"fontSize":9,"color":t.dim()})),
        ]),
        json!(["choice", id]),
    )
}
/// The open Choice's list, under its button, above every field of the window.
pub fn choice_list(t: &Theme, id: &str, value: &Value, options: &[(Value, Value)], width: f64) -> Vec<Value> {
    let rows = options
        .iter()
        .map(|(v, label)| {
            let on = v == value;
            hover(
                click(
                    view(json!({"height":24,"paddingHorizontal":8,"borderRadius":5,"flexDirection":"row","alignItems":"center","gap":6}), vec![
                        hover(text(json!(if on { "✓" } else { "" }), json!({"width":12,"fontSize":11,"color":t.text()})), json!({"color":"#fff"})),
                        hover(text(label.clone(), json!({"flex":1,"minWidth":0,"fontSize":13,"color":t.text(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})), json!({"color":"#fff"})),
                    ]),
                    json!(["pick", {"id":id,"v":v}]),
                ),
                json!({"backgroundColor":t.accent()}),
            )
        })
        .collect();
    let mut list = view(
        json!({"position":"absolute","left":0,"top":32,"width":width,"borderRadius":8,"padding":5,"zIndex":901,
            "backgroundColor":if t.dark() {"#2c2c31"} else {"#ffffff"},"border":format!("1px solid {}", t.s(t.sep())),"boxShadow":"0 10px 30px rgba(0,0,0,0.35)"}),
        rows,
    );
    list["anchorKey"] = json!(format!("choice:{id}"));
    vec![
        on(view(json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":900}), vec![]), "onMouseDown", json!(["choiceClose"])),
        list,
    ]
}

/// ui.jsx TextArea: a multi-line field the app edits itself (notes, keys, seed phrases).
pub fn textarea(t: &Theme, id: &str, value: &str, placeholder: Value, min_height: f64, mono: bool, secure: bool, focused: bool) -> Value {
    let caret = if focused { "▏" } else { "" };
    let body = if !value.is_empty() {
        let shown: String = if secure && !focused { value.chars().map(|c| if c == '\n' { '\n' } else { '•' }).collect() } else { value.into() };
        let mut st = json!({"fontSize":13,"color":t.text(),"lineHeight":18});
        if mono {
            st["fontFamily"] = json!("monospace");
        }
        text(json!(format!("{shown}{caret}")), st)
    } else {
        text(if focused { json!("▏") } else { placeholder }, json!({"fontSize":13,"color":t.dim()}))
    };
    on(
        keyed(&format!("ta:{id}"), json!({"minHeight":min_height,"paddingHorizontal":10,"paddingVertical":7,"borderRadius":7,"backgroundColor":t.input_bg(),
            "border":format!("1px solid {}", t.s(if focused { t.accent() } else { t.sep() })),"cursor":"text"}), vec![body]),
        "onMouseDown",
        json!(["taFocus", id]),
    )
}

/// The RGBA pixels of a QR code (hex rows from axia-vault), with its quiet zone.
pub fn qr_pixels(qr: &Value, scale: usize) -> Option<(usize, Vec<u8>)> {
    let n = qr["size"].as_u64()? as usize;
    if n == 0 || n > 256 || scale == 0 || scale > 16 {
        return None;
    }
    let quiet = 4;
    let side = (n + quiet * 2) * scale;
    let mut data = vec![255u8; side * side * 4];
    let rows = qr["rows"].as_array()?;
    for y in 0..n {
        let row = rows.get(y).and_then(Value::as_str).unwrap_or("").as_bytes();
        for x in 0..n {
            let nib = row.get(x >> 2).and_then(|c| (*c as char).to_digit(16)).unwrap_or(0);
            if (nib >> (3 - (x & 3))) & 1 == 0 {
                continue;
            }
            for dy in 0..scale {
                let mut o = (((y + quiet) * scale + dy) * side + (x + quiet) * scale) * 4;
                for _ in 0..scale {
                    data[o] = 0;
                    data[o + 1] = 0;
                    data[o + 2] = 0;
                    o += 4;
                }
            }
        }
    }
    Some((side, data))
}

/// base64 → bytes (attachment previews).
pub fn b64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let clean: Vec<u8> = s.bytes().filter_map(val).collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for ch in clean.chunks(4) {
        if ch.len() < 2 {
            break;
        }
        let mut v = 0u32;
        for (i, c) in ch.iter().enumerate() {
            v |= (*c as u32) << (18 - 6 * i);
        }
        out.push((v >> 16) as u8);
        if ch.len() > 2 {
            out.push((v >> 8) as u8);
        }
        if ch.len() > 3 {
            out.push(v as u8);
        }
    }
    out
}

/// A tag's color: its own, else one of eight picked by its name.
pub fn tag_color(tags: &Value, name: &str) -> String {
    if let Some(c) = tags.as_array().into_iter().flatten().find(|x| x["name"] == name).and_then(|x| x["color"].as_str()) {
        return c.into();
    }
    let mut h: u32 = 0;
    for ch in name.chars() {
        let mut unit = [0u16; 2];
        h = h.wrapping_mul(31).wrapping_add(ch.encode_utf16(&mut unit)[0] as u32);   // JS charCodeAt(0) of each code point
    }
    ["#0a84ff", "#5e5ce6", "#bf5af2", "#ff375f", "#ff9f0a", "#30d158", "#64d2ff", "#ac8e68"][(h % 8) as usize].into()
}
