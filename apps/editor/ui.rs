//! The editor's widget trees.
use super::*;
use serde_json::{json, Value};

fn view(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
fn text(label: impl Into<Value>, style: Value) -> Value {
    json!({"kind":"text","label":label.into(),"style":style})
}
fn on(mut node: Value, event: &str, action: Value) -> Value {
    node["events"][event] = action;
    node
}
fn click(node: Value, action: Value) -> Value {
    on(node, "onClick", action)
}
/// One line of text that is cut with an ellipsis.
fn clipped(mut node: Value) -> Value {
    node["style"]["whiteSpace"] = json!("nowrap");
    node["style"]["overflow"] = json!("hidden");
    node["style"]["textOverflow"] = json!("ellipsis");
    node
}
fn rule(key: &str, style: Value, color: &Value) -> Value {
    let mut style = style;
    style["position"] = json!("absolute");
    style["backgroundColor"] = color.clone();
    style["pointerEvents"] = json!("none");
    view(key, style, vec![])
}
fn button(key: &str, label: &str, command: &str, primary: bool) -> Value {
    json!({"kind":"button","key":key,"label":label,"action":[command],"primary":primary})
}

struct Colors {
    window: Value,
    sidebar: Value,
    separator: Value,
    text: Value,
    dim: Value,
    accent: Value,
    editor: &'static str,
    gutter: &'static str,
    gutter_text: &'static str,
    gutter_active: &'static str,
    line: &'static str,
    caret: &'static str,
    selection: &'static str,
    selection_blur: &'static str,
    found: &'static str,
    current: &'static str,
}
fn colors(theme: &Value, dark: bool) -> Colors {
    let token = |name: &str, fallback: &str| {
        let value = &theme[name];
        if value.is_string() {
            value.clone()
        } else {
            json!(fallback)
        }
    };
    let pick = |d: &'static str, l: &'static str| if dark { d } else { l };
    Colors {
        window: token("windowBg", pick("#242428", "#f6f6f8")),
        sidebar: token("sidebarBg", pick("#2a2a2f", "#ececef")),
        separator: token("separator", pick("#3a3a40", "#d6d6da")),
        text: token("text", pick("#ececf0", "#1d1d20")),
        dim: token("textDim", pick("#8e8e96", "#6e6e76")),
        accent: token("accent", "#0a84ff"),
        editor: pick("#1e1e23", "#ffffff"),
        gutter: pick("#1a1a1e", "#f6f6f8"),
        gutter_text: pick("#5c5c66", "#a1a1aa"),
        gutter_active: pick("#d4d4d8", "#3f3f46"),
        line: pick("#26262c", "#f4f4fa"),
        caret: pick("#7aa8ff", "#2563eb"),
        selection: pick("#3b4f7a", "#b4d7ff"),
        selection_blur: pick("#33333b", "#e4e4e7"),
        found: pick("#a1803066", "#fde68a99"),
        current: pick("#d6a12e", "#f59e0b"),
    }
}

/// A violet card with a sheet of highlighted lines.
pub(super) fn icon(size: f64) -> Value {
    let s = if size.is_finite() { size.clamp(1., 512.) } else { 32. };
    let bar = |key: String, x: f64, y: f64, w: f64, h: f64, color: &str| {
        view(&key, json!({"position":"absolute","left":s*x,"top":s*y,"width":s*w,"height":s*h,"borderRadius":s*0.03,"backgroundColor":color}), vec![])
    };
    let mut sheet = bar("sheet".into(), 0.2, 0.16, 0.6, 0.7, "#fbfaff");
    sheet["style"]["borderRadius"] = json!(s * 0.06);
    sheet["style"]["boxShadow"] = json!(format!("0 {}px {}px rgba(0,0,0,0.3)", s * 0.02, s * 0.06));
    let mut kids = vec![sheet];
    for (i, (width, color)) in [(0.36, "#fc5fa3"), (0.26, "#5dd8ff"), (0.4, "#8b5cf6"), (0.2, "#d0bf69"), (0.32, "#67b7a4")].into_iter().enumerate() {
        kids.push(bar(format!("line-{i}"), 0.3, 0.27 + 0.1 * i as f64, width, 0.055, color));
    }
    kids.push(bar("margin".into(), 0.22, 0.255, 0.03, 0.48, "#c4b5fd"));
    view(
        "editor-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #8b5cf6, #3b1f8f)",
            "boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)", s * 0.04, s * 0.12),"overflow":"hidden"}),
        kids,
    )
}

