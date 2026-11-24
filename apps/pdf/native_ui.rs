//! Scene nodes of the native PDF window. Pure functions of the window facts: the
//! host draws them and sends actions back as `cmd`/`key` requests.
use serde_json::{json, Value};

pub(super) const TOOLBAR: f64 = 44.;
pub(super) const STATUS: f64 = 26.;
pub(super) const SIDEBAR: f64 = 184.;
const ACCENT_FOREGROUND: &str = "#ffffff";
const ERROR_RED: &str = "#ff453a";

pub(super) struct Facts<'a> {
    pub theme: &'a Value,
    pub width: f64,
    pub has_doc: bool,
    pub opening: bool,
    pub need_pass: bool,
    pub password: &'a str,
    pub error: &'a Value,
    pub path: bool,
    pub pages: usize,
    pub page: usize,
    pub pct: f64,
    pub single: bool,
    pub invert: bool,
    pub side: Option<&'a str>,
    pub status: Value,
    pub frames: &'a [Value],
    pub images: &'a std::collections::HashMap<u64, i64>,
    pub scrollbar: Option<(f64, f64)>,
    pub overlays: &'a std::collections::HashMap<u64, Vec<Value>>,
    pub goto: Option<&'a str>,
    pub find_query: &'a str,
    pub find_focus: u64,
    pub status_more: bool,
    pub recent: Option<&'a [(String, String)]>,
    pub sidebar: Value,
    pub view_h: f64,
    pub view_w: f64,
}

