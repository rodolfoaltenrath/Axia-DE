use super::{App, Busy};
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
fn dim(label: Value, t: &Value) -> Value {
    text(
        label,
        json!({"fontSize":11,"color":t["textDim"],"lineHeight":1.4}),
    )
}
fn click(mut node: Value, action: Value) -> Value {
    node["events"]["onClick"] = action;
    node
}
fn button(label: Value, action: &str, primary: bool, disabled: bool) -> Value {
    json!({"kind":"button","key":action,"label":label,"action":[action],"primary":primary,"disabled":disabled})
}
fn option(cmd: &str, id: Value, label: Value, selected: bool, t: &Value) -> Value {
    click(
        v(
            &format!("{cmd}:{id}"),
            json!({"paddingHorizontal":10,"height":24,"borderRadius":6,"justifyContent":"center","alignItems":"center","backgroundColor":if selected{t["accent"].clone()}else{t["control"].clone()}}),
            vec![text(
                label,
                json!({"fontSize":12,"color":if selected{json!("#fff")}else{t["text"].clone()},"whiteSpace":"nowrap"}),
            )],
        ),
        json!([cmd, id]),
    )
}
fn row(key: &str, children: Vec<Value>) -> Value {
    v(
        key,
        json!({"flexDirection":"row","flexWrap":"wrap","gap":6}),
        children,
    )
}
fn caption(label: Value, t: &Value) -> Value {
    text(
        label,
        json!({"fontSize":11,"fontWeight":600,"color":t["textDim"],"marginTop":6}),
    )
}
pub(super) fn icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    v(
        "scanner-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #66d4cf, #0f7c78)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden"}),
        vec![
            v(
                "bed",
                json!({"position":"absolute","left":s*0.12,"top":s*0.46,"width":s*0.76,"height":s*0.3,"borderRadius":s*0.06,"backgroundColor":"#e9f7f6"}),
                vec![],
            ),
            v(
                "lid",
                json!({"position":"absolute","left":s*0.12,"top":s*0.3,"width":s*0.76,"height":s*0.12,"borderRadius":s*0.05,"backgroundColor":"#cfe9e7"}),
                vec![],
            ),
            v(
                "light",
                json!({"position":"absolute","left":s*0.18,"top":s*0.54,"width":s*0.64,"height":s*0.05,"backgroundColor":"#7cf5a0"}),
                vec![],
            ),
            v(
                "line",
                json!({"position":"absolute","left":s*0.18,"top":s*0.64,"width":s*0.3,"height":s*0.04,"backgroundColor":"#9fbfbd"}),
                vec![],
            ),
        ],
    )
}
fn empty(title: Value, hint: Value, t: &Value, with_icon: bool) -> Value {
    let mut children = vec![];
    if with_icon {
        children.push(icon(56.));
    }
    if !title.is_null() {
        children.push(text(
            title,
            json!({"fontSize":15,"fontWeight":600,"color":t["text"],"textAlign":"center"}),
        ));
    }
    if !hint.is_null() {
        children.push(text(
            hint,
            json!({"fontSize":13,"color":t["textDim"],"textAlign":"center","lineHeight":1.4}),
        ));
    }
    v(
        "empty",
        json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":12,"padding":20}),
        children,
    )
}
fn chooser(app: &App) -> Value {
    let t = &app.theme;
    if app.found.as_ref().is_none_or(|s| s.is_empty()) {
        return empty(
            key("scanner.choose"),
            key(if app.found.is_none() {
                "scanner.searching"
            } else {
                "scanner.none"
            }),
            t,
            true,
        );
    }
    let rows=app.found.as_ref().unwrap().iter().enumerate().map(|(i,device)|{
        click(v(&format!("device:{i}"),json!({"flexDirection":"row","gap":12,"paddingVertical":10,"alignItems":"center"}),vec![
            icon(32.),v("name",json!({"flex":1,"minWidth":0,"gap":2}),vec![
                text(device["name"].clone(),json!({"fontSize":13,"fontWeight":600,"color":t["text"],"whiteSpace":"nowrap","overflow":"hidden"})),
                dim(json!(format!("{} {}:{}",device["model"].as_str().unwrap_or(""),device["addr"].as_str().unwrap_or(""),device["port"])),t)
            ]),text(json!("›"),json!({"fontSize":15,"color":t["textDim"]}))
        ]),json!(["choose",i]))
    }).collect();
    v(
        "chooser",
        json!({"flex":1,"padding":20,"gap":12}),
        vec![
            text(
                key("scanner.choose"),
                json!({"fontSize":15,"fontWeight":600,"color":t["text"]}),
            ),
            json!({"kind":"scroll","key":"devices","style":{"flex":1},"children":[v("discovered",json!({"borderRadius":10,"borderWidth":1,"borderColor":t["separator"],"backgroundColor":if t["mode"]=="dark"{"rgba(255,255,255,0.04)"}else{"rgba(0,0,0,0.025)"},"paddingHorizontal":12}),rows)]}),
        ],
    )
}
fn preview(app: &App) -> Value {
    let t = &app.theme;
    let children = if let (Some(image), Some(frame)) = (&app.preview, app.frame()) {
        let mut image_nodes = vec![
            json!({"kind":"documentImage","key":"preview-image","image":image.id,"style":{"width":frame.w,"height":frame.h}}),
        ];
        if let Some([x, y, w, h]) = app.area.filter(|r| r[2] > 0.) {
            image_nodes.push(v("area",json!({"position":"absolute","left":x*frame.w,"top":y*frame.h,"width":w*frame.w,"height":h*frame.h,"borderWidth":2,"borderColor":t["accent"]}),vec![
            v("shade",json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"backgroundColor":t["accent"],"opacity":0.13}),vec![])
        ]));
        }
        let mut image = v(
            "preview-page",
            json!({"width":frame.w,"height":frame.h,"flexShrink":0,"backgroundColor":"#ffffff","boxShadow":"0 2px 10px rgba(0,0,0,0.35)","direction":"ltr"}),
            image_nodes,
        );
        image["events"]["onMouseDown"] = json!(["cropStart"]);
        vec![image]
    } else {
        vec![empty(
            if app.busy == Some(Busy::Preview) {
                key("scanner.previewing")
            } else {
                Value::Null
            },
            if app.busy == Some(Busy::Preview) {
                Value::Null
            } else {
                key("scanner.previewHint")
            },
            t,
            false,
        )]
    };
    v(
        "preview",
        json!({"flex":1,"minWidth":0,"alignItems":"center","justifyContent":"center","padding":20,"overflow":"hidden","backgroundColor":if t["mode"]=="dark"{"#2a2a2e"}else{"#e3e3e8"}}),
        children,
    )
}
fn panel(app: &App) -> Value {
    let t = &app.theme;
    let device = app.scanner.as_ref().unwrap();
    let busy = app.busy.is_some();
    let subtitle = if let Some(caps) = &app.caps {
        caps["model"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| json!(s))
            .unwrap_or_else(|| {
                json!(format!(
                    "{}:{}",
                    device["addr"].as_str().unwrap_or(""),
                    device["port"]
                ))
            })
    } else if app.error.is_null() {
        key("scanner.connecting")
    } else {
        Value::Null
    };
    let mut content = vec![v(
        "device",
        json!({"gap":2}),
        vec![
            text(
                device["name"].clone(),
                json!({"fontSize":15,"fontWeight":600,"color":t["text"]}),
            ),
            dim(subtitle, t),
        ],
    )];
    if let Some(caps) = &app.caps {
        if caps["platen"] == true && caps["adf"] == true {
            content.push(row(
                "source",
                ["Platen", "Feeder"]
                    .iter()
                    .map(|s| {
                        option(
                            "source",
                            json!(s),
                            key(if *s == "Platen" {
                                "scanner.platen"
                            } else {
                                "scanner.feeder"
                            }),
                            app.source == *s,
                            t,
                        )
                    })
                    .collect(),
            ));
        }
        content.push(caption(key("scanner.mode"), t));
        let mut colors = app.colors();
        if colors.is_empty() {
            colors.push("RGB24".into());
        }
        content.push(row(
            "colors",
            colors
                .iter()
                .map(|c| {
                    option(
                        "color",
                        json!(c),
                        key(match c.as_str() {
                            "Grayscale8" => "scanner.color.gray",
                            "BlackAndWhite1" => "scanner.color.bw",
                            _ => "scanner.color.color",
                        }),
                        app.color == *c,
                        t,
                    )
                })
                .collect(),
        ));
        content.push(caption(key("scanner.resolution"), t));
        let mut resolutions = app.resolutions();
        if resolutions.is_empty() {
            resolutions = vec![75, 150, 300, 600];
        }
        content.push(row(
            "resolutions",
            resolutions
                .into_iter()
                .take(8)
                .map(|r| {
                    option(
                        "resolution",
                        json!(r),
                        json!({"key":"scanner.dpi","params":{"dpi":r}}),
                        app.res == r,
                        t,
                    )
                })
                .collect(),
        ));
        content.push(caption(key("scanner.format"), t));
        content.push(row(
            "formats",
            [("pdf", "PDF"), ("png", "PNG"), ("jpeg", "JPEG")]
                .into_iter()
                .map(|(id, label)| option("format", json!(id), json!(label), app.format == id, t))
                .collect(),
        ));
        let region = app.region();
        content.push(dim(if app.area.is_some(){json!({"key":"scanner.areaSet","params":{"w":(region["w"].as_f64().unwrap_or(0.)/300.*25.4).round(),"h":(region["h"].as_f64().unwrap_or(0.)/300.*25.4).round()}})}else{key("scanner.areaAll")},t));
    }
    if !app.error.is_null() {
        content.push(text(
            app.error.clone(),
            json!({"fontSize":12,"color":"#ff453a","lineHeight":1.4}),
        ));
    }
    if !app.notice.is_null() {
        content.push(text(
            app.notice.clone(),
            json!({"fontSize":12,"color":t["accent"],"lineHeight":1.4}),
        ));
    }
    let mut actions = vec![];
    if app.saved.as_ref().is_some_and(|s| s.format != "pdf") {
        actions.push(button(key("scanner.print"), "print", false, false));
    }
    actions.push(button(
        key(if app.busy == Some(Busy::Preview) {
            "scanner.working"
        } else {
            "scanner.preview"
        }),
        "preview",
        false,
        busy || app.caps.is_none(),
    ));
    actions.push(button(
        key(if app.busy == Some(Busy::Scan) {
            "scanner.working"
        } else {
            "scanner.scan"
        }),
        "scan",
        true,
        busy || app.caps.is_none(),
    ));
    v(
        "panel",
        json!({"width":280,"flexShrink":0,"backgroundColor":t["sidebarBg"]}),
        vec![
            v(
                "separator",
                json!({"position":"absolute","start":0,"top":0,"bottom":0,"width":1,"backgroundColor":t["separator"]}),
                vec![],
            ),
            json!({"kind":"scroll","key":"options","style":{"flex":1},"children":[v("options-body",json!({"paddingHorizontal":16,"paddingTop":16,"paddingBottom":12,"gap":8}),content)]}),
            v(
                "footer",
                json!({"paddingHorizontal":16,"paddingTop":12,"paddingBottom":16,"gap":10}),
                vec![
                    v(
                        "footer-rule",
                        json!({"position":"absolute","left":0,"right":0,"top":0,"height":1,"backgroundColor":t["separator"]}),
                        vec![],
                    ),
                    v(
                        "actions",
                        json!({"flexDirection":"row","justifyContent":"flex-end","flexWrap":"wrap","gap":8}),
                        actions,
                    ),
                    button(key("scanner.other"), "other", false, busy),
                ],
            ),
        ],
    )
}
pub(super) fn tree(app: &App) -> Value {
    let children = if app.scanner.is_some() {
        vec![preview(app), panel(app)]
    } else {
        vec![chooser(app)]
    };
    v(
        "scanner",
        json!({"flex":1,"flexDirection":"row","direction":if app.rtl{"rtl"}else{"ltr"}}),
        children,
    )
}
