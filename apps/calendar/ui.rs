//! Calendar scenes and controls. All actions are selected and reduced in Rust.
use super::*;
fn v(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
fn label(k: &str) -> Value {
    json!({"key":format!("calendar.{k}")})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn token(a: &App, k: &str, f: &str) -> String {
    a.theme[k].as_str().unwrap_or(f).into()
}
fn fg(a: &App) -> String {
    token(a, "text", "#eeeeee")
}
fn dim(a: &App) -> String {
    token(a, "textDim", "#999999")
}
fn accent(a: &App) -> String {
    token(a, "accent", "#007aff")
}
fn click(mut n: Value, action: Value) -> Value {
    n["events"]["onClick"] = action;
    n
}
fn spacer() -> Value {
    v("spacer", json!({"flex":1}), vec![])
}
fn button(k: &str, action: Value, disabled: bool) -> Value {
    json!({"kind":"button","key":format!("button-{k}"),"label":label(k),"small":true,"disabled":disabled,"action":action})
}
fn input(
    key: &str,
    value: &str,
    placeholder: Value,
    change: Value,
    submit: &str,
    auto: bool,
    busy: bool,
) -> Value {
    json!({"kind":"input","key":key,"value":value,"placeholder":placeholder,"change":if busy{json!(["noop"])}else{change},"submit":[submit],"cancel":["close"],"autoFocus":auto,"size":13,"style":{"height":26,"opacity":if busy{0.5}else{1.}}})
}
fn tool(a: &App, key: &str, glyph: &str, action: Value, active: bool, disabled: bool) -> Value {
    let mut n = click(
        v(
            key,
            json!({"width":28,"height":28,"borderRadius":6,"alignItems":"center","justifyContent":"center","opacity":if disabled{0.4}else{1.},"backgroundColor":if active{token(a,"hover","rgba(127,127,127,0.12)")}else{"transparent".into()}}),
            vec![text(
                json!(glyph),
                json!({"fontSize":18,"color":if active{accent(a)}else{fg(a)}}),
            )],
        ),
        if disabled { json!(["noop"]) } else { action },
    );
    n["tooltip"] = label(key);
    n["hoverStyle"] = json!({"backgroundColor":token(a,"hover","rgba(127,127,127,0.12)")});
    n
}
fn border(a: &App) -> String {
    token(a, "separator", "#555555")
}
fn event_title(e: &Value) -> Value {
    if s(&e["title"]).is_empty() {
        label("untitled")
    } else {
        e["title"].clone()
    }
}
fn direction(s: &str) -> &'static str {
    for c in s.chars() {
        if c.is_alphabetic() {
            return if matches!(c as u32,0x0590..=0x08ff|0xfb1d..=0xfdff|0xfe70..=0xfeff) {
                "rtl"
            } else {
                "ltr"
            };
        }
    }
    "ltr"
}
pub fn icon(size: f64, now: i64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    v("calendar-icon",json!({"width":s,"height":s,"borderRadius":s*0.225,"backgroundColor":"#ffffff","overflow":"hidden","borderWidth":1,"borderColor":"rgba(0,0,0,0.08)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12)}),vec![v("red-strip",json!({"height":s*0.27,"backgroundColor":"#f2463d","alignItems":"center","justifyContent":"center"}),vec![v("dots",json!({"flexDirection":"row","gap":s*0.05}),(0..3).map(|i|v(&i.to_string(),json!({"width":s*0.07,"height":s*0.07,"borderRadius":s*0.035,"backgroundColor":"rgba(255,255,255,0.85)"}),vec![])).collect())]),v("day",json!({"flex":1,"alignItems":"center","justifyContent":"center"}),vec![text(json!(crate::clock::local(now).tm_mday.to_string()),json!({"fontSize":s*0.46,"fontWeight":300,"color":"#1c1c1e"}))])])
}
fn mini_month(a: &mut App, month: i64, selected: i64, now: i64, where_: &str, cell: f64) -> Value {
    let title = a.month_title(month);
    let mut head = vec![text(
        json!(title),
        json!({"flex":1,"fontSize":12,"fontWeight":600,"color":fg(a),"whiteSpace":"nowrap","overflow":"hidden"}),
    )];
    for (delta, glyph) in [
        (json!(-1), if a.rtl { "›" } else { "‹" }),
        (json!("today"), "●"),
        (json!(1), if a.rtl { "‹" } else { "›" }),
    ] {
        head.push(click(
            v(
                &delta.to_string(),
                json!({"width":20,"height":20,"alignItems":"center","justifyContent":"center"}),
                vec![text(
                    json!(glyph),
                    json!({"fontSize":if glyph=="●"{7}else{15},"color":dim(a)}),
                )],
            ),
            json!(["month",{"where":where_,"delta":delta}]),
        ));
    }
    let names=(0..7).map(|i|{let name=a.weekday((i+1)%7);text(json!(name.chars().next().map(|c|c.to_string()).unwrap_or_default()),json!({"width":cell,"textAlign":"center","fontSize":10,"fontWeight":600,"color":dim(a)}))}).collect();
    let mut children = vec![
        v(
            "heading",
            json!({"height":24,"flexDirection":"row","alignItems":"center"}),
            head,
        ),
        v("weekdays", json!({"flexDirection":"row"}), names),
    ];
    let first = date::week(month);
    let tm = crate::clock::local(month);
    for r in 0..6 {
        let mut cells = vec![];
        for c in 0..7 {
            let day = date::add_days(first, r * 7 + c);
            let d = crate::clock::local(day);
            let sel = date::ymd(day) == date::ymd(selected);
            let today = date::ymd(day) == date::ymd(now);
            let mut parts = vec![v(
                "circle",
                json!({"width":cell-4.,"height":cell-4.,"borderRadius":(cell-4.)/2.,"alignItems":"center","justifyContent":"center","backgroundColor":if sel{if today{"#ff3b30".into()}else{accent(a)}}else{"transparent".into()}}),
                vec![text(
                    json!(d.tm_mday.to_string()),
                    json!({"fontSize":11,"fontWeight":if today||sel{600}else{400},"color":if sel{"#ffffff".into()}else if today{"#ff3b30".into()}else if d.tm_mon==tm.tm_mon{fg(a)}else{dim(a)}}),
                )],
            )];
            if where_ == "side" && !sel && a.state.days.contains_key(&date::ymd(day)) {
                parts.push(v("mark",json!({"position":"absolute","bottom":1,"width":3,"height":3,"borderRadius":2,"backgroundColor":dim(a)}),vec![]));
            }
            cells.push(click(v(&date::ymd(day),json!({"width":cell,"height":cell,"alignItems":"center","justifyContent":"center"}),parts),if where_=="side"{json!(["cursor",day])}else{json!(["pick",{"field":a.picker,"value":day}])}));
        }
        children.push(v(&r.to_string(), json!({"flexDirection":"row"}), cells));
    }
    v("mini-month", json!({"width":cell*7.,"gap":2}), children)
}
fn sidebar(a: &mut App, now: i64) -> Value {
    let cals = a.state.event_cals();
    let mut groups: Vec<(String, String, Vec<Value>)> = vec![];
    for c in cals {
        let account = s(&c["acct"]);
        if let Some(g) = groups.iter_mut().find(|g| g.0 == account) {
            g.2.push(c);
        } else {
            let name = if account.is_empty() {
                a.tr("onThisComputer")
            } else {
                c["acctName"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(account)
                    .into()
            };
            groups.push((account.into(), name, vec![c]));
        }
    }
    groups.sort_by(|a, b| {
        a.0.is_empty()
            .cmp(&b.0.is_empty())
            .reverse()
            .then(a.1.cmp(&b.1))
    });
    let mut rows = vec![];
    for (id, name, cals) in groups {
        let mut contents = vec![text(
            json!(name),
            json!({"fontSize":11,"fontWeight":600,"color":dim(a),"marginHorizontal":8,"marginBottom":2,"whiteSpace":"nowrap","overflow":"hidden"}),
        )];
        for c in cals {
            let color = c["color"].as_str().unwrap_or(COLORS[5]);
            let checked = c["hidden"] != true;
            let check = v(
                "check",
                json!({"width":14,"height":14,"borderRadius":4,"alignItems":"center","justifyContent":"center","borderWidth":1.5,"borderColor":color,"backgroundColor":if checked{color}else{"transparent"}}),
                if checked {
                    vec![text(
                        json!("✓"),
                        json!({"fontSize":10,"fontWeight":700,"color":"#ffffff"}),
                    )]
                } else {
                    vec![]
                },
            );
            let mut n = click(
                v(
                    &format!("cal-{}", s(&c["id"])),
                    json!({"height":28,"paddingHorizontal":8,"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":8}),
                    vec![
                        check,
                        text(
                            c["name"].clone(),
                            json!({"flex":1,"fontSize":13,"color":fg(a),"whiteSpace":"nowrap","overflow":"hidden"}),
                        ),
                    ],
                ),
                json!(["calToggle", c["id"]]),
            );
            n["events"]["onContextMenu"] = json!(["menu", c["id"]]);
            n["hoverStyle"] = json!({"backgroundColor":token(a,"hover","rgba(127,127,127,0.12)")});
            contents.push(n);
        }
        rows.push(v(&id, json!({"gap":2,"marginBottom":14}), contents));
    }
    if rows.is_empty() {
        rows.push(text(
            label("noCalendars"),
            json!({"fontSize":12,"color":dim(a),"margin":6}),
        ));
    }
    let (month, selected) = (a.side_month, a.state.cursor);
    let mini = mini_month(a, month, selected, now, "side", 26.);
    v(
        "sidebar",
        json!({"width":216,"backgroundColor":token(a,"sidebarBg","#202024"),"borderEndWidth":1,"borderColor":border(a)}),
        vec![
            json!({"kind":"scroll","key":"calendars-scroll","style":{"flex":1},"children":[v("calendars",json!({"paddingHorizontal":10,"paddingTop":12,"paddingBottom":10,"gap":2}),rows)]}),
            v(
                "mini",
                json!({"paddingHorizontal":10,"paddingTop":12,"alignItems":"center","borderTopWidth":1,"borderColor":border(a)}),
                vec![mini],
            ),
            v(
                "footer",
                json!({"height":40,"paddingHorizontal":10,"justifyContent":"center","alignItems":"flex-start","borderTopWidth":1,"borderColor":border(a)}),
                vec![button("newCalendar", json!(["calNew"]), false)],
            ),
        ],
    )
}
fn all_day_chip(a: &App, e: &Value) -> Value {
    click(
        v(
            &key(e),
            json!({"height":18,"borderRadius":4,"paddingHorizontal":6,"justifyContent":"center","backgroundColor":color(e,&a.state.cals)}),
            vec![text(
                event_title(e),
                json!({"fontSize":11,"fontWeight":600,"color":"#ffffff","whiteSpace":"nowrap","overflow":"hidden","direction":direction(s(&e["title"]))}),
            )],
        ),
        json!(["event", key(e)]),
    )
}
fn timed_chip(a: &mut App, e: &Value) -> Value {
    let time = a.time(num(&e["start"]));
    click(
        v(
            &key(e),
            json!({"height":16,"flexDirection":"row","alignItems":"center","gap":4,"paddingHorizontal":3}),
            vec![
                v(
                    "color",
                    json!({"width":7,"height":7,"borderRadius":4,"backgroundColor":color(e,&a.state.cals)}),
                    vec![],
                ),
                text(
                    event_title(e),
                    json!({"flex":1,"fontSize":11,"color":fg(a),"whiteSpace":"nowrap","overflow":"hidden","direction":direction(s(&e["title"]))}),
                ),
                text(
                    json!(time),
                    json!({"fontSize":10,"color":dim(a),"whiteSpace":"nowrap"}),
                ),
            ],
        ),
        json!(["event", key(e)]),
    )
}
fn month_grid(a: &mut App, now: i64) -> Value {
    let cursor = crate::clock::local(a.state.cursor);
    let first = date::week(date::month(a.state.cursor, 0));
    let cell_h = ((a.height - 42. - if a.width < 760. { 42. } else { 50. } - 26.) / 6.).max(1.);
    let fit = ((cell_h - 28.) / 18.).floor().max(1.) as usize;
    let names=(0..7).map(|i|{let name=a.weekday((i+1)%7);text(json!(name),json!({"flex":1,"fontSize":11,"fontWeight":600,"color":dim(a),"textAlign":"end","paddingHorizontal":8}))}).collect();
    let mut rows = vec![];
    for r in 0..6 {
        let mut cells = vec![];
        for c in 0..7 {
            let day = date::add_days(first, r * 7 + c);
            let d = crate::clock::local(day);
            let today = date::ymd(day) == date::ymd(now);
            let in_month = d.tm_mon == cursor.tm_mon;
            let indices = a
                .state
                .days
                .get(&date::ymd(day))
                .cloned()
                .unwrap_or_default();
            let count = indices.len();
            let show = if count > fit {
                fit.saturating_sub(1)
            } else {
                count
            };
            let mut children = vec![v(
                "number-row",
                json!({"flexDirection":"row","justifyContent":"flex-end"}),
                vec![click(
                    v(
                        "number",
                        json!({"minWidth":22,"height":22,"paddingHorizontal":4,"borderRadius":11,"alignItems":"center","justifyContent":"center","backgroundColor":if today{"#ff3b30"}else{"transparent"}}),
                        vec![text(
                            json!(d.tm_mday.to_string()),
                            json!({"fontSize":12,"fontWeight":if today{700}else{500},"color":if today{"#ffffff".into()}else if in_month{fg(a)}else{dim(a)}}),
                        )],
                    ),
                    json!(["day", day]),
                )],
            )];
            for i in indices.iter().take(show) {
                let e = a.state.events[*i].clone();
                children.push(if e["allDay"] == true {
                    all_day_chip(a, &e)
                } else {
                    timed_chip(a, &e)
                });
            }
            if count > show {
                children.push(click(
                    v(
                        "more",
                        json!({"marginHorizontal":4}),
                        vec![text(
                            json!({"key":"calendar.more","params":{"count":count-show}}),
                            json!({"fontSize":10,"fontWeight":600,"color":dim(a)}),
                        )],
                    ),
                    json!(["day", day]),
                ));
            }
            cells.push(click(v(&date::ymd(day),json!({"flex":1,"padding":3,"gap":2,"overflow":"hidden","borderTopWidth":1,"borderStartWidth":if c>0{1}else{0},"borderColor":border(a),"backgroundColor":if in_month{"transparent"}else if a.theme["mode"]=="dark"{"rgba(255,255,255,0.02)"}else{"rgba(0,0,0,0.025)"}}),children),json!(["new",{"day":day,"min":540}])));
        }
        rows.push(v(
            &format!("week-{r}"),
            json!({"flex":1,"flexDirection":"row"}),
            cells,
        ));
    }
    v(
        "month-grid",
        json!({"flex":1}),
        vec![
            v(
                "weekdays",
                json!({"height":26,"flexDirection":"row","alignItems":"center"}),
                names,
            ),
            v("weeks", json!({"flex":1}), rows),
        ],
    )
}
fn event_block(a: &mut App, seg: &Segment, col_w: f64) -> Value {
    let e = a.state.events[seg.index].clone();
    let w = (col_w - 4.) / seg.cols.max(1) as f64;
    let left = if a.rtl {
        col_w - 2. - (seg.col + 1) as f64 * w
    } else {
        2. + seg.col as f64 * w
    };
    let height = ((seg.bottom - seg.top) * HOUR / 60. - 2.).max(18.);
    let color = color(&e, &a.state.cals);
    let mut children = vec![
        v(
            "stripe",
            json!({"position":"absolute","top":0,"bottom":0,"width":3,"backgroundColor":color}),
            vec![],
        ),
        text(
            event_title(&e),
            json!({"fontSize":11,"fontWeight":600,"color":fg(a),"whiteSpace":if height<34.||w-1.<72.{json!("nowrap")}else{Value::Null},"overflow":"hidden","direction":direction(s(&e["title"]))}),
        ),
    ];
    children[0]["style"][if a.rtl { "right" } else { "left" }] = json!(0);
    if height >= 34. {
        let time = a.time(num(&e["start"]));
        children.push(text(json!(if s(&e["loc"]).is_empty(){time}else{format!("{time} · {}",s(&e["loc"]))}),json!({"fontSize":10,"color":dim(a),"whiteSpace":"nowrap","overflow":"hidden","direction":direction(if s(&e["loc"]).is_empty(){s(&e["title"])}else{s(&e["loc"])})})));
    }
    click(
        v(
            &key(&e),
            json!({"position":"absolute","top":seg.top*HOUR/60.+1.,"left":left,"width":(w-1.).max(8.),"height":height,"borderRadius":4,"overflow":"hidden","backgroundColor":tint(&color,a.theme["mode"]=="dark"),"paddingVertical":2,"paddingLeft":if a.rtl{4}else{7},"paddingRight":if a.rtl{7}else{4}}),
            children,
        ),
        json!(["event", key(&e)]),
    )
}
fn time_grid(a: &mut App, now: i64) -> Value {
    let single = a.state.view == "day";
    let first = if single {
        date::day(a.state.cursor)
    } else {
        date::week(a.state.cursor)
    };
    let dates: Vec<_> = (0..if single { 1 } else { 7 })
        .map(|i| date::add_days(first, i))
        .collect();
    let col_w = (a.width - if a.show_sidebar() { 216. } else { 0. } - GUTTER) / dates.len() as f64;
    let stacked = !single && col_w < 80.;
    let mut headers = vec![v("gutter", json!({"width":GUTTER}), vec![])];
    let mut allday = vec![v(
        "all-day-label",
        json!({"width":GUTTER,"justifyContent":"center","paddingVertical":3,"paddingHorizontal":8}),
        vec![text(
            label("allDay"),
            json!({"fontSize":10,"lineHeight":12,"color":dim(a),"textAlign":"end"}),
        )],
    )];
    let mut columns = vec![];
    let mut max_all = 1usize;
    for day in &dates {
        let tm = crate::clock::local(*day);
        let today = date::ymd(*day) == date::ymd(now);
        let name = a.weekday(tm.tm_wday as usize);
        headers.push(click(v(&date::ymd(*day),json!({"flex":1,"flexDirection":if stacked{"column"}else{"row"},"alignItems":"center","justifyContent":"center","gap":if stacked{1}else{5},"overflow":"hidden"}),vec![text(json!(name),json!({"fontSize":if stacked{10}else{12},"fontWeight":if stacked{600}else{400},"color":if today{"#ff3b30".into()}else{dim(a)},"whiteSpace":"nowrap"})),v("number",json!({"minWidth":24,"height":24,"paddingHorizontal":4,"borderRadius":12,"alignItems":"center","justifyContent":"center","backgroundColor":if today{"#ff3b30"}else{"transparent"}}),vec![text(json!(tm.tm_mday.to_string()),json!({"fontSize":15,"fontWeight":if today{600}else{500},"color":if today{"#ffffff".into()}else{fg(a)}}))])]),if single{json!(["noop"])}else{json!(["day",day])}));
        let indices = a
            .state
            .days
            .get(&date::ymd(*day))
            .cloned()
            .unwrap_or_default();
        let all: Vec<_> = indices
            .iter()
            .copied()
            .filter(|i| a.state.events[*i]["allDay"] == true)
            .collect();
        max_all = max_all.max(all.len());
        let mut chips = all
            .iter()
            .take(4)
            .map(|i| all_day_chip(a, &a.state.events[*i]))
            .collect::<Vec<_>>();
        if all.len() > 4 {
            chips.push(text(
                json!({"key":"calendar.more","params":{"count":all.len()-4}}),
                json!({"fontSize":10,"color":dim(a),"marginHorizontal":4}),
            ));
        }
        allday.push(click(v(&date::ymd(*day),json!({"flex":1,"padding":2,"gap":2,"overflow":"hidden","borderStartWidth":1,"borderColor":border(a)}),chips),json!(["new",{"day":day}])));
        let mut content = vec![];
        for half in 0..48 {
            content.push(click(v(&format!("slot-{half}"),json!({"position":"absolute","left":0,"right":0,"top":half as f64*HOUR/2.,"height":HOUR/2.,"borderTopWidth":if half%2==0{1}else{0},"borderColor":border(a)}),vec![]),json!(["new",{"day":day,"min":half*30}])));
        }
        for seg in layout(*day, &a.state.events, &indices) {
            content.push(event_block(a, &seg, col_w));
        }
        if today {
            let mut line = v(
                "now",
                json!({"position":"absolute","left":0,"right":0,"top":date::minutes(now) as f64*HOUR/60.-1.,"height":2,"backgroundColor":"#ff3b30","pointerEvents":"none"}),
                vec![v(
                    "dot",
                    json!({"position":"absolute","top":-3,"width":8,"height":8,"borderRadius":4,"backgroundColor":"#ff3b30"}),
                    vec![],
                )],
            );
            line["children"][0]["style"][if a.rtl { "right" } else { "left" }] = json!(-4);
            content.push(line);
        }
        columns.push(v(
            &date::ymd(*day),
            json!({"flex":1,"borderStartWidth":1,"borderColor":border(a)}),
            content,
        ));
    }
    let times=(1..24).map(|hour|{let time=a.time(date::at_minutes(date::day(now),hour*60));text(json!(time),json!({"position":"absolute","top":hour as f64*HOUR-7.,"left":0,"right":0,"paddingHorizontal":8,"fontSize":10,"color":dim(a),"textAlign":"end","whiteSpace":"nowrap"}))}).collect();
    v(
        "time-grid",
        json!({"flex":1}),
        vec![
            v(
                "day-headers",
                json!({"height":if single{32}else if stacked{46}else{36},"flexDirection":"row","borderBottomWidth":1,"borderColor":border(a)}),
                headers,
            ),
            v(
                "all-day",
                json!({"height":(max_all.min(4)*20+2+if max_all>4{12}else{0}).max(30),"flexDirection":"row","borderBottomWidth":1,"borderColor":border(a)}),
                allday,
            ),
            json!({"kind":"scroll","key":format!("hours-{}",a.state.view),"initialOffset":8.*HOUR-16.,"style":{"flex":1},"children":[v("hours",json!({"height":24.*HOUR+8.,"flexDirection":"row"}),vec![v("time-labels",json!({"width":GUTTER}),times),v("days",json!({"flex":1,"flexDirection":"row"}),columns)])]}),
        ],
    )
}
fn choice(a: &App, field: &str, title: Value, width: f64, busy: bool) -> Value {
    click(
        v(
            &format!("choice-{field}"),
            json!({"width":width,"height":26,"paddingHorizontal":9,"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":6,"backgroundColor":token(a,"control","#3a3a3f"),"opacity":if busy{0.5}else{1.}}),
            vec![
                text(
                    title,
                    json!({"flex":1,"fontSize":13,"color":fg(a),"whiteSpace":"nowrap","overflow":"hidden"}),
                ),
                text(json!("▾"), json!({"fontSize":9,"color":dim(a)})),
            ],
        ),
        if busy {
            json!(["noop"])
        } else {
            json!(["picker", field])
        },
    )
}
fn field(a: &App, k: &str, children: Vec<Value>) -> Value {
    v(
        k,
        json!({"flexDirection":"row","alignItems":"center","gap":10,"minHeight":28}),
        vec![
            text(
                if k.is_empty() { json!("") } else { label(k) },
                json!({"width":70,"fontSize":12,"color":dim(a),"textAlign":"end"}),
            ),
            v(
                "control",
                json!({"flex":1,"flexDirection":"row","alignItems":"center","gap":8,"flexWrap":"wrap"}),
                children,
            ),
        ],
    )
}
fn alert_title(alarm: i64) -> Value {
    match alarm {
        -1 => label("alert.none"),
        0 => label("alert.atTime"),
        60 => label("alert.hourBefore"),
        1440 => label("alert.dayBefore"),
        _ => json!({"key":"calendar.alert.minutes","params":{"count":alarm}}),
    }
}
fn repeat_title(rule: &str) -> Value {
    if rule.is_empty() {
        label("repeat.none")
    } else if let Some(freq) = FREQS.iter().find(|f| rule == format!("FREQ={f}")) {
        label(&format!("repeat.{}", freq.to_lowercase()))
    } else {
        label("repeat.custom")
    }
}
fn sheet(a: &App, key: &str, width: f64, body: Vec<Value>, footer: Vec<Value>) -> Value {
    v(
        &format!("{key}-layer"),
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":100,"alignItems":"center","justifyContent":"center","backgroundColor":"rgba(0,0,0,0.3)"}),
        vec![v(
            key,
            json!({"width":width,"maxWidth":"94%","maxHeight":"94%","borderRadius":12,"backgroundColor":token(a,"windowBg","#252529"),"borderWidth":1,"borderColor":border(a),"boxShadow":"0 18px 50px rgba(0,0,0,0.45)","overflow":"hidden"}),
            vec![
                v(
                    "body-height",
                    json!({"height":(a.height*0.94-58.).min(if key=="event-sheet"{530.}else{160.}).max(80.)}),
                    vec![
                        json!({"kind":"scroll","key":"sheet-scroll","style":{"flex":1},"children":[v("body",json!({"padding":20,"gap":10}),body)]}),
                    ],
                ),
                v(
                    "footer",
                    json!({"height":48,"paddingHorizontal":16,"flexDirection":"row","alignItems":"center","gap":8}),
                    footer,
                ),
            ],
        )],
    )
}
fn event_sheet(a: &mut App, now: i64) -> Value {
    let e = a.state.event.as_ref().unwrap();
    let original = e.original.clone();
    let d = e.draft.clone();
    let new = e.new;
    let busy = e.busy;
    let error = e.error.clone();
    let mut body = vec![];
    if new {
        body.push(text(
            label("newEvent"),
            json!({"fontSize":15,"fontWeight":600,"color":fg(a)}),
        ));
    }
    body.push(input(
        "event-title",
        s(&d["title"]),
        label("titlePlaceholder"),
        json!(["draft", "title"]),
        "save",
        new,
        busy,
    ));
    body.push(input(
        "event-location",
        s(&d["loc"]),
        label("locationPlaceholder"),
        json!(["draft", "loc"]),
        "save",
        false,
        busy,
    ));
    let all = d["allDay"] == true;
    body.push(field(a,"allDay",vec![click(v("toggle",json!({"width":42,"height":24,"borderRadius":12,"direction":"ltr","backgroundColor":if all{accent(a)}else{token(a,"control","#3a3a3f")}}),vec![v("knob",json!({"position":"absolute","top":2,"left":if all{20}else{2},"width":20,"height":20,"borderRadius":10,"backgroundColor":"#ffffff"}),vec![])]),if busy{json!(["noop"])}else{json!(["allDay"])})]));
    for (label_key, df, mf) in [("starts", "sDay", "sMin"), ("ends", "eDay", "eMin")] {
        let caption = a.date(num(&d[df]), "short");
        let mut controls = vec![choice(a, df, json!(caption), 130., busy)];
        if !all {
            let time = a.time(date::at_minutes(num(&d[df]), num(&d[mf]) as i32));
            controls.push(choice(a, mf, json!(time), 110., busy));
        }
        body.push(field(a, label_key, controls));
    }
    body.push(field(
        a,
        "repeat",
        vec![choice(a, "rrule", repeat_title(s(&d["rrule"])), 200., busy)],
    ));
    body.push(field(
        a,
        "alert",
        vec![choice(
            a,
            "alarm",
            alert_title(num(&d["alarm"])),
            200.,
            busy,
        )],
    ));
    let cals = a.state.event_cals();
    body.push(field(
        a,
        "calendar",
        if cals.is_empty() {
            vec![text(
                label("defaultName"),
                json!({"fontSize":12,"color":dim(a)}),
            )]
        } else {
            vec![choice(
                a,
                "cal",
                cals.iter()
                    .find(|c| c["id"] == d["cal"])
                    .map(|c| c["name"].clone())
                    .unwrap_or(json!("—")),
                200.,
                busy,
            )]
        },
    ));
    if recurring(&original, new) {
        body.push(field(
            a,
            "",
            vec![text(
                label("seriesNote"),
                json!({"fontSize":11,"color":dim(a)}),
            )],
        ));
    }
    let att = original["att"].as_array().cloned().unwrap_or_default();
    if original["org"].is_object() || !att.is_empty() {
        body.push(text(
            json!({"key":"calendar.invitees","params":{"count":att.len()}}),
            json!({"fontSize":11,"fontWeight":600,"color":dim(a)}),
        ));
        let mut people = vec![];
        if original["org"].is_object() {
            let org = &original["org"];
            people.push(person(
                a,
                org["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(s(&org["email"])),
                label("organizer"),
                "★",
                "#8e8e93",
            ));
        }
        for p in att {
            let (g, c, ps) = participation(s(&p["ps"]));
            people.push(person(
                a,
                p["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(s(&p["email"])),
                label(&format!("ps.{ps}")),
                g,
                c,
            ));
        }
        body.push(v(
            "invitees-height",
            json!({"height":(people.len()*22).min(110)}),
            vec![json!({"kind":"scroll","key":"invitees","style":{"flex":1},"children":people})],
        ));
    }
    let caret = if a.state.notes_focus && (now - a.notes_since).max(0) / 530 % 2 == 0 {
        "▏"
    } else {
        ""
    };
    let notes = s(&d["notes"]);
    let mut note = click(
        v(
            "notes-input",
            json!({"minHeight":60,"padding":7,"borderRadius":7,"backgroundColor":token(a,"inputBg","#202024"),"borderWidth":1,"borderColor":if a.state.notes_focus{accent(a)}else{border(a)}}),
            vec![text(
                if notes.is_empty() && !a.state.notes_focus {
                    label("notesPlaceholder")
                } else {
                    json!(format!("{notes}{caret}"))
                },
                json!({"fontSize":13,"lineHeight":18,"color":if notes.is_empty()&&!a.state.notes_focus{dim(a)}else{fg(a)}}),
            )],
        ),
        json!(["noop"]),
    );
    note["events"]["onMouseDown"] = if busy {
        json!(["noop"])
    } else {
        json!(["notesFocus"])
    };
    body.push(note);
    let mut footer = vec![];
    if !new {
        let mut b = button("delete", json!(["delete"]), busy);
        b["destructive"] = json!(true);
        footer.push(b);
    }
    footer.push(text(
        if error.is_empty() {
            json!("")
        } else {
            json!({"key":error})
        },
        json!({"flex":1,"fontSize":12,"color":"#ff3b30"}),
    ));
    footer.extend([
        button("cancel", json!(["close"]), false),
        button(if new { "add" } else { "save" }, json!(["save"]), busy),
    ]);
    sheet(a, "event-sheet", 500., body, footer)
}
fn person(a: &App, name: &str, detail: Value, glyph: &str, color: &str) -> Value {
    v(
        name,
        json!({"height":22,"flexDirection":"row","alignItems":"center","gap":8}),
        vec![
            v(
                "glyph",
                json!({"width":16,"height":16,"borderRadius":8,"alignItems":"center","justifyContent":"center","backgroundColor":color}),
                vec![text(
                    json!(glyph),
                    json!({"fontSize":9,"fontWeight":700,"color":"#ffffff"}),
                )],
            ),
            text(
                json!(name),
                json!({"flex":1,"fontSize":12,"color":fg(a),"whiteSpace":"nowrap","overflow":"hidden"}),
            ),
            text(detail, json!({"fontSize":11,"color":dim(a)})),
        ],
    )
}
fn calendar_sheet(a: &App) -> Value {
    let c = a.state.calendar.as_ref().unwrap();
    let body=vec![text(label(if c.id.is_some(){"editCalendar"}else{"newCalendar"}),json!({"fontSize":15,"fontWeight":600,"color":fg(a)})),input("calendar-name",&c.name,label("calendarName"),json!(["calDraft","name"]),"calSave",true,c.busy),text(label("color"),json!({"fontSize":11,"fontWeight":600,"color":dim(a)})),v("colors",json!({"flexDirection":"row","gap":8,"flexWrap":"wrap"}),COLORS.iter().map(|color|click(v(color,json!({"width":24,"height":24,"borderRadius":12,"alignItems":"center","justifyContent":"center","borderWidth":2,"borderColor":if c.color==*color{fg(a)}else{"transparent".into()}}),vec![v("color",json!({"width":18,"height":18,"borderRadius":9,"backgroundColor":color}),vec![])]),if c.busy{json!(["noop"])}else{json!(["calDraft",{"field":"color","value":color}])})).collect())];
    sheet(
        a,
        "calendar-sheet",
        380.,
        body,
        vec![
            text(
                if c.error.is_empty() {
                    json!("")
                } else {
                    json!({"key":c.error})
                },
                json!({"flex":1,"fontSize":12,"color":"#ff3b30"}),
            ),
            button("cancel", json!(["close"]), false),
            button(
                if c.id.is_some() { "save" } else { "create" },
                json!(["calSave"]),
                c.busy || c.name.trim().is_empty(),
            ),
        ],
    )
}
fn popup(
    a: &App,
    id: &str,
    anchor: &str,
    width: f64,
    children: Vec<Value>,
    dismiss: Value,
) -> Value {
    let mut node = click(
        v(
            id,
            json!({"position":"absolute","left":8,"top":8,"width":width,"padding":6,"borderRadius":10,"backgroundColor":if a.theme["mode"]=="dark"{"#2c2c31"}else{"#ffffff"},"borderWidth":1,"borderColor":border(a),"boxShadow":"0 12px 32px rgba(0,0,0,0.32)"}),
            children,
        ),
        json!(["noop"]),
    );
    node["anchorKey"] = json!(anchor);
    node["fitPopover"] = json!(true);
    let mut bg = v(
        "dismiss",
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0}),
        vec![],
    );
    bg["events"]["onMouseDown"] = dismiss;
    v(
        &format!("{id}-layer"),
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":200}),
        vec![bg, node],
    )
}
fn option(
    a: &App,
    key: &str,
    title: Value,
    action: Value,
    checked: bool,
    disabled: bool,
    destructive: bool,
) -> Value {
    let mut n = click(
        v(
            key,
            json!({"height":26,"paddingHorizontal":8,"borderRadius":5,"flexDirection":"row","alignItems":"center","gap":6,"opacity":if disabled{0.4}else{1.}}),
            vec![
                text(
                    json!(if checked { "✓" } else { "" }),
                    json!({"width":12,"fontSize":11,"color":fg(a)}),
                ),
                text(
                    title,
                    json!({"flex":1,"fontSize":13,"color":if destructive{"#ff3b30".into()}else{fg(a)},"whiteSpace":"nowrap","overflow":"hidden"}),
                ),
            ],
        ),
        if disabled { json!(["noop"]) } else { action },
    );
    n["hoverStyle"] = json!({"backgroundColor":token(a,"hover","rgba(127,127,127,0.12)")});
    n
}
fn menu(a: &App) -> Value {
    let id = &a.state.menu;
    let Some(c) = a.state.cals.iter().find(|c| c["id"] == id.as_str()) else {
        return Value::Null;
    };
    popup(
        a,
        "calendar-menu",
        &format!("cal-{id}"),
        220.,
        vec![
            option(
                a,
                "editCalendar",
                label("editCalendar"),
                json!(["calEdit", id]),
                false,
                false,
                false,
            ),
            option(
                a,
                "calToggle",
                label(if c["hidden"] == true { "show" } else { "hide" }),
                json!(["calToggle", id]),
                false,
                false,
                false,
            ),
            option(
                a,
                "deleteCalendar",
                label("deleteCalendar"),
                json!(["calDelete", id]),
                false,
                c["kind"] != "local",
                true,
            ),
        ],
        json!(["menu", ""]),
    )
}
fn picker(a: &mut App, now: i64) -> Value {
    let field = a.picker.clone();
    let Some(d) = a.state.event.as_ref().map(|e| e.draft.clone()) else {
        return Value::Null;
    };
    if field == "sDay" || field == "eDay" {
        let month = a.picker_month;
        let mini = mini_month(a, month, num(&d[&field]), now, "picker", 28.);
        return popup(
            a,
            "date-picker",
            &format!("choice-{field}"),
            212.,
            vec![mini],
            json!(["closePicker"]),
        );
    }
    if field == "delete" {
        return popup(
            a,
            "delete-options",
            "button-delete",
            250.,
            vec![
                option(
                    a,
                    "this",
                    label("deleteThis"),
                    json!(["eventDelete", "this"]),
                    false,
                    false,
                    true,
                ),
                option(
                    a,
                    "all",
                    label("deleteAll"),
                    json!(["eventDelete", "all"]),
                    false,
                    false,
                    true,
                ),
            ],
            json!(["closePicker"]),
        );
    }
    let mut opts: Vec<(Value, Value)> = vec![];
    match field.as_str() {
        "sMin" | "eMin" => {
            let value = num(&d[&field]);
            let mut times: Vec<_> = (0..1440).step_by(15).map(|n| n as i64).collect();
            if !times.contains(&value) {
                times.push(value);
                times.sort();
            }
            for min in times {
                let time = a.time(date::at_minutes(date::day(now), min as i32));
                opts.push((json!(min), json!(time)));
            }
        }
        "rrule" => {
            opts.push((json!(""), label("repeat.none")));
            for freq in FREQS {
                opts.push((
                    json!(format!("FREQ={freq}")),
                    label(&format!("repeat.{}", freq.to_lowercase())),
                ));
            }
            if !s(&d["rrule"]).is_empty() && !opts.iter().any(|(v, _)| *v == d["rrule"]) {
                opts.push((d["rrule"].clone(), label("repeat.custom")));
            }
        }
        "alarm" => {
            let mut alerts = vec![-1];
            alerts.extend(ALERTS);
            let value = num(&d["alarm"]);
            if value >= 0 && !alerts.contains(&value) {
                alerts.push(value);
            }
            for min in alerts {
                opts.push((json!(min), alert_title(min)));
            }
        }
        "cal" => {
            opts.extend(
                a.state
                    .event_cals()
                    .iter()
                    .map(|c| (c["id"].clone(), c["name"].clone())),
            );
        }
        _ => {}
    }
    let selected = opts.iter().position(|(v, _)| *v == d[&field]).unwrap_or(0);
    let height = (opts.len() as f64 * 26.)
        .min(260.)
        .min((a.height - 50.).max(40.));
    let offset = (selected as f64 * 26. - height / 2.).max(0.);
    let rows = opts
        .into_iter()
        .map(|(value, title)| {
            option(
                a,
                &value.to_string(),
                title,
                json!(["pick",{"field":field,"value":value}]),
                value == d[&field],
                false,
                false,
            )
        })
        .collect::<Vec<_>>();
    popup(
        a,
        "options",
        &format!("choice-{field}"),
        230.,
        vec![v(
            "options-height",
            json!({"height":height}),
            vec![
                json!({"kind":"scroll","key":format!("options-{field}"),"initialOffset":offset,"style":{"flex":1},"children":rows}),
            ],
        )],
        json!(["closePicker"]),
    )
}
pub fn tree(a: &mut App, now: i64) -> Value {
    let mut children = vec![];
    if a.show_sidebar() {
        children.push(sidebar(a, now));
    }
    let mut toolbar = vec![
        tool(
            a,
            "toggleSidebar",
            "▤",
            json!(["sidebar"]),
            a.show_sidebar(),
            false,
        ),
        tool(
            a,
            "newEvent",
            "+",
            json!(["new"]),
            false,
            a.state.unavailable,
        ),
        spacer(),
    ];
    let mut tabs = vec![];
    for mode in ["day", "week", "month"] {
        let mut b = button(&format!("view.{mode}"), json!(["view", mode]), false);
        if a.state.view == mode {
            b["style"] = json!({"backgroundColor":token(a,"selection","rgba(127,127,127,0.25)")});
        }
        tabs.push(b);
    }
    toolbar.push(v("views", json!({"flexDirection":"row","gap":2}), tabs));
    toolbar.extend([spacer(), v("balance", json!({"width":64}), vec![])]);
    let title = if a.state.view == "day" {
        a.date(a.state.cursor, "long")
    } else {
        a.month_title(if a.state.view == "week" {
            date::add_days(date::week(a.state.cursor), 3)
        } else {
            a.state.cursor
        })
    };
    let main = vec![
        v(
            "toolbar",
            json!({"height":42,"paddingHorizontal":8,"flexDirection":"row","alignItems":"center","gap":4}),
            toolbar,
        ),
        v(
            "period",
            json!({"height":if a.width<760.{42}else{50},"paddingHorizontal":16,"flexDirection":"row","alignItems":"center","gap":12}),
            vec![
                text(
                    json!(title),
                    json!({"flex":1,"fontSize":if a.width<760.{17}else{22},"fontWeight":700,"color":fg(a),"whiteSpace":"nowrap","overflow":"hidden"}),
                ),
                tool(
                    a,
                    "previous",
                    if a.rtl { "›" } else { "‹" },
                    json!(["go", -1]),
                    false,
                    false,
                ),
                button("today", json!(["today"]), false),
                tool(
                    a,
                    "next",
                    if a.rtl { "‹" } else { "›" },
                    json!(["go", 1]),
                    false,
                    false,
                ),
            ],
        ),
        if a.state.unavailable {
            v(
                "unavailable",
                json!({"flex":1,"padding":20,"gap":10,"alignItems":"center","justifyContent":"center"}),
                vec![
                    text(
                        label("unavailable"),
                        json!({"fontSize":16,"fontWeight":600,"color":dim(a),"textAlign":"center"}),
                    ),
                    text(
                        label("unavailableHint"),
                        json!({"fontSize":12,"color":dim(a),"textAlign":"center"}),
                    ),
                ],
            )
        } else if a.state.view == "month" {
            month_grid(a, now)
        } else {
            time_grid(a, now)
        },
    ];
    children.push(v("main", json!({"flex":1,"minWidth":0}), main));
    if a.state.event.is_some() {
        children.push(event_sheet(a, now));
    }
    if a.state.calendar.is_some() {
        children.push(calendar_sheet(a));
    }
    if !a.state.menu.is_empty() {
        children.push(menu(a));
    }
    if !a.picker.is_empty() {
        children.push(picker(a, now));
    }
    v(
        "calendar",
        json!({"flex":1,"flexDirection":"row","direction":if a.rtl{"rtl"}else{"ltr"},"backgroundColor":token(a,"windowBg","#252529")}),
        children,
    )
}
