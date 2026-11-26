//! Mail's widget trees: the sidebar, the toolbar, the message list, the reading pane and the
//! sheets over them. Drawing a message also says which of its images are still missing.
use super::html::{self, Wanted};
use super::model::{self, CATEGORY_COLORS, Source};
use super::sheets::{self, Setup, Sheet, Step};
use super::window::{list_height, Main};
use super::{list_of, str_of, Base};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(crate) const TOOLBAR: f64 = 46.;
pub(crate) const LIST_TITLE: f64 = 40.;
pub(crate) const SEARCH: f64 = 38.;
const ROW: f64 = model::ROW;
const SIDE_ROW: f64 = 28.;

// ---------------------------------------------------------------- pieces shared with compose

pub(crate) fn view(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
pub(crate) fn text(label: impl Into<Value>, style: Value) -> Value {
    json!({"kind":"text","label":label.into(),"style":style})
}
pub(crate) fn on(mut node: Value, event: &str, action: Value) -> Value {
    node["events"][event] = action;
    node
}
pub(crate) fn click(node: Value, action: Value) -> Value {
    on(node, "onClick", action)
}
/// One line of text that is cut with an ellipsis.
pub(crate) fn clipped(mut node: Value) -> Value {
    node["style"]["whiteSpace"] = json!("nowrap");
    node["style"]["overflow"] = json!("hidden");
    node["style"]["textOverflow"] = json!("ellipsis");
    node
}
pub(crate) fn scroll(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"scroll","key":key,"style":style,"children":children})
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Looks {
    pub primary: bool,
    pub destructive: bool,
    pub small: bool,
    pub plain: bool,
    pub disabled: bool,
}
pub(crate) const PRIMARY: Looks = Looks { primary: true, destructive: false, small: false, plain: false, disabled: false };
pub(crate) const LINK: Looks = Looks { primary: false, destructive: false, small: true, plain: true, disabled: false };
pub(crate) fn button(key: &str, label: &str, action: Value, looks: Looks) -> Value {
    json!({"kind":"button","key":key,"label":label,"action":action,"primary":looks.primary,"destructive":looks.destructive,
        "small":looks.small,"plain":looks.plain,"disabled":looks.disabled})
}
pub(crate) fn field(key: &str, value: &str, placeholder: &str, change: Value) -> Value {
    json!({"kind":"input","key":key,"value":value,"placeholder":placeholder,"change":change,"size":13,
        "style":{"flex":1,"minWidth":0,"height":28}})
}
pub(crate) fn rule(base: &Base, key: &str) -> Value {
    view(key, json!({"height":1,"flexShrink":0,"backgroundColor":base.token("separator", "#3a3a40")}), vec![])
}
/// A hairline at the end edge of a column.
fn edge(base: &Base, key: &str) -> Value {
    view(key, json!({"position":"absolute","end":0,"top":0,"bottom":0,"width":1,"pointerEvents":"none","backgroundColor":base.token("separator", "#3a3a40")}), vec![])
}
fn row(key: &str, gap: f64, children: Vec<Value>) -> Value {
    view(key, json!({"flexDirection":"row","alignItems":"center","gap":gap}), children)
}
fn spacer(key: &str) -> Value {
    view(key, json!({"flex":1}), vec![])
}

// ---------------------------------------------------------------- our own drawings

/// An envelope on a blue tile.
pub(crate) fn icon(size: f64) -> Value {
    let s = if size.is_finite() { size.clamp(1., 512.) } else { 32. };
    let mut kids = vec![view(
        "paper",
        json!({"position":"absolute","left":s*0.16,"top":s*0.27,"width":s*0.68,"height":s*0.46,"borderRadius":s*0.06,"backgroundColor":"#ffffff",
            "boxShadow":format!("0 {}px {}px rgba(0,0,0,0.25)", s * 0.02, s * 0.05)}),
        vec![],
    )];
    // the flap: stacked bands narrowing to a point
    for i in 0..5 {
        let n = f64::from(i);
        kids.push(view(&format!("flap-{i}"), json!({"position":"absolute","left":s*(0.2+n*0.06),"top":s*(0.31+n*0.045),"width":s*(0.6-n*0.12),
            "height":(s*0.04).max(1.),"borderRadius":s*0.02,"backgroundColor":if i == 4 { "#0a5fd0" } else { "#cfe6ff" }}), vec![]));
    }
    view("mail-icon", json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(165deg, #3fa9ff, #0a5fd0)","overflow":"hidden",
        "boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)", s * 0.04, s * 0.12)}), kids)
}

/// Monochrome shapes drawn with views (color emoji would ignore the theme).
fn glyph(key: &str, kind: &str, s: f64, color: &str) -> Value {
    let line = 1.5;
    let bar = |key: &str, style: Value| view(key, style, vec![]);
    let frame = |children| view(key, json!({"width":s,"height":s,"justifyContent":"center","alignItems":"center","gap":1,"flexShrink":0}), children);
    match kind {
        "tray" => frame(vec![view("box", json!({"width":s*0.86,"height":s*0.62,"borderRadius":s*0.14,"borderWidth":line,"borderColor":color}), vec![
            bar("slot", json!({"position":"absolute","left":0,"right":0,"top":s*0.22,"height":line,"backgroundColor":color})),
        ])]),
        "archive" => frame(vec![
            bar("lid", json!({"width":s*0.9,"height":s*0.2,"borderRadius":1.5,"backgroundColor":color})),
            view("box", json!({"width":s*0.76,"height":s*0.48,"borderBottomLeftRadius":s*0.1,"borderBottomRightRadius":s*0.1,"borderWidth":line,"borderColor":color,"alignItems":"center"}),
                vec![bar("handle", json!({"marginTop":s*0.1,"width":s*0.28,"height":line,"backgroundColor":color}))]),
        ]),
        "trash" => frame(vec![
            bar("knob", json!({"width":s*0.3,"height":s*0.1,"borderTopLeftRadius":1.5,"borderTopRightRadius":1.5,"backgroundColor":color})),
            bar("lid", json!({"width":s*0.8,"height":line,"backgroundColor":color})),
            bar("can", json!({"width":s*0.6,"height":s*0.54,"borderBottomLeftRadius":s*0.1,"borderBottomRightRadius":s*0.1,"borderWidth":line,"borderColor":color})),
        ]),
        "calendar" => frame(vec![view("page", json!({"width":s*0.8,"height":s*0.74,"borderRadius":s*0.14,"borderWidth":line,"borderColor":color,"overflow":"hidden"}),
            vec![bar("band", json!({"height":s*0.18,"backgroundColor":color}))])]),
        _ => frame(vec![view("folder", json!({"width":s*0.86,"height":s*0.66}), vec![
            bar("tab", json!({"width":s*0.36,"height":s*0.16,"borderTopLeftRadius":s*0.08,"borderTopRightRadius":s*0.08,"backgroundColor":color})),
            bar("body", json!({"flex":1,"borderRadius":s*0.1,"borderTopLeftRadius":0,"borderWidth":line,"borderColor":color})),
        ])]),
    }
}
fn mark(label: &str, size: f64, color: &str) -> Value {
    text(label, json!({"fontSize":size,"color":color,"textAlign":"center"}))
}
fn folder_icon(folder: &Value, color: &str) -> Value {
    match str_of(&folder["special"]) {
        "inbox" => glyph("glyph", "tray", 16., color),
        "archive" => glyph("glyph", "archive", 16., color),
        "trash" => glyph("glyph", "trash", 16., color),
        "drafts" => mark("✎", 13., color),
        "sent" => mark("➤", 12., color),
        "junk" => mark("⚠", 13., color),
        "all" => mark("≡", 13., color),
        "flagged" => mark("⚑", 13., color),
        _ => glyph("glyph", "folder", 16., color),
    }
}
fn dot(key: &str, size: f64, color: &str) -> Value {
    view(key, json!({"width":size,"height":size,"borderRadius":size/2.,"flexShrink":0,"backgroundColor":color}), vec![])
}

// ---------------------------------------------------------------- sidebar

