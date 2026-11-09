//! Window layout and the dashboard: sidebar, header and the summary cards.
//! The window frame (title bar and its buttons) belongs to the window manager.
use super::{
    arr, charts,
    paint::{self, Icon, Ring},
    theme::{self, Palette},
    ui::{bytes, key, n, pct, rate, text, view},
    worker, App, Tab,
};
use serde_json::{json, Value};
use std::sync::Arc;
pub const SIDEBAR: f64 = 208.;
const GAP: f64 = 16.;
const PAD: f64 = 24.;
fn label(k: Value, size: f64, weight: f64, color: &str) -> Value {
    let mut t = text(k);
    t["style"] = json!({"fontSize":size,"fontWeight":weight,"color":color,"textAlign":"start","whiteSpace":"nowrap","textOverflow":"ellipsis","overflow":"hidden"});
    t
}
fn plain(s: impl Into<String>) -> Value {
    json!(s.into())
}
fn row(style: Value, children: Vec<Value>) -> Value {
    let mut v = view(style, children);
    v["style"]["flexDirection"] = json!("row");
    v
}
fn spacer() -> Value {
    view(json!({"flex":1}), vec![])
}
fn dot(color: &str) -> Value {
    view(
        json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":color}),
        vec![],
    )
}
fn link(accent: &str, k: &str, action: Value) -> Value {
    let mut v = row(
        json!({"paddingTop":4,"cursor":"pointer","alignSelf":"flex-start"}),
        vec![label(key(k), 12., 500., accent)],
    );
    v["events"] = json!({ "onClick": action });
    v
}
fn rgba_hex(c: [u8; 4]) -> String {
    format!("{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
}
/// A line icon at `size` points, drawn at twice the size and cached by colour.
pub fn icon(app: &mut App, icon: Icon, size: f64, color: [u8; 4]) -> Value {
    let px = (size * 2.).round().clamp(8., 256.) as usize;
    let id = format!("icon-{}-{px}-{}", icon.id(), rgba_hex(color));
    if !app.icons.contains_key(&id) {
        if app.icons.len() >= 96 {
            app.icons.clear();
        }
        app.icons
            .insert(id.clone(), Arc::new(paint::icon(icon, px, color)));
    }
    // A constant generation: the host copies an icon's pixels once.
    json!({"kind":"raster","image":id,"generation":1,"style":{"width":size,"height":size}})
}
fn accent(context: &Value) -> String {
    context["theme"]["accent"]
        .as_str()
        .filter(|a| a.len() == 7 && a.starts_with('#'))
        .unwrap_or("#0a84ff")
        .to_owned()
}
pub fn sidebar(app: &mut App, context: &Value, voltage: bool) -> Value {
    let p = theme::palette(context);
    let tint = theme::rgba(&accent(context));
    let entries = [
        (Tab::Overview, Icon::Dashboard, "monitor.nav.dashboard"),
        (Tab::Processes, Icon::Processes, "monitor.nav.processes"),
        (Tab::Cpu, Icon::Cpu, "monitor.nav.cpu"),
        (Tab::Memory, Icon::Memory, "monitor.memory"),
        (Tab::Disks, Icon::Storage, "monitor.nav.storage"),
        (Tab::Network, Icon::Network, "monitor.network"),
        (Tab::Cooling, Icon::Cooling, "monitor.tab.cooling"),
        (Tab::Voltage, Icon::Voltage, "monitor.tab.voltage"),
    ];
    let mut items = Vec::new();
    for (tab, ic, k) in entries {
        if tab == Tab::Voltage && !voltage {
            continue;
        }
        let on = app.section.is_empty() && app.tab == tab;
        items.push(nav(app, &p, ic, tint, k, on, json!(["tab", tab.id()])));
    }
    items.push(spacer());
    let on = !app.section.is_empty();
    items.push(nav(
        app,
        &p,
        Icon::Settings,
        p.ink_dim,
        "monitor.nav.settings",
        on,
        json!(["section", "privacy"]),
    ));
    view(
        json!({"width":SIDEBAR,"flexShrink":0,"backgroundColor":p.sidebar,"paddingTop":14,"paddingBottom":14,"paddingLeft":10,"paddingRight":10,"gap":2,"borderEndWidth":1,"borderEndColor":p.line}),
        items,
    )
}
fn nav(
    app: &mut App,
    p: &Palette,
    ic: Icon,
    tint: [u8; 4],
    k: &str,
    on: bool,
    action: Value,
) -> Value {
    let mut v = row(
        json!({"alignItems":"center","gap":10,"height":32,"paddingLeft":10,"paddingRight":10,"borderRadius":7,"backgroundColor":if on {p.selected} else {"#00000000"},"cursor":"pointer"}),
        vec![
            icon(app, ic, 18., tint),
            label(key(k), 13., if on { 600. } else { 400. }, p.text),
        ],
    );
    v["events"] = json!({ "onClick": action });
    v
}
fn uptime(seconds: f64) -> Value {
    let s = seconds.max(0.) as u64;
    let (d, h, m) = (s / 86400, s % 86400 / 3600, s % 3600 / 60);
    if d > 0 {
        json!({"key":"monitor.uptimeDays","params":{"count":d,"h":h,"m":m}})
    } else {
        json!({"key":"monitor.uptimeHours","params":{"h":h,"m":m}})
    }
}
pub fn header(snapshot: &worker::Snapshot, last: &Value, context: &Value) -> Value {
    let p = theme::palette(context);
    let sys = &snapshot.system;
    let host = sys["host"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("—");
    let model = sys["model"].as_str().unwrap_or("");
    let cpu = snapshot.info["cpuModel"].as_str().unwrap_or("");
    let os = sys["os"].as_str().unwrap_or("");
    let column = |a: Value, b: Value| {
        view(
            json!({"gap":3,"minWidth":0,"flexShrink":1}),
            vec![label(a, 14., 600., p.text), label(b, 12., 400., p.dim)],
        )
    };
    row(
        json!({"alignItems":"center","gap":28,"paddingLeft":PAD,"paddingRight":PAD,"paddingTop":16,"paddingBottom":16,"borderBottomWidth":1,"borderBottomColor":p.line,"flexShrink":0}),
        vec![
            row(
                json!({"alignItems":"center","gap":14,"minWidth":0,"flexShrink":1}),
                vec![
                    json!({"kind":"image","src":"asset:icons/axia-icon-128.png","style":{"width":44,"height":44,"borderRadius":10,"flexShrink":0}}),
                    column(plain(host), uptime(n(&last["uptime"]))),
                ],
            ),
            column(
                plain(if os.is_empty() { "Axia" } else { os }),
                plain(if model.is_empty() { cpu } else { model }),
            ),
        ],
    )
}
fn card(p: &Palette, title: &str, children: Vec<Value>) -> Value {
    let mut content = vec![label(key(title), 12., 700., p.text)];
    content.extend(children);
    view(
        json!({"backgroundColor":p.card,"borderRadius":12,"padding":16,"gap":12,"shadowColor":p.shadow,"shadowBlur":if p.dark {18} else {14},"shadowOffsetY":2,"borderWidth":if p.dark {1} else {0},"borderColor":p.line,"minWidth":0}),
        content,
    )
}
fn separator(p: &Palette) -> Value {
    view(
        json!({"height":1,"backgroundColor":p.line,"alignSelf":"stretch"}),
        vec![],
    )
}
/// Label above a value, as in the figure grids of the summary.
fn figure(p: &Palette, k: &str, value: String) -> Value {
    view(
        json!({"gap":2,"minWidth":0}),
        vec![
            label(key(k), 11., 400., p.dim),
            label(plain(value), 12., 500., p.text),
        ],
    )
}
fn legend(p: &Palette, color: &str, k: &str, value: String) -> Value {
    row(
        json!({"gap":6,"alignItems":"flex-start","minWidth":0}),
        vec![
            view(json!({"paddingTop":4}), vec![dot(color)]),
            view(
                json!({"gap":1,"minWidth":0,"flexShrink":1}),
                vec![
                    label(key(k), 11., 400., p.text),
                    label(plain(value), 11., 400., p.dim),
                ],
            ),
        ],
    )
}
/// Equal columns, row by row: the cells keep their width when a row is not full.
pub(super) fn grid(cells: Vec<Value>, columns: usize, gap: f64) -> Value {
    let columns = columns.max(1);
    let mut rows = Vec::new();
    let mut cells = cells.into_iter().peekable();
    while cells.peek().is_some() {
        let mut line = Vec::new();
        for _ in 0..columns {
            let cell = cells.next().unwrap_or_else(|| view(json!({}), vec![]));
            line.push(view(
                json!({"flex":1,"flexBasis":0,"minWidth":0}),
                vec![cell],
            ));
        }
        rows.push(row(json!({"gap":gap}), line));
    }
    view(json!({"gap":8}), rows)
}
/// Memory split into kernel, apps, cache, free and the rest of the reclaimable part.
fn memory_parts(m: &Value) -> [(&'static str, &'static str, f64); 5] {
    let total = n(&m["total"]).max(1.);
    let used = n(&m["used"]);
    let kernel = n(&m["kernel"]).min(used);
    let cache = n(&m["cache"]);
    let free = n(&m["free"]);
    let other = (total - used - cache - free).max(0.);
    [
        (theme::KERNEL, "monitor.dash.kernel", kernel),
        (theme::APPS, "monitor.dash.apps", used - kernel),
        (theme::CACHE, "monitor.dash.cache", cache),
        (theme::FREE, "monitor.dash.free", free),
        (theme::OTHER, "monitor.dash.other", other),
    ]
}
fn root_fs(snapshot: &worker::Snapshot) -> Option<&Value> {
    arr(&snapshot.system["filesystems"]).first()
}
fn fs_name(fs: &Value) -> Value {
    match fs["mount"].as_str() {
        Some("/") | None => key("monitor.dash.systemDisk"),
        Some(m) => plain(m.rsplit('/').find(|s| !s.is_empty()).unwrap_or(m)),
    }
}
fn summary(app: &mut App, snapshot: &worker::Snapshot, last: &Value, p: &Palette) -> Value {
    let m = &last["mem"];
    let total = n(&m["total"]).max(1.);
    let parts = memory_parts(m);
    let cpu = n(&last["cpu"]).clamp(0., 1.);
    let disk = root_fs(snapshot)
        .map(|f| n(&f["used"]) / n(&f["total"]).max(1.))
        .unwrap_or(0.);
    // Drawn at 2x and shown at 144 points.
    let (size, w) = (288usize, 17.);
    let (r1, r2, r3) = (133., 105., 77.);
    let mut arcs = Vec::new();
    let mut at = 0.;
    for (color, _, v) in &parts[..3] {
        let end = at + v / total;
        arcs.push(Ring {
            radius: r1,
            width: w,
            start: at,
            end,
            color: theme::rgba(color),
        });
        at = end;
    }
    arcs.push(Ring {
        radius: r2,
        width: w,
        start: 0.,
        end: cpu,
        color: theme::rgba(theme::CPU),
    });
    arcs.push(Ring {
        radius: r3,
        width: w,
        start: 0.,
        end: disk,
        color: theme::rgba(theme::DISK),
    });
    let raster = paint::rings(
        size,
        &[(r1, w), (r2, w), (r3, w)],
        &arcs,
        p.ring_track,
        app.generation,
    );
    app.images.insert("summary-rings".into(), raster.into());
    let rings = json!({"kind":"raster","image":"summary-rings","generation":app.generation,"style":{"width":144,"height":144}});
    let figure_row = |app: &mut App, ic: Icon, color: Option<&str>, value: String| {
        let mut items = Vec::new();
        items.push(match color {
            Some(c) => dot(c),
            None => view(json!({"width":8}), vec![]),
        });
        items.push(icon(app, ic, 16., p.ink_dim));
        items.push(label(plain(value), 15., 500., p.text));
        row(json!({"alignItems":"center","gap":8}), items)
    };
    let left = view(
        json!({"alignItems":"center","gap":12,"width":150,"flexShrink":0}),
        vec![
            rings,
            view(
                json!({"gap":6}),
                vec![
                    figure_row(app, Icon::Memory, None, bytes(total)),
                    figure_row(app, Icon::Cpu, Some(theme::CPU), pct(cpu)),
                    figure_row(app, Icon::Storage, Some(theme::DISK), pct(disk)),
                ],
            ),
        ],
    );
    let legends: Vec<_> = parts
        .iter()
        .map(|(c, k, v)| legend(p, c, k, bytes(*v)))
        .collect();
    let mut right = vec![
        label(key("monitor.dash.memoryTitle"), 12., 600., p.text),
        grid(legends, 3, 8.),
        separator(p),
        label(
            plain(
                snapshot.info["cpuModel"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("CPU"),
            ),
            12.,
            600.,
            p.text,
        ),
        grid(
            vec![
                figure(
                    p,
                    "monitor.dash.cores",
                    n(&snapshot.info["physical"]).to_string(),
                ),
                figure(
                    p,
                    "monitor.dash.logical",
                    n(&snapshot.info["cores"]).to_string(),
                ),
            ],
            3,
            8.,
        ),
    ];
    if let Some(fs) = root_fs(snapshot) {
        right.push(separator(p));
        right.push(row(
            json!({"alignItems":"center","gap":6}),
            vec![
                icon(app, Icon::Storage, 14., p.ink_dim),
                label(fs_name(fs), 12., 600., p.text),
            ],
        ));
        right.push(grid(
            vec![
                figure(p, "monitor.dash.used", bytes(n(&fs["used"]))),
                figure(p, "monitor.dash.total", bytes(n(&fs["total"]))),
                figure(p, "monitor.dash.available", bytes(n(&fs["avail"]))),
            ],
            3,
            8.,
        ));
    }
    card(
        p,
        "monitor.dash.summary",
        vec![row(
            json!({"gap":18,"alignItems":"flex-start"}),
            vec![left, view(json!({"flex":1,"gap":8,"minWidth":0}), right)],
        )],
    )
}
/// A small history chart at the card's inner width.
fn spark(
    app: &mut App,
    id: &str,
    series: Vec<(Vec<f64>, [u8; 4])>,
    top: f64,
    width: f64,
    height: usize,
) -> Value {
    let w = width.clamp(80., 1200.) as usize;
    let top = if top > 0. {
        top
    } else {
        series
            .iter()
            .flat_map(|s| s.0.iter())
            .copied()
            .filter(|v| v.is_finite())
            .fold(1., f64::max)
            * 1.1
    };
    let raster = charts::area(w, height, &series, top, worker::SAMPLES, app.generation);
    app.images.insert(id.into(), raster.into());
    json!({"kind":"raster","image":id,"generation":app.generation,"style":{"width":"100%","height":height}})
}
fn series(snapshot: &worker::Snapshot, pick: impl Fn(&Value) -> f64) -> Vec<f64> {
    snapshot.samples.iter().map(|s| pick(s)).collect()
}
fn memory_card(
    app: &mut App,
    snapshot: &worker::Snapshot,
    last: &Value,
    p: &Palette,
    accent: &str,
    width: f64,
) -> Value {
    let m = &last["mem"];
    let total = n(&m["total"]).max(1.);
    let parts = memory_parts(m);
    let bar = row(
        json!({"height":10,"borderRadius":5,"overflow":"hidden","backgroundColor":p.track}),
        parts
            .iter()
            .filter(|(_, _, v)| *v > 0.)
            .map(|(c, _, v)| {
                view(
                    json!({"flexGrow":v / total,"flexBasis":0,"backgroundColor":c}),
                    vec![],
                )
            })
            .collect(),
    );
    let legends = grid(
        parts
            .iter()
            .map(|(c, k, v)| legend(p, c, k, bytes(*v)))
            .collect(),
        if width >= 560. { 5 } else { 3 },
        8.,
    );
    let pressure = n(&last["pressure"]["some"]["avg10"]);
    let chart = spark(
        app,
        "dash-pressure",
        vec![(
            series(snapshot, |s| n(&s["pressure"]["some"]["avg10"])),
            theme::rgba(theme::APPS),
        )],
        100.,
        width - 32.,
        40,
    );
    card(
        p,
        "monitor.dash.memory",
        vec![
            bar,
            legends,
            row(
                json!({"alignItems":"center"}),
                vec![
                    label(key("monitor.dash.pressure"), 11., 500., p.dim),
                    spacer(),
                    label(plain(format!("{pressure:.1}%")), 11., 500., p.dim),
                ],
            ),
            chart,
            link(accent, "monitor.dash.showApps", json!(["showApps", "rss"])),
        ],
    )
}
fn disks_card(app: &mut App, snapshot: &worker::Snapshot, p: &Palette) -> Option<Value> {
    let list = arr(&snapshot.system["filesystems"]);
    if list.is_empty() {
        return None;
    }
    let rows = list
        .iter()
        .map(|fs| {
            let share = (n(&fs["used"]) / n(&fs["total"]).max(1.)).clamp(0., 1.);
            row(
                json!({"alignItems":"center","gap":10}),
                vec![
                    icon(app, Icon::Storage, 16., p.ink_dim),
                    view(
                        json!({"flex":1,"minWidth":0}),
                        vec![label(fs_name(fs), 12., 500., p.text)],
                    ),
                    row(
                        json!({"width":56,"height":6,"borderRadius":3,"overflow":"hidden","backgroundColor":p.track}),
                        vec![view(
                            json!({"width":format!("{}%",share*100.),"backgroundColor":theme::CACHE}),
                            vec![],
                        )],
                    ),
                    label(
                        plain(format!("{} / {}", bytes(n(&fs["used"])), bytes(n(&fs["total"])))),
                        12.,
                        400.,
                        p.dim,
                    ),
                ],
            )
        })
        .collect();
    Some(card(p, "monitor.dash.disks", rows))
}
fn cpu_card(
    app: &mut App,
    snapshot: &worker::Snapshot,
    last: &Value,
    p: &Palette,
    accent: &str,
    width: f64,
) -> Value {
    let cores = arr(&last["cores"]);
    let tint = theme::CACHE;
    // Up to 64 bars; larger machines show the busiest ones.
    let mut values: Vec<f64> = cores.iter().map(|c| n(c).clamp(0., 1.)).collect();
    if values.len() > 64 {
        values.sort_by(|a, b| b.total_cmp(a));
        values.truncate(64);
    }
    let bars = row(
        json!({"height":44,"alignItems":"flex-end","gap":2,"flex":1,"minWidth":0}),
        values
            .iter()
            .map(|v| {
                view(
                    json!({"flex":1,"height":(v * 44.).max(2.),"backgroundColor":tint,"borderTopLeftRadius":2,"borderTopRightRadius":2}),
                    vec![],
                )
            })
            .collect(),
    );
    let half = ((width - 32. - 16.) / 2.).max(80.);
    let chart = spark(
        app,
        "dash-cpu",
        vec![(series(snapshot, |s| n(&s["cpu"])), theme::rgba(tint))],
        1.,
        half,
        44,
    );
    let column = |title: Value, body: Value| {
        view(
            json!({"flex":1,"gap":6,"minWidth":0}),
            vec![label(title, 11., 500., p.dim), body],
        )
    };
    card(
        p,
        "monitor.dash.processor",
        vec![
            row(
                json!({"gap":16}),
                vec![
                    column(key("monitor.dash.cores"), bars),
                    column(plain(pct(n(&last["cpu"]))), chart),
                ],
            ),
            link(accent, "monitor.dash.showApps", json!(["showApps", "cpu"])),
        ],
    )
}
fn network_card(
    app: &mut App,
    snapshot: &worker::Snapshot,
    last: &Value,
    p: &Palette,
    width: f64,
) -> Value {
    let net = &snapshot.system["network"];
    let primary = arr(&net["ifs"]).first().cloned().unwrap_or(Value::Null);
    let address = arr(&primary["addrs"])
        .first()
        .and_then(Value::as_str)
        .map(plain)
        .unwrap_or_else(|| key("monitor.dash.noAddress"));
    let kind = if primary.is_null() || primary["up"] != true {
        "monitor.dash.offline"
    } else if primary["wireless"] == true {
        "monitor.dash.wifi"
    } else {
        "monitor.dash.ethernet"
    };
    let chip = row(
        json!({"alignItems":"center","gap":6,"paddingLeft":8,"paddingRight":8,"height":22,"borderRadius":6,"backgroundColor":p.selected}),
        vec![
            dot(if kind == "monitor.dash.offline" {
                theme::OTHER
            } else {
                theme::FREE
            }),
            label(key(kind), 11., 500., p.text),
            label(
                plain(primary["name"].as_str().unwrap_or("")),
                11.,
                400.,
                p.dim,
            ),
        ],
    );
    let entry = |app: &mut App, ic: Icon, title: Value, sub: String| {
        row(
            json!({"alignItems":"center","gap":10,"minWidth":0}),
            vec![
                icon(app, ic, 18., p.ink_dim),
                view(
                    json!({"gap":2,"minWidth":0}),
                    vec![
                        label(title, 12., 500., p.text),
                        label(plain(sub), 10., 400., p.dim),
                    ],
                ),
            ],
        )
    };
    let local = entry(
        app,
        Icon::Laptop,
        address,
        primary["mac"].as_str().unwrap_or("").to_owned(),
    );
    let gateway = entry(
        app,
        Icon::Router,
        net["gateway"]
            .as_str()
            .map(plain)
            .unwrap_or_else(|| plain("—")),
        net["gatewayMac"].as_str().unwrap_or("").to_owned(),
    );
    let chart = spark(
        app,
        "dash-network",
        vec![
            (
                series(snapshot, |s| n(&s["net"]["rx"])),
                theme::rgba(theme::RX),
            ),
            (
                series(snapshot, |s| n(&s["net"]["tx"])),
                theme::rgba(theme::TX),
            ),
        ],
        0.,
        width - 32.,
        44,
    );
    let speed = |color: &str, arrow: &str, v: f64| {
        row(
            json!({"alignItems":"center","gap":5}),
            vec![
                dot(color),
                label(plain(format!("{arrow} {}", rate(v))), 11., 500., p.text),
            ],
        )
    };
    card(
        p,
        "monitor.dash.network",
        vec![
            row(json!({"alignItems":"center"}), vec![chip, spacer()]),
            grid(vec![local, gateway], 2, 12.),
            row(
                json!({"alignItems":"center","gap":14}),
                vec![
                    label(key("monitor.dash.speed"), 11., 500., p.dim),
                    spacer(),
                    speed(theme::RX, "↓", n(&last["net"]["rx"])),
                    speed(theme::TX, "↑", n(&last["net"]["tx"])),
                ],
            ),
            chart,
        ],
    )
}
fn battery_card(snapshot: &worker::Snapshot, p: &Palette) -> Option<Value> {
    let b = &snapshot.system["battery"];
    if b.is_null() {
        return None;
    }
    let capacity = b["capacity"].as_f64();
    let status = match b["status"].as_str().unwrap_or("") {
        "Charging" => "monitor.bat.charging",
        "Discharging" => "monitor.bat.discharging",
        "Full" => "monitor.bat.full",
        "Not charging" => "monitor.bat.notCharging",
        _ => "monitor.bat.unknown",
    };
    let health = match (b["full"].as_f64(), b["design"].as_f64()) {
        (Some(f), Some(d)) if d > 0. => Some(f / d),
        _ => None,
    };
    let (health_key, health_color) = match health {
        Some(h) if h >= 0.8 => ("monitor.bat.good", theme::FREE),
        Some(h) if h >= 0.6 => ("monitor.bat.fair", theme::APPS),
        Some(_) => ("monitor.bat.poor", theme::KERNEL),
        None => ("monitor.bat.unknown", theme::OTHER),
    };
    let unit = b["unit"].as_str().unwrap_or("mAh");
    let amount = |v: Option<f64>| match v {
        Some(v) if unit == "mAh" => format!("{v:.0} mAh"),
        Some(v) => format!("{v:.1} Wh"),
        None => "—".into(),
    };
    let level = row(
        json!({"alignItems":"center","gap":2}),
        vec![
            row(
                json!({"height":24,"paddingLeft":6,"paddingRight":6,"borderRadius":6,"borderWidth":1.5,"borderColor":p.text,"alignItems":"center"}),
                vec![label(
                    plain(
                        capacity
                            .map(|c| format!("{c:.0}%"))
                            .unwrap_or_else(|| "—".into()),
                    ),
                    12.,
                    600.,
                    p.text,
                )],
            ),
            view(
                json!({"width":3,"height":8,"borderTopRightRadius":2,"borderBottomRightRadius":2,"backgroundColor":p.text}),
                vec![],
            ),
        ],
    );
    let share = health.unwrap_or(0.).clamp(0., 1.);
    let mut body = vec![
        row(
            json!({"alignItems":"center","gap":12}),
            vec![
                level,
                label(key(status), 12., 400., p.text),
                spacer(),
                label(key(health_key), 12., 400., p.dim),
                dot(health_color),
            ],
        ),
        separator(p),
    ];
    if let Some(cycles) = b["cycles"].as_f64() {
        body.push(row(
            json!({"alignItems":"center"}),
            vec![
                label(key("monitor.bat.cycles"), 11., 400., p.dim),
                spacer(),
                label(plain(format!("{cycles:.0}")), 12., 500., p.text),
            ],
        ));
    }
    body.push(row(
        json!({"height":10,"borderRadius":5,"overflow":"hidden","backgroundColor":p.track}),
        vec![view(
            json!({"width":format!("{}%",share*100.),"backgroundColor":theme::FREE}),
            vec![],
        )],
    ));
    body.push(row(
        json!({"alignItems":"flex-start","gap":12}),
        vec![
            label(key("monitor.bat.capacity"), 11., 400., p.dim),
            spacer(),
            figure(p, "monitor.bat.current", amount(b["full"].as_f64())),
            figure(p, "monitor.bat.maximum", amount(b["design"].as_f64())),
        ],
    ));
    Some(card(p, "monitor.dash.battery", body))
}
/// The dashboard cards in one or two columns, depending on the window width.
pub fn view_cards(
    app: &mut App,
    snapshot: &worker::Snapshot,
    last: &Value,
    context: &Value,
) -> Vec<Value> {
    let p = theme::palette(context);
    let accent = accent(context);
    let width = context["width"].as_f64().unwrap_or(1180.);
    let content = (width - SIDEBAR - PAD * 2.).max(320.);
    let two = content >= 720.;
    let column = if two { (content - GAP) / 2. } else { content };
    let summary = summary(app, snapshot, last, &p);
    let memory = memory_card(app, snapshot, last, &p, &accent, column);
    let disks = disks_card(app, snapshot, &p);
    let cpu = cpu_card(app, snapshot, last, &p, &accent, column);
    let network = network_card(app, snapshot, last, &p, column);
    let battery = battery_card(snapshot, &p);
    if !two {
        let mut all = vec![summary, memory, cpu];
        all.extend(disks);
        all.push(network);
        all.extend(battery);
        return all;
    }
    let mut left = vec![summary];
    left.extend(battery);
    left.push(cpu);
    let mut right = vec![memory];
    right.extend(disks);
    right.push(network);
    vec![row(
        json!({"gap":GAP,"alignItems":"flex-start"}),
        vec![
            view(json!({"flex":1,"gap":GAP,"minWidth":0}), left),
            view(json!({"flex":1,"gap":GAP,"minWidth":0}), right),
        ],
    )]
}
