//! The original Reminders layout, including its local popup and sheet widgets.
use super::*;
fn v(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
fn label(k: &str) -> Value {
    json!({"key":format!("reminders.{k}")})
}
fn text(s: Value, size: f64, color: &str) -> Value {
    json!({"kind":"text","label":s,"style":{"fontSize":size,"color":color}})
}
fn styled_text(s: Value, style: Value) -> Value {
    json!({"kind":"text","label":s,"style":style})
}
fn click(mut node: Value, action: Value) -> Value {
    node["events"]["onClick"] = action;
    node
}
fn token(a: &App, k: &str, fallback: &str) -> String {
    a.theme[k].as_str().unwrap_or(fallback).into()
}
fn radius(a: &App, small: bool) -> Value {
    a.theme[if small {
        "controlRadiusSmall"
    } else {
        "controlRadius"
    }]
    .as_f64()
    .map_or(json!(if small { 6 } else { 7 }), |n| json!(n))
}
fn spacer() -> Value {
    v("spacer", json!({"flex":1}), vec![])
}
fn rule(a: &App, edge: &str, inset: f64) -> Value {
    let mut s = json!({"position":"absolute","backgroundColor":token(a,"separator","#555555"),"pointerEvents":"none"});
    if edge == "top" || edge == "bottom" {
        s["left"] = json!(inset);
        s["right"] = json!(inset);
        s["height"] = json!(1);
    } else {
        s["top"] = json!(inset);
        s["bottom"] = json!(inset);
        s["width"] = json!(1);
    }
    s[edge] = json!(0);
    v("separator", s, vec![])
}
fn button(
    a: &App,
    k: &str,
    action: Value,
    small: bool,
    primary: bool,
    destructive: bool,
    disabled: bool,
) -> Value {
    let fg = if primary {
        "#ffffff".into()
    } else if destructive {
        "#ff453a".into()
    } else {
        token(a, "text", "#ffffff")
    };
    let bg = token(a, if primary { "accent" } else { "control" }, "#3a3a3f");
    let border = if a.theme["buttonBorder"] == true {
        json!({"borderWidth":1,"borderColor":if primary{token(a,"accent","#007aff")}else{token(a,"separator","#555555")}})
    } else {
        json!({})
    };
    let mut node = v(
        k,
        json!([{"height":if small{22}else{28},"paddingHorizontal":if small{10}else{14},"borderRadius":radius(a,small),"flexDirection":"row","gap":6,"justifyContent":"center","alignItems":"center","backgroundColor":bg,"opacity":if disabled{0.4}else{1.}},border]),
        vec![styled_text(
            label(k),
            json!({"fontSize":if small{12}else{13},"fontWeight":500,"color":fg,"whiteSpace":"nowrap"}),
        )],
    );
    if !disabled {
        node = click(node, action);
        node["hoverStyle"] = json!({"opacity":0.85});
    }
    node
}
fn toolbar_button(
    a: &App,
    k: &str,
    glyph: Option<&str>,
    action: Value,
    small: bool,
    active: bool,
    disabled: bool,
) -> Value {
    let color = token(
        a,
        if active {
            "accent"
        } else if disabled {
            "textDim"
        } else {
            "text"
        },
        "#ffffff",
    );
    let mut children = vec![];
    if let Some(g) = glyph {
        children.push(text(json!(g), 20., &color));
    } else {
        children.push(styled_text(label(k),json!({"fontSize":if small{12}else{13},"fontWeight":500,"color":color,"whiteSpace":"nowrap"})));
    }
    let h = if small { 22 } else { 28 };
    let bg = if active {
        if a.theme["mode"] == "dark" {
            "rgba(255,255,255,0.12)"
        } else {
            "rgba(0,0,0,0.08)"
        }
    } else {
        "transparent"
    };
    let mut node = v(
        k,
        json!({"height":h,"minWidth":h,"paddingHorizontal":if glyph.is_some(){0}else{8},"borderRadius":radius(a,true),"flexDirection":"row","gap":6,"alignItems":"center","justifyContent":"center","opacity":if disabled{0.45}else{1.},"backgroundColor":bg}),
        children,
    );
    if glyph.is_some() {
        node["tooltip"] = label(k);
    }
    if !disabled {
        node = click(node, action);
        node["hoverStyle"] = json!({"backgroundColor":if active{bg.into()}else{token(a,"hover","rgba(127,127,127,0.12)")}});
    }
    node
}
fn input(
    key: &str,
    value: &str,
    placeholder: Value,
    change: Value,
    submit: Value,
    cancel: Value,
    auto: bool,
    style: Value,
) -> Value {
    json!({"kind":"input","key":key,"value":value,"placeholder":placeholder,"change":change,"submit":submit,"cancel":cancel,"autoFocus":auto,"keepFocus":key=="new-title","size":style["fontSize"].as_f64().unwrap_or(13.),"style":style})
}
pub(super) fn icon(s: f64) -> Value {
    let s = if s.is_finite() {
        s.clamp(1., 512.)
    } else {
        32.
    };
    let children=["#007aff","#ff3b30","#ff9500"].iter().enumerate().map(|(i,c)|v(&format!("line-{i}"),json!({"flexDirection":"row","alignItems":"center","gap":s*0.08}),vec![v("dot",json!({"width":s*0.13,"height":s*0.13,"borderRadius":s*0.065,"backgroundColor":c}),vec![]),v("line",json!({"width":s*if i==2{0.3}else{0.42},"height":(s*0.035).max(1.),"borderRadius":s*0.02,"backgroundColor":"#c7c7cc"}),vec![])])).collect();
    v(
        "reminders-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"backgroundColor":"#ffffff","border":"1px solid rgba(0,0,0,0.08)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"paddingHorizontal":s*0.17,"justifyContent":"center","gap":s*0.09}),
        children,
    )
}
fn tile(a: &App, k: &str, color: &str, count: Value, now: i64) -> Value {
    let selected = a.sel == k;
    let dark = a.theme["mode"] == "dark";
    let glyph = match k {
        "today" => crate::clock::local(now).tm_mday.to_string(),
        "scheduled" => "▦".into(),
        "all" => "☰".into(),
        "flagged" => "⚑".into(),
        _ => "✓".into(),
    };
    let mut top = vec![
        v(
            "glyph",
            json!({"width":26,"height":26,"borderRadius":13,"alignItems":"center","justifyContent":"center","backgroundColor":if selected{"#ffffff"}else{color}}),
            vec![styled_text(
                json!(glyph),
                json!({"fontSize":12,"fontWeight":600,"color":if selected{color}else{"#ffffff"}}),
            )],
        ),
        spacer(),
    ];
    if k != "completed" {
        top.push(styled_text(json!(count.as_u64().unwrap_or(0).to_string()),json!({"fontSize":20,"fontWeight":700,"color":if selected{"#ffffff".into()}else{token(a,"text","#ffffff")}})));
    }
    click(
        v(
            k,
            json!({"width":104,"height":64,"borderRadius":10,"paddingHorizontal":9,"paddingTop":8,"paddingBottom":7,"justifyContent":"space-between","backgroundColor":if selected{color}else if dark{"rgba(255,255,255,0.07)"}else{"#ffffff"},"border":format!("1px solid {}",if selected{"transparent"}else if dark{"rgba(255,255,255,0.06)"}else{"rgba(0,0,0,0.06)"}),"boxShadow":if dark||selected{"none"}else{"0 1px 1px rgba(0,0,0,0.06)"}}),
            vec![
                v(
                    "top",
                    json!({"flexDirection":"row","alignItems":"center"}),
                    top,
                ),
                styled_text(
                    label(&format!("smart.{k}")),
                    json!({"fontSize":12,"fontWeight":600,"color":if selected{"#ffffff".into()}else{token(a,"textDim","#999999")},"whiteSpace":"nowrap","overflow":"hidden"}),
                ),
            ],
        ),
        json!(["select", k]),
    )
}
fn sidebar(a: &App, now: i64) -> Value {
    let c = counts(&a.items, now);
    let mut rows = vec![
        v(
            "tiles",
            json!({"flexDirection":"row","flexWrap":"wrap","gap":8}),
            SMART
                .iter()
                .map(|(k, color)| tile(a, k, color, c[k].clone(), now))
                .collect(),
        ),
        styled_text(
            label("myLists"),
            json!({"fontSize":11,"fontWeight":600,"color":token(a,"textDim","#999999"),"marginHorizontal":8,"marginTop":18,"marginBottom":4}),
        ),
    ];
    for l in &a.lists {
        let id = string(&l["id"]);
        let selected = a.sel == format!("list:{id}");
        let color = l["color"].as_str().unwrap_or(COLORS[5]);
        let mut row = click(
            v(
                &format!("list-{id}"),
                json!({"height":32,"paddingHorizontal":8,"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":8,"backgroundColor":if selected{if a.theme["mode"]=="dark"{"rgba(255,255,255,0.14)"}else{"rgba(0,0,0,0.09)"}}else{"transparent"}}),
                vec![
                    v(
                        "glyph",
                        json!({"width":22,"height":22,"borderRadius":11,"alignItems":"center","justifyContent":"center","backgroundColor":color}),
                        vec![text(json!("☰"), 11., "#ffffff")],
                    ),
                    styled_text(
                        l["name"].clone(),
                        json!({"flex":1,"fontSize":13,"color":token(a,"text","#ffffff"),"whiteSpace":"nowrap","overflow":"hidden"}),
                    ),
                    text(
                        json!(c["lists"][id].as_u64().unwrap_or(0).to_string()),
                        12.,
                        &token(a, "textDim", "#999999"),
                    ),
                ],
            ),
            json!(["select", format!("list:{id}")]),
        );
        row["events"]["onContextMenu"] = json!(["listMenu", id]);
        if !selected {
            row["hoverStyle"] =
                json!({"backgroundColor":token(a,"hover","rgba(127,127,127,0.12)")});
        }
        rows.push(row);
    }
    if a.lists.is_empty() {
        rows.push(styled_text(
            label("noLists"),
            json!({"fontSize":12,"color":token(a,"textDim","#999999"),"marginHorizontal":8}),
        ));
    }
    let mut add = click(
        v(
            "add-list",
            json!({"height":22,"paddingHorizontal":10,"borderRadius":radius(a,true),"alignSelf":"flex-start","flexDirection":"row","gap":6,"alignItems":"center","justifyContent":"center"}),
            vec![
                text(json!("⊕"), 12., &token(a, "text", "#ffffff")),
                styled_text(
                    label("addList"),
                    json!({"fontSize":12,"fontWeight":500,"color":token(a,"text","#ffffff"),"whiteSpace":"nowrap"}),
                ),
            ],
        ),
        json!(["newList"]),
    );
    add["hoverStyle"] = json!({"backgroundColor":token(a,"hover","rgba(127,127,127,0.12)")});
    v(
        "sidebar",
        json!({"width":236,"backgroundColor":token(a,"sidebarBg","#252529")}),
        vec![
            rule(a, if a.rtl { "left" } else { "right" }, 0.),
            json!({"kind":"scroll","key":"lists-scroll","style":{"flex":1},"children":[v("lists",json!({"paddingHorizontal":10,"paddingTop":12,"paddingBottom":10,"gap":2}),rows)]}),
            v(
                "add-list-footer",
                json!({"height":40,"paddingHorizontal":10,"justifyContent":"center"}),
                vec![rule(a, "top", 0.), add],
            ),
        ],
    )
}
fn due_text(a: &mut App, it: &Value) -> String {
    let Some(due) = it["due"].as_i64() else {
        return String::new();
    };
    if it["dueAllDay"] == true {
        return a.date(parse_day(string(&it["dueDate"])).unwrap_or(due), "short");
    }
    let date = a.date(due, "short");
    let time = a.time(due);
    a.i18n
        .caption(&json!({"key":"reminders.dueAt","params":{"date":date,"time":time}}))
}
fn row(a: &mut App, it: &Value, depth: usize, now: i64) -> Value {
    let id = key(it);
    let list = a.lists.iter().find(|l| l["id"] == it["list"]);
    let color = list
        .and_then(|l| l["color"].as_str())
        .or_else(|| it["color"].as_str())
        .unwrap_or(COLORS[5])
        .to_owned();
    let list_name = if !a.sel.starts_with("list:") && a.sel != "all" {
        list.map(|l| string(&l["name"]).to_owned())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let done = a
        .toggles
        .get(&id)
        .map(|t| t.1)
        .unwrap_or(it["done"] == true);
    let circle = click(
        v(
            "circle",
            json!({"width":18,"height":18,"borderRadius":9,"alignItems":"center","justifyContent":"center","border":format!("1.5px solid {}",if done{&color}else{"#a1a1a6"})}),
            if done {
                vec![v(
                    "fill",
                    json!({"width":13,"height":13,"borderRadius":6.5,"backgroundColor":color}),
                    vec![],
                )]
            } else {
                vec![]
            },
        ),
        json!(["toggle", id]),
    );
    let mut content = vec![];
    if let Some((_, edit)) = a.editing.as_ref().filter(|(k, _)| k == &id) {
        let mut field = input(
            &format!("title-{id}"),
            edit,
            Value::Null,
            json!(["titleText"]),
            json!(["commitTitle"]),
            json!(["cancelTitle"]),
            true,
            json!({"height":24}),
        );
        field["blur"] = json!(["commitTitle"]);
        content.push(field);
    } else {
        let mut heading = vec![];
        let marks = marks(it["prio"].as_i64().unwrap_or(0));
        if !marks.is_empty() {
            heading.push(styled_text(
                json!(marks),
                json!({"fontSize":13,"fontWeight":700,"color":color}),
            ));
        }
        heading.push(styled_text(if string(&it["title"]).is_empty(){label("untitled")}else{it["title"].clone()},json!({"flex":1,"fontSize":13,"color":token(a,if done{"textDim"}else{"text"},"#ffffff")})));
        content.push(click(
            v(
                "title",
                json!({"flexDirection":"row","gap":4,"minHeight":20,"alignItems":"center"}),
                heading,
            ),
            json!(["editTitle", id]),
        ));
    }
    if !string(&it["notes"]).is_empty() {
        content.push(styled_text(json!(string(&it["notes"]).split('\n').next().unwrap_or("")),json!({"fontSize":12,"color":token(a,"textDim","#999999"),"whiteSpace":"nowrap","overflow":"hidden"})));
    }
    let due = due_text(a, it);
    let mut sub = vec![];
    if !due.is_empty() {
        sub.push(text(
            json!(due),
            12.,
            &if overdue(it, now) {
                RED.into()
            } else {
                token(a, "textDim", "#999999")
            },
        ));
    }
    if !list_name.is_empty() {
        sub.push(text(json!(list_name), 12., &token(a, "textDim", "#999999")));
    }
    if !sub.is_empty() {
        content.push(v("subtitle", json!({"flexDirection":"row","gap":6}), sub));
    }
    let mut children = vec![
        v("check", json!({"paddingTop":1}), vec![circle]),
        v("content", json!({"flex":1,"gap":2}), content),
    ];
    if it["flag"] == true {
        children.push(styled_text(
            json!("⚑"),
            json!({"fontSize":13,"color":"#ff9500","paddingTop":1}),
        ));
    }
    let lit = a.hover == id || a.inspector.as_ref().is_some_and(|i| key(&i.item) == id);
    let accent = if lit {
        token(a, "accent", "#007aff")
    } else {
        "transparent".into()
    };
    children.push(click(v(&format!("info-{id}"),json!({"width":20,"height":20,"borderRadius":10,"alignItems":"center","justifyContent":"center","border":format!("1.2px solid {accent}")}),vec![styled_text(json!("i"),json!({"fontSize":11,"fontWeight":700,"color":accent}))]),json!(["inspect",id])));
    let mut sep = json!({"position":"absolute","bottom":0,"height":1,"backgroundColor":token(a,"separator","#555555"),"pointerEvents":"none"});
    sep[if a.rtl { "right" } else { "left" }] = json!(36);
    sep[if a.rtl { "left" } else { "right" }] = json!(0);
    children.push(v("separator", sep, vec![]));
    let mut row = v(
        &format!("todo-{id}"),
        json!({"flexDirection":"row","alignItems":"flex-start","gap":10,"paddingVertical":8,"paddingHorizontal":8,"marginHorizontal":if depth>0{28}else{0}}),
        children,
    );
    row["events"] = json!({"onMouseEnter":["hover",id],"onMouseLeave":["leave",id]});
    row
}
fn new_row(a: &App, color: &str) -> Value {
    let mut children = vec![click(
        v(
            "add-circle",
            json!({"width":18,"height":18,"borderRadius":9,"alignItems":"center","justifyContent":"center","backgroundColor":if a.new_active{"transparent"}else{color}}),
            if a.new_active {
                vec![v(
                    "circle",
                    json!({"width":18,"height":18,"borderRadius":9,"border":"1.5px solid #a1a1a6"}),
                    vec![],
                )]
            } else {
                vec![styled_text(
                    json!("+"),
                    json!({"fontSize":13,"fontWeight":600,"color":"#ffffff"}),
                )]
            },
        ),
        json!(["new"]),
    )];
    children.push(if a.new_active {
        v(
            "new-field",
            json!({"flex":1}),
            vec![input(
                "new-title",
                &a.new_text,
                label("newReminder"),
                json!(["newText"]),
                json!(["add"]),
                json!(["cancelNew"]),
                true,
                json!({"height":26}),
            )],
        )
    } else {
        click(
            v(
                "new-label",
                json!({"flex":1}),
                vec![styled_text(
                    label("newReminder"),
                    json!({"fontSize":13,"color":token(a,"textDim","#999999")}),
                )],
            ),
            json!(["new"]),
        )
    });
    if a.new_error {
        children.push(text(label("saveFailed"), 12., RED));
    }
    v(
        "new-row",
        json!({"flexDirection":"row","alignItems":"center","gap":10,"minHeight":36,"paddingVertical":6,"paddingHorizontal":8}),
        children,
    )
}
fn field(a: &App, k: &str, children: Vec<Value>, top: bool) -> Value {
    v(
        k,
        json!({"flexDirection":"row","alignItems":if top{"flex-start"}else{"center"},"gap":10,"minHeight":28}),
        vec![
            styled_text(
                if k.is_empty() { json!("") } else { label(k) },
                json!({"width":70,"fontSize":12,"color":token(a,"textDim","#999999"),"textAlign":"end","marginTop":if top{6}else{0}}),
            ),
            v(
                "control",
                json!({"flex":1,"flexDirection":"row","alignItems":"center","gap":8,"flexWrap":"wrap"}),
                children,
            ),
        ],
    )
}
fn choice(a: &App, k: &str, title: Value, color: Option<&str>, width: f64) -> Value {
    let mut children = vec![];
    if let Some(color) = color {
        children.push(v(
            "color",
            json!({"width":10,"height":10,"borderRadius":5,"backgroundColor":color}),
            vec![],
        ));
    }
    children.push(styled_text(title,json!({"flex":1,"fontSize":13,"color":token(a,"text","#ffffff"),"whiteSpace":"nowrap","overflow":"hidden"})));
    children.push(text(json!("▼"), 9., &token(a, "textDim", "#999999")));
    click(
        v(
            &format!("choice-{k}"),
            json!({"width":width,"height":26,"paddingHorizontal":9,"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":6,"backgroundColor":token(a,"control","#3a3a3f")}),
            children,
        ),
        json!(["picker", k]),
    )
}
fn popup(
    a: &App,
    id: &str,
    anchor: &str,
    width: f64,
    padding: f64,
    children: Vec<Value>,
    dismiss: Value,
    z: i32,
) -> Value {
    let mut box_node = click(
        v(
            id,
            json!({"position":"absolute","left":8,"top":8,"width":width,"padding":padding,"borderRadius":10,"backgroundColor":if a.theme["mode"]=="dark"{"#2c2c31"}else{"#ffffff"},"border":format!("1px solid {}",token(a,"separator","#555555")),"boxShadow":"0 12px 32px rgba(0,0,0,0.32)"}),
            children,
        ),
        json!(["noop"]),
    );
    box_node["anchorKey"] = json!(anchor);
    box_node["fitPopover"] = json!(true);
    v(
        &format!("{id}-layer"),
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":z}),
        vec![
            {
                let mut bg = v(
                    "dismiss",
                    json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0}),
                    vec![],
                );
                bg["events"]["onMouseDown"] = dismiss;
                bg
            },
            box_node,
        ],
    )
}
fn inspector(a: &mut App, now: i64) -> Value {
    let Inspector { item, draft: d } = a.inspector.as_ref().unwrap();
    let item = item.clone();
    let d = d.clone();
    let mode = string(&d["mode"]);
    let mut children = vec![
        input(
            "inspector-title",
            string(&d["title"]),
            Value::Null,
            json!(["draft", "title"]),
            json!(["saveInspector"]),
            Value::Null,
            false,
            json!({"fontSize":15}),
        ),
        {
            let value = string(&d["notes"]);
            let caret = if a.notes_focus && (now - a.notes_since).max(0) / 530 % 2 == 0 {
                "▏"
            } else {
                ""
            };
            let shown = if a.notes_focus {
                json!(format!("{value}{caret}"))
            } else if value.is_empty() {
                label("notes")
            } else {
                json!(value)
            };
            let color = token(
                a,
                if value.is_empty() && !a.notes_focus {
                    "textDim"
                } else {
                    "text"
                },
                "#ffffff",
            );
            let mut n = v(
                "notes",
                json!({"minHeight":48,"padding":7,"borderRadius":7,"backgroundColor":token(a,"inputBg","#202024"),"border":format!("1px solid {}",token(a,if a.notes_focus{"accent"}else{"separator"},"#555555"))}),
                vec![styled_text(
                    shown,
                    json!({"fontSize":13,"color":color,"lineHeight":18}),
                )],
            );
            n["events"]["onMouseDown"] = json!(["notesFocus"]);
            n
        },
    ];
    let segments=[("none","dueNone"),("date","dueDate"),("time","dueTime")].iter().map(|(value,k)|{
        let selected=mode==*value;
        click(v(value,json!({"paddingHorizontal":10,"height":18,"borderRadius":(radius(a,true).as_f64().unwrap_or(6.)-1.).max(2.),"justifyContent":"center","alignItems":"center","flexGrow":0,"backgroundColor":if selected{token(a,"accent","#007aff")}else{"transparent".into()}}),vec![styled_text(label(k),json!({"fontSize":11,"fontWeight":600,"whiteSpace":"nowrap","color":if selected{"#ffffff".into()}else{token(a,"text","#ffffff")}}))]),json!(["setMode",value]))
    }).collect();
    children.push(field(a,"due",vec![v("due-mode",json!({"flexDirection":"row","backgroundColor":token(a,"control","#3a3a3f"),"borderRadius":radius(a,true),"padding":2}),segments)],false));
    if mode != "none" {
        let day = d["day"].as_i64().unwrap_or(0);
        let date = a.date(day, "numeric");
        let mut fields = vec![click(
            v(
                "choice-day",
                json!({"width":110,"height":26,"paddingHorizontal":9,"borderRadius":6,"justifyContent":"center","backgroundColor":token(a,"control","#3a3a3f")}),
                vec![styled_text(
                    json!(date),
                    json!({"fontSize":13,"color":token(a,"text","#ffffff"),"whiteSpace":"nowrap"}),
                )],
            ),
            json!(["picker", "day"]),
        )];
        if mode == "time" {
            let time = a.time(at_minutes(day, d["min"].as_i64().unwrap_or(540) as i32));
            fields.push(choice(a, "min", json!(time), None, 96.));
        }
        children.push(field(a, "", fields, false));
    }
    let prio = d["prio"].as_i64().unwrap_or(0);
    let priority = if prio >= 6 {
        "prio.low"
    } else if prio >= 2 {
        "prio.medium"
    } else if prio == 1 {
        "prio.high"
    } else {
        "prio.none"
    };
    children.push(field(
        a,
        "priority",
        vec![choice(a, "prio", label(priority), None, 150.)],
        false,
    ));
    let on = d["flag"] == true;
    let toggle = click(
        v(
            "flag-toggle",
            json!({"width":42,"height":24,"borderRadius":12,"direction":"ltr","backgroundColor":token(a,if on{"accent"}else{"control"},"#3a3a3f")}),
            vec![v(
                "knob",
                json!({"position":"absolute","top":2,"left":if on{20}else{2},"width":20,"height":20,"borderRadius":10,"backgroundColor":"#ffffff","boxShadow":"0 1px 3px rgba(0,0,0,0.35)"}),
                vec![],
            )],
        ),
        json!(["toggleFlag"]),
    );
    children.push(field(a, "flagged", vec![toggle], false));
    let list = a.lists.iter().find(|l| l["id"] == d["list"]);
    children.push(field(
        a,
        "list",
        vec![choice(
            a,
            "list",
            list.map(|l| l["name"].clone()).unwrap_or(json!("")),
            list.and_then(|l| l["color"].as_str()),
            180.,
        )],
        false,
    ));
    children.push(v(
        "actions",
        json!({"flexDirection":"row","gap":8,"marginTop":4}),
        vec![
            button(a, "delete", json!(["deleteTodo"]), true, false, true, false),
            spacer(),
            button(
                a,
                "done",
                json!(["saveInspector"]),
                true,
                true,
                false,
                false,
            ),
        ],
    ));
    let completed = if let Some(t) = item["doneAt"].as_i64().filter(|i| *i != 0) {
        let date = a.date(t, "short");
        json!({"key":"reminders.completedOn","params":{"date":date}})
    } else {
        json!("")
    };
    children.push(text(completed, 10., &token(a, "textDim", "#999999")));
    popup(
        a,
        "inspector",
        &format!("info-{}", key(&item)),
        340.,
        12.,
        vec![v("fields", json!({"gap":9}), children)],
        json!(["saveInspector"]),
        200,
    )
}
fn menu_row(
    a: &App,
    key: &str,
    title: Value,
    color: Option<&str>,
    checked: bool,
    destructive: bool,
    disabled: bool,
    action: Value,
) -> Value {
    let mut children = vec![text(
        json!(if checked { "✓" } else { "" }),
        11.,
        &token(a, "text", "#ffffff"),
    )];
    children[0]["style"]["width"] = json!(12);
    if let Some(color) = color {
        children.push(v(
            "color",
            json!({"width":10,"height":10,"borderRadius":5,"backgroundColor":color}),
            vec![],
        ));
    }
    let mut title = styled_text(
        title,
        json!({"flex":1,"fontSize":13,"whiteSpace":"nowrap","overflow":"hidden","color":if destructive{RED.into()}else{token(a,"text","#ffffff")}}),
    );
    title["hoverStyle"] = json!({"color":"#ffffff"});
    children.push(title);
    let mut node = v(
        key,
        json!({"height":26,"paddingHorizontal":8,"borderRadius":5,"flexDirection":"row","alignItems":"center","gap":6,"opacity":if disabled{0.4}else{1.}}),
        children,
    );
    if !disabled {
        node = click(node, action);
        node["hoverStyle"] = json!({"backgroundColor":token(a,"accent","#007aff")});
    }
    node
}
fn list_menu(a: &App, id: &str) -> Value {
    let local = a
        .lists
        .iter()
        .any(|l| l["id"] == id && l["kind"] == "local");
    popup(
        a,
        "list-menu",
        &format!("list-{id}"),
        200.,
        6.,
        vec![
            menu_row(
                a,
                "edit",
                label("editList"),
                None,
                false,
                false,
                false,
                json!(["editList", id]),
            ),
            v(
                "rule",
                json!({"height":1,"marginVertical":4,"marginHorizontal":6,"backgroundColor":token(a,"separator","#555555")}),
                vec![],
            ),
            menu_row(
                a,
                "delete",
                label("deleteList"),
                None,
                false,
                true,
                !local,
                json!(["deleteList", id]),
            ),
        ],
        json!(["closeMenu"]),
        200,
    )
}
fn month_picker(a: &mut App, now: i64) -> Value {
    let day = a
        .inspector
        .as_ref()
        .and_then(|i| i.draft["day"].as_i64())
        .unwrap_or(now);
    let m = crate::clock::local(a.picker_month);
    let first = add_days(a.picker_month, -((m.tm_wday + 6) % 7));
    let month_name = a
        .i18n
        .caption(&json!({"key":format!("calendar.month.{}",m.tm_mon)}));
    let title = a.i18n.caption(
        &json!({"key":"calendar.monthYear","params":{"month":month_name,"year":m.tm_year+1900}}),
    );
    let mut header = vec![styled_text(
        json!(title),
        json!({"flex":1,"fontSize":12,"fontWeight":600,"color":token(a,"text","#ffffff"),"marginHorizontal":4,"whiteSpace":"nowrap","overflow":"hidden"}),
    )];
    for (delta, glyph) in [
        ("-1", if a.rtl { "›" } else { "‹" }),
        ("today", "●"),
        ("1", if a.rtl { "‹" } else { "›" }),
    ] {
        let mut n = click(
            v(
                delta,
                json!({"width":20,"height":20,"borderRadius":5,"alignItems":"center","justifyContent":"center"}),
                vec![text(
                    json!(glyph),
                    if delta == "today" { 7. } else { 15. },
                    &token(a, "textDim", "#999999"),
                )],
            ),
            json!(["month", delta]),
        );
        n["hoverStyle"] = json!({"backgroundColor":token(a,"hover","rgba(127,127,127,0.12)")});
        header.push(n);
    }
    let lang = a.i18n.lang.clone();
    let names = a
        .i18n
        .entry(&lang, "format", "weekdays")
        .or_else(|| a.i18n.entry("en", "format", "weekdays"))
        .unwrap_or(Value::Null);
    let weekdays=[1,2,3,4,5,6,0].iter().map(|wd|styled_text(json!(string(&names[*wd]).chars().next().map(|c|c.to_string()).unwrap_or_default()),json!({"width":28,"textAlign":"center","fontSize":10,"fontWeight":600,"color":token(a,"textDim","#999999")}))).collect();
    let mut children = vec![
        v(
            "header",
            json!({"flexDirection":"row","alignItems":"center","height":24}),
            header,
        ),
        v("weekdays", json!({"flexDirection":"row"}), weekdays),
    ];
    for r in 0..6 {
        let mut cells = vec![];
        for c in 0..7 {
            let d = add_days(first, r * 7 + c);
            let t = crate::clock::local(d);
            let selected = ymd(day) == ymd(d);
            let today = ymd(now) == ymd(d);
            cells.push(click(v(&format!("day-{}",ymd(d)),json!({"width":28,"height":28,"alignItems":"center","justifyContent":"center"}),vec![v("circle",json!({"width":24,"height":24,"borderRadius":12,"alignItems":"center","justifyContent":"center","backgroundColor":if selected{if today{RED.into()}else{token(a,"accent","#007aff")}}else{"transparent".into()}}),vec![styled_text(json!(t.tm_mday.to_string()),json!({"fontSize":11,"fontWeight":if today||selected{600}else{400},"color":if selected{"#ffffff".into()}else if today{RED.into()}else if t.tm_mon==m.tm_mon{token(a,"text","#ffffff")}else{token(a,"textDim","#999999")}}))])]),json!(["pickDay",d])));
        }
        children.push(v(
            &format!("week-{r}"),
            json!({"flexDirection":"row"}),
            cells,
        ));
    }
    popup(
        a,
        "date-picker",
        "choice-day",
        212.,
        8.,
        vec![v("month", json!({"width":196,"gap":2}), children)],
        json!(["closePicker"]),
        201,
    )
}
fn choice_picker(a: &mut App) -> Value {
    let d = a.inspector.as_ref().unwrap().draft.clone();
    let field = a.picker.clone();
    let mut options = vec![];
    match field.as_str() {
        "prio" => {
            let p = d["prio"].as_i64().unwrap_or(0);
            let value = if p >= 6 {
                9
            } else if p >= 2 {
                5
            } else {
                p
            };
            for (prio, k) in [
                (0, "prio.none"),
                (9, "prio.low"),
                (5, "prio.medium"),
                (1, "prio.high"),
            ] {
                options.push(menu_row(
                    a,
                    &prio.to_string(),
                    label(k),
                    None,
                    value == prio,
                    false,
                    false,
                    json!(["pickValue", prio]),
                ));
            }
        }
        "list" => {
            for l in &a.lists {
                options.push(menu_row(
                    a,
                    string(&l["id"]),
                    l["name"].clone(),
                    l["color"].as_str(),
                    l["id"] == d["list"],
                    false,
                    false,
                    json!(["pickValue", l["id"]]),
                ));
            }
        }
        "min" => {
            let value = d["min"].as_i64().unwrap_or(540);
            let mut minutes = (0..1440).step_by(15).collect::<Vec<_>>();
            if !minutes.contains(&value) {
                minutes.push(value);
                minutes.sort();
            }
            for min in minutes {
                let time = a.time(at_minutes(d["day"].as_i64().unwrap_or(0), min as i32));
                options.push(menu_row(
                    a,
                    &min.to_string(),
                    json!(time),
                    None,
                    min == value,
                    false,
                    false,
                    json!(["pickValue", min]),
                ));
            }
        }
        _ => {}
    }
    let width = if field == "list" { 180. } else { 170. };
    let current = if field == "min" {
        d["min"].as_i64().unwrap_or(540).to_string()
    } else {
        string(&d["list"]).to_owned()
    };
    let offset = (options
        .iter()
        .position(|o| o["key"] == current)
        .unwrap_or(0) as i64
        * 26
        - 100)
        .max(0);
    let children = if options.len() > 12 {
        vec![v(
            "scroll-height",
            json!({"height":260}),
            vec![
                json!({"kind":"scroll","key":format!("options-{field}"),"style":{"flex":1},"initialOffset":offset,"children":options}),
            ],
        )]
    } else {
        options
    };
    popup(
        a,
        "choices",
        &format!("choice-{field}"),
        width,
        6.,
        children,
        json!(["closePicker"]),
        201,
    )
}
fn list_sheet(a: &App) -> Value {
    let d = a.list_draft.as_ref().unwrap();
    let editing = d.id.is_some();
    let colors=COLORS.iter().map(|c|click(v(c,json!({"width":24,"height":24,"borderRadius":12,"alignItems":"center","justifyContent":"center","border":format!("2px solid {}",if d.color==*c{token(a,"text","#ffffff")}else{"transparent".into()})}),vec![v("fill",json!({"width":18,"height":18,"borderRadius":9,"backgroundColor":c}),vec![])]),json!(["listColor",c]))).collect();
    v(
        "list-sheet",
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":100,"alignItems":"center","justifyContent":"center","backgroundColor":"rgba(0,0,0,0.3)"}),
        vec![v(
            "sheet",
            json!({"width":380,"maxWidth":"94%","maxHeight":"94%","borderRadius":12,"backgroundColor":token(a,"windowBg","#252529"),"border":format!("1px solid {}",token(a,"separator","#555555")),"boxShadow":"0 18px 50px rgba(0,0,0,0.45)","overflow":"hidden"}),
            vec![
                v(
                    "heading",
                    json!({"paddingHorizontal":20,"paddingTop":18,"paddingBottom":4}),
                    vec![styled_text(
                        label(if editing { "editList" } else { "newList" }),
                        json!({"fontSize":15,"fontWeight":600,"color":token(a,"text","#ffffff")}),
                    )],
                ),
                v(
                    "body",
                    json!({"paddingHorizontal":20,"paddingVertical":12,"gap":10,"flexShrink":1}),
                    vec![
                        v(
                            "list-icon",
                            json!({"alignItems":"center","marginVertical":4}),
                            vec![v(
                                "circle",
                                json!({"width":56,"height":56,"borderRadius":28,"alignItems":"center","justifyContent":"center","backgroundColor":d.color}),
                                vec![text(json!("☰"), 26., "#ffffff")],
                            )],
                        ),
                        input(
                            "list-name",
                            &d.name,
                            label("listName"),
                            json!(["listName"]),
                            json!(["saveList"]),
                            json!(["cancelList"]),
                            true,
                            json!({}),
                        ),
                        styled_text(
                            label("color"),
                            json!({"fontSize":11,"fontWeight":600,"color":token(a,"textDim","#999999")}),
                        ),
                        v(
                            "colors",
                            json!({"flexDirection":"row","gap":8,"flexWrap":"wrap"}),
                            colors,
                        ),
                    ],
                ),
                v(
                    "footer",
                    json!({"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":20,"paddingVertical":14}),
                    vec![
                        rule(a, "top", 0.),
                        styled_text(
                            if d.error {
                                label("saveFailed")
                            } else {
                                json!("")
                            },
                            json!({"flex":1,"fontSize":12,"color":RED}),
                        ),
                        button(
                            a,
                            "cancel",
                            json!(["cancelList"]),
                            false,
                            false,
                            false,
                            false,
                        ),
                        button(
                            a,
                            if editing { "save" } else { "create" },
                            json!(["saveList"]),
                            false,
                            true,
                            false,
                            d.name.trim().is_empty(),
                        ),
                    ],
                ),
            ],
        )],
    )
}
pub(super) fn tree(a: &mut App, now: i64) -> Value {
    let mut toolbar = vec![spacer()];
    if a.sel != "completed" {
        toolbar.push(toolbar_button(
            a,
            if a.show_done {
                "hideCompleted"
            } else {
                "showCompleted"
            },
            None,
            json!(["showDone"]),
            true,
            a.show_done,
            false,
        ));
    }
    toolbar.push(toolbar_button(
        a,
        "newReminder",
        Some("+"),
        json!(["new"]),
        false,
        false,
        a.unavailable || a.sel == "completed",
    ));
    let mut main = vec![v(
        "toolbar",
        json!({"height":44,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":12,"zIndex":10}),
        toolbar,
    )];
    if a.unavailable {
        main.push(v("unavailable",json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":8,"padding":24}),vec![styled_text(label("unavailable"),json!({"fontSize":17,"fontWeight":600,"color":token(a,"text","#ffffff")})),styled_text(label("unavailableHint"),json!({"fontSize":13,"color":token(a,"textDim","#999999"),"textAlign":"center"}))]));
    } else {
        let list = a
            .sel
            .strip_prefix("list:")
            .and_then(|id| a.lists.iter().find(|l| l["id"] == id));
        let title = list
            .map(|l| l["name"].clone())
            .unwrap_or_else(|| label(&format!("smart.{}", a.sel)));
        let color = list
            .map(|l| l["color"].as_str().unwrap_or(COLORS[5]).to_owned())
            .unwrap_or_else(|| {
                SMART
                    .iter()
                    .find(|(k, _)| *k == a.sel)
                    .map(|(_, c)| (*c).into())
                    .unwrap_or_else(|| token(a, "text", "#ffffff"))
            });
        let groups = sections(&a.sel, &a.lists, &a.items, a.show_done, now);
        let total: usize = groups.iter().map(|s| s.rows.len()).sum();
        let open = groups
            .iter()
            .flat_map(|s| &s.rows)
            .filter(|(it, _)| it["done"] != true)
            .count();
        let mut rows = vec![];
        for section in groups {
            let mut children = vec![];
            if !section.title.is_null() {
                let title = if let Some(day) = section.title["day"].as_str() {
                    json!(a.date(parse_day(day).unwrap_or(now), "long"))
                } else {
                    section.title.clone()
                };
                children.push(styled_text(title,json!({"fontSize":15,"fontWeight":600,"color":if section.color.is_empty(){token(a,"text","#ffffff")}else{section.color},"marginHorizontal":8,"marginTop":6,"marginBottom":2})));
            }
            for (it, depth) in section.rows {
                children.push(row(a, &it, depth, now));
            }
            rows.push(v(
                &section.id,
                json!({"marginBottom":if section.title.is_null(){0}else{14}}),
                children,
            ));
        }
        if total == 0 && a.items_loaded {
            if a.sel == "completed" {
                rows.push(styled_text(label("nothingCompleted"),json!({"fontSize":13,"color":token(a,"textDim","#999999"),"marginHorizontal":8,"marginVertical":10})));
            } else if a.sel != "all" && !a.sel.starts_with("list:") {
                rows.push(styled_text(label("empty"),json!({"fontSize":13,"color":token(a,"textDim","#999999"),"marginHorizontal":8,"marginVertical":8})));
            }
        }
        if a.sel != "completed" {
            let target = if let Some(id) = a.sel.strip_prefix("list:") {
                a.lists.iter().find(|l| l["id"] == id)
            } else {
                a.lists.first()
            };
            let c = target
                .and_then(|l| l["color"].as_str())
                .unwrap_or(&color)
                .to_owned();
            rows.push(new_row(a, &c));
        }
        main.push(v("list-view",json!({"flex":1}),vec![v("heading",json!({"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":20,"paddingBottom":10}),vec![styled_text(title,json!({"flex":1,"fontSize":26,"fontWeight":700,"color":color,"whiteSpace":"nowrap","overflow":"hidden"})),styled_text(json!(if a.sel=="completed"{total}else{open}.to_string()),json!({"fontSize":26,"fontWeight":600,"color":color}))]),json!({"kind":"scroll","key":format!("items-{}",a.sel),"style":{"flex":1},"children":[v("rows",json!({"paddingHorizontal":12,"paddingBottom":20}),rows)]})]));
    }
    let mut children = vec![sidebar(a, now), v("main", json!({"flex":1}), main)];
    if a.list_draft.is_some() {
        children.push(list_sheet(a));
    }
    if let Some(menu) = a.menu.clone() {
        children.push(list_menu(a, &menu));
    }
    if a.inspector.is_some() {
        children.push(inspector(a, now));
        if !a.picker.is_empty() {
            children.push(if a.picker == "day" {
                month_picker(a, now)
            } else {
                choice_picker(a)
            });
        }
    }
    v(
        "reminders",
        json!({"flex":1,"flexDirection":"row","backgroundColor":token(a,"windowBg","#252529"),"direction":if a.rtl{"rtl"}else{"ltr"}}),
        children,
    )
}