struct SideRow<'a> {
    key: String,
    icon: Value,
    label: &'a str,
    count: u64,
    selected: bool,
    indent: f64,
}
fn side_row(base: &Base, it: SideRow, action: Value) -> Value {
    let color = if it.selected { "#ffffff".to_owned() } else { base.token("text", "#ececf0") };
    let count_color = if it.selected { "rgba(255,255,255,0.85)".to_owned() } else { base.token("textDim", "#8e8e96") };
    let mut kids = vec![
        view("icon", json!({"width":18,"alignItems":"center","justifyContent":"center"}), vec![it.icon]),
        clipped(text(it.label, json!({"flex":1,"fontSize":13,"color":color}))),
    ];
    if it.count > 0 {
        kids.push(text(it.count.to_string(), json!({"fontSize":11,"fontWeight":500,"color":count_color})));
    }
    let background = if it.selected { base.token("accent", "#0a84ff") } else { "transparent".to_owned() };
    let mut node = click(
        view(&it.key, json!({"height":SIDE_ROW,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":8,"paddingStart":8.+it.indent*14.,
            "paddingEnd":8,"borderRadius":6,"backgroundColor":background}), kids),
        action,
    );
    if !it.selected {
        node["hoverStyle"] = json!({"backgroundColor":base.token("hover", "rgba(127,127,127,0.14)")});
    }
    node
}
fn caption(base: &Base, key: &str, label: &str, top: f64) -> Value {
    view(key, json!({"flexShrink":0,"marginHorizontal":8,"marginTop":top,"marginBottom":4}), vec![
        text(label, json!({"fontSize":11,"fontWeight":600,"color":base.token("textDim", "#8e8e96")})),
    ])
}

