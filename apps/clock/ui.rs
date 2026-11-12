use super::{local_abbr, App};
use crate::clock;
use serde_json::{json, Value};
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn click(mut node: Value, cmd: &str, k: &str) -> Value {
    node["events"] = json!({"onClick":[cmd,k]});
    node
}
fn caption(label: Value, t: &Value) -> Value {
    text(
        label,
        json!({"fontSize":12,"color":t["textDim"],"textAlign":"start"}),
    )
}
fn hairline(edge: &str, t: &Value) -> Value {
    view(
        json!({"position":"absolute","left":0,"right":0,edge:0,"height":1,"backgroundColor":t["separator"]}),
        vec![],
    )
}
fn button(
    label: Value,
    cmd: &str,
    k: &str,
    primary: bool,
    plain: bool,
    app: &App,
    t: &Value,
) -> Value {
    let hover = app.hover == format!("{cmd}:{k}");
    let color = if primary {
        json!("#fff")
    } else {
        t["text"].clone()
    };
    let mut node = view(
        json!({"height":28,"paddingHorizontal":14,"borderRadius":t["controlRadius"],"flexDirection":"row","alignItems":"center","justifyContent":"center","gap":6,"backgroundColor":if primary{t["accent"].clone()}else if plain{if hover{t["hover"].clone()}else{json!("transparent")}}else{t["control"].clone()},"borderWidth":if t["buttonBorder"]==true&&!plain{1}else{0},"borderColor":if primary{t["accent"].clone()}else{t["separator"].clone()},"opacity":if hover&&!plain{0.85}else{1.}}),
        vec![text(
            label,
            json!({"fontSize":13,"fontWeight":500,"color":color,"whiteSpace":"nowrap"}),
        )],
    );
    node["events"] = json!({"onClick":[cmd,k],"onMouseEnter":["hover",format!("{cmd}:{k}")],"onMouseLeave":["hover",""]});
    node
}
fn face(size: f64, h: f64, m: f64, s: Option<f64>, dark: bool, accent: &Value) -> Value {
    let r = size / 2.;
    let fg = if dark { "#f2f2f7" } else { "#1c1c1e" };
    let mut children = vec![];
    for i in 0..12 {
        let a = i as f64 / 12. * std::f64::consts::TAU;
        let d = if i % 3 != 0 {
            size * 0.035
        } else {
            size * 0.06
        };
        children.push(view(json!({"position":"absolute","left":r+a.sin()*r*0.82-d/2.,"top":r-a.cos()*r*0.82-d/2.,"width":d,"height":d,"borderRadius":d/2.,"backgroundColor":fg,"opacity":if i%3!=0{0.45}else{0.9}}),vec![]));
    }
    let mut hand = |frac: f64, len: f64, w: f64, color: Value| {
        let a = frac * std::f64::consts::TAU;
        let n = ((len * r / (w * 0.7)).round() as usize).max(4);
        for k in 1..=n {
            let q = k as f64 / n as f64 * len * r;
            children.push(view(json!({"position":"absolute","left":r+a.sin()*q-w/2.,"top":r-a.cos()*q-w/2.,"width":w,"height":w,"borderRadius":w/2.,"backgroundColor":color}),vec![]));
        }
    };
    hand(
        (h % 12. + m / 60.) / 12.,
        0.48,
        (size * 0.07).max(2.),
        json!(fg),
    );
    hand(
        (m + s.unwrap_or(0.) / 60.) / 60.,
        0.7,
        (size * 0.05).max(1.5),
        json!(fg),
    );
    if let Some(s) = s {
        hand(s / 60., 0.76, (size * 0.025).max(1.), accent.clone());
    }
    children.push(view(json!({"position":"absolute","left":r-size*0.05,"top":r-size*0.05,"width":size*0.1,"height":size*0.1,"borderRadius":size*0.05,"backgroundColor":accent}),vec![]));
    view(
        json!({"width":size,"height":size,"borderRadius":r,"backgroundColor":if dark{"#2c2c2e"}else{"#ffffff"},"borderWidth":1,"borderColor":if dark{"#48484a"}else{"#d1d1d6"},"direction":"ltr"}),
        children,
    )
}
pub fn icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    view(
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(180deg, #2b2b30, #0c0c0e)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"alignItems":"center","justifyContent":"center"}),
        vec![face(s * 0.78, 10., 9., Some(36.), false, &json!("#ff9f0a"))],
    )
}
fn stepper(value: i64, field: &str, t: &Value) -> Value {
    view(
        json!({"flexDirection":"row","alignItems":"center","backgroundColor":t["control"],"borderRadius":t["controlRadius"],"height":26}),
        vec![
            click(
                view(
                    json!({"width":26,"height":26,"alignItems":"center","justifyContent":"center"}),
                    vec![text(
                        json!("−"),
                        json!({"fontSize":15,"fontWeight":600,"color":t["text"]}),
                    )],
                ),
                "step",
                &format!("{field}:-1"),
            ),
            text(
                json!(format!("{value:02}")),
                json!({"width":56,"textAlign":"center","fontSize":12,"fontWeight":600,"color":t["text"]}),
            ),
            click(
                view(
                    json!({"width":26,"height":26,"alignItems":"center","justifyContent":"center"}),
                    vec![text(
                        json!("+"),
                        json!({"fontSize":15,"fontWeight":600,"color":t["text"]}),
                    )],
                ),
                "step",
                &format!("{field}:1"),
            ),
        ],
    )
}
fn empty(icon: Value, title: Value, hint: Value, t: &Value) -> Value {
    view(
        json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":8,"padding":24}),
        vec![
            icon,
            text(
                title,
                json!({"fontSize":15,"fontWeight":600,"color":t["text"],"textAlign":"center"}),
            ),
            text(
                hint,
                json!({"fontSize":12,"color":t["textDim"],"textAlign":"center","maxWidth":360,"lineHeight":1.4}),
            ),
        ],
    )
}
fn toolbar(app: &App, t: &Value) -> Value {
    let small = app.width < 520.;
    let tabs=[("world","clock.tab.world"),("alarms","clock.tab.alarms"),("timer","clock.tab.timer"),("stopwatch","clock.tab.stopwatch")].into_iter().map(|(id,k)|{let on=app.tab==id;click(view(json!({"paddingHorizontal":if small{10}else{14},"height":if small{18}else{24},"borderRadius":(t[if small{"controlRadiusSmall"}else{"controlRadius"}].as_f64().unwrap_or(6.)-1.).max(2.),"justifyContent":"center","alignItems":"center","backgroundColor":if on{json!(if t["mode"]=="dark"{"rgba(255,255,255,0.18)"}else{"#ffffff"})}else{json!("transparent")},"boxShadow":if on{"0 1px 2px rgba(0,0,0,0.18)"}else{"none"}}),vec![text(key(k),json!({"fontSize":if small{11}else{12},"fontWeight":if on{600}else{500},"whiteSpace":"nowrap","color":t["text"]}))]),"tab",id)}).collect();
    let add = if app.tab == "world" || app.tab == "alarms" {
        click(
            view(
                json!({"height":28,"width":28,"alignItems":"center","justifyContent":"center","borderRadius":t["controlRadiusSmall"]}),
                vec![text(json!("+"), json!({"fontSize":20,"color":t["text"]}))],
            ),
            "add",
            "",
        )
    } else {
        Value::Null
    };
    view(
        json!({"height":44,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":12,"zIndex":10}),
        vec![
            view(json!({"flex":1}), vec![]),
            view(
                json!({"flexDirection":"row","backgroundColor":t["control"],"borderRadius":if small{t["controlRadiusSmall"].as_f64().unwrap_or(6.)}else{t["controlRadius"].as_f64().unwrap_or(7.)+1.},"padding":2}),
                tabs,
            ),
            view(
                json!({"flex":1,"flexDirection":"row","justifyContent":"flex-end"}),
                vec![add],
            ),
            hairline("bottom", t),
        ],
    )
}
fn city_row(app: &mut App, zone: Option<&str>, now: i64, t: &Value) -> Value {
    let here = clock::local(now);
    let (date, offset, abbr) = if let Some(z) = zone {
        let Some(v) = app.zone_time(z, now) else {
            return Value::Null;
        };
        v
    } else {
        (
            here,
            here.tm_gmtoff,
            app.offsets
                .get(&app.zone)
                .map(|(_, a)| a.clone())
                .unwrap_or_else(|| local_abbr(&here)),
        )
    };
    let day_diff = {
        let mut d = date;
        d.tm_hour = 0;
        d.tm_min = 0;
        d.tm_sec = 0;
        let a = unsafe { libc::timegm(&mut d) };
        let mut h = here;
        h.tm_hour = 0;
        h.tm_min = 0;
        h.tm_sec = 0;
        let b = unsafe { libc::timegm(&mut h) };
        ((a - b) as f64 / 86400.).round() as i64
    };
    let diff_h = (offset - here.tm_gmtoff) as f64 / 3600.;
    let diff = if diff_h == 0. {
        key("clock.world.same")
    } else {
        json!({"key":"clock.world.diff","params":{"h":format!("{}{}",if diff_h>0.{"+"}else{"−"},diff_h.abs().to_string().replace('.',&app.decimal))}})
    };
    let day = key(if day_diff == 0 {
        "clock.world.today"
    } else if day_diff > 0 {
        "clock.world.tomorrow"
    } else {
        "clock.world.yesterday"
    });
    let label = zone
        .map(App::city)
        .map(Value::String)
        .unwrap_or_else(|| key("clock.world.local"));
    let subtitle = if zone.is_none() {
        json!(format!(
            "{}{}",
            App::city(&app.zone),
            if abbr.is_empty() {
                String::new()
            } else {
                format!(" · {abbr}")
            }
        ))
    } else {
        let a = app.i18n_caption(&day);
        let b = app.i18n_caption(&diff);
        json!(format!(
            "{a}, {b}{}",
            if abbr.is_empty() || abbr.starts_with(['+', '-']) {
                String::new()
            } else {
                format!(" · {abbr}")
            }
        ))
    };
    let time = app.time(&date);
    let hover = app.hover == format!("city:{}", zone.unwrap_or("local"));
    let mut remove = view(
        json!({"width":22,"height":22,"borderRadius":11,"alignItems":"center","justifyContent":"center","backgroundColor":if hover&&zone.is_some(){t["control"].clone()}else{json!("transparent")}}),
        vec![text(
            json!("✕"),
            json!({"fontSize":11,"fontWeight":600,"color":if hover&&zone.is_some(){t["text"].clone()}else{json!("transparent")}}),
        )],
    );
    if let Some(z) = zone {
        remove["events"] = json!({"onClick":["zoneRemove",z]});
    }
    let mut node = view(
        json!({"flexDirection":"row","alignItems":"center","gap":16,"minHeight":68,"paddingHorizontal":12,"borderRadius":10,"backgroundColor":if hover{t["hover"].clone()}else{json!("transparent")}}),
        vec![
            face(
                44.,
                date.tm_hour as f64,
                date.tm_min as f64,
                Some(date.tm_sec as f64),
                t["mode"] == "dark" && !(7..19).contains(&date.tm_hour),
                &t["accent"],
            ),
            view(
                json!({"flex":1,"minWidth":0,"gap":2}),
                vec![
                    text(
                        label,
                        json!({"fontSize":15,"fontWeight":600,"color":t["text"],"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start"}),
                    ),
                    text(
                        subtitle,
                        json!({"fontSize":12,"color":t["textDim"],"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start"}),
                    ),
                ],
            ),
            text(
                json!(time),
                json!({"fontSize":28,"fontWeight":300,"color":t["text"],"whiteSpace":"nowrap","textAlign":"end"}),
            ),
            remove,
        ],
    );
    node["key"] = json!(zone.unwrap_or("local"));
    node["events"] = json!({"onMouseEnter":["hover",format!("city:{}",zone.unwrap_or("local"))],"onMouseLeave":["hover",""]});
    node
}
fn world(app: &mut App, now: i64, t: &Value, state: &Value) -> Value {
    let cities: Vec<String> = state["config"]["cities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let mut rows = vec![
        city_row(app, None, now, t),
        view(
            json!({"height":1,"marginHorizontal":12,"marginVertical":4}),
            vec![hairline("bottom", t)],
        ),
    ];
    for z in &cities {
        rows.push(city_row(app, Some(z), now, t));
    }
    let mut children = vec![view(
        json!({"paddingHorizontal":8,"paddingVertical":8}),
        rows,
    )];
    if cities.is_empty() {
        children.push(text(key("clock.world.empty"),json!({"fontSize":12,"color":t["textDim"],"textAlign":"center","lineHeight":1.4,"paddingHorizontal":24,"paddingTop":16})));
    }
    json!({"kind":"scroll","key":"world","style":{"flex":1},"children":children})
}
fn sheet(children: Vec<Value>, height: Option<i32>, t: &Value, gap: i32) -> Value {
    let mut style = json!({"width":380,"borderRadius":12,"backgroundColor":t["windowBg"],"borderWidth":1,"borderColor":t["separator"],"boxShadow":"0 12px 40px rgba(0,0,0,0.45)","padding":20,"gap":gap});
    if let Some(h) = height {
        style["height"] = json!(h);
    }
    view(
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"backgroundColor":"rgba(0,0,0,0.35)","alignItems":"center","justifyContent":"center","zIndex":20}),
        vec![view(style, children)],
    )
}
fn zone_picker(app: &App, t: &Value) -> Value {
    let list=app.zone_list().into_iter().map(|z|click(view(json!({"height":28,"flexDirection":"row","alignItems":"center","paddingHorizontal":8,"borderRadius":6}),vec![text(json!(App::city(&z)),json!({"flex":1,"fontSize":13,"color":t["text"],"textAlign":"start"})),text(json!(App::region(&z)),json!({"fontSize":11,"color":t["textDim"]}))]),"zonePick",&z)).collect::<Vec<_>>();
    sheet(
        vec![
            text(
                key("clock.world.add"),
                json!({"fontSize":15,"fontWeight":600,"color":t["text"],"textAlign":"start"}),
            ),
            json!({"kind":"input","key":"zone-query","value":app.query,"change":["query",""],"cancel":["dismiss",""],"submit":["zoneFirst",""],"placeholder":key("clock.world.search")}),
            view(
                json!({"flex":1,"borderRadius":8,"borderWidth":1,"borderColor":t["separator"]}),
                vec![json!({"kind":"scroll","style":{"flex":1,"padding":4},"children":list})],
            ),
            view(
                json!({"flexDirection":"row","justifyContent":"flex-end"}),
                vec![button(
                    key("clock.cancel"),
                    "dismiss",
                    "",
                    false,
                    false,
                    app,
                    t,
                )],
            ),
        ],
        Some(400),
        t,
        12,
    )
}
fn alarms(app: &mut App, now: i64, t: &Value, state: &Value) -> Value {
    let mut alarms = state["config"]["alarms"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    alarms.sort_by_key(|a| a["h"].as_i64().unwrap_or(0) * 60 + a["m"].as_i64().unwrap_or(0));
    if alarms.is_empty() {
        return empty(
            face(56., 7., 0., None, t["mode"] == "dark", &t["accent"]),
            key("clock.alarm.empty"),
            key("clock.alarm.closedHint"),
            t,
        );
    }
    let next = alarms.iter().filter_map(|a| clock::next_fire(a, now)).min();
    let note = next
        .map(|next| {
            let min = (((next - now) as f64 / 60000.).round() as i64).max(1);
            let when = if min >= 60 {
                json!({"key":"clock.inHours","params":{"h":min/60,"m":min%60}})
            } else {
                json!({"key":"clock.inMinutes","params":{"m":min}})
            };
            json!({"key":"clock.alarm.next","params":{"when":when}})
        })
        .unwrap_or_else(|| key("clock.alarm.none"));
    let mut rows = vec![text(
        note,
        json!({"fontSize":12,"color":t["textDim"],"paddingBottom":4,"textAlign":"start"}),
    )];
    for a in alarms {
        let id = a["id"].as_str().unwrap_or("");
        let mut date = clock::local(now);
        date.tm_hour = a["h"].as_i64().unwrap_or(7) as i32;
        date.tm_min = a["m"].as_i64().unwrap_or(0) as i32;
        let time = app.time(&date);
        let days = a["days"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_i64)
            .collect::<Vec<_>>();
        let day_text = app.days(&days);
        let day_text = app.i18n_caption(&day_text);
        let label = a["label"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| app.i18n_caption(&key("clock.alarm.default")));
        let snooze = a["snooze"].as_i64().filter(|s| *s > now).map(|s| {
            let time = app.time(&clock::local(s));
            app.i18n_caption(&json!({"key":"clock.alarm.snoozed","params":{"time":time}}))
        });
        let toggle = click(
            view(
                json!({"width":42,"height":24,"borderRadius":12,"direction":"ltr","backgroundColor":if a["on"]==true{t["accent"].clone()}else{t["control"].clone()}}),
                vec![view(
                    json!({"position":"absolute","top":2,"left":if a["on"]==true{20}else{2},"width":20,"height":20,"borderRadius":10,"backgroundColor":"#fff","boxShadow":"0 1px 3px rgba(0,0,0,0.35)"}),
                    vec![],
                )],
            ),
            "alarmToggle",
            id,
        );
        rows.push(view(json!({"flexDirection":"row","alignItems":"center","gap":12,"minHeight":72,"paddingVertical":8}),vec![click(view(json!({"flex":1,"minWidth":0,"gap":2}),vec![text(json!(time),json!({"fontSize":34,"fontWeight":300,"color":if a["on"]==true{t["text"].clone()}else{t["textDim"].clone()},"textAlign":"start"})),text(json!(format!("{label} · {day_text}{}",snooze.map(|s|format!(" · {s}")).unwrap_or_default())),json!({"fontSize":12,"color":t["textDim"],"whiteSpace":"nowrap","overflow":"hidden","textAlign":"start"}))]),"alarmEdit",id),toggle,hairline("bottom",t)]));
    }
    rows.push(text(key("clock.alarm.closedHint"),json!({"fontSize":11,"color":t["textDim"],"paddingTop":12,"lineHeight":1.4,"textAlign":"start"})));
    json!({"kind":"scroll","key":"alarms","style":{"flex":1},"children":[view(json!({"paddingHorizontal":20,"paddingTop":12,"paddingBottom":20}),rows)]})
}
fn alarm_editor(app: &mut App, t: &Value, now: i64) -> Value {
    let Some(e) = app.editing.as_ref() else {
        return Value::Null;
    };
    let (id, h, m, days, label) = (e.id.clone(), e.h, e.m, e.days.clone(), e.label.clone());
    let mut date = clock::local(now);
    date.tm_hour = h as i32;
    date.tm_min = m as i32;
    let time = app.time(&date);
    let day_buttons=[1,2,3,4,5,6,0].into_iter().map(|d|{let on=days.contains(&d);click(view(json!({"width":40,"height":30,"borderRadius":8,"alignItems":"center","justifyContent":"center","backgroundColor":if on{t["accent"].clone()}else{t["control"].clone()}}),vec![text(json!(app.weekday(d)),json!({"fontSize":12,"fontWeight":600,"color":if on{json!("#fff")}else{t["text"].clone()}}))]),"day",&d.to_string())}).collect();
    let day_text = app.days(&days);
    let mut actions = vec![];
    if !id.is_empty() {
        actions.push(button(
            key("clock.alarm.delete"),
            "alarmDelete",
            "",
            false,
            false,
            app,
            t,
        ));
    }
    actions.extend([
        view(json!({"flex":1}), vec![]),
        button(key("clock.cancel"), "dismiss", "", false, false, app, t),
        button(key("clock.save"), "alarmSave", "", true, false, app, t),
    ]);
    sheet(
        vec![
            text(
                key(if id.is_empty() {
                    "clock.alarm.new"
                } else {
                    "clock.alarm.edit"
                }),
                json!({"fontSize":15,"fontWeight":600,"color":t["text"],"textAlign":"start"}),
            ),
            view(
                json!({"flexDirection":"row","alignItems":"center","justifyContent":"center","gap":10,"direction":"ltr"}),
                vec![
                    stepper(h, "h", t),
                    text(json!(":"), json!({"fontSize":22,"color":t["text"]})),
                    stepper(m, "m", t),
                ],
            ),
            text(
                json!(time),
                json!({"fontSize":28,"fontWeight":300,"color":t["text"],"textAlign":"center"}),
            ),
            view(
                json!({"gap":6}),
                vec![
                    caption(key("clock.alarm.repeat"), t),
                    view(
                        json!({"flexDirection":"row","gap":6,"justifyContent":"center"}),
                        day_buttons,
                    ),
                    text(
                        day_text,
                        json!({"fontSize":11,"color":t["textDim"],"textAlign":"center"}),
                    ),
                ],
            ),
            json!({"kind":"input","key":"alarm-label","value":label,"change":["label",""],"submit":["alarmSave",""],"placeholder":key("clock.alarm.label")}),
            view(json!({"flexDirection":"row","gap":8}), actions),
        ],
        None,
        t,
        16,
    )
}
fn timer(app: &mut App, now: i64, t: &Value, state: &Value) -> Value {
    let timer = &state["timer"];
    if timer["state"] == "idle" {
        let fields = ["clock.timer.h", "clock.timer.m", "clock.timer.s"]
            .into_iter()
            .enumerate()
            .map(|(i, k)| {
                view(
                    json!({"alignItems":"center","gap":4}),
                    vec![
                        text(key(k), json!({"fontSize":11,"color":t["textDim"]})),
                        stepper(app.timer_fields[i], &i.to_string(), t),
                    ],
                )
            })
            .collect();
        let presets=[1,3,5,10,15,30].into_iter().map(|p|click(view(json!({"height":28,"paddingHorizontal":12,"borderRadius":14,"justifyContent":"center","backgroundColor":t["control"]}),vec![text(json!({"key":"clock.timer.preset","params":{"n":p}}),json!({"fontSize":12,"color":t["text"]}))]),"preset",&p.to_string())).collect();
        return view(
            json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":18}),
            vec![
                view(
                    json!({"flexDirection":"row","alignItems":"flex-end","gap":12,"direction":"ltr"}),
                    fields,
                ),
                view(
                    json!({"flexDirection":"row","gap":6,"flexWrap":"wrap","justifyContent":"center","maxWidth":420}),
                    presets,
                ),
                button(
                    key("clock.timer.start"),
                    "timerStart",
                    "",
                    true,
                    false,
                    app,
                    t,
                ),
                text(
                    key("clock.timer.hint"),
                    json!({"fontSize":11,"color":t["textDim"],"maxWidth":380,"textAlign":"center","lineHeight":1.4}),
                ),
            ],
        );
    }
    let left = clock::timer_left(timer, now);
    let duration = timer["duration"].as_i64().unwrap_or(0);
    let frac = if duration > 0 {
        (left as f64 / duration as f64).clamp(0., 1.)
    } else {
        0.
    };
    let paused = timer["state"] == "paused";
    let note = if paused {
        key("clock.timer.paused")
    } else {
        let time = app.time(&clock::local(timer["endsAt"].as_i64().unwrap_or(now)));
        json!({"key":"clock.timer.endsAt","params":{"time":time}})
    };
    view(
        json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":20}),
        vec![
            text(
                json!(clock::duration(left + 999, false, &app.decimal)),
                json!({"fontSize":64,"fontWeight":200,"color":t["text"],"fontFamily":"monospace"}),
            ),
            view(
                json!({"width":320,"height":6,"borderRadius":3,"backgroundColor":t["control"],"direction":"ltr"}),
                vec![view(
                    json!({"width":(320.*frac).round(),"height":6,"borderRadius":3,"backgroundColor":t["accent"]}),
                    vec![],
                )],
            ),
            text(note, json!({"fontSize":12,"color":t["textDim"]})),
            view(
                json!({"flexDirection":"row","gap":10}),
                vec![
                    button(key("clock.cancel"), "timerReset", "", false, false, app, t),
                    button(
                        key(if paused {
                            "clock.timer.resume"
                        } else {
                            "clock.timer.pause"
                        }),
                        if paused { "timerResume" } else { "timerPause" },
                        "",
                        true,
                        false,
                        app,
                        t,
                    ),
                ],
            ),
        ],
    )
}
fn stopwatch(app: &App, now: i64, t: &Value, state: &Value) -> Value {
    let st = &state["stopwatch"];
    let running = st["running"] == true;
    let laps = st["laps"].as_array().cloned().unwrap_or_default();
    let splits: Vec<_> = laps
        .iter()
        .enumerate()
        .map(|(i, v)| {
            v.as_i64().unwrap_or(0) - laps.get(i + 1).and_then(Value::as_i64).unwrap_or(0)
        })
        .collect();
    let best = (splits.len() > 1)
        .then(|| splits.iter().min().copied())
        .flatten();
    let worst = (splits.len() > 1)
        .then(|| splits.iter().max().copied())
        .flatten();
    let rows=laps.iter().enumerate().map(|(i,v)|{let split=splits[i];let color=if Some(split)==best{json!("#30d158")}else if Some(split)==worst{json!("#ff453a")}else{t["text"].clone()};view(json!({"flexDirection":"row","alignItems":"center","minHeight":32,"paddingHorizontal":4}),vec![hairline("bottom",t),text(json!({"key":"clock.sw.lapN","params":{"n":laps.len()-i}}),json!({"flex":1,"fontSize":13,"color":color,"textAlign":"start"})),text(json!(clock::duration(split,true,&app.decimal)),json!({"width":120,"fontSize":13,"color":color,"fontFamily":"monospace","textAlign":"end"})),text(json!(clock::duration(v.as_i64().unwrap_or(0),true,&app.decimal)),json!({"width":120,"fontSize":13,"color":t["textDim"],"fontFamily":"monospace","textAlign":"end"}))])}).collect::<Vec<_>>();
    view(
        json!({"flex":1,"alignItems":"center","paddingTop":24,"gap":20}),
        vec![
            text(
                json!(clock::duration(clock::elapsed(st, now), true, &app.decimal)),
                json!({"fontSize":60,"fontWeight":200,"color":t["text"],"fontFamily":"monospace"}),
            ),
            view(
                json!({"flexDirection":"row","gap":10}),
                vec![
                    button(
                        key(if running {
                            "clock.sw.lap"
                        } else {
                            "clock.sw.reset"
                        }),
                        if running { "swLap" } else { "swReset" },
                        "",
                        false,
                        false,
                        app,
                        t,
                    ),
                    button(
                        key(if running {
                            "clock.sw.stop"
                        } else {
                            "clock.sw.start"
                        }),
                        if running { "swStop" } else { "swStart" },
                        "",
                        true,
                        false,
                        app,
                        t,
                    ),
                ],
            ),
            json!({"kind":"scroll","key":"laps","style":{"alignSelf":"stretch","flex":1,"paddingHorizontal":24},"children":rows}),
        ],
    )
}
pub fn tree(app: &mut App, now: i64) -> Value {
    let t = app.theme.clone();
    let state = clock::with(|s| json!({"config":s.config,"timer":s.timer,"stopwatch":s.stopwatch}));
    let body = match app.tab.as_str() {
        "alarms" => alarms(app, now, &t, &state),
        "timer" => timer(app, now, &t, &state),
        "stopwatch" => stopwatch(app, now, &t, &state),
        _ => world(app, now, &t, &state),
    };
    let mut children = vec![toolbar(app, &t), body];
    if app.tab == "world" && app.picking {
        children.push(zone_picker(app, &t));
    }
    if app.tab == "alarms" && app.editing.is_some() {
        children.push(alarm_editor(app, &t, now));
    }
    view(json!({"flex":1}), children)
}