pub(super) fn menus(app: &mut App) -> Value {
    // (label key, command, shortcut); an empty command is a separator
    type Rows = &'static [(&'static str, &'static str, &'static str)];
    const SEPARATOR: (&str, &str, &str) = ("", "", "");
    const FILE: Rows = &[
        ("cmd.new", "new", "❖N"),
        ("cmd.open", "open", "❖O"),
        ("cmd.openFolder", "openFolder", "❖⇧O"),
        SEPARATOR,
        ("cmd.save", "save", "❖S"),
        ("cmd.saveAs", "saveAs", "❖⇧S"),
        SEPARATOR,
        ("cmd.print", "print", "❖P"),
        SEPARATOR,
        ("cmd.close", "close", "❖W"),
    ];
    const EDIT: Rows = &[
        ("cmd.undo", "undo", "❖Z"),
        ("cmd.redo", "redo", "❖⇧Z"),
        SEPARATOR,
        ("cmd.cut", "cut", "❖X"),
        ("cmd.copy", "copy", "❖C"),
        ("cmd.paste", "paste", "Ctrl+V"),
        ("cmd.selectAll", "selectAll", "❖A"),
        SEPARATOR,
        ("cmd.selectNext", "selectNext", "❖D"),
        ("cmd.singleCursor", "singleCursor", "Esc"),
        SEPARATOR,
        ("cmd.find", "find", "❖F"),
        ("cmd.findNext", "findNext", "❖G"),
        SEPARATOR,
        ("cmd.comment", "toggleComment", "❖/"),
        ("cmd.duplicate", "duplicateLines", "❖⇧D"),
        ("cmd.indent", "indent", "❖]"),
        ("cmd.outdent", "outdent", "❖["),
        ("cmd.moveUp", "moveUp", "⌥↑"),
        ("cmd.moveDown", "moveDown", "⌥↓"),
    ];
    const VIEW: Rows = &[("cmd.toggleProject", "toggleProject", "❖\\"), ("cmd.language", "language", "")];
    const GO: Rows = &[
        ("cmd.gotoLine", "gotoLine", "❖L"),
        ("cmd.gotoFile", "gotoFile", "❖T"),
        ("cmd.nextTab", "nextTab", "Ctrl+Tab"),
        ("cmd.prevTab", "prevTab", "Ctrl+⇧Tab"),
    ];
    let menus: Vec<Value> = [("menu.file", FILE), ("menu.edit", EDIT), ("menu.view", VIEW), ("menu.go", GO)]
        .into_iter()
        .map(|(title, rows)| {
            let items: Vec<Value> = rows
                .iter()
                .map(|&(label, command, hint)| {
                    if command.is_empty() {
                        json!("-")
                    } else {
                        json!({"label":app.tr(label),"action":command,"hint":hint})
                    }
                })
                .collect();
            json!({"label":app.tr(title),"items":items})
        })
        .collect();
    json!(menus)
}

