//! Scene of the native Preferences window: the profile list and the sections on the left, the
//! form on the right and the import panel on top. Pure functions of the facts the app gathers;
//! every control sends a `cmd`/`key` request back (`pf…`).
use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub(super) const SIDE: f64 = 196.;
pub(super) const SIDE_NARROW: f64 = 168.;
const LABEL_W: f64 = 170.;
/// Under this width the labels of the form go above their controls.
pub(super) const NARROW: f64 = 480.;
/// Under this width the sidebar gets a little narrower.
pub(super) const NARROW_WINDOW: f64 = 420.;
const SWATCHES: [&str; 6] = ["#ff453a", "#ff9f0a", "#ffd60a", "#32d74b", "#0a84ff", "#bf5af2"];

pub(super) const SECTIONS: [(&str, &str, &str); 7] = [
    ("general", "terminal.prefs.sec.general", "≡"),
    ("colors", "terminal.prefs.sec.colors", "◐"),
    ("text", "terminal.prefs.sec.text", "A"),
    ("window", "terminal.prefs.sec.window", "▭"),
    ("terminal", "terminal.prefs.sec.terminal", "▤"),
    ("keys", "terminal.prefs.sec.keys", "⌨"),
    ("triggers", "terminal.prefs.sec.triggers", "⟳"),
];

pub(super) struct ProfileRow {
    pub id: String,
    pub name: Value,
    pub default: bool,
}
pub(super) struct ShortcutRow {
    pub action: String,
    pub label: String,
    pub first: Option<String>,
    pub others: Vec<String>,
    pub changed: bool,
}
pub(super) struct ImportRow {
    pub name: String,
    pub detail: String,
    pub picked: bool,
    pub star: bool,
    pub scheme: Value,
}
pub(super) struct ImportResult {
    pub profiles: Vec<ImportRow>,
    pub schemes: Vec<Value>,
    pub warnings: Vec<String>,
    pub can_default: bool,
}
pub(super) struct ImportFacts {
    pub path: String,
    pub files: Vec<String>,
    pub error: Option<String>,
    pub result: Option<ImportResult>,
}
pub(super) struct Facts<'a> {
    pub theme: &'a Value,
    pub width: f64,
    pub cfg: Value,
    pub p: Value,
    pub scheme: Value,
    pub profiles: Vec<ProfileRow>,
    pub presets: Vec<(String, Value)>,
    pub imported: Vec<(String, Value)>,
    pub section: usize,
    pub picker: Option<String>,
    pub color_edit: Option<(String, String)>,
    pub capture: Option<Value>,
    pub notice: Option<Value>,
    pub import: Option<ImportFacts>,
    pub mono: Vec<String>,
    pub families: Vec<String>,
    pub actions: Vec<(String, String)>,
    pub shortcuts: Vec<ShortcutRow>,
    pub labels: HashMap<String, String>,
}

fn tok<'a>(theme: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    theme[key].as_str().unwrap_or(fallback)
}
fn node(key: Option<&str>, style: Value, children: Vec<Value>) -> Value {
    let mut v = json!({"kind":"view","style":style,"children":children});
    if let Some(key) = key {
        v["key"] = json!(key);
    }
    v
}
fn view(style: Value, children: Vec<Value>) -> Value {
    node(None, style, children)
}
fn keyed(key: &str, style: Value, children: Vec<Value>) -> Value {
    node(Some(key), style, children)
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn click(mut n: Value, name: &str, action: Value) -> Value {
    n["events"][name] = action;
    n
}
fn caption(key: &str) -> Value {
    json!({"key":key})
}
fn pf(key: &str) -> Value {
    json!({"key":format!("terminal.prefs.{key}")})
}
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.) != 0.,
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn num(v: &Value) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(0.)
}

// ---- controls ----

