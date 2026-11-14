use super::{emoji, markdown, store, App};
use crate::native_view::NativeApp;
use serde_json::{json, Value};
pub(super) fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
pub(super) fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
pub(super) fn click(mut node: Value, action: Value) -> Value {
    node["events"]["onClick"] = action;
    node
}
fn v(id: &str, style: Value, children: Vec<Value>) -> Value {
    let mut v = view(style, children);
    v["key"] = json!(id);
    v
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn label(c: Value, t: &Value, dim: bool) -> Value {
    text(
        c,
        json!({"fontSize":13,"color":t[if dim{"textDim"}else{"text"}],"lineHeight":1.4}),
    )
}
fn button(c: Value, cmd: &str, arg: Value, primary: bool) -> Value {
    json!({"kind":"button","key":format!("{cmd}:{arg}"),"label":c,"action":[cmd,arg],"primary":primary,"small":true})
}
fn input(id: &str, value: &str, placeholder: Value) -> Value {
    json!({"kind":"input","key":id,"value":value,"change":[id],"placeholder":placeholder,"style":{"flex":1,"minWidth":0,"minHeight":28}})
}
fn row(id: &str, c: Vec<Value>) -> Value {
    v(
        id,
        json!({"flexDirection":"row","gap":6,"alignItems":"center"}),
        c,
    )
}
fn paper(c: &str) -> (&'static str, &'static str) {
    match c {
        "pink" => ("#ffd9e8", "#ffc2db"),
        "green" => ("#d9f6cc", "#c1edab"),
        "blue" => ("#d6ecff", "#bcdfff"),
        "purple" => ("#e9ddff", "#d9c6ff"),
        "gray" => ("#eeeeee", "#dedede"),
        _ => ("#fff6a8", "#fde68a"),
    }
}
fn paper_theme() -> Value {
    json!({"mode":"light","text":"#2a2a2a","textDim":"#6b6b6b","accent":"#0a5fd0","separator":"rgba(0,0,0,0.14)","hover":"rgba(0,0,0,0.06)","control":"rgba(0,0,0,0.08)","inputBg":"rgba(255,255,255,0.7)"})
}
pub(super) fn icon(s: f64) -> Value {
    let s = if s.is_finite() {
        s.clamp(1., 512.)
    } else {
        32.
    };
    let mut c = vec![v(
        "paper",
        json!({"position":"absolute","left":s*0.2,"top":s*0.18,"width":s*0.6,"height":s*0.64,"borderRadius":s*0.05,"backgroundColor":"#fffbe0","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.25)",s*0.02,s*0.05)}),
        vec![],
    )];
    for i in 0..4 {
        c.push(v(&format!("line-{i}"),json!({"position":"absolute","left":s*0.28,"top":s*(0.32+i as f64*0.11),"width":s*if i==3{0.26}else{0.44},"height":(s*0.035).max(1.),"borderRadius":s*0.02,"backgroundColor":if i==0{"#e0a800"}else{"#c9b98a"}}),vec![]));
    }
    c.push(v("fold",json!({"position":"absolute","left":s*0.66,"top":s*0.68,"width":s*0.14,"height":s*0.14,"backgroundColor":"#f3d77a","borderRadius":s*0.03}),vec![]));
    v(
        "notes-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #ffe066, #f7b733)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden"}),
        c,
    )
}
fn dot(color: &str, selected: bool, size: f64) -> Value {
    click(
        v(
            color,
            json!({"width":size,"height":size,"borderRadius":size/2.,"backgroundColor":paper(color).0,"borderWidth":1.5,"borderColor":if selected{"#333"}else{"rgba(0,0,0,0.25)"},"flexShrink":0}),
            vec![],
        ),
        json!(["color", color]),
    )
}
fn headings(source: &str) -> Vec<(usize, usize, String)> {
    let mut fenced = false;
    let h = regex::Regex::new(r"^(#{1,6})\s+(.*?)\s*#*\s*$").unwrap();
    source
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            if l.starts_with("```") || l.starts_with("~~~") {
                fenced = !fenced;
            }
            if fenced {
                return None;
            }
            let c = h.captures(l)?;
            Some((i, c[1].len(), c[2].into()))
        })
        .collect()
}
fn read(app: &App, n: &store::Note, width: f64) -> Value {
    let t = paper_theme();
    let base = store::with(&app.home, |s| s.path(&n.slug).unwrap_or_default());
    let expanded = emoji::expand(&n.body);
    let images = regex::Regex::new(r"!\[([^\]]*)\]\(([^)\s]+)\)").unwrap();
    let source = images
        .replace_all(&expanded, |c: &regex::Captures| {
            let path = std::path::Path::new(&c[2]);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                base.join(path)
            };
            if path
                .canonicalize()
                .ok()
                .is_some_and(|p| p.starts_with(&app.home))
            {
                format!("![{}]({})", &c[1], path.to_string_lossy())
            } else {
                format!("[{}]", &c[1])
            }
        })
        .into_owned();
    let blocks = markdown::parse(&source);
    let mut rendered = markdown::render(&blocks, width, &t);
    let heads = headings(&n.body);
    let mut index = 0;
    for (block, node) in blocks
        .iter()
        .zip(rendered["children"].as_array_mut().into_iter().flatten())
    {
        if matches!(block, markdown::Block::Heading(..)) {
            if let Some((line, _, _)) = heads.get(index) {
                node["key"] = json!(format!("heading-{line}"));
            }
            index += 1;
        }
    }
    let mut fenced = false;
    let tasks: Vec<_> = n
        .body
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            if l.trim_start().starts_with("```") || l.trim_start().starts_with("~~~") {
                fenced = !fenced;
            }
            if !fenced
                && regex::Regex::new(r"^\s*(>\s*)*([-*+]|\d+[.)])\s+\[[ xX]\]")
                    .unwrap()
                    .is_match(l)
            {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    fn decorate(node: &mut Value, tasks: &[usize], at: &mut usize) {
        if node["style"]["width"] == 16
            && node["style"]["height"] == 16
            && node["style"]["borderRadius"] == 4
        {
            if let Some(line) = tasks.get(*at) {
                node["events"]["onClick"] = json!(["toggleTask", line]);
                *at += 1;
            }
        }
        for child in node["children"].as_array_mut().into_iter().flatten() {
            decorate(child, tasks, at);
        }
    }
    decorate(&mut rendered, &tasks, &mut 0);
    let body = click(
        v(
            "reading-body",
            json!({"paddingVertical":if app.widget{10}else{20},"paddingHorizontal":if app.widget{10}else{28},"minHeight":60,"direction":direction(&n.body)}),
            if n.body.trim().is_empty() {
                vec![label(key("notes.empty"), &t, true)]
            } else {
                vec![rendered]
            },
        ),
        json!(["mode", "edit"]),
    );
    let mut body = body;
    body["events"].as_object_mut().unwrap().remove("onClick");
    body["events"]["onDoubleClick"] = json!(["mode", "edit"]);
    let scroll = json!({"kind":"scroll","key":"reading","style":{"flex":1},"children":[body]});
    if !app.widget && app.toc && width >= 520. && heads.len() > 1 {
        let mut toc = vec![label(key("notes.toc"), &t, true)];
        for (line, level, s) in heads {
            toc.push(click(
                v(
                    &format!("toc-{line}"),
                    json!({"paddingVertical":4,"paddingLeft":(level-1)*10}),
                    vec![label(json!(s), &t, level != 1)],
                ),
                json!(["tocJump", line.to_string()]),
            ));
        }
        let mut reader = row(
            "reader",
            vec![
                v(
                    "outline",
                    json!({"width":180,"paddingTop":20,"paddingHorizontal":16,"alignSelf":"stretch","borderEndWidth":1,"borderEndColor":t["separator"]}),
                    toc,
                ),
                scroll,
            ],
        );
        reader["style"]["flex"] = json!(1);
        reader["style"]["alignItems"] = json!("stretch");
        reader
    } else {
        scroll
    }
}
fn emoji_panel(app: &App, t: &Value) -> Value {
    let list = if app.emoji_query.is_empty() {
        emoji::all()
            .iter()
            .filter(|e| e.cat == app.emoji_cat)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        emoji::search(&app.emoji_query)
    };
    let mut search = input("emojiQuery", &app.emoji_query, key("notes.emoji.search"));
    search["autoFocus"] = json!(true);
    search["submit"] = json!(["emojiFirst"]);
    search["cancel"] = json!(["dismiss"]);
    let mut content = vec![search];
    if app.emoji_query.is_empty() {
        content.push(v(
            "categories",
            json!({"flexDirection":"row","flexWrap":"wrap","gap":3}),
            emoji::CATEGORIES
                .iter()
                .map(|c| {
                    button(
                        key(&format!("notes.emoji.cat.{c}")),
                        "emojiCat",
                        json!(c),
                        *c == app.emoji_cat,
                    )
                })
                .collect(),
        ));
    }
    content.push(json!({"kind":"scroll","style":{"maxHeight":220,"flex":1},"children":[v("emojis",json!({"flexDirection":"row","flexWrap":"wrap","gap":2}),list.into_iter().take(96).map(|e|click(v(&e.name,json!({"width":32,"height":30,"alignItems":"center","justifyContent":"center"}),vec![text(json!(e.ch),json!({"fontSize":18}))]),json!(["emoji",e.ch]))).collect())]}));
    content.push(button(key("notes.close"), "dismiss", Value::Null, false));
    v(
        "emoji-panel",
        json!({"position":"absolute","right":8,"top":if app.widget{28}else{48},"width":308f64.min((app.width-16.).max(160.)),"maxHeight":(app.height-60.).max(110.),"padding":10,"gap":8,"borderRadius":12,"backgroundColor":t["windowBg"],"borderWidth":1,"borderColor":t["separator"],"boxShadow":"0 12px 30px rgba(0,0,0,0.35)"}),
        content,
    )
}
fn panel(app: &App, note: Option<&store::Note>, t: &Value) -> Value {
    match app.panel.as_str() {
        "emoji" => emoji_panel(app, t),
        "image" | "attach" => {
            let mut rows = vec![];
            for pick in &app.picks {
                rows.push(button(
                    json!(format!(
                        "{} {}",
                        if pick.dir { "▸" } else { " " },
                        pick.name
                    )),
                    "pickFile",
                    json!(pick.path),
                    false,
                ));
            }
            if rows.is_empty() {
                rows.push(label(key("notes.picker.empty"), t, true));
            }
            v(
                "picker-overlay",
                json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"backgroundColor":"rgba(0,0,0,0.3)","alignItems":"center","justifyContent":"center"}),
                vec![v(
                    "picker",
                    json!({"width":420f64.min(app.width-24.),"height":(app.height-32.).min(440.),"padding":16,"gap":8,"borderRadius":12,"backgroundColor":t["windowBg"]}),
                    vec![
                        row(
                            "path",
                            vec![
                                button(json!("⌂"), "pickHome", Value::Null, false),
                                button(json!("↑"), "pickUp", Value::Null, false),
                                label(json!(app.picker.to_string_lossy()), t, true),
                            ],
                        ),
                        json!({"kind":"scroll","key":"picker-list","style":{"flex":1},"children":rows}),
                        button(key("notes.cancel"), "dismiss", Value::Null, false),
                    ],
                )],
            )
        }
        "delete" => v(
            "delete-overlay",
            json!({"position":"absolute","left":0,"right":0,"top":0,"bottom":0,"alignItems":"center","justifyContent":"center","backgroundColor":"rgba(0,0,0,0.25)"}),
            vec![v(
                "delete-dialog",
                json!({"width":340,"padding":20,"gap":8,"borderRadius":12,"backgroundColor":t["windowBg"],"borderWidth":1,"borderColor":t["separator"]}),
                vec![
                    label(
                        json!({"key":"notes.deleteTitle","params":{"title":note.map(|n|n.title.clone()).unwrap_or_default()}}),
                        t,
                        false,
                    ),
                    label(key("notes.deleteHint"), t, true),
                    row(
                        "delete-actions",
                        vec![
                            button(key("notes.cancel"), "dismiss", Value::Null, false),
                            button(key("notes.delete"), "confirmDelete", Value::Null, true),
                        ],
                    ),
                ],
            )],
        ),
        "more" | "insert" => {
            let commands = if app.panel == "insert" {
                vec![
                    ("notes.insertEmoji", "panel", "emoji"),
                    ("notes.insertImage", "panel", "image"),
                    ("notes.insertFile", "panel", "attach"),
                ]
            } else {
                vec![
                    ("notes.duplicate", "duplicate", ""),
                    ("notes.showDesktop", "desktop", ""),
                    ("notes.openFolder", "folder", ""),
                    ("notes.print", "print", ""),
                    ("notes.toc", "toc", ""),
                    ("notes.delete", "delete", ""),
                ]
            };
            let mut items = commands
                .into_iter()
                .map(|(l, c, a)| button(key(l), c, json!(a), false))
                .collect::<Vec<_>>();
            if app.panel == "more" {
                items.push(row(
                    "colors",
                    store::COLORS
                        .iter()
                        .map(|c| dot(c, note.is_some_and(|n| n.color == *c), 14.))
                        .collect(),
                ));
            }
            items.push(button(key("notes.close"), "dismiss", Value::Null, false));
            v(
                "popup",
                json!({"position":"absolute","right":8,"top":46,"width":220,"padding":8,"gap":4,"borderRadius":10,"backgroundColor":t["windowBg"],"borderWidth":1,"borderColor":t["separator"],"boxShadow":"0 8px 24px rgba(0,0,0,0.3)"}),
                items,
            )
        }
        _ => Value::Null,
    }
}
pub(super) fn tree(app: &mut App, now: i64) -> Value {
    if app.home.is_empty() {
        return view(json!({}), vec![]);
    }
    let note = app.note();
    let t = app.theme.clone();
    let side = if app.width < 720. { 200. } else { 240. };
    let body_width = if app.widget {
        app.width
    } else {
        app.width - side
    };
    let header = if app.widget { 26. } else { 44. };
    app.editor.x = if app.widget || app.rtl { 0. } else { side };
    app.editor.y = header;
    app.editor.width = body_width;
    app.editor.height = (app.height - header).max(1.);
    app.editor.padding = if app.widget { 8. } else { 14. };
    app.editor.size = if app.widget { 13. } else { 14. };
    app.editor.layout();
    if app.widget && note.is_none() {
        return view(json!({}), vec![]);
    }
    let content = if let Some(n) = &note {
        let body = if app.mode == "edit" {
            app.editor.render(now)
        } else {
            read(app, n, body_width)
        };
        let toolbar = if app.widget {
            let mut bar = store::COLORS
                .iter()
                .map(|c| dot(c, n.color == *c, 11.))
                .collect::<Vec<_>>();
            bar.extend([
                button(
                    json!(if app.mode == "edit" { "👁" } else { "✎" }),
                    "toggle",
                    Value::Null,
                    false,
                ),
                button(json!("😀"), "panel", json!("emoji"), false),
                button(
                    json!(if n.pinned { "📌" } else { "📍" }),
                    "pin",
                    Value::Null,
                    false,
                ),
                button(json!("⤢"), "openWindow", Value::Null, false),
                button(json!("✕"), "hideWidget", Value::Null, false),
            ]);
            let mut h = v(
                "widget-header",
                json!({"height":26,"flexDirection":"row","gap":4,"alignItems":"center","paddingHorizontal":6,"backgroundColor":if app.hover||app.mode=="edit"{paper(&n.color).1}else{paper(&n.color).0},"flexShrink":0}),
                if app.hover || app.mode == "edit" || !app.panel.is_empty() {
                    bar
                } else {
                    vec![label(json!(n.title), &paper_theme(), true)]
                },
            );
            h["events"]["onMouseDown"] = json!(["dragStart", "move"]);
            h
        } else {
            let mut controls = vec![
                button(key("notes.read"), "mode", json!("read"), app.mode == "read"),
                button(key("notes.edit"), "mode", json!("edit"), app.mode == "edit"),
            ];
            if body_width >= 470. {
                controls.extend(store::COLORS.iter().map(|c| dot(c, n.color == *c, 14.)));
            }
            controls.push(v("spacer", json!({"flex":1}), vec![]));
            controls.push(button(key("notes.insert"), "panel", json!("insert"), false));
            controls.push(button(key("notes.more"), "panel", json!("more"), false));
            v(
                "toolbar",
                json!({"height":44,"paddingHorizontal":8,"flexDirection":"row","gap":6,"alignItems":"center","flexShrink":0}),
                controls,
            )
        };
        let mut c = vec![
            toolbar,
            v(
                "paper",
                json!({"flex":1,"backgroundColor":paper(&n.color).0}),
                vec![body],
            ),
        ];
        if app.widget {
            let mut handle = v(
                "resize",
                json!({"position":"absolute","right":0,"bottom":0,"width":16,"height":16}),
                vec![text(
                    json!("◢"),
                    json!({"fontSize":10,"color":"rgba(0,0,0,0.35)"}),
                )],
            );
            handle["events"]["onMouseDown"] = json!(["dragStart", "size"]);
            c.push(handle);
        }
        v(
            "content",
            json!({"flex":1,"minWidth":0,"backgroundColor":paper(&n.color).0}),
            c,
        )
    } else {
        v(
            "empty",
            json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":12}),
            vec![
                icon(56.),
                label(key("notes.none"), &t, false),
                label(key("notes.noneHint"), &t, true),
                button(key("notes.new"), "new", Value::Null, true),
            ],
        )
    };
    let mut children = vec![];
    if !app.widget {
        let notes = store::with(&app.home, |s| s.list.clone());
        let query = app.query.trim().to_lowercase();
        let rows=notes.iter().filter(|n|query.is_empty()||format!("{}\n{}",n.title,n.body).to_lowercase().contains(&query)).map(|n|{let on=note.as_ref().is_some_and(|x|x.slug==n.slug);let date=note_date(app,&n.updated);let mut colors=t.clone();if on{colors["text"]=json!("#fff");colors["textDim"]=json!("rgba(255,255,255,0.75)");}click(v(&n.slug,json!({"flexDirection":"row","gap":10,"paddingVertical":9,"paddingHorizontal":10,"borderRadius":8,"marginBottom":2,"backgroundColor":if on{t["accent"].clone()}else{json!("transparent")}}),vec![dot(&n.color,false,8.),v("summary",json!({"flex":1,"gap":2}),vec![label(if n.title.is_empty(){key("notes.untitled")}else{json!(n.title)},&colors,false),label(json!(format!("{date} {}",if n.desktop{app.i18n.caption(&key("notes.onDesktop"))}else{store::preview(&n.body)})),&colors,true)])]),json!(["select",n.slug]))}).collect::<Vec<_>>();
        children.push(v("sidebar",json!({"width":side,"backgroundColor":t["sidebarBg"],"paddingTop":12,"paddingHorizontal":10,"gap":10,"borderEndWidth":1,"borderEndColor":t["separator"]}),vec![row("search",vec![input("query",&app.query,key("notes.search")),button(json!("✎"),"new",Value::Null,false)]),json!({"kind":"scroll","key":"notes-list","style":{"flex":1},"children":rows})]));
    }
    children.push(content);
    let error = store::with(&app.home, |s| s.error.clone());
    if !app.error.is_empty() || !error.is_empty() {
        children.push(v("error",json!({"position":"absolute","left":8,"bottom":8,"padding":6,"backgroundColor":t["windowBg"]}),vec![text(json!(if app.error.is_empty(){error}else{app.error.clone()}),json!({"fontSize":12,"color":"#e5534b"}))]));
    }
    if app.text_input() {
        if let Some(ime) = composition(app) {
            children.push(ime);
        }
    }
    if !app.panel.is_empty() {
        children.push(panel(app, note.as_ref(), &t));
    }
    let mut root = v(
        "notes",
        json!({"flex":1,"flexDirection":"row","direction":if app.rtl{"rtl"}else{"ltr"},"overflow":"hidden","borderRadius":if app.widget{4}else{0}}),
        children,
    );
    if app.widget {
        root["events"]["onMouseEnter"] = json!(["hover", "on"]);
        root["events"]["onMouseLeave"] = json!(["hover", "off"]);
    }
    root
}