fn sidebar(app: &mut App, c: &Colors, now: i64) -> Value {
    let name = if app.root.is_empty() { app.tr("home") } else { base_name(&app.root).to_owned() };
    let rows = app.tree_rows(now);
    let height = (app.height - TABS).max(TREE_ROW);
    app.tree_top = app.tree_top.min((rows.len() as f64 * TREE_ROW - height + TREE_ROW).max(0.));
    let first = (app.tree_top / TREE_ROW).floor() as usize;
    let count = (height / TREE_ROW).ceil() as usize + 1;
    let active = app.tab().path.clone();
    let list: Vec<Value> = rows
        .iter()
        .enumerate()
        .skip(first)
        .take(count)
        .map(|(i, (path, label, dir, depth))| {
            let on_this = active.as_deref() == Some(app.absolute(path).as_str());
            let (color, mark) = if on_this { (json!("#ffffff"), json!("#ffffff")) } else { (c.text.clone(), c.dim.clone()) };
            let glyph = if !dir { "" } else if app.expanded.contains(path) { "▾" } else { "▸" };
            let row = view(
                &format!("tree:{path}"),
                json!({"position":"absolute","left":8,"right":8,"top":i as f64 * TREE_ROW - app.tree_top,"height":TREE_ROW,"flexDirection":"row",
                    "alignItems":"center","paddingStart":6 + depth * 14,"paddingEnd":8,"gap":4,"borderRadius":6,
                    "backgroundColor":if on_this { c.accent.clone() } else { json!("transparent") }}),
                vec![
                    text(glyph, json!({"width":12,"fontSize":10,"textAlign":"center","color":mark})),
                    clipped(text(label.as_str(), json!({"flex":1,"fontSize":13,"color":color}))),
                ],
            );
            click(row, json!([if *dir { "treeToggle" } else { "treeOpen" }, path]))
        })
        .collect();
    let mut open_folder = click(
        view("open-folder", json!({"width":24,"height":22,"borderRadius":5,"alignItems":"center","justifyContent":"center"}), vec![
            text("…", json!({"fontSize":14,"color":c.text})),
        ]),
        json!(["menu", "openFolder"]),
    );
    open_folder["tooltip"] = json!({"key":"editor.cmd.openFolder"});
    view(
        "sidebar",
        json!({"width":SIDEBAR,"flexShrink":0,"backgroundColor":c.sidebar}),
        vec![
            view("project", json!({"height":TABS,"flexDirection":"row","alignItems":"center","paddingStart":18,"paddingEnd":8,"gap":6}), vec![
                clipped(text(name, json!({"flex":1,"fontSize":11,"fontWeight":600,"color":c.dim}))),
                open_folder,
            ]),
            on(view("tree", json!({"flex":1,"overflow":"hidden"}), list), "onWheel", json!(["treeWheel"])),
            rule("sidebar-rule", json!({"end":0,"top":0,"bottom":0,"width":1}), &c.separator),
        ],
    )
}

fn tab_bar(app: &mut App, c: &Colors) -> Value {
    let ids: Vec<u64> = app.tabs.iter().map(|t| t.id).collect();
    let mut kids = vec![rule("tabs-rule", json!({"left":0,"right":0,"bottom":0,"height":1}), &c.separator)];
    for id in ids {
        let name = app.tab_name(id);
        let dirty = app.tab_by_id(id).is_some_and(|t| t.doc.dirty());
        let active = app.active == id;
        let close = click(
            view("close", json!({"width":16,"height":16,"borderRadius":4,"flexShrink":0,"alignItems":"center","justifyContent":"center"}), vec![
                text(if dirty { "●" } else { "×" }, json!({"fontSize":if dirty {10} else {12},"color":if dirty { &c.text } else { &c.dim }})),
            ]),
            json!(["closeTab", id]),
        );
        let tab = view(
            &format!("tab:{id}"),
            json!({"maxWidth":200,"minWidth":0,"flexShrink":1,"paddingStart":14,"paddingEnd":8,"flexDirection":"row","alignItems":"center","gap":6,
                "backgroundColor":if active { json!(c.editor) } else { json!("transparent") },"borderRightWidth":1,"borderColor":c.separator}),
            vec![clipped(text(name, json!({"fontSize":12,"flexShrink":1,"color":if active { &c.text } else { &c.dim }}))), close],
        );
        kids.push(click(tab, json!(["tab", id])));
    }
    view("tabs", json!({"height":TABS,"flexShrink":0,"flexDirection":"row","overflow":"hidden","backgroundColor":c.sidebar,"direction":"ltr"}), kids)
}

