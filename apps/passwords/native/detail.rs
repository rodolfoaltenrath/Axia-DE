//! One item (apps/passwords/Detail.jsx): viewing it (fields with show/copy, the 2FA code with its
//! countdown, the Wi-Fi QR, notes, attachments, history, the change reminder) and editing it (its
//! category's template, own fields, tags, the generator, 2FA from a QR).
use super::glyphs::glyph;
use super::ui::*;
use super::{App, Editor, Viewing};
use serde_json::{json, Value};

pub const HIDDEN: [&str; 3] = ["hidden", "totp", "pin"];
const DAY: f64 = 86400.;

pub fn hidden(f: &Value) -> bool {
    HIDDEN.contains(&f["t"].as_str().unwrap_or(""))
}

/// The 2FA codes of the open item, with their countdown.
fn totp_box(app: &App, t: &Theme, v: &Viewing, now: i64) -> Vec<Value> {
    let elapsed = now as f64 / 1000. - v.totp_at;
    v.totp
        .iter()
        .map(|c| {
            let left = (c["remaining"].as_f64().unwrap_or(0.) - elapsed).round().max(0.);
            let period = c["period"].as_f64().unwrap_or(0.);
            let frac = if period > 0. { left / period } else { 0. };
            let code = c["code"].as_str().unwrap_or("");
            let shown = if code.chars().count() == 6 { format!("{} {}", &code[..3], &code[3..]) } else { code.into() };
            let hot = left <= 5.;
            let mut kids = vec![
                view(json!({"flexDirection":"row","alignItems":"center","gap":10}), vec![
                    view(json!({"flex":1,"minWidth":0,"alignItems":"flex-start"}), vec![
                        text(json!(shown), json!({"fontSize":22,"fontWeight":600,"fontFamily":"monospace","color":if hot { json!(RED) } else { t.text() },"direction":"ltr"})),
                    ]),
                    text(kp("totp.left", json!({"s":left as i64})), json!({"fontSize":12,"color":if hot { json!(RED) } else { t.dim() }})),
                    icon_button(t, Some("⧉"), None, Value::Null, k("copy"), false, None, json!(["copyCode", code])),
                ]),
                view(json!({"height":3,"borderRadius":2,"backgroundColor":t.control(),"direction":"ltr"}), vec![
                    view(json!({"width":format!("{}%", (frac * 100.).round()),"height":3,"borderRadius":2,"backgroundColor":if hot { json!(RED) } else { t.accent() }}), vec![]),
                ]),
            ];
            let who: Vec<&str> = [c["issuer"].as_str(), c["account"].as_str()].into_iter().flatten().filter(|s| !s.is_empty()).collect();
            if !who.is_empty() {
                kids.push(text(json!(who.join(" · ")), json!({"fontSize":11,"color":t.dim()})));
            }
            let _ = app;
            view(json!({"gap":6,"paddingTop":2,"paddingBottom":4}), kids)
        })
        .collect()
}