fn sidebar(base: &mut Base, main: &Main, width: f64) -> Value {
    let accent = base.token("accent", "#0a84ff");
    let dim = base.token("textDim", "#8e8e96");
    let color = |selected: bool| if selected { "#ffffff".to_owned() } else { accent.clone() };
    let label = base.t("favorites");
    let mut rows = vec![caption(base, "favorites", &label, 4.)];
    let unread = base.facts.status["unread"].as_u64().unwrap_or(0);
    for (filter, label, icon, count) in [("all", "unifiedInboxShort", "", unread), ("unread", "unread", "●", 0), ("flagged", "flagged", "⚑", 0)] {
        let selected = matches!(&main.source, Source::Unified { filter: f, label: 0 } if *f == filter);
        let label = base.t(label);
        let drawn = if icon.is_empty() { glyph("glyph", "tray", 16., &color(selected)) } else { mark(icon, if icon == "●" { 10. } else { 13. }, &color(selected)) };
        rows.push(side_row(base, SideRow { key: format!("unified:{filter}"), icon: drawn, label: &label, count, selected, indent: 0. }, json!(["unified", filter])));
    }
    let warns = |base: &Base, account: &str, state: &str| {
        base.facts.status["authFailed"][account] == true || matches!(state, "auth" | "cert" | "offline")
    };
    let header = |account: &Value, state: &str, bad: bool, calendar: bool| {
        let id = str_of(&account["id"]);
        let mut kids = Vec::new();
        if calendar {
            kids.push(glyph("kind", "calendar", 12., &dim));
        }
        kids.push(clipped(text(str_of(&account["name"]), json!({"flex":1,"fontSize":11,"fontWeight":600,"color":dim}))));
        if state == "sync" || state == "connecting" && !calendar {
            kids.push(text("⟳", json!({"fontSize":10,"color":dim})));
        }
        if bad {
            kids.push(click(view("warning", json!({"paddingHorizontal":4}), vec![text("⚠", json!({"fontSize":11,"color":"#ff9f0a"}))]), json!(["accountWarning", id])));
        }
        view(&format!("account:{id}"), json!({"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":6,"marginHorizontal":8,"marginTop":16,"marginBottom":4}), kids)
    };
    for account in &main.folders {
        let id = str_of(&account["id"]);
        let reported = str_of(&base.facts.status["accounts"][id]["status"]);
        let state = if reported.is_empty() { str_of(&account["status"]) } else { reported }.to_owned();
        let bad = warns(base, id, &state);
        rows.push(header(account, &state, bad, false));
        for folder in list_of(&account["folders"]).iter().filter(|f| f["noselect"] != true && f["noselect"] != 1) {
            let name = str_of(&folder["name"]);
            let special = str_of(&folder["special"]);
            let count = match special {
                "drafts" => folder["total"].as_u64(),
                "sent" | "trash" => Some(0),
                _ => folder["unread"].as_u64(),
            };
            let delimiter = str_of(&folder["delim"]);
            let nested = special.is_empty() && !delimiter.is_empty() && name.contains(delimiter);
            let selected = matches!(&main.source, Source::Folder { account: a, folder: f } if a == id && f == name);
            let label = base.folder_name(folder);
            rows.push(side_row(
                base,
                SideRow { key: format!("folder:{id}:{name}"), icon: folder_icon(folder, &color(selected)), label: &label, count: count.unwrap_or(0), selected, indent: f64::from(u8::from(nested)) },
                json!(["folder", [id, name]]),
            ));
        }
    }
    for account in base.accounts("dav") {
        let id = str_of(&account["id"]);
        let state = str_of(&base.facts.status["accounts"][id]["status"]).to_owned();
        let bad = warns(base, id, &state);
        rows.push(header(&account, &state, bad, true));
    }
    let label = base.t("categories");
    rows.push(caption(base, "categories", &label, 16.));
    for (i, shade) in CATEGORY_COLORS.iter().enumerate() {
        let label = i as u64 + 1;
        let selected = matches!(main.source, Source::Unified { label: l, .. } if u64::from(l) == label);
        let name = base.category_name(label);
        rows.push(side_row(base, SideRow { key: format!("category:{label}"), icon: dot("dot", 10., shade), label: &name, count: 0, selected, indent: 0. }, json!(["category", label])));
    }
    let add = base.t("addAccount");
    let mut gear = click(
        view("gear", json!({"width":28,"height":26,"borderRadius":6,"alignItems":"center","justifyContent":"center"}), vec![text("⚙", json!({"fontSize":14,"color":base.token("text", "#ececf0")}))]),
        json!(["accounts"]),
    );
    gear["tooltip"] = json!({"key":"mail.menu.accounts"});
    gear["hoverStyle"] = json!({"backgroundColor":base.token("hover", "rgba(127,127,127,0.14)")});
    view("sidebar", json!({"width":width,"flexShrink":0,"backgroundColor":base.token("sidebarBg", "#2a2a2f")}), vec![
        scroll("accounts-list", json!({"flex":1,"minHeight":0}), vec![view("accounts-rows", json!({"padding":10,"gap":1}), rows)]),
        rule(base, "sidebar-rule"),
        view("sidebar-footer", json!({"height":40,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":4,"paddingHorizontal":8}), vec![
            button("add-account", &format!("⊕ {add}"), json!(["addAccount"]), LINK),
            spacer("footer-space"),
            gear,
        ]),
        edge(base, "sidebar-edge"),
    ])
}

// ---------------------------------------------------------------- toolbar and list

fn tool(base: &Base, name: &str, drawn: Value, command: &str, enabled: bool) -> Value {
    let mut node = view(&format!("tool:{command}"), json!({"width":34,"height":28,"borderRadius":6,"alignItems":"center","justifyContent":"center",
        "opacity":if enabled { 1. } else { 0.35 }}), vec![drawn]);
    node["tooltip"] = json!({"key":format!("mail.{name}")});
    if enabled {
        node["hoverStyle"] = json!({"backgroundColor":base.token("hover", "rgba(127,127,127,0.14)")});
        node = click(node, json!([command]));
    }
    node
}
fn toolbar(base: &Base, main: &Main) -> Value {
    let color = base.token("text", "#ececf0");
    let symbol = |s: &str| mark(s, 15., &color);
    let (reading, chosen) = (main.message.is_some(), main.selected.is_some());
    let divider = |key: &str| view(key, json!({"width":1,"height":18,"marginHorizontal":4,"backgroundColor":base.token("separator", "#3a3a40")}), vec![]);
    let dots = view("dots", json!({"width":16,"height":16,"alignItems":"center","justifyContent":"center","gap":1}), vec![
        dot("one", 6., CATEGORY_COLORS[0]),
        row("two", 1., vec![dot("left", 6., CATEGORY_COLORS[3]), dot("right", 6., CATEGORY_COLORS[4])]),
    ]);
    view("toolbar", json!({"height":TOOLBAR,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":2,"paddingHorizontal":10}), vec![
        tool(base, "new", symbol("✎"), "compose", true),
        divider("first"),
        tool(base, "reply", symbol("↩"), "reply", reading),
        tool(base, "replyAll", symbol("⇇"), "replyAll", reading),
        tool(base, "forward", symbol("↪"), "forward", reading),
        divider("second"),
        tool(base, "archive", glyph("glyph", "archive", 16., &color), "archive", chosen),
        tool(base, "delete", glyph("glyph", "trash", 16., &color), "delete", chosen),
        tool(base, "flag", symbol("⚑"), "flag", chosen),
        tool(base, "category", dots, "labels", chosen),
    ])
}

fn list_title(base: &mut Base, main: &Main) -> String {
    match &main.source {
        Source::Search(query) => base.tp("search.results", json!({"q":query})),
        Source::Unified { filter, label } => match (*label, *filter) {
            (0, "unread") => base.t("unread"),
            (0, "flagged") => base.t("flagged"),
            (0, _) => base.t("unifiedInbox"),
            (label, _) => base.category_name(u64::from(label)),
        },
        Source::Folder { account, folder } => {
            let found = main
                .folders
                .iter()
                .find(|a| a["id"] == account.as_str())
                .and_then(|a| list_of(&a["folders"]).iter().find(|f| f["name"] == folder.as_str()));
            base.folder_name(&found.cloned().unwrap_or_else(|| json!({"display":folder})))
        }
    }
}

fn list_row(base: &mut Base, item: &Value, index: usize, selected: bool, now: i64) -> Value {
    let unread = model::is_unread(item);
    let accent = base.token("accent", "#0a84ff");
    let foreground = if selected { "#ffffff".to_owned() } else { base.token("text", "#ececf0") };
    let dim = if selected { "rgba(255,255,255,0.8)".to_owned() } else { base.token("textDim", "#8e8e96") };
    let sender = match (str_of(&item["from"]), str_of(&item["email"])) {
        ("", "") => base.t("unknownSender"),
        ("", email) => email.to_owned(),
        (name, _) => name.to_owned(),
    };
    let subject = match str_of(&item["subject"]) {
        "" => base.t("noSubject"),
        subject => subject.to_owned(),
    };
    let mut first = vec![clipped(text(sender, json!({"flex":1,"fontSize":13,"fontWeight":if unread {600} else {500},"color":foreground})))];
    if let Some(n) = item["n"].as_u64().filter(|n| *n > 1) {
        let pill = if selected { "rgba(255,255,255,0.22)".to_owned() } else { base.token("control", "rgba(127,127,127,0.25)") };
        first.push(view("count", json!({"minWidth":18,"height":15,"paddingHorizontal":5,"borderRadius":8,"alignItems":"center","justifyContent":"center","backgroundColor":pill}),
            vec![text(n.to_string(), json!({"fontSize":10,"fontWeight":600,"color":foreground}))]));
    }
    if item["att"] == true || item["att"] == 1 {
        first.push(text("⧉", json!({"fontSize":12,"color":dim})));
    }
    if model::is_flagged(item) {
        first.push(text("⚑", json!({"fontSize":11,"color":if selected { "#ffffff" } else { "#ff9f0a" }})));
    }
    let date = base.list_date(item["date"].as_i64().unwrap_or(0), now);
    first.push(text(date, json!({"fontSize":11,"color":dim,"whiteSpace":"nowrap"})));
    let mut second: Vec<Value> = list_of(&item["labels"])
        .iter()
        .filter_map(Value::as_u64)
        .filter(|l| (1..=6).contains(l))
        .map(|l| {
            let mut node = dot(&format!("label-{l}"), 8., CATEGORY_COLORS[l as usize - 1]);
            if selected {
                node["style"]["borderWidth"] = json!(1);
                node["style"]["borderColor"] = json!("#ffffff");
            }
            node
        })
        .collect();
    if item["answered"] == true || item["answered"] == 1 {
        second.push(text("↩", json!({"fontSize":11,"color":dim})));
    }
    second.push(clipped(text(subject, json!({"flex":1,"fontSize":12,"fontWeight":if unread {600} else {400},"color":foreground}))));
    let unread_dot = if unread { vec![dot("unread", 8., if selected { "#ffffff" } else { &accent })] } else { vec![] };
    let body = view("card", json!({"flex":1,"flexDirection":"row","paddingHorizontal":8,"paddingVertical":8,"gap":6,"borderRadius":7,
        "backgroundColor":if selected { accent.clone() } else { "transparent".to_owned() }}), vec![
        view("mark", json!({"width":10,"alignItems":"center","paddingTop":5}), unread_dot),
        view("lines", json!({"flex":1,"minWidth":0,"gap":1,"overflow":"hidden"}), vec![
            view("first", json!({"flexDirection":"row","alignItems":"center","gap":6,"height":18}), first),
            view("second", json!({"flexDirection":"row","alignItems":"center","gap":4,"height":17}), second),
            view("preview", json!({"height":32,"overflow":"hidden"}), vec![text(str_of(&item["preview"]), json!({"fontSize":12,"color":dim,"lineHeight":16}))]),
        ]),
    ]);
    let node = view(&format!("item:{}", model::item_key(item)), json!({"flex":1,"paddingHorizontal":6,"paddingVertical":2}), vec![body]);
    on(click(node, json!(["row", index])), "onContextMenu", json!(["rowMenu", index]))
}

fn rows(base: &mut Base, main: &Main, now: i64) -> Value {
    let height = list_height(base);
    let total = if main.list.source.as_ref() == Some(&main.source) { main.list.total } else { 0 };
    let offset = main.offset.min(main.max_offset(base));
    let first = ((offset / ROW).floor() as usize).saturating_sub(1);
    let end = (((offset + height) / ROW).ceil() as usize + 1).min(total);
    let selected = main.selected_key();
    let separator = base.token("separator", "#3a3a40");
    let mut kids = Vec::new();
    for i in first..end {
        let item = main.list.items.get(&i);
        let key = item.map_or_else(|| format!("placeholder:{i}"), |item| format!("slot:{}", model::item_key(item)));
        let mut content = Vec::new();
        if let Some(item) = item {
            let here = selected.as_deref() == Some(&model::item_key(item));
            content.push(list_row(base, item, i, here, now));
        }
        content.push(view("line", json!({"position":"absolute","bottom":0,"height":1,"start":30,"end":14,"backgroundColor":separator}), vec![]));
        kids.push(view(&key, json!({"position":"absolute","left":0,"right":0,"top":i as f64 * ROW - offset,"height":ROW}), content));
    }
    if total == 0 && !main.list.loading {
        let nothing = base.t(if matches!(main.source, Source::Search(_)) { "search.none" } else { "empty" });
        kids.push(view("nothing", json!({"flex":1,"alignItems":"center","justifyContent":"center","padding":20}), vec![
            text(nothing, json!({"fontSize":13,"color":base.token("textDim", "#8e8e96"),"textAlign":"center"})),
        ]));
    }
    let content = total as f64 * ROW;
    if content > height {
        let size = (height * height / content).max(28.);
        let top = (height - size) * (offset / (content - height));
        let shade = if base.dark { "rgba(255,255,255,0.3)" } else { "rgba(0,0,0,0.28)" };
        kids.push(on(view("scrollbar", json!({"position":"absolute","end":2,"width":6,"borderRadius":3,"height":size,"top":top,"backgroundColor":shade}), vec![]),
            "onMouseDown", json!(["barDown"])));
    }
    on(view("messages", json!({"flex":1,"minHeight":0,"position":"relative","overflow":"hidden"}), kids), "onWheel", json!(["listWheel"]))
}

fn list_column(base: &mut Base, main: &Main, width: f64, now: i64) -> Value {
    let title = list_title(base, main);
    let placeholder = base.t("search.placeholder");
    let mut search = field(&format!("search:{}", main.search_field), &main.query, &placeholder, json!(["query"]));
    search["cancel"] = json!(["queryClear"]);
    search["style"] = json!({"height":28});
    let notice = |base: &Base, key: &str, label: String| text(label, json!({"fontSize":13,"color":base.token("textDim", "#8e8e96"),"key":key}));
    let body = if !base.facts.ready {
        view("waiting", json!({"flex":1}), vec![])
    } else if !base.facts.available {
        let label = base.t("unavailable");
        view("unavailable", json!({"flex":1,"padding":16}), vec![notice(base, "unavailable-text", label)])
    } else if base.accounts("mail").is_empty() {
        let (label, add) = (base.t("noAccounts"), base.t("addAccount"));
        view("no-accounts", json!({"flex":1,"padding":18,"gap":10,"alignItems":"flex-start"}), vec![
            notice(base, "no-accounts-text", label),
            button("first-account", &add, json!(["addAccount"]), PRIMARY),
        ])
    } else {
        rows(base, main, now)
    };
    view("list", json!({"width":width,"flexShrink":0}), vec![
        view("list-title", json!({"height":LIST_TITLE,"flexShrink":0,"justifyContent":"center","paddingHorizontal":14}), vec![
            clipped(text(title, json!({"fontSize":15,"fontWeight":600,"color":base.token("text", "#ececf0")}))),
        ]),
        view("search-row", json!({"height":SEARCH,"flexShrink":0,"paddingHorizontal":12}), vec![search]),
        rule(base, "search-rule"),
        body,
        edge(base, "list-edge"),
    ])
}

// ---------------------------------------------------------------- the reading pane

/// A drawn message's missing images: the message, whether remote ones may load, and which.
type Missing = Vec<(Value, bool, Vec<Wanted>)>;

fn banner(key: &str, color: &str, children: Vec<Value>) -> Value {
    view(key, json!({"flexDirection":"row","alignItems":"center","gap":8,"padding":8,"paddingHorizontal":12,"borderRadius":8,"backgroundColor":color}), children)
}

fn invite_card(base: &mut Base, main: &Main, key: &str) -> Option<Value> {
    let invite = main.invites.get(key)?;
    let data = invite.data.as_ref()?;
    let event = &data["event"];
    let (foreground, dim) = (base.token("text", "#ececf0"), base.token("textDim", "#8e8e96"));
    let start = event["start"].as_i64().unwrap_or(0);
    let when = if event["allDay"] == true || event["allDay"] == 1 {
        base.date(start, "long")
    } else {
        let end = base.time(event["end"].as_i64().unwrap_or(start));
        format!("{} – {end}", base.full_date(start))
    };
    let day = crate::clock::local(start);
    let month = base.month_short(day.tm_mon).to_uppercase();
    let page = view("date", json!({"width":44,"height":48,"borderRadius":8,"backgroundColor":"#ffffff","overflow":"hidden","alignItems":"center","flexShrink":0}), vec![
        view("month", json!({"height":14,"alignSelf":"stretch","backgroundColor":"#ff453a","alignItems":"center","justifyContent":"center"}),
            vec![text(month, json!({"fontSize":9,"color":"#ffffff","fontWeight":700}))]),
        text(day.tm_mday.to_string(), json!({"fontSize":20,"fontWeight":700,"color":"#222222"})),
    ]);
    let title = match str_of(&event["title"]) {
        "" => base.t("invite.untitled"),
        title => title.to_owned(),
    };
    let mut facts = vec![text(title, json!({"fontSize":14,"fontWeight":600,"color":foreground})), text(when, json!({"fontSize":12,"color":dim}))];
    if !str_of(&event["loc"]).is_empty() {
        facts.push(text(str_of(&event["loc"]), json!({"fontSize":12,"color":dim})));
    }
    if event["org"].is_object() {
        let organizer = match str_of(&event["org"]["name"]) {
            "" => str_of(&event["org"]["email"]),
            name => name,
        };
        facts.push(text(base.tp("invite.organizer", json!({"name":organizer})), json!({"fontSize":12,"color":dim})));
    }
    let mut kids = vec![row("invite-head", 10., vec![page, view("facts", json!({"flex":1,"gap":2}), facts)])];
    match str_of(&data["method"]) {
        "REPLY" => kids.push(text(base.t("invite.reply"), json!({"fontSize":12,"color":dim}))),
        "CANCEL" => kids.push(text(base.t("invite.cancelled"), json!({"fontSize":12,"color":"#ff453a"}))),
        _ => {
            let status = if invite.done.is_empty() { str_of(&data["myStatus"]) } else { &invite.done };
            let mut answers = Vec::new();
            for (partstat, chosen, plain) in [("ACCEPTED", "invite.accepted", "invite.accept"), ("TENTATIVE", "invite.maybe", "invite.maybe"), ("DECLINED", "invite.declined", "invite.decline")] {
                let label = if status == partstat { format!("✓ {}", base.t(chosen)) } else { base.t(plain) };
                let looks = Looks { primary: partstat == "ACCEPTED" && status != partstat, destructive: partstat == "DECLINED", disabled: invite.busy, ..Looks::default() };
                answers.push(button(&format!("invite-{partstat}"), &label, json!(["invite", [key, partstat]]), looks));
            }
            if data["inCalendar"] == true || data["inCalendar"] == 1 || matches!(invite.done.as_str(), "ACCEPTED" | "TENTATIVE") {
                answers.push(button("invite-show", &base.t("invite.show"), json!(["inviteShow", key]), LINK));
            }
            kids.push(row("invite-answers", 8., answers));
        }
    }
    if !invite.error.is_empty() {
        kids.push(text(invite.error.as_str(), json!({"fontSize":12,"color":"#ff453a"})));
    }
    Some(view("invite", json!({"padding":12,"borderRadius":10,"gap":6,"backgroundColor":base.token("control", "rgba(127,127,127,0.18)")}), kids))
}

/// A sheet of paper with the extension on a colored band.
fn document_glyph(name: &str, mime: &str) -> Value {
    let (extension, color) = model::doc_glyph(name, mime);
    view("doc", json!({"width":24,"height":30,"borderRadius":3,"flexShrink":0,"backgroundColor":"#ffffff","borderWidth":1,"borderColor":"rgba(0,0,0,0.15)",
        "justifyContent":"flex-end","overflow":"hidden"}), vec![
        view("band", json!({"height":11,"backgroundColor":color,"alignItems":"center","justifyContent":"center"}),
            vec![text(extension, json!({"fontSize":7,"fontWeight":700,"color":"#ffffff"}))]),
    ])
}

fn attachments(base: &mut Base, main: &Main, message: &Value, key: &str) -> Option<Value> {
    let html = message["html"].is_object();
    let files: Vec<&Value> = list_of(&message["atts"])
        .iter()
        // pictures the HTML shows in place are not listed again
        .filter(|a| !((a["inline"] == true || a["inline"] == 1) && !str_of(&a["cid"]).is_empty() && html))
        .collect();
    if files.is_empty() {
        return None;
    }
    let (open, save, working) = (base.t("att.open"), base.t("att.save"), base.t("working"));
    let cards: Vec<Value> = files
        .into_iter()
        .map(|file| {
            let index = file["idx"].as_i64().unwrap_or(-1);
            let name = model::safe_name(str_of(&file["name"]));
            let busy = main.attaching.as_ref().is_some_and(|(m, i)| m == key && *i == index);
            let size = if busy { working.clone() } else { model::format_size(file["size"].as_f64().unwrap_or(0.)) };
            let facts = click(view("facts", json!({"flexShrink":1,"minWidth":40,"overflow":"hidden"}), vec![
                clipped(text(name.as_str(), json!({"fontSize":12,"fontWeight":500,"color":base.token("text", "#ececf0")}))),
                text(size, json!({"fontSize":11,"color":base.token("textDim", "#8e8e96")})),
            ]), json!(["attachmentOpen", [key, index]]));
            view(&format!("attachment:{index}"), json!({"flexDirection":"row","alignItems":"center","gap":8,"paddingVertical":6,"paddingHorizontal":8,"borderRadius":8,
                "maxWidth":360,"flexShrink":1,"minWidth":0,"backgroundColor":base.token("control", "rgba(127,127,127,0.18)"),
                "borderWidth":1,"borderColor":base.token("separator", "#3a3a40")}), vec![
                document_glyph(&name, str_of(&file["type"])),
                facts,
                button("open", &open, json!(["attachmentOpen", [key, index]]), LINK),
                button("save", &save, json!(["attachmentSave", [key, index]]), LINK),
            ])
        })
        .collect();
    Some(view("attachments", json!({"flexDirection":"row","flexWrap":"wrap","gap":8}), cards))
}

fn addresses(list: &Value) -> String {
    list_of(list).iter().map(model::address_short).collect::<Vec<_>>().join(", ")
}

fn message_view(base: &mut Base, main: &Main, message: &Value, width: f64, missing: &mut Missing) -> Value {
    let key = model::item_key(message);
    let (foreground, dim) = (base.token("text", "#ececf0"), base.token("textDim", "#8e8e96"));
    let control = base.token("control", "rgba(127,127,127,0.18)");
    let from = &message["from"][0];
    let sender = model::address_short(from).to_owned();
    let initial = sender.chars().next().unwrap_or('?').to_uppercase().to_string();
    let date = base.full_date(message["date"].as_i64().unwrap_or(0));
    let wide = width > 420.;
    let mut top = vec![clipped(text(sender, json!({"flex":1,"fontSize":14,"fontWeight":600,"color":foreground})))];
    if wide {
        top.push(text(date.as_str(), json!({"fontSize":11,"color":dim,"whiteSpace":"nowrap"})));
    }
    let mut facts = vec![row("sender", 10., top)];
    if !str_of(&from["name"]).is_empty() {
        facts.push(clipped(text(str_of(&from["email"]), json!({"fontSize":12,"color":dim}))));
    }
    facts.push(text(format!("{}: {}", base.t("to"), addresses(&message["to"])), json!({"fontSize":12,"color":dim})));
    if !list_of(&message["cc"]).is_empty() {
        facts.push(text(format!("{}: {}", base.t("cc"), addresses(&message["cc"])), json!({"fontSize":12,"color":dim})));
    }
    if !wide {
        facts.push(text(date, json!({"fontSize":11,"color":dim})));
    }
    let mut kids = vec![
        view("header", json!({"flexDirection":"row","gap":12,"alignItems":"flex-start"}), vec![
            view("avatar", json!({"width":40,"height":40,"borderRadius":20,"flexShrink":0,"background":"linear-gradient(180deg, #a5a5ad, #85858d)",
                "alignItems":"center","justifyContent":"center"}), vec![text(initial, json!({"fontSize":16,"fontWeight":600,"color":"#ffffff"}))]),
            view("who", json!({"flex":1,"minWidth":0,"gap":2,"overflow":"hidden"}), facts),
        ]),
        rule(base, "header-rule"),
    ];
    let remote = main.shows_remote(base, message);
    let stats = &message["html"]["stats"];
    let blocked = stats["remote"].as_u64().unwrap_or(0);
    if blocked > 0 && !remote {
        let mut explanation = base.tp("images.blocked", json!({"count":blocked}));
        if let Some(trackers) = stats["trackers"].as_u64().filter(|n| *n > 0) {
            explanation = format!("{explanation} {}", base.tp("images.trackers", json!({"count":trackers})));
        }
        let (load, always) = (base.t("images.load"), base.t("images.always"));
        kids.push(banner("images-banner", &control, vec![
            text(explanation, json!({"flex":1,"fontSize":12,"color":foreground})),
            button("load-images", &load, json!(["loadImages", key]), Looks { small: true, ..Looks::default() }),
            button("trust-images", &always, json!(["trustImages", key]), LINK),
        ]));
    }
    let deceptive = list_of(&message["html"]["links"]).iter().filter(|l| !l["w"].is_null() && l["w"] != false && l["w"] != 0).count();
    if deceptive > 0 {
        let warning = base.tp("links.warning", json!({"count":deceptive}));
        kids.push(banner("links-banner", "#ff9f0a33", vec![text(format!("⚠ {warning}"), json!({"flex":1,"fontSize":12,"color":foreground}))]));
    }
    kids.extend(invite_card(base, main, &key));
    kids.extend(attachments(base, main, message, &key));
    let accent = base.token("accent", "#0a84ff");
    let colors = html::Colors { text: &foreground, dim: &dim, accent: &accent, dark: base.dark };
    if message["html"].is_object() {
        let none = HashMap::new();
        let pictures = main.pictures.get(&key).unwrap_or(&none);
        let (body, wanted) = html::document(&key, &message["html"], width, remote, pictures, &colors);
        kids.push(body);
        if !wanted.is_empty() {
            missing.push((message.clone(), remote, wanted));
        }
    } else {
        kids.push(html::plain(&format!("plain:{key}"), str_of(&message["text"]), width, &colors));
    }
    if !message["unsubscribe"].is_null() && message["unsubscribe"] != false && message["unsubscribe"] != "" {
        kids.push(text(base.t("unsubscribeHint"), json!({"fontSize":11,"color":dim})));
    }
    view(&format!("message:{key}"), json!({"gap":12}), kids)
}

/// Another message of the conversation: one line, or the whole message when expanded.
fn thread_item(base: &mut Base, main: &Main, item: &Value, width: f64, now: i64, missing: &mut Missing) -> Value {
    let key = model::item_key(item);
    if main.expanded.contains(&key) {
        if let Some(message) = main.siblings.get(&key) {
            let collapse = base.t("thread.collapse");
            return view(&format!("thread:{key}"), json!({"gap":6,"alignItems":"stretch"}), vec![
                row("collapse-row", 0., vec![button("collapse", &collapse, json!(["collapse", key]), LINK)]),
                message_view(base, main, message, width, missing),
            ]);
        }
    }
    let dim = base.token("textDim", "#8e8e96");
    let sender = match str_of(&item["from"]) {
        "" => str_of(&item["email"]),
        name => name,
    };
    let date = base.list_date(item["date"].as_i64().unwrap_or(0), now);
    let node = view(&format!("thread:{key}"), json!({"paddingVertical":9,"paddingHorizontal":12,"borderRadius":8,"flexDirection":"row","alignItems":"center","gap":10,
        "backgroundColor":base.token("control", "rgba(127,127,127,0.18)")}), vec![
        text(sender, json!({"fontSize":13,"fontWeight":600,"color":base.token("text", "#ececf0"),"whiteSpace":"nowrap"})),
        clipped(text(str_of(&item["preview"]), json!({"flex":1,"fontSize":12,"color":dim}))),
        text(date, json!({"fontSize":11,"color":dim})),
    ]);
    click(node, json!(["expand", key]))
}

fn reading(base: &mut Base, main: &Main, width: f64, now: i64, missing: &mut Missing) -> Value {
    let (foreground, dim) = (base.token("text", "#ececf0"), base.token("textDim", "#8e8e96"));
    let Some(message) = &main.message else {
        let label = if main.load_error.is_empty() { base.t("noSelection") } else { main.load_error.clone() };
        return view("reading-empty", json!({"flex":1,"minWidth":0,"alignItems":"center","justifyContent":"center","gap":8,"padding":24}), vec![
            text("✉", json!({"fontSize":40,"color":dim,"opacity":0.6})),
            text(label, json!({"fontSize":13,"color":dim,"textAlign":"center"})),
        ]);
    };
    let inner = width - 40.;
    let key = model::item_key(message);
    let subject = match str_of(&message["subject"]) {
        "" => base.t("noSubject"),
        subject => subject.to_owned(),
    };
    let mut kids = vec![text(subject, json!({"fontSize":20,"fontWeight":600,"color":foreground,"lineHeight":26}))];
    if !main.load_error.is_empty() {
        kids.push(banner("failure", "#ff453a22", vec![text(main.load_error.as_str(), json!({"flex":1,"fontSize":12,"color":foreground}))]));
    }
    let labels: Vec<u64> = main.selected.as_ref().map(|s| list_of(&s["labels"]).iter().filter_map(Value::as_u64).filter(|l| (1..=6).contains(l)).collect()).unwrap_or_default();
    if !labels.is_empty() {
        let chips = labels
            .into_iter()
            .map(|l| {
                view(&format!("chip-{l}"), json!({"paddingHorizontal":8,"height":18,"borderRadius":9,"justifyContent":"center",
                    "backgroundColor":format!("{}44", CATEGORY_COLORS[l as usize - 1])}), vec![text(base.category_name(l), json!({"fontSize":11,"color":foreground}))])
            })
            .collect();
        kids.push(row("chips", 6., chips));
    }
    if main.thread.len() > 1 {
        for item in main.thread.iter().filter(|x| !(x["folder"] == message["folder"] && x["uid"] == message["uid"])) {
            kids.push(thread_item(base, main, item, inner, now, missing));
        }
    }
    kids.push(message_view(base, main, message, inner, missing));
    // a new key for each message: the pane starts at its top
    view("reading", json!({"flex":1,"minWidth":0}), vec![scroll(&format!("reading:{key}"), json!({"flex":1,"minHeight":0}), vec![
        view("reading-content", json!({"paddingHorizontal":20,"paddingTop":16,"paddingBottom":24,"gap":12}), kids),
    ])])
}

// ---------------------------------------------------------------- sheets

fn modal(base: &Base, key: &str, width: f64, children: Vec<Value>) -> Value {
    let panel = click(
        view(key, json!({"width":width,"maxWidth":"96%","maxHeight":"92%","padding":18,"gap":10,"borderRadius":14,"overflow":"hidden",
            "backgroundColor":base.token("windowBg", "#242428"),"borderWidth":1,"borderColor":base.token("separator", "#3a3a40"),
            "boxShadow":"0 20px 50px rgba(0,0,0,0.45)"}), children),
        json!(["noop"]),
    );
    click(view("sheet", json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"alignItems":"center","justifyContent":"center",
        "backgroundColor":"rgba(0,0,0,0.32)"}), vec![panel]), json!(["noop"]))
}
fn heading(base: &Base, label: String, color: Option<&str>) -> Value {
    text(label, json!({"fontSize":16,"fontWeight":700,"color":color.map_or_else(|| base.token("text", "#ececf0"), str::to_owned)}))
}
fn buttons(children: Vec<Value>) -> Value {
    view("sheet-buttons", json!({"flexDirection":"row","justifyContent":"flex-end","gap":8,"marginTop":4,"flexShrink":0}), children)
}
fn form_row(base: &Base, key: &str, label: String, control: Value) -> Value {
    view(&format!("row-{key}"), json!({"flexDirection":"row","alignItems":"center","gap":10,"flexShrink":0}), vec![
        text(label, json!({"width":110,"fontSize":12,"textAlign":"end","color":base.token("textDim", "#8e8e96")})),
        control,
    ])
}
fn switch(base: &Base, key: &str, on_now: bool, action: Value) -> Value {
    let track = if on_now { base.token("accent", "#0a84ff") } else { "rgba(127,127,127,0.4)".to_owned() };
    click(view(key, json!({"width":34,"height":20,"borderRadius":10,"flexShrink":0,"backgroundColor":track}), vec![
        view("knob", json!({"position":"absolute","top":2,"left":if on_now { 16 } else { 2 },"width":16,"height":16,"borderRadius":8,"backgroundColor":"#ffffff"}), vec![]),
    ]), action)
}
pub(crate) fn segmented(base: &Base, key: &str, options: &[(Value, &str)], chosen: usize) -> Value {
    let parts = options
        .iter()
        .enumerate()
        .map(|(i, (action, label))| {
            let here = i == chosen;
            click(view(&format!("{key}-{i}"), json!({"height":24,"paddingHorizontal":10,"borderRadius":5,"justifyContent":"center",
                "backgroundColor":if here { base.token("accent", "#0a84ff") } else { "transparent".to_owned() }}),
                vec![text(*label, json!({"fontSize":12,"whiteSpace":"nowrap","color":if here { "#ffffff".to_owned() } else { base.token("text", "#ececf0") }}))]),
                action.clone())
        })
        .collect();
    view(key, json!({"flexDirection":"row","padding":2,"borderRadius":7,"alignSelf":"flex-start","backgroundColor":base.token("control", "rgba(127,127,127,0.2)")}), parts)
}