fn block(key: String, left: f64, top: f64, width: f64, height: f64, color: &Value) -> Value {
    view(&key, json!({"position":"absolute","left":left,"top":top,"width":width,"height":height,"backgroundColor":color}), vec![])
}

fn text_area(app: &mut App, c: &Colors, now: i64) -> Value {
    let (first, last) = app.visible();
    let gutter = app.gutter();
    let (top, left, tab_size) = (app.tab().top, app.tab().left, app.tab().tab_size);
    let start = gutter + PAD - left;
    let y = |line: usize| line as f64 * LINE - top;
    let mut kids = Vec::new();
    let primary = app.tab().doc.primary();
    if !app.tab().doc.any_selection() && (first..=last).contains(&primary.l) {
        kids.push(view("current-line", json!({"position":"absolute","left":0,"right":0,"top":y(primary.l),"height":LINE,"backgroundColor":c.line}), vec![]));
    }
    if app.find.open {
        let current = app.find.found.current(&app.tab().doc);
        for (i, m) in app.find.found.list.iter().enumerate() {
            if m.line < first || m.line > last {
                continue;
            }
            let (x0, x1) = (app.x_of(m.from()), app.x_of(m.to()));
            let mut node = block(format!("match:{i}"), start + x0, y(m.line) + 1., (x1 - x0).max(2.), LINE - 2.,
                &json!(if i == current { c.current } else { c.found }));
            node["style"]["borderRadius"] = json!(3);
            kids.push(node);
        }
    }
    let focused = app.focused && app.editing;
    let selection = json!(if focused { c.selection } else { c.selection_blur });
    for (i, k) in app.tab().doc.cursors.iter().enumerate() {
        if !k.has_selection() {
            continue;
        }
        let (s, e) = (k.start(), k.end());
        for l in s.l.max(first)..=e.l.min(last) {
            let x0 = if l == s.l { app.x_of(s) } else { 0. };
            let x1 = if l == e.l {
                app.x_of(e)
            } else {
                app.x_of(pos(l, app.tab().doc.len(l))) + app.cell * 0.6
            };
            if x1 > x0 {
                kids.push(block(format!("selection:{i}:{l}"), start + x0, y(l), x1 - x0, LINE, &selection));
            }
        }
    }
    // highlighted lines
    {
        let dark = app.dark;
        let index = app.index();
        let tab = &mut app.tabs[index];
        let changed = std::mem::replace(&mut tab.doc.first_changed, usize::MAX);
        tab.highlight.invalidate(tab.language, changed);
        for l in first..=last {
            let tokens = tab.highlight.tokens(&tab.doc, l);
            let mut column = 0;
            let spans: Vec<Value> = tokens
                .iter()
                .enumerate()
                .map(|(i, (source, kind))| {
                    let mut shown = String::with_capacity(source.len());
                    for ch in source.chars() {
                        if ch == '\t' {
                            let n = tab_size - column % tab_size;
                            shown.extend(std::iter::repeat_n(' ', n));
                            column += n;
                        } else {
                            shown.push(ch);
                            column += 1;
                        }
                    }
                    json!({"kind":"text","key":i,"label":shown,"style":{"fontFamily":"monospace","fontSize":FONT,"whiteSpace":"nowrap","flexShrink":0,
                        "color":syntax::color(kind, dark),"fontWeight":if matches!(*kind, "bold" | "heading") {700} else {400}}})
                })
                .collect();
            kids.push(view(&format!("line:{l}"), json!({"position":"absolute","left":start,"top":y(l),"height":LINE,"flexDirection":"row","alignItems":"center"}), spans));
        }
    }
    let blink_on = ((now - app.blink_from) / BLINK_MS) % 2 == 0;
    if focused && blink_on {
        for (i, k) in app.tab().doc.cursors.iter().enumerate() {
            if (first..=last).contains(&k.l) {
                kids.push(block(format!("caret:{i}"), start + app.x_of(k.head()) - 1., y(k.l) + 2., 2., LINE - 4., &json!(c.caret)));
            }
        }
    }
    let active: std::collections::HashSet<usize> = app.tab().doc.cursors.iter().map(|k| k.l).collect();
    let numbers: Vec<Value> = (first..=last)
        .map(|l| {
            let mut node = text(
                (l + 1).to_string(),
                json!({"position":"absolute","left":0,"top":y(l) + 2.,"width":gutter - 12.,"textAlign":"right","fontFamily":"monospace",
                    "fontSize":FONT - 1.,"color":if active.contains(&l) { c.gutter_active } else { c.gutter_text }}),
            );
            node["key"] = json!(format!("number:{l}"));
            node
        })
        .collect();
    kids.push(view("gutter", json!({"position":"absolute","left":0,"top":0,"bottom":0,"width":gutter,"backgroundColor":c.gutter,"overflow":"hidden"}), numbers));
    let area = view(
        &format!("area:{}", app.active),
        json!({"flex":1,"minHeight":0,"position":"relative","overflow":"hidden","backgroundColor":c.editor,"direction":"ltr"}),
        kids,
    );
    on(on(area, "onMouseDown", json!(["areaDown"])), "onWheel", json!(["areaWheel"]))
}

