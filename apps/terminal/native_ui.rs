//! Scene nodes of the native Terminal window: tab bar, panes, dividers and status bar.
//! Pure functions of the window facts; the host draws them and sends actions back as
//! `cmd`/`key` requests. Pane terminals are `terminal` nodes bound to their session.
use crate::workspace::{DividerRect, Rect};
use serde_json::{json, Value};

pub(super) const TAB_BAR: f64 = 36.;
pub(super) const STATUS_BAR: f64 = 24.;

pub(super) struct TabFacts {
    pub id: String,
    pub title: String,
    pub active: bool,
    pub bell: bool,
    pub activity: bool,
    pub failed: bool,
    pub color: Option<String>,
}
pub(super) struct PaneFacts {
    pub sid: i64,
    pub rect: Rect,
    pub dim: bool,
    pub badge: String,
}
pub(super) struct StatusFacts {
    pub cwd: String,
    pub program: String,
    pub busy: bool,
    pub last_exit: i64,
    pub history: i64,
    pub alt_screen: bool,
    pub size: (i64, i64),
    pub clock: String,
}
pub(super) struct FindFacts {
    pub query: String,
    pub cs: bool,
    pub regex: bool,
    pub count: i64,
    pub index: i64,
    pub error: bool,
}
pub(super) struct Facts<'a> {
    pub find: Option<FindFacts>,
    pub find_focus: u64,
    pub dialog: Option<(Value, Value)>,
    pub toasts: Vec<Value>,
    pub theme: &'a Value,
    pub pane_bg: String,
    pub width: f64,
    pub tabs: Vec<TabFacts>,
    pub broadcast: bool,
    pub panes: Vec<PaneFacts>,
    pub dividers: &'a [DividerRect],
    pub status: Option<StatusFacts>,
}