fn server_fields(base: &mut Base, setup: &Setup, which: &str, title: &str) -> Vec<Value> {
    let server = &setup.servers[which];
    let host = field(&format!("{which}-host"), str_of(&server["host"]), &format!("{which}.example.com"), json!(["setupText", format!("{which}Host")]));
    let port_text = server["port"].as_u64().filter(|p| *p > 0).map_or_else(String::new, |p| p.to_string());
    let port = field(&format!("{which}-port"), &port_text, "", json!(["setupText", format!("{which}Port")]));
    let security = segmented(base, &format!("{which}-security"), &[
        (json!(["setupSecurity", [which, "ssl"]]), "SSL/TLS"),
        (json!(["setupSecurity", [which, "starttls"]]), "STARTTLS"),
    ], usize::from(server["security"] == "starttls"));
    let title = base.t(title);
    let (server_label, port_label, security_label) = (base.t("setup.server"), base.t("setup.port"), base.t("setup.security"));
    vec![
        text(title, json!({"fontSize":12,"fontWeight":600,"color":base.token("text", "#ececf0")})),
        form_row(base, &format!("{which}-server"), server_label, host),
        form_row(base, &format!("{which}-port-row"), port_label, port),
        form_row(base, &format!("{which}-security-row"), security_label, security),
    ]
}

