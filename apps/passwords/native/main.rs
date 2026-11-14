//! The unlocked window (apps/passwords/index.jsx): a source list (search, sections, categories,
//! tags), the item list and the detail pane; under 960 px the columns get narrower.
use super::glyphs::glyph;
use super::ui::*;
use super::{detail, security, sheets, App};
use serde_json::{json, Value};

/// A source-list row: 28 px, a glyph or tile, the label and a count; `twisty`: the disclosure
/// triangle of categories with children.
#[allow(clippy::too_many_arguments)]
fn side_row(t: &Theme, rtl: bool, key: &str, icon: Value, label: Value, count: Option<i64>, selected: bool, action: Value, depth: usize,
    dbl: Option<Value>, twisty: Option<(bool, Value)>) -> Value {
    let mut kids = vec![];
    match twisty {
        Some((open, act)) => kids.push(click(
            view(json!({"width":12,"height":20,"marginInlineEnd":-4,"alignItems":"center","justifyContent":"center"}), vec![
                text(json!("▶"), json!({"fontSize":8,"color":if selected { json!("#fff") } else { t.dim() },
                    "transform":if open {"rotate(90deg)"} else if rtl {"rotate(180deg)"} else {"none"}})),
            ]),
            act,
        )),
        None if depth > 0 => kids.push(view(json!({"width":12,"marginInlineEnd":-4}), vec![])),
        None => {}
    }
    kids.push(icon);
    kids.push(text(label, json!({"flex":1,"minWidth":0,"fontSize":13,"fontWeight":500,"color":if selected { json!("#fff") } else { t.text() },
        "whiteSpace":"nowrap","textOverflow":"ellipsis"})));
    if let Some(n) = count.filter(|n| *n > 0) {
        kids.push(text(json!(n.to_string()), json!({"fontSize":11,"color":if selected { json!("rgba(255,255,255,0.85)") } else { t.dim() }})));
    }
    let mut row = keyed(key, json!({"height":28,"paddingInlineStart":8 + depth * 16,"paddingInlineEnd":10,"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":8,
        "backgroundColor":if selected { t.accent() } else { json!("transparent") }}), kids);
    row = click(row, action);
    if let Some(d) = dbl {
        row = on(row, "onDoubleClick", d);
    }
    if !selected {
        row = hover(row, json!({"backgroundColor":t.hover()}));
    }
    row
}

fn side_glyph(t: &Theme, kind: &str, selected: bool) -> Value {
    glyph(kind, 15., &t.s(if selected { json!("#fff") } else { t.accent() }))
}

fn sidebar(app: &App, t: &Theme) -> Value {
    let cats = app.all_cats();
    let counts = app.summary(&cats);
    let n = |path: &str| counts.pointer(path).and_then(Value::as_i64).unwrap_or(0);
    let sel = app.win.sel.as_str();
    let row = |key: &str, glyph_kind: &str, label: &str, count: i64| {
        side_row(t, app.rtl, &format!("side:{key}"), side_glyph(t, glyph_kind, sel == key), k(label), Some(count), sel == key, json!(["sel", key]), 0, None, None)
    };
    let mut kids = vec![
        row("all", "lock", "side.all", n("/all")),
        row("fav", "star", "side.fav", n("/fav")),
        row("codes", "clock", "side.codes", n("/codes")),
        row("security", "shield", "side.security", app.win.issues),
        row("cat:wifi", "wifi", "side.wifi", n("/categories/wifi")),
        row("cat:notes", "note", "side.notes", n("/categories/notes")),
        row("cat:cards", "card", "side.cards", n("/categories/cards")),
        row("archived", "archive", "side.archived", n("/archived")),
        view(json!({"flexDirection":"row","alignItems":"center","marginTop":8}), vec![
            view(json!({"flex":1}), vec![section(t, k("side.categories"))]),
            {
                let mut b = tool(t, Tool { icon: json!("+"), icon_size: 15., small: true, color: Some(t.dim()), tip: k("menu.newCat"), ..Tool::default() }, json!(["newCat"]));
                b["style"]["marginTop"] = json!(6);
                b
            },
        ]),
    ];
    // the category tree
    fn tree(app: &App, t: &Theme, cats: &[Value], counts: &Value, pid: &str, depth: usize, out: &mut Vec<Value>) {
        if depth > 16 {
            return;
        }
        let sel = app.win.sel.as_str();
        for c in cats.iter().filter(|c| c["parent"].as_str().unwrap_or("") == pid) {
            let id = c["id"].as_str().unwrap_or("");
            let n = counts["categories"][id].as_i64().unwrap_or(0);
            let has_kids = cats.iter().any(|x| x["parent"].as_str().unwrap_or("") == id && !id.is_empty());
            let key = format!("cat:{id}");
            if c["builtin"] == true && n == 0 && !has_kids && sel != key {
                continue;            // empty built-in ones take no room
            }
            let open = app.win.open.get(id).copied().unwrap_or(true);
            let dbl = (c["builtin"] != true).then(|| json!(["editCat", id]));
            out.push(side_row(t, app.rtl, &format!("side:{key}"), cat_icon(c, app.builtin(id).as_ref(), 18.), app.cat_name(c), Some(n), sel == key,
                json!(["sel", key]), depth, dbl, has_kids.then(|| (open, json!(["twisty", id])))));
            if has_kids && open {
                tree(app, t, cats, counts, id, depth + 1, out);
            }
        }
    }
    tree(app, t, cats.as_array().map(Vec::as_slice).unwrap_or(&[]), &counts, "", 0, &mut kids);
    let tags = app.live_tags();
    if !tags.is_empty() {
        kids.push(view(json!({"marginTop":8}), vec![section(t, k("side.tags"))]));
    }
    for tg in &tags {
        let key = format!("tag:{tg}");
        let dot = view(json!({"width":18,"alignItems":"center"}), vec![view(json!({"width":9,"height":9,"borderRadius":5,"backgroundColor":tag_color(&app.tags, tg)}), vec![])]);
        kids.push(side_row(t, app.rtl, &format!("side:{key}"), dot, json!(tg), Some(counts["tags"][tg.as_str()].as_i64().unwrap_or(0)), sel == key,
            json!(["sel", key]), 0, Some(json!(["editTag", tg])), None));
    }
    kids.push(view(json!({"marginTop":8}), vec![section(t, k("side.system"))]));
    kids.push(side_row(t, app.rtl, "side:login", side_glyph(t, "person", sel == "login"), k("side.login"), Some(app.login.len() as i64), sel == "login",
        json!(["sel", "login"]), 0, None, None));
    kids.push(row("trash", "trash", "side.trash", n("/trash")));
    json!({"kind":"scroll","key":"sidebar","style":{"flex":1},"children":[view(json!({"paddingHorizontal":10,"paddingBottom":12,"gap":2}), kids)]})
}

