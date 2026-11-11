use super::{icons, markdown, App};
use serde_json::{json, Value};
pub(super) fn key(k: &str) -> Value {
    json!({"key":k})
}
pub(super) fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
pub(super) fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
pub(super) fn click(mut node: Value, action: Value) -> Value {
    node["events"] = json!({"onClick":action});
    node
}
fn hover(mut node: Value, id: &str) -> Value {
    node["events"]["onMouseEnter"] = json!(["hover", id]);
    node["events"]["onMouseLeave"] = json!(["hover", ""]);
    node
}
pub fn icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    view(
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(180deg, #5ac8fa, #0a60d0)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"alignItems":"center","justifyContent":"center"}),
        vec![view(
            json!({"width":s*0.62,"height":s*0.62,"borderRadius":s*0.31,"backgroundColor":"rgba(255,255,255,0.95)","alignItems":"center","justifyContent":"center"}),
            vec![text(
                json!("?"),
                json!({"fontSize":s*0.44,"fontWeight":700,"color":"#0a60d0","lineHeight":s*0.62}),
            )],
        )],
    )
}
fn button(app: &App, label: Value, action: Value, primary: bool, plain: bool, id: &str) -> Value {
    let t = &app.theme;
    let over = app.hover == id;
    hover(
        click(
            view(
                json!({"height":28,"paddingHorizontal":14,"borderRadius":t["controlRadius"],"flexDirection":"row","gap":6,"justifyContent":"center","alignItems":"center","backgroundColor":if primary{t["accent"].clone()}else if plain{if over{t["hover"].clone()}else{json!("transparent")}}else{t["control"].clone()},"borderWidth":if t["buttonBorder"]==true&&!plain{1}else{0},"borderColor":if primary{&t["accent"]}else{&t["separator"]},"opacity":if over&&!plain{0.85}else{1.}}),
                vec![text(
                    label,
                    json!({"fontSize":13,"fontWeight":500,"color":if primary{json!("#fff")}else{t["text"].clone()},"whiteSpace":"nowrap"}),
                )],
            ),
            action,
        ),
        id,
    )
}
fn empty(label: Value, t: &Value) -> Value {
    view(
        json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":8,"padding":24}),
        vec![text(
            label,
            json!({"fontSize":15,"fontWeight":600,"color":t["text"],"textAlign":"center"}),
        )],
    )
}
fn sidebar_row(
    app: &App,
    label: Value,
    icon: Value,
    on: bool,
    action: Value,
    id: &str,
    rtl: bool,
) -> Value {
    let t = &app.theme;
    let ind = t["selection"] == "indicator";
    let filled = on && !ind;
    let mut children = vec![];
    if on && ind {
        children.push(view(json!({"position":"absolute",if rtl{"right"}else{"left"}:0,"top":8,"width":3,"height":12,"borderRadius":2,"backgroundColor":t["accent"]}),vec![]));
    }
    children.push(if icon.is_string(){text(icon,json!({"width":18,"textAlign":"center","fontSize":14,"color":if filled{json!("#fff")}else{t["accent"].clone()}}))}else{view(json!({"width":18,"alignItems":"center"}),vec![icon])});
    children.push(text(label,json!({"flex":1,"fontSize":13,"fontWeight":if on&&ind{600}else{500},"color":if filled{json!("#fff")}else{t["text"].clone()},"whiteSpace":"nowrap","overflow":"hidden"})));
    let mut row = hover(
        click(
            view(
                json!({"height":28,"paddingLeft":10,"paddingRight":10,"borderRadius":t["controlRadiusSmall"],"flexDirection":"row","alignItems":"center","gap":8,"backgroundColor":if filled{t["accent"].clone()}else if on{json!(if t["mode"]=="dark"{"rgba(255,255,255,0.09)"}else{"rgba(0,0,0,0.06)"})}else if app.hover==id{t["hover"].clone()}else{json!("transparent")}}),
                children,
            ),
            action,
        ),
        id,
    );
    row["key"] = json!(id);
    row
}
fn sidebar(app: &App, rtl: bool) -> Value {
    let t = &app.theme;
    let mut rows = vec![];
    if let Some(results) = &app.results {
        for r in results {
            let on = r["id"] == app.article && !app.tour;
            let mut row = click(
                view(
                    json!({"paddingVertical":8,"paddingHorizontal":10,"borderRadius":6,"gap":2,"backgroundColor":if on{t["accent"].clone()}else{json!("transparent")}}),
                    vec![
                        text(
                            r["title"].clone(),
                            json!({"fontSize":13,"fontWeight":600,"color":if on{json!("#fff")}else{t["text"].clone()},"textAlign":"start"}),
                        ),
                        text(
                            r["snippet"].clone(),
                            json!({"fontSize":11,"color":if on{json!("rgba(255,255,255,0.85)")}else{t["textDim"].clone()},"lineHeight":1.35,"maxHeight":44,"overflow":"hidden","textAlign":"start"}),
                        ),
                    ],
                ),
                json!(["article", r["id"]]),
            );
            row["key"] = r["id"].clone();
            rows.push(row);
        }
        if results.is_empty() {
            rows.push(text(
                key("help.noResults"),
                json!({"fontSize":12,"color":t["textDim"],"textAlign":"center","paddingTop":20}),
            ));
        }
    } else {
        rows.push(view(json!({"height":4}), vec![]));
        rows.push(sidebar_row(
            app,
            key("help.tour.open"),
            json!("☺"),
            app.tour,
            json!(["tour"]),
            "tour",
            rtl,
        ));
        for sec in ["start", "desktop", "apps", "system", "settings"] {
            let items:Vec<_>=app.list.iter().filter(|it|if it.section.is_empty(){"apps"}else{&it.section}==sec).collect();
            if items.is_empty() {
                continue;
            }
            let mut group = vec![text(
                key(&format!("help.section.{sec}")),
                json!({"fontSize":11,"fontWeight":600,"color":t["textDim"],"marginLeft":10,"marginTop":10,"marginBottom":4}),
            )];
            for it in items {
                let icon = app
                    .app(&it.app)
                    .filter(|a| a["icon"] == true)
                    .and_then(|_| icons::app_icon(&it.app, 18.))
                    .unwrap_or(json!("≡"));
                group.push(sidebar_row(
                    app,
                    json!(it.title),
                    icon,
                    !app.tour && app.article == it.id,
                    json!(["article", it.id]),
                    &it.id,
                    rtl,
                ));
            }
            let mut node = view(json!({"gap":2}), group);
            node["key"] = json!(sec);
            rows.push(node);
        }
    }
    let lead = if rtl { "right" } else { "left" };
    let trail = if rtl { "left" } else { "right" };
    let mut search = vec![
        json!({"kind":"input","value":app.query,"placeholder":key("help.search"),"change":["query"],"cancel":["query",""],"submit":["submit"],"style":{"flex":1,if rtl{"paddingRight"}else{"paddingLeft"}:26,if rtl{"paddingLeft"}else{"paddingRight"}:if app.query.is_empty(){10}else{26}}}),
        text(
            json!("⌕"),
            json!({"position":"absolute",lead:9,"top":5,"fontSize":13,"color":t["textDim"],"pointerEvents":"none"}),
        ),
    ];
    if !app.query.is_empty() {
        search.push(click(view(json!({"position":"absolute",trail:6,"top":6,"width":16,"height":16,"borderRadius":8,"alignItems":"center","justifyContent":"center","backgroundColor":t["textDim"]}),vec![text(json!("✕"),json!({"fontSize":10,"fontWeight":700,"color":if t["mode"]=="dark"{"#1f1f23"}else{"#fff"}}))]),json!(["query",""])));
    }
    view(
        json!({"width":240,"backgroundColor":t["sidebarBg"]}),
        vec![
            view(
                json!({"paddingHorizontal":10,"paddingTop":12,"paddingBottom":8}),
                vec![view(
                    json!({"flexDirection":"row","alignItems":"center"}),
                    search,
                )],
            ),
            json!({"kind":"scroll","key":"sidebar-scroll","style":{"flex":1},"children":[view(json!({"paddingHorizontal":10,"paddingBottom":12,"gap":2}),rows)]}),
            view(
                json!({"position":"absolute",trail:0,"top":0,"bottom":0,"width":1,"backgroundColor":t["separator"]}),
                vec![],
            ),
        ],
    )
}
fn tour(app: &App) -> Value {
    let t = &app.theme;
    let ids: &[&str] = match app.step {
        1 => &["files", "terminal", "store"],
        3 => &["settings", "monitor"],
        4 => &["clock", "help"],
        5 => &["help"],
        _ => &[],
    };
    let art = if app.step == 0 {
        vec![icons::app_icon("about", 112.).unwrap()]
    } else if app.step == 2 {
        vec![text(
            json!("⌕"),
            json!({"fontSize":64,"color":t["textDim"]}),
        )]
    } else {
        ids.iter()
            .filter(|id| app.app(id).is_some())
            .filter_map(|id| icons::app_icon(id, 84.))
            .collect()
    };
    let mut children = vec![
        view(
            json!({"height":120,"flexDirection":"row","gap":16,"alignItems":"center","justifyContent":"center"}),
            art,
        ),
        text(
            key(&format!("help.tour.{}.title", app.step)),
            json!({"fontSize":26,"fontWeight":600,"color":t["text"],"textAlign":"center"}),
        ),
        text(
            key(&format!("help.tour.{}.body", app.step)),
            json!({"fontSize":14,"color":t["textDim"],"textAlign":"center","lineHeight":1.5,"maxWidth":520}),
        ),
    ];
    if app.step == 3 {
        children.push(view(
            json!({"flexDirection":"row","gap":8}),
            ["appearance", "network", "users"]
                .into_iter()
                .map(|sec| {
                    button(
                        app,
                        key(&format!("settings.section.{sec}")),
                        json!(["link", format!("settings:{sec}")]),
                        false,
                        false,
                        sec,
                    )
                })
                .collect(),
        ));
    }
    children.push(view(json!({"flexDirection":"row","gap":6,"marginTop":4}),(0..6).map(|i|click(view(json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":if app.step==i{&t["accent"]}else{&t["control"]}}),vec![]),json!(["step",i.to_string()]))).collect()));
    let mut actions = vec![];
    if app.step != 5 {
        actions.push(button(
            app,
            key("help.tour.skip"),
            json!(["finish"]),
            false,
            true,
            "skip",
        ));
    }
    if app.step > 0 {
        actions.push(button(
            app,
            key("help.tour.back"),
            json!(["back"]),
            false,
            false,
            "back",
        ));
    }
    actions.push(button(
        app,
        key(if app.step == 5 {
            "help.tour.finish"
        } else {
            "help.tour.next"
        }),
        json!(["next"]),
        true,
        false,
        "next",
    ));
    children.push(view(
        json!({"flexDirection":"row","gap":8,"alignItems":"center","marginTop":8}),
        actions,
    ));
    let mut node = view(
        json!({"flex":1,"alignItems":"center","justifyContent":"center","padding":32,"gap":16}),
        children,
    );
    node["key"] = json!("tour");
    node
}
fn article(app: &App) -> Value {
    let t = &app.theme;
    let Some(article) = &app.body else {
        return empty(key("help.notFound"), t);
    };
    let col = (app.width - 240. - 64.).clamp(280., 680.);
    let mut children = vec![];
    if let Some(a) = app.app(&article.meta.app) {
        let mut header = vec![];
        if let Some(icon) = icons::app_icon(&article.meta.app, 48.) {
            header.push(icon);
        }
        header.push(view(json!({"flex":1}), vec![]));
        header.push(button(
            app,
            json!({"key":"help.openApp","params":{"app":a["name"]}}),
            json!(["open", article.meta.app]),
            false,
            false,
            "open-app",
        ));
        children.push(view(
            json!({"flexDirection":"row","alignItems":"center","gap":12,"marginBottom":8}),
            header,
        ));
    }
    children.push(markdown::render(&app.document, col, t));
    json!({"kind":"scroll","key":"article-scroll","style":{"flex":1},"children":[view(json!({"alignItems":"center","paddingTop":24,"paddingBottom":32}),vec![view(json!({"width":col,"direction":article_direction(&article.body)}),children)])]})
}
fn article_direction(source: &str) -> &'static str {
    if crate::runtime::native::text_rtl(source) {
        "rtl"
    } else {
        "ltr"
    }
}
pub fn tree(app: &mut App) -> Value {
    let rtl = app.rtl();
    if app.list.is_empty() {
        return empty(key("help.unavailable"), &app.theme);
    }
    view(
        json!({"flex":1,"flexDirection":"row","direction":if rtl{"rtl"}else{"ltr"}}),
        vec![
            sidebar(app, rtl),
            view(
                json!({"flex":1,"minWidth":0}),
                vec![if app.tour { tour(app) } else { article(app) }],
            ),
        ],
    )
}