fn setup_sheet(base: &mut Base, setup: &Setup) -> Value {
    let (foreground, dim) = (base.token("text", "#ececf0"), base.token("textDim", "#8e8e96"));
    let title = base.t(if setup.dav { "setup.titleDav" } else { "setup.title" });
    let mut kids = vec![heading(base, title, None)];
    let result = setup.created.as_deref().map(|id| sheets::connection(base, id));
    match setup.step {
        Step::Who => {
            let (mail, dav) = (base.t("setup.kindMail"), base.t("setup.kindDav"));
            kids.push(segmented(base, "kind", &[(json!(["setupKind", "mail"]), &mail), (json!(["setupKind", "dav"]), &dav)], usize::from(setup.dav)));
            let placeholder = base.t(if setup.dav { "setup.davNamePh" } else { "setup.namePh" });
            let mut name = field("setup-name", &setup.name, &placeholder, json!(["setupText", "name"]));
            name["autoFocus"] = json!(true);
            let label = base.t("setup.name");
            kids.push(form_row(base, "name", label, name));
            if setup.dav {
                let url = field("setup-url", &setup.url, "https://cloud.example.com/remote.php/dav", json!(["setupText", "url"]));
                let label = base.t("setup.url");
                kids.push(form_row(base, "url", label, url));
                let user = field("setup-dav-user", &setup.dav_user, "", json!(["setupText", "davUser"]));
                let label = base.t("setup.user");
                kids.push(form_row(base, "dav-user", label, user));
            } else {
                let placeholder = base.t("setup.emailPh");
                let mut email = field("setup-email", &setup.email, &placeholder, json!(["setupText", "email"]));
                email["submit"] = json!(["setupNext"]);
                let label = base.t("setup.email");
                kids.push(form_row(base, "email", label, email));
            }
            let mut password = field("setup-password", &setup.password, "", json!(["setupText", "password"]));
            password["secure"] = json!(true);
            password["submit"] = json!(["setupNext"]);
            let label = base.t("setup.password");
            kids.push(form_row(base, "password", label, password));
            kids.push(text(base.t("setup.keyringNote"), json!({"fontSize":11,"color":dim})));
        }
        Step::Servers => {
            let provider = str_of(&setup.servers["name"]).to_owned();
            let named = json!({"name":provider});
            let mut inside = Vec::new();
            if setup.servers["known"] == true {
                let mut card = vec![
                    text(base.tp("setup.found", named.clone()), json!({"fontSize":13,"fontWeight":600,"color":foreground})),
                    text(base.tp("setup.appPassword", named.clone()), json!({"fontSize":12,"color":dim})),
                ];
                if !str_of(&setup.servers["appPassword"]).is_empty() {
                    let address = str_of(&setup.servers["appPassword"]);
                    card.push(click(view("app-password", json!({}), vec![text(address, json!({"fontSize":11,"color":base.token("accent", "#0a84ff")}))]), json!(["url", address])));
                }
                inside.push(view("provider", json!({"padding":10,"borderRadius":8,"gap":4,"backgroundColor":base.token("control", "rgba(127,127,127,0.18)")}), card));
            }
            if !str_of(&setup.servers["oauth"]).is_empty() {
                // OAuth needs a client ID registered with the provider; until then, app passwords
                let (label, why) = (base.tp("setup.oauth", named.clone()), base.t("setup.oauthNeedsId"));
                inside.push(row("oauth", 8., vec![
                    button("oauth-button", &label, json!(["noop"]), Looks { disabled: true, ..Looks::default() }),
                    text(why, json!({"flex":1,"fontSize":11,"color":dim})),
                ]));
            }
            if !str_of(&setup.servers["caldav"]).is_empty() || !str_of(&setup.servers["carddav"]).is_empty() {
                let only_oauth = setup.servers["davNote"] == "oauth";
                let label = base.tp(if only_oauth { "setup.davOauth" } else { "setup.alsoDav" }, named);
                inside.push(row("also-dav", 8., vec![
                    switch(base, "dav-switch", setup.with_dav && !only_oauth, if only_oauth { json!(["noop"]) } else { json!(["setupToggle", "dav"]) }),
                    text(label, json!({"flex":1,"fontSize":12,"color":if only_oauth { &dim } else { &foreground }})),
                ]));
            }
            let manual = base.t("setup.manual");
            inside.push(row("manual", 8., vec![
                switch(base, "manual-switch", setup.manual, json!(["setupToggle", "manual"])),
                text(manual, json!({"flex":1,"fontSize":12,"color":foreground})),
            ]));
            if setup.manual {
                let user = field("setup-user", str_of(&setup.servers["user"]), "", json!(["setupText", "user"]));
                let label = base.t("setup.user");
                inside.push(form_row(base, "user", label, user));
                inside.extend(server_fields(base, setup, "imap", "setup.incoming"));
                inside.extend(server_fields(base, setup, "smtp", "setup.outgoing"));
            }
            let wanted: f64 = if setup.manual { 420. } else { 210. };
            let height = wanted.min((base.height * 0.92 - 130.).max(120.));
            kids.push(scroll("servers", json!({"height":height}), vec![view("servers-content", json!({"gap":10,"paddingEnd":8}), inside)]));
        }
        Step::Result => {
            let state = result.unwrap_or("working");
            let color = match state {
                "ok" => "#30d158".to_owned(),
                "working" => foreground.clone(),
                _ => "#ff453a".to_owned(),
            };
            kids.push(text(base.t(&format!("setup.result.{state}")), json!({"fontSize":14,"color":color})));
            if state == "auth" {
                kids.push(text(base.t("setup.authHelp"), json!({"fontSize":12,"color":dim})));
            }
        }
    }
    if !setup.error.is_empty() {
        kids.push(text(setup.error.as_str(), json!({"fontSize":12,"color":"#ff453a"})));
    }
    let mut foot = Vec::new();
    if setup.step == Step::Result && setup.created.is_some() && result != Some("ok") {
        foot.push(button("setup-remove", &base.t("setup.removeAccount"), json!(["setupRemove"]), Looks::default()));
    }
    let close = base.t(if setup.step == Step::Result { "close" } else { "cancel" });
    foot.push(button("setup-close", &close, json!(["sheetClose"]), Looks::default()));
    let forward = Looks { disabled: setup.busy, ..PRIMARY };
    match setup.step {
        Step::Who => foot.push(button("setup-next", &base.t(if setup.dav { "setup.add" } else { "setup.next" }), json!(["setupNext"]), forward)),
        Step::Servers => foot.push(button("setup-add", &base.t("setup.add"), json!(["setupAdd"]), forward)),
        Step::Result => {}
    }
    kids.push(buttons(foot));
    modal(base, "setup", 460., kids)
}

