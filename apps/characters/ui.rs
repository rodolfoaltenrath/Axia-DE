use super::{
    data::{data, BLOCKS},
    App,
};
use serde_json::{json, Value};
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn click(mut node: Value, action: Value) -> Value {
    node["events"] = json!({"onClick":action});
    node
}
fn hairline(edge: &str, t: &Value) -> Value {
    view(
        json!({"position":"absolute",edge:0,"top":0,"bottom":0,"width":1,"backgroundColor":t["separator"]}),
        vec![],
    )
}
pub fn icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    let shape = |left: f64, top: f64, w: f64, h: f64, r: f64, color: &str| {
        view(
            json!({"position":"absolute","left":s*left,"top":s*top,"width":s*w,"height":s*h,"borderRadius":s*r,"backgroundColor":color}),
            vec![],
        )
    };
    view(
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #5ac8fa, #5856d6)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden"}),
        vec![
            shape(0.14, 0.16, 0.5, 0.5, 0.25, "#ffd60a"),
            shape(0.27, 0.3, 0.06, 0.09, 0.03, "#3a2a00"),
            shape(0.45, 0.3, 0.06, 0.09, 0.03, "#3a2a00"),
            shape(0.27, 0.47, 0.24, 0.07, 0.04, "#3a2a00"),
            text(
                json!("Ω"),
                json!({"position":"absolute","left":s*0.56,"top":s*0.46,"fontSize":s*0.36,"fontWeight":700,"color":"#ffffff"}),
            ),
        ],
    )
}
fn empty(title: Value, hint: Value, icon: bool, t: &Value) -> Value {
    let mut children = vec![];
    if icon {
        children.push(text(
            json!("☺"),
            json!({"fontSize":40,"color":t["textDim"],"marginBottom":4}),
        ));
    }
    children.push(text(
        title,
        json!({"fontSize":15,"fontWeight":600,"color":t["text"],"textAlign":"center"}),
    ));
    if !hint.is_null() {
        children.push(text(hint,json!({"fontSize":12,"color":t["textDim"],"textAlign":"center","maxWidth":360,"lineHeight":1.4})));
    }
    view(
        json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":8,"padding":if icon{16}else{24}}),
        children,
    )
}
fn tabs(app: &App, t: &Value) -> Value {
    let items=[("emoji","characters.tab.emoji"),("symbols","characters.tab.symbols")].into_iter().map(|(id,label)|{
        let on=app.tab==id;
        click(view(json!({"paddingHorizontal":14,"height":24,"borderRadius":(t["controlRadius"].as_f64().unwrap_or(7.)-1.).max(2.),"justifyContent":"center","alignItems":"center","backgroundColor":if on {if t["mode"]=="dark" {json!("rgba(255,255,255,0.18)")}else{json!("#ffffff")}}else{json!("transparent")},"boxShadow":if on {"0 1px 2px rgba(0,0,0,0.18)"}else{"none"}}),vec![text(key(label),json!({"fontSize":12,"fontWeight":if on{600}else{500},"whiteSpace":"nowrap","color":t["text"]}))]),json!(["tab",id]))
    }).collect();
    view(
        json!({"flexDirection":"row","backgroundColor":t["control"],"borderRadius":t["controlRadius"].as_f64().unwrap_or(7.)+1.,"padding":2}),
        items,
    )
}
fn tones(app: &App, t: &Value) -> Value {
    let children=(0..=5u8).map(|n|{
        let shown=if n==0 {"✋".to_owned()}else{format!("✋{}",char::from_u32(0x1f3fa+n as u32).unwrap())};
        click(view(json!({"width":22,"height":22,"borderRadius":11,"alignItems":"center","justifyContent":"center","borderWidth":2,"borderColor":if app.tone==n {t["accent"].clone()}else{json!("transparent")}}),vec![text(json!(shown),json!({"fontSize":14}))]),json!(["tone",n.to_string()]))
    }).collect();
    view(json!({"flexDirection":"row","gap":4}), children)
}
fn search(app: &App, t: &Value, rtl: bool) -> Value {
    let mut input = json!({"kind":"input","value":app.query,"change":["query",""],"cancel":["query",""],"placeholder":key("emoji.search"),"style":{"flex":1}});
    input["style"][if rtl { "paddingRight" } else { "paddingLeft" }] = json!(26);
    input["style"][if rtl { "paddingLeft" } else { "paddingRight" }] =
        json!(if app.query.is_empty() { 10 } else { 26 });
    let mut magnifier = text(
        json!("⌕"),
        json!({"position":"absolute","top":5,"fontSize":13,"color":t["textDim"],"pointerEvents":"none"}),
    );
    magnifier["style"][if rtl { "right" } else { "left" }] = json!(9);
    let mut children = vec![input, magnifier];
    if !app.query.is_empty() {
        let mut clear = click(
            view(
                json!({"position":"absolute","top":6,"width":16,"height":16,"borderRadius":8,"alignItems":"center","justifyContent":"center","backgroundColor":t["textDim"]}),
                vec![text(
                    json!("✕"),
                    json!({"fontSize":10,"fontWeight":700,"color":if t["mode"]=="dark" {"#1f1f23"}else{"#fff"}}),
                )],
            ),
            json!(["query", ""]),
        );
        clear["style"][if rtl { "left" } else { "right" }] = json!(6);
        children.push(clear);
    }
    view(
        json!({"width":180,"flexDirection":"row","alignItems":"center"}),
        children,
    )
}
fn side_item(app: &App, id: &str, label: Value, on: bool, t: &Value) -> Value {
    let indicator = t["selection"] == "indicator";
    let filled = on && !indicator;
    let mut children = vec![];
    if on && indicator {
        children.push(view(json!({"position":"absolute","insetInlineStart":0,"top":8,"width":3,"height":12,"borderRadius":2,"backgroundColor":t["accent"]}),vec![]));
    }
    children.push(text(label,json!({"flex":1,"fontSize":13,"fontWeight":if on&&indicator{600}else{500},"color":if filled{json!("#fff")}else{t["text"].clone()},"whiteSpace":"nowrap","overflow":"hidden"})));
    let hover = format!("side:{id}");
    let mut node = view(
        json!({"height":28,"paddingLeft":10,"paddingRight":10,"borderRadius":t["controlRadiusSmall"],"flexDirection":"row","alignItems":"center","gap":8,"backgroundColor":if filled{t["accent"].clone()}else if on{json!(if t["mode"]=="dark"{"rgba(255,255,255,0.09)"}else{"rgba(0,0,0,0.06)"})}else if app.hover==hover{t["hover"].clone()}else{json!("transparent")}}),
        children,
    );
    node["key"] = json!(id);
    node["events"] = json!({"onClick":[if app.tab=="emoji"{"group"}else{"block"},id],"onMouseEnter":["hover",hover],"onMouseLeave":["hover",""]});
    node
}
fn preview(app: &mut App, t: &Value, rtl: bool) -> Value {
    if app.pick.is_empty() {
        return view(
            json!({"width":200,"backgroundColor":t["sidebarBg"]}),
            vec![
                hairline(if rtl { "right" } else { "left" }, t),
                empty(
                    key("characters.pickTitle"),
                    key("characters.pickHint"),
                    true,
                    t,
                ),
            ],
        );
    }
    let ch = app.pick.clone();
    let name = app.name(&ch);
    let cps = ch
        .chars()
        .map(|c| format!("U+{:04X}", c as u32))
        .collect::<Vec<_>>()
        .join(" ");
    let mut children = vec![
        hairline(if rtl { "right" } else { "left" }, t),
        view(
            json!({"width":128,"height":128,"borderRadius":14,"alignItems":"center","justifyContent":"center","marginBottom":6,"backgroundColor":if t["mode"]=="dark"{"rgba(255,255,255,0.06)"}else{"#ffffff"},"borderWidth":1,"borderColor":t["separator"]}),
            vec![text(
                json!(ch),
                json!({"fontSize":72,"color":t["text"],"lineHeight":110}),
            )],
        ),
    ];
    if !name.is_empty() {
        children.push(text(
            json!(name),
            json!({"fontSize":15,"fontWeight":600,"color":t["text"],"textAlign":"center"}),
        ));
    }
    children.push(text(
        json!(cps),
        json!({"fontSize":11,"color":t["textDim"],"textAlign":"center","direction":"ltr"}),
    ));
    // Use the same primary fill as the original SDK Button.
    let mut copy = click(
        view(
            json!({"height":28,"paddingHorizontal":14,"borderRadius":t["controlRadius"],"flexDirection":"row","gap":6,"justifyContent":"center","alignItems":"center","backgroundColor":t["accent"],"borderWidth":if t["buttonBorder"]==true{1}else{0},"borderColor":t["accent"],"marginTop":8,"opacity":if app.hover=="copy"{0.85}else{1.}}),
            vec![text(
                key("characters.copy"),
                json!({"fontSize":13,"fontWeight":500,"color":"#fff","whiteSpace":"nowrap"}),
            )],
        ),
        json!(["copy", ""]),
    );
    copy["events"]["onMouseEnter"] = json!(["hover", "copy"]);
    copy["events"]["onMouseLeave"] = json!(["hover", ""]);
    children.push(copy);
    view(
        json!({"width":200,"alignItems":"center","gap":8,"paddingHorizontal":16,"paddingTop":24,"backgroundColor":t["sidebarBg"]}),
        children,
    )
}
pub fn tree(app: &mut App) -> Value {
    let t = app.theme.clone();
    let rtl = app.rtl();
    let mut toolbar = vec![tabs(app, &t), view(json!({"flex":1}), vec![])];
    if app.tab == "emoji" {
        toolbar.extend([tones(app, &t), search(app, &t, rtl)]);
    }
    toolbar.push(view(json!({"position":"absolute","left":0,"right":0,"bottom":0,"height":1,"backgroundColor":t["separator"]}),vec![]));
    let mut side = vec![];
    if app.tab == "emoji" {
        side.push(side_item(
            app,
            "recent",
            key("emoji.group.recent"),
            app.query.trim().is_empty() && app.group == "recent",
            &t,
        ));
        for (id, _) in &data().groups {
            side.push(side_item(
                app,
                id,
                key(&format!("emoji.group.{id}")),
                app.query.trim().is_empty() && app.group == *id,
                &t,
            ));
        }
    } else {
        for (i, (id, _, _)) in BLOCKS.iter().enumerate() {
            side.push(side_item(
                app,
                &i.to_string(),
                key(&format!("characters.block.{id}")),
                app.block == i,
                &t,
            ));
        }
    }
    side.push(view(json!({"height":10}), vec![]));
    let sidebar = view(
        json!({"width":180,"backgroundColor":t["sidebarBg"]}),
        vec![
            json!({"kind":"scroll","style":{"flex":1,"paddingHorizontal":10,"paddingTop":10},"children":side}),
            hairline(if rtl { "left" } else { "right" }, &t),
        ],
    );
    let list = app.list();
    let content = if list.is_empty() {
        empty(
            key(if app.group == "recent" && app.query.trim().is_empty() {
                "emoji.noRecent"
            } else {
                "emoji.none"
            }),
            Value::Null,
            false,
            &t,
        )
    } else {
        let cells=list.into_iter().map(|ch|{
            let on=app.pick==ch;
            let bg=if on {if app.tab=="emoji"{json!(format!("{}55",t["accent"].as_str().unwrap_or("#0a84ff")))}else{t["accent"].clone()}}else{json!("transparent")};
            let node=click(view(json!({"width":44,"height":44,"alignItems":"center","justifyContent":"center","borderRadius":8,"backgroundColor":bg}),vec![text(json!(ch),if app.tab=="emoji"{json!({"fontSize":27,"lineHeight":44})}else{json!({"fontSize":20,"color":if on{json!("#fff")}else{t["text"].clone()}})})]),json!(["pick",ch]));node
        }).collect();
        json!({"kind":"scroll","key":if app.tab=="emoji"{"emoji-grid"}else{"symbol-grid"},"style":{"flex":1,"padding":12},"children":[view(json!({"flexDirection":"row","flexWrap":"wrap"}),cells)]})
    };
    let preview = preview(app, &t, rtl);
    view(
        json!({"flex":1}),
        vec![
            view(
                json!({"height":44,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":12,"zIndex":10}),
                toolbar,
            ),
            view(
                json!({"flex":1,"flexDirection":"row"}),
                vec![
                    sidebar,
                    view(
                        json!({"flex":1,"minWidth":0,"direction":"ltr"}),
                        vec![content],
                    ),
                    preview,
                ],
            ),
        ],
    )
}