fn list_row(app: &App, t: &Theme, e: &Value, selected: bool) -> Value {
    let dim = if selected { json!("rgba(255,255,255,0.82)") } else { t.dim() };
    let (cat, b) = app.cat_of(e);
    let mut kids = vec![
        entry_icon(e, &cat, b.as_ref(), 30.),
        view(json!({"flex":1,"minWidth":0,"gap":1}), vec![
            text(if e["title"].as_str().unwrap_or("").is_empty() { k("untitled") } else { e["title"].clone() },
                json!({"fontSize":13,"fontWeight":600,"color":if selected { json!("#fff") } else { t.text() },"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
            text(if e["sub"].as_str().unwrap_or("").is_empty() { app.cat_name(&cat) } else { e["sub"].clone() },
                json!({"fontSize":12,"color":dim,"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
        ]),
    ];
    if e["natt"].as_u64().unwrap_or(0) > 0 {
        kids.push(glyph("clip", 13., &t.s(dim.clone())));
    }
    if e["totp"] == true {
        kids.push(text(json!("2FA"), json!({"fontSize":10,"fontWeight":600,"color":dim})));
    }
    if e["fav"] == true {
        kids.push(text(json!("★"), json!({"fontSize":12,"color":if selected {"#fff"} else {"#ffb300"}})));
    }
    let id = e["id"].as_str().unwrap_or("");
    let mut row = click(
        keyed(&format!("row:{id}"), json!({"height":48,"flexDirection":"row","alignItems":"center","gap":10,"paddingHorizontal":10,"borderRadius":8,
            "backgroundColor":if selected { t.accent() } else { json!("transparent") }}), kids),
        json!(["open", id]),
    );
    if !selected {
        row = hover(row, json!({"backgroundColor":t.hover()}));
    }
    row
}

/// The title of the list for the selection.
fn list_title(app: &App) -> Value {
    let sel = app.win.sel.as_str();
    match sel {
        "all" => k("side.all"),
        "fav" => k("side.fav"),
        "archived" => k("side.archived"),
        "trash" => k("side.trash"),
        "login" => k("side.login"),
        _ if sel.starts_with("tag:") => json!(format!("#{}", &sel[4..])),
        _ if sel.starts_with("cat:") => {
            let id = &sel[4..];
            let cats = app.all_cats();
            let c = cats.as_array().into_iter().flatten().find(|c| c["id"] == id).cloned().or_else(|| app.builtin(id)).unwrap_or(Value::Null);
            app.cat_name(&c)
        }
        _ => json!(""),
    }
}

pub fn window(app: &App, t: &Theme, now: i64) -> Value {
    let narrow = app.width < 960.;
    let (side_w, list_w) = if narrow { (200., 250.) } else { (220., 290.) };
    let sel = app.win.sel.as_str();
    let full = sel == "security" || sel == "codes";
    let lock_icon = glyph("lock", 14., &t.s(t.text()));
    let side = view(json!({"width":side_w,"backgroundColor":t.get("sidebarBg", "#26262b"),"borderInlineEndWidth":1,"borderInlineEndColor":t.sep()}), vec![
        view(json!({"paddingHorizontal":10,"paddingTop":10,"paddingBottom":6}), vec![search(t, "q", &app.win.q, k("search"), app.rtl)]),
        sidebar(app, t),
        view(json!({"height":41,"flexDirection":"row","alignItems":"center","gap":4,"paddingHorizontal":10,"borderTopWidth":1,"borderTopColor":t.sep()}), vec![
            tool(t, Tool { icon: json!("⚄"), icon_size: 16., tip: k("menu.generator"), ..Tool::default() }, json!(["generator"])),
            tool(t, Tool { icon: json!("⚙"), icon_size: 15., tip: k("menu.settings"), ..Tool::default() }, json!(["settings"])),
            view(json!({"flex":1}), vec![]),
            tool(t, Tool { icon: lock_icon, label: k("lock"), tip: k("menu.lock"), ..Tool::default() }, json!(["lock"])),
        ]),
    ]);
    let mut kids = vec![side];
    if full {
        let pane = if sel == "security" { security::report(app, t) } else { security::codes(app, t, now) };
        kids.push(view(json!({"flex":1,"minWidth":0}), vec![pane]));
    } else {
        let list = app.visible_list();
        let can_add = sel != "login" && sel != "trash";
        let searching = !app.win.q.trim().is_empty();
        let count_line = match app.win.search_ms {
            Some(ms) if searching && sel != "login" => kp("list.found", json!({"count":list.len(),"ms":format!("{ms:.1}")})),
            _ => kp("list.count", json!({"count":list.len()})),
        };
        let mut head = vec![
            view(json!({"flex":1,"minWidth":0}), vec![
                text(list_title(app), json!({"fontSize":17,"fontWeight":600,"color":t.text(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
                text(count_line, json!({"fontSize":11,"color":t.dim(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
            ]),
            tool(t, Tool { label: k(if app.win.sort_title { "list.sortName" } else { "list.sortDate" }), small: true, color: Some(t.dim()), tip: k("list.sortHint"), ..Tool::default() },
                json!(["sort"])),
        ];
        if can_add {
            head.push(tool(t, Tool { icon: json!("+"), icon_size: 17., tip: k("menu.new"), ..Tool::default() }, json!(["new"])));
        }
        let mut col = vec![view(json!({"height":56,"flexDirection":"row","alignItems":"center","paddingInlineStart":16,"paddingInlineEnd":10,"gap":4}), head)];
        if sel == "login" {
            col.push(text(k("login.hint"), json!({"fontSize":11,"color":t.dim(),"paddingHorizontal":16,"paddingBottom":8,"lineHeight":15})));
        }
        if sel == "trash" && !list.is_empty() {
            col.push(view(json!({"paddingHorizontal":16,"paddingBottom":8,"alignItems":"flex-start"}), vec![
                button(t, k("trash.empty"), json!(["emptyTrash"]), Btn { small: true, destructive: true, ..Btn::default() }),
            ]));
        }
        if list.is_empty() {
            let kind = if !app.win.q.is_empty() { "note" } else if sel == "trash" { "trash" } else { "lock" };
            let hint = if !app.win.q.is_empty() { Value::Null } else if can_add { k("list.emptyHint") } else { Value::Null };
            col.push(empty(t, glyph(kind, 30., &t.s(t.dim())), k(if !app.win.q.is_empty() { "list.noResults" } else { "list.empty" }), hint,
                json!({"justifyContent":"flex-start","paddingTop":72})));
        } else {
            let mut rows: Vec<Value> = list.iter().take(600).map(|e| list_row(app, t, e, app.win.cur.as_deref() == e["id"].as_str())).collect();
            if list.len() > 600 {
                rows.push(text(kp("list.more", json!({"count":list.len() - 600})), json!({"fontSize":11,"color":t.dim(),"padding":10})));
            }
            col.push(json!({"kind":"scroll","key":"list","style":{"flex":1},"children":[view(json!({"paddingHorizontal":8,"paddingBottom":10,"gap":2}), rows)]}));
        }
        kids.push(view(json!({"width":list_w,"borderInlineEndWidth":1,"borderInlineEndColor":t.sep()}), col));
        let pane_w = (app.width - side_w - list_w - 2.).max(200.);
        let detail = if let Some(ed) = &app.editor {
            detail::editor(app, t, ed, pane_w)
        } else if let Some(v) = &app.viewing {
            detail::entry(app, t, v, pane_w, now)
        } else {
            let some = !list.is_empty();
            empty(t, app_icon(56.), k(if some { "pick" } else { "list.empty" }), k(if some { "pickHint" } else { "emptyHint" }), json!({}))
        };
        kids.push(view(json!({"flex":1,"minWidth":0}), vec![detail]));
    }
    if let Some(sheet) = sheets::sheet(app, t) {
        kids.push(sheet);
    }
    view(json!({"flex":1,"flexDirection":"row","backgroundColor":t.bg()}), kids)
}