fn accounts_sheet(base: &mut Base, selected: &str, signature: &str) -> Value {
    let (foreground, dim) = (base.token("text", "#ececf0"), base.token("textDim", "#8e8e96"));
    let accent = base.token("accent", "#0a84ff");
    let dav = base.t("setup.kindDav");
    let names: Vec<Value> = base
        .all_accounts()
        .iter()
        .map(|account| {
            let id = str_of(&account["id"]);
            let here = id == selected;
            let name = match str_of(&account["name"]) {
                "" => str_of(&account["email"]),
                name => name,
            };
            let second = if account["kind"] == "dav" { dav.as_str() } else { str_of(&account["email"]) };
            click(view(&format!("account-{id}"), json!({"padding":6,"borderRadius":6,"backgroundColor":if here { accent.as_str() } else { "transparent" }}), vec![
                clipped(text(name, json!({"fontSize":13,"color":if here { "#ffffff" } else { &foreground }}))),
                clipped(text(second, json!({"fontSize":11,"color":if here { "#ffffffcc" } else { &dim }}))),
            ]), json!(["accountSelect", id]))
        })
        .collect();
    let add = base.t("accounts.add");
    let left = view("account-names", json!({"width":200,"flexShrink":0,"gap":4,"padding":6,"borderRadius":8,"backgroundColor":base.token("sidebarBg", "#2a2a2f")}), vec![
        scroll("account-scroll", json!({"flex":1,"minHeight":0}), vec![view("account-rows", json!({"gap":2}), names)]),
        button("accounts-add", &add, json!(["accountAdd"]), Looks::default()),
    ]);
    let right = match base.account(selected).cloned() {
        None => view("account-none", json!({"flex":1}), vec![text(base.t("accounts.none"), json!({"fontSize":13,"color":dim}))]),
        Some(account) => {
            let name = match str_of(&account["name"]) {
                "" => str_of(&account["email"]),
                name => name,
            };
            let report = &base.facts.status["accounts"][selected];
            let state = match str_of(&report["status"]) {
                "" => "idle",
                state => state,
            }
            .to_owned();
            let error = str_of(&report["error"]).to_owned();
            let mut line = format!("{}: {}", base.t("accounts.status"), base.t(&format!("status.{state}")));
            if !error.is_empty() {
                line = format!("{line} · {error}");
            }
            let mut details = vec![text(name, json!({"fontSize":14,"fontWeight":600,"color":foreground})), text(line, json!({"fontSize":12,"color":dim}))];
            if account["kind"] == "mail" {
                let servers = format!("IMAP {}:{} · SMTP {}:{}", str_of(&account["imap"]["host"]), account["imap"]["port"], str_of(&account["smtp"]["host"]), account["smtp"]["port"]);
                details.push(text(servers, json!({"fontSize":12,"color":dim})));
                details.push(text(base.t("accounts.signature"), json!({"fontSize":12,"fontWeight":600,"color":foreground,"marginTop":6})));
                let placeholder = base.t("accounts.signaturePh");
                let mut input = field(&format!("signature:{selected}"), signature, &placeholder, json!(["signatureText"]));
                input["style"] = json!({"height":28});
                details.push(input);
                let save = base.t("accounts.saveSignature");
                details.push(row("signature-row", 0., vec![button("signature-save", &save, json!(["signatureSave"]), Looks::default())]));
            }
            details.push(spacer("details-space"));
            let (password, remove) = (base.t("accounts.password"), base.t("accounts.remove"));
            details.push(row("account-actions", 8., vec![
                button("account-password", &password, json!(["accountPassword"]), Looks::default()),
                button("account-remove", &remove, json!(["accountRemove"]), Looks { destructive: true, ..Looks::default() }),
            ]));
            view("account-details", json!({"flex":1,"minWidth":0,"gap":8}), details)
        }
    };
    let title = base.t("accounts.title");
    let notify = base.t("accounts.notify");
    let close = base.t("close");
    let height = 300_f64.min((base.height * 0.92 - 150.).max(160.));
    modal(base, "accounts", 620., vec![
        heading(base, title, None),
        view("account-columns", json!({"flexDirection":"row","gap":12,"height":height}), vec![left, right]),
        row("account-foot", 8., vec![
            switch(base, "notify-switch", base.facts.config["notify"] != false, json!(["notifyToggle"])),
            text(notify, json!({"flex":1,"fontSize":12,"color":foreground})),
            button("accounts-close", &close, json!(["sheetClose"]), Looks::default()),
        ]),
    ])
}