fn field(key: String, value: &str, placeholder: &str, change: &str) -> Value {
    json!({"kind":"input","key":key,"value":value,"placeholder":placeholder,"change":[change],"size":13,
        "style":{"flex":1,"minWidth":0,"height":28}})
}
fn chip(c: &Colors, key: &str, label: &str, tooltip: &str, action: &str, on_now: bool) -> Value {
    let mut node = click(
        view(key, json!({"minWidth":30,"height":28,"borderRadius":6,"paddingHorizontal":6,"alignItems":"center","justifyContent":"center",
            "backgroundColor":if on_now { c.accent.clone() } else { json!("transparent") }}), vec![
            text(label, json!({"fontSize":12,"color":if on_now { json!("#ffffff") } else { c.text.clone() }})),
        ]),
        json!([action]),
    );
    node["tooltip"] = json!({"key":tooltip});
    node
}

fn find_bar(app: &mut App, c: &Colors) -> Value {
    let status = if app.find.found.bad_pattern {
        app.tr("find.badRegex")
    } else if app.find.query.text.is_empty() {
        String::new()
    } else {
        let count = app.find.found.list.len();
        let current = if count == 0 { 0 } else { app.find.found.current(&app.tab().doc) + 1 };
        app.tr_with("find.count", json!({"current":current,"count":count}))
    };
    let status_color = if app.find.found.bad_pattern { json!("#ff453a") } else { c.dim.clone() };
    let placeholder = app.tr("find.placeholder");
    let replace_placeholder = app.tr("find.replacePlaceholder");
    let mut query = field(format!("find:{}", app.find.focus), &app.find.query.text, &placeholder, "findQuery");
    query["autoFocus"] = json!(true);
    query["keys"] = json!(["Enter"]);
    query["onKey"] = json!(["findKey"]);
    query["cancel"] = json!(["findClose"]);
    let mut replacement = field("find-replacement".into(), &app.find.replacement, &replace_placeholder, "findReplacement");
    replacement["submit"] = json!(["replace"]);
    replacement["keepFocus"] = json!(true);
    replacement["cancel"] = json!(["findClose"]);
    let row = |key: &str, kids| view(key, json!({"height":28,"flexDirection":"row","alignItems":"center","gap":8}), kids);
    let first = row("find-row", vec![
        query,
        chip(c, "regex", ".*", "editor.find.regex", "findRegex", app.find.query.regex),
        chip(c, "case", "Aa", "editor.find.case", "findCase", app.find.query.case_sensitive),
        text(status, json!({"width":92,"fontSize":11,"textAlign":"center","color":status_color})),
        chip(c, "previous", "↑", "editor.find.prev", "findPrev", false),
        chip(c, "next", "↓", "editor.find.next", "findNext", false),
        chip(c, "close-find", "✕", "editor.find.close", "findClose", false),
    ]);
    let (one, all) = (app.tr("find.replace"), app.tr("find.replaceAll"));
    let second = row("replace-row", vec![replacement, button("replace", &one, "replace", false), button("replace-all", &all, "replaceAll", false)]);
    view("find-bar", json!({"height":FIND,"flexShrink":0,"paddingVertical":8,"paddingHorizontal":12,"gap":8,"backgroundColor":c.sidebar}), vec![
        first,
        second,
        rule("find-rule", json!({"left":0,"right":0,"top":0,"height":1}), &c.separator),
    ])
}

