use super::{basename, duration, stem, App};
use crate::native_view::date_format;
use serde_json::{json, Value};
fn v(id: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":id,"style":style,"children":children})
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn text(label: Value, size: f64, color: &Value) -> Value {
    json!({"kind":"text","label":label,"style":{"fontSize":size,"color":color}})
}
fn click(mut node: Value, cmd: &str, arg: Value) -> Value {
    node["events"]["onClick"] = json!([cmd, arg]);
    node
}
fn button(label: Value, cmd: &str, primary: bool, disabled: bool) -> Value {
    json!({"kind":"button","key":cmd,"label":label,"action":[cmd],"primary":primary,"disabled":disabled})
}
fn row(id: &str, children: Vec<Value>) -> Value {
    v(
        id,
        json!({"flexDirection":"row","alignItems":"center","flexWrap":"wrap","gap":8}),
        children,
    )
}
fn option(label: Value, cmd: &str, arg: Value, selected: bool, t: &Value) -> Value {
    click(
        v(
            &format!("{cmd}:{arg}"),
            json!({"paddingHorizontal":10,"height":26,"borderRadius":6,"justifyContent":"center","backgroundColor":if selected{t["accent"].clone()}else{t["control"].clone()}}),
            vec![text(
                label,
                12.,
                &if selected {
                    json!("#fff")
                } else {
                    t["text"].clone()
                },
            )],
        ),
        cmd,
        arg,
    )
}
pub(super) fn icon(s: f64) -> Value {
    let s = if s.is_finite() {
        s.clamp(1., 512.)
    } else {
        32.
    };
    v("recorder-icon",json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #2b2b30, #0e0e10)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden"}),[0.18,0.34,0.52,0.3,0.6,0.42,0.24,0.12].iter().enumerate().map(|(i,h)|v(&format!("bar{i}"),json!({"position":"absolute","left":s*(0.16+i as f64*0.09),"top":s*(0.5-h/2.),"width":s*0.05,"height":s*h,"borderRadius":s*0.025,"backgroundColor":if i==4{"#fff"}else{"#ff453a"}}),vec![])).collect())
}
fn empty(t: &Value) -> Value {
    v(
        "empty",
        json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":12,"padding":20}),
        vec![
            icon(56.),
            text(key("recorder.emptyTitle"), 15., &t["text"]),
            text(key("recorder.hint"), 13., &t["textDim"]),
        ],
    )
}
fn date(app: &mut App, ms: i64, style: &str) -> String {
    date_format::date(&mut app.i18n, &app.region, &crate::clock::local(ms), style)
}
pub(super) fn tree(app: &mut App) -> Value {
    let t = app.theme.clone();
    let entries = app.entries.clone();
    let rows=entries.iter().map(|e|{
        let selected=app.selection.as_ref()==Some(&e.path);
        let date=date(app,e.mtime,"short");
        let ext=basename(&e.path).rsplit('.').next().unwrap_or("").to_uppercase();
        click(v(&e.path,json!({"paddingHorizontal":10,"paddingVertical":7,"gap":2,"borderRadius":7,"marginBottom":2,"backgroundColor":if selected{t["accent"].clone()}else{json!("transparent")}}),vec![
            json!({"kind":"text","label":stem(&e.path),"style":{"fontSize":13,"fontWeight":600,"color":if selected{json!("#fff")}else{t["text"].clone()},"whiteSpace":"nowrap","overflow":"hidden"}}),
            text(json!(format!("{date} · {ext}")),11.,&if selected{json!("rgba(255,255,255,0.8)")}else{t["textDim"].clone()})
        ]),"select",json!(e.path))
    }).collect::<Vec<_>>();
    let list = if rows.is_empty() {
        vec![text(key("recorder.empty"), 12., &t["textDim"])]
    } else {
        rows
    };
    let mut controls = vec![];
    if app.rec > 0 {
        controls.push(v("levels",json!({"alignSelf":"stretch","height":48,"flexDirection":"row","alignItems":"center","gap":1,"direction":"ltr","overflow":"hidden"}),app.levels.iter().enumerate().map(|(i,l)|v(&format!("level{i}"),json!({"width":1,"height":(l.sqrt()*60.).clamp(1.,46.),"backgroundColor":"#ff453a","flexShrink":0}),vec![])).collect()));
        controls.push(text(json!(duration(app.rec_ms)), 22., &t["text"]));
    }
    let mut record_actions = vec![];
    if app.rec > 0 {
        record_actions.push(button(
            json!(if app.paused { "▶" } else { "Ⅱ" }),
            "pause",
            false,
            app.stopping,
        ));
    }
    record_actions.push(click(v("record",json!({"width":56,"height":56,"borderRadius":28,"borderWidth":3,"borderColor":if t["mode"]=="dark"{"rgba(255,255,255,0.28)"}else{"rgba(0,0,0,0.18)"},"alignItems":"center","justifyContent":"center","opacity":if app.record_start{0.5}else{1.}}),vec![v("record-dot",json!({"width":if app.rec>0{20}else{42},"height":if app.rec>0{20}else{42},"borderRadius":if app.rec>0{5}else{21},"backgroundColor":"#ff3b30"}),vec![])]),"record",Value::Null));
    controls.push(row("record-actions", record_actions));
    let mics = app.microphones();
    if mics.len() > 1 && app.rec == 0 && !app.record_start {
        controls.push(row(
            "sources",
            mics.iter()
                .take(3)
                .map(|m| {
                    let desc = m["description"].as_str().unwrap_or("");
                    let label = if desc.chars().count() > 14 {
                        format!("{}…", desc.chars().take(13).collect::<String>())
                    } else {
                        desc.into()
                    };
                    option(
                        json!(label),
                        "source",
                        m["name"].clone(),
                        m["name"] == app.source_name(),
                        &t,
                    )
                })
                .collect(),
        ));
    }
    let sidebar = v(
        "sidebar",
        json!({"width":240,"flexShrink":0,"backgroundColor":t["sidebarBg"]}),
        vec![
            v(
                "separator",
                json!({"position":"absolute","end":0,"top":0,"bottom":0,"width":1,"backgroundColor":t["separator"]}),
                vec![],
            ),
            v(
                "heading",
                json!({"height":44,"justifyContent":"center","paddingHorizontal":20}),
                vec![text(key("recorder.all"), 15., &t["text"])],
            ),
            json!({"kind":"scroll","key":"recordings","style":{"flex":1},"children":[v("list",json!({"paddingHorizontal":10}),list)]}),
            v(
                "record-controls",
                json!({"paddingHorizontal":16,"paddingTop":12,"paddingBottom":16,"gap":10,"alignItems":"center","borderTopWidth":1,"borderTopColor":t["separator"]}),
                controls,
            ),
        ],
    );
    let mut content = vec![];
    let cur = app
        .selection
        .as_ref()
        .and_then(|p| app.entries.iter().find(|e| e.path == *p))
        .cloned();
    if let Some(cur) = cur {
        let name = if let Some(name) = &app.renaming {
            json!({"kind":"input","key":"rename-input","value":name,"autoFocus":true,"selectAll":true,"change":["name"],"submit":["renameSubmit"],"cancel":["renameCancel"],"style":{"fontSize":17}})
        } else {
            let mut title = text(json!(stem(&cur.path)), 28., &t["text"]);
            title["style"]["fontWeight"] = json!(600);
            title["style"]["whiteSpace"] = json!("nowrap");
            title["style"]["overflow"] = json!("hidden");
            title["events"]["onDoubleClick"] = json!(["rename"]);
            title
        };
        let when = date(app, cur.mtime, "long");
        content.push(v(
            "title",
            json!({"gap":4}),
            vec![
                name,
                text(
                    json!(format!(
                        "{} · {}",
                        when,
                        app.wave
                            .as_ref()
                            .map_or_else(|| "…".into(), |w| duration(w.duration))
                    )),
                    12.,
                    &t["textDim"],
                ),
            ],
        ));
        let width = app.wave_width();
        let mut waveform = if app.wave.is_some() {
            vec![
                json!({"kind":"raster","image":"wave","generation":app.raster_generation,"style":{"width":width-2,"height":104,"flexShrink":0}}),
            ]
        } else {
            vec![text(json!("…"), 12., &t["textDim"])]
        };
        if let Some(trim) = app.trim {
            for (i, f) in trim.iter().enumerate() {
                waveform.push(v(&format!("handle{i}"),json!({"position":"absolute","left":f*width as f64-2.,"top":0,"width":4,"height":120,"borderRadius":2,"backgroundColor":"#ffcc00"}),vec![]));
            }
        }
        let mut wave = v(
            "waveform",
            json!({"width":width,"height":120,"borderRadius":10,"backgroundColor":t["hover"],"borderWidth":1,"borderColor":t["separator"],"justifyContent":"center","direction":"ltr","overflow":"hidden","flexShrink":0}),
            waveform,
        );
        wave["events"]["onMouseDown"] = json!(["waveDown"]);
        content.push(wave);
        let mut actions = vec![
            button(
                key(if app.playing {
                    "recorder.pause"
                } else {
                    "recorder.play"
                }),
                "play",
                true,
                app.play_start,
            ),
            button(key("recorder.rename"), "rename", false, app.exporting),
            button(
                key(if app.trim.is_some() {
                    "recorder.cancelTrim"
                } else {
                    "recorder.trim"
                }),
                "trim",
                false,
                false,
            ),
        ];
        if app.trim.is_some() {
            actions.push(button(
                key("recorder.saveTrim"),
                "saveTrim",
                true,
                app.exporting || app.wave.is_none(),
            ));
        }
        content.push(row("playback", actions));
        content.push(v(
            "rule",
            json!({"height":1,"backgroundColor":t["separator"],"marginVertical":4}),
            vec![],
        ));
        let mut export = vec![text(key("recorder.export"), 13., &t["text"])];
        for (fmt, label) in [("opus", "Opus"), ("mp3", "MP3"), ("wav", "WAV")] {
            export.push(option(
                json!(label),
                "format",
                json!(fmt),
                app.format == fmt,
                &t,
            ));
        }
        export.push(button(
            key("recorder.exportBtn"),
            "export",
            false,
            app.exporting,
        ));
        content.push(row("export", export));
    } else {
        content.push(empty(&t));
    }
    if !app.message.is_null() {
        content.push(text(app.message.clone(), 12., &t["accent"]));
    }
    // Keep the detail actions reachable at the minimum window height.
    let detail = json!({"kind":"scroll","key":"detail-scroll","style":{"flex":1,"minWidth":0},"children":[v("detail",json!({"padding":20,"gap":12,"minHeight":"100%"}),content)]});
    v(
        "recorder",
        json!({"flex":1,"flexDirection":"row","direction":if app.rtl{"rtl"}else{"ltr"}}),
        vec![sidebar, detail],
    )
}
