use super::{level, valid_match, App, Entry, ROW};
use serde_json::{json, Value};
fn v(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn click(mut node: Value, action: Value) -> Value {
    node["key"] = json!(action.to_string());
    node["events"]["onClick"] = action;
    node
}
fn line(edge: &str, t: &Value) -> Value {
    v(
        "line",
        json!({"position":"absolute","left":0,"right":0,edge:0,"height":1,"backgroundColor":t["separator"]}),
        vec![],
    )
}
fn dot(p: i64) -> Value {
    let color = match level(p) {
        "error" => "#ff453a",
        "warning" => "#ff9f0a",
        _ => "transparent",
    };
    v(
        "dot",
        json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":color,"borderWidth":if color=="transparent"{1}else{0},"borderColor":"rgba(128,128,128,0.5)"}),
        vec![],
    )
}
fn button(label: Value, action: Value, active: bool, t: &Value) -> Value {
    click(
        v(
            "",
            json!({"height":28,"paddingHorizontal":10,"borderRadius":6,"justifyContent":"center","backgroundColor":if active{t["accent"].clone()}else{json!("transparent")}}),
            vec![text(
                label,
                json!({"fontSize":13,"fontWeight":500,"color":if active{json!("#fff")}else{t["text"].clone()}}),
            )],
        ),
        action,
    )
}
fn segmented(cmd: &str, value: &str, options: &[(&str, &str)], t: &Value) -> Value {
    v(
        cmd,
        json!({"flexDirection":"row","padding":2,"backgroundColor":t["control"],"borderRadius":t["controlRadiusSmall"]}),
        options
            .iter()
            .map(|(id, label)| {
                let mut node = button(key(label), json!([cmd, id]), false, t);
                node["key"] = json!(id);
                node["style"]["height"] = json!(18);
                node["style"]["borderRadius"] = json!(4);
                node["style"]["backgroundColor"] = json!(if *id == value {
                    if t["mode"] == "dark" {
                        "rgba(255,255,255,0.18)"
                    } else {
                        "#ffffff"
                    }
                } else {
                    "transparent"
                });
                node["children"][0]["style"]["fontSize"] = json!(11);
                node
            })
            .collect(),
    )
}
pub(super) fn icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    let mut nodes=vec![v("lights",json!({"flexDirection":"row","gap":s*0.05,"marginBottom":s*0.02}),["#ff5f57","#febc2e","#28c840"].iter().map(|c|v(c,json!({"width":s*0.08,"height":s*0.08,"borderRadius":s*0.04,"backgroundColor":c}),vec![])).collect())];
    for (i, (c, w)) in [
        ("#ff453a", 0.42),
        ("#d0d0d6", 0.62),
        ("#ffd60a", 0.5),
        ("#d0d0d6", 0.68),
        ("#30d158", 0.36),
    ]
    .iter()
    .enumerate()
    {
        nodes.push(v(&i.to_string(),json!({"flexDirection":"row","gap":s*0.05,"alignItems":"center"}),vec![
            v("dot",json!({"width":s*0.06,"height":s*0.06,"borderRadius":s*0.03,"backgroundColor":c}),vec![]),
            v("line",json!({"width":s*w,"height":1_f64.max(s*0.045),"borderRadius":s*0.02,"backgroundColor":if i%2==1{"rgba(255,255,255,0.45)"}else{"rgba(255,255,255,0.7)"}}),vec![])
        ]));
    }
    v(
        "logs-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"backgroundColor":"#2b2d33","padding":s*0.16,"gap":s*0.075,"boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"direction":"ltr"}),
        nodes,
    )
}
fn empty(title: Value, hint: Value, t: &Value) -> Value {
    v(
        "empty",
        json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":8,"padding":20}),
        vec![
            text(
                title,
                json!({"fontSize":15,"fontWeight":600,"color":t["text"],"textAlign":"center"}),
            ),
            text(
                hint,
                json!({"fontSize":13,"color":t["textDim"],"textAlign":"center"}),
            ),
        ],
    )
}
fn row(e: &Entry, i: usize, on: bool, t: &Value) -> Value {
    let fg = if on {
        json!("#fff")
    } else if level(e.p) == "debug" {
        t["textDim"].clone()
    } else {
        t["text"].clone()
    };
    click(
        v(
            &format!("{}-{}-{}-{i}", e.t, e.s, e.line),
            json!({"height":ROW,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":12,"backgroundColor":if on{t["accent"].clone()}else if i%2==1{json!(if t["mode"]=="dark"{"rgba(255,255,255,0.03)"}else{"rgba(0,0,0,0.03)"})}else{json!("transparent")}}),
            vec![
                v(
                    "severity",
                    json!({"width":10,"alignItems":"center"}),
                    vec![dot(e.p)],
                ),
                text(
                    json!(if e.t > 0 {
                        App::clock(e.t)
                    } else {
                        format!("#{}", e.line)
                    }),
                    json!({"width":92,"fontSize":11,"fontFamily":"monospace","whiteSpace":"nowrap","direction":"ltr","color":if on{json!("#fff")}else{t["textDim"].clone()}}),
                ),
                text(
                    json!(if e.id.is_empty() { "—" } else { &e.id }),
                    json!({"width":140,"fontSize":12,"color":fg,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start"}),
                ),
                text(
                    json!(e.m.replace('\n', " ⏎ ")),
                    json!({"flex":1,"fontSize":12,"color":fg,"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start"}),
                ),
            ],
        ),
        json!(["select", i]),
    )
}
fn detail(app: &mut App) -> Value {
    let t = app.theme.clone();
    let Some(e) = app.selected.and_then(|i| app.entries.get(i)).cloned() else {
        return empty(Value::Null, key("logs.pick"), &t);
    };
    let when = if e.t > 0 {
        json!(format!("{} {}", app.date(e.t, true), App::clock(e.t)))
    } else {
        json!({"key":"logs.line","params":{"n":e.line}})
    };
    let severity = app.caption(&format!("logs.level.{}", level(e.p)));
    let fields = [
        ("time", when),
        (
            "level",
            json!(format!(
                "{severity}{}",
                if e.p >= 0 {
                    format!(" ({})", e.p)
                } else {
                    String::new()
                }
            )),
        ),
        ("process", json!(if e.id.is_empty() { "—" } else { &e.id })),
        (
            "pid",
            json!(if e.pid >= 0 {
                e.pid.to_string()
            } else {
                "—".into()
            }),
        ),
        ("unit", json!(if e.u.is_empty() { "—" } else { &e.u })),
        (
            "uid",
            json!(if e.uid >= 0 {
                e.uid.to_string()
            } else {
                "—".into()
            }),
        ),
        ("boot", json!(if e.b.is_empty() { "—" } else { &e.b })),
    ];
    json!({"kind":"scroll","key":"details","style":{"flex":1},"children":[v("entry",json!({"flexDirection":"row","gap":16,"paddingHorizontal":16,"paddingVertical":12}),vec![
        v("message",json!({"flex":1,"minWidth":0,"gap":4}),vec![
            v("process",json!({"flexDirection":"row","alignItems":"center","gap":8}),vec![dot(e.p),text(json!(e.id),json!({"fontSize":13,"fontWeight":600,"color":t["text"],"whiteSpace":"nowrap","overflow":"hidden"}))]),
            text(json!(e.m),json!({"fontSize":12,"fontFamily":"monospace","lineHeight":1.4,"color":t["text"],"textAlign":"start"}))
        ]),
        v("fields",json!({"width":if app.width-210.<640.{210}else{260},"gap":3}),fields.into_iter().map(|(k,value)|v(k,json!({"flexDirection":"row","gap":8}),vec![
            text(key(&format!("logs.field.{k}")),json!({"width":76,"fontSize":11,"color":t["textDim"],"textAlign":"end"})),
            text(value,json!({"flex":1,"fontSize":11,"color":t["text"],"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start"}))
        ])).collect())
    ])]})
}
pub(super) fn tree(app: &mut App) -> Value {
    let t = app.theme.clone();
    let narrow = app.width - 210. < 640.;
    let mut sources = vec![text(
        key("logs.section.journal"),
        json!({"fontSize":11,"fontWeight":600,"color":t["textDim"],"marginTop":12,"marginBottom":4,"paddingHorizontal":10}),
    )];
    for (id, glyph) in [("user", "☺"), ("system", "▣"), ("kernel", "◐")] {
        sources.push(click(v(id,json!({"height":28,"paddingHorizontal":10,"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":8,"backgroundColor":if app.source==id{t["accent"].clone()}else{json!("transparent")}}),vec![
        text(json!(glyph),json!({"width":18,"fontSize":14,"color":if app.source==id{json!("#fff")}else{t["accent"].clone()}})),text(key(&format!("logs.src.{id}")),json!({"fontSize":13,"color":if app.source==id{json!("#fff")}else{t["text"].clone()}}))
    ]),json!(["source",id])));
    }
    sources.push(text(key("logs.section.lordos"),json!({"fontSize":11,"fontWeight":600,"color":t["textDim"],"marginTop":12,"marginBottom":4,"paddingHorizontal":10})));
    if app.files.is_empty() {
        sources.push(text(
            key("logs.noFiles"),
            json!({"fontSize":11,"color":t["textDim"],"paddingHorizontal":10}),
        ));
    }
    for file in &app.files {
        if let Some(name) = file["name"].as_str() {
            sources.push(button(
                json!(format!("≡  {}", name.strip_suffix(".log").unwrap_or(name))),
                json!(["file", name]),
                app.source == "lordos" && app.file == name,
                &t,
            ));
        }
    }
    let hint = match app.source.as_str() {
        "user" => "user",
        "lordos" => "lordos",
        _ => {
            if app.system {
                "systemDirect"
            } else {
                "systemAdmin"
            }
        }
    };
    let sidebar = v(
        "sidebar",
        json!({"width":210,"flexShrink":0,"backgroundColor":t["sidebarBg"],"paddingHorizontal":10,"paddingTop":4,"gap":2}),
        vec![
            json!({"kind":"scroll","key":"sources","style":{"flex":1},"children":sources}),
            text(
                key(&format!("logs.hint.{hint}")),
                json!({"fontSize":11,"color":t["textDim"],"lineHeight":1.4,"paddingHorizontal":8,"paddingBottom":12}),
            ),
            v(
                "separator",
                json!({"position":"absolute","right":0,"top":0,"bottom":0,"width":1,"backgroundColor":t["separator"]}),
                vec![],
            ),
        ],
    );
    let toolbar = v(
        "toolbar",
        json!({"height":40,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":12}),
        vec![
            button(
                json!({"parts":["● ",key("logs.live")]}),
                json!(["live"]),
                app.live,
                &t,
            ),
            button(key("logs.export"), json!(["export"]), false, &t),
            v("space", json!({"flex":1}), vec![]),
            json!({"kind":"input","key":"process","value":app.process,"change":["process"],"placeholder":key("logs.process"),"style":{"width":if narrow{130}else{150},"borderWidth":if app.process.is_empty()||valid_match(&app.process){0}else{1},"borderColor":"#ff453a"}}),
            json!({"kind":"input","key":"search","value":app.query,"change":["search"],"placeholder":key("logs.search"),"style":{"width":if narrow{190}else{220}}}),
        ],
    );
    let filters = v(
        "filters",
        json!({"height":40,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":12}),
        vec![
            segmented(
                "level",
                &app.level,
                &[
                    ("all", "logs.lv.all"),
                    ("info", "logs.lv.info"),
                    ("warning", "logs.lv.warning"),
                    ("error", "logs.lv.error"),
                ],
                &t,
            ),
            segmented(
                "range",
                &app.range,
                &[
                    ("15m", "logs.range.15m"),
                    ("hour", "logs.range.hour"),
                    ("day", "logs.range.day"),
                    ("all", "logs.range.all"),
                ],
                &t,
            ),
            line("bottom", &t),
        ],
    );
    let headings = v(
        "headings",
        json!({"height":26,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":12}),
        vec![
            v("dot", json!({"width":10}), vec![]),
            text(
                key("logs.field.time"),
                json!({"width":92,"fontSize":11,"fontWeight":600,"color":t["textDim"]}),
            ),
            text(
                key("logs.field.process"),
                json!({"width":140,"fontSize":11,"fontWeight":600,"color":t["textDim"]}),
            ),
            text(
                key("logs.field.message"),
                json!({"flex":1,"fontSize":11,"fontWeight":600,"color":t["textDim"]}),
            ),
            line("bottom", &t),
        ],
    );
    let body = if !app.error.is_empty() {
        empty(key("logs.errorTitle"), json!(app.error), &t)
    } else if app.entries.is_empty() {
        empty(
            Value::Null,
            key(if app.loading {
                "logs.loading"
            } else {
                "logs.empty"
            }),
            &t,
        )
    } else {
        let first = ((app.offset / ROW).floor() as usize).saturating_sub(2);
        let last =
            (((app.offset + app.list_height()) / ROW).ceil() as usize + 2).min(app.entries.len());
        let rows = (first..last)
            .map(|i| row(&app.entries[i], i, app.selected == Some(i), &t))
            .collect();
        let mut children = vec![v(
            "rows",
            json!({"position":"absolute","left":0,"right":0,"top":first as f64*ROW-app.offset}),
            rows,
        )];
        let total = app.entries.len() as f64 * ROW;
        let h = app.list_height();
        if total > h {
            let bar = (h * h / total).max(28.).min(h);
            let mut thumb = v(
                "thumb",
                json!({"position":"absolute","right":2,"width":7,"height":bar,"borderRadius":4,"top":(h-bar)*app.offset/app.max_offset().max(1.),"backgroundColor":if t["mode"]=="dark"{"rgba(255,255,255,0.3)"}else{"rgba(0,0,0,0.28)"}}),
                vec![],
            );
            thumb["events"]["onMouseDown"] = json!(["dragScroll"]);
            children.push(thumb);
        }
        let mut list = v("entries", json!({"flex":1,"overflow":"hidden"}), children);
        list["events"]["onWheel"] = json!(["scroll"]);
        list
    };
    let detail = detail(app);
    let mut footer = vec![
        line("top", &t),
        text(
            json!({"key":"logs.count","params":{"count":app.entries.len(),"n":{"number":app.entries.len()}}}),
            json!({"fontSize":11,"color":t["textDim"]}),
        ),
    ];
    for (severity, key, color) in [
        ("error", "logs.errors", "#ff453a"),
        ("warning", "logs.warnings", "#ff9f0a"),
    ] {
        let n = app
            .entries
            .iter()
            .filter(|e| level(e.p) == severity)
            .count();
        if n > 0 {
            footer.push(text(
                json!({"key":key,"params":{"count":n,"n":{"number":n}}}),
                json!({"fontSize":11,"color":color}),
            ));
        }
    }
    if app.elapsed > 0 && !narrow {
        footer.push(text(
            json!({"key":"logs.took","params":{"ms":{"number":app.elapsed}}}),
            json!({"fontSize":11,"color":t["textDim"]}),
        ));
    }
    footer.push(v("space", json!({"flex":1}), vec![]));
    if !app.notice.is_null() {
        footer.push(text(
            app.notice.clone(),
            json!({"fontSize":11,"color":t["accent"],"whiteSpace":"nowrap","overflow":"hidden"}),
        ));
    } else if app.live {
        footer.push(v(
            "live",
            json!({"flexDirection":"row","alignItems":"center","gap":6}),
            vec![
                v(
                    "dot",
                    json!({"width":7,"height":7,"borderRadius":4,"backgroundColor":"#30d158"}),
                    vec![],
                ),
                text(
                    key("logs.following"),
                    json!({"fontSize":11,"color":t["textDim"]}),
                ),
            ],
        ));
    }
    v(
        "logs",
        json!({"flex":1,"flexDirection":"row"}),
        vec![
            sidebar,
            v(
                "main",
                json!({"flex":1,"minWidth":0}),
                vec![
                    toolbar,
                    filters,
                    headings,
                    body,
                    v(
                        "detail",
                        json!({"height":app.detail_height(),"flexShrink":0}),
                        vec![line("top", &t), detail],
                    ),
                    v(
                        "footer",
                        json!({"height":26,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":12}),
                        footer,
                    ),
                ],
            ),
        ],
    )
}