fn status_bar(app: &mut App, c: &Colors) -> Value {
    let small = |label: String, color: &Value| text(label, json!({"fontSize":11,"color":color,"whiteSpace":"nowrap"}));
    let doc = &app.tab().doc;
    let k = doc.primary();
    let tab_size = app.tab().tab_size;
    let column = doc.line(k.l)[..k.c].iter().map(|&ch| if ch == '\t' { tab_size } else { 1 }).sum::<usize>() + 1;
    let selected: usize = doc.cursors.iter().filter(|k| k.has_selection()).map(|k| doc.text_in(k.start(), k.end()).chars().count()).sum();
    let (cursors, use_tabs, language, eol) = (doc.cursors.len(), app.tab().use_tabs, app.tab().language, doc.eol);
    let position = app.tr_with("status.position", json!({"line":k.l + 1,"col":column}));
    let mut kids = vec![rule("status-rule", json!({"left":0,"right":0,"top":0,"height":1}), &c.separator), small(position, &c.dim)];
    if cursors > 1 {
        let label = app.tr_with("status.cursors", json!({"count":cursors}));
        kids.push(small(label, &c.accent));
    }
    if selected > 0 {
        let label = app.tr_with("status.selected", json!({"count":selected}));
        kids.push(small(label, &c.dim));
    }
    if let Some((ok, message)) = app.message.clone() {
        kids.push(clipped(text(message, json!({"fontSize":11,"flexShrink":1,"color":if ok {"#30d158"} else {"#ff453a"}}))));
    }
    kids.push(view("status-space", json!({"flex":1}), vec![]));
    let language_name = if language == "plain" { app.tr("plain") } else { syntax::by_id(language).name.to_owned() };
    let indentation = app.tr_with(if use_tabs { "status.tabs" } else { "status.spaces" }, json!({"n":tab_size}));
    let action = |key: &str, node: Value, command: &str| click(view(key, json!({"height":STATUS,"justifyContent":"center","flexShrink":0}), vec![node]), json!([command]));
    kids.extend([
        click(view("status-language", json!({"height":STATUS,"justifyContent":"center","flexShrink":0}), vec![small(language_name, &c.text)]), json!(["menu", "language"])),
        action("status-indent", small(indentation, &c.text), "useTabs"),
        action("status-tab-size", small(format!("⇥ {tab_size}"), &c.dim), "tabSize"),
        small("UTF-8".into(), &c.dim),
        action("status-eol", small(if eol == "\n" { "LF" } else { "CRLF" }.into(), &c.text), "eol"),
    ]);
    view("status", json!({"height":STATUS,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":16,"paddingHorizontal":14,
        "backgroundColor":c.sidebar,"direction":"ltr"}), kids)
}