fn button(f: &Facts, key: &str, label: Value, action: Value, primary: bool, small: bool) -> Value {
    let th = f.theme;
    click(
        keyed(
            key,
            json!({"height":if small {22} else {26},"paddingHorizontal":if small {10} else {14},"borderRadius":6,"alignItems":"center","justifyContent":"center",
                "backgroundColor":if primary {tok(th,"accent","#0a84ff")} else {tok(th,"control","#3a3a3e")},"flexShrink":0}),
            vec![text(label, json!({"fontSize":if small {12} else {13},"whiteSpace":"nowrap","color":if primary {"#ffffff"} else {tok(th,"text","#ffffff")}}))],
        ),
        "onClick",
        action,
    )
}
fn toggle(f: &Facts, key: &str, on: bool, field: &str) -> Value {
    let th = f.theme;
    click(
        keyed(
            key,
            json!({"width":34,"height":18,"borderRadius":9,"direction":"ltr","flexShrink":0,"backgroundColor":if on {tok(th,"accent","#0a84ff")} else {tok(th,"control","#3a3a3e")}}),
            vec![view(
                json!({"position":"absolute","top":2,"left":if on {18} else {2},"width":14,"height":14,"borderRadius":7,"backgroundColor":"#ffffff","boxShadow":"0 1px 3px rgba(0,0,0,0.35)"}),
                vec![],
            )],
        ),
        "onClick",
        json!(["pfFlip", field]),
    )
}
/// A row of options; `value` is compared with each option's value.
fn segments(f: &Facts, id: &str, options: &[(Value, Value)], current: &Value, action: &dyn Fn(&Value) -> Value) -> Value {
    let th = f.theme;
    keyed(
        id,
        json!({"flexDirection":"row","padding":2,"borderRadius":6,"backgroundColor":tok(th,"control","#3a3a3e"),"flexShrink":0}),
        options
            .iter()
            .enumerate()
            .map(|(i, (value, label))| {
                let on = value == current;
                click(
                    keyed(
                        &format!("{id}-{i}"),
                        json!({"paddingHorizontal":10,"height":18,"borderRadius":5,"alignItems":"center","justifyContent":"center",
                            "backgroundColor":if on {tok(th,"accent","#0a84ff")} else {"transparent"}}),
                        vec![text(label.clone(), json!({"fontSize":11,"fontWeight":600,"whiteSpace":"nowrap","color":if on {"#ffffff"} else {tok(th,"text","#ffffff")}}))],
                    ),
                    "onClick",
                    action(value),
                )
            })
            .collect(),
    )
}
fn field_segments(f: &Facts, id: &str, field: &str, options: &[(Value, Value)]) -> Value {
    segments(f, id, options, &f.p[field], &|value| json!(["pfVal", [field, value]]))
}
fn stepper(f: &Facts, id: &str, field: &str, min: f64, max: f64, step: f64, shown: String) -> Value {
    let th = f.theme;
    let value = num(&f.p[field]);
    let arrow = |glyph: &str, name: &str, d: f64| {
        let next = (((value + d) / step).round() * step).clamp(min, max);
        let next = (next * 100.).round() / 100.;
        click(
            keyed(&format!("{id}-{name}"), json!({"width":26,"height":26,"alignItems":"center","justifyContent":"center"}),
                vec![text(json!(glyph), json!({"fontSize":15,"fontWeight":600,"color":tok(th,"text","#ffffff")}))]),
            "onClick",
            json!(["pfVal", [field, next]]),
        )
    };
    keyed(
        id,
        json!({"flexDirection":"row","alignItems":"center","height":26,"borderRadius":7,"backgroundColor":tok(th,"control","#3a3a3e"),"flexShrink":0}),
        vec![
            arrow("−", "minus", -step),
            text(json!(shown), json!({"width":64,"textAlign":"center","fontSize":12,"fontWeight":600,"color":tok(th,"text","#ffffff")})),
            arrow("+", "plus", step),
        ],
    )
}
fn slider(f: &Facts, id: &str, field: &str, min: f64, max: f64) -> Value {
    let th = f.theme;
    let width = 200.;
    let fraction = ((num(&f.p[field]) - min) / (max - min)).clamp(0., 1.);
    let mut n = keyed(
        id,
        json!({"width":width,"height":20,"justifyContent":"center","direction":"ltr","flexShrink":0}),
        vec![
            view(
                json!({"height":4,"borderRadius":2,"backgroundColor":tok(th,"control","#3a3a3e")}),
                vec![view(json!({"width":format!("{}%",fraction*100.),"height":4,"borderRadius":2,"backgroundColor":tok(th,"accent","#0a84ff")}), vec![])],
            ),
            view(
                json!({"position":"absolute","left":(width-16.)*fraction,"top":2,"width":16,"height":16,"borderRadius":8,"backgroundColor":"#ffffff","boxShadow":"0 1px 3px rgba(0,0,0,0.3)"}),
                vec![],
            ),
        ],
    );
    n["events"]["onMouseDown"] = json!(["pfSlide", [field, min, max]]);
    n
}
fn input(id: &str, value: &str, placeholder: Value, change: Value, width: f64) -> Value {
    json!({"kind":"input","key":id,"value":value,"placeholder":placeholder,"change":change,"size":13,"style":{"width":width,"maxWidth":"100%","height":24}})
}
fn field_input(id: &str, f: &Facts, field: &str, placeholder: Value, width: f64) -> Value {
    input(id, f.p[field].as_str().unwrap_or(""), placeholder, json!(["pfText", field]), width)
}
/// A drop-down that opens inline (the Preferences window has no floating layers).
fn picker(f: &Facts, id: &str, options: &[(Value, String)], current: &Value, width: f64) -> Value {
    let th = f.theme;
    let label = options.iter().find(|(v, _)| v == current).map_or_else(|| "—".to_owned(), |(_, l)| l.clone());
    let open = f.picker.as_deref() == Some(id);
    let head = click(
        keyed(
            &format!("{id}-head"),
            json!({"height":24,"borderRadius":6,"paddingLeft":9,"paddingRight":4,"flexDirection":"row","alignItems":"center","gap":6,
                "backgroundColor":tok(th,"control","#3a3a3e")}),
            vec![
                text(json!(label), json!({"flex":1,"minWidth":0,"fontSize":12,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start","color":tok(th,"text","#ffffff")})),
                view(
                    json!({"width":16,"height":18,"borderRadius":4,"backgroundColor":tok(th,"accent","#0a84ff"),"alignItems":"center","justifyContent":"center"}),
                    vec![text(json!("▲"), json!({"fontSize":8,"lineHeight":8,"color":"#ffffff"})), text(json!("▼"), json!({"fontSize":8,"lineHeight":8,"color":"#ffffff"}))],
                ),
            ],
        ),
        "onClick",
        json!(["pfPicker", id]),
    );
    let mut children = vec![head];
    if open {
        let height = ((options.len() * 24 + 10) as f64).min(200.);
        children.push(json!({"kind":"scroll","key":format!("{id}-list"),
            "style":{"height":height,"marginTop":4,"borderRadius":8,"borderWidth":1,"borderColor":tok(th,"separator","#444444"),
                "backgroundColor":if th["mode"] == "dark" {"#2c2c31"} else {"#ffffff"},"boxShadow":"0 6px 18px rgba(0,0,0,0.25)"},
            "children":[view(json!({"padding":4}), options.iter().enumerate().map(|(i, (value, label))| {
                let on = value == current;
                click(
                    keyed(
                        &format!("{id}-{i}"),
                        json!({"height":24,"justifyContent":"center","paddingHorizontal":8,"borderRadius":5,"backgroundColor":if on {tok(th,"accent","#0a84ff")} else {"transparent"}}),
                        vec![text(json!(label), json!({"fontSize":12,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start","color":if on {"#ffffff"} else {tok(th,"text","#ffffff")}}))],
                    ),
                    "onClick",
                    json!(["pfPick", [id, value]]),
                )
            }).collect())]}));
    }
    keyed(&format!("{id}-picker"), json!({"width":width,"maxWidth":"100%"}), children)
}
fn combo_button(f: &Facts, id: &str, spec: Value, combo: Option<&str>, width: f64) -> Value {
    let th = f.theme;
    let active = f.capture.as_ref() == Some(&spec);
    let label = if active {
        pf("combo.press")
    } else {
        match combo.filter(|c| !c.is_empty()) {
            Some(c) => json!(f.labels.get(c).cloned().unwrap_or_else(|| c.to_owned())),
            None => pf("combo.choose"),
        }
    };
    click(
        keyed(
            id,
            json!({"width":width,"height":24,"borderRadius":6,"alignItems":"center","justifyContent":"center","paddingHorizontal":6,"flexShrink":0,
                "borderWidth":1,"borderColor":if active {tok(th,"accent","#0a84ff")} else {tok(th,"separator","#444444")},
                "backgroundColor":if active {"rgba(10,132,255,0.15)"} else {tok(th,"control","#3a3a3e")}}),
            vec![text(label, json!({"fontSize":12,"whiteSpace":"nowrap","direction":"ltr","color":if active {tok(th,"accent","#0a84ff")} else {tok(th,"text","#ffffff")}}))],
        ),
        "onClick",
        json!(["pfCapture", spec]),
    )
}
fn color_well(f: &Facts, key: &str, label: Value, value: &str) -> Value {
    let th = f.theme;
    let editing = f.color_edit.as_ref().filter(|(k, _)| k == key);
    let swatch = click(
        keyed("swatch", json!({"width":28,"height":18,"borderRadius":5,"backgroundColor":value,"borderWidth":1,"borderColor":tok(th,"separator","#444444"),"flexShrink":0}), vec![]),
        "onClick",
        json!(["pfColorEdit", key]),
    );
    let second = match editing {
        Some((_, edit)) => json!({"kind":"input","key":format!("color-{key}"),"value":edit,"autoFocus":true,"change":["pfColorText"],"submit":["pfColorOk"],
            "cancel":["pfColorCancel"],"size":12,"style":{"width":92,"height":22}}),
        None => text(label, json!({"flex":1,"minWidth":0,"fontSize":11,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start","color":tok(th,"textDim","#999999")})),
    };
    keyed(&format!("well-{key}"), json!({"width":150,"height":26,"flexDirection":"row","alignItems":"center","gap":8}), vec![swatch, second])
}
fn scheme_card(f: &Facts, id: &str, scheme: &Value, selected: bool, action: Value) -> Value {
    let th = f.theme;
    let ansi = |i: usize| scheme["ansi"][i].as_str().unwrap_or("#888888").to_owned();
    click(
        keyed(
            &format!("scheme-{id}"),
            json!({"padding":2,"borderRadius":10,"borderWidth":2,"borderColor":if selected {tok(th,"accent","#0a84ff")} else {"transparent"}}),
            vec![view(
                json!({"width":128,"height":62,"borderRadius":7,"padding":8,"backgroundColor":scheme["bg"].as_str().unwrap_or("#000000"),"borderWidth":1,
                    "borderColor":tok(th,"separator","#444444"),"overflow":"hidden","direction":"ltr"}),
                vec![
                    text(json!(scheme["name"].as_str().unwrap_or("")), json!({"fontSize":11,"fontWeight":600,"whiteSpace":"nowrap","overflow":"hidden","color":scheme["fg"].as_str().unwrap_or("#ffffff")})),
                    view(
                        json!({"flexDirection":"row","marginTop":2}),
                        vec![
                            text(json!("axia@axia "), json!({"fontSize":10,"fontFamily":"monospace","color":ansi(2)})),
                            text(json!("~"), json!({"fontSize":10,"fontFamily":"monospace","color":ansi(4)})),
                        ],
                    ),
                    view(json!({"flexDirection":"row","marginTop":6,"gap":2}), (0..8).map(|i| view(json!({"width":12,"height":6,"borderRadius":2,"backgroundColor":ansi(i)}), vec![])).collect()),
                ],
            )],
        ),
        "onClick",
        action,
    )
}

// ---- layout of the form ----

fn narrow(f: &Facts) -> bool {
    let side = if f.width - SIDE < NARROW_WINDOW { SIDE_NARROW } else { SIDE };
    f.width - side < NARROW
}
fn field(f: &Facts, label: Value, controls: Vec<Value>, note: Option<Value>) -> Value {
    let th = f.theme;
    let narrow = narrow(f);
    let label = if narrow {
        text(label, json!({"fontSize":12,"fontWeight":500,"textAlign":"start","color":tok(th,"text","#ffffff")}))
    } else {
        text(label, json!({"width":LABEL_W,"paddingTop":6,"lineHeight":1.3,"fontSize":13,"textAlign":"end","color":tok(th,"text","#ffffff")}))
    };
    let mut column = vec![view(json!({"minHeight":28,"flexDirection":"row","flexWrap":"wrap","alignItems":"center","gap":8}), controls)];
    if let Some(note) = note {
        column.push(text(note, json!({"fontSize":11,"lineHeight":1.4,"textAlign":"start","color":tok(th,"textDim","#999999")})));
    }
    view(
        json!({"flexDirection":if narrow {"column"} else {"row"},"alignItems":if narrow {"stretch"} else {"flex-start"},"gap":if narrow {4} else {12},"paddingVertical":4}),
        vec![label, view(json!({"flex":1,"minWidth":0,"gap":3}), column)],
    )
}
fn heading(f: &Facts, label: Value, top: f64) -> Value {
    text(label, json!({"fontSize":13,"fontWeight":600,"marginTop":top,"marginBottom":8,"textAlign":"start","color":tok(f.theme,"text","#ffffff")}))
}
fn group(f: &Facts, rows: Vec<Value>) -> Value {
    let th = f.theme;
    view(
        json!({"paddingHorizontal":10,"borderRadius":8,"borderWidth":1,"borderColor":tok(th,"separator","#444444"),
            "backgroundColor":if th["mode"] == "dark" {"rgba(255,255,255,0.04)"} else {"rgba(0,0,0,0.025)"}}),
        rows,
    )
}
fn pct(v: &Value) -> String {
    format!("{}%", (num(v) * 100.).round())
}
fn on(f: &Facts, field: &str) -> bool {
    truthy(&f.p[field])
}
fn options(list: &[(&str, Value)]) -> Vec<(Value, Value)> {
    list.iter().map(|(v, l)| (json!(v), l.clone())).collect()
}
fn flag(f: &Facts, label: &str, name: &str, note: Option<Value>) -> Value {
    field(f, pf(label), vec![toggle(f, &format!("t-{name}"), on(f, name), name)], note)
}

// ---- sections ----

fn general(f: &Facts) -> Vec<Value> {
    let th = f.theme;
    let p = &f.p;
    let is_default = f.cfg["defaultProfile"] == p["id"];
    let tab_color = p["tabColor"].as_str().unwrap_or("");
    let swatch = |color: &str| {
        let none = color.is_empty();
        click(
            keyed(
                &format!("tab-{}", if none { "none" } else { color }),
                json!({"width":20,"height":20,"borderRadius":10,"alignItems":"center","justifyContent":"center","flexShrink":0,
                    "backgroundColor":if none {"transparent"} else {color},"borderWidth":if none {2} else {0},"borderColor":tok(th,"textDim","#999999")}),
                if tab_color == color {
                    vec![view(json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":if none {tok(th,"textDim","#999999")} else {"#ffffff"}}), vec![])]
                } else {
                    vec![]
                },
            ),
            "onClick",
            json!(["pfTabColor", color]),
        )
    };
    let mut swatches = vec![swatch("")];
    swatches.extend(SWATCHES.iter().map(|c| swatch(c)));
    if !tab_color.is_empty() && !SWATCHES.contains(&tab_color) {
        swatches.push(keyed(
            "tab-custom",
            json!({"width":20,"height":20,"borderRadius":10,"backgroundColor":tab_color,"alignItems":"center","justifyContent":"center"}),
            vec![view(json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":"#ffffff"}), vec![])],
        ));
    }
    let rules = p["rules"].as_str().unwrap_or("").replace('\n', " ; ");
    let mut rows = vec![
        field(f, pf("name"), vec![field_input("name", f, "name", json!(""), 240.)], None),
        field(
            f,
            pf("defaultProfile"),
            vec![if is_default {
                text(pf("isDefault"), json!({"fontSize":12,"color":tok(th,"textDim","#999999")}))
            } else {
                button(f, "make-default", pf("useDefault"), json!(["pfMakeDefault"]), false, false)
            }],
            None,
        ),
        field(
            f,
            pf("workingDir"),
            vec![field_segments(f, "dir", "workingDir", &options(&[("home", pf("dir.home")), ("reuse", pf("dir.reuse")), ("custom", pf("dir.custom"))]))],
            None,
        ),
    ];
    if p["workingDir"] == "custom" {
        rows.push(field(f, pf("folder"), vec![field_input("customDir", f, "customDir", json!("~/projects"), 240.)], None));
    }
    rows.extend([
        field(f, pf("initialText"), vec![field_input("initialText", f, "initialText", pf("initialTextPh"), 240.)], Some(json!("\\n = Enter"))),
        field(f, pf("badge"), vec![field_input("badge", f, "badge", pf("badgePh"), 200.)], Some(pf("badgeNote"))),
        field(f, pf("tabColor"), swatches, None),
        field(
            f,
            pf("rules"),
            vec![input("rules", &rules, json!("~/prod/** ; root@* ; *.prod.com"), json!(["pfRules"]), 260.)],
            Some(json!({"parts":[pf("rulesNote"), json!(". "), pf("rulesHint")]})),
        ),
        flag(f, "confirmClose", "confirmClose", None),
        field(
            f,
            pf("hotkey"),
            vec![segments(
                f,
                "hotkey",
                &[(json!("F12"), json!("F12")), (json!("Super+`"), json!("❖`")), (json!("off"), pf("off"))],
                &f.cfg["hotkey"],
                &|value| json!(["pfHotkey", value]),
            )],
            None,
        ),
    ]);
    rows
}
fn colors(f: &Facts) -> Vec<Value> {
    let th = f.theme;
    let p = &f.p;
    let scheme = &f.scheme;
    let selected = |id: &str| p["colors"] == id;
    let cards = |list: &[(String, Value)]| {
        view(
            json!({"flexDirection":"row","flexWrap":"wrap","gap":4}),
            list.iter().map(|(id, s)| scheme_card(f, id, s, selected(id), json!(["pfScheme", id]))).collect(),
        )
    };
    let get = |k: &str| scheme[k].as_str().filter(|s| !s.is_empty()).or_else(|| scheme["fg"].as_str()).unwrap_or("#ffffff").to_owned();
    let column = |rows: Vec<Value>| view(json!({}), rows);
    let wells = |list: &[(&str, Value)]| column(list.iter().map(|(k, l)| color_well(f, k, l.clone(), &get(k))).collect());
    let ansi_wells = |bright: bool| {
        column(
            ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"]
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let idx = i + if bright { 8 } else { 0 };
                    let label = if bright { json!({"key":"terminal.prefs.bright","params":{"color":{"key":format!("terminal.prefs.ansi.{name}")}}}) } else { json!({"key":format!("terminal.prefs.ansi.{name}")}) };
                    color_well(f, &format!("ansi:{idx}"), label, scheme["ansi"][idx].as_str().unwrap_or("#888888"))
                })
                .collect(),
        )
    };
    let mut out = vec![heading(f, pf("schemes"), 0.), cards(&f.presets)];
    if !f.imported.is_empty() {
        out.push(heading(f, pf("imported"), 20.));
        out.push(cards(&f.imported));
    }
    out.push(heading(f, if p["colors"] == "custom" { pf("customColors") } else { pf("editColors") }, 20.));
    out.push(view(
        json!({"flexDirection":"row","flexWrap":"wrap","gap":16}),
        vec![
            wells(&[("fg", pf("sec.text")), ("bg", pf("c.bg")), ("bold", pf("c.bold")), ("link", pf("c.link"))]),
            wells(&[("cursor", pf("cursor")), ("cursorText", pf("c.cursorText")), ("selection", pf("c.selection")), ("selectionText", pf("c.selectionText"))]),
            ansi_wells(false),
            ansi_wells(true),
        ],
    ));
    out.push(text(pf("colorHint"), json!({"fontSize":11,"marginTop":8,"lineHeight":1.4,"textAlign":"start","color":tok(th,"textDim","#999999")})));
    out
}
fn text_section(f: &Facts) -> Vec<Value> {
    let p = &f.p;
    let mut fonts = vec![(json!(""), pf_text(f, "fontBuiltin"))];
    fonts.extend(f.mono.iter().filter(|m| *m != "JetBrains Mono").map(|m| (json!(m), m.clone())));
    let mut non_ascii = vec![(json!(""), pf_text(f, "same"))];
    non_ascii.extend(f.families.iter().map(|m| (json!(m), m.clone())));
    vec![
        field(
            f,
            pf("font"),
            vec![picker(f, "font", &fonts, &p["font"], 240.)],
            Some(json!({"key":"terminal.prefs.monoCount","params":{"count":f.mono.len()}})),
        ),
        flag(f, "ligatures", "ligatures", Some(json!("-> => != === (JetBrains Mono, Fira Code…)"))),
        flag(f, "bidi", "bidi", Some(pf("bidiNote"))),
        field(f, pf("nonAscii"), vec![picker(f, "nonAscii", &non_ascii, &p["nonAsciiFont"], 240.)], Some(pf("nonAsciiNote"))),
        field(f, pf("size"), vec![stepper(f, "size", "fontSize", 7., 40., 1., format!("{} pt", num(&p["fontSize"])))], None),
        field(f, pf("lineSpacing"), vec![stepper(f, "spacing", "lineSpacing", 0.8, 2., 0.05, pct(&p["lineSpacing"]))], None),
        field(f, pf("padding"), vec![stepper(f, "padding", "padding", 0., 40., 1., format!("{} px", num(&p["padding"])))], None),
        field(
            f,
            pf("cursor"),
            vec![field_segments(f, "cursor", "cursorStyle", &options(&[("block", pf("cur.block")), ("underline", pf("cur.underline")), ("beam", pf("cur.beam"))]))],
            None,
        ),
        flag(f, "cursorBlink", "cursorBlink", None),
        flag(f, "cursorGuide", "cursorGuide", Some(pf("cursorGuideNote"))),
        flag(f, "boldBright", "boldBright", None),
        flag(f, "useBold", "useBoldFont", Some(pf("useBoldNote"))),
        flag(f, "useItalic", "useItalic", None),
        flag(f, "blinkText", "blinkText", None),
        field(f, pf("minContrast"), vec![slider(f, "contrast", "minimumContrast", 0., 1.)], Some(json!(pct(&p["minimumContrast"])))),
    ]
}
/// The text of a Preferences caption as the facts carry it (a fixed English fallback: the
/// picker shows plain text, so the app resolves these before building the facts).
fn pf_text(f: &Facts, key: &str) -> String {
    f.labels.get(&format!("terminal.prefs.{key}")).cloned().unwrap_or_else(|| key.to_owned())
}
fn window_section(f: &Facts) -> Vec<Value> {
    let p = &f.p;
    let size = |v: &Value| if num(v) > 0. { format!("{}", num(v)) } else { "—".to_owned() };
    vec![
        field(f, pf("transparency"), vec![slider(f, "transparency", "transparency", 0., 0.9)], Some(json!(pct(&p["transparency"])))),
        flag(f, "blur", "blur", None),
        flag(f, "dimInactive", "dimInactive", None),
        flag(f, "statusBar", "statusBar", None),
        field(f, pf("columns"), vec![stepper(f, "columns", "columns", 0., 400., 10., size(&p["columns"]))], Some(pf("columnsNote"))),
        field(f, pf("rows"), vec![stepper(f, "rows", "rows", 0., 200., 5., size(&p["rows"]))], None),
    ]
}
fn terminal_section(f: &Facts) -> Vec<Value> {
    let p = &f.p;
    vec![
        field(
            f,
            pf("scrollback"),
            vec![field_segments(
                f,
                "scrollback",
                "scrollback",
                &[(json!(1000), json!("1.000")), (json!(10000), json!("10.000")), (json!(100000), json!("100.000")), (json!(1000000), pf("million"))],
            )],
            Some(pf("scrollbackNote")),
        ),
        flag(f, "mouseReporting", "mouseReporting", Some(pf("mouseNote"))),
        flag(f, "copyOnSelect", "copyOnSelect", None),
        flag(f, "altScroll", "alternateScroll", None),
        field(f, pf("bell"), vec![field_segments(f, "bell", "bell", &options(&[("visual", pf("bellVisual")), ("none", pf("bellNone"))]))], None),
        field(
            f,
            pf("wordChars"),
            vec![input("wordChars", p["wordChars"].as_str().unwrap_or(""), json!(""), json!(["pfText", "wordChars"]), 160.)],
            Some(pf("wordNote")),
        ),
    ]
}
const KINDS: [&str; 5] = ["text", "escape", "hex", "ignore", "action"];
fn kind_of(m: &Value) -> &'static str {
    match m["action"].as_str() {
        Some("text") => "text",
        Some("escape") => "escape",
        Some("hex") => "hex",
        Some("ignore") => "ignore",
        _ => "action",
    }
}
fn key_map(f: &Facts, global: bool, list: &Value) -> Value {
    let th = f.theme;
    let scope = if global { 1 } else { 0 };
    let kind_label = |k: &str| match k {
        "text" => pf("sec.text"),
        "escape" => json!("Esc+"),
        "hex" => json!("Hex"),
        "ignore" => pf("kind.ignore"),
        _ => pf("kind.action"),
    };
    let rows: Vec<Value> = list
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let id = format!("km{scope}-{i}");
            let kind = kind_of(m);
            let mut row = vec![
                combo_button(f, &format!("{id}-combo"), json!(["map", global, i]), m["combo"].as_str(), 96.),
                segments(
                    f,
                    &format!("{id}-kind"),
                    &KINDS.iter().map(|k| (json!(k), kind_label(k))).collect::<Vec<_>>(),
                    &json!(kind),
                    &|value| json!(["pfMapKind", [global, i, value]]),
                ),
            ];
            if kind == "action" {
                let actions: Vec<(Value, String)> = f.actions.iter().map(|(a, l)| (json!(a), l.clone())).collect();
                row.push(picker(f, &format!("{id}-act"), &actions, &m["action"], 150.));
            } else if kind != "ignore" {
                let placeholder = match kind {
                    "hex" => json!("0x1b 0x5b 0x41"),
                    "escape" => pf("escPh"),
                    _ => json!("git status\\n"),
                };
                row.push(input(&format!("{id}-param"), m["param"].as_str().unwrap_or(""), placeholder, json!(["pfMapText", [global, i]]), 150.));
            }
            row.push(view(json!({"flex":1}), vec![]));
            row.push(button(f, &format!("{id}-del"), json!("−"), json!(["pfMapDel", [global, i]]), false, true));
            view(json!({"flexDirection":"row","flexWrap":"wrap","alignItems":"center","gap":8,"paddingVertical":8}), row)
        })
        .collect();
    let mut col = vec![];
    if !rows.is_empty() {
        col.push(group(f, rows));
    }
    col.push(view(
        json!({"alignSelf":"flex-start"}),
        vec![button(f, &format!("km{scope}-add"), pf("keys.add"), json!(["pfMapAdd", global]), false, true)],
    ));
    let _ = th;
    view(json!({"gap":8}), col)
}
fn keys_section(f: &Facts) -> Vec<Value> {
    let th = f.theme;
    let rows: Vec<Value> = f
        .shortcuts
        .iter()
        .map(|s| {
            let mut row = vec![text(json!(s.label), json!({"flex":1,"minWidth":0,"fontSize":13,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start","color":tok(th,"text","#ffffff")}))];
            if !narrow(f) && !s.others.is_empty() {
                row.push(text(json!(s.others.join("  ")), json!({"fontSize":11,"whiteSpace":"nowrap","direction":"ltr","color":tok(th,"textDim","#999999")})));
            }
            if s.changed {
                row.push(click(
                    keyed(
                        &format!("sc-{}-reset", s.action),
                        json!({"height":22,"paddingHorizontal":8,"justifyContent":"center"}),
                        vec![text(pf("reset"), json!({"fontSize":12,"color":tok(th,"accent","#0a84ff")}))],
                    ),
                    "onClick",
                    json!(["pfReset", s.action]),
                ));
            }
            row.push(combo_button(f, &format!("sc-{}", s.action), json!(["sc", s.action]), s.first.as_deref(), 120.));
            keyed(&format!("sc-row-{}", s.action), json!({"flexDirection":"row","alignItems":"center","minHeight":34,"gap":8,"paddingVertical":4}), row)
        })
        .collect();
    vec![
        flag(f, "altEsc", "altSendsEscape", None),
        heading(f, pf("keys.profile"), 20.),
        key_map(f, false, &f.p["keyMap"]),
        heading(f, pf("keys.global"), 20.),
        key_map(f, true, &f.cfg["keyMap"]),
        heading(f, pf("keys.shortcuts"), 20.),
        group(f, rows),
    ]
}
fn triggers(f: &Facts) -> Vec<Value> {
    let th = f.theme;
    let list = f.p["triggers"].as_array().cloned().unwrap_or_default();
    let actions: Vec<(Value, String)> = ["notify", "bell", "attention", "send", "capture", "highlight", "mark", "title", "stop"]
        .iter()
        .map(|a| (json!(a), pf_text(f, &if *a == "bell" { "bell".to_owned() } else { format!("tr.{a}") })))
        .collect();
    let rows: Vec<Value> = list
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let id = format!("tr-{i}");
            view(
                json!({"flexDirection":"row","flexWrap":"wrap","alignItems":"center","gap":8,"paddingVertical":8}),
                vec![
                    toggle_at(f, &format!("{id}-on"), t["enabled"] != false, json!(["pfTrToggle", i])),
                    input(&format!("{id}-regex"), t["regex"].as_str().unwrap_or(""), json!("regex"), json!(["pfTrText", [i, "regex"]]), 150.),
                    picker(f, &id, &actions, &t["action"], 130.),
                    input(&format!("{id}-param"), t["param"].as_str().unwrap_or(""), pf("param"), json!(["pfTrText", [i, "param"]]), 130.),
                    view(json!({"flex":1}), vec![]),
                    button(f, &format!("{id}-del"), json!("−"), json!(["pfTrDel", i]), false, true),
                ],
            )
        })
        .collect();
    let mut out = vec![text(pf("triggersHint"), json!({"fontSize":12,"marginBottom":12,"lineHeight":1.45,"textAlign":"start","color":tok(th,"textDim","#999999")}))];
    if !rows.is_empty() {
        out.push(group(f, rows));
    }
    out.push(view(json!({"alignSelf":"flex-start","marginTop":8}), vec![button(f, "tr-add", pf("tr.add"), json!(["pfTrAdd"]), false, true)]));
    out
}
/// A switch that runs its own command (the triggers' enabled flag is per row).
fn toggle_at(f: &Facts, key: &str, on: bool, action: Value) -> Value {
    let mut t = toggle(f, key, on, "");
    t["events"]["onClick"] = action;
    t
}

// ---- window ----

fn sidebar_item(f: &Facts, key: &str, label: Value, detail: Option<&str>, icon: Option<&str>, selected: bool, action: Value) -> Value {
    let th = f.theme;
    let fg = if selected { "#ffffff" } else { tok(th, "text", "#ffffff") };
    let mut children = vec![];
    if let Some(icon) = icon {
        children.push(text(json!(icon), json!({"width":16,"textAlign":"center","fontSize":12,"color":if selected {"#ffffff"} else {tok(th,"textDim","#999999")}})));
    }
    children.push(text(label, json!({"flex":1,"minWidth":0,"fontSize":13,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start","color":fg})));
    if let Some(detail) = detail {
        children.push(text(json!(detail), json!({"fontSize":11,"color":fg})));
    }
    let mut item = click(
        keyed(
            key,
            json!({"height":26,"flexDirection":"row","alignItems":"center","gap":6,"paddingHorizontal":8,"borderRadius":6,
                "backgroundColor":if selected {tok(th,"accent","#0a84ff")} else {"transparent"}}),
            children,
        ),
        "onClick",
        action,
    );
    if !selected {
        item["hoverStyle"] = json!({"backgroundColor":tok(th,"hover","rgba(127,127,127,0.12)")});
    }
    item
}
fn tool_button(f: &Facts, key: &str, glyph: &str, tip: &str, disabled: bool, action: Value) -> Value {
    let th = f.theme;
    let mut b = click(
        keyed(
            key,
            json!({"width":26,"height":22,"borderRadius":5,"alignItems":"center","justifyContent":"center","backgroundColor":tok(th,"control","#3a3a3e"),
                "opacity":if disabled {0.4} else {1.}}),
            vec![text(json!(glyph), json!({"fontSize":14,"color":tok(th,"text","#ffffff")}))],
        ),
        "onClick",
        if disabled { json!(["noop"]) } else { action },
    );
    b["tooltip"] = pf(tip);
    b
}
fn rule(f: &Facts, edge: &str) -> Value {
    let mut style = Map::new();
    style.insert("position".into(), json!("absolute"));
    style.insert("backgroundColor".into(), json!(tok(f.theme, "separator", "#444444")));
    style.insert("pointerEvents".into(), json!("none"));
    match edge {
        "end" => style.extend([("right".into(), json!(0)), ("top".into(), json!(0)), ("bottom".into(), json!(0)), ("width".into(), json!(1))]),
        "top" => style.extend([("left".into(), json!(0)), ("right".into(), json!(0)), ("top".into(), json!(0)), ("height".into(), json!(1))]),
        _ => style.extend([("left".into(), json!(0)), ("right".into(), json!(0)), ("bottom".into(), json!(0)), ("height".into(), json!(1))]),
    }
    keyed(&format!("rule-{edge}"), Value::Object(style), vec![])
}
fn sidebar(f: &Facts) -> Value {
    let th = f.theme;
    let mut list: Vec<Value> = vec![text(
        pf("profiles"),
        json!({"fontSize":11,"fontWeight":600,"marginTop":6,"marginBottom":4,"paddingHorizontal":6,"textAlign":"start","color":tok(th,"textDim","#999999")}),
    )];
    let selected = f.p["id"].as_str().unwrap_or("");
    list.extend(f.profiles.iter().map(|r| {
        sidebar_item(f, &format!("profile-{}", r.id), r.name.clone(), r.default.then_some("★"), None, r.id == selected, json!(["pfSelect", r.id]))
    }));
    list.push(view(
        json!({"flexDirection":"row","alignItems":"center","gap":4,"marginTop":8,"paddingHorizontal":4}),
        vec![
            tool_button(f, "add", "+", "newProfile", false, json!(["pfAdd"])),
            tool_button(f, "remove", "−", "remove", f.profiles.len() < 2, json!(["pfRemove"])),
            tool_button(f, "duplicate", "⧉", "duplicate", false, json!(["pfDup"])),
        ],
    ));
    list.push(view(json!({"height":1,"marginVertical":12,"marginHorizontal":4,"backgroundColor":tok(th,"separator","#444444")}), vec![]));
    list.extend(SECTIONS.iter().enumerate().map(|(i, (id, label, icon))| {
        sidebar_item(f, &format!("section-{id}"), caption(label), None, Some(*icon), i == f.section, json!(["pfSection", i]))
    }));
    let width = if f.width - SIDE < NARROW_WINDOW { SIDE_NARROW } else { SIDE };
    keyed(
        "sidebar",
        json!({"width":width,"flexShrink":0,"backgroundColor":tok(th,"sidebarBg","#2a2a2e"),"paddingTop":8}),
        vec![
            rule(f, "end"),
            json!({"kind":"scroll","key":"sidebar-scroll","style":{"flex":1},"children":[view(json!({"paddingHorizontal":10,"paddingBottom":12}), list)]}),
            view(
                json!({"padding":10}),
                vec![rule(f, "top"), click(
                    keyed("import", json!({"height":24,"borderRadius":6,"alignItems":"center","justifyContent":"center"}),
                        vec![text(pf("importBtn"), json!({"fontSize":12,"color":tok(th,"accent","#0a84ff")}))]),
                    "onClick",
                    json!(["pfImportOpen"]),
                )],
            ),
        ],
    )
}
fn main_pane(f: &Facts) -> Value {
    let th = f.theme;
    let (id, label, _) = SECTIONS[f.section.min(SECTIONS.len() - 1)];
    let body = match id {
        "general" => general(f),
        "colors" => colors(f),
        "text" => text_section(f),
        "window" => window_section(f),
        "terminal" => terminal_section(f),
        "keys" => keys_section(f),
        _ => triggers(f),
    };
    let mut children = vec![view(
        json!({"height":48,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":24}),
        vec![
            rule(f, "bottom"),
            view(
                json!({"flex":1,"minWidth":0}),
                vec![
                    text(caption(label), json!({"fontSize":15,"fontWeight":600,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start","color":tok(th,"text","#ffffff")})),
                    text(f.profiles.iter().find(|r| r.id == f.p["id"].as_str().unwrap_or("")).map_or(json!(""), |r| r.name.clone()),
                        json!({"fontSize":11,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start","color":tok(th,"textDim","#999999")})),
                ],
            ),
            button(f, "done", pf("done"), json!(["pfDone"]), true, false),
        ],
    )];
    if let Some(notice) = &f.notice {
        children.push(view(
            json!({"marginHorizontal":24,"marginTop":12,"paddingHorizontal":12,"paddingVertical":8,"borderRadius":8,"backgroundColor":"rgba(50,215,75,0.16)"}),
            vec![text(notice.clone(), json!({"fontSize":12,"textAlign":"start","color":tok(th,"text","#ffffff")}))],
        ));
    }
    children.push(json!({"kind":"scroll","key":format!("page-{id}"),"style":{"flex":1},"children":[view(json!({"paddingHorizontal":24,"paddingTop":16,"paddingBottom":24}), body)]}));
    keyed("main", json!({"flex":1,"minWidth":0}), children)
}
fn import_panel(f: &Facts, import: &ImportFacts) -> Value {
    let th = f.theme;
    let dim = tok(th, "textDim", "#999999");
    let mut found: Vec<Value> = vec![heading(f, pf("import.found"), 0.)];
    if import.files.is_empty() {
        found.push(text(pf("import.none"), json!({"fontSize":12,"lineHeight":1.4,"textAlign":"start","color":dim})));
    }
    found.extend(import.files.iter().enumerate().map(|(i, name)| {
        sidebar_item(f, &format!("file-{i}"), json!(name), None, None, import.path == format!("~/{name}"), json!(["pfImportFile", name]))
    }));
    let mut details: Vec<Value> = vec![];
    if let Some(error) = &import.error {
        details.push(text(json!({"key":"terminal.prefs.import.failed","params":{"error":error}}), json!({"fontSize":13,"color":"#ff453a","textAlign":"start"})));
    }
    if let Some(result) = &import.result {
        if !result.profiles.is_empty() {
            details.push(heading(f, json!({"key":"terminal.prefs.import.profiles","params":{"count":result.profiles.len()}}), 0.));
        }
        for (i, row) in result.profiles.iter().enumerate() {
            let s = &row.scheme;
            let strip: Vec<Value> = (1..7)
                .map(|k| view(json!({"flex":1,"borderRadius":1,"backgroundColor":s["ansi"][k].as_str().unwrap_or("#888888")}), vec![]))
                .collect();
            details.push(view(
                json!({"flexDirection":"row","alignItems":"center","gap":8,"minHeight":32}),
                vec![
                    toggle_at(f, &format!("pick-{i}"), row.picked, json!(["pfImportPick", i])),
                    view(json!({"width":54,"height":20,"borderRadius":4,"flexDirection":"row","gap":1,"padding":3,"backgroundColor":s["bg"].as_str().unwrap_or("#000000")}), strip),
                    text(json!(format!("{}{}", row.name, if row.star { "  ★" } else { "" })), json!({"fontSize":13,"color":tok(th,"text","#ffffff")})),
                    text(json!(row.detail), json!({"fontSize":11,"color":dim})),
                ],
            ));
        }
        if !result.schemes.is_empty() {
            details.push(heading(f, json!({"key":"terminal.prefs.import.schemes","params":{"count":result.schemes.len()}}), 20.));
        }
        details.push(view(
            json!({"flexDirection":"row","flexWrap":"wrap","gap":8}),
            result.schemes.iter().take(12).enumerate().map(|(i, s)| scheme_card(f, &format!("imp-{i}"), s, false, json!(["noop"]))).collect(),
        ));
        if !result.warnings.is_empty() {
            details.push(heading(f, pf("import.warnings"), 20.));
        }
        details.extend(result.warnings.iter().map(|w| text(json!(format!("• {w}")), json!({"fontSize":12,"lineHeight":1.5,"textAlign":"start","color":dim}))));
        let mut buttons = vec![];
        if result.can_default {
            buttons.push(button(f, "do-default", pf("import.doDefault"), json!(["pfImportApply", true]), false, false));
        }
        buttons.push(button(f, "do", pf("import.do"), json!(["pfImportApply", false]), true, false));
        details.push(view(json!({"flexDirection":"row","flexWrap":"wrap","gap":8,"marginTop":12,"justifyContent":"flex-end"}), buttons));
    }
    let mut overlay = keyed(
        "import-panel",
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"backgroundColor":tok(th,"windowBg","#2a2a2e"),"padding":24,"gap":12}),
        vec![
            view(
                json!({"gap":4}),
                vec![
                    text(pf("import.title"), json!({"fontSize":15,"fontWeight":600,"textAlign":"start","color":tok(th,"text","#ffffff")})),
                    text(pf("import.intro"), json!({"fontSize":12,"lineHeight":1.4,"textAlign":"start","color":dim})),
                ],
            ),
            view(
                json!({"flexDirection":"row","gap":8,"alignItems":"center"}),
                vec![
                    json!({"kind":"input","key":"import-path","value":import.path,"placeholder":pf("import.placeholder"),"autoFocus":true,"change":["pfImportPath"],
                        "submit":["pfImportLoad"],"cancel":["pfImportClose"],"size":13,"style":{"flex":1,"height":24}}),
                    button(f, "cancel", pf("cancel"), json!(["pfImportClose"]), false, false),
                    button(f, "open", pf("import.open"), json!(["pfImportLoad"]), true, false),
                ],
            ),
            view(
                json!({"flex":1,"flexDirection":"row","gap":16}),
                vec![
                    keyed("found", json!({"width":220}), found),
                    json!({"kind":"scroll","key":"import-details","style":{"flex":1},"children":[view(json!({}), details)]}),
                ],
            ),
        ],
    );
    overlay["events"]["onMouseDown"] = json!(["noop"]);
    overlay
}
pub(super) fn tree(f: &Facts) -> Value {
    let th = f.theme;
    let mut children = vec![sidebar(f), main_pane(f)];
    if let Some(import) = &f.import {
        children.push(import_panel(f, import));
    }
    let mut root = keyed(
        "preferences",
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"flexDirection":"row","backgroundColor":tok(th,"windowBg","#2a2a2e")}),
        children,
    );
    root["events"]["onMouseDown"] = json!(["noop"]);
    root
}
