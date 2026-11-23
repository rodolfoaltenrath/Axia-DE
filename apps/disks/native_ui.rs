use super::{arr, key, policy, smart, text, App, Dialog};
use serde_json::{json, Value};
fn v(id: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":id,"style":style,"children":children})
}
fn n(value: &Value) -> f64 {
    value.as_f64().filter(|n| n.is_finite()).unwrap_or(0.)
}
fn num(value: f64, digits: u32) -> Value {
    json!({"number":value,"digits":digits})
}
fn bytes(value: f64) -> Value {
    json!({"bytes":value})
}
fn tr(name: &str, params: Value) -> Value {
    json!({"key":format!("disks.{name}"),"params":params})
}
fn label(value: Value, t: &Value, dim: bool, size: u32) -> Value {
    json!({"kind":"text","label":value,"style":{"fontSize":size,"color":t[if dim{"textDim"}else{"text"}],"textAlign":"start","lineHeight":1.4}})
}
// Unformatted JS numbers stringify integral floats as integers. Preserve that
// convention; explicit {number,digits} captions keep their requested precision.
fn normalize_caption(value: &mut Value) {
    match value {
        Value::Number(number) => {
            if number.is_f64() {
                if let Some(n) = number
                    .as_f64()
                    .filter(|n| n.fract() == 0. && *n >= i64::MIN as f64 && *n < i64::MAX as f64)
                {
                    *value = json!(n as i64);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_caption(value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                normalize_caption(value);
            }
        }
        _ => {}
    }
}
fn normalize_labels(node: &mut Value) {
    for key in ["label", "placeholder"] {
        if let Some(value) = node.get_mut(key) {
            normalize_caption(value);
        }
    }
    if let Some(children) = node["children"].as_array_mut() {
        for child in children {
            normalize_labels(child);
        }
    }
}

#[cfg(test)]
#[test]
fn raw_numeric_captions_match_javascript_without_losing_explicit_precision() {
    let root = format!("{}/../../assets", env!("CARGO_MANIFEST_DIR"));
    let mut i18n = axia_native::i18n::I18n::new(&root, None);
    i18n.lang = "en".into();
    i18n.decimal = ".".into();
    let mut value = tr("smart.testRunning", json!({"pct":10.0}));
    normalize_caption(&mut value);
    assert_eq!(i18n.caption(&value), "Running: 10% done");
    let mut value = num(10.5, 1);
    normalize_caption(&mut value);
    assert_eq!(i18n.caption(&value), "10.5");
}
fn dim(value: Value, t: &Value) -> Value {
    label(value, t, true, 12)
}
fn row(id: &str, children: Vec<Value>) -> Value {
    v(
        id,
        json!({"flexDirection":"row","alignItems":"center","gap":8,"justifyContent":if id=="footer"{"flex-end"}else{"flex-start"}}),
        children,
    )
}
fn joined(values: Vec<Value>, separator: &str) -> Value {
    let mut parts = vec![];
    for value in values.into_iter().filter(|v| {
        !v.is_null()
            && v != ""
            && v.get("parts")
                .and_then(Value::as_array)
                .is_none_or(|p| !p.is_empty())
    }) {
        if !parts.is_empty() {
            parts.push(json!(separator));
        }
        parts.push(value);
    }
    json!({"parts":parts})
}
fn button(value: Value, cmd: &str, arg: Value, disabled: bool, primary: bool, t: &Value) -> Value {
    json!({"kind":"button","key":format!("{cmd}:{arg}"),"label":value,"action":[cmd,arg],"disabled":disabled,"style":if primary{json!({"borderWidth":1,"borderColor":t["accent"],"backgroundColor":format!("{}22",t["accent"].as_str().unwrap_or("#0a84ff"))})}else{json!({})}})
}
fn input(id: &str, value: &str, placeholder: Value, cmd: &str, submit: &str) -> Value {
    let mut node = json!({"kind":"input","key":id,"value":value,"placeholder":placeholder,"change":[cmd],"style":{"minHeight":28,"minWidth":80}});
    if !submit.is_empty() {
        node["submit"] = json!([submit]);
    }
    node
}
fn field(id: &str, name: Value, value: Value, t: &Value) -> Value {
    row(
        id,
        vec![
            v(
                "name",
                json!({"width":170,"flexShrink":0}),
                vec![dim(name, t)],
            ),
            v(
                "value",
                json!({"flex":1,"minWidth":0}),
                vec![label(
                    if value.is_null() || value == "" {
                        json!("—")
                    } else {
                        value
                    },
                    t,
                    false,
                    13,
                )],
            ),
        ],
    )
}
fn info(name: &str, value: Value, t: &Value) -> Value {
    field(name, key(name), value, t)
}
fn group(id: &str, title: Value, footer: Value, children: Vec<Value>, t: &Value) -> Value {
    let mut body = vec![];
    if !title.is_null() {
        body.push(dim(title, t));
    }
    body.push(v("body",json!({"borderWidth":1,"borderColor":t["separator"],"borderRadius":10,"padding":12,"gap":7}),children));
    if !footer.is_null() {
        body.push(dim(footer, t));
    }
    v(id, json!({"gap":8}), body)
}
fn scroll(id: &str, children: Vec<Value>) -> Value {
    json!({"kind":"scroll","key":id,"style":{"flex":1,"minWidth":0,"minHeight":0},"children":[v("content",json!({"padding":24,"gap":20}),children)]})
}
fn disk_title(disk: &Value) -> Value {
    if text(&disk["model"]).is_empty() {
        disk["name"].clone()
    } else {
        disk["model"].clone()
    }
}
fn part_title(part: &Value) -> Value {
    if !text(&part["label"]).is_empty() {
        part["label"].clone()
    } else if !text(&part["partlabel"]).is_empty() {
        part["partlabel"].clone()
    } else {
        tr("partNumber", json!({"n":part["number"]}))
    }
}
fn bus(disk: &Value) -> Value {
    let kind = text(&disk["transport"]);
    key(&format!(
        "bus.{}",
        if ["usb", "sata", "nvme", "virtio", "mmc", "loop", "scsi"].contains(&kind) {
            kind
        } else {
            "scsi"
        }
    ))
}
fn drive_glyph(disk: &Value, size: f64) -> Value {
    let usb = disk["removable"] == true || ["usb", "mmc"].contains(&text(&disk["transport"]));
    let color = if disk["transport"] == "loop" {
        "#a78bfa"
    } else if usb {
        "#f59e0b"
    } else if disk["transport"] == "nvme" {
        "#3b82f6"
    } else {
        "#8e96a3"
    };
    let inner = if usb {
        v(
            "plug",
            json!({"width":size*0.34,"height":size*0.2,"marginTop":-size*0.12,"backgroundColor":"#c7ccd3","borderRadius":size*0.03}),
            vec![],
        )
    } else {
        v(
            "platter",
            json!({"width":size*0.3,"height":size*0.3,"borderRadius":size*0.15,"backgroundColor":"rgba(255,255,255,0.55)"}),
            vec![],
        )
    };
    v(
        "drive",
        json!({"width":size,"height":size,"alignItems":"center","justifyContent":"center","flexShrink":0}),
        vec![v(
            "case",
            json!({"width":size*if usb{0.62}else{0.9},"height":size*if usb{0.9}else{0.62},"borderRadius":size*0.14,"backgroundColor":color,"alignItems":"center","justifyContent":if usb{"flex-start"}else{"center"}}),
            vec![inner],
        )],
    )
}
fn part_glyph(kind: &str, size: f64) -> Value {
    v(
        "partition",
        json!({"width":size,"height":size,"borderRadius":size*0.25,"backgroundColor":policy::fs_color(kind),"flexShrink":0}),
        vec![],
    )
}
fn sidebar(app: &App) -> Value {
    let t = &app.theme;
    let mut rows = vec![];
    for kind in ["internal", "external", "images"] {
        let disks: Vec<_> = arr(&app.list)
            .iter()
            .filter(|d| match kind {
                "internal" => d["removable"] != true && d["transport"] != "loop",
                "external" => d["removable"] == true,
                _ => d["removable"] != true && d["transport"] == "loop",
            })
            .collect();
        if disks.is_empty() {
            continue;
        }
        rows.push(v(
            kind,
            json!({"paddingHorizontal":10,"marginTop":16,"marginBottom":4}),
            vec![dim(key(kind), t)],
        ));
        for disk in disks {
            let selected = disk["name"] == app.selected && app.part.is_empty();
            let subtitle = joined(
                vec![
                    bytes(n(&disk["size"])),
                    bus(disk),
                    if disk["system"] == true {
                        key("systemShort")
                    } else {
                        Value::Null
                    },
                ],
                " · ",
            );
            rows.push(side_row(
                text(&disk["name"]),
                disk_title(disk),
                subtitle,
                drive_glyph(disk, 20.),
                selected,
                false,
                json!([disk["name"], ""]),
                t,
            ));
            for part in arr(&disk["partitions"]) {
                rows.push(side_row(
                    text(&part["name"]),
                    part_title(part),
                    joined(
                        vec![
                            part["name"].clone(),
                            bytes(n(&part["size"])),
                            part["mountpoint"].clone(),
                        ],
                        " · ",
                    ),
                    part_glyph(text(&part["fstype"]), 12.),
                    part["name"] == app.part,
                    true,
                    json!([disk["name"], part["name"]]),
                    t,
                ));
            }
        }
    }
    v(
        "sidebar",
        json!({"width":240,"flexShrink":0,"backgroundColor":t["sidebarBg"]}),
        vec![
            json!({"kind":"scroll","style":{"flex":1},"children":[v("disks",json!({"paddingHorizontal":10,"paddingBottom":16,"gap":2}),rows)]}),
        ],
    )
}
fn side_row(
    id: &str,
    title: Value,
    subtitle: Value,
    glyph: Value,
    selected: bool,
    indent: bool,
    selection: Value,
    t: &Value,
) -> Value {
    let mut main = label(title, t, false, 13);
    main["style"]["fontWeight"] = json!(if indent { 400 } else { 500 });
    main["style"]["whiteSpace"] = json!("nowrap");
    main["style"]["overflow"] = json!("hidden");
    let mut sub = dim(subtitle, t);
    sub["style"]["fontSize"] = json!(11);
    sub["style"]["whiteSpace"] = json!("nowrap");
    sub["style"]["overflow"] = json!("hidden");
    if selected {
        main["style"]["color"] = json!("#fff");
        sub["style"]["color"] = json!("rgba(255,255,255,0.8)");
    }
    let mut node = v(
        id,
        json!({"flexDirection":"row","alignItems":"center","gap":8,"minHeight":36,"paddingVertical":4,"paddingHorizontal":10,"borderRadius":6,"backgroundColor":if selected{t["accent"].clone()}else{json!("transparent")}}),
        vec![
            v(
                "indent",
                json!({"width":if indent{14}else{0},"flexShrink":0}),
                vec![],
            ),
            v(
                "glyph",
                json!({"width":22,"alignItems":"center"}),
                vec![glyph],
            ),
            v(
                "labels",
                json!({"flex":1,"minWidth":0,"gap":1}),
                vec![main, sub],
            ),
        ],
    );
    node["events"]["onClick"] = json!(["select", selection]);
    if !selected {
        node["hover"] = json!({"backgroundColor":t["hover"]});
    }
    node
}
fn toolbar(app: &mut App) -> Value {
    let t = app.theme.clone();
    let rtl = app.i18n.entry(&app.i18n.lang.clone(), "meta", "direction") == Some(json!("rtl"));
    let actions = app.actions();
    let mut buttons = vec![];
    let mut used = 0.;
    let room = (app.width - 240. - 68.).max(0.);
    let mut overflow = vec![];
    let mut menu_actions = vec![];
    for action in actions {
        let caption = key(match action {
            "image" => "makeBootable",
            "label" => "rename",
            other => other,
        });
        let width = axia_native::measure_text(&app.i18n.caption(&caption), 13., 500.) + 36.;
        if used + width <= room && overflow.is_empty() {
            used += width;
            buttons.push(button(caption, "act", json!(action), app.busy, false, &t));
        } else {
            menu_actions.push(action.to_owned());
            overflow.push(json!({"label":caption,"action":["act",action],"disabled":app.busy,"danger":action=="erase"}));
        }
    }
    if buttons.is_empty() && overflow.is_empty() {
        buttons.push(v(
            "policy",
            json!({"flex":1,"minWidth":0}),
            vec![dim(
                key(if app.disk()["system"] == true {
                    "systemNote"
                } else if app.disk()["ro"] == true {
                    "readOnlyNote"
                } else {
                    "adminNote"
                }),
                &t,
            )],
        ));
    }
    buttons.push(v("spacer", json!({"flex":1}), vec![]));
    if !overflow.is_empty() {
        buttons.push(button(json!("⋯"), "more", json!(""), app.busy, false, &t));
    }
    let mut children = vec![v(
        "toolbar",
        json!({"minHeight":44,"paddingHorizontal":16,"paddingVertical":8,"gap":8,"flexDirection":"row","alignItems":"center","borderBottomWidth":1,"borderBottomColor":t["separator"],"flexShrink":0}),
        buttons,
    )];
    app.menu_actions = if menu_actions.is_empty() {
        app.actions().into_iter().map(str::to_owned).collect()
    } else {
        menu_actions
    };
    if app.menu {
        let items = if overflow.is_empty() {
            app.actions().into_iter().map(|action|json!({"label":key(match action{"image"=>"makeBootable","label"=>"rename",other=>other}),"action":["act",action],"disabled":app.busy})).collect()
        } else {
            overflow
        };
        children.push(json!({"kind":"menu","x":if rtl{0.}else{(app.width-230.).max(0.)},"y":44,"items":items,"active":app.menu_active,"sub":-1,"layer":"popover","events":{"onDismiss":["menuDismiss"]}}));
    }
    v("toolbar-area", json!({"flexShrink":0}), children)
}
fn usage(disk: &Value, part: &Value, t: &Value) -> Value {
    let track = |children| {
        v(
            "track",
            json!({"height":18,"borderRadius":6,"backgroundColor":t["control"],"overflow":"hidden","flexDirection":"row","gap":1,"direction":"ltr"}),
            children,
        )
    };
    if part.is_object() {
        let mounted = !text(&part["mountpoint"]).is_empty() && part["used"].is_number();
        let size = n(&part["size"]).max(1.);
        let frac = (n(&part["used"]) / size).clamp(0., 1.);
        let segments = if mounted {
            vec![
                v(
                    "used",
                    json!({"flexGrow":frac,"backgroundColor":policy::fs_color(text(&part["fstype"]))}),
                    vec![],
                ),
                v("free", json!({"flexGrow":1.-frac}), vec![]),
            ]
        } else {
            vec![]
        };
        return v(
            "usage",
            json!({"gap":8}),
            vec![
                track(segments),
                dim(
                    if mounted {
                        tr(
                            "usedOf",
                            json!({"used":bytes(n(&part["used"])),"size":bytes(size),"free":bytes(n(&part["avail"]))}),
                        )
                    } else {
                        key("notMountedUsage")
                    },
                    t,
                ),
            ],
        );
    }
    let total = n(&disk["size"]).max(1.);
    let covered: f64 = arr(&disk["partitions"]).iter().map(|p| n(&p["size"])).sum();
    let mut segments = vec![];
    let mut legend = vec![];
    for part in arr(&disk["partitions"]) {
        segments.push(v(text(&part["name"]),json!({"flexGrow":n(&part["size"])/total,"backgroundColor":policy::fs_color(text(&part["fstype"]))}),vec![]));
        legend.push(row(
            text(&part["name"]),
            vec![
                part_glyph(text(&part["fstype"]), 8.),
                label(part_title(part), t, false, 11),
                label(bytes(n(&part["size"])), t, true, 11),
            ],
        ));
    }
    if segments.is_empty() && !text(&disk["fstype"]).is_empty() {
        segments.push(v(
            "whole",
            json!({"flexGrow":1,"backgroundColor":policy::fs_color(text(&disk["fstype"]))}),
            vec![],
        ));
    } else if covered < total {
        segments.push(v("free", json!({"flexGrow":(total-covered)/total}), vec![]));
    }
    if !arr(&disk["partitions"]).is_empty() && total - covered > 16. * 1048576. {
        legend.push(row(
            "free",
            vec![
                v(
                    "glyph",
                    json!({"width":8,"height":8,"borderRadius":2,"backgroundColor":t["control"]}),
                    vec![],
                ),
                label(
                    tr("unallocated", json!({"size":bytes(total-covered)})),
                    t,
                    true,
                    11,
                ),
            ],
        ));
    }
    v(
        "usage",
        json!({"gap":8}),
        vec![
            track(segments),
            v(
                "legend",
                json!({"flexDirection":"row","flexWrap":"wrap","gap":12}),
                legend,
            ),
        ],
    )
}
fn details(app: &App) -> Value {
    let t = &app.theme;
    let disk = app.disk();
    let part = app.partition();
    let target = policy::target(disk, part);
    let is_part = part.is_object();
    let subtitle = if is_part {
        joined(
            vec![
                part["name"].clone(),
                bytes(n(&part["size"])),
                if text(&part["fstype"]).is_empty() {
                    key("noFs")
                } else {
                    json!(policy::fs_name(text(&part["fstype"])))
                },
            ],
            " · ",
        )
    } else {
        joined(
            vec![
                disk["name"].clone(),
                bytes(n(&disk["size"])),
                bus(disk),
                if text(&disk["table"]).is_empty() {
                    Value::Null
                } else {
                    key(if disk["table"] == "gpt" {
                        "table.gpt"
                    } else {
                        "table.dos"
                    })
                },
            ],
            " · ",
        )
    };
    let mut heading = vec![
        if is_part {
            part_glyph(text(&part["fstype"]), 40.)
        } else {
            drive_glyph(disk, 52.)
        },
        v(
            "title",
            json!({"flex":1,"minWidth":0,"gap":2}),
            vec![
                label(
                    if is_part {
                        part_title(part)
                    } else {
                        disk_title(disk)
                    },
                    t,
                    false,
                    25,
                ),
                dim(subtitle, t),
            ],
        ),
    ];
    if disk["system"] == true {
        heading.push(v("badge",json!({"paddingHorizontal":8,"height":22,"justifyContent":"center","borderRadius":6,"backgroundColor":t["control"]}),vec![label(key("systemBadge"),t,false,11)]));
    }
    let mut body = vec![row("heading", heading), usage(disk, part, t)];
    if app.busy && app.dialog.is_none() {
        body.push(dim(key("working"), t));
    } else if !app.message.is_null() {
        body.push(dim(app.message.clone(), t));
    }
    let fields = if is_part {
        vec![
            info("device", json!(format!("/dev/{}", text(&part["name"]))), t),
            info(
                "fsType",
                if text(&part["fstype"]).is_empty() {
                    key("noFs")
                } else {
                    json!(format!(
                        "{}{}",
                        policy::fs_name(text(&part["fstype"])),
                        if text(&part["fsversion"]).is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", text(&part["fsversion"]))
                        }
                    ))
                },
                t,
            ),
            info("label", part["label"].clone(), t),
            info("uuid", part["uuid"].clone(), t),
            info("partName", part["partlabel"].clone(), t),
            info(
                "mountPoint",
                if text(&part["mountpoint"]).is_empty() {
                    key("notMounted")
                } else {
                    part["mountpoint"].clone()
                },
                t,
            ),
            info("offset", bytes(n(&part["start"])), t),
        ]
    } else {
        vec![
            info("device", json!(format!("/dev/{}", text(&disk["name"]))), t),
            info(
                "model",
                joined(vec![disk["vendor"].clone(), disk["model"].clone()], " "),
                t,
            ),
            info("serial", disk["serial"].clone(), t),
            info(
                "capacity",
                tr(
                    "bytes",
                    json!({"size":bytes(n(&disk["size"])),"n":disk["size"]}),
                ),
                t,
            ),
            info("connection", bus(disk), t),
            info(
                "kind",
                key(if disk["transport"] == "loop" {
                    "kindImage"
                } else if disk["removable"] == true {
                    "kindRemovable"
                } else if disk["rotational"] == true {
                    "kindHdd"
                } else {
                    "kindSsd"
                }),
                t,
            ),
            info(
                "partTable",
                if text(&disk["table"]).is_empty() {
                    key("noTable")
                } else {
                    key(if disk["table"] == "gpt" {
                        "table.gpt"
                    } else {
                        "table.dos"
                    })
                },
                t,
            ),
            info(
                "partitions",
                num(arr(&disk["partitions"]).len() as f64, 0),
                t,
            ),
        ]
    };
    body.push(group("details", Value::Null, Value::Null, fields, t));
    if !is_part {
        body.extend(health(app, disk, t));
    }
    if target["ro"] == true {
        body.push(dim(key("readOnlyNote"), t));
    }
    scroll(&format!("details-{}", text(&disk["name"])), body)
}
fn health_color(health: &str) -> &'static str {
    match health {
        "ok" => "#30d158",
        "warning" => "#ff9f0a",
        "failing" => "#ff453a",
        _ => "#8e8e93",
    }
}
fn hours(value: f64) -> Value {
    tr(
        "hours",
        json!({"count":value,"v":num(value,0),"d":num(value/24.,0)}),
    )
}
fn health_line(name: &str, value: Value, t: &Value) -> Option<Value> {
    (!value.is_null()).then(|| info(name, value, t))
}
fn full_health(m: &Value, t: &Value) -> Vec<Value> {
    let mut fields = vec![];
    let temperature = if m["temp"].is_null() {
        Value::Null
    } else if !m["tempMax"].is_null() {
        tr(
            "smart.tempWithLimit",
            json!({"v":m["temp"],"max":m["tempMax"],"crit":if m["tempCrit"].is_null(){json!("—")}else{m["tempCrit"].clone()}}),
        )
    } else {
        tr("celsius", json!({"v":m["temp"]}))
    };
    fields.push(health_line("temperature", temperature, t));
    fields.push(health_line(
        "powerOn",
        if m["hours"].is_null() {
            Value::Null
        } else {
            hours(n(&m["hours"]))
        },
        t,
    ));
    for (caption, name) in [
        ("powerCycles", "cycles"),
        ("reallocated", "reallocated"),
        ("pending", "pending"),
        ("uncorrectable", "uncorrectable"),
    ] {
        fields.push(health_line(
            caption,
            if m[name].is_null() {
                Value::Null
            } else {
                num(n(&m[name]), 0)
            },
            t,
        ));
    }
    fields.push(health_line(
        "smart.wear",
        if m["wear"].is_object() {
            tr("smart.wearValue", json!({"v":m["wear"]["used"]}))
        } else {
            Value::Null
        },
        t,
    ));
    let nvme = &m["nvme"];
    if nvme.is_object() {
        fields.push(health_line("spare",if nvme["spare"].is_null(){Value::Null}else{tr("spareValue",json!({"v":nvme["spare"],"min":if nvme["spareThreshold"].is_null(){json!("—")}else{nvme["spareThreshold"].clone()}}))},t));
        for (name, field) in [
            ("mediaErrors", "mediaErrors"),
            ("unsafeShutdowns", "unsafeShutdowns"),
        ] {
            fields.push(health_line(
                name,
                if nvme[field].is_null() {
                    Value::Null
                } else {
                    num(n(&nvme[field]), 0)
                },
                t,
            ));
        }
    }
    for (caption, name) in [("dataRead", "read"), ("dataWritten", "written")] {
        fields.push(health_line(
            caption,
            if m[name].is_null() {
                Value::Null
            } else {
                bytes(n(&m[name]))
            },
            t,
        ));
    }
    fields.push(health_line(
        "selfStatus",
        if m["passed"].is_boolean() {
            key(if m["passed"] == true {
                "statusOk"
            } else {
                "statusFail"
            })
        } else {
            Value::Null
        },
        t,
    ));
    fields.push(health_line(
        "smart.firmware",
        if text(&m["firmware"]).is_empty() {
            Value::Null
        } else {
            m["firmware"].clone()
        },
        t,
    ));
    let mut rows: Vec<Value> = fields.into_iter().flatten().collect();
    for row in &mut rows {
        let name = text(&row["key"]);
        let warning = match name {
            "reallocated" | "pending" | "uncorrectable" => n(&m[name]) > 0.,
            "mediaErrors" => n(&m["nvme"]["mediaErrors"]) > 0.,
            "smart.wear" => n(&m["wear"]["used"]) >= 90.,
            _ => false,
        };
        if warning || name == "selfStatus" && m["passed"] == false {
            row["children"][1]["children"][0]["style"]["color"] = json!(if name == "selfStatus" {
                "#ff453a"
            } else {
                "#ff9f0a"
            });
        }
    }
    rows
}
fn own_health(s: &Value, t: &Value) -> Vec<Value> {
    let mut rows = vec![];
    if s["temp"].as_f64().is_some_and(|n| n >= 0.) {
        rows.push(info(
            "temperature",
            tr("celsius", json!({"v":s["temp"]})),
            t,
        ));
    }
    if s["hours"].as_f64().is_some_and(|n| n >= 0.) {
        rows.push(info("powerOn", hours(n(&s["hours"])), t));
    }
    if s["cycles"].as_f64().is_some_and(|n| n >= 0.) {
        rows.push(info("powerCycles", num(n(&s["cycles"]), 0), t));
    }
    if s["kind"] == "ata" {
        for name in ["reallocated", "pending", "uncorrectable"] {
            if s[name].as_f64().is_some_and(|n| n >= 0.) {
                rows.push(info(name, num(n(&s[name]), 0), t));
            }
        }
        if s["status"] == 1 || s["status"] == 0 {
            rows.push(info(
                "selfStatus",
                key(if s["status"] == 1 {
                    "statusOk"
                } else {
                    "statusFail"
                }),
                t,
            ));
        }
    } else {
        for (caption, name) in [
            ("lifeUsed", "used"),
            ("spare", "spare"),
            ("dataRead", "read"),
            ("dataWritten", "written"),
            ("mediaErrors", "mediaErrors"),
            ("unsafeShutdowns", "unsafeShutdowns"),
        ] {
            if !s[name].as_f64().is_some_and(|n| n >= 0.) {
                continue;
            }
            let value = match name {
                "used" => tr("pctValue", json!({"v":s[name]})),
                "spare" => tr("spareValue", json!({"v":s[name],"min":s["spareThreshold"]})),
                "read" | "written" => bytes(n(&s[name])),
                _ => num(n(&s[name]), 0),
            };
            rows.push(info(caption, value, t));
        }
    }
    rows
}
fn reason_count(m: &Value, reason: &str) -> Value {
    match reason {
        "reallocated" | "pending" | "uncorrectable" => m[reason].clone(),
        "mediaErrors" => m["nvme"]["mediaErrors"].clone(),
        "worn" => m["wear"]["used"].clone(),
        "grownDefects" => m["grownDefects"].clone(),
        _ => json!(0),
    }
}
fn health(app: &App, disk: &Value, t: &Value) -> Vec<Value> {
    let name = text(&disk["name"]);
    let state = app.health.get(name).unwrap_or(&Value::Null);
    let m = &state["m"];
    let own = &state["summary"];
    let health = if m.is_object() {
        text(&m["health"])
    } else {
        text(&own["health"])
    };
    let loading = app.health_busy.iter().any(|d| d == name);
    let can = policy::own_smart(disk);
    let mut head = vec![];
    if !health.is_empty() {
        head.push(v(
            "dot",
            json!({"width":10,"height":10,"borderRadius":5,"backgroundColor":health_color(health)}),
            vec![],
        ));
    }
    let mut status = vec![label(
        if health.is_empty() {
            key("notChecked")
        } else {
            key(&format!("healthState.{health}"))
        },
        t,
        false,
        13,
    )];
    if m.is_object() && m["supported"] != true {
        status.push(dim(
            key(
                if arr(&m["messages"]).iter().any(|x| x["severity"] == "error") {
                    "smart.unsupportedBridge"
                } else {
                    "noSmart"
                },
            ),
            t,
        ));
    } else if !health.is_empty() {
        status.push(dim(key(&format!("healthHint.{health}")), t));
    } else if !can {
        status.push(dim(key("noSmart"), t));
    } else if loading {
        status.push(dim(key("reading"), t));
    } else if !state["error"].is_null() {
        status.push(dim(json!({"error":state["error"]}), t));
    }
    if !arr(&m["reasons"]).is_empty() {
        let mut reasons = dim(
            joined(
                arr(&m["reasons"])
                    .iter()
                    .map(|reason| {
                        tr(
                            &format!("reason.{}", text(reason)),
                            json!({"n":reason_count(m,text(reason))}),
                        )
                    })
                    .collect(),
                " · ",
            ),
            t,
        );
        reasons["style"]["color"] = json!(health_color(health));
        status.push(reasons);
    }
    head.push(v("status", json!({"flex":1,"gap":2,"minWidth":0}), status));
    if can {
        head.push(button(
            key(if state.is_object() && !loading {
                "checkAgain"
            } else {
                "checkHealth"
            }),
            "health",
            json!(""),
            loading,
            false,
            t,
        ));
    }
    let mut content = vec![row("health-head", head)];
    if m.is_object() {
        content.extend(full_health(m, t));
    } else if own.is_object() {
        content.extend(own_health(own, t));
        if !arr(&state["attrs"]).is_empty() {
            content.push(attributes(app, &json!({"attrs":state["attrs"]}), true, t));
        }
    }
    let footer = if state["ms"].is_number() {
        tr(
            if m.is_object() {
                "smart.readWith"
            } else {
                "readIn"
            },
            json!({"ms":format!("{} ms",state["ms"])}),
        )
    } else {
        Value::Null
    };
    let mut out = vec![group("health", key("health"), footer, content, t)];
    let installed = app.tools["smartctl"] == true || n(&app.tools["smartctl"]) > 0.;
    if can && !installed {
        out.push(group(
            "install",
            key("smart.advanced"),
            Value::Null,
            vec![
                dim(key("smart.installHint"), t),
                if app.admin {
                    v("no-admin", json!({}), vec![])
                } else {
                    dim(key("smart.installAdmin"), t)
                },
                button(
                    key(if app.installing {
                        "smart.installing"
                    } else {
                        "smart.install"
                    }),
                    "install",
                    json!(""),
                    !app.admin || app.installing,
                    false,
                    t,
                ),
            ],
            t,
        ));
    }
    if m["supported"] == true {
        out.push(self_tests(app, disk, m, t));
        out.push(error_log(m, t));
    }
    if m["tempHistory"].is_object() {
        out.push(temperature_history(&m["tempHistory"], &m["tempMax"], t));
    }
    if !arr(&m["attrs"]).is_empty() || m["nvme"].is_object() {
        out.push(attributes(app, m, false, t));
    }
    out
}
fn self_tests(app: &App, disk: &Value, m: &Value, t: &Value) -> Value {
    let state = &m["selfTest"];
    let running = state["running"] == true;
    let allowed = app.admin || disk["removable"] == true;
    let busy = app
        .health_busy
        .iter()
        .any(|name| name == text(&disk["name"]));
    let mut rows = vec![dim(key("smart.testHint"), t)];
    if running {
        rows.push(label(
            if state["remaining"].is_number() {
                tr(
                    "smart.testRunning",
                    json!({"pct":100.-n(&state["remaining"])}),
                )
            } else {
                key("smart.testRunningNoPct")
            },
            t,
            false,
            13,
        ));
        rows.push(progress_bar(
            if state["remaining"].is_number() {
                (100. - n(&state["remaining"])) / 100.
            } else {
                0.05
            },
            t,
        ));
    }
    if !allowed {
        rows.push(dim(key("adminNote"), t));
    }
    if state["supported"] != true {
        rows.push(dim(key("smart.testUnsupported"), t));
    } else {
        let mut buttons = vec![];
        for kind in if running {
            vec!["abort"]
        } else {
            vec!["short", "long"]
        } {
            let caption = if kind == "abort" {
                key("smart.testAbort")
            } else if state["polling"][kind].is_number() {
                tr(
                    if kind == "short" {
                        "smart.testShortMin"
                    } else {
                        "smart.testLongMin"
                    },
                    json!({"n":state["polling"][kind]}),
                )
            } else {
                key(if kind == "short" {
                    "smart.testShort"
                } else {
                    "smart.testLong"
                })
            };
            buttons.push(button(
                caption,
                "test",
                json!(kind),
                busy || !allowed,
                false,
                t,
            ));
        }
        rows.push(v(
            "controls",
            json!({"flexDirection":"row","flexWrap":"wrap","gap":8}),
            buttons,
        ));
    }
    for (i, result) in arr(&m["selfTests"]).iter().take(8).enumerate() {
        let name = if result["type"] == "other" {
            if text(&result["typeText"]).is_empty() {
                key("smart.testOther")
            } else {
                result["typeText"].clone()
            }
        } else {
            key(&format!("smart.test.{}", text(&result["type"])))
        };
        let value = joined(
            vec![
                if text(&result["resultKey"]).is_empty() {
                    result["result"].clone()
                } else {
                    key(&format!("smart.nvmeResult.{}", text(&result["resultKey"])))
                },
                if result["hours"].is_number() {
                    tr("smart.atHour", json!({"h":num(n(&result["hours"]),0)}))
                } else {
                    Value::Null
                },
            ],
            " · ",
        );
        let mut row = field(&i.to_string(), name, value, t);
        if result["passed"] != true {
            row["children"][1]["children"][0]["style"]["color"] = json!("#ff453a");
        }
        rows.push(row);
    }
    if arr(&m["selfTests"]).is_empty() {
        rows.push(info("smart.testResults", key("smart.testNone"), t));
    }
    group("tests", key("smart.tests"), Value::Null, rows, t)
}
fn progress_bar(fraction: f64, t: &Value) -> Value {
    v(
        "progress",
        json!({"height":8,"borderRadius":4,"backgroundColor":t["control"],"overflow":"hidden","flexDirection":"row","direction":"ltr"}),
        vec![
            v(
                "done",
                json!({"flexGrow":fraction.clamp(0.,1.),"backgroundColor":t["accent"]}),
                vec![],
            ),
            v("left", json!({"flexGrow":1.-fraction.clamp(0.,1.)}), vec![]),
        ],
    )
}
fn temperature_history(history: &Value, max: &Value, t: &Value) -> Value {
    let values = arr(&history["values"]);
    let values = &values[values.len().saturating_sub(128)..];
    let known: Vec<_> = values.iter().filter_map(Value::as_f64).collect();
    let lo = known.iter().copied().reduce(f64::min).unwrap_or(0.);
    let hi = known.iter().copied().reduce(f64::max).unwrap_or(0.);
    let lower = lo - 2.;
    let upper = hi.max(n(max)) + 2.;
    let bars=values.iter().enumerate().map(|(i,value)|v(&i.to_string(),json!({"flex":1,"height":if value.is_null(){1.}else{((n(value)-lower)/(upper-lower).max(1.)*54.).max(2.)},"borderRadius":1,"backgroundColor":if value.is_null(){t["separator"].clone()}else{json!(if max.is_number()&&n(value)>=n(max){"#ff453a"}else{"#ff9f0a"})}}),vec![])).collect();
    group(
        "temperature-history",
        key("smart.tempHistory"),
        tr(
            "smart.tempHistoryFoot",
            json!({"n":values.len(),"min":history["interval"],"lo":lo,"hi":hi}),
        ),
        vec![v(
            "bars",
            json!({"height":70,"flexDirection":"row","alignItems":"flex-end","gap":1,"paddingVertical":8,"direction":"ltr"}),
            bars,
        )],
        t,
    )
}
fn error_log(m: &Value, t: &Value) -> Value {
    let errors = &m["errors"];
    let mut rows = vec![info(
        "smart.errorCount",
        if n(&errors["count"]) > 0. {
            num(n(&errors["count"]), 0)
        } else {
            key("smart.errorNone")
        },
        t,
    )];
    if m["kind"] == "nvme" && n(&errors["count"]) > 0. {
        rows.push(dim(key("smart.nvmeErrorsHint"), t));
    }
    for (i, error) in arr(&errors["list"]).iter().take(6).enumerate() {
        rows.push(field(
            &i.to_string(),
            if error["hours"].is_number() {
                tr("smart.atHour", json!({"h":num(n(&error["hours"]),0)}))
            } else {
                json!(format!("#{}", error["n"].as_u64().unwrap_or(i as u64 + 1)))
            },
            error["text"].clone(),
            t,
        ));
    }
    group("errors", key("smart.errorLog"), Value::Null, rows, t)
}
fn nvme_rows(nvme: &Value, t: &Value) -> Vec<Value> {
    if !nvme.is_object() {
        return vec![];
    }
    let mut rows = vec![info(
        "nvme.criticalWarning",
        if n(&nvme["criticalWarning"]) > 0. {
            json!(format!(
                "0x{:02x}",
                nvme["criticalWarning"].as_u64().unwrap_or(0)
            ))
        } else {
            key("nvme.none")
        },
        t,
    )];
    for (name, field) in [
        ("temperature", "temp"),
        ("lifeUsed", "used"),
        ("dataRead", "read"),
        ("dataWritten", "written"),
        ("nvme.hostReads", "hostReads"),
        ("nvme.hostWrites", "hostWrites"),
        ("nvme.busy", "busyMinutes"),
        ("powerCycles", "cycles"),
        ("powerOn", "hours"),
        ("unsafeShutdowns", "unsafeShutdowns"),
        ("mediaErrors", "mediaErrors"),
        ("nvme.errorEntries", "errorLogEntries"),
        ("nvme.warnTemp", "warningTempMinutes"),
        ("nvme.critTemp", "criticalTempMinutes"),
    ] {
        if nvme[field].is_null() {
            continue;
        }
        let value = match field {
            "temp" => tr("celsius", json!({"v":nvme[field]})),
            "used" => tr("pctValue", json!({"v":nvme[field]})),
            "read" | "written" => bytes(n(&nvme[field])),
            "busyMinutes" | "warningTempMinutes" | "criticalTempMinutes" => {
                tr("nvme.minutes", json!({"v":num(n(&nvme[field]),0)}))
            }
            "hours" => hours(n(&nvme[field])),
            _ => num(n(&nvme[field]), 0),
        };
        rows.push(info(name, value, t));
    }
    if !arr(&nvme["sensors"]).is_empty() {
        rows.push(info(
            "nvme.sensors",
            joined(
                arr(&nvme["sensors"])
                    .iter()
                    .map(|temp| tr("celsius", json!({"v":temp})))
                    .collect(),
                " · ",
            ),
            t,
        ));
    }
    if !nvme["spare"].is_null() {
        rows.push(info("spare",tr("spareValue",json!({"v":nvme["spare"],"min":if nvme["spareThreshold"].is_null(){json!("—")}else{nvme["spareThreshold"].clone()}})),t));
    }
    rows
}
fn attributes(app: &App, m: &Value, own: bool, t: &Value) -> Value {
    let open = *app.attrs.get(&app.selected).unwrap_or(&false);
    let count = if !arr(&m["attrs"]).is_empty() {
        arr(&m["attrs"]).len()
    } else {
        nvme_rows(&m["nvme"], t).len()
    };
    let mut rows = vec![button(
        if open {
            key("hideAttrs")
        } else {
            tr("showAttrs", json!({"n":count}))
        },
        "attrs",
        json!(""),
        false,
        false,
        t,
    )];
    if open {
        if arr(&m["attrs"]).is_empty() {
            rows.extend(nvme_rows(&m["nvme"], t));
        } else {
            let cols = [
                ("attrName", 2.6),
                ("attrValue", 0.8),
                ("attrWorst", 0.8),
                ("attrThresh", 0.9),
                ("attrRaw", 1.6),
            ];
            rows.push(v(
                "columns",
                json!({"flexDirection":"row","gap":8,"paddingVertical":4}),
                cols.iter()
                    .map(|(name, flex)| {
                        v(
                            name,
                            json!({"flex":flex,"minWidth":0}),
                            vec![label(key(name), t, true, 11)],
                        )
                    })
                    .collect(),
            ));
            let known = [
                1, 3, 4, 5, 7, 9, 10, 12, 177, 183, 184, 187, 188, 190, 192, 193, 194, 196, 197,
                198, 199, 231, 233, 241, 242,
            ];
            for (i, a) in arr(&m["attrs"]).iter().enumerate() {
                let caption = if own {
                    if known.contains(&a["id"].as_u64().unwrap_or(0)) {
                        key(&format!("attr.a{}", a["id"]))
                    } else {
                        tr("attr.unknown", json!({"id":a["id"]}))
                    }
                } else if !text(&a["key"]).is_empty() {
                    key(&format!("attr.{}", text(&a["key"])))
                } else {
                    let pretty = smart("pretty", json!({"name":a["name"]}));
                    if text(&pretty).is_empty() {
                        tr("attr.unknown", json!({"id":a["id"]}))
                    } else {
                        pretty
                    }
                };
                let mut names = vec![label(
                    joined(
                        vec![
                            a["id"].clone(),
                            caption,
                            if a["prefail"] == true {
                                json!("*")
                            } else {
                                Value::Null
                            },
                        ],
                        " · ",
                    ),
                    t,
                    false,
                    11,
                )];
                if !own && !text(&a["key"]).is_empty() {
                    names.push(label(a["name"].clone(), t, true, 10));
                }
                let mut cells = vec![v("name", json!({"flex":2.6,"minWidth":0}), names)];
                for (field, flex) in [
                    ("value", 0.8),
                    ("worst", 0.8),
                    ("thresh", 0.9),
                    ("raw", 1.6),
                ] {
                    cells.push(v(
                        field,
                        json!({"flex":flex,"minWidth":0}),
                        vec![label(
                            if a[field].is_null() {
                                json!("—")
                            } else {
                                a[field].clone()
                            },
                            t,
                            false,
                            11,
                        )],
                    ));
                }
                let mut r = v(
                    &i.to_string(),
                    json!({"flexDirection":"row","gap":8,"paddingVertical":3}),
                    cells,
                );
                if a["failed"] == "now" || a["failing"] == true {
                    if let Some(children) = r["children"][0]["children"].as_array_mut() {
                        if let Some(first) = children.first_mut() {
                            first["style"]["color"] = json!("#ff453a");
                        }
                    }
                }
                rows.push(r);
            }
        }
    }
    group(
        if own { "own-attributes" } else { "attributes" },
        if own {
            Value::Null
        } else {
            key("smart.attributes")
        },
        if open && !arr(&m["attrs"]).is_empty() {
            key("prefailNote")
        } else {
            Value::Null
        },
        rows,
        t,
    )
}
fn dialog_title(dialog: &Dialog) -> Value {
    let name = if dialog.part.is_object() {
        part_title(&dialog.part)
    } else {
        disk_title(&dialog.disk)
    };
    tr(
        match dialog.kind.as_str() {
            "format" => "formatTitle",
            "erase" => "eraseTitle",
            "label" => "renameTitle",
            _ => "imageTitle",
        },
        json!({"name":name}),
    )
}
fn erased(dialog: &Dialog, t: &Value) -> Value {
    let disk = &dialog.disk;
    let part = &dialog.part;
    let mut rows = vec![label(
        if part.is_object() {
            tr(
                "confirm.part",
                json!({"part":part["name"],"disk":disk_title(disk),"size":bytes(n(&part["size"]))}),
            )
        } else {
            tr(
                "confirm.disk",
                json!({"disk":disk["name"],"model":disk_title(disk),"size":bytes(n(&disk["size"]))}),
            )
        },
        t,
        false,
        12,
    )];
    let parts = if part.is_object() {
        vec![part]
    } else {
        arr(&disk["partitions"]).iter().collect()
    };
    for p in parts.iter() {
        rows.push(row(
            text(&p["name"]),
            vec![
                part_glyph(text(&p["fstype"]), 10.),
                v(
                    "description",
                    json!({"flex":1,"minWidth":0}),
                    vec![label(
                        joined(
                            vec![
                                p["name"].clone(),
                                part_title(p),
                                if text(&p["fstype"]).is_empty() {
                                    key("noFs")
                                } else {
                                    json!(policy::fs_name(text(&p["fstype"])))
                                },
                                bytes(n(&p["size"])),
                                if text(&p["mountpoint"]).is_empty() {
                                    Value::Null
                                } else {
                                    tr("confirm.mountedAt", json!({"path":p["mountpoint"]}))
                                },
                                if p["used"].is_number() {
                                    tr("confirm.usedData", json!({"used":bytes(n(&p["used"]))}))
                                } else {
                                    Value::Null
                                },
                            ],
                            " · ",
                        ),
                        t,
                        false,
                        11,
                    )],
                ),
            ],
        ));
    }
    if parts.is_empty() {
        rows.push(dim(
            if text(&disk["fstype"]).is_empty() {
                key("confirm.empty")
            } else {
                joined(
                    vec![
                        json!(policy::fs_name(text(&disk["fstype"]))),
                        disk["label"].clone(),
                    ],
                    " · ",
                )
            },
            t,
        ));
    }
    let mut warning = dim(key("confirm.irreversible"), t);
    warning["style"]["color"] = json!("#ff453a");
    rows.push(warning);
    v(
        "erased",
        json!({"padding":10,"gap":6,"borderRadius":8,"backgroundColor":"rgba(255,69,58,0.10)","borderWidth":1,"borderColor":"rgba(255,69,58,0.35)"}),
        rows,
    )
}
fn dialog(app: &App, d: &Dialog, t: &Value) -> Value {
    let valid = app.dialog_valid(d);
    let mut rows = vec![];
    if d.step == "run" {
        if app.busy {
            if !app.progress.is_null() {
                let total = n(&app.progress["total"]);
                let frac = if total > 0. {
                    n(&app.progress["done"]) / total
                } else {
                    0.
                };
                rows.push(progress_bar(frac, t));
                if total > 0. {
                    rows.push(dim(tr("progress",json!({"done":bytes(n(&app.progress["done"])),"total":bytes(total),"pct":num((frac*100.).floor(),0)})),t));
                }
            } else {
                rows.push(dim(key("working"), t));
            }
        }
        if !app.job_error.is_null() {
            let mut error = dim(app.job_error.clone(), t);
            error["style"]["color"] = json!("#ff453a");
            rows.push(error);
        } else if !app.result.is_null() {
            let mut ok = dim(app.message.clone(), t);
            ok["style"]["color"] = json!("#30d158");
            rows.push(ok);
        }
        rows.push(row(
            "footer",
            if app.busy {
                if d.kind == "image" || d.kind == "erase" && d.mode == "zero" {
                    vec![button(
                        key("cancel"),
                        "cancelJob",
                        json!(""),
                        false,
                        false,
                        t,
                    )]
                } else {
                    vec![]
                }
            } else {
                vec![button(
                    key("done"),
                    "dialogClose",
                    json!(""),
                    false,
                    true,
                    t,
                )]
            },
        ));
    } else if d.step == "confirm" {
        if d.kind == "image" {
            rows.push(label(tr("imageWillWrite",json!({"file":policy::image_path(&d.path,&app.home).rsplit('/').next().unwrap_or(""),"size":bytes(n(&d.stat["size"]))})),t,false,12));
        }
        rows.push(erased(d, t));
        let typed = d.part.is_object() || d.typed.trim() == text(&d.disk["name"]);
        if !d.part.is_object() {
            rows.push(label(
                tr("confirm.typeName", json!({"name":d.disk["name"]})),
                t,
                false,
                12,
            ));
            rows.push(input(
                "typed",
                &d.typed,
                d.disk["name"].clone(),
                "typed",
                "confirm",
            ));
        }
        let mut go = button(
            key(match d.kind.as_str() {
                "format" => "formatGo",
                "erase" => "eraseGo",
                _ => "imageGo",
            }),
            "confirm",
            json!(""),
            !typed || !valid || app.busy,
            false,
            t,
        );
        go["destructive"] = json!(true);
        go["style"]["borderWidth"] = json!(1);
        go["style"]["borderColor"] = json!("#ff453a");
        go["style"]["backgroundColor"] = json!("rgba(255,69,58,0.10)");
        rows.push(row(
            "footer",
            vec![
                button(key("back"), "dialogBack", json!(""), false, false, t),
                go,
            ],
        ));
    } else {
        match d.kind.as_str() {
            "format" => {
                let formats = policy::formats(&app.tools);
                if formats.is_empty() {
                    rows.push(dim(key("noTools"), t));
                } else {
                    rows.push(dim(key("fsChoose"), t));
                    rows.push(row(
                        "formats",
                        formats
                            .iter()
                            .map(|kind| {
                                button(
                                    json!(policy::fs_name(kind)),
                                    "format",
                                    json!(kind),
                                    false,
                                    *kind == d.format,
                                    t,
                                )
                            })
                            .collect(),
                    ));
                    rows.push(dim(key(&format!("fsHint.{}", d.format)), t));
                    rows.push(input(
                        "label",
                        &d.label,
                        tr(
                            "labelPlaceholder",
                            json!({"n":policy::label_limit(&d.format)}),
                        ),
                        "label",
                        "dialogNext",
                    ));
                    if !policy::label_valid(&d.label, &d.format) {
                        rows.push(dim(
                            tr("badLabel", json!({"n":policy::label_limit(&d.format)})),
                            t,
                        ));
                    }
                    if !d.part.is_object() {
                        rows.push(dim(key("formatWholeHint"), t));
                    }
                }
            }
            "erase" => {
                rows.push(row(
                    "mode",
                    ["quick", "zero"]
                        .iter()
                        .map(|mode| {
                            button(
                                key(if *mode == "quick" {
                                    "eraseQuick"
                                } else {
                                    "eraseZero"
                                }),
                                "eraseMode",
                                json!(mode),
                                false,
                                *mode == d.mode,
                                t,
                            )
                        })
                        .collect(),
                ));
                rows.push(dim(key(&format!("eraseHint.{}", d.mode)), t));
            }
            "label" => {
                let kind = text(&d.part["fstype"]);
                rows.push(input(
                    "label",
                    &d.label,
                    tr("labelPlaceholder", json!({"n":policy::label_limit(kind)})),
                    "label",
                    "confirm",
                ));
                if !policy::label_valid(&d.label, kind) {
                    rows.push(dim(
                        tr("badLabel", json!({"n":policy::label_limit(kind)})),
                        t,
                    ));
                }
                if !text(&d.part["mountpoint"]).is_empty() && !kind.starts_with("ext") {
                    rows.push(dim(key("renameUnmount"), t));
                }
            }
            "image" => {
                rows.push(dim(key("imageHint"), t));
                rows.push(input(
                    "path",
                    &d.path,
                    key("imagePath"),
                    "imagePath",
                    "dialogNext",
                ));
                let selected = policy::image_path(&d.path, &app.home);
                let files = arr(&d.images)
                    .iter()
                    .map(|file| {
                        let chosen = file["path"] == selected;
                        let mut item = row(
                            text(&file["path"]),
                            vec![
                                v(
                                    "name",
                                    json!({"flex":1,"minWidth":0}),
                                    vec![label(file["name"].clone(), t, false, 12)],
                                ),
                                label(bytes(n(&file["size"])), t, true, 11),
                            ],
                        );
                        item["style"]["paddingVertical"] = json!(4);
                        item["style"]["paddingHorizontal"] = json!(8);
                        item["style"]["borderRadius"] = json!(6);
                        if chosen {
                            item["style"]["backgroundColor"] = t["accent"].clone();
                            item["children"][0]["children"][0]["style"]["color"] = json!("#fff");
                            item["children"][1]["style"]["color"] = json!("#fff");
                        }
                        item["events"]["onClick"] = json!(["imagePath", file["path"]]);
                        item
                    })
                    .collect();
                rows.push(if arr(&d.images).is_empty(){dim(key("imageNone"),t)}else{json!({"kind":"scroll","style":{"maxHeight":150},"children":[v("files",json!({"gap":2}),files)]})});
                if d.stat.is_object() && n(&d.stat["size"]) > n(&d.disk["size"]) {
                    rows.push(dim(tr("imageTooBig",json!({"size":bytes(n(&d.stat["size"])),"disk":bytes(n(&d.disk["size"]))})),t));
                } else if !d.path.is_empty() && d.stat.is_null() {
                    rows.push(dim(key("imageMissing"), t));
                }
            }
            _ => {}
        }
        rows.push(row(
            "footer",
            vec![
                button(key("cancel"), "dialogClose", json!(""), false, false, t),
                button(
                    key(if d.kind == "label" {
                        "rename"
                    } else {
                        "continue"
                    }),
                    "dialogNext",
                    json!(""),
                    !valid,
                    false,
                    t,
                ),
            ],
        ));
    }
    v(
        "dialog-overlay",
        json!({"position":"absolute","left":0,"right":0,"top":0,"bottom":0,"backgroundColor":"rgba(0,0,0,0.35)","alignItems":"center","justifyContent":"center"}),
        vec![v(
            "dialog",
            json!({"width":480,"maxHeight":"90%","borderRadius":12,"backgroundColor":t["windowBg"],"borderWidth":1,"borderColor":t["separator"],"boxShadow":"0 12px 40px rgba(0,0,0,0.45)","padding":20,"gap":12}),
            vec![
                label(dialog_title(d), t, false, 15),
                json!({"kind":"scroll","key":"dialog-scroll","style":{"flexShrink":1,"minHeight":0},"children":[v("body",json!({"gap":10}),rows)]}),
            ],
        )],
    )
}
pub fn tree(app: &mut App) -> Value {
    let t = app.theme.clone();
    let body = if !app.ready {
        v(
            "loading",
            json!({"flex":1,"alignItems":"center","justifyContent":"center"}),
            vec![dim(key("working"), &t)],
        )
    } else if !app.available {
        v(
            "unavailable",
            json!({"flex":1,"alignItems":"center","justifyContent":"center"}),
            vec![label(key("unavailable"), &t, true, 14)],
        )
    } else {
        let detail = if app.disk().is_object() {
            let toolbar = toolbar(app);
            v(
                "content",
                json!({"flex":1,"minWidth":0}),
                vec![toolbar, details(app)],
            )
        } else {
            v(
                "empty",
                json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":12}),
                vec![icon(56.), label(key("none"), &t, true, 14)],
            )
        };
        v(
            "layout",
            json!({"flex":1,"flexDirection":"row","minHeight":0,"alignItems":"stretch"}),
            vec![sidebar(app), detail],
        )
    };
    let mut children = vec![body];
    if let Some(d) = app.dialog.as_ref() {
        children.push(dialog(app, d, &t));
    }
    let rtl = app.i18n.entry(&app.i18n.lang.clone(), "meta", "direction") == Some(json!("rtl"));
    let mut root = v(
        "disks",
        json!({"flex":1,"backgroundColor":t["windowBg"],"direction":if rtl{"rtl"}else{"ltr"}}),
        children,
    );
    normalize_labels(&mut root);
    root
}
pub fn icon(size: f64) -> Value {
    v(
        "disks-icon",
        json!({"width":size,"height":size,"borderRadius":size*0.225,"overflow":"hidden","alignItems":"center","justifyContent":"center","background":"linear-gradient(160deg, #5e6b7d, #2b323d)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",size*0.04,size*0.12)}),
        vec![v(
            "case",
            json!({"width":size*0.7,"height":size*0.56,"borderRadius":size*0.08,"background":"linear-gradient(180deg, #f2f4f7, #b9c0ca)","alignItems":"center","justifyContent":"center","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.4)",size*0.02,size*0.05)}),
            vec![
                v(
                    "platter",
                    json!({"width":size*0.4,"height":size*0.4,"borderRadius":size*0.2,"background":"linear-gradient(140deg, #ffffff, #8f9aa8)","borderWidth":(size*0.02).max(1.),"borderColor":"#7d8794","alignItems":"center","justifyContent":"center"}),
                    vec![v(
                        "center",
                        json!({"width":size*0.1,"height":size*0.1,"borderRadius":size*0.05,"backgroundColor":"#5b6573"}),
                        vec![],
                    )],
                ),
                v(
                    "arm",
                    json!({"position":"absolute","right":size*0.08,"top":size*0.08,"width":size*0.05,"height":size*0.26,"borderRadius":size*0.025,"backgroundColor":"#4b5563"}),
                    vec![],
                ),
                v(
                    "light",
                    json!({"position":"absolute","left":size*0.06,"bottom":size*0.05,"width":size*0.06,"height":size*0.06,"borderRadius":size*0.03,"backgroundColor":"#30d158"}),
                    vec![],
                ),
            ],
        )],
    )
}