fn tok<'a>(theme: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    theme[key].as_str().unwrap_or(fallback)
}
fn view(key: impl Into<Value>, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key.into(),"style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn click(mut node: Value, name: &str, action: Value) -> Value {
    node["events"][name] = action;
    node
}
/// Long titles keep their end: `…tail`.
fn shorten(title: &str, max: usize) -> String {
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= max {
        return title.to_owned();
    }
    format!("…{}", chars[chars.len() - max.saturating_sub(1)..].iter().collect::<String>())
}
fn tab(f: &Facts, t: &TabFacts, index: usize, max_chars: usize) -> Value {
    let th = f.theme;
    let dark = th["mode"] == "dark";
    let mut children = vec![click(
        view(
            "close",
            json!({"width":16,"height":16,"borderRadius":4,"alignItems":"center","justifyContent":"center"}),
            vec![text(json!("✕"), json!({"fontSize":13,"color":tok(th,"textDim","#999999")}))],
        ),
        "onClick",
        json!(["closeTab", t.id]),
    )];
    if t.bell {
        children.push(text(json!("●"), json!({"fontSize":11,"color":"#ffd60a"})));
    } else if t.activity && !t.active {
        children.push(text(json!("●"), json!({"fontSize":9,"color":tok(th,"accent","#0a84ff")})));
    }
    if t.failed && !t.bell {
        children.push(text(json!("●"), json!({"fontSize":10,"color":"#ff453a"})));
    }
    children.push(view(
        "title",
        json!({"flex":1,"overflow":"hidden"}),
        vec![text(
            json!(shorten(&t.title, max_chars)),
            json!({"fontSize":12,"fontWeight":if t.active {600} else {500},"whiteSpace":"nowrap","textAlign":"center",
                "color":if t.active {tok(th,"text","#ffffff")} else {tok(th,"textDim","#999999")}}),
        )],
    ));
    children.push(text(
        json!(if index < 9 { format!("❖{}", index + 1) } else { String::new() }),
        json!({"fontSize":11,"color":tok(th,"textDim","#999999"),"opacity":if t.active {1.} else {0.7}}),
    ));
    if let Some(color) = &t.color {
        children.push(view("color", json!({"position":"absolute","left":8,"right":8,"bottom":1,"height":3,"borderRadius":2,"backgroundColor":color}), vec![]));
    }
    let mut node = click(
        view(
            json!(t.id),
            json!({"flex":1,"maxWidth":260,"minWidth":90,"height":26,"flexDirection":"row","alignItems":"center","paddingHorizontal":8,"gap":6,
                "borderRadius":6,"backgroundColor":if t.active {if dark {"rgba(255,255,255,0.12)"} else {"#ffffff"}} else {"transparent"},
                "boxShadow":if t.active && !dark {"0 1px 2px rgba(0,0,0,0.12)"} else {"none"}}),
            children,
        ),
        "onMouseDown",
        json!(["tab", t.id]),
    );
    node["hoverStyle"] = json!({"backgroundColor":tok(th,"hover","rgba(127,127,127,0.12)")});
    node
}
fn tab_bar(f: &Facts) -> Value {
    let th = f.theme;
    let tab_width = (260f64).min((f.width - 50.) / f.tabs.len().max(1) as f64);
    let max_chars = (((tab_width - 64.) / 6.6).floor() as i64).max(6) as usize;
    let mut children: Vec<Value> = f.tabs.iter().enumerate().map(|(i, t)| tab(f, t, i, max_chars)).collect();
    let mut plus = click(
        view(
            "new-tab",
            json!({"width":26,"height":26,"borderRadius":6,"alignItems":"center","justifyContent":"center","flexShrink":0}),
            vec![text(json!("+"), json!({"fontSize":17,"color":tok(th,"textDim","#999999")}))],
        ),
        "onClick",
        json!(["newTab"]),
    );
    plus["tooltip"] = json!({"key":"terminal.action.newTab"});
    children.push(plus);
    if f.broadcast {
        children.push(view(
            "broadcast",
            json!({"paddingHorizontal":8,"height":22,"borderRadius":5,"justifyContent":"center","backgroundColor":"#ff453a"}),
            vec![text(json!({"key":"terminal.broadcasting"}), json!({"fontSize":11,"fontWeight":600,"color":"#ffffff"}))],
        ));
    }
    children.push(view(
        "rule",
        json!({"position":"absolute","left":0,"right":0,"bottom":0,"height":1,"backgroundColor":tok(th,"separator","#444444"),"pointerEvents":"none"}),
        vec![],
    ));
    view(
        "tabs",
        json!({"height":TAB_BAR,"flexShrink":0,"flexDirection":"row","alignItems":"center","paddingHorizontal":8,"gap":4,
            "backgroundColor":tok(th,"sidebarBg","#2a2a2e")}),
        children,
    )
}
fn pane(f: &Facts, p: &PaneFacts) -> Value {
    let (w, h) = (p.rect.w.max(0.), p.rect.h.max(0.));
    let mut children = vec![json!({"kind":"terminal","key":"terminal","session":p.sid,
        "style":{"position":"absolute","left":0,"top":0,"width":w,"height":h}})];
    if !p.badge.is_empty() {
        children.push(text(
            json!(p.badge),
            json!({"position":"absolute","right":18,"top":10,"fontSize":30,"fontWeight":700,"color":"rgba(255,255,255,0.13)",
                "pointerEvents":"none","whiteSpace":"nowrap"}),
        ));
    }
    if p.dim {
        children.push(view(
            "dim",
            json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"backgroundColor":"rgba(0,0,0,0.28)","pointerEvents":"none"}),
            vec![],
        ));
    }
    let _ = f;
    let mut node = view(
        json!(format!("pane-{}", p.sid)),
        json!({"position":"absolute","left":p.rect.x,"top":p.rect.y,"width":w,"height":h,"overflow":"hidden"}),
        children,
    );
    node["events"] = json!({"onMouseDown":["paneDown",p.sid],"onMouseMove":["paneMove",p.sid],"onWheel":["paneWheel",p.sid],
        "onContextMenu":["paneMenu",p.sid]});
    node
}
fn divider(f: &Facts, d: &DividerRect) -> Value {
    let row = d.dir == crate::workspace::Dir::Row;
    let line = if row {
        json!({"position":"absolute","left":2,"top":0,"bottom":0,"width":1,"backgroundColor":tok(f.theme,"separator","#444444")})
    } else {
        json!({"position":"absolute","top":2,"left":0,"right":0,"height":1,"backgroundColor":tok(f.theme,"separator","#444444")})
    };
    let mut node = view(
        json!(format!("divider-{}", d.split)),
        json!({"position":"absolute","left":d.rect.x,"top":d.rect.y,"width":d.rect.w,"height":d.rect.h,"backgroundColor":f.pane_bg}),
        vec![view("line", line, vec![])],
    );
    node["events"]["onMouseDown"] = json!(["divider", d.split]);
    node["hoverStyle"] = json!({"backgroundColor":tok(f.theme,"hover","rgba(127,127,127,0.25)")});
    node
}
fn status_bar(f: &Facts, s: &StatusFacts) -> Value {
    let th = f.theme;
    let dim = tok(th, "textDim", "#999999");
    let item = |label: Value, color: Option<&str>| text(label, json!({"fontSize":12,"color":color.unwrap_or(dim),"whiteSpace":"nowrap"}));
    let mut children = vec![
        view("rule", json!({"position":"absolute","left":0,"right":0,"top":0,"height":1,"backgroundColor":tok(th,"separator","#444444")}), vec![]),
        item(json!(if s.cwd.is_empty() { "~".to_owned() } else { s.cwd.clone() }), None),
        item(
            json!(if s.busy { format!("▶ {}", s.program) } else if s.program.is_empty() { "shell".to_owned() } else { s.program.clone() }),
            s.busy.then(|| tok(th, "accent", "#0a84ff")),
        ),
    ];
    if s.last_exit > 0 {
        children.push(item(json!({"key":"terminal.status.lastExit","params":{"code":s.last_exit}}), Some("#ff453a")));
    }
    children.push(view("fill", json!({"flex":1}), vec![]));
    if s.history > 0 {
        children.push(item(json!({"key":"terminal.status.history","params":{"n":s.history}}), None));
    }
    if s.alt_screen {
        children.push(item(json!({"key":"terminal.status.altScreen"}), None));
    }
    children.push(item(json!(format!("{}×{}", s.size.0, s.size.1)), None));
    children.push(item(json!(s.clock), None));
    view(
        "status",
        json!({"height":STATUS_BAR,"flexShrink":0,"flexDirection":"row","alignItems":"center","paddingHorizontal":12,"gap":16,
            "backgroundColor":tok(th,"sidebarBg","#2a2a2e")}),
        children,
    )
}
fn small_button(f: &Facts, key: &str, label: &str, action: Value, on: bool) -> Value {
    let th = f.theme;
    click(
        view(
            key,
            json!({"minWidth":26,"height":22,"paddingHorizontal":4,"borderRadius":5,"alignItems":"center","justifyContent":"center",
                "backgroundColor":if on {tok(th,"hover","rgba(127,127,127,0.25)")} else {"transparent"}}),
            vec![text(json!(label), json!({"fontSize":12,"color":if on {tok(th,"accent","#0a84ff")} else {tok(th,"text","#ffffff")}}))],
        ),
        "onClick",
        action,
    )
}
fn find_bar(f: &Facts, find: &FindFacts) -> Value {
    let th = f.theme;
    let label = if find.error {
        json!({"key":"terminal.find.badRegex"})
    } else if find.count > 0 {
        json!({"key":"terminal.find.position","params":{"i":find.count - find.index,"n":find.count}})
    } else if !find.query.is_empty() {
        json!({"key":"terminal.find.none"})
    } else {
        json!("")
    };
    view(
        "find",
        json!({"position":"absolute","right":12,"top":8,"flexDirection":"row","alignItems":"center","gap":4,"padding":6,"borderRadius":10,
            "backgroundColor":tok(th,"windowBg","#2a2a2e"),"borderWidth":1,"borderColor":tok(th,"separator","#444444"),
            "boxShadow":"0 6px 18px rgba(0,0,0,0.35)"}),
        vec![
            json!({"kind":"input","key":format!("find-{}",f.find_focus),"value":find.query,"placeholder":{"key":"terminal.find.placeholder"},
                "autoFocus":true,"change":["findChange"],"cancel":["findClose"],"keys":["Enter","ArrowUp","ArrowDown"],"onKey":["findKey"],
                "size":13,"style":{"width":200,"height":26}}),
            text(label, json!({"width":88,"fontSize":11,"textAlign":"center","color":tok(th,"textDim","#999999")})),
            small_button(f, "up", "↑", json!(["findMove",-1]), false),
            small_button(f, "down", "↓", json!(["findMove",1]), false),
            small_button(f, "case", "Aa", json!(["findCs"]), find.cs),
            small_button(f, "regex", ".*", json!(["findRegex"]), find.regex),
            small_button(f, "close", "✕", json!(["findClose"]), false),
        ],
    )
}
fn dialog(f: &Facts, title: &Value, detail: &Value) -> Value {
    let th = f.theme;
    let button = |key: &str, label: &str, action: &str, primary: bool| {
        click(
            view(
                key,
                json!({"height":28,"paddingHorizontal":14,"borderRadius":6,"alignItems":"center","justifyContent":"center",
                    "backgroundColor":if primary {tok(th,"accent","#0a84ff")} else {tok(th,"control","#3a3a3e")}}),
                vec![text(json!({"key":label}), json!({"fontSize":13,"color":if primary {"#ffffff"} else {tok(th,"text","#ffffff")}}))],
            ),
            "onClick",
            json!([action]),
        )
    };
    let mut overlay = view(
        "dialog",
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"alignItems":"center","justifyContent":"center","backgroundColor":"rgba(0,0,0,0.35)"}),
        vec![view(
            "card",
            json!({"width":340,"padding":20,"borderRadius":12,"backgroundColor":tok(th,"windowBg","#2a2a2e"),"borderWidth":1,
                "borderColor":tok(th,"separator","#444444"),"boxShadow":"0 20px 50px rgba(0,0,0,0.5)"}),
            vec![
                text(title.clone(), json!({"fontSize":15,"fontWeight":600,"color":tok(th,"text","#ffffff")})),
                text(detail.clone(), json!({"fontSize":13,"marginTop":6,"color":tok(th,"textDim","#999999")})),
                view(
                    "buttons",
                    json!({"flexDirection":"row","justifyContent":"flex-end","gap":8,"marginTop":20}),
                    vec![button("cancel", "terminal.dialog.cancel", "dialogCancel", false), button("confirm", "terminal.dialog.close", "dialogConfirm", true)],
                ),
            ],
        )],
    );
    overlay["events"]["onMouseDown"] = json!(["noop"]);
    overlay
}
fn toasts(f: &Facts) -> Value {
    let th = f.theme;
    view(
        "toasts",
        json!({"position":"absolute","right":14,"bottom":10,"gap":6,"alignItems":"flex-end","pointerEvents":"none"}),
        f.toasts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                view(
                    json!(format!("toast-{i}")),
                    json!({"maxWidth":340,"paddingHorizontal":12,"paddingVertical":8,"borderRadius":9,"backgroundColor":tok(th,"windowBg","#2a2a2e"),
                        "borderWidth":1,"borderColor":tok(th,"separator","#444444"),"boxShadow":"0 8px 22px rgba(0,0,0,0.4)"}),
                    vec![text(t.clone(), json!({"fontSize":12,"color":tok(th,"text","#ffffff")}))],
                )
            })
            .collect(),
    )
}
pub(super) fn tree(f: &Facts) -> Value {
    let mut area: Vec<Value> = f.panes.iter().map(|p| pane(f, p)).collect();
    area.extend(f.dividers.iter().map(|d| divider(f, d)));
    if let Some(find) = &f.find {
        area.push(find_bar(f, find));
    }
    if !f.toasts.is_empty() {
        area.push(toasts(f));
    }
    let mut children = vec![
        tab_bar(f),
        view("panes", json!({"flex":1,"overflow":"hidden","direction":"ltr","backgroundColor":f.pane_bg}), area),
    ];
    if let Some(status) = &f.status {
        children.push(status_bar(f, status));
    }
    if let Some((title, detail)) = &f.dialog {
        children.push(dialog(f, title, detail));
    }
    view("terminal", json!({"flex":1,"backgroundColor":f.pane_bg}), children)
}
/// The app icon (Dock, launcher): a dark tile with the prompt.
pub(super) fn icon(size: f64) -> Value {
    let s = if size.is_finite() { size.clamp(1., 512.) } else { 32. };
    view(
        "terminal-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(180deg, #2c2c31, #111113)",
            "borderWidth":1,"borderColor":"rgba(255,255,255,0.15)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12)}),
        vec![text(
            json!(">_"),
            json!({"position":"absolute","left":s*0.17,"top":s*0.16,"fontSize":s*0.32,"fontWeight":700,"color":"#4ade80"}),
        )],
    )
}