fn field_row(app: &App, t: &Theme, v: &Viewing, f: &Value, index: usize, now: i64) -> Value {
    let label = app.field_label(f);
    let fk = f["k"].as_str().unwrap_or("");
    if f["t"] == "totp" {
        let mut kids = vec![super::ui::label(t, label)];
        kids.extend(totp_box(app, t, v, now));
        return view(json!({"gap":4,"paddingVertical":8}), kids);
    }
    let hide = hidden(f);
    let has = f["has"] == true;
    let key = format!("{fk}#{index}");
    let shown = v.shown.get(&key);
    let raw = f["v"].as_str().unwrap_or("");
    let mut value: Value = if hide {
        match shown {
            Some(s) => json!(s),
            None => json!(if has { "••••••••••" } else { "" }),
        }
    } else {
        json!(raw)
    };
    if f["t"] == "bool" {
        value = k(if raw == "true" || raw == "1" { "yes" } else { "no" });
    }
    if fk == "doc_type" && ["dni", "passport", "license", "health"].contains(&raw) {
        value = k(&format!("doc.{raw}"));
    }
    if fk == "security" && raw == "none" {
        value = k("wifi.open");
    }
    if value.as_str() == Some("") && !hide {
        return Value::Null;
    }
    let multi = value.as_str().is_some_and(|s| s.contains('\n'));
    let mono = hide || f["t"] == "pin";
    let mut vs = json!({"alignSelf":"flex-start","maxWidth":"100%","fontSize":13,"color":if f["t"] == "url" { t.accent() } else { t.text() },"lineHeight":18});
    if mono {
        vs["fontFamily"] = json!("monospace");
        vs["direction"] = json!("ltr");
    }
    if !multi {
        vs["whiteSpace"] = json!("nowrap");
        vs["textOverflow"] = json!("ellipsis");
    }
    let mut kids = vec![view(json!({"flex":1,"minWidth":0,"gap":2}), vec![super::ui::label(t, label), text(value, vs)])];
    if hide && has {
        let on = shown.is_some();
        kids.push(icon_button(t, None, Some(if on { "eyeOff" } else { "eye" }), Value::Null, k(if on { "hide" } else { "show" }), false, None,
            json!(["reveal", {"k":fk,"i":index}])));
    }
    if has || !hide {
        kids.push(icon_button(t, Some("⧉"), None, Value::Null, k("copy"), false, None, json!(["copyField", {"k":fk,"i":index}])));
    }
    view(json!({"flexDirection":"row","alignItems":if multi {"flex-start"} else {"center"},"gap":6,"paddingVertical":8}), kids)
}