fn sheet(base: &mut Base, main: &Main) -> Option<Value> {
    let (foreground, dim) = (base.token("text", "#ececf0"), base.token("textDim", "#8e8e96"));
    let control = base.token("control", "rgba(127,127,127,0.18)");
    let (cancel, close) = (base.t("cancel"), base.t("close"));
    Some(match &main.sheet {
        Sheet::None => return None,
        Sheet::Setup(setup) => setup_sheet(base, setup),
        Sheet::Accounts { selected, signature } => accounts_sheet(base, selected, signature),
        Sheet::Certificate(certificate) => {
            let server = format!("{}:{}", str_of(&certificate["host"]), certificate["port"]);
            let (title, body) = (base.t("cert.title"), base.tp("cert.body", json!({"host":server})));
            let mut facts = Vec::new();
            for (label, field_name) in [("cert.reason", "reason"), ("cert.subject", "subject"), ("cert.issuer", "issuer"), ("cert.expires", "notAfter"), ("", "sha256")] {
                let value = str_of(&certificate[field_name]);
                if value.is_empty() && field_name == "notAfter" {
                    continue;
                }
                let label = if label.is_empty() { "SHA-256".to_owned() } else { base.t(label) };
                facts.push(view(&format!("cert-{field_name}"), json!({"flexDirection":"row","gap":8}), vec![
                    text(label, json!({"width":110,"fontSize":11,"textAlign":"end","color":dim})),
                    text(value, json!({"flex":1,"fontSize":11,"fontFamily":"monospace","color":foreground})),
                ]));
            }
            let (advice, trust) = (base.t("cert.advice"), base.t("cert.trust"));
            modal(base, "certificate", 520., vec![
                heading(base, title, Some("#ff453a")),
                text(body, json!({"fontSize":13,"color":foreground})),
                view("cert-facts", json!({"padding":10,"borderRadius":8,"gap":3,"backgroundColor":control}), facts),
                text(advice, json!({"fontSize":12,"color":dim})),
                buttons(vec![
                    button("cert-cancel", &cancel, json!(["sheetClose"]), Looks::default()),
                    button("cert-trust", &trust, json!(["certificateTrust"]), Looks { destructive: true, ..Looks::default() }),
                ]),
            ])
        }
        Sheet::Password { account, value, busy, error } => {
            let user = base.account(account).map_or(account.as_str(), |a| str_of(&a["user"])).to_owned();
            let (title, body) = (base.t("password.title"), base.tp("password.body", json!({"user":user})));
            let save = base.t(if *busy { "working" } else { "password.save" });
            let mut input = field(&format!("password:{account}"), value, "", json!(["passwordText"]));
            input["secure"] = json!(true);
            input["autoFocus"] = json!(true);
            input["submit"] = json!(["passwordSave"]);
            input["cancel"] = json!(["sheetClose"]);
            input["style"] = json!({"height":28});
            let mut kids = vec![heading(base, title, None), text(body, json!({"fontSize":13,"color":dim})), input];
            if !error.is_empty() {
                kids.push(text(error.as_str(), json!({"fontSize":12,"color":"#ff453a"})));
            }
            kids.push(buttons(vec![
                button("password-cancel", &cancel, json!(["sheetClose"]), Looks::default()),
                button("password-save", &save, json!(["passwordSave"]), Looks { disabled: *busy, ..PRIMARY }),
            ]));
            modal(base, "password", 460., kids)
        }
        // there is no web engine here: the address is shown and can be copied; one whose visible
        // text names another site is called out
        Sheet::Link(link) => {
            let url = str_of(&link["h"]);
            let deceptive = !link["w"].is_null() && link["w"] != false && link["w"] != 0;
            let title = base.t(if deceptive { "links.deceptiveTitle" } else { "links.title" });
            let mut kids = vec![heading(base, title, deceptive.then_some("#ff453a"))];
            if deceptive {
                let explanation = base.tp("links.deceptiveBody", json!({"shown":str_of(&link["txt"]).trim(),"host":model::link_host(url)}));
                kids.push(text(explanation, json!({"fontSize":13,"color":foreground})));
            }
            let (none, copy) = (base.t("links.noBrowser"), base.t("links.copy"));
            kids.push(view("link-address", json!({"padding":10,"borderRadius":8,"backgroundColor":control,"overflow":"hidden"}),
                vec![text(url, json!({"fontSize":12,"fontFamily":"monospace","color":foreground}))]));
            kids.push(text(none, json!({"fontSize":12,"color":dim})));
            kids.push(buttons(vec![
                button("link-close", &close, json!(["sheetClose"]), Looks::default()),
                button("link-copy", &copy, json!(["linkCopy"]), Looks { primary: !deceptive, ..Looks::default() }),
            ]));
            modal(base, "link", 500., kids)
        }
        Sheet::Labels => {
            let chosen: Vec<u64> = main.selected.as_ref().map(|s| list_of(&s["labels"]).iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
            let label = base.t("category");
            let mut kids = vec![heading(base, label, None)];
            for (i, color) in CATEGORY_COLORS.iter().enumerate() {
                let label = i as u64 + 1;
                let mut line = vec![dot("dot", 12., color), text(base.category_name(label), json!({"flex":1,"fontSize":13,"color":foreground}))];
                if chosen.contains(&label) {
                    line.push(text("✓", json!({"color":base.token("accent", "#0a84ff")})));
                }
                let mut node = click(view(&format!("label-row-{label}"), json!({"flexDirection":"row","alignItems":"center","gap":8,"padding":6,"borderRadius":6}), line), json!(["label", label]));
                node["hoverStyle"] = json!({"backgroundColor":base.token("hover", "rgba(127,127,127,0.14)")});
                kids.push(node);
            }
            kids.push(buttons(vec![button("labels-close", &close, json!(["sheetClose"]), Looks::default())]));
            modal(base, "labels", 280., kids)
        }
    })
}

// ---------------------------------------------------------------- the window

/// Pane widths: narrower ones in small windows.
pub(crate) fn panes(width: f64) -> (f64, f64) {
    if width < 1000. {
        (200., 290.)
    } else {
        (220., 330.)
    }
}

pub(crate) fn tree(base: &mut Base, main: &mut Main, now: i64) -> Value {
    let (side, list) = panes(base.width);
    let reading_width = (base.width - side - list).max(200.);
    let mut missing = Missing::new();
    let content = view("content", json!({"flex":1,"minWidth":0}), vec![
        toolbar(base, main),
        view("panes", json!({"flex":1,"minHeight":0,"flexDirection":"row"}), vec![
            list_column(base, main, list, now),
            reading(base, main, reading_width, now, &mut missing),
        ]),
    ]);
    let mut root = vec![sidebar(base, main, side), content];
    root.extend(sheet(base, main));
    for (message, remote, wanted) in missing {
        main.want_pictures(base, &message, remote, wanted);
    }
    view("mail", json!({"flex":1,"flexDirection":"row","position":"relative","overflow":"hidden","backgroundColor":base.token("windowBg", "#242428")}), root)
}

pub(crate) fn menus(base: &mut Base, main: &Main) -> Value {
    let (reading, chosen) = (main.message.is_some(), main.selected.is_some());
    let mut item = |label: &str, command: &str, hint: &str, enabled: bool| {
        json!({"label":base.t(label),"action":command,"hint":hint,"disabled":!enabled})
    };
    let file = vec![
        item("new", "compose", "❖N", true),
        item("menu.addAccount", "addAccount", "", true),
        item("menu.accounts", "accounts", "", true),
        json!("-"),
        item("menu.sync", "sync", "", true),
    ];
    let message = vec![
        item("reply", "reply", "❖R", reading),
        item("replyAll", "replyAll", "❖⇧R", reading),
        item("forward", "forward", "❖J", reading),
        json!("-"),
        item("archive", "archive", "❖E", chosen),
        item("delete", "delete", "⌦", chosen),
        item("junk", "junk", "", chosen),
        item("markUnread", "unread", "", chosen),
        item("flag", "flag", "", chosen),
    ];
    json!([{"label":base.t("menu.file"),"items":file},{"label":base.t("menu.message"),"items":message}])
}