fn direction(body: &str) -> &'static str {
    for c in body.chars() {
        if c.is_alphabetic() {
            return if ('\u{0590}'..='\u{08ff}').contains(&c)
                || ('\u{fb1d}'..='\u{fdff}').contains(&c)
                || ('\u{fe70}'..='\u{feff}').contains(&c)
            {
                "rtl"
            } else {
                "ltr"
            };
        }
    }
    "ltr"
}
fn note_date(app: &mut App, iso: &str) -> String {
    let Ok(iso) = std::ffi::CString::new(iso) else {
        return String::new();
    };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::strptime(iso.as_ptr(), c"%Y-%m-%dT%H:%M:%S".as_ptr(), &mut tm) }.is_null() {
        return String::new();
    }
    let timestamp = unsafe { libc::timegm(&mut tm) };
    unsafe {
        libc::localtime_r(&timestamp, &mut tm);
    }
    crate::native_view::date_format::date(&mut app.i18n, &app.region, &tm, "short")
}
fn composition(app: &App) -> Option<Value> {
    let st = &app.composition;
    let preedit = st["preedit"].as_str().unwrap_or("");
    let candidates = st["candidates"].as_array();
    if preedit.is_empty() && candidates.is_none_or(|c| c.is_empty()) {
        return None;
    }
    let selected = st["selected"].as_u64().unwrap_or(0) as usize;
    let mut children = vec![];
    if !preedit.is_empty() {
        children.push(text(json!(preedit),json!({"fontSize":15,"color":"#222","borderBottomWidth":1,"borderBottomColor":"#0a5fd0"})));
    }
    if let Some(list) = candidates {
        let first = selected / 9 * 9;
        children.push(row("ime-candidates",list.iter().enumerate().skip(first).take(9).map(|(i,c)|text(json!(format!("{} {}",i-first+1,c.as_str().unwrap_or(""))),json!({"fontSize":14,"padding":4,"borderRadius":4,"backgroundColor":if i==selected{"#0a5fd0"}else{"#fff"},"color":if i==selected{"#fff"}else{"#222"}}))).collect()));
    }
    Some(v(
        "composition",
        json!({"position":"absolute","left":(app.editor.x+app.editor.padding+app.editor.caret_x(app.editor.b)).min((app.width-280.).max(0.)),"top":(app.editor.y+app.editor.padding+(app.editor.row_of(app.editor.b)+1)as f64*app.editor.lh()-app.editor.scroll).clamp(0.,(app.height-90.).max(0.)),"maxWidth":app.width,"padding":6,"borderRadius":6,"backgroundColor":"#fff","borderWidth":1,"borderColor":"#aaa","pointerEvents":"none"}),
        children,
    ))
}
