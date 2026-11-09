//! Complete Monitor widget tree. The shared host binds widgets, pixels and localization only.
use super::{arr, charts, worker, App, Tab};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::OnceLock};
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn text(label: Value) -> Value {
    json!({"kind":"text","label":label,"style":{"fontSize":12,"textAlign":"start"}})
}
fn dim(label: Value) -> Value {
    let mut t = text(label);
    t["style"]["opacity"] = json!(0.65);
    t
}
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn row(children: Vec<Value>) -> Value {
    view(
        json!({"flexDirection":"row","alignItems":"center","gap":12,"flexWrap":"wrap"}),
        children,
    )
}
fn button(label: Value, cmd: &str, arg: &str) -> Value {
    json!({"kind":"button","label":label,"action":[cmd,arg]})
}
fn selected_button(label: Value, cmd: &str, arg: &str, on: bool) -> Value {
    let mut b = button(label, cmd, arg);
    b["style"] = json!({"opacity":if on{1.}else{0.65}});
    b
}
fn card(label: Value, children: Vec<Value>, context: &Value) -> Value {
    let dark = context["theme"]["mode"] != "light";
    let mut content = vec![{
        let mut t = text(label);
        t["style"]["fontWeight"] = json!(600);
        t
    }];
    content.extend(children);
    view(
        json!({"padding":16,"gap":12,"borderRadius":10,"backgroundColor":if dark{"#26272c"}else{"#f4f4f6"},"border":format!("1px solid {}",context["theme"]["separator"].as_str().unwrap_or("#444")),"minWidth":0}),
        content,
    )
}
fn big(value: String, label: Value) -> Value {
    let mut t = text(json!(value));
    t["style"]["fontSize"] = json!(26);
    t["style"]["fontWeight"] = json!(600);
    view(json!({"gap":4}), vec![t, dim(label)])
}
fn n(v: &Value) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(0.)
}
fn bytes(v: f64) -> String {
    let mut v = v.max(0.);
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut i = 0;
    while v >= 1024. && i + 1 < units.len() {
        v /= 1024.;
        i += 1
    }
    format!("{:.1} {}", v, units[i])
}
fn rate(v: f64) -> String {
    format!("{}/s", bytes(v))
}
fn pct(v: f64) -> String {
    format!("{:.0}%", v * 100.)
}
fn number(v: f64) -> String {
    format!("{v:.2}")
}
fn cell(label: Value, width: Value) -> Value {
    let mut t = text(label);
    t["style"]["width"] = width;
    t["style"]["whiteSpace"] = json!("nowrap");
    t
}
fn stat(label: &str, value: String) -> Value {
    view(
        json!({"minWidth":120,"flex":1,"gap":4,"padding":12}),
        vec![dim(key(label)), text(json!(value))],
    )
}
fn chart(
    app: &mut App,
    id: &str,
    series: Vec<(Vec<f64>, [u8; 4])>,
    top: f64,
    capacity: usize,
    height: usize,
    context: &Value,
) -> Value {
    let width = (context["width"].as_f64().unwrap_or(1140.) - 72.).clamp(200., 1840.) as usize;
    let top = if top > 0. {
        top
    } else {
        series
            .iter()
            .flat_map(|s| s.0.iter())
            .copied()
            .filter(|n| n.is_finite())
            .fold(1., f64::max)
            * 1.05
    };
    let raster = charts::area(width, height, &series, top, capacity, app.generation);
    app.images.insert(id.into(), raster.into());
    view(
        json!({"height":height+22,"gap":4}),
        vec![
            dim(json!(number(top))),
            json!({"kind":"raster","image":id,"generation":app.generation,"style":{"width":"100%","height":height}}),
        ],
    )
}
fn values(snapshot: &worker::Snapshot, pick: impl Fn(&Value) -> f64) -> Vec<f64> {
    snapshot.samples.iter().map(|s| pick(s)).collect()
}
pub fn icon(size: f64) -> Value {
    let size = if size.is_finite() {
        size.clamp(8., 512.)
    } else {
        32.
    };
    view(json!({"width":size,"height":size,"borderRadius":size*0.22,"backgroundColor":"#30a448","flexDirection":"row","alignItems":"center","justifyContent":"center","gap":size*0.06}),[0.3,0.65,0.45,0.8].iter().map(|h|view(json!({"width":size*0.1,"height":size*h,"backgroundColor":"#fff","borderRadius":size*0.03}),vec![])).collect())
}
fn memory(app: &mut App, snapshot: &worker::Snapshot, last: &Value, c: &Value) -> Vec<Value> {
    let m = &last["mem"];
    let total = n(&m["total"]).max(1.);
    let used = n(&m["used"]);
    let cache = n(&m["cache"]);
    let bar = view(
        json!({"height":22,"borderRadius":6,"overflow":"hidden","backgroundColor":"#555","flexDirection":"row"}),
        vec![
            view(
                json!({"width":format!("{}%",used/total*100.),"height":22,"backgroundColor":"#8b5cf6"}),
                vec![],
            ),
            view(
                json!({"width":format!("{}%",cache/total*100.),"height":22,"backgroundColor":"#c4b5fd"}),
                vec![],
            ),
        ],
    );
    let stats = row(vec![
        stat("monitor.memUsed", bytes(used)),
        stat("monitor.memCache", bytes(cache)),
        stat("monitor.memFree", bytes(n(&m["free"]))),
        stat(
            "monitor.swap",
            format!(
                "{} / {}",
                bytes(n(&m["swapUsed"])),
                bytes(n(&m["swapTotal"]))
            ),
        ),
    ]);
    let graph = chart(
        app,
        "memory",
        vec![
            (
                values(snapshot, |s| n(&s["mem"]["used"]) + n(&s["mem"]["cache"])),
                [196, 181, 253, 255],
            ),
            (
                values(snapshot, |s| n(&s["mem"]["used"])),
                [139, 92, 246, 255],
            ),
        ],
        total,
        worker::SAMPLES,
        170,
        c,
    );
    vec![
        card(
            key("monitor.memUse"),
            vec![
                big(bytes(n(&m["avail"])), key("monitor.memAvailable")),
                bar,
                stats,
                dim(json!({"key":"monitor.inTotal","params":{"total":bytes(total)}})),
            ],
            c,
        ),
        card(
            key("monitor.last60"),
            vec![
                graph,
                button(
                    key(if app.frozen.is_some() {
                        "monitor.resume"
                    } else {
                        "monitor.pause"
                    }),
                    "pause",
                    "",
                ),
            ],
            c,
        ),
        dim(key("monitor.cacheHint")),
        row(vec![
            stat(
                "monitor.memoryPressure",
                format!("{:.2}%", n(&last["pressure"]["some"]["avg10"])),
            ),
            stat("monitor.dirty", bytes(n(&m["dirty"]))),
            stat("monitor.writeback", bytes(n(&m["writeback"]))),
        ]),
    ]
}
fn overview(app: &mut App, snapshot: &worker::Snapshot, last: &Value, c: &Value) -> Vec<Value> {
    let cpu = chart(
        app,
        "overview-cpu",
        vec![(values(snapshot, |s| n(&s["cpu"])), [10, 132, 255, 255])],
        1.,
        worker::SAMPLES,
        86,
        c,
    );
    let mem = chart(
        app,
        "overview-memory",
        vec![(
            values(snapshot, |s| n(&s["mem"]["used"])),
            [139, 92, 246, 255],
        )],
        n(&last["mem"]["total"]),
        worker::SAMPLES,
        86,
        c,
    );
    let net = chart(
        app,
        "overview-network",
        vec![
            (values(snapshot, |s| n(&s["net"]["rx"])), [34, 197, 94, 255]),
            (
                values(snapshot, |s| n(&s["net"]["tx"])),
                [249, 115, 22, 255],
            ),
        ],
        0.,
        worker::SAMPLES,
        86,
        c,
    );
    let disk = chart(
        app,
        "overview-disk",
        vec![
            (
                values(snapshot, |s| n(&s["disk"]["read"])),
                [47, 124, 246, 255],
            ),
            (
                values(snapshot, |s| n(&s["disk"]["write"])),
                [236, 72, 153, 255],
            ),
        ],
        0.,
        worker::SAMPLES,
        86,
        c,
    );
    vec![
        card(
            key("monitor.cpu"),
            vec![
                big(pct(n(&last["cpu"])), json!(snapshot.info["cpuModel"])),
                cpu,
            ],
            c,
        ),
        card(
            key("monitor.memory"),
            vec![
                big(bytes(n(&last["mem"]["used"])), key("monitor.memUsed")),
                mem,
                row(vec![
                    stat("monitor.memAvailable", bytes(n(&last["mem"]["avail"]))),
                    stat("monitor.memCache", bytes(n(&last["mem"]["cache"]))),
                ]),
            ],
            c,
        ),
        card(
            key("monitor.network"),
            vec![
                row(vec![
                    stat("monitor.received", rate(n(&last["net"]["rx"]))),
                    stat("monitor.sent", rate(n(&last["net"]["tx"]))),
                ]),
                net,
            ],
            c,
        ),
        card(
            key("monitor.disks"),
            vec![
                row(vec![
                    stat("monitor.read", rate(n(&last["disk"]["read"]))),
                    stat("monitor.write", rate(n(&last["disk"]["write"]))),
                ]),
                disk,
            ],
            c,
        ),
        row(vec![
            stat(
                "monitor.cpuTemp",
                last["temp"]
                    .as_f64()
                    .map(|v| format!("{v:.0} °C"))
                    .unwrap_or_else(|| "—".into()),
            ),
            stat(
                "monitor.fan",
                last["fan"]
                    .as_f64()
                    .map(|v| format!("{v:.0} RPM"))
                    .unwrap_or_else(|| "—".into()),
            ),
            stat("monitor.load1m", number(n(&last["load"][0]))),
            stat("monitor.load5m", number(n(&last["load"][1]))),
            stat("monitor.load15m", number(n(&last["load"][2]))),
        ]),
    ]
}
fn cpu(app: &mut App, snapshot: &worker::Snapshot, last: &Value, c: &Value) -> Vec<Value> {
    let graph = chart(
        app,
        "cpu-total",
        vec![(values(snapshot, |s| n(&s["cpu"])), [10, 132, 255, 255])],
        1.,
        worker::SAMPLES,
        160,
        c,
    );
    let mut out = vec![card(
        key("monitor.totalUse"),
        vec![
            big(pct(n(&last["cpu"])), json!(snapshot.info["cpuModel"])),
            graph,
        ],
        c,
    )];
    let mut rows = Vec::new();
    for (i, value) in arr(&last["cores"]).iter().enumerate().take(256) {
        let graph = chart(
            app,
            &format!("cpu-{i}"),
            vec![(values(snapshot, |s| n(&s["cores"][i])), [10, 132, 255, 255])],
            1.,
            worker::SAMPLES,
            54,
            c,
        );
        rows.push(view(
            json!({"width":"23%","gap":6}),
            vec![
                text(json!({"key":"monitor.core","params":{"n":i}})),
                text(json!(pct(n(value)))),
                graph,
            ],
        ));
    }
    out.push(row(rows));
    out.push(row(vec![
        stat("monitor.load1m", number(n(&last["load"][0]))),
        stat(
            "monitor.tasks",
            format!("{} / {}", last["tasks"]["running"], last["tasks"]["total"]),
        ),
    ]));
    out
}
fn processes(app: &mut App, snapshot: &worker::Snapshot, c: &Value) -> Vec<Value> {
    let query = app.query.to_lowercase();
    let mut procs: Vec<_> = arr(&snapshot.processes["list"])
        .iter()
        .filter(|p| {
            (!app.mine || p["own"] == true)
                && (query.is_empty()
                    || format!("{} {} {}", p["name"], p["pid"], p["exe"])
                        .to_lowercase()
                        .contains(&query))
        })
        .collect();
    procs.sort_by(|a, b| {
        let cmp = if app.sort == "name" {
            a["name"].as_str().cmp(&b["name"].as_str())
        } else {
            n(&a[&app.sort]).total_cmp(&n(&b[&app.sort]))
        };
        if app.reverse {
            cmp.reverse()
        } else {
            cmp
        }
    });
    let total = procs.len();
    let mut rows = vec![row([
        ("monitor.process", "name"),
        ("monitor.cpuPct", "cpu"),
        ("monitor.memory", "rss"),
        ("monitor.threads", "threads"),
        ("monitor.pid", "pid"),
        ("monitor.user", "uid"),
    ]
    .iter()
    .map(|(label, sort)| button(key(label), "sort", sort))
    .collect())];
    for p in procs.into_iter().take(250) {
        let mut r = row(vec![
            cell(json!(p["name"]), json!(220)),
            cell(json!(pct(n(&p["cpu"]))), json!(75)),
            cell(json!(bytes(n(&p["rss"]))), json!(100)),
            cell(json!(p["threads"]), json!(60)),
            cell(json!(p["pid"]), json!(75)),
            cell(json!(p["user"]), json!(70)),
        ]);
        r["key"] = json!(format!("{}:{}", p["pid"], p["st"]));
        r["events"] = json!({"onClick":["select",format!("{}:{}",p["pid"],p["st"])]});
        rows.push(r);
    }
    let mut out = vec![
        row(vec![
            json!({"kind":"input","value":app.query,"placeholder":key("monitor.search"),"style":{"width":280}}),
            selected_button(key("monitor.mine"), "mine", "", app.mine),
            dim(json!({"key":"monitor.procCount","params":{"count":total}})),
        ]),
        card(key("monitor.processes"), rows, c),
    ];
    if total > 250 {
        out.push(dim(
            json!({"key":"monitor.procFoot","params":{"shown":250,"count":total,"cpu":"—"}}),
        ))
    }
    if let Some((pid, st)) = app.selected {
        let info = snapshot
            .details
            .get(&(pid, st))
            .cloned()
            .unwrap_or(Value::Null);
        let p = arr(&snapshot.processes["list"])
            .iter()
            .find(|p| p["pid"].as_i64() == Some(pid as i64) && p["st"].as_u64() == Some(st));
        let mut content = vec![
            row(vec![
                text(json!(format!("PID {pid}"))),
                button(key("monitor.proc.close"), "detailClose", ""),
            ]),
            row([
                ("monitor.memory", "memory"),
                ("monitor.proc.tab.stats", "stats"),
                ("monitor.proc.tab.files", "files"),
            ]
            .iter()
            .map(|(label, tab)| {
                selected_button(key(label), "detailTab", tab, app.detail_tab == *tab)
            })
            .collect()),
        ];
        if info.is_null() {
            content.push(dim(key("monitor.loading")))
        } else if info["gone"] == true {
            content.push(dim(key("monitor.proc.gone")))
        } else {
            match app.detail_tab.as_str() {
                "files" => {
                    content.push(text(json!(info["exe"])));
                    content.push(text(json!(info["cwd"])));
                    content.push(dim(json!(info["cmd"])));
                    for f in arr(&info["files"]).iter().take(200) {
                        content.push(row(vec![
                            cell(f["fd"].clone(), json!(50)),
                            text(f["path"].clone()),
                        ]));
                    }
                }
                "stats" => {
                    for (label, field) in [
                        ("monitor.threads", "threads"),
                        ("monitor.proc.mem.virtual", "vsize"),
                        ("monitor.proc.st.cpuTime", "cpuSeconds"),
                        ("monitor.minorFaults", "minor"),
                        ("monitor.majorFaults", "major"),
                        ("monitor.read", "readBytes"),
                        ("monitor.write", "writeBytes"),
                    ] {
                        content.push(row(vec![dim(key(label)), text(info[field].clone())]));
                    }
                }
                _ => {
                    for (label, field) in [
                        ("monitor.proc.mem.real", "rss"),
                        ("monitor.proc.mem.virtual", "vsize"),
                        ("monitor.proc.mem.pss", "pss"),
                        ("monitor.proc.mem.private", "private"),
                        ("monitor.proc.mem.shared", "shared"),
                        ("monitor.proc.mem.anon", "anon"),
                        ("monitor.proc.mem.swap", "swap"),
                    ] {
                        content.push(row(vec![
                            dim(key(label)),
                            text(json!(bytes(n(&info[field])))),
                        ]));
                    }
                }
            }
        }
        if let Some(p) = p {
            if let Some(reason) = p["prot"].as_str() {
                content.push(dim(key(&format!("monitor.proc.why.{reason}"))));
            } else if p["own"] != true {
                content.push(dim(
                    json!({"key":"monitor.proc.needAdmin","params":{"user":p["user"]}}),
                ));
            } else if app.confirming {
                content.push(text(
                    json!({"key":"monitor.proc.confirm","params":{"name":p["name"]}}),
                ));
                content.push(dim(key("monitor.proc.confirmHint")));
                let mut force = button(key("monitor.proc.forceQuit"), "signal", "KILL");
                force["destructive"] = json!(true);
                content.push(row(vec![
                    button(key("monitor.proc.cancel"), "cancel", ""),
                    button(key("monitor.proc.quit"), "signal", "TERM"),
                    force,
                ]));
            } else {
                content.push(button(key("monitor.proc.quitDots"), "confirm", ""));
            }
        }
        if let Some(error) = &app.error {
            content.push(text(json!({"error":error})));
        }
        out.push(card(key("monitor.proc.getInfo"), content, c));
    }
    out
}
fn disks(app: &mut App, snapshot: &worker::Snapshot, last: &Value, c: &Value) -> Vec<Value> {
    let list = arr(&last["disk"]["disks"]);
    let mut rows = Vec::new();
    for disk in list {
        let name = disk["name"].as_str().unwrap_or("");
        let mut r = row(vec![
            cell(
                json!(disk["model"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(name)),
                json!(180),
            ),
            cell(json!(format!("/dev/{name}")), json!(120)),
            cell(json!(bytes(n(&disk["size"]))), json!(90)),
            cell(
                json!(disk["temp"]
                    .as_f64()
                    .map(|v| format!("{v:.0} °C"))
                    .unwrap_or_else(|| "—".into())),
                json!(80),
            ),
            cell(json!(rate(n(&disk["read"]))), json!(110)),
            cell(json!(rate(n(&disk["write"]))), json!(110)),
            dim(key("monitor.health.unknown")),
        ]);
        r["events"] = json!({"onClick":["disk",name]});
        rows.push(r);
    }
    if rows.is_empty() {
        rows.push(dim(key("monitor.noDisks")))
    }
    let mut out = vec![card(key("monitor.disks"), rows, c)];
    let chosen = list
        .iter()
        .find(|d| d["name"] == app.disk)
        .or_else(|| list.first());
    if let Some(d) = chosen {
        let name = d["name"].as_str().unwrap_or("");
        app.disk = name.into();
        let graph = chart(
            app,
            "disk-io",
            vec![
                (
                    values(snapshot, |s| {
                        arr(&s["disk"]["disks"])
                            .iter()
                            .find(|d| d["name"] == name)
                            .map(|d| n(&d["read"]))
                            .unwrap_or(0.)
                    }),
                    [47, 124, 246, 255],
                ),
                (
                    values(snapshot, |s| {
                        arr(&s["disk"]["disks"])
                            .iter()
                            .find(|d| d["name"] == name)
                            .map(|d| n(&d["write"]))
                            .unwrap_or(0.)
                    }),
                    [236, 72, 153, 255],
                ),
            ],
            0.,
            worker::SAMPLES,
            130,
            c,
        );
        let temps = app
            .disk_history
            .get(name)
            .map(|h| h.iter().map(|p| p.1).collect())
            .unwrap_or_default();
        let temp_graph = chart(
            app,
            "disk-temp",
            vec![(temps, [255, 159, 10, 255])],
            100.,
            600,
            100,
            c,
        );
        out.push(card(
            json!(format!("/dev/{name}")),
            vec![
                graph,
                temp_graph,
                dim(key(if d["tempIdle"] == true {
                    "monitor.temp.resting"
                } else if d["sensor"].is_null() {
                    "monitor.temp.noSensor"
                } else {
                    "monitor.temp.col"
                })),
                button(key("monitor.openInDisks"), "openDisks", ""),
            ],
            c,
        ));
    }
    out.push(row(vec![
        selected_button(
            key("monitor.diskAlert.toggle"),
            "pref",
            "diskAlerts",
            app.prefs["diskAlerts"] != false,
        ),
        dim(key("monitor.health.unknown")),
    ]));
    out
}
fn cooling(app: &mut App, snapshot: &worker::Snapshot, c: &Value) -> Vec<Value> {
    let sensor = &snapshot.sensors;
    let mut stats = Vec::new();
    for t in arr(&sensor["temps"]) {
        stats.push(row(vec![
            text(json!(format!(
                "{} · {}",
                t["chip"].as_str().unwrap_or(""),
                t["label"].as_str().unwrap_or("")
            ))),
            text(json!(format!("{:.1} °C", n(&t["value"])))),
        ]));
    }
    for fan in arr(&sensor["fans"]) {
        stats.push(row(vec![
            text(fan["label"].clone()),
            text(json!(format!("{:.0} RPM", n(&fan["rpm"])))),
        ]));
    }
    if stats.is_empty() {
        stats.push(dim(key("cooling.noFans")))
    }
    let daemon = &snapshot.fan;
    if !daemon.is_null() {
        if let Some(profile) = daemon["profile"].as_str() {
            stats.push(row(vec![
                dim(key("cooling.profile")),
                text(key(&format!("cooling.profiles.{profile}"))),
            ]));
        }
        for fan in arr(&daemon["fans"]) {
            stats.push(text(json!(format!("{}% · {} RPM", fan["pct"], fan["rpm"]))));
        }
        if !arr(&daemon["fans"]).is_empty() {
            stats.push(button(
                key(if daemon["force"].as_str().unwrap_or("off") != "off" {
                    "cooling.stopForce"
                } else {
                    "cooling.force100"
                }),
                "fanForce",
                "",
            ));
        }
    }
    if let Some(error) = &app.error {
        stats.push(text(json!({"error":error})));
    }
    let graph = chart(
        app,
        "cooling",
        vec![(values(snapshot, |s| n(&s["temp"])), [255, 159, 10, 255])],
        100.,
        worker::SAMPLES,
        150,
        c,
    );
    vec![
        card(key("monitor.tab.cooling"), stats, c),
        card(key("cooling.history"), vec![graph], c),
        row(vec![
            dim(key("cooling.systemManaged")),
            button(key("cooling.customize"), "settings", "cooling"),
        ]),
    ]
}
pub fn primary_voltage(v: &Value) -> Option<(String, f64)> {
    for supply in arr(&v["supplies"]) {
        if supply["battery"] == true {
            return Some((supply["name"].as_str()?.into(), supply["v"].as_f64()?));
        }
    }
    for supply in arr(&v["supplies"]) {
        return Some((supply["name"].as_str()?.into(), supply["v"].as_f64()?));
    }
    for chip in arr(&v["chips"]) {
        for rail in arr(&chip["in"]) {
            if rail["label"].as_str().is_some_and(|s| !s.is_empty()) && n(&rail["v"]) > 0. {
                return Some((format!("{}/{}", chip["dev"], rail["i"]), n(&rail["v"])));
            }
        }
    }
    None
}
fn voltage(app: &mut App, snapshot: &worker::Snapshot, c: &Value) -> Vec<Value> {
    let voltage = &snapshot.sensors["voltage"];
    let mut out = Vec::new();
    if let Some((name, value)) = primary_voltage(voltage) {
        let vals: Vec<_> = app.voltage_history.iter().map(|p| p.1).collect();
        let lo = vals.iter().copied().fold(value, f64::min);
        let hi = vals.iter().copied().fold(value, f64::max);
        let pad = ((hi - lo) * 0.25).max(hi.abs() * 0.01).max(0.005);
        let floor = lo - pad;
        let graph = chart(
            app,
            "voltage",
            vec![(
                vals.iter().map(|v| v - floor).collect(),
                [50, 173, 230, 255],
            )],
            hi - lo + 2. * pad,
            150,
            130,
            c,
        );
        out.push(card(
            json!(name),
            vec![
                big(format!("{value:.3} V"), key("monitor.volt.voltage")),
                graph,
            ],
            c,
        ));
    }
    for chip in arr(&voltage["chips"]) {
        let rails = arr(&chip["in"])
            .iter()
            .map(|r| {
                row(vec![
                    text(json!(r["label"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("in{}", r["i"])))),
                    text(json!(format!("{:.3} V", n(&r["v"])))),
                ])
            })
            .collect();
        out.push(card(chip["name"].clone(), rails, c));
    }
    if !arr(&voltage["supplies"]).is_empty() {
        let rows = arr(&voltage["supplies"])
            .iter()
            .map(|s| {
                row(vec![
                    text(s["name"].clone()),
                    text(json!(format!("{:.3} V", n(&s["v"])))),
                    text(json!(s["a"]
                        .as_f64()
                        .map(|a| format!("{a:.3} A"))
                        .unwrap_or_default())),
                ])
            })
            .collect();
        out.push(card(key("monitor.volt.supplies"), rows, c));
    }
    if out.is_empty() {
        out.push(dim(key("monitor.volt.noSources")))
    }
    out
}
fn country_positions() -> &'static Value {
    static POSITIONS: OnceLock<Value> = OnceLock::new();
    POSITIONS
        .get_or_init(|| serde_json::from_str(super::countries::POSITIONS).unwrap_or(Value::Null))
}
fn network(app: &mut App, snapshot: &worker::Snapshot, last: &Value, c: &Value) -> Vec<Value> {
    if app.prefs["connections"] == false {
        return vec![
            dim(key("monitor.net.disabled")),
            button(key("monitor.net.privacyLink"), "section", "privacy"),
        ];
    }
    let query = app.query.to_lowercase();
    let list: Vec<_> = arr(&snapshot.connections["list"])
        .iter()
        .filter(|s| {
            (app.app.is_empty() || s["app"] == app.app || s["name"] == app.app)
                && (app.scope.is_empty() || s["scope"] == app.scope)
                && (app.protocol.is_empty() || s["proto"] == app.protocol)
                && (query.is_empty() || s.to_string().to_lowercase().contains(&query))
        })
        .collect();
    let mut out = vec![
        row(vec![
            json!({"kind":"input","value":app.query,"placeholder":key("monitor.net.search"),"style":{"width":320}}),
            button(key("monitor.net.showAll"), "app", ""),
            button(key("monitor.hist.title"), "history", ""),
            button(key("monitor.net.privacyLink"), "section", "privacy"),
        ]),
        row([
            ("monitor.all", ""),
            ("monitor.net.scope.internet", "internet"),
            ("monitor.net.scope.lan", "lan"),
            ("monitor.net.scope.local", "local"),
        ]
        .iter()
        .map(|(label, scope)| selected_button(key(label), "scope", scope, app.scope == *scope))
        .collect()),
        row(["", "tcp", "udp", "icmp", "raw"]
            .iter()
            .map(|proto| {
                selected_button(
                    if proto.is_empty() {
                        key("monitor.all")
                    } else {
                        json!(proto.to_uppercase())
                    },
                    "protocol",
                    proto,
                    app.protocol == *proto,
                )
            })
            .collect()),
        row(vec![
            stat("monitor.received", rate(n(&last["net"]["rx"]))),
            stat("monitor.sent", rate(n(&last["net"]["tx"]))),
            stat(
                "monitor.net.sinceBoot",
                format!(
                    "↓ {} · ↑ {}",
                    bytes(n(&last["net"]["rxTotal"])),
                    bytes(n(&last["net"]["txTotal"]))
                ),
            ),
        ]),
    ];
    if app.history_open {
        let history = &snapshot.history;
        let rx = arr(&history["rx"]).iter().map(n).collect::<Vec<_>>();
        let tx = arr(&history["tx"]).iter().map(n).collect::<Vec<_>>();
        let count = rx.len().max(2);
        let graph = chart(
            app,
            "network-history",
            vec![(rx, [34, 197, 94, 255]), (tx, [249, 115, 22, 255])],
            0.,
            count,
            150,
            c,
        );
        out.push(card(
            key("monitor.hist.title"),
            vec![
                row([
                    ("1 h", "3600"),
                    ("24 h", "86400"),
                    ("7 d", "604800"),
                    ("30 d", "2592000"),
                ]
                .iter()
                .map(|(label, span)| button(json!(label), "historySpan", span))
                .collect()),
                graph,
                dim(key("monitor.privacy.historyHint")),
            ],
            c,
        ));
    }
    if app.prefs["geo"] != false {
        let positions = country_positions();
        let places: Vec<_> = list
            .iter()
            .filter_map(|s| s["cc"].as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .filter_map(|cc| Some((positions[cc][0].as_f64()?, positions[cc][1].as_f64()?)))
            .collect();
        let home = snapshot.info["home"]["cc"]
            .as_str()
            .and_then(|cc| Some((positions[cc][0].as_f64()?, positions[cc][1].as_f64()?)));
        let width = (c["width"].as_f64().unwrap_or(1140.) - 72.).clamp(200., 1840.) as usize;
        let fingerprint = format!(
            "{width}/{}/{}/{}/{:?}/{:?}",
            app.map_lon, app.map_lat, app.map_zoom, places, home
        );
        if app.map_request != fingerprint {
            app.map_request = fingerprint;
            if let Some(lease) = &app.lease {
                lease.service.job(worker::Job::Map(
                    lease.id,
                    charts::MapView {
                        width,
                        places,
                        home,
                        lon: app.map_lon,
                        lat: app.map_lat,
                        zoom: app.map_zoom,
                        generation: app.generation,
                    },
                ));
            }
        }
        let map_generation = if let Some(lease) = &app.lease {
            if let Some(image) = snapshot.maps.get(&lease.id) {
                app.images.insert("world".into(), image.clone());
                image.generation
            } else {
                0
            }
        } else {
            0
        };
        out.push(card(key("monitor.net.map"),vec![json!({"kind":"raster","image":"world","generation":map_generation,"style":{"width":"100%","height":280}}),row(vec![button(json!("−"),"zoom","out"),button(json!("+"),"zoom","in"),button(json!("←"),"pan","left"),button(json!("→"),"pan","right"),button(json!("↑"),"pan","up"),button(json!("↓"),"pan","down")]),dim(key("monitor.net.privacyShort"))],c));
    }
    let mut groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for s in &list {
        let name = s["app"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| s["name"].as_str())
            .unwrap_or("?");
        groups.entry(name.into()).or_default().push(s);
    }
    let mut rows = Vec::new();
    for (name, sockets) in groups.into_iter().take(100) {
        rows.push(button(
            json!(format!("{name} ({})", sockets.len())),
            "app",
            &name,
        ));
        for socket in sockets.into_iter().take(40) {
            let mut edit = button(key("fwrules.blockThisAddress"), "destination", "");
            edit["value"] = json!({"app":socket["app"],"dst":socket["remote"]});
            edit["disabled"] = json!(socket["app"].as_str().unwrap_or("").is_empty());
            rows.push(row(vec![
                edit,
                cell(socket["proto"].clone(), json!(50)),
                cell(
                    json!(format!(
                        "{}:{}",
                        socket["remote"].as_str().unwrap_or(""),
                        socket["port"]
                    )),
                    json!(260),
                ),
                cell(socket["pid"].clone(), json!(60)),
                cell(
                    socket["cc"]
                        .as_str()
                        .map(|cc| key(&format!("countries.{cc}")))
                        .unwrap_or(json!("—")),
                    json!(160),
                ),
                dim(key(if socket["listening"] == true {
                    "monitor.net.listening"
                } else {
                    "monitor.net.active"
                })),
            ]));
        }
    }
    if rows.is_empty() {
        rows.push(dim(key("monitor.net.none")))
    }
    out.push(card(key("monitor.net.connections"), rows, c));
    if !app.app.is_empty() {
        out.push(row(vec![
            button(key("monitor.net.rule"), "section", "rules"),
            button(key("monitor.net.privacyLink"), "section", "firewall"),
        ]));
    }
    out
}
fn privacy(app: &App, c: &Value) -> Vec<Value> {
    let mut rows = Vec::new();
    for (field, label, hint, default) in [
        (
            "connections",
            "monitor.privacy.connections",
            "monitor.privacy.connectionsHint",
            true,
        ),
        (
            "geo",
            "monitor.privacy.geo",
            "monitor.privacy.geoHint",
            true,
        ),
        (
            "history",
            "monitor.privacy.history",
            "monitor.privacy.historyHint",
            true,
        ),
        (
            "menuItem",
            "monitor.privacy.menuItem",
            "monitor.privacy.menuItemHint",
            false,
        ),
    ] {
        let on = app.prefs[field].as_bool().unwrap_or(default);
        rows.push(row(vec![
            selected_button(key(label), "pref", field, on),
            text(json!(if on { "●" } else { "○" })),
        ]));
        rows.push(dim(key(hint)));
    }
    rows.push(button(
        key(if app.cleared {
            "monitor.privacy.cleared"
        } else {
            "monitor.privacy.clear"
        }),
        "clearHistory",
        "",
    ));
    vec![
        card(key("monitor.privacy.title"), rows, c),
        dim(key("monitor.privacy.local")),
    ]
}
fn firewall(app: &App, c: &Value) -> Vec<Value> {
    let fw = &app.services["fw"];
    let status = &fw["status"];
    let mut out = vec![
        dim(key("firewall.footer")),
        app_choices(app),
        button(key("fwrules.title"), "section", "rules"),
    ];
    if status.is_null() || status["connected"] == false {
        out.push(dim(key("firewall.status.noDaemon")))
    }
    let state = &fw["state"];
    let mut rows = vec![row([
        ("firewall.mode.silent", "silent"),
        ("firewall.mode.deny", "deny"),
        ("firewall.mode.off", "off"),
    ]
    .iter()
    .map(|(label, mode)| {
        selected_button(
            key(label),
            "rule",
            &format!("setMode:{mode}"),
            state["mode"] == *mode,
        )
    })
    .collect())];
    for field in ["trusted", "restricted"] {
        if let Some(values) = state[field].as_object() {
            for (id, value) in values.iter().take(200) {
                rows.push(row(vec![
                    button(json!(id), "app", id),
                    text(json!(value.to_string())),
                ]));
            }
        } else {
            for value in arr(&state[field]).iter().take(200) {
                rows.push(text(value.clone()));
            }
        }
    }
    let counts = &fw["poll"]["counts"];
    if let Some(map) = counts.as_object() {
        for (app, v) in map.iter().take(200) {
            rows.push(row(vec![
                button(json!(app), "app", app),
                stat("monitor.net.allowed", v[0].to_string()),
                stat("monitor.net.denied", v[1].to_string()),
                text(json!(format!(
                    "↓ {} · ↑ {}",
                    bytes(n(&v[2])),
                    bytes(n(&v[3]))
                ))),
            ]));
        }
    }
    out.push(card(key("monitor.net.rule"), rows, c));
    out
}
fn app_choices(app: &App) -> Value {
    let mut choices = BTreeMap::new();
    choices.insert("lordos".to_owned(), "Axia DE".to_owned());
    for item in arr(&app.catalog) {
        let id = item["id"].as_str().unwrap_or("");
        let id = if item["kind"] == "linux" {
            id.to_owned()
        } else if item["isolated"] == true || item["package"] == true {
            format!("lpk:{id}")
        } else {
            "lordos".into()
        };
        choices
            .entry(id)
            .or_insert_with(|| item["name"].as_str().unwrap_or("").to_owned());
    }
    if let Some(map) = app.services["fw"]["poll"]["counts"].as_object() {
        for id in map.keys() {
            choices.entry(id.clone()).or_insert(id.clone());
        }
    }
    row(choices
        .into_iter()
        .take(100)
        .map(|(id, name)| selected_button(json!(name), "app", &id, app.app == id))
        .collect())
}
fn field(app: &App, name: &str, label: &str) -> Value {
    view(
        json!({"gap":4}),
        vec![
            dim(key(label)),
            json!({"kind":"input","value":app.form[name],"change":["field",name],"style":{"width":320}}),
        ],
    )
}
fn rules(app: &App, c: &Value) -> Vec<Value> {
    let mut out = vec![app_choices(app), text(json!(app.app))];
    if app.app.is_empty() {
        return vec![card(key("fwrules.title"), out, c)];
    }
    out.push(row([
        ("fwrules.default", "default"),
        ("fwrules.allow", "allow"),
        ("fwrules.block", "deny"),
    ]
    .iter()
    .map(|(label, v)| button(key(label), "rule", &format!("setAppRule:{v}")))
    .collect()));
    out.push(row(vec![
        button(key("firewall.trusted"), "rule", "setTrusted:true"),
        button(key("firewall.remove"), "rule", "setTrusted:false"),
    ]));
    let flags = &app.services["fw"]["state"]["restricted"][&app.app];
    out.push(row([
        "no-internet",
        "no-lan",
        "no-network",
        "no-incoming",
        "no-camera",
        "no-mic",
        "no-screen",
        "no-usb",
        "no-bluetooth",
        "no-files",
        "no-notifications",
        "no-clipboard",
    ]
    .iter()
    .filter(|flag| {
        !(app.app == "lordos"
            && ["no-files", "no-usb", "no-clipboard", "no-bluetooth"].contains(flag))
            && !(app.app.starts_with("desktop:") && **flag == "no-clipboard")
    })
    .map(|flag| {
        selected_button(
            key(&format!("firewall.r.{flag}")),
            "restriction",
            flag,
            arr(flags).iter().any(|v| v == flag),
        )
    })
    .collect()));
    for rule in arr(&app.rules_data).iter().take(200) {
        let id = rule["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| rule["id"].to_string());
        out.push(row(vec![
            text(json!(format!(
                "{} {} {} {}",
                rule["action"].as_str().unwrap_or(""),
                rule["dst"].as_str().unwrap_or(""),
                rule["port"].as_str().unwrap_or(""),
                rule["proto"].as_str().unwrap_or("")
            ))),
            button(key("fwrules.sheet.editTitle"), "editRule", &id),
            button(key("firewall.remove"), "rule", &format!("removeRule:{id}")),
        ]));
    }
    out.push(field(app, "dst", "fwrules.field.dst"));
    out.push(field(app, "port", "fwrules.field.port"));
    for (name, choices) in [
        (
            "proto",
            vec![
                ("", key("monitor.all")),
                ("tcp", json!("TCP")),
                ("udp", json!("UDP")),
            ],
        ),
        (
            "action",
            vec![
                ("allow", key("fwrules.allow")),
                ("deny", key("fwrules.block")),
            ],
        ),
        (
            "dir",
            vec![
                ("out", key("monitor.net.outgoing")),
                ("in", key("monitor.net.incoming")),
            ],
        ),
    ] {
        out.push(row(choices
            .into_iter()
            .map(|(value, label)| {
                let mut b = selected_button(label, "field", name, app.form[name] == value);
                b["value"] = json!(value);
                b
            })
            .collect()));
    }
    out.push(button(key("fwrules.save"), "saveRule", ""));
    if let Some(error) = &app.form_error {
        out.push(dim(key(&format!("fwrules.err.{error}"))))
    }
    if let Some(error) = &app.error {
        out.push(text(json!({"error":error})))
    }
    vec![card(key("fwrules.title"), out, c)]
}
fn blocklists(app: &App, c: &Value) -> Vec<Value> {
    let mut rows = vec![button(
        key("firewall.lists.update"),
        "rule",
        "updateLists:true",
    )];
    for list in arr(&app.services["fw"]["lists"]["lists"]) {
        let id = list["id"].as_str().unwrap_or("");
        rows.push(row(vec![
            text(list["name"].as_str().map(|s| json!(s)).unwrap_or(json!(id))),
            selected_button(
                key("firewall.lists.enabled"),
                "rule",
                &format!("setList:{id}:{}", list["on"] != true),
                list["on"] == true,
            ),
            text(list["error"].clone()),
            button(key("firewall.remove"), "rule", &format!("removeList:{id}")),
        ]));
    }
    rows.push(field(app, "url", "firewall.lists.urlPlaceholder"));
    rows.push(row(["domains", "hosts", "ips"]
        .iter()
        .map(|format| {
            let mut b = selected_button(
                key(&format!("firewall.lists.format.{format}")),
                "field",
                "format",
                app.form["format"] == *format,
            );
            b["value"] = json!(format);
            b
        })
        .collect()));
    rows.push(button(key("firewall.lists.add"), "addList", ""));
    if let Some(error) = &app.form_error {
        rows.push(dim(key(&format!("firewall.lists.err.{error}"))))
    }
    rows.push(dim(key("firewall.lists.footer")));
    vec![card(key("firewall.lists.title"), rows, c)]
}
fn apply_text_color(node: &mut Value, color: &Value) {
    if node["kind"] == "text" && node["style"]["color"].is_null() {
        node["style"]["color"] = color.clone();
    }
    if let Some(children) = node["children"].as_array_mut() {
        for child in children {
            apply_text_color(child, color);
        }
    }
}
pub fn tree(app: &mut App, context: &Value) -> Value {
    let snapshot = app.frozen.clone().unwrap_or_else(|| app.snapshot.clone());
    let last = snapshot
        .samples
        .back()
        .cloned()
        .unwrap_or_else(|| std::sync::Arc::new(Value::Null));
    let section = app.section.clone();
    let mut children = if section.is_empty() {
        let tabs = Tab::ALL
            .into_iter()
            .filter(|t| *t != Tab::Voltage || last["volt"] == true)
            .map(|tab| selected_button(key(tab.label()), "tab", tab.id(), tab == app.tab))
            .collect();
        vec![row(tabs)]
    } else {
        vec![row(vec![
            button(key("monitor.title"), "section", ""),
            button(key("monitor.privacy.title"), "section", "privacy"),
            button(key("monitor.net.rule"), "section", "firewall"),
            button(key("firewall.lists.title"), "section", "blocklists"),
        ])]
    };
    let content = match section.as_str() {
        "privacy" => privacy(app, context),
        "firewall" => firewall(app, context),
        "rules" => rules(app, context),
        "blocklists" => blocklists(app, context),
        _ => {
            if last.is_null() {
                vec![dim(key("monitor.waiting"))]
            } else {
                match app.tab {
                    Tab::Overview => overview(app, &snapshot, &last, context),
                    Tab::Cpu => cpu(app, &snapshot, &last, context),
                    Tab::Memory => memory(app, &snapshot, &last, context),
                    Tab::Network => network(app, &snapshot, &last, context),
                    Tab::Disks => disks(app, &snapshot, &last, context),
                    Tab::Processes => processes(app, &snapshot, context),
                    Tab::Cooling => cooling(app, &snapshot, context),
                    Tab::Voltage => voltage(app, &snapshot, context),
                }
            }
        }
    };
    children.push(json!({"kind":"scroll","style":{"flex":1},"children":[view(json!({"padding":20,"gap":16}),content)]}));
    children.push(row(vec![dim(key("monitor.footer")),dim(json!({"key":"monitor.loadFooter","params":{"load":arr(&last["load"]).iter().map(|n|number(n.as_f64().unwrap_or(0.))).collect::<Vec<_>>().join(" · ")}}))]));
    let mut tree = view(json!({"flex":1,"gap":8}), children);
    apply_text_color(&mut tree, &context["theme"]["text"]);
    tree
}