fn tok<'a>(theme: &'a Value, key: &str, fallback: &'a str) -> &'a str {
    theme[key].as_str().unwrap_or(fallback)
}
pub(super) fn view(key: impl Into<Value>, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key.into(),"style":style,"children":children})
}
pub(super) fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
pub(super) fn click(mut node: Value, action: Value) -> Value {
    node["events"]["onClick"] = action;
    node
}
fn label(key: &str) -> Value {
    json!({"key":key})
}
fn caption(key: &str, params: Value) -> Value {
    json!({"key":key,"params":params})
}
fn tool(f: &Facts, key: &str, glyph: &str, action: Value, tip: &str, active: bool, disabled: bool) -> Value {
    let t = f.theme;
    let mut node = click(
        view(
            key,
            json!({"width":28,"height":28,"borderRadius":6,"alignItems":"center","justifyContent":"center","flexShrink":0,
                "opacity":if disabled {0.4} else {1.},
                "backgroundColor":if active {tok(t,"hover","rgba(127,127,127,0.18)")} else {"transparent"}}),
            vec![text(
                json!(glyph),
                json!({"fontSize":17,"color":if active {tok(t,"accent","#0a84ff")} else {tok(t,"text","#ffffff")}}),
            )],
        ),
        if disabled { json!(["noop"]) } else { action },
    );
    node["tooltip"] = label(tip);
    node["hoverStyle"] = json!({"backgroundColor":tok(t,"hover","rgba(127,127,127,0.12)")});
    node
}
fn toolbar(f: &Facts) -> Value {
    let t = f.theme;
    let narrow = f.width < 760.;
    let dim = tok(t, "textDim", "#999999");
    let page_label = if f.pages == 0 {
        text(json!(""), json!({"fontSize":12}))
    } else if f.width < 640. {
        text(json!(format!("{} / {}", f.page + 1, f.pages)), json!({"fontSize":12,"color":dim,"whiteSpace":"nowrap"}))
    } else {
        text(
            caption("pdf.pageOf", json!({"page":f.page + 1,"count":f.pages})),
            json!({"fontSize":12,"color":dim,"whiteSpace":"nowrap"}),
        )
    };
    let mut items = vec![
        tool(f, "sidebar", "◫", json!(["toggleSide"]), "pdf.tool.sidebar", f.side.is_some(), false),
        match f.goto {
            Some(value) => json!({"kind":"input","key":"goto","value":value,"placeholder":label("pdf.goPlaceholder"),"autoFocus":true,
                "change":["gotoChange"],"submit":["gotoSubmit"],"cancel":["gotoCancel"],"size":13,"style":{"width":96,"height":26}}),
            None => click(view("page-label", json!({"paddingHorizontal":4,"overflow":"hidden","flexShrink":1}), vec![page_label]), json!(["gotoOpen"])),
        },
        view("spacer", json!({"flex":1}), vec![]),
        view(
            "zoom",
            json!({"flexDirection":"row","alignItems":"center","direction":"ltr"}),
            vec![
                tool(f, "zoom-out", "−", json!(["zoom",-1]), "pdf.zoom.out", false, !f.has_doc),
                click(
                    view("zoom-pct", json!({"minWidth":48,"alignItems":"center","justifyContent":"center","height":28}), vec![text(
                        json!(if f.has_doc { format!("{} %", f.pct as i64) } else { "—".into() }),
                        json!({"fontSize":11,"fontWeight":500,"color":tok(t,"text","#ffffff")}),
                    )]),
                    json!(["zoomToggle"]),
                ),
                tool(f, "zoom-in", "+", json!(["zoom",1]), "pdf.zoom.in", false, !f.has_doc),
            ],
        ),
    ];
    if !narrow {
        items.push(tool(f, "rotate", "↻", json!(["rotate",1]), "pdf.rotateRight", false, !f.has_doc));
        items.push(tool(f, "invert", "◐", json!(["invert"]), "pdf.invert", f.invert, !f.has_doc));
    }
    let find_width = if f.width < 760. { 130 } else { 180 };
    items.push(if f.has_doc {
        json!({"kind":"input","key":format!("find-{}",f.find_focus),"value":f.find_query,"placeholder":label("pdf.find"),
            "autoFocus":f.find_focus > 0,"change":["findChange"],"submit":["findSubmit"],"size":13,"style":{"width":find_width,"height":26}})
    } else {
        view("find-space", json!({"width":find_width}), vec![])
    });
    items.push(view(
        "toolbar-rule",
        json!({"position":"absolute","left":0,"right":0,"bottom":0,"height":1,"backgroundColor":tok(t,"separator","#444444"),"pointerEvents":"none"}),
        vec![],
    ));
    view(
        "toolbar",
        json!({"height":TOOLBAR,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":6,"paddingHorizontal":12}),
        items,
    )
}
fn welcome(f: &Facts) -> Value {
    let t = f.theme;
    let dark = t["mode"] == "dark";
    let dim = tok(t, "textDim", "#999999");
    let mut column = vec![];
    match f.recent {
        None => column.push(text(label("pdf.searching"), json!({"fontSize":12,"color":dim,"textAlign":"center"}))),
        Some([]) => column.push(text(label("pdf.noneFound"), json!({"fontSize":12,"color":dim,"textAlign":"center"}))),
        Some(list) => {
            column.push(text(label("pdf.recent"), json!({"fontSize":11,"fontWeight":600,"color":dim,"marginLeft":8})));
            let rows = list
                .iter()
                .take(8)
                .enumerate()
                .map(|(i, (name, path))| {
                    let mut children = vec![];
                    if i > 0 {
                        children.push(view("rule", json!({"position":"absolute","left":12,"right":0,"top":0,"height":1,"backgroundColor":tok(t,"separator","#444444")}), vec![]));
                    }
                    children.push(text(json!("PDF"), json!({"fontSize":10,"color":ERROR_RED,"fontWeight":700})));
                    children.push(text(json!(name), json!({"flex":1,"fontSize":13,"color":tok(t,"text","#ffffff"),"whiteSpace":"nowrap","overflow":"hidden"})));
                    let mut row = click(
                        view(json!(path), json!({"height":32,"flexDirection":"row","gap":10,"paddingHorizontal":12,"alignItems":"center"}), children),
                        json!(["open", path]),
                    );
                    row["hoverStyle"] = json!({"backgroundColor":tok(t,"hover","rgba(127,127,127,0.12)")});
                    row
                })
                .collect();
            column.push(view(
                "recent",
                json!({"borderRadius":10,"overflow":"hidden","borderWidth":1,"borderColor":tok(t,"separator","#444444"),
                    "backgroundColor":if dark {"rgba(255,255,255,0.04)"} else {"rgba(255,255,255,0.7)"}}),
                rows,
            ));
        }
    }
    view(
        "welcome",
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"alignItems":"center","justifyContent":"center","gap":8}),
        vec![
            text(label("pdf.welcome"), json!({"fontSize":18,"fontWeight":600,"color":tok(t,"text","#ffffff")})),
            text(label("pdf.welcomeHint"), json!({"fontSize":12,"color":dim,"marginBottom":8})),
            view("recent-column", json!({"width":420,"maxHeight":300,"gap":6}), column),
        ],
    )
}
fn password(f: &Facts) -> Value {
    let t = f.theme;
    let mut children = vec![
        text(label("pdf.passwordTitle"), json!({"fontSize":15,"fontWeight":600,"color":tok(t,"text","#ffffff")})),
        json!({"kind":"input","key":"password","value":f.password,"placeholder":label("pdf.password"),"secure":true,"autoFocus":true,
            "change":["passwordChange"],"submit":["password"],"size":13,"style":{"height":28}}),
    ];
    if f.error.as_object().is_some_and(|m| !m.is_empty()) {
        children.push(text(f.error.clone(), json!({"fontSize":11,"color":ERROR_RED})));
    }
    view(
        "password",
        json!({"position":"absolute","left":(f.view_w/2.-160.).max(0.),"top":(f.view_h/2.-70.).max(0.),"width":320,"padding":20,"gap":12,
            "borderRadius":12,"backgroundColor":tok(t,"windowBg","#2a2a2e"),"boxShadow":"0 12px 30px rgba(0,0,0,0.4)"}),
        children,
    )
}
fn failure(f: &Facts) -> Value {
    let t = f.theme;
    view(
        "failure",
        json!({"position":"absolute","left":0,"right":0,"top":(f.view_h/2.-40.).max(0.),"alignItems":"center","gap":10}),
        vec![
            text(f.error.clone(), json!({"fontSize":13,"color":ERROR_RED,"textAlign":"center"})),
            click(
                view(
                    "reopen",
                    json!({"height":28,"paddingHorizontal":14,"borderRadius":6,"alignItems":"center","justifyContent":"center",
                        "backgroundColor":tok(t,"accent","#0a84ff")}),
                    vec![text(label("pdf.reopen"), json!({"fontSize":13,"color":ACCENT_FOREGROUND}))],
                ),
                json!(["reopen"]),
            ),
        ],
    )
}
fn pages(f: &Facts) -> Vec<Value> {
    f.frames
        .iter()
        .map(|frame| {
            let page = frame["page"].as_u64().unwrap_or(0);
            let (w, h) = (frame["width"].as_f64().unwrap_or(0.), frame["height"].as_f64().unwrap_or(0.));
            let mut children = vec![];
            if let Some(image) = f.images.get(&page).copied() {
                children.push(json!({"kind":"documentImage","key":"image","image":image,
                    "style":{"position":"absolute","left":0,"top":0,"width":w,"height":h,"objectFit":"fill"}}));
            }
            children.extend(f.overlays.get(&page).cloned().unwrap_or_default());
            view(
                json!(format!("page-{page}")),
                json!({"position":"absolute","left":frame["left"],"top":frame["top"],"width":w,"height":h,
                    "backgroundColor":if f.invert {"#1b1b1f"} else {"#ffffff"},"boxShadow":"0 2px 10px rgba(0,0,0,0.35)","overflow":"hidden"}),
                children,
            )
        })
        .collect()
}
fn status(f: &Facts) -> Value {
    let t = f.theme;
    let dim = tok(t, "textDim", "#999999");
    view(
        "status",
        json!({"height":STATUS,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":12,
            "backgroundColor":if t["mode"] == "dark" {"rgba(0,0,0,0.12)"} else {"rgba(0,0,0,0.03)"}}),
        vec![
            view("status-rule", json!({"position":"absolute","left":0,"right":0,"top":0,"height":1,"backgroundColor":tok(t,"separator","#444444")}), vec![]),
            view("status-text", json!({"flex":1,"flexDirection":"row","overflow":"hidden"}), {
                let mut parts = vec![text(f.status.clone(), json!({"fontSize":11,"color":dim,"whiteSpace":"nowrap"}))];
                if f.status_more {
                    parts.push(text(json!("…"), json!({"fontSize":11,"color":dim})));
                }
                parts
            }),
            text(label(if f.single { "pdf.view.single" } else { "pdf.view.continuous" }), json!({"fontSize":11,"color":dim})),
        ],
    )
}
pub(super) fn tree(f: &Facts) -> Value {
    let t = f.theme;
    let dark = t["mode"] == "dark";
    let mut content = pages(f);
    if !f.path {
        content.push(welcome(f));
    } else if f.need_pass {
        content.push(password(f));
    } else if f.error.as_object().is_some_and(|e| !e.is_empty()) {
        content.push(failure(f));
    } else if !f.has_doc || f.opening {
        content.push(text(label("pdf.opening"), json!({"margin":40,"fontSize":13,"color":tok(t,"textDim","#999999"),"textAlign":"center"})));
    }
    if let Some((top, height)) = f.scrollbar {
        content.push(view(
            "scrollbar",
            json!({"position":"absolute","right":3,"width":6,"borderRadius":3,"top":top,"height":height,
                "backgroundColor":if dark {"rgba(255,255,255,0.3)"} else {"rgba(0,0,0,0.3)"},"pointerEvents":"none"}),
            vec![],
        ));
    }
    let mut area = view(
        "area",
        json!({"flex":1,"overflow":"hidden","direction":"ltr","backgroundColor":if dark {"#2a2a2e"} else {"#d9d9de"}}),
        content,
    );
    area["events"]["onWheel"] = json!(["wheel"]);
    area["events"]["onMouseDown"] = json!(["down"]);
    let mut body = vec![];
    if f.side.is_some() && f.has_doc {
        body.push(view("sidebar", json!({"width":SIDEBAR,"flexShrink":0}), vec![f.sidebar.clone()]));
    }
    body.push(area);
    view(
        "pdf",
        json!({"flex":1,"backgroundColor":tok(t,"windowBg","#1e1e22")}),
        vec![toolbar(f), view("body", json!({"flex":1,"flexDirection":"row"}), body), status(f)],
    )
}
/// The app icon (Dock, launcher): two sheets over a red tile, one line highlighted.
pub(super) fn icon(size: f64) -> Value {
    let s = if size.is_finite() { size.clamp(1., 512.) } else { 32. };
    let rule = |y: f64, i: usize| {
        view(
            json!(format!("rule-{i}")),
            json!({"position":"absolute","left":s*0.28,"top":s*y,"width":s*if i == 4 {0.22} else {0.36},"height":(s*0.04).max(1.),
                "borderRadius":s*0.02,"backgroundColor":if i == 2 {"#ffd60a"} else {"#d4d4dc"}}),
            vec![],
        )
    };
    let mut children = vec![
        view("back", json!({"position":"absolute","left":s*0.3,"top":s*0.12,"width":s*0.5,"height":s*0.64,"borderRadius":s*0.04,"backgroundColor":"rgba(255,255,255,0.55)"}), vec![]),
        view("front", json!({"position":"absolute","left":s*0.2,"top":s*0.2,"width":s*0.52,"height":s*0.66,"borderRadius":s*0.045,"backgroundColor":"#ffffff",
            "boxShadow":format!("0 {}px {}px rgba(0,0,0,0.3)",s*0.02,s*0.06)}), vec![]),
    ];
    children.extend([0.32, 0.42, 0.52, 0.62, 0.72].iter().enumerate().map(|(i, y)| rule(*y, i)));
    children.push(text(json!("PDF"), json!({"position":"absolute","left":s*0.48,"top":s*0.66,"fontSize":s*0.13,"fontWeight":700,"color":"#c8102e"})));
    view(
        "pdf-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #ff7a59, #c8102e)",
            "boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden"}),
        children,
    )
}