pub fn is_image(a: &Value) -> bool {
    let n = a["name"].as_str().unwrap_or("").to_lowercase();
    a["mime"].as_str().unwrap_or("").starts_with("image/") || n.ends_with(".png") || n.ends_with(".jpg") || n.ends_with(".jpeg")
}
pub fn is_text(a: &Value) -> bool {
    let n = a["name"].as_str().unwrap_or("").to_lowercase();
    a["mime"].as_str().unwrap_or("").starts_with("text/")
        || [".txt", ".key", ".lic", ".pem", ".asc", ".md", ".json", ".csv"].iter().any(|e| n.ends_with(e))
}
pub fn mime_of(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.ends_with(".png") {
        "image/png"
    } else if n.ends_with(".jpg") || n.ends_with(".jpeg") {
        "image/jpeg"
    } else if [".txt", ".key", ".lic", ".md"].iter().any(|e| n.ends_with(e)) {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

fn attachments(app: &App, t: &Theme, e: &Value) -> Value {
    let att = e["att"].as_array().cloned().unwrap_or_default();
    let mut kids = vec![view(json!({"flexDirection":"row","alignItems":"center","gap":8}), vec![
        text(k("att.title"), json!({"flex":1,"minWidth":0,"fontSize":13,"fontWeight":600,"color":t.text()})),
        button(t, k("att.add"), json!(["attAdd"]), Btn { small: true, ..Btn::default() }),
    ])];
    if att.is_empty() {
        kids.push(hint(t, k("att.none"), None));
    } else {
        let rows = att
            .iter()
            .map(|a| {
                let id = a["id"].clone();
                let mut r = vec![
                    glyph(if is_image(a) { "image" } else { "clip" }, 16., &t.s(t.dim())),
                    view(json!({"flex":1,"minWidth":0}), vec![
                        text(a["name"].clone(), json!({"fontSize":13,"color":t.text(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
                        text(json!({"bytes":a["size"].as_f64().unwrap_or(0.)}), json!({"fontSize":11,"color":t.dim()})),
                    ]),
                ];
                if is_image(a) || is_text(a) {
                    r.push(icon_button(t, None, Some("eye"), Value::Null, k("att.view"), false, None, json!(["attView", id])));
                }
                r.push(icon_button(t, Some("↓"), None, Value::Null, k("att.save"), false, None, json!(["attSave", id])));
                r.push(icon_button(t, Some("✕"), None, Value::Null, k("att.remove"), false, None, json!(["attDel", id])));
                view(json!({"flexDirection":"row","alignItems":"center","gap":10,"paddingVertical":8}), r)
            })
            .collect();
        kids.push(group(t, rows, json!({})));
    }
    let _ = app;
    view(json!({"gap":8}), kids)
}

fn history(app: &App, t: &Theme, v: &Viewing, e: &Value) -> Value {
    let hist = v.hist_full.clone().unwrap_or_else(|| e["hist"].clone());
    let hist = hist.as_array().cloned().unwrap_or_default();
    if hist.is_empty() {
        return Value::Null;
    }
    let mut kids = vec![click(
        view(json!({"flexDirection":"row","alignItems":"center","gap":6,"height":24}), vec![
            text(json!("▶"), json!({"fontSize":8,"color":t.dim(),"transform":if v.hist_open {"rotate(90deg)"} else if app.rtl {"rotate(180deg)"} else {"none"}})),
            text(kp("hist.title", json!({"count":hist.len()})), json!({"fontSize":13,"fontWeight":600,"color":t.text()})),
        ]),
        json!(["histToggle"]),
    )];
    if v.hist_open {
        let mut items = vec![];
        if v.hist_full.is_none() {
            let mut b = button(t, k("hist.reveal"), json!(["histReveal"]), Btn { small: true, ..Btn::default() });
            b["style"]["alignSelf"] = json!("flex-start");
            items.push(b);
        }
        for h in &hist {
            let when = app.date_time(h["t"].as_f64().unwrap_or(0.));
            let mut lines = vec![text(json!(when), json!({"fontSize":11,"color":t.dim()}))];
            if h["title"] != e["title"] {
                lines.push(text(json!({"parts":[k("edit.title"), ": ", h["title"].as_str().unwrap_or("")]}), json!({"fontSize":12,"color":t.text()})));
            }
            for f in h["f"].as_array().into_iter().flatten() {
                let val = if let Some(s) = f["v"].as_str() { s.to_string() } else if f["has"] == true { "••••••".into() } else { String::new() };
                lines.push(text(json!({"parts":[app.field_label(f), ": ", val]}), json!({"fontSize":12,"color":t.text()})));
            }
            items.push(view(json!({"paddingHorizontal":12,"paddingVertical":8,"borderRadius":8,"border":format!("1px solid {}", t.s(t.sep())),"gap":3}), lines));
        }
        kids.push(view(json!({"gap":6}), items));
    }
    view(json!({"gap":6}), kids)
}

pub fn entry(app: &App, t: &Theme, v: &Viewing, pane_w: f64, now: i64) -> Value {
    if !v.err.is_null() {
        return empty(t, glyph("lock", 36., &t.s(t.dim())), v.err.clone(), Value::Null, json!({}));
    }
    let Some(e) = &v.e else { return view(json!({"flex":1}), vec![]) };
    let (cat, _) = app.cat_of(e);
    let b = app.builtin(cat["template"].as_str().unwrap_or(e["cat"].as_str().unwrap_or(""))).or_else(|| app.builtin(e["cat"].as_str().unwrap_or("")));
    let main = v.v == "main";
    let fields = e["f"].as_array().cloned().unwrap_or_default();
    let now_s = now as f64 / 1000.;
    let has_pw = fields.iter().any(|f| f["k"] == "password" && f["has"] == true);
    let since = e["pwch"].as_f64().filter(|x| *x > 0.).or_else(|| e["created"].as_f64()).unwrap_or(0.);
    let age_days = (has_pw && since > 0.).then(|| ((now_s - since) / DAY).floor() as i64);
    let rotate = e["rotate"].as_i64().unwrap_or(0);
    let due_in = age_days.filter(|_| rotate > 0).map(|a| rotate - a);
    let compact = pane_w < 420.;
    let mut seen = std::collections::HashMap::<String, usize>::new();
    let rows: Vec<Value> = fields
        .iter()
        .filter_map(|f| {
            let fk = f["k"].as_str().unwrap_or("").to_string();
            let i = seen.entry(fk).and_modify(|n| *n += 1).or_insert(0);
            let r = field_row(app, t, v, f, *i, now);
            (!r.is_null()).then_some(r)
        })
        .collect();
    let url = fields.iter().find(|f| f["k"] == "url").map(|f| f["v"].clone()).unwrap_or(Value::Null);
    let mut icon_entry = e.clone();
    icon_entry["url"] = url;
    let fav = e["fav"] == true;
    let mut chips = vec![chip(t, app.cat_name(&cat), cat["color"].as_str().or_else(|| b.as_ref().and_then(|b| b["color"].as_str())).unwrap_or("#8e8e93"), None)];
    for tg in e["tags"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        chips.push(chip(t, json!(format!("#{tg}")), &tag_color(&app.tags, tg), None));
    }
    let mut actions = vec![];
    if main {
        actions.push(icon_button(t, Some(if fav { "★" } else { "☆" }), None, Value::Null, k(if fav { "unfav" } else { "fav" }), fav,
            fav.then(|| json!("#ffb300")), json!(["act", {"op":"fav","on":!fav}])));
    }
    if e["trash"] != true {
        actions.push(button(t, k("edit.button"), json!(["edit"]), Btn { primary: true, ..Btn::default() }));
    }
    let title = e["title"].as_str().unwrap_or("");
    let mut kids = vec![view(json!({"flexDirection":"row","alignItems":"flex-start","gap":14}), vec![
        entry_icon(&icon_entry, &cat, b.as_ref(), if compact { 44. } else { 52. }),
        view(json!({"flex":1,"minWidth":0,"gap":6,"paddingTop":2}), vec![
            text(if title.is_empty() { k("untitled") } else { json!(title) }, json!({"alignSelf":"flex-start","maxWidth":"100%","fontSize":if compact {17} else {22},
                "fontWeight":600,"color":t.text(),"lineHeight":if compact {22} else {27},"maxHeight":if compact {44} else {54},"overflow":"hidden","textOverflow":"ellipsis","numberOfLines":2})),
            view(json!({"flexDirection":"row","flexWrap":"wrap","gap":6,"alignItems":"center"}), chips),
        ]),
        view(json!({"flexDirection":"row","alignItems":"center","gap":6,"paddingTop":if compact {8} else {12}}), actions),
    ])];
    if e["trash"] == true {
        kids.push(warning(t, k("trash.inTrash"), false));
    }
    if b.as_ref().is_some_and(|b| b["warning"] == true) {
        kids.push(warning(t, k("crypto.warning"), true));
    }
    if let Some(due) = due_in {
        let late = due <= 0;
        kids.push(view(json!({"flexDirection":"row","alignItems":"center","gap":10,"paddingHorizontal":12,"paddingVertical":10,"borderRadius":10,
            "backgroundColor":if late { json!("rgba(255,69,58,0.10)") } else { t.control() }}), vec![
            glyph("clock", 15., &t.s(if late { json!(RED) } else { t.dim() })),
            text(if late { kp("rotate.overdue", json!({"days":rotate,"ago":-due})) } else { kp("rotate.due", json!({"days":rotate,"left":due})) },
                json!({"flex":1,"fontSize":12,"lineHeight":17,"color":if late { json!(RED) } else { t.text() }})),
        ]));
    }
    if !rows.is_empty() {
        kids.push(group(t, rows, json!({})));
    }
    if e["cat"] == "wifi" || b.as_ref().is_some_and(|b| b["id"] == "wifi") {
        let mut w = vec![button(t, k(if v.qr.is_some() { "wifi.hideQr" } else { "wifi.showQr" }), json!(["qr"]), Btn { small: true, ..Btn::default() })];
        if let Some((side, _)) = &v.qr {
            w.push(view(json!({"padding":12,"borderRadius":12,"backgroundColor":"#fff","gap":6,"alignItems":"center","border":format!("1px solid {}", t.s(t.sep()))}), vec![
                json!({"kind":"raster","key":"qr","image":"qr","generation":v.qr_gen,"style":{"width":side,"height":side,"borderRadius":8}}),
                text(k("wifi.scan"), json!({"fontSize":11,"color":"#333"})),
            ]));
        }
        kids.push(view(json!({"gap":10,"alignItems":"flex-start"}), w));
    }
    if let Some(notes) = e["notes"].as_str().filter(|s| !s.is_empty()) {
        let width = (pane_w - 74.).max(160.);
        let md = crate::help::markdown::render(&crate::help::markdown::parse(notes), width, t.0);
        kids.push(view(json!({"gap":6}), vec![
            label(t, k("field.notes")),
            view(json!({"paddingHorizontal":12,"paddingVertical":10,"borderRadius":10,"border":format!("1px solid {}", t.s(t.sep())),
                "backgroundColor":if t.dark() {"rgba(255,255,255,0.04)"} else {"rgba(0,0,0,0.025)"}}), vec![md]),
        ]));
    }
    if main && e["trash"] != true {
        kids.push(attachments(app, t, e));
    }
    kids.push(history(app, t, v, e));
    let created = app.date(e["created"].as_f64().unwrap_or(0.));
    let modified = app.date_time(e["modified"].as_f64().unwrap_or(0.));
    kids.push(text(kp("dates", json!({"created":created,"modified":modified})), json!({"fontSize":11,"color":t.dim()})));
    let mut foot = vec![];
    let trash = e["trash"] == true;
    if main && !trash {
        let arch = e["arch"] == true;
        foot.push(button(t, k(if arch { "unarchive" } else { "archive" }), json!(["act", {"op":"archive","on":!arch}]), Btn { small: true, ..Btn::default() }));
        foot.push(button(t, k("trash.move"), json!(["act", {"op":"trash"}]), Btn { small: true, destructive: true, ..Btn::default() }));
    }
    if trash {
        foot.push(button(t, k("trash.restore"), json!(["act", {"op":"restore"}]), Btn { small: true, ..Btn::default() }));
    }
    if trash || v.v == "login" {
        foot.push(button(t, k("trash.purge"), json!(["act", {"op":"purge"}]), Btn { small: true, destructive: true, ..Btn::default() }));
    }
    kids.push(view(json!({"flexDirection":"row","gap":8,"flexWrap":"wrap","paddingTop":12,"borderTopWidth":1,"borderTopColor":t.sep()}), foot));
    json!({"kind":"scroll","key":format!("entry:{}", v.id),"style":{"flex":1},"children":[
        view(json!({"paddingHorizontal":24,"paddingTop":20,"paddingBottom":24,"gap":16}), kids.into_iter().filter(|k| !k.is_null()).collect()),
    ]})
}

fn field_editor(app: &App, t: &Theme, ed: &Editor, i: usize) -> Value {
    let f = &ed.fields[i];
    let ft = f["t"].as_str().unwrap_or("text");
    let hide = hidden(f) && ft != "totp";
    let keep = f["keep"] == true;
    let show = f["show"] == true;
    let multiline = f["multiline"] == true || ft == "multiline";
    let val = f["v"].as_str().unwrap_or("");
    let input_node = if ft == "bool" {
        toggle(t, val == "true", false, json!(["fbool", i]))
    } else if let Some(choices) = f["choices"].as_array() {
        let fk = f["k"].as_str().unwrap_or("");
        let cap = |c: &str| {
            if fk == "doc_type" {
                k(&format!("doc.{c}"))
            } else if fk == "security" && c == "none" {
                k("wifi.open")
            } else {
                json!(c)
            }
        };
        let id = format!("fch:{i}");
        let current = choices.iter().find(|c| c.as_str() == Some(val)).map(|c| cap(c.as_str().unwrap_or(""))).unwrap_or(json!(val));
        let _ = choices;
        choice(t, &id, current, app.choice.as_deref() == Some(id.as_str()), 240.)
    } else if multiline {
        let inner = if keep {
            button(t, k("edit.replaceSecret"), json!(["freplace", i]), Btn { small: true, ..Btn::default() })
        } else {
            textarea(t, &format!("f:{i}"), val, Value::Null, 70., hide, hide && !show, app.ta.as_deref() == Some(&format!("f:{i}")))
        };
        view(json!({"flex":1,"minWidth":0,"alignItems":if keep {"flex-start"} else {"stretch"}}), vec![inner])
    } else if keep {
        view(json!({"flex":1,"minWidth":0,"height":28,"flexDirection":"row","gap":8,"alignItems":"center","paddingHorizontal":10,"borderRadius":7,
            "border":format!("1px dashed {}", t.s(t.sep()))}), vec![
            text(k("edit.keptSecret"), json!({"flex":1,"minWidth":0,"fontSize":13,"color":t.dim(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
            {
                let mut b = button(t, k("edit.replaceSecret"), json!(["freplace", i]), Btn { small: true, plain: true, ..Btn::default() });
                b["style"]["marginInlineEnd"] = json!(-6);
                b
            },
        ])
    } else {
        let ph = if ft == "totp" { k("totp.placeholder") } else if ft == "date" { k("edit.datePh") } else { json!("") };
        input(&format!("fv:{i}"), val, ph, hide && !show, false, None, json!({"flex":1,"minWidth":0}))
    };
    let head_label = if f["custom"] == true {
        input(&format!("fl:{i}"), f["l"].as_str().unwrap_or(""), k("edit.fieldName"), false, false, None, json!({"height":24,"width":200,"fontSize":12}))
    } else {
        label(t, app.field_label(f))
    };
    let mut head = vec![view(json!({"flex":1,"minWidth":0}), vec![head_label])];
    if app.field_removable(ed, i) {
        head.push(button(t, k("edit.removeField"), json!(["fremove", i]), Btn { small: true, plain: true, destructive: true, ..Btn::default() }));
    }
    let mut line = vec![input_node];
    if hide && !keep && f["multiline"] != true {
        line.push(icon_button(t, None, Some(if show { "eyeOff" } else { "eye" }), Value::Null, k(if show { "hide" } else { "show" }), false, None, json!(["fshow", i])));
    }
    if f["k"] == "password" && !keep {
        line.push(icon_button(t, Some("⚄"), None, k("edit.generate"), k("menu.generator"), false, None, json!(["fgen", i])));
    }
    if ft == "totp" {
        line.push(icon_button(t, Some("▦"), None, k("totp.fromQr"), Value::Null, false, None, json!(["fqr", i])));
    }
    view(json!({"gap":6}), vec![
        view(json!({"flexDirection":"row","alignItems":"center","minHeight":16,"gap":8}), head),
        view(json!({"flexDirection":"row","alignItems":"center","gap":4}), line),
    ])
}

pub const ROTATE: [i64; 6] = [0, 30, 60, 90, 180, 365];
pub fn rotate_caption(n: i64) -> Value {
    match n {
        0 => k("rotate.never"),
        365 => k("rotate.year"),
        n => kp("rotate.days", json!({"n":n})),
    }
}

pub fn editor(app: &App, t: &Theme, ed: &Editor, pane_w: f64) -> Value {
    let is_new = ed.entry.is_none();
    let stack = pane_w < 440.;
    let cats = app.all_cats();
    let cat_v = cats.as_array().into_iter().flatten().find(|c| c["id"] == ed.cat.as_str()).cloned();
    let b = app.builtin(&ed.cat).or_else(|| cat_v.as_ref().and_then(|c| app.builtin(c["template"].as_str().unwrap_or(""))));
    let cat_label = cat_v.as_ref().map(|c| app.cat_name(c)).unwrap_or(json!(ed.cat));
    let mut title_input = input("title", &ed.title, k("edit.titlePh"), false, is_new, None, json!({}));
    if !is_new {
        title_input.as_object_mut().unwrap().remove("autoFocus");
    }
    let mut kids = vec![
        text(k(if is_new { "edit.new" } else { "edit.editing" }), json!({"fontSize":17,"fontWeight":600,"color":t.text()})),
        view(json!({"flexDirection":if stack {"column"} else {"row"},"gap":if stack {16} else {12},"alignItems":if stack {"stretch"} else {"flex-end"}}), vec![
            view(json!({"flex":if stack { Value::Null } else { json!(1) },"minWidth":0,"gap":6}), vec![label(t, k("edit.title")), title_input]),
            view(json!({"gap":6}), vec![label(t, k("edit.category")),
                choice(t, "cat", cat_label, app.choice.as_deref() == Some("cat"), if stack { (pane_w - 48.).max(160.) } else { 200. })]),
        ]),
    ];
    if b.as_ref().is_some_and(|b| b["warning"] == true) {
        kids.push(warning(t, k("crypto.warning"), true));
    }
    for i in 0..ed.fields.len() {
        kids.push(field_editor(app, t, ed, i));
    }
    if let Some(s) = &ed.strength {
        kids.push(strength_bar(t, s["score"].as_i64().unwrap_or(0), &s["bits"]));
        if s["common"] == true {
            kids.push(text(k("report.commonHint"), json!({"fontSize":12,"color":RED})));
        }
    }
    if ed.adding {
        kids.push(view(json!({"flexDirection":"row","flexWrap":"wrap","gap":8,"alignItems":"center"}), vec![
            choice(t, "ftype", k(&format!("ftype.{}", ed.new_type)), app.choice.as_deref() == Some("ftype"), 170.),
            button(t, k("cancel"), json!(["addCancel"]), Btn { small: true, ..Btn::default() }),
            button(t, k("edit.addField"), json!(["addField"]), Btn { small: true, ..Btn::default() }),
        ]));
    } else {
        let mut b = button(t, k("edit.addFieldDots"), json!(["addOpen"]), Btn { small: true, icon: Some("+"), ..Btn::default() });
        b["style"]["alignSelf"] = json!("flex-start");
        kids.push(b);
    }
    kids.push(view(json!({"height":1,"backgroundColor":t.sep()}), vec![]));
    let mut tag_row: Vec<Value> = ed.tags.iter().map(|tg| chip(t, json!(format!("#{tg}")), &tag_color(&app.tags, tg), Some(json!(["tagDel", tg])))).collect();
    tag_row.push(input("tagIn", &ed.tag_in, k("edit.tagPh"), false, false, Some("tagAdd"), json!({"width":150,"height":24,"fontSize":12})));
    kids.push(view(json!({"gap":6}), vec![label(t, k("edit.tags")), view(json!({"flexDirection":"row","flexWrap":"wrap","gap":6,"alignItems":"center"}), tag_row)]));
    kids.push(view(json!({"flexDirection":"row","flexWrap":"wrap","alignItems":"center","gap":12}), vec![
        text(k("rotate.label"), json!({"flex":1,"minWidth":160,"fontSize":13,"color":t.text()})),
        choice(t, "rotate", rotate_caption(ed.rotate), app.choice.as_deref() == Some("rotate"), 170.),
    ]));
    let notes_first = b.as_ref().is_some_and(|b| b["notesFirst"] == true);
    kids.push(view(json!({"gap":6}), vec![
        label(t, k("field.notes")),
        textarea(t, "notes", &ed.notes, k("edit.notesPh"), if notes_first { 180. } else { 80. }, false, false, app.ta.as_deref() == Some("notes")),
        hint(t, k("edit.markdown"), None),
    ]));
    if is_new && b.as_ref().is_some_and(|b| b["attachments"] == true) {
        kids.push(hint(t, k("edit.attachLater"), None));
    }
    if !ed.err.is_null() {
        kids.push(text(ed.err.clone(), json!({"fontSize":12,"color":RED})));
    }
    view(json!({"flex":1}), vec![
        json!({"kind":"scroll","key":"editor","style":{"flex":1},"children":[view(json!({"paddingHorizontal":24,"paddingTop":20,"paddingBottom":24,"gap":16}), kids)]}),
        view(json!({"height":53,"flexDirection":"row","alignItems":"center","justifyContent":"flex-end","gap":8,"paddingHorizontal":20,"borderTopWidth":1,"borderTopColor":t.sep()}), vec![
            button(t, k("cancel"), json!(["editCancel"]), Btn::default()),
            button(t, k(if ed.busy { "saving" } else { "save" }), json!(["editSave"]), Btn { primary: true, ..Btn::default() }),
        ]),
    ])
}
