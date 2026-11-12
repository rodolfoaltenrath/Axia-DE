//! Axia Store widget trees: a translucent sidebar with search and sections, and a
//! content area with a large title, editorial cards and grids of app cards.
//! Only the sidebar glyphs and the editorial artwork are rasters (`&mut App`, to
//! cache them); everything else reads the store's state.
use super::{
    art::{self, Glyph},
    catalog::{Entry, Group, Icon, Source, CATEGORIES},
    App, Page,
};
use crate::native_view::paint::{rgba, Image};
use serde_json::{json, Value};
use std::sync::Arc;
pub const SIDEBAR: f64 = 220.;
const PAD: f64 = 32.;
const GAP: f64 = 20.;
/// Below this content width the editorial cards stack instead of sharing a row.
const STACK: f64 = 560.;
/// App icon sizes: cards and the app page.
const CARD_ICON: f64 = 56.;
const PAGE_ICON: f64 = 112.;
#[derive(Clone, Copy)]
struct Palette {
    main: &'static str,
    sidebar: &'static str,
    selected: &'static str,
    text: &'static str,
    dim: &'static str,
    line: &'static str,
    pill: &'static str,
    field: &'static str,
    card: &'static str,
}
fn palette(context: &Value) -> Palette {
    if context["theme"]["mode"] == "light" {
        Palette {
            main: "#ffffff",
            sidebar: "#ececf0e0",
            selected: "#0000001a",
            text: "#1d1d1f",
            dim: "#6e6e73",
            line: "#e5e5ea",
            pill: "#ececf1",
            field: "#0000000f",
            card: "#f5f5f7",
        }
    } else {
        Palette {
            main: "#1e1e21",
            sidebar: "#2a2a2fe0",
            selected: "#ffffff1f",
            text: "#f5f5f7",
            dim: "#98989f",
            line: "#38383d",
            pill: "#3a3a3f",
            field: "#ffffff14",
            card: "#2a2a2e",
        }
    }
}
/// What every page needs to draw: colours, the accent and the content width.
struct Ui {
    p: Palette,
    accent: String,
    width: f64,
}
fn accent(context: &Value) -> String {
    context["theme"]["accent"]
        .as_str()
        .filter(|a| a.len() == 7 && a.starts_with('#'))
        .unwrap_or("#0a84ff")
        .to_owned()
}
fn key(k: &str) -> Value {
    json!({"key": format!("store.{k}")})
}
fn keyp(k: &str, params: Value) -> Value {
    json!({"key": format!("store.{k}"), "params": params})
}
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn row(style: Value, children: Vec<Value>) -> Value {
    let mut v = view(style, children);
    v["style"]["flexDirection"] = json!("row");
    v
}
/// One line, cut with an ellipsis.
fn text(label: Value, size: f64, weight: f64, color: &str) -> Value {
    json!({"kind":"text","label":label,"style":{"fontSize":size,"fontWeight":weight,"color":color,
        "textAlign":"start","whiteSpace":"nowrap","textOverflow":"ellipsis","overflow":"hidden"}})
}
/// Wrapping text (descriptions).
fn para(label: Value, size: f64, color: &str) -> Value {
    json!({"kind":"text","label":label,"style":{"fontSize":size,"color":color,"textAlign":"start","lineHeight":1.45}})
}
fn click(mut v: Value, action: Value) -> Value {
    v["events"] = json!({ "onClick": action });
    v["style"]["cursor"] = json!("pointer");
    v
}
fn spacer() -> Value {
    view(json!({"flex":1}), vec![])
}
/// A cached raster shown at `w`×`h` points.
fn raster(app: &mut App, id: String, w: f64, h: f64, draw: impl FnOnce() -> Image) -> Value {
    if !app.images.contains_key(&id) {
        if app.images.len() > 96 {
            app.images.clear();
        }
        app.images.insert(id.clone(), Arc::new(draw()));
    }
    // A constant generation: each id's pixels never change, the host copies them once.
    json!({"kind":"raster","image":id,"generation":1,"style":{"width":w,"height":h}})
}
fn glyph(app: &mut App, g: Glyph, size: f64, color: &str) -> Value {
    let px = (size * 2.).round() as usize;
    let id = format!("glyph-{}-{px}-{}", g.id(), color.trim_start_matches('#'));
    let c = rgba(color);
    raster(app, id, size, size, || art::glyph(g, px, c))
}
/// Icon trees of the apps that draw their own, made once per size.
fn cache_icons(app: &mut App) {
    let ids: Vec<String> = app
        .entries
        .iter()
        .filter_map(|e| match &e.icon {
            Icon::Compiled(id) => Some(id.clone()),
            _ => None,
        })
        .collect();
    for id in ids {
        for size in [CARD_ICON, PAGE_ICON] {
            let k = (id.clone(), size as u32);
            if !app.icons.contains_key(&k) {
                let tree = crate::native_view::icon_tree(&id, size).unwrap_or(Value::Null);
                app.icons.insert(k, tree);
            }
        }
    }
}
/// An app's icon: its PNG, its own drawing or its initial.
fn app_icon(app: &App, entry: &Entry, size: f64) -> Value {
    match &entry.icon {
        Icon::File(path) => json!({"kind":"image","src":path,
            "style":{"width":size,"height":size,"objectFit":"contain"}}),
        Icon::Compiled(id) => app
            .icons
            .get(&(id.clone(), size as u32))
            .cloned()
            .unwrap_or(Value::Null),
        Icon::Letter => {
            let (light, dark) = art::letter_colors(&entry.name);
            let letter: String = entry
                .name
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_else(|| "?".into());
            view(
                json!({"width":size,"height":size,"borderRadius":size*0.225,"alignItems":"center",
                    "justifyContent":"center","background":format!("linear-gradient(180deg, {light}, {dark})")}),
                vec![json!({"kind":"text","label":letter,
                    "style":{"fontSize":size*0.5,"fontWeight":700,"color":"#ffffff"}})],
            )
        }
    }
}
/// The pill on an app (Open for what is installed).
fn pill(ui: &Ui, label: &str, action: Value) -> Value {
    click(
        row(
            json!({"height":22,"paddingLeft":14,"paddingRight":14,"borderRadius":11,"backgroundColor":ui.p.pill,
                "alignItems":"center","justifyContent":"center","alignSelf":"flex-start"}),
            vec![text(key(label), 11., 700., &ui.accent)],
        ),
        action,
    )
}
fn sidebar(app: &mut App, ui: &Ui) -> Value {
    let p = ui.p;
    let search = row(
        json!({"height":28,"borderRadius":7,"backgroundColor":p.field,"alignItems":"center","gap":6,
            "paddingLeft":8,"paddingRight":4,"marginBottom":10}),
        vec![
            glyph(app, Glyph::Search, 14., p.dim),
            json!({"kind":"input","value":app.query,"placeholder":key("search"),
                "style":{"flex":1,"height":26,"paddingLeft":2,"paddingRight":2,"backgroundColor":"transparent",
                    "borderWidth":0,"borderRadius":0}}),
        ],
    );
    let mut items = vec![search];
    let current = app.page.nav();
    let updates = app.updates().len();
    for g in Glyph::NAV {
        let id = g.id();
        let on = current == id;
        let mut cells = vec![
            glyph(app, g, 18., &ui.accent),
            text(
                key(&format!("nav.{id}")),
                13.,
                if on { 600. } else { 500. },
                p.text,
            ),
            spacer(),
        ];
        if g == Glyph::Updates && updates > 0 {
            cells.push(row(
                json!({"minWidth":20,"height":18,"paddingLeft":6,"paddingRight":6,"borderRadius":9,
                    "backgroundColor":p.pill,"alignItems":"center","justifyContent":"center"}),
                vec![text(json!(updates.to_string()), 11., 600., p.text)],
            ));
        }
        items.push(click(
            row(
                json!({"height":30,"paddingLeft":10,"paddingRight":8,"gap":10,"borderRadius":7,"alignItems":"center",
                    "backgroundColor": if on {p.selected} else {"#00000000"}}),
                cells,
            ),
            json!(["page", id]),
        ));
    }
    // Translucent over what is behind the window, where the window lets it through.
    view(
        json!({"width":SIDEBAR,"flexShrink":0,"backgroundColor":p.sidebar,"backdropFilter":"blur(30px)",
            "paddingTop":14,"paddingBottom":14,"paddingLeft":12,"paddingRight":12,"gap":2,
            "borderEndWidth":1,"borderEndColor":p.line}),
        items,
    )
}
fn title(label: Value, ui: &Ui) -> Value {
    text(label, 30., 700., ui.p.text)
}
fn back(app: &App, ui: &Ui) -> Option<Value> {
    (!app.history.is_empty()).then(|| {
        click(
            row(
                json!({"alignItems":"center","alignSelf":"flex-start"}),
                vec![text(key("back"), 13., 500., &ui.accent)],
            ),
            json!(["back", ""]),
        )
    })
}
/// An editorial card: a blue uppercase tag, a title and subtitle, then the artwork.
fn hero(app: &mut App, ui: &Ui, story: &str, target: Option<&str>, width: f64) -> Value {
    let height = (width * 0.56).round();
    let seed = art::hash(story);
    // Drawn at twice the size, in steps of 64 points: resizing the window reuses the
    // artwork until the next step instead of drawing it again at every width.
    let pw = ((width / 64.).ceil() * 128.) as usize;
    let ph = (pw as f64 * 0.56).round() as usize;
    let id = format!("hero-{seed:x}-{pw}x{ph}");
    let artwork = raster(app, id, width, height, || art::hero(seed, pw, ph));
    let card = view(
        json!({"gap":3,"flex":1,"flexBasis":0,"minWidth":0}),
        vec![
            text(key(&format!("story.{story}.tag")), 11., 700., &ui.accent),
            text(key(&format!("story.{story}.title")), 19., 600., ui.p.text),
            text(key(&format!("story.{story}.subtitle")), 15., 400., ui.p.dim),
            view(
                json!({"marginTop":8,"borderRadius":10,"overflow":"hidden","width":width,"height":height}),
                vec![artwork],
            ),
        ],
    );
    match target.and_then(|t| app.entry(t)).map(|e| e.id.clone()) {
        Some(id) => click(card, json!(["app", id])),
        None => card,
    }
}
fn heroes(app: &mut App, ui: &Ui, stories: &[(&str, Option<&str>)]) -> Value {
    if ui.width < STACK {
        let cards = stories
            .iter()
            .map(|(s, t)| hero(app, ui, s, *t, ui.width))
            .collect();
        return view(json!({"gap":GAP}), cards);
    }
    let w = ((ui.width - GAP) / 2.).floor();
    let cards = stories
        .iter()
        .map(|(s, t)| hero(app, ui, s, *t, w))
        .collect();
    row(json!({"gap":GAP}), cards)
}
fn section_header(label: Value, ui: &Ui, more: Option<Value>) -> Value {
    let mut cells = vec![text(label, 19., 700., ui.p.text), spacer()];
    if let Some(action) = more {
        cells.push(click(
            text(key("seeAllShort"), 13., 500., &ui.accent),
            action,
        ));
    }
    row(
        json!({"alignItems":"flex-end","paddingBottom":8,"borderBottomWidth":1,"borderBottomColor":ui.p.line}),
        cells,
    )
}
fn summary(entry: &Entry) -> Value {
    if entry.summary.is_empty() {
        key(&format!("source.{}", source_id(entry.source)))
    } else {
        json!(entry.summary)
    }
}
fn app_card(app: &App, ui: &Ui, entry: &Entry) -> Value {
    let body = view(
        json!({"flex":1,"minWidth":0,"gap":2}),
        vec![
            text(json!(entry.name), 13., 600., ui.p.text),
            text(summary(entry), 12., 400., ui.p.dim),
            view(
                json!({"marginTop":6}),
                vec![pill(ui, "pill.open", json!(["open", entry.id]))],
            ),
        ],
    );
    click(
        row(
            json!({"gap":12,"alignItems":"center","height":76,"minWidth":0}),
            vec![app_icon(app, entry, CARD_ICON), body],
        ),
        json!(["app", entry.id]),
    )
}
fn columns(width: f64) -> usize {
    if width >= 760. {
        3
    } else if width >= 460. {
        2
    } else {
        1
    }
}
/// Equal columns row by row (cells keep their width when a row is not full).
fn grid(cells: Vec<Value>, columns: usize, gap: f64) -> Value {
    let mut rows = Vec::new();
    let mut cells = cells.into_iter().peekable();
    while cells.peek().is_some() {
        let line = (0..columns.max(1))
            .map(|_| {
                let cell = cells.next().unwrap_or_else(|| view(json!({}), vec![]));
                view(json!({"flex":1,"flexBasis":0,"minWidth":0}), vec![cell])
            })
            .collect();
        rows.push(row(json!({"gap":gap}), line));
    }
    view(json!({"gap":12}), rows)
}
fn apps_grid<'a>(app: &App, ui: &Ui, list: impl Iterator<Item = &'a Entry>) -> Value {
    grid(
        list.map(|e| app_card(app, ui, e)).collect(),
        columns(ui.width),
        GAP,
    )
}
/// A titled grid of apps, or nothing when the list is empty.
fn section<'a>(
    app: &App,
    ui: &Ui,
    label: Value,
    more: Option<Value>,
    list: impl Iterator<Item = &'a Entry>,
) -> Option<Value> {
    let list: Vec<&Entry> = list.collect();
    (!list.is_empty()).then(|| {
        view(
            json!({"gap":14}),
            vec![
                section_header(label, ui, more),
                apps_grid(app, ui, list.into_iter()),
            ],
        )
    })
}
fn empty(ui: &Ui, title_key: &str, detail_key: &str) -> Value {
    view(
        json!({"alignItems":"center","justifyContent":"center","gap":6,"paddingTop":60,"paddingBottom":60}),
        vec![
            text(key(title_key), 17., 600., ui.p.text),
            para(key(detail_key), 13., ui.p.dim),
        ],
    )
}
fn source_id(s: Source) -> &'static str {
    match s {
        Source::System => "system",
        Source::Package => "package",
        Source::Linux => "linux",
    }
}
/// The editorial cards of a page and the app each one opens.
fn stories(page: &Page) -> [(&'static str, Option<&'static str>); 2] {
    match page {
        Page::Group(Group::Create) => [("create1", None), ("create2", None)],
        Page::Group(Group::Work) => [("work1", None), ("work2", Some("monitor"))],
        Page::Group(Group::Play) => [("play1", None), ("play2", None)],
        Page::Group(Group::Develop) => [("develop1", Some("terminal")), ("develop2", None)],
        _ => [("welcome", Some("store")), ("monitor", Some("monitor"))],
    }
}
fn discover(app: &mut App, ui: &Ui) -> Vec<Value> {
    let mut out = vec![
        title(key("nav.discover"), ui),
        heroes(app, ui, &stories(&Page::Discover)),
    ];
    let app = &*app;
    out.extend(section(
        app,
        ui,
        key("section.essentials"),
        Some(json!(["page", "installed"])),
        app.entries
            .iter()
            .filter(|e| e.source == Source::System)
            .take(6),
    ));
    for g in Group::ALL {
        out.extend(section(
            app,
            ui,
            key(&format!("section.{}", g.id())),
            Some(json!(["page", g.id()])),
            app.entries.iter().filter(|e| e.in_group(g)).take(6),
        ));
    }
    out
}
fn group(app: &mut App, ui: &Ui, g: Group) -> Vec<Value> {
    let mut out = vec![
        title(key(&format!("nav.{}", g.id())), ui),
        heroes(app, ui, &stories(&Page::Group(g))),
        section_header(key("section.all"), ui, None),
    ];
    let app = &*app;
    let mut list = app.entries.iter().filter(|e| e.in_group(g)).peekable();
    out.push(if list.peek().is_none() {
        empty(ui, "empty.title", "empty.catalog")
    } else {
        apps_grid(app, ui, list)
    });
    out
}
fn categories(app: &App, ui: &Ui) -> Vec<Value> {
    let tiles = CATEGORIES
        .iter()
        .map(|(id, _, color)| {
            let count = app.entries.iter().filter(|e| e.in_category(id)).count();
            click(
                view(
                    json!({"height":96,"borderRadius":12,"backgroundColor":color,"padding":16,
                        "justifyContent":"flex-end","gap":2}),
                    vec![
                        text(key(&format!("category.{id}")), 17., 700., "#ffffff"),
                        text(keyp("count", json!({"count":count})), 12., 500., "#ffffffcc"),
                    ],
                ),
                json!(["category", id]),
            )
        })
        .collect();
    vec![
        title(key("nav.categories"), ui),
        grid(tiles, columns(ui.width), 16.),
    ]
}
fn category(app: &App, ui: &Ui, id: &str) -> Vec<Value> {
    let mut out: Vec<Value> = back(app, ui).into_iter().collect();
    out.push(title(key(&format!("category.{id}")), ui));
    let mut list = app.entries.iter().filter(|e| e.in_category(id)).peekable();
    out.push(if list.peek().is_none() {
        empty(ui, "empty.title", "empty.catalog")
    } else {
        apps_grid(app, ui, list)
    });
    out
}
fn search(app: &App, ui: &Ui) -> Vec<Value> {
    let query = app.query.trim().to_lowercase();
    let mut list = app.entries.iter().filter(|e| e.matches(&query)).peekable();
    vec![
        title(keyp("results", json!({"query": app.query.trim()})), ui),
        if list.peek().is_none() {
            empty(ui, "empty.search", "empty.searchDetail")
        } else {
            apps_grid(app, ui, list)
        },
    ]
}
fn updates(app: &App, ui: &Ui) -> Vec<Value> {
    let mut out = vec![title(key("nav.updates"), ui)];
    if app.updates().is_empty() {
        out.push(empty(ui, "updates.none", "updates.noneDetail"));
    }
    out
}
fn notice(app: &App, ui: &Ui) -> Option<Value> {
    let (ok, caption) = app.message.as_ref()?;
    Some(row(
        json!({"alignItems":"center","gap":10,"padding":12,"borderRadius":10,
            "backgroundColor": if *ok {"#30d15826"} else {"#ff453a26"}}),
        vec![
            view(
                json!({"flex":1,"minWidth":0}),
                vec![para(caption.clone(), 13., ui.p.text)],
            ),
            click(
                text(key("dismiss"), 12., 600., &ui.accent),
                json!(["dismiss", ""]),
            ),
        ],
    ))
}
fn trust(sig: &Value) -> Value {
    let (color, k) = match sig["trust"].as_str().unwrap_or("unsigned") {
        "lordos" => ("#30d158", "trust.axia"),
        "verified" => ("#0a84ff", "trust.verified"),
        "signed" => ("#8e8e93", "trust.signed"),
        _ => ("#ff9f0a", "trust.unsigned"),
    };
    row(
        json!({"alignItems":"center","gap":6}),
        vec![
            view(
                json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":color}),
                vec![],
            ),
            text(keyp(k, json!({"name": sig["signer"]})), 11., 500., color),
        ],
    )
}
fn button(ui: &Ui, label: &str, action: Value, primary: bool) -> Value {
    let (bg, fg) = if primary {
        (ui.accent.as_str(), "#ffffff")
    } else {
        (ui.p.pill, ui.p.text)
    };
    click(
        row(
            json!({"height":26,"paddingLeft":14,"paddingRight":14,"borderRadius":7,"backgroundColor":bg,
                "alignItems":"center","justifyContent":"center"}),
            vec![text(key(label), 12., 600., fg)],
        ),
        action,
    )
}
/// One installed Axia package: name, version, signature state and its actions.
fn package_row(app: &App, ui: &Ui, pkg: &Value) -> Value {
    let id = pkg["id"].as_str().unwrap_or("");
    let m = &pkg["manifest"];
    let name = super::catalog_text(&m["name"], &app.lang).unwrap_or_else(|| id.to_owned());
    let st = app.statuses.get(id).unwrap_or(&Value::Null);
    let mut info = vec![
        text(json!(name), 13., 600., ui.p.text),
        text(
            keyp("version", json!({"version": m["version"]})),
            12.,
            400.,
            ui.p.dim,
        ),
    ];
    if st["installed"] == true {
        info.push(if st["hasOriginal"] == true {
            trust(st)
        } else {
            text(key("manage.noOriginal"), 11., 400., ui.p.dim)
        });
    }
    let modified = st["modified"] == true;
    if modified {
        info.push(text(
            keyp("manage.modified", json!({"count": st["changedCount"]})),
            12.,
            600.,
            "#ff9f0a",
        ));
    }
    let mut actions = vec![];
    if modified {
        if st["hasOriginal"] == true {
            actions.push(button(ui, "manage.restore", json!(["restore", id]), false));
        }
        actions.push(button(ui, "manage.share", json!(["share", id]), false));
    }
    actions.push(button(ui, "remove", json!(["remove", id]), false));
    if app.entry(id).is_some() {
        actions.push(button(ui, "open", json!(["open", id]), true));
    }
    row(
        json!({"alignItems":"center","gap":12,"paddingTop":8,"paddingBottom":8}),
        vec![
            view(json!({"flex":1,"minWidth":0,"gap":3}), info),
            row(json!({"gap":8}), actions),
        ],
    )
}
fn installed(app: &App, ui: &Ui) -> Vec<Value> {
    let mut out = vec![title(key("nav.installed"), ui)];
    out.extend(notice(app, ui));
    if !app.packages.is_empty() {
        let mut rows = vec![section_header(key("section.packages"), ui, None)];
        rows.extend(app.packages.iter().map(|pkg| package_row(app, ui, pkg)));
        out.push(view(json!({"gap":4}), rows));
    }
    if !app.files.is_empty() {
        let mut rows = vec![section_header(key("section.files"), ui, None)];
        for f in &app.files {
            rows.push(row(
                json!({"alignItems":"center","gap":12,"paddingTop":8,"paddingBottom":8}),
                vec![
                    view(
                        json!({"flex":1,"minWidth":0,"gap":2}),
                        vec![
                            text(json!(f.name), 13., 600., ui.p.text),
                            text(
                                json!(format!("{} · {}", tilde(app, &f.path), size(f.size))),
                                12.,
                                400.,
                                ui.p.dim,
                            ),
                        ],
                    ),
                    button(ui, "install", json!(["file", f.path]), true),
                ],
            ));
        }
        out.push(view(json!({"gap":4}), rows));
    }
    out.extend(section(
        app,
        ui,
        keyp("section.allCount", json!({"count": app.entries.len()})),
        None,
        app.entries.iter(),
    ));
    out
}
fn tilde(app: &App, path: &str) -> String {
    match app
        .home
        .as_ref()
        .and_then(|h| path.strip_prefix(&*h.to_string_lossy()))
    {
        Some(rest) => format!("~{rest}"),
        None => path.to_owned(),
    }
}
fn size(bytes: u64) -> String {
    let mut v = bytes as f64;
    let mut unit = 0;
    let units = ["B", "KB", "MB", "GB"];
    while v >= 1000. && unit + 1 < units.len() {
        v /= 1000.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", units[unit])
    }
}
fn detail(app: &App, ui: &Ui, id: &str) -> Vec<Value> {
    let Some(entry) = app.entry(id) else {
        return vec![empty(ui, "empty.title", "empty.catalog")];
    };
    let mut out: Vec<Value> = back(app, ui).into_iter().collect();
    out.push(row(
        json!({"gap":20,"alignItems":"center"}),
        vec![
            app_icon(app, entry, PAGE_ICON),
            view(
                json!({"flex":1,"minWidth":0,"gap":4}),
                vec![
                    text(json!(entry.name), 26., 700., ui.p.text),
                    text(summary(entry), 15., 400., ui.p.dim),
                    view(
                        json!({"marginTop":10}),
                        vec![pill(ui, "pill.open", json!(["open", entry.id]))],
                    ),
                ],
            ),
        ],
    ));
    let category = CATEGORIES
        .iter()
        .find(|c| entry.in_category(c.0))
        .map(|c| key(&format!("category.{}", c.0)))
        .unwrap_or_else(|| json!("—"));
    let fact = |label: &str, value: Value| {
        view(
            json!({"gap":3,"flex":1,"flexBasis":0,"minWidth":0}),
            vec![
                text(key(label), 11., 600., ui.p.dim),
                text(value, 13., 500., ui.p.text),
            ],
        )
    };
    let version = if entry.version.is_empty() {
        json!("—")
    } else {
        json!(entry.version)
    };
    out.push(row(
        json!({"gap":GAP,"paddingTop":14,"paddingBottom":14,"borderTopWidth":1,"borderBottomWidth":1,
            "borderTopColor":ui.p.line,"borderBottomColor":ui.p.line}),
        vec![
            fact("fact.category", category),
            fact("fact.source", key(&format!("source.{}", source_id(entry.source)))),
            fact("fact.version", version),
        ],
    ));
    out
}
fn file(app: &App, ui: &Ui, path: &str) -> Vec<Value> {
    let mut out: Vec<Value> = back(app, ui).into_iter().collect();
    out.push(title(key("file.title"), ui));
    out.push(text(json!(tilde(app, path)), 12., 400., ui.p.dim));
    out.extend(notice(app, ui));
    let mut card = vec![];
    match &app.inspect {
        None => card.push(text(key("working"), 13., 400., ui.p.dim)),
        Some(info) if !info["error"].is_null() => card.push(para(
            keyp("file.invalid", json!({"error": {"error": info["error"]}})),
            13.,
            ui.p.text,
        )),
        Some(info) => {
            let m = &info["manifest"];
            let id = m["id"].as_str().unwrap_or("");
            let name = super::catalog_text(&m["name"], &app.lang).unwrap_or_default();
            card.push(text(
                json!(format!("{name} {}", m["version"].as_str().unwrap_or(""))),
                17.,
                600.,
                ui.p.text,
            ));
            if let Some(d) = super::catalog_text(&m["description"], &app.lang) {
                card.push(para(json!(d), 13., ui.p.text));
            }
            card.push(text(
                keyp(
                    "file.size",
                    json!({"size": size(info["size"].as_f64().unwrap_or(0.) as u64), "files": info["files"]}),
                ),
                12.,
                400.,
                ui.p.dim,
            ));
            let perms: Vec<&str> = m["permissions"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let list = if perms.is_empty() {
                key("file.noPermissions")
            } else {
                json!(perms.join(", "))
            };
            card.push(text(
                keyp("file.permissions", json!({"list": list})),
                12.,
                400.,
                ui.p.dim,
            ));
            card.push(trust(&info["signature"]));
            let done = app.message.as_ref().is_some_and(|m| m.0);
            if let Some(old) = app.packages.iter().find(|pkg| pkg["id"] == id).filter(|_| !done) {
                card.push(text(
                    keyp("file.replaces", json!({"version": old["manifest"]["version"]})),
                    12.,
                    400.,
                    ui.p.dim,
                ));
            }
            let actions = if !done {
                vec![
                    button(ui, "cancel", json!(["back", ""]), false),
                    button(ui, "install", json!(["install", path]), true),
                ]
            } else if app.entry(id).is_some() {
                // Installed, and the shell already lists it: it can be opened from here.
                vec![button(ui, "open", json!(["open", id]), true)]
            } else {
                vec![]
            };
            if !actions.is_empty() {
                card.push(row(
                    json!({"gap":8,"justifyContent":"flex-end","marginTop":6}),
                    actions,
                ));
            }
        }
    }
    out.push(view(
        json!({"gap":8,"padding":20,"borderRadius":12,"backgroundColor":ui.p.card,
            "borderWidth":1,"borderColor":ui.p.line}),
        card,
    ));
    out
}
pub fn tree(app: &mut App, context: &Value) -> Value {
    let width = context["width"].as_f64().unwrap_or(1080.);
    let ui = Ui {
        p: palette(context),
        accent: accent(context),
        width: (width - SIDEBAR - PAD * 2.).max(280.),
    };
    cache_icons(app);
    let body = match app.page.clone() {
        Page::Discover => discover(app, &ui),
        Page::Group(g) => group(app, &ui, g),
        Page::Categories => categories(app, &ui),
        Page::Category(id) => category(app, &ui, &id),
        Page::Updates => updates(app, &ui),
        Page::Installed => installed(app, &ui),
        Page::Search => search(app, &ui),
        Page::App(id) => detail(app, &ui, &id),
        Page::File(path) => file(app, &ui, &path),
    };
    let main = json!({"kind":"scroll","style":{"flex":1,"backgroundColor":ui.p.main},"children":[
        view(json!({"paddingTop":28,"paddingBottom":40,"paddingLeft":PAD,"paddingRight":PAD,"gap":26}), body)
    ]});
    let sidebar = sidebar(app, &ui);
    row(
        json!({"flex":1,"backgroundColor":ui.p.main}),
        vec![sidebar, main],
    )
}