/// A card over the window; a click outside it runs `outside`.
fn card(c: &Colors, key: &str, width: f64, top: f64, shade: bool, outside: Option<&str>, children: Vec<Value>) -> Value {
    let panel = click(
        view(key, json!({"marginTop":top,"width":width,"maxWidth":"100%","padding":12,"gap":8,"borderRadius":12,"backgroundColor":c.window,
            "borderWidth":1,"borderColor":c.separator,"boxShadow":"0 16px 40px rgba(0,0,0,0.45)"}), children),
        json!(["noop"]),
    );
    let backdrop = view("backdrop", json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"alignItems":"center",
        "backgroundColor":if shade {"rgba(0,0,0,0.35)"} else {"transparent"}}), vec![panel]);
    click(backdrop, json!([outside.unwrap_or("noop")]))
}
fn title(c: &Colors, label: String) -> Value {
    clipped(text(label, json!({"fontSize":11,"fontWeight":600,"color":c.dim,"marginHorizontal":4})))
}

fn chooser(app: &mut App, c: &Colors) -> Value {
    let (query, selected, files) = match &app.overlay {
        Overlay::GoToFile { query, selected } => (query.clone(), *selected, true),
        Overlay::Languages { query, selected } => (query.clone(), *selected, false),
        _ => (String::new(), 0, false),
    };
    let heading = if files {
        let folder = if app.root.is_empty() { app.tr("home") } else { base_name(&app.root).to_owned() };
        app.tr_with("gotoFile.title", json!({"folder":folder}))
    } else {
        app.tr("languageTitle")
    };
    let placeholder = app.tr(if files { "gotoFile.placeholder" } else { "languagePlaceholder" });
    let mut input = field(if files { "go-to-file".into() } else { "languages".into() }, &query, &placeholder, "listQuery");
    input["autoFocus"] = json!(true);
    input["keys"] = json!(["ArrowDown", "ArrowUp"]);
    input["onKey"] = json!(["listKey"]);
    input["submit"] = json!(["listSubmit"]);
    input["keepFocus"] = json!(true);
    input["cancel"] = json!(["overlayClose"]);
    input["style"] = json!({"height":28});
    // the selected row stays inside the visible part of the list
    let shown = (((app.height - 130.) / 28.).floor() as usize).clamp(3, 11);
    let first = selected.saturating_sub(shown - 1);
    let rows: Vec<Value> = app
        .choices()
        .into_iter()
        .enumerate()
        .skip(first)
        .take(shown)
        .map(|(i, (key, _))| {
            let here = i == selected;
            let main = if here { json!("#ffffff") } else { c.text.clone() };
            let secondary = if here { json!("#ffffff") } else { c.dim.clone() };
            let parts = if files {
                vec![
                    text(base_name(&key), json!({"fontSize":12,"fontWeight":600,"color":main,"whiteSpace":"nowrap"})),
                    clipped(text(folder_of(&key), json!({"fontSize":11,"flexShrink":1,"color":secondary}))),
                ]
            } else {
                vec![text(key.as_str(), json!({"fontSize":12,"color":main}))]
            };
            click(
                view(&format!("choice:{key}"), json!({"height":28,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":10,"borderRadius":6,
                    "backgroundColor":if here { c.accent.clone() } else { json!("transparent") }}), parts),
                json!(["choose", i]),
            )
        })
        .collect();
    card(c, "chooser", 480., 30., false, Some("overlayClose"), vec![title(c, heading), input, view("choices", json!({"overflow":"hidden"}), rows)])
}

fn go_to_line(app: &mut App, c: &Colors, query: &str) -> Value {
    let heading = app.tr_with("gotoLine.title", json!({"count":app.tab().doc.line_count()}));
    let placeholder = app.tr("gotoLine.placeholder");
    let mut input = field("go-to-line".into(), query, &placeholder, "lineQuery");
    input["autoFocus"] = json!(true);
    input["submit"] = json!(["lineSubmit"]);
    input["cancel"] = json!(["overlayClose"]);
    input["style"] = json!({"height":28});
    card(c, "line-card", 320., 30., false, Some("overlayClose"), vec![title(c, heading), input])
}

