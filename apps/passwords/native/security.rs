//! The security report (weak, reused, old and breached passwords; never over the network) and
//! the view with every 2FA code and its countdown (apps/passwords/Security.jsx).
use super::glyphs::glyph;
use super::ui::*;
use super::App;
use serde_json::{json, Value};

/// One finding: a colored dot, the title and the count, a hint line and the items.
fn card(t: &Theme, title: Value, count: usize, color: &str, hint_caption: Value, kids: Vec<Value>) -> Value {
    let kids: Vec<Value> = kids.into_iter().filter(|k| !k.is_null()).collect();
    let mut body = vec![view(json!({"flexDirection":"row","alignItems":"center","gap":8}), vec![
        view(json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":color}), vec![]),
        text(title, json!({"flex":1,"minWidth":0,"fontSize":13,"fontWeight":600,"color":t.text()})),
        text(json!(count.to_string()), json!({"fontSize":17,"fontWeight":600,"color":if count > 0 { json!(color) } else { t.dim() }})),
    ])];
    if !hint_caption.is_null() {
        body.push(hint(t, hint_caption, None));
    }
    let has = !kids.is_empty();
    if has {
        body.push(view(json!({"marginTop":4}), kids));
    }
    view(json!({"borderRadius":10,"border":format!("1px solid {}", t.s(t.sep())),"backgroundColor":if t.dark() {"rgba(255,255,255,0.04)"} else {"rgba(0,0,0,0.025)"},
        "paddingHorizontal":14,"paddingTop":12,"paddingBottom":if has {6} else {12},"gap":4}), body)
}

fn item(app: &App, t: &Theme, id: &Value, sub: Value) -> Value {
    let Some(e) = app.entries.iter().find(|e| e["id"] == *id) else { return Value::Null };
    let (cat, b) = app.cat_of(e);
    let mut lines = vec![text(if e["title"].as_str().unwrap_or("").is_empty() { k("untitled") } else { e["title"].clone() },
        json!({"fontSize":13,"color":t.text(),"whiteSpace":"nowrap","textOverflow":"ellipsis"}))];
    if !sub.is_null() {
        lines.push(text(sub, json!({"fontSize":11,"color":t.dim(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})));
    }
    hover(
        click(
            view(json!({"flexDirection":"row","alignItems":"center","gap":10,"minHeight":40,"paddingHorizontal":6,"marginHorizontal":-6,"borderRadius":6,
                "borderTopWidth":1,"borderTopColor":t.sep()}), vec![
                entry_icon(e, &cat, b.as_ref(), 24.),
                view(json!({"flex":1,"minWidth":0}), lines),
                text(json!("›"), json!({"fontSize":14,"color":t.dim()})),
            ]),
            json!(["openFrom", id]),
        ),
        json!({"backgroundColor":t.hover()}),
    )
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// The number of findings shown in the sidebar (weak, reused, old).
pub fn issues(rep: &Value) -> i64 {
    (list(rep, "weak").len() + list(rep, "reused").iter().map(|g| g.as_array().map_or(0, Vec::len)).sum::<usize>() + list(rep, "old").len()) as i64
}

pub fn report(app: &App, t: &Theme) -> Value {
    let Some(rep) = &app.security.rep else {
        return view(json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":8,"padding":24}), vec![
            text(k("report.loading"), json!({"fontSize":13,"color":t.dim(),"textAlign":"center","maxWidth":360,"lineHeight":18})),
        ]);
    };
    let breached = rep["breached"].as_array();
    let issues = issues(rep) as usize + breached.map_or(0, Vec::len);
    let total = app.entries.iter().filter(|e| e["trash"] != true).count();
    let weak: Vec<Value> = list(rep, "weak").iter().map(|w| {
        let sub = if w["common"] == true { k("report.common") } else {
            json!({"parts":[strength_label(w["score"].as_i64().unwrap_or(0)), " · ", kp("report.bits", json!({"bits":w["bits"]}))]})
        };
        item(app, t, &w["id"], sub)
    }).collect();
    let reused: Vec<Value> = list(rep, "reused").iter().map(|g| {
        let ids = g.as_array().cloned().unwrap_or_default();
        let mut rows = vec![text(kp("report.sameIn", json!({"count":ids.len()})), json!({"fontSize":11,"fontWeight":600,"color":t.dim(),"paddingTop":8,"paddingBottom":4}))];
        rows.extend(ids.iter().map(|id| item(app, t, id, Value::Null)));
        view(json!({}), rows.into_iter().filter(|r| !r.is_null()).collect())
    }).collect();
    let old: Vec<Value> = list(rep, "old").iter().map(|o| item(app, t, &o["id"], kp("report.age", json!({"days":o["days"],"limit":o["limit"]})))).collect();
    let mut br: Vec<Value> = breached.into_iter().flatten().map(|b| item(app, t, &b["id"], kp("report.seen", json!({"count":b["count"]})))).collect();
    br.push(view(json!({"paddingTop":4,"paddingBottom":6,"alignItems":"flex-start"}), vec![
        button(t, k("report.hibpChoose"), json!(["hibp"]), Btn { small: true, ..Btn::default() }),
    ]));
    let breached_count = breached.map_or_else(|| list(rep, "weak").iter().filter(|w| w["common"] == true).count(), Vec::len);
    let kids = vec![
        view(json!({"flexDirection":"row","alignItems":"center","gap":14,"marginBottom":4}), vec![
            view(json!({"width":48,"height":48,"borderRadius":12,"alignItems":"center","justifyContent":"center",
                "background":if issues > 0 {"linear-gradient(180deg, #ffb340, #ff9500)"} else {"linear-gradient(180deg, #4cd964, #28b84a)"}}), vec![
                glyph("shield", 28., "#fff"),
                view(json!({"position":"absolute","left":0,"right":0,"top":0,"bottom":0,"alignItems":"center","justifyContent":"center","paddingBottom":3}), vec![
                    text(json!(if issues > 0 { "!" } else { "✓" }), json!({"fontSize":13,"fontWeight":700,"color":"#fff"})),
                ]),
            ]),
            view(json!({"flex":1,"minWidth":0,"gap":2}), vec![
                text(k("report.title"), json!({"fontSize":22,"fontWeight":600,"color":t.text(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
                text(if issues > 0 { kp("report.issues", json!({"count":issues,"total":total})) } else { kp("report.allGood", json!({"total":total})) },
                    json!({"fontSize":12,"color":t.dim()})),
            ]),
            button(t, k(if app.security.busy { "report.loading" } else { "report.again" }), json!(["reportAgain"]), Btn { small: true, ..Btn::default() }),
        ]),
        card(t, k("report.weak"), list(rep, "weak").len(), RED, k("report.weakHint"), weak),
        card(t, k("report.reused"), list(rep, "reused").len(), ORANGE, k("report.reusedHint"), reused),
        card(t, k("report.old"), list(rep, "old").len(), YELLOW, k("report.oldHint"), old),
        card(t, k("report.breached"), breached_count, RED, k("report.breachedHint"), br),
        view(json!({"paddingHorizontal":4}), vec![hint(t, k("report.offline"), None)]),
    ];
    json!({"kind":"scroll","key":"security","style":{"flex":1},"children":[
        view(json!({"paddingHorizontal":24,"paddingTop":20,"paddingBottom":24,"gap":12,"width":"100%","maxWidth":760,"alignSelf":"center"}), kids),
    ]})
}

pub fn codes(app: &App, t: &Theme, now: i64) -> Value {
    let c = &app.codes;
    if c.codes.is_empty() {
        return empty(t, glyph("clock", 40., &t.s(t.dim())), k("side.codes"), k("codes.empty"), json!({}));
    }
    let el = now as f64 / 1000. - c.at;
    let mut kids = vec![text(k("side.codes"), json!({"fontSize":22,"fontWeight":600,"color":t.text(),"marginBottom":8}))];
    for code in &c.codes {
        let e = app.entries.iter().find(|e| e["id"] == code["id"]);
        let left = (code["remaining"].as_f64().unwrap_or(0.) - el).round().max(0.);
        let period = code["period"].as_f64().unwrap_or(0.);
        let s = code["code"].as_str().unwrap_or("");
        let shown = if s.chars().count() == 6 { format!("{} {}", &s[..3], &s[3..]) } else { s.into() };
        let hot = left <= 5.;
        let mut row = vec![];
        if let Some(e) = e {
            let (cat, b) = app.cat_of(e);
            row.push(entry_icon(e, &cat, b.as_ref(), 32.));
        }
        let title = e.and_then(|e| e["title"].as_str()).filter(|s| !s.is_empty()).or(code["issuer"].as_str()).unwrap_or("");
        let sub = code["account"].as_str().filter(|s| !s.is_empty()).or_else(|| e.and_then(|e| e["sub"].as_str())).unwrap_or("");
        row.push(view(json!({"flex":1,"minWidth":0,"gap":2}), vec![
            text(json!(title), json!({"fontSize":13,"fontWeight":600,"color":t.text(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
            text(json!(sub), json!({"fontSize":11,"color":t.dim(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
        ]));
        row.push(text(json!(shown), json!({"fontSize":20,"fontWeight":600,"fontFamily":"monospace","color":if hot { json!(RED) } else { t.text() },"direction":"ltr"})));
        row.push(view(json!({"width":34,"alignItems":"center","gap":4}), vec![
            text(json!(format!("{}s", left as i64)), json!({"fontSize":12,"color":if hot { json!(RED) } else { t.dim() }})),
            view(json!({"width":30,"height":3,"borderRadius":2,"backgroundColor":t.bg(),"direction":"ltr"}), vec![
                view(json!({"width":(30. * if period > 0. { left / period } else { 0. }).round(),"height":3,"borderRadius":2,"backgroundColor":if hot { RED } else { GREEN }}), vec![]),
            ]),
        ]));
        row.push(icon_button(t, Some("⧉"), None, Value::Null, k("copy"), false, None, json!(["copyCode", s])));
        kids.push(click(
            keyed(&format!("code:{}:{}", code["id"].as_str().unwrap_or(""), code["k"].as_str().unwrap_or("")),
                json!({"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":12,"height":60,"borderRadius":10,"border":format!("1px solid {}", t.s(t.sep())),
                    "backgroundColor":if t.dark() {"rgba(255,255,255,0.04)"} else {"rgba(0,0,0,0.025)"}}), row),
            json!(["openFrom", code["id"]]),
        ));
    }
    json!({"kind":"scroll","key":"codes","style":{"flex":1},"children":[
        view(json!({"paddingHorizontal":24,"paddingTop":20,"paddingBottom":24,"gap":8,"width":"100%","maxWidth":760,"alignSelf":"center"}), kids),
    ]})
}