fn confirm(app: &mut App, c: &Colors, tab: u64) -> Value {
    let name = app.tab_name(tab);
    let heading = app.tr_with("confirm.title", json!({"name":name}));
    let (body, discard, cancel, save) = (app.tr("confirm.body"), app.tr("confirm.discard"), app.tr("cancel"), app.tr("save"));
    card(c, "confirm", 400., 60., true, None, vec![
        text(heading, json!({"fontSize":15,"fontWeight":600,"color":c.text})),
        text(body, json!({"fontSize":13,"color":c.dim,"lineHeight":1.4})),
        view("confirm-buttons", json!({"flexDirection":"row","gap":8,"justifyContent":"flex-end","marginTop":12}), vec![
            button("discard", &discard, "discard", false),
            button("cancel", &cancel, "overlayClose", false),
            button("confirm-save", &save, "confirmSave", true),
        ]),
    ])
}

/// The input method's composition and candidates, under the caret.
fn composition(app: &App, c: &Colors) -> Option<Value> {
    let state = &app.composition;
    let preedit = text_of(&state["preedit"]);
    let candidates = state["candidates"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if !app.takes_text() || (preedit.is_empty() && candidates.is_empty()) {
        return None;
    }
    let selected = state["selected"].as_u64().unwrap_or(0) as usize;
    let mut kids = Vec::new();
    if !preedit.is_empty() {
        kids.push(text(preedit, json!({"fontSize":15,"color":c.text,"borderBottomWidth":1,"borderBottomColor":c.accent})));
    }
    let first = selected / 9 * 9;
    let list: Vec<Value> = candidates
        .iter()
        .enumerate()
        .skip(first)
        .take(9)
        .map(|(i, word)| {
            text(format!("{} {}", i - first + 1, text_of(word)), json!({"fontSize":14,"padding":4,"borderRadius":4,
                "backgroundColor":if i == selected { c.accent.clone() } else { json!("transparent") },
                "color":if i == selected { json!("#ffffff") } else { c.text.clone() }}))
        })
        .collect();
    if !list.is_empty() {
        kids.push(view("candidates", json!({"flexDirection":"row","gap":2}), list));
    }
    let k = app.tab().doc.primary();
    let (width, height) = app.area_size();
    let side = if app.panel && !app.rtl { SIDEBAR } else { 0. };
    let x = side + (app.gutter() + PAD + app.x_of(k.head()) - app.tab().left).clamp(0., (width - 200.).max(0.));
    let y = TABS + ((k.l + 1) as f64 * LINE - app.tab().top).clamp(0., (height - 40.).max(0.));
    Some(view("composition", json!({"position":"absolute","left":x,"top":y,"maxWidth":width,"padding":6,"gap":4,"borderRadius":6,
        "backgroundColor":c.window,"borderWidth":1,"borderColor":c.separator,"pointerEvents":"none"}), kids))
}

pub(super) fn tree(app: &mut App, now: i64) -> Value {
    let c = colors(&app.theme, app.dark);
    let mut root = Vec::new();
    if app.panel {
        root.push(sidebar(app, &c, now));
    }
    let mut column = vec![tab_bar(app, &c), text_area(app, &c, now)];
    if app.find.open {
        column.push(find_bar(app, &c));
    }
    column.push(status_bar(app, &c));
    root.push(view("column", json!({"flex":1,"minWidth":0}), column));
    match &app.overlay {
        Overlay::GoToFile { .. } | Overlay::Languages { .. } => root.push(chooser(app, &c)),
        Overlay::GoToLine { query } => {
            let query = query.clone();
            root.push(go_to_line(app, &c, &query));
        }
        Overlay::Confirm { tab, .. } => {
            let tab = *tab;
            root.push(confirm(app, &c, tab));
        }
        Overlay::None | Overlay::Picking => {}
    }
    root.extend(composition(app, &c));
    let mut tree = view("editor", json!({"flex":1,"flexDirection":"row","position":"relative","overflow":"hidden","backgroundColor":c.editor}), root);
    tree["dropAction"] = json!(["drop"]);
    tree
}
