use super::{arr, camel, check, key, model, sharing, text, App};
use serde_json::{json, Value};

fn v(id: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":id,"style":style,"children":children})
}
fn label(caption: Value, t: &Value, dim: bool, size: u64) -> Value {
    json!({"kind":"text","label":caption,"style":{"fontSize":size,"color":t[if dim{"textDim"}else{"text"}],"textAlign":"start","lineHeight":1.4}})
}
fn row(id: &str, children: Vec<Value>) -> Value {
    v(
        id,
        json!({"flexDirection":"row","alignItems":"center","gap":10}),
        children,
    )
}
fn spacer() -> Value {
    v("spacer", json!({"flex":1}), vec![])
}
fn button(caption: Value, cmd: &str, arg: &str, selected: bool, disabled: bool) -> Value {
    json!({"kind":"button","key":format!("{cmd}:{arg}"),"label":caption,"action":[cmd,arg],"primary":selected,"disabled":disabled})
}
fn highlight_buttons(node: &mut Value, t: &Value) {
    if node["kind"] == "button" && node["primary"] == true {
        node["style"] = json!({"borderWidth":1,"borderColor":t["accent"],"backgroundColor":format!("{}22",t["accent"].as_str().unwrap_or("#0a84ff"))});
    }
    if let Some(children) = node["children"].as_array_mut() {
        for child in children {
            highlight_buttons(child, t);
        }
    }
}
fn scroll(id: &str, children: Vec<Value>) -> Value {
    json!({"kind":"scroll","key":id,"style":{"flex":1,"minWidth":0},"children":[v("padding",json!({"padding":24,"gap":20}),children)]})
}
fn group(id: &str, title: Value, footer: Value, children: Vec<Value>, t: &Value) -> Value {
    let mut body = vec![];
    if !title.is_null() {
        body.push(label(title, t, true, 12));
    }
    body.push(v("body",json!({"padding":12,"borderRadius":10,"borderWidth":1,"borderColor":t["separator"],"gap":6}),children));
    if !footer.is_null() {
        body.push(label(footer, t, true, 12));
    }
    v(id, json!({"gap":8}), body)
}
fn kv(id: &str, caption: Value, value: Value, t: &Value) -> Value {
    row(
        id,
        vec![
            v(
                "name",
                json!({"width":170,"flexShrink":0}),
                vec![label(caption, t, true, 13)],
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
fn field(id: &str, value: Value, t: &Value) -> Value {
    kv(id, leaf_caption(id), value, t)
}
fn bytes(value: f64) -> Value {
    json!({"bytes":value})
}
fn number(value: f64, digits: u32) -> Value {
    json!({"number":value,"digits":digits})
}
fn joined(parts: Vec<Value>, separator: &str) -> Value {
    let mut out = vec![];
    for part in parts.into_iter().filter(|p| !p.is_null() && p != "") {
        if !out.is_empty() {
            out.push(json!(separator));
        }
        out.push(part);
    }
    json!({"parts":out})
}
fn color(state: &str) -> &'static str {
    match state {
        "yes" | "works" | "pass" => "#30d158",
        "limits" | "warn" => "#ff9f0a",
        "no" | "fail" => "#ff453a",
        "info" => "#0a84ff",
        _ => "#8e8e93",
    }
}
fn state_row(
    id: &str,
    caption: Value,
    state: &str,
    detail: Value,
    namespace: &str,
    t: &Value,
) -> Value {
    let mut state_label = label(key(&format!("sysinfo.{namespace}.{state}")), t, false, 12);
    state_label["style"]["color"] = json!(color(state));
    row(
        id,
        vec![
            v(
                "dot",
                json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":color(state),"flexShrink":0}),
                vec![],
            ),
            v(
                "name",
                json!({"width":160,"flexShrink":0}),
                vec![label(caption, t, false, 13)],
            ),
            v(
                "state",
                json!({"width":110,"flexShrink":0}),
                vec![state_label],
            ),
            v(
                "detail",
                json!({"flex":1,"minWidth":0}),
                vec![label(detail, t, true, 12)],
            ),
        ],
    )
}
fn title(caption: Value, t: &Value) -> Value {
    label(caption, t, false, 22)
}
fn yes_no(value: &Value) -> Value {
    if value.is_null() {
        Value::Null
    } else {
        key(if value == true {
            "sysinfo.yes"
        } else {
            "sysinfo.no"
        })
    }
}
fn card(id: &str, value: Value, sub: Value, t: &Value) -> Value {
    v(
        id,
        json!({"flex":1,"minWidth":0,"padding":14,"minHeight":104,"borderWidth":1,"borderColor":t["separator"],"borderRadius":10,"gap":5}),
        vec![
            label(
                key(&if id == "isolation" {
                    "sysinfo.isolation".into()
                } else {
                    format!("sysinfo.section.{id}")
                }),
                t,
                true,
                12,
            ),
            label(value, t, false, 19),
            label(sub, t, true, 12),
        ],
    )
}
fn cpu_name(s: &str) -> String {
    s.replace("(R)", "")
        .replace("(TM)", "")
        .split_whitespace()
        .filter(|word| !["CPU", "Processor"].contains(word))
        .collect::<Vec<_>>()
        .join(" ")
}
fn overview(app: &App) -> Value {
    let t = &app.theme;
    let r = &app.snapshot["report"];
    let cpu = &r["cpu"];
    let sw = &r["software"];
    let session = &r["session"];
    let gpu = arr(&r["gpus"])
        .iter()
        .find(|g| g["connected"].as_i64().is_some_and(|n| n > 0))
        .or_else(|| arr(&r["gpus"]).first());
    let gpu_name = gpu
        .map(|g| joined(vec![g["vendor"].clone(), g["device"].clone()], " "))
        .unwrap_or(key("sysinfo.none"));
    let name = model("machineName", json!([r]));
    let mut children = vec![row(
        "heading",
        vec![
            icon(64.),
            v(
                "machine",
                json!({"flex":1,"gap":4}),
                vec![
                    title(
                        if name == "" {
                            key("sysinfo.thisComputer")
                        } else {
                            name
                        },
                        t,
                    ),
                    label(
                        joined(vec![sw["lordos"].clone(), sw["os"].clone()], " · "),
                        t,
                        true,
                        12,
                    ),
                ],
            ),
        ],
    )];
    if session["fixture"] == true {
        children.push(label(key("sysinfo.fixture"), t, true, 12));
    }
    children.push(row("cards1",vec![
        card("cpu",json!(cpu_name(text(&cpu["model"]))),json!({"key":"sysinfo.coresThreads","params":{"cores":cpu["cores"],"threads":cpu["threads"]}}),t),
        card("memory",bytes(r["memory"]["totalKB"].as_f64().unwrap_or(0.)*1024.),
            if r["memory"]["swapKB"].as_f64().unwrap_or(0.)>0. {json!({"key":"sysinfo.withSwap","params":{"size":bytes(r["memory"]["swapKB"].as_f64().unwrap_or(0.)*1024.)}})} else {Value::Null},t),
        card("graphics",gpu_name,if session["composition"]=="gpu" {key("sysinfo.composedGpu")} else {json!({"key":"sysinfo.composedCpu","params":{"reason":key(&format!("sysinfo.reason.{}",camel(text(&model("compositionReason",json!([session]))))))}})},t)]));
    children.push(row(
        "cards2",
        vec![
            card(
                "displays",
                json!(arr(&r["displays"])
                    .iter()
                    .map(|d| text(&d["name"]))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")),
                json!(arr(&r["displays"])
                    .iter()
                    .map(|d| format!("{} × {}", d["nativeWidth"], d["nativeHeight"]))
                    .collect::<Vec<_>>()
                    .join(", ")),
                t,
            ),
            card(
                "storage",
                bytes(
                    arr(&r["storage"])
                        .iter()
                        .filter_map(|d| d["bytes"].as_f64())
                        .sum(),
                ),
                json!({"key":"sysinfo.disks","params":{"count":arr(&r["storage"]).len()}}),
                t,
            ),
            card(
                "isolation",
                if sw["landlockAbi"].as_u64().unwrap_or(0) > 0 {
                    json!({"key":"sysinfo.landlock","params":{"abi":sw["landlockAbi"]}})
                } else {
                    key("sysinfo.landlockUnknown")
                },
                joined(
                    vec![
                        if sw["bpfLsm"] == true {
                            key("sysinfo.bpfLsm")
                        } else {
                            Value::Null
                        },
                        if sw["cgroup2"] == true {
                            key("sysinfo.cgroup2")
                        } else {
                            Value::Null
                        },
                    ],
                    " · ",
                ),
                t,
            ),
        ],
    ));
    scroll("overview", children)
}
fn formatted(field: &str, value: &Value) -> Value {
    if value.is_boolean() {
        return yes_no(value);
    }
    if let Some(n) = value.as_f64() {
        if field.ends_with("KB") {
            return bytes(n * 1024.);
        }
        if field.ends_with("MB") {
            return bytes(n * 1048576.);
        }
        if field == "bytes" {
            return bytes(n);
        }
        if field.ends_with("MHz") {
            return json!({"parts":[number(n/1000.,2)," GHz"]});
        }
        if field.ends_with("C") {
            return json!({"parts":[number(if field=="milliC" {n/1000.} else {n},1)," °C"]});
        }
        return number(n, if n.fract() == 0. { 0 } else { 2 });
    }
    value.clone()
}
fn hardware_details(section: &str, r: &Value, t: &Value) -> Vec<Value> {
    let list = arr(&r[section]);
    let rows=match section {
        "cpu"=>{
            let c=&r["cpu"];let mut rows=vec![field("model",json!(cpu_name(text(&c["model"]))),t),field("vendor",c["vendor"].clone(),t),field("arch",c["arch"].clone(),t),
                field("layout",json!({"key":"sysinfo.layout","params":{"sockets":c["sockets"],"cores":c["cores"],"threads":c["threads"]}}),t),
                field("frequency",if c["maxMHz"].as_f64().unwrap_or(0.)>0. {json!({"key":"sysinfo.freqRange","params":{"min":number(c["minMHz"].as_f64().unwrap_or(0.)/1000.,2),"max":number(c["maxMHz"].as_f64().unwrap_or(0.)/1000.,2)}})} else {Value::Null},t),field("governor",c["governor"].clone(),t)];
            let caches=[("l1dKB","L1d"),("l1iKB","L1i"),("l2KB","L2"),("l3KB","L3")].into_iter().filter(|(k,_)|c[*k].as_f64().unwrap_or(0.)>0.).map(|(k,n)|json!({"parts":[format!("{n} "),c[k]," KB"]})).collect();
            rows.extend([field("cache",joined(caches," · "),t),field("instructions",c["flags"].clone(),t),field("microcode",c["microcode"].clone(),t)]);rows
        },
        "displays"=>return list.iter().map(|d|group(text(&d["connector"]),d["connector"].clone(),Value::Null,vec![
            field("monitor",joined(vec![d["name"].clone(),d["manufacturer"].clone()]," · "),t),
            field("native",if d["nativeWidth"].as_u64().unwrap_or(0)>0 {json!(format!("{} × {}",d["nativeWidth"],d["nativeHeight"]))} else {d["mode"].clone()},t),
            field("size",if d["diagonalTenths"].as_f64().unwrap_or(0.)>0. {json!({"key":"sysinfo.diagonal","params":{"inch":number(d["diagonalTenths"].as_f64().unwrap_or(0.)/10.,1),"w":d["widthCm"],"h":d["heightCm"]}})} else {Value::Null},t),field("year",d["year"].clone(),t),field("card",d["card"].clone(),t)],t)).collect(),
        "storage"=>return list.iter().map(|d|group(text(&d["name"]),d["name"].clone(),Value::Null,vec![field("model",joined(vec![d["vendor"].clone(),d["model"].clone()]," "),t),field("size",bytes(d["bytes"].as_f64().unwrap_or(0.)),t),
            field("connection",joined(vec![if d["transport"].is_string() {key(&format!("sysinfo.bus.{}",text(&d["transport"])))} else {Value::Null},d["link"].clone()]," · "),t),
            field("kind",key(if d["removable"]==true {"sysinfo.removable"} else if d["rotational"]==true {"sysinfo.hdd"} else {"sysinfo.ssd"}),t)],t)).collect(),
        "network"=>list.iter().enumerate().map(|(i,n)|kv(&i.to_string(),key(&format!("sysinfo.net.{}",text(&n["kind"]))),joined(vec![n["driver"].clone(),
            if n["up"]==true {if n["speedMbps"].as_u64().unwrap_or(0)>0 {json!({"key":"sysinfo.upAt","params":{"mbps":n["speedMbps"]}})} else {key("sysinfo.up")}} else {key("sysinfo.down")}]," · "),t)).collect(),
        "audio"=>list.iter().enumerate().map(|(i,a)|kv(&i.to_string(),key("sysinfo.f.card"),a.clone(),t)).collect(),
        "bluetooth"=>list.iter().enumerate().map(|(i,a)|kv(&i.to_string(),key("sysinfo.f.controller"),a["driver"].clone(),t)).collect(),
        "power"=>return list.iter().enumerate().map(|(i,p)|group(&i.to_string(),key(&format!("sysinfo.power.{}",text(&p["kind"]))),Value::Null,
            if p["kind"]=="battery" {vec![field("model",joined(vec![p["vendor"].clone(),p["model"].clone()]," "),t),field("technology",p["technology"].clone(),t),
                field("charge",if p["capacity"].is_number() {json!({"parts":[p["capacity"]," %"]})} else {Value::Null},t),
                field("health",if p["health"].is_number() {json!({"key":"sysinfo.health","params":{"health":p["health"]}})} else {Value::Null},t),field("status",p["status"].clone(),t)]}
            else {vec![field("plugged",yes_no(&p["online"]),t)]},t)).collect(),
        "thermal"=>list.iter().enumerate().map(|(i,x)|kv(&i.to_string(),x["sensor"].clone(),joined(vec![if x["milliC"].is_number() {formatted("milliC",&x["milliC"])} else {Value::Null},
            if x["fans"].as_u64().unwrap_or(0)>0 {json!({"key":"sysinfo.fans","params":{"count":x["fans"]}})} else {Value::Null}]," · "),t)).collect(),
        "pci"|"usb"=>list.iter().enumerate().map(|(i,d)|{
            let name=if section=="pci" {joined(vec![d["vendor"].clone(),d["device"].clone()]," ")} else {joined(vec![d["vendor"].clone(),d["product"].clone()]," ")};
            let caption=if section=="pci" {if d["className"].is_string() {d["className"].clone()} else {d["class"].clone()}} else if d["speedMbps"].as_f64().unwrap_or(0.)>0. {json!({"key":"sysinfo.usbSpeed","params":{"mbps":d["speedMbps"]}})} else {json!("USB")};
            let name=if name["parts"].as_array().is_some_and(Vec::is_empty) {json!(format!("{}:{}",text(&d["vendorId"]),text(&d[if section=="pci" {"deviceId"} else {"productId"}])))} else {name};
            kv(&i.to_string(),caption,joined(vec![name,d["driver"].clone()]," · "),t)
        }).collect(),
        "software"=>{
            let sw=&r["software"];let mut rows=vec![field("system",sw["os"].clone(),t),field("kernel",sw["kernel"].clone(),t),field("lsm",sw["lsm"].clone(),t),field("landlock",sw["landlockAbi"].clone(),t),field("cgroup2",yes_no(&sw["cgroup2"]),t)];
            if sw["bootMedium"].is_string() {rows.push(field("boot",sharing::summary_caption("boot",text(&sw["bootMedium"]),&json!({"system":r})),t));}
            if let Some(services)=sw["services"].as_object() {rows.push(group("services",key("sysinfo.services"),Value::Null,services.iter().map(|(name,on)|kv(name,json!(name),key(if on==true {"sysinfo.running"} else {"sysinfo.notRunning"}),t)).collect(),t));}
            if let Some(config)=sw["kernelConfig"].as_object() {rows.push(group("kernel",key("sysinfo.kernelOptions"),Value::Null,config.iter().map(|(name,value)|kv(name,json!(name.trim_start_matches("CONFIG_")),key(&format!("sysinfo.kconf.{}",text(value))),t)).collect(),t));}rows
        },
        "raspberryPi"=>{
            let pi=&r[section];let state=model("piStatus",json!([pi]));let mut rows=object_rows(pi,t);
            for name in ["power","throttling"] {if state[name].is_string() {rows.push(field(name,key(&format!("sysinfo.pi.{}",text(&state[name]))),t));}}rows
        },
        _=>return structured(&r[section],t),
    };
    vec![group(section, Value::Null, Value::Null, rows, t)]
}
fn leaf_caption(field: &str) -> Value {
    let alias = match field {
        "totalKB" => "total",
        "swapKB" => "swap",
        "bytes" => "size",
        "maxMHz" => "frequency",
        "governor" => "governor",
        "boardVendor" | "biosVendor" => "vendor",
        "boardName" => "product",
        "boardVersion" | "biosVersion" => "version",
        "biosDate" => "date",
        "landlockAbi" => "landlock",
        "os" => "system",
        "flags" => "instructions",
        _ => field,
    };
    if [
        "arch",
        "board",
        "boot",
        "bootloaderDate",
        "bootloaderVersion",
        "cache",
        "card",
        "case",
        "cgroup2",
        "charge",
        "connection",
        "controller",
        "date",
        "firmware",
        "frequency",
        "governor",
        "health",
        "instructions",
        "kernel",
        "kind",
        "landlock",
        "layout",
        "lsm",
        "microcode",
        "model",
        "monitor",
        "native",
        "plugged",
        "power",
        "product",
        "revision",
        "secureBoot",
        "size",
        "status",
        "swap",
        "system",
        "technology",
        "throttling",
        "total",
        "vendor",
        "version",
        "year",
        "zram",
        "madeBy",
        "chip",
        "boardMemory",
    ]
    .contains(&alias)
    {
        key(&format!("sysinfo.f.{alias}"))
    } else {
        json!(humanize(alias))
    }
}
fn object_rows(value: &Value, t: &Value) -> Vec<Value> {
    let mut rows = vec![];
    if let Some(object) = value.as_object() {
        for (name, value) in object {
            if value.is_array() || value.is_object() {
                continue;
            }
            rows.push(kv(name, leaf_caption(name), formatted(name, value), t));
        }
        for (name, value) in object {
            if value.is_object() || value.is_array() {
                rows.push(group(
                    name,
                    json!(humanize(name)),
                    Value::Null,
                    structured(value, t),
                    t,
                ));
            }
        }
    } else {
        rows.push(label(formatted("", value), t, false, 13));
    }
    rows
}
fn humanize(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if i == 0 {
            out.extend(ch.to_uppercase());
        } else {
            if ch.is_uppercase() {
                out.push(' ');
            }
            out.push(ch);
        }
    }
    out
}
fn structured(value: &Value, t: &Value) -> Vec<Value> {
    if let Some(values) = value.as_array() {
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                group(
                    &index.to_string(),
                    Value::Null,
                    Value::Null,
                    object_rows(value, t),
                    t,
                )
            })
            .collect()
    } else {
        object_rows(value, t)
    }
}
fn layer_detail(layer: &Value) -> Value {
    if !layer["note"].is_null() {
        return json!({"key":format!("sysinfo.note.{}",camel(text(&layer["note"]))),"params":layer["vars"]});
    }
    match text(&layer["id"]) {
        "device" => layer["vars"]["name"].clone(),
        "driver" => layer["vars"]["driver"].clone(),
        "kms" | "egl" => {
            json!({"key":format!("sysinfo.detail.{}",text(&layer["id"])),"params":layer["vars"]})
        }
        _ => Value::Null,
    }
}
fn hardware(app: &App) -> Value {
    let t = &app.theme;
    let report = &app.snapshot["report"];
    let sections = model("hardwareSections", json!([report]));
    let mut children = vec![title(key(&format!("sysinfo.section.{}", app.section)), t)];
    match app.section.as_str() {
        "computer" => {
            let machine = &report["machine"];
            let pi = &report["raspberryPi"];
            let board = model("piBoard", json!([pi["revision"]]));
            let kind = if machine["virtualization"].is_string() && machine["virtualization"] != "" {
                json!({"key":"sysinfo.kind.vm","params":{"name":machine["virtualization"]}})
            } else if machine["chassis"].is_string() {
                key(&format!(
                    "sysinfo.kind.{}",
                    camel(text(&machine["chassis"]))
                ))
            } else if pi.is_object() {
                key("sysinfo.share.kind.board")
            } else {
                Value::Null
            };
            let product = if machine["product"].is_string() {
                machine["product"].clone()
            } else {
                board["product"].clone()
            };
            children.push(group(
                "computer",
                key("sysinfo.group.computer"),
                Value::Null,
                vec![
                    field(
                        "vendor",
                        if machine["vendor"].is_string() {
                            machine["vendor"].clone()
                        } else if pi.is_object() {
                            json!("Raspberry Pi Ltd")
                        } else {
                            Value::Null
                        },
                        t,
                    ),
                    field(
                        "product",
                        joined(vec![product, machine["version"].clone()], " "),
                        t,
                    ),
                    field("model", machine["model"].clone(), t),
                    field("kind", kind, t),
                ],
                t,
            ));
            if !report["raspberryPi"].is_null() {
                children.push(group(
                    "board",
                    key("sysinfo.group.board"),
                    Value::Null,
                    object_rows(&board, t),
                    t,
                ));
                children.push(group(
                    "bootloader",
                    key("sysinfo.group.bootloader"),
                    Value::Null,
                    vec![
                        field("revision", pi["revision"].clone(), t),
                        field(
                            "bootloaderVersion",
                            json!(text(&pi["bootloaderVersion"])
                                .chars()
                                .take(12)
                                .collect::<String>()),
                            t,
                        ),
                        field(
                            "bootloaderDate",
                            app.run_times
                                .get(text(&pi["bootloaderDate"]))
                                .unwrap_or(&pi["bootloaderDate"])
                                .clone(),
                            t,
                        ),
                    ],
                    t,
                ));
            } else {
                children.push(group(
                    "firmware",
                    key("sysinfo.group.firmware"),
                    Value::Null,
                    vec![
                        field(
                            "kind",
                            if machine["firmware"].is_string() {
                                key(&format!("sysinfo.firmware.{}", text(&machine["firmware"])))
                            } else {
                                Value::Null
                            },
                            t,
                        ),
                        field("vendor", machine["biosVendor"].clone(), t),
                        field("version", machine["biosVersion"].clone(), t),
                        field(
                            "date",
                            app.run_times
                                .get(text(&machine["biosDate"]))
                                .unwrap_or(&machine["biosDate"])
                                .clone(),
                            t,
                        ),
                        field(
                            "secureBoot",
                            if machine["secureBoot"].is_null() {
                                Value::Null
                            } else {
                                key(if machine["secureBoot"] == true {
                                    "sysinfo.on"
                                } else {
                                    "sysinfo.off"
                                })
                            },
                            t,
                        ),
                    ],
                    t,
                ));
            }
            if ["boardVendor", "boardName", "boardVersion"]
                .iter()
                .any(|name| machine[*name].is_string())
            {
                children.push(group(
                    "mainboard",
                    key("sysinfo.group.board"),
                    Value::Null,
                    vec![
                        field("vendor", machine["boardVendor"].clone(), t),
                        field("product", machine["boardName"].clone(), t),
                        field("version", machine["boardVersion"].clone(), t),
                    ],
                    t,
                ));
            }
            if !app.snapshot["case"].is_null() {
                children.push(field(
                    "case",
                    key(&format!(
                        "sysinfo.case.{}",
                        camel(text(&app.snapshot["case"]))
                    )),
                    t,
                ));
            }
            let mut serials: Vec<_> = arr(&app.serials)
                .iter()
                .map(|serial| {
                    kv(
                        text(&serial["kind"]),
                        key(&format!("sysinfo.serial.{}", text(&serial["kind"]))),
                        serial["value"].clone(),
                        t,
                    )
                })
                .collect();
            if app.serials.is_array() && arr(&app.serials).is_empty() {
                serials.push(label(key("sysinfo.serial.none"), t, true, 12));
            }
            serials.push(row(
                "serial-controls",
                vec![
                    v(
                        "hint",
                        json!({"flex":1}),
                        vec![label(
                            key(if app.snapshot["canAdmin"] == true {
                                "sysinfo.serial.hint"
                            } else {
                                "sysinfo.serial.adminOnly"
                            }),
                            t,
                            true,
                            12,
                        )],
                    ),
                    button(
                        key(if app.serials.is_null() {
                            "sysinfo.serial.show"
                        } else {
                            "sysinfo.serial.hide"
                        }),
                        if app.serials.is_null() {
                            "serials"
                        } else {
                            "hideSerials"
                        },
                        "",
                        false,
                        app.serial_busy || app.snapshot["canAdmin"] != true,
                    ),
                ],
            ));
            children.push(group(
                "serials",
                key("sysinfo.serial.title"),
                key("sysinfo.serial.footer"),
                serials,
                t,
            ));
        }
        "memory" => {
            children.push(group(
                "memory",
                Value::Null,
                Value::Null,
                object_rows(&report["memory"], t),
                t,
            ));
            let mut modules:Vec<_>=arr(&app.modules).iter().enumerate().map(|(i,module)|kv(&i.to_string(),if module["slot"]=="" {json!({"key":"sysinfo.modules.slot","params":{"n":i+1}})} else {module["slot"].clone()},
                if module["sizeMB"].as_f64().unwrap_or(0.)>0. {joined(vec![bytes(module["sizeMB"].as_f64().unwrap_or(0.)*1048576.),module["type"].clone(),module["maker"].clone(),if module["speed"].as_u64().unwrap_or(0)>0 {json!({"key":"sysinfo.modules.speed","params":{"mts":if module["configured"].as_u64().unwrap_or(0)>0 && module["configured"]!=module["speed"] {json!(format!("{}/{}",module["configured"],module["speed"]))} else {module["speed"].clone()}}})} else {Value::Null}]," · ")} else {key("sysinfo.modules.empty")},t)).collect();
            if app.modules.is_null() || arr(&app.modules).is_empty() {
                modules.push(label(
                    key(if app.modules_busy {
                        "sysinfo.modules.reading"
                    } else if app.modules.is_null() {
                        "sysinfo.modules.hint"
                    } else {
                        "sysinfo.modules.none"
                    }),
                    t,
                    true,
                    12,
                ));
            }
            modules.push(button(
                key("sysinfo.modules.read"),
                "modules",
                "",
                false,
                app.modules_busy || app.snapshot["canAdmin"] != true,
            ));
            children.push(group(
                "modules",
                key("sysinfo.modules.title"),
                key("sysinfo.modules.footer"),
                modules,
                t,
            ));
        }
        "graphics" => {
            for gpu in arr(&report["gpus"]) {
                let layers = model("cardLayers", json!([gpu]));
                let rows = arr(&layers)
                    .iter()
                    .map(|l| {
                        state_row(
                            text(&l["id"]),
                            key(&format!("sysinfo.layer.{}", text(&l["id"]))),
                            text(&l["state"]),
                            layer_detail(l),
                            "state",
                            t,
                        )
                    })
                    .collect();
                children.push(group(
                    text(&gpu["card"]),
                    gpu["card"].clone(),
                    Value::Null,
                    rows,
                    t,
                ));
            }
            let layers = model("sessionLayers", json!([report["session"]]));
            let rows = arr(&layers)
                .iter()
                .map(|l| {
                    state_row(
                        text(&l["id"]),
                        key(&format!("sysinfo.layer.{}", text(&l["id"]))),
                        text(&l["state"]),
                        if l["id"] == "composition" && !l["note"].is_null() {
                            key(&format!("sysinfo.reason.{}", camel(text(&l["note"]))))
                        } else {
                            layer_detail(l)
                        },
                        "state",
                        t,
                    )
                })
                .collect();
            children.push(group(
                "session",
                key("sysinfo.thisSession"),
                key("sysinfo.uncheckedFooter"),
                rows,
                t,
            ));
        }
        "peripherals" => {
            let peripherals = model(
                "peripherals",
                json!([app.local["devices"], app.local["bluetooth"]]),
            );
            let rows=arr(&peripherals).iter().map(|p|kv(text(&p["kind"]),key(&format!("sysinfo.periph.{}",text(&p["kind"]))),
                joined(vec![p["name"].clone(),key(&format!("sysinfo.periph.via.{}",text(&p["connection"]))),
                    if p["connected"]==true {Value::Null} else {key("sysinfo.periph.notConnected")},
                    if p["battery"].is_number() {json!({"key":"sysinfo.periph.battery","params":{"pct":p["battery"]}})} else {Value::Null}]," · "),t)).collect();
            children.push(group(
                "peripherals",
                Value::Null,
                key("sysinfo.periph.footer"),
                rows,
                t,
            ));
        }
        _ => children.extend(hardware_details(&app.section, report, t)),
    }
    let sidebar = arr(&sections)
        .iter()
        .filter_map(Value::as_str)
        .map(|id| {
            button(
                key(&format!("sysinfo.section.{id}")),
                "section",
                id,
                app.section == id,
                false,
            )
        })
        .collect();
    v(
        "hardware",
        json!({"flex":1,"flexDirection":"row","minHeight":0}),
        vec![
            v(
                "sidebar",
                json!({"width":196,"backgroundColor":t["sidebarBg"],"padding":8}),
                vec![
                    json!({"kind":"scroll","style":{"flex":1},"children":[v("sections",json!({"gap":3}),sidebar)]}),
                ],
            ),
            scroll(&format!("hardware-{}", app.section), children),
        ],
    )
}
fn compat_detail(row: &Value) -> Value {
    let note = text(&row["note"]);
    if let Some(reason) = note.strip_prefix("cpu.") {
        json!({"key":"sysinfo.compat.cpuBecause","params":{"reason":key(&format!("sysinfo.reason.{}",camel(reason)))}})
    } else if !note.is_empty() {
        json!({"key":format!("sysinfo.compat.{}",note.replace('.' ,"_")),"params":row["vars"]})
    } else if row["vars"]["count"].is_number() {
        json!({"key":format!("sysinfo.compat.count.{}",text(&row["id"])),"params":row["vars"]})
    } else {
        Value::Null
    }
}
fn compatibility(app: &App) -> Value {
    let t = &app.theme;
    let report = &app.snapshot["report"];
    let rows = model("compatibility", json!([report]));
    let comm = &app.snapshot["community"];
    let by_component = sharing::community(&comm["snapshot"], report);
    let mut content = vec![];
    for c in arr(&rows) {
        let id = text(&c["id"]);
        content.push(state_row(
            id,
            key(&format!("sysinfo.component.{id}")),
            text(&c["state"]),
            compat_detail(c),
            "state",
            t,
        ));
        let counts = &by_component[id];
        if counts["reports"].as_u64().unwrap_or(0) > 0 {
            let note = if counts["references"] == counts["reports"] {
                let works = counts["works"].as_u64().unwrap_or(0);
                let limits = counts["limits"].as_u64().unwrap_or(0);
                let no = counts["no"].as_u64().unwrap_or(0);
                key(&format!(
                    "sysinfo.community.reference.{}",
                    if no > works + limits {
                        "no"
                    } else if limits > works {
                        "limits"
                    } else {
                        "works"
                    }
                ))
            } else {
                json!({"key":"sysinfo.community.counts","params":{"count":counts["reports"],"works":counts["works"],"limits":counts["limits"],"no":counts["no"]}})
            };
            content.push(label(
                joined(vec![key("sysinfo.community.label"), note], " · "),
                t,
                true,
                12,
            ));
        }
    }
    scroll(
        "compat",
        vec![
            group(
                "compatibility",
                Value::Null,
                key("sysinfo.compatFooter"),
                content,
                t,
            ),
            label(
                if comm["state"] == "ok" {
                    json!({"key":"sysinfo.community.footer","params":{"date":comm["snapshot"]["generated"],"count":comm["snapshot"]["reports"],"version":report["software"]["lordos"]}})
                } else {
                    key(if comm["state"] == "missing" {
                        "sysinfo.community.none.missing"
                    } else {
                        "sysinfo.community.none.unverified"
                    })
                },
                t,
                true,
                12,
            ),
        ],
    )
}
fn check_rows(rows: &Value, t: &Value) -> Vec<Value> {
    arr(rows).iter().map(|c|{
        let id=text(&c["id"]);let note=text(&c["note"]);
        let detail=if let Some(reason)=note.strip_prefix("gpuNot.") {json!({"key":"sysinfo.compat.cpuBecause","params":{"reason":key(&format!("sysinfo.reason.{}",camel(reason)))}})}
            else if note.is_empty() {Value::Null} else {json!({"key":format!("sysinfo.checkNote.{note}"),"params":c["vars"]})};
        v(id,json!({"paddingVertical":7,"gap":3}),vec![state_row(id,key(&format!("sysinfo.check.{id}")),text(&c["state"]),detail,"checkState",t),
            label(joined(vec![key(&format!("sysinfo.level.{}",text(&c["level"]))),if c["paper"].is_string() {json!({"key":"sysinfo.paperRef","params":{"ref":c["paper"]}})} else {Value::Null}]," · "),t,true,11)])
    }).collect()
}
fn checks(app: &App) -> Value {
    let t = &app.theme;
    let s = &app.snapshot;
    let rows = check(
        "evaluate",
        json!({"report":s["report"],"facts":s["facts"],"code":s["code"],"stats":s["stats"]}),
    );
    let catalog = crate::request(&json!({"cmd":"catalog"}));
    let probe = if !app.probe.is_null() {
        check("evaluateProbe", app.probe.clone())
    } else {
        json!(arr(&catalog["probeChecks"])
            .iter()
            .map(|id| json!({"id":id,"level":"effective","state":"pending"}))
            .collect::<Vec<_>>())
    };
    let bench = check("evaluateBench", s["testbench"].clone());
    scroll(
        "checks",
        vec![
            group(
                "checks-main",
                Value::Null,
                key("sysinfo.checksFooter"),
                check_rows(&rows, t),
                t,
            ),
            row(
                "probe-toolbar",
                vec![
                    v(
                        "probe-caption",
                        json!({"flex":1,"gap":4}),
                        vec![
                            title(key("sysinfo.probe.title"), t),
                            label(key("sysinfo.probe.intro"), t, true, 12),
                        ],
                    ),
                    button(
                        key(if app.probe_until > 0 {
                            "sysinfo.probe.running"
                        } else if !app.probe.is_null() {
                            "sysinfo.probe.again"
                        } else {
                            "sysinfo.probe.run"
                        }),
                        "probe",
                        "",
                        true,
                        app.probe_until > 0,
                    ),
                ],
            ),
            group("probe", Value::Null, Value::Null, check_rows(&probe, t), t),
            row(
                "bench-toolbar",
                vec![
                    v(
                        "bench-caption",
                        json!({"flex":1,"gap":4}),
                        vec![
                            title(key("sysinfo.bench.title"), t),
                            label(key("sysinfo.bench.intro"), t, true, 12),
                        ],
                    ),
                    button(
                        key("sysinfo.bench.reload"),
                        "testbench",
                        "reload",
                        false,
                        false,
                    ),
                    button(key("sysinfo.bench.open"), "testbench", "open", true, false),
                ],
            ),
            group(
                "testbench",
                Value::Null,
                Value::Null,
                check_rows(&bench, t),
                t,
            ),
            group(
                "pending",
                key("sysinfo.checksPendingTitle"),
                key("sysinfo.checksPendingFooter"),
                check_rows(
                    &json!(arr(&catalog["pending"])
                        .iter()
                        .map(|id| json!({"id":id,"level":"effective","state":"pending"}))
                        .collect::<Vec<_>>()),
                    t,
                ),
                t,
            ),
        ],
    )
}
fn note_parts(notes: &Value, namespace: &str) -> Value {
    let mut parts = vec![];
    if arr(notes).is_empty()
        || namespace == "sysinfo.perf.note" && arr(notes).iter().all(|n| n["info"] == true)
    {
        parts.push(key(&format!("{namespace}.none")));
    }
    for n in arr(notes)
        .iter()
        .filter(|n| n["info"] != true)
        .chain(arr(notes).iter().filter(|n| n["info"] == true))
    {
        let mut params = n["vars"].clone();
        if params["ids"].is_array() {
            params["list"] = joined(
                arr(&params["ids"])
                    .iter()
                    .map(|id| key(&format!("sysinfo.perf.sbx.{}", text(id))))
                    .collect(),
                ", ",
            );
        }
        if namespace == "sysinfo.perf.dnote" {
            if let Some(values) = params.as_object_mut() {
                for value in values.values_mut() {
                    if let Some(n) = value.as_f64() {
                        *value = number(n, 1);
                    }
                }
            }
        }
        parts.push(json!({"key":format!("{namespace}.{}",text(&n["id"])),"params":params}));
    }
    joined(parts, " ")
}

#[cfg(test)]
#[test]
fn benchmark_notes_resolve_sandbox_names_and_preserve_the_no_warning_message() {
    let root = format!("{}/../../assets", env!("CARGO_MANIFEST_DIR"));
    let mut i18n = axia_native::i18n::I18n::new(&root, None);
    i18n.lang = "en".into();
    let leaks = note_parts(
        &json!([{"id":"sandboxLeaks","vars":{"ids":["helperInet","helperExec"]}}]),
        "sysinfo.perf.note",
    );
    let text = i18n.caption(&leaks);
    assert!(text.contains("open a network connection"));
    assert!(text.contains("run a program"));
    assert!(!text.contains("{list}"));
    let clear = note_parts(&json!([]), "sysinfo.perf.note");
    assert!(i18n.caption(&clear).contains("no failed measurement"));
}
fn conditions(res: &Value, t: &Value) -> Vec<Value> {
    let b = &res["before"];
    let a = &res["after"];
    let line =
        |name: &str, value: Value| kv(name, key(&format!("sysinfo.perf.c.{name}")), value, t);
    let temp = |c: &Value| {
        if c["tempC"].is_number() {
            json!({"key":"sysinfo.perf.tempC","params":{"v":number(c["tempC"].as_f64().unwrap_or(0.),0)}})
        } else {
            Value::Null
        }
    };
    let mut temperatures = vec![];
    if b["tempC"].is_number() {
        temperatures.push(json!({"key":"sysinfo.perf.startEnd","params":{"a":temp(b),"b":if a["tempC"].is_number() {temp(a)} else {json!("—")}}}));
    }
    for (field, note) in [("passiveC", "passive"), ("criticalC", "critical")] {
        if b[field].as_f64().unwrap_or(0.) > 0. {
            temperatures.push(json!({"key":format!("sysinfo.perf.{note}"),"params":{"v":number(b[field].as_f64().unwrap_or(0.),0)}}));
        }
    }
    let mut rows = vec![
        line(
            "versions",
            joined(
                vec![
                    json!(format!("Axia DE {}", text(&res["lordos"]))),
                    json!({"key":"sysinfo.perf.kernel","params":{"v":res["kernel"]}}),
                    json!(format!("AxiaBench {}", res["lordbench"])),
                ],
                " · ",
            ),
        ),
        line(
            "threads",
            json!({"key":"sysinfo.perf.threadsOf","params":{"n":res["threads"],"cpus":res["cpus"]}}),
        ),
        line(
            "quiet",
            json!({"key":if res["quiet"]["quiet"]==true {"sysinfo.perf.quietYes"} else {"sysinfo.perf.quietNo"},"params":{"pct":number(res["quiet"]["busyPct"].as_f64().unwrap_or(0.),0),"s":number(res["quiet"]["waitedS"].as_f64().unwrap_or(0.),0)}}),
        ),
        line("temps", joined(temperatures, " · ")),
        line(
            "power",
            joined(
                vec![
                    key(&format!(
                        "sysinfo.perf.power.{}",
                        b["power"].as_str().unwrap_or("unknown")
                    )),
                    if b["batteryPct"].is_number() {
                        json!({"parts":[b["batteryPct"]," %"]})
                    } else {
                        Value::Null
                    },
                    b["platformProfile"].clone(),
                ],
                " · ",
            ),
        ),
    ];
    let firmware = model("piFirmware", json!([res]));
    if !firmware.is_null() {
        let vars = json!({"temp":number(firmware["tempLimitC"].as_f64().unwrap_or(80.),0)});
        let notes = if arr(&firmware["events"]).is_empty() {
            vec![
                json!({"key":if firmware["flags"]==true {"sysinfo.perf.pi.none"} else if firmware["voltageAlarm"]==true {"sysinfo.perf.pi.noneVoltage"} else {"sysinfo.perf.pi.noneTemp"},"params":vars}),
            ]
        } else {
            arr(&firmware["events"])
                .iter()
                .map(|id| json!({"key":format!("sysinfo.perf.pi.{}",text(id)),"params":vars}))
                .collect()
        };
        rows.push(line("piFirmware", joined(notes, ", ")));
    }
    let displays = arr(&res["session"]["displays"])
        .iter()
        .filter(|d| d["mode"].is_object())
        .map(|d| {
            let mut parts = vec![
                json!(format!("{}×{} @ ", d["mode"]["w"], d["mode"]["h"])),
                number(d["mode"]["refresh"].as_f64().unwrap_or(0.) / 1000., 0),
                json!(" Hz"),
            ];
            if d["scale"].as_f64().is_some_and(|n| n != 0. && n != 100.) {
                parts.push(json!(format!(" ({} %)", d["scale"])));
            }
            json!({"parts":parts})
        })
        .collect();
    rows.extend([line("cpufreq",joined(vec![b["governor"].clone(),if b["freqMaxMHz"].as_f64().unwrap_or(0.)>0. {json!({"key":"sysinfo.perf.maxMHz","params":{"v":number(b["freqMaxMHz"].as_f64().unwrap_or(0.),0)}})} else {Value::Null}]," · ")),
        line("gpu",joined(vec![if res["gpuKind"].is_string() {key(&format!("sysinfo.perf.gpuKind.{}",text(&res["gpuKind"])))} else {Value::Null},res["session"]["renderer"].clone()]," · ")),
        line("displays",joined(vec![joined(displays,", "),if res["session"]["composition"].is_string() {json!({"key":"sysinfo.perf.composition","params":{"c":res["session"]["composition"]}})} else {Value::Null}]," · ")),
        line("storage",if res["storage"].is_object()&&res["storage"]["kind"]!="none" {json!({"key":"sysinfo.perf.storageOn","params":{"kind":key(&format!("sysinfo.perf.disk.{}",text(&res["storage"]["kind"]))),"mb":res["storage"]["mb"]}})} else {key("sysinfo.perf.storageNone")} ),
        line("live",key(if res["live"]==true {"sysinfo.perf.liveYes"} else {"sysinfo.perf.liveNo"}))]);
    rows
}
fn results(app: &App, res: &Value, slot: usize) -> Value {
    let t = &app.theme;
    let desktop = res["mode"] == "desktop";
    let catalog = model("perfCatalog", json!([]));
    let references = model("comparableReferences", json!([res]));
    let prev = if app.saved[slot]["at"] == res["at"] {
        &app.saved[slot + 1]
    } else {
        &app.saved[slot]
    };
    let changes = model("changes", json!([res, prev]));
    let mut run_caption = vec![
        json!({"key":"sysinfo.perf.ranAt","params":{"mode":key(&format!("sysinfo.perf.mode.{}",text(&res["mode"]))),"time":app.run_times[res["at"].to_string()],"s":res["seconds"]}}),
    ];
    if res["status"].is_string() && res["status"] != "done" {
        run_caption.push(key(&format!(
            "sysinfo.perf.status.{}",
            text(&res["status"])
        )));
    }
    if res["stopReason"].is_string() {
        run_caption.push(key(&format!(
            "sysinfo.perf.why.{}",
            text(&res["stopReason"])
        )));
    }
    let mut body = vec![row(
        "result-toolbar",
        vec![
            v(
                "run-caption",
                json!({"flex":1}),
                vec![label(joined(run_caption, " · "), t, true, 12)],
            ),
            button(
                key("sysinfo.report.save"),
                "save",
                &slot.to_string(),
                false,
                app.export.is_some(),
            ),
        ],
    )];
    let stability = model("stability", json!([res]));
    let mut verdicts = vec![state_row(
        "stability",
        key("sysinfo.perf.v.stability"),
        text(&stability["state"]),
        note_parts(&stability["notes"], "sysinfo.perf.note"),
        "perf.verdictState",
        t,
    )];
    if desktop {
        let verdict = model("desktopVerdict", json!([res]));
        verdicts.insert(
            0,
            state_row(
                "desktop",
                key("sysinfo.perf.v.desktop"),
                text(&verdict["state"]),
                note_parts(&verdict["notes"], "sysinfo.perf.dnote"),
                "perf.deskState",
                t,
            ),
        );
    } else {
        let compat = model(
            "compatibilityVerdict",
            json!([model("compatibility", json!([app.snapshot["report"]]))]),
        );
        verdicts.push(state_row(
            "compatibility",
            key("sysinfo.perf.v.compatibility"),
            text(&compat["state"]),
            json!({"key":"sysinfo.perf.compatCount","params":compat["count"]}),
            "perf.verdictState",
            t,
        ));
        let mut summaries = vec![];
        for reference in arr(&references) {
            let ratios = model("categoryRatios", json!([res, reference]));
            let list=joined(arr(&catalog["categories"]).iter().filter(|c|ratios[text(&c["id"])].is_number()).map(|c|json!({"key":"sysinfo.perf.catTimes","params":{"cat":key(&format!("sysinfo.perf.cat.{}",text(&c["id"]))),"x":number(ratios[text(&c["id"])].as_f64().unwrap_or(0.),2)}})).collect(),", ");
            summaries.push(json!({"key":"sysinfo.perf.againstRef","params":{"ref":key(&format!("sysinfo.perf.ref.{}",text(&reference["id"]))),"list":list}}));
        }
        verdicts.push(state_row(
            "performance",
            key("sysinfo.perf.v.performance"),
            if summaries.is_empty() {
                "unknown"
            } else {
                "pass"
            },
            if summaries.is_empty() {
                key("sysinfo.perf.noRefs")
            } else {
                joined(summaries, "\n")
            },
            "perf.verdictState",
            t,
        ));
    }
    body.push(group(
        "verdicts",
        key("sysinfo.perf.verdicts"),
        if desktop {json!({"key":"sysinfo.perf.deskFooter","params":{"budget":number(res["frames"]["budgetMs"].as_f64().unwrap_or(0.),1)}})} else {key("sysinfo.perf.verdictsFooter")},
        verdicts,
        t,
    ));
    let categories = &catalog[if desktop { "desktop" } else { "categories" }];
    for category in arr(categories) {
        let mut measurements = vec![];
        for id in arr(&category["ids"]).iter().filter_map(Value::as_str) {
            let result = &res["results"][id];
            let mut details = vec![];
            if !desktop && result.is_object() {
                for reference in arr(&references) {
                    let ratio = model("ratio", json!([res, reference, id]));
                    if let Some(ratio) = ratio.as_f64() {
                        details.push(json!({"key":"sysinfo.perf.timesRef","params":{"x":number(ratio,2),"ref":key(&format!("sysinfo.perf.ref.{}",text(&reference["id"])))}}));
                    }
                }
            }
            if let Some(change) = changes[id].as_f64().filter(|n| n.abs() >= 0.005) {
                details.push(json!({"key":if change>0. {"sysinfo.perf.better"} else {"sysinfo.perf.worse"},"params":{"pct":number(change.abs()*100.,0)}}));
            }
            if res["skipped"][id].is_string() {
                details.push(key(&format!(
                    "sysinfo.perf.skip.{}",
                    text(&res["skipped"][id])
                )));
            }
            measurements.push(v(
                id,
                json!({"gap":3,"paddingVertical":5}),
                vec![
                    kv(
                        id,
                        key(&format!("sysinfo.perf.m.{}", id.replace('.', "_"))),
                        if result.is_object() {
                            joined(
                                vec![
                                    number(result["value"].as_f64().unwrap_or(0.), 2),
                                    result["unit"].clone(),
                                ],
                                " ",
                            )
                        } else {
                            json!("—")
                        },
                        t,
                    ),
                    label(joined(details, " · "), t, true, 12),
                ],
            ));
        }
        if category["id"] == "sustained" {
            for sample in arr(&model("sustainedSeries", json!([res]))) {
                measurements.push(label(json!({"key":if sample[2].is_null() {"sysinfo.perf.sampleNoTemp"} else {"sysinfo.perf.sample"},"params":{"s":sample[0],"g":sample[1],"temp":sample[2]}}),t,true,12));
            }
        }
        body.push(group(
            text(&category["id"]),
            key(&format!("sysinfo.perf.cat.{}", text(&category["id"]))),
            Value::Null,
            measurements,
            t,
        ));
    }
    let diffs = model("conditionDiffs", json!([res, prev]));
    if !changes.is_null() && !arr(&diffs).is_empty() {
        body.push(group("diffs",key("sysinfo.perf.diffs"),Value::Null,arr(&diffs).iter().map(|d|kv(text(&d["id"]),key(&format!("sysinfo.perf.diff.{}",text(&d["id"]))),json!({"key":"sysinfo.perf.diffLine","params":{"now":d["now"],"before":d["before"]}}),t)).collect(),t));
    }
    body.push(group(
        "conditions",
        key("sysinfo.perf.conditions"),
        key("sysinfo.perf.conditionsFooter"),
        conditions(res, t),
        t,
    ));
    v("result", json!({"gap":20}), body)
}
fn performance(app: &App) -> Value {
    if app.bench["state"] == "unavailable" {
        return scroll(
            "performance",
            vec![label(key("sysinfo.perf.unavailable"), &app.theme, true, 13)],
        );
    }
    let t = &app.theme;
    let mut buttons = vec![v(
        "caption",
        json!({"flex":1,"gap":4}),
        vec![
            title(json!("AxiaBench"), t),
            label(key("sysinfo.perf.intro"), t, true, 12),
        ],
    )];
    if app.running() {
        buttons.push(button(
            key("sysinfo.perf.cancel"),
            "benchCancel",
            "",
            false,
            false,
        ));
    } else {
        for (id, primary) in [("desktop", false), ("full", false), ("quick", true)] {
            buttons.push(button(
                key(&format!("sysinfo.perf.{id}")),
                "benchStart",
                id,
                primary,
                false,
            ));
        }
    }
    let mut body = vec![row("bench-controls", buttons)];
    if app.running() {
        body.push(label(if app.bench["state"]=="waiting" {key("sysinfo.perf.waiting")} else {json!({"key":"sysinfo.perf.measuring","params":{"what":key(&format!("sysinfo.perf.m.{}",text(&app.bench["step"]).replace('.',"_")))}})},t,false,13));
        body.push(v("progress",json!({"height":6,"backgroundColor":t["control"],"borderRadius":3}),vec![v("fill",json!({"height":6,"width":format!("{}%",(app.bench["progress"].as_f64().unwrap_or(0.).clamp(0.,1.)*100.).round()),"backgroundColor":t["accent"],"borderRadius":3}),vec![])]));
        body.push(label(
            key(if app.bench["mode"] == "desktop" {
                "sysinfo.perf.deskKeepStill"
            } else {
                "sysinfo.perf.keepStill"
            }),
            t,
            true,
            12,
        ));
    } else {
        body.push(label(
            json!({"key":"sysinfo.perf.fullNote","params":{"min":3}}),
            t,
            true,
            12,
        ));
    }
    for slot in [0, 2] {
        let fresh = &app.bench["result"];
        let res = if fresh.is_object() && (fresh["mode"] == "desktop") == (slot == 2) {
            fresh
        } else if app.running() {
            &Value::Null
        } else {
            &app.saved[slot]
        };
        if res.is_object() {
            if slot == 2 {
                body.push(title(key("sysinfo.perf.deskTitle"), t));
            }
            body.push(results(app, res, slot));
        }
    }
    if !app.running() {
        let mut boots = vec![];
        for boot in arr(&app.snapshot["boots"]).iter().rev().take(6) {
            let mut details = vec![];
            if boot["boots"].as_u64().unwrap_or(0) > 0 {
                details.push(json!({"key":"sysinfo.perf.bootLine","params":{"median":number(boot["median"].as_f64().unwrap_or(0.),1),"min":number(boot["min"].as_f64().unwrap_or(0.),1),"max":number(boot["max"].as_f64().unwrap_or(0.),1),"n":boot["boots"]}}));
                if boot["first"].is_number() {
                    details.push(json!({"key":"sysinfo.perf.bootFirst","params":{"s":number(boot["first"].as_f64().unwrap_or(0.),1)}}));
                }
            } else {
                details.push(json!({"key":"sysinfo.perf.bootOnlyFirst","params":{"s":number(boot["first"].as_f64().unwrap_or(0.),1)}}));
            }
            if boot["test"].is_object() {
                let test = &boot["test"];
                details.push(json!({"key":"sysinfo.perf.bootTestLine","params":{"median":number(test["median"].as_f64().unwrap_or(0.),1),"min":number(test["min"].as_f64().unwrap_or(0.),1),"max":number(test["max"].as_f64().unwrap_or(0.),1),"n":test["boots"],"of":test["of"]}}));
            }
            boots.push(kv(
                text(&boot["version"]),
                boot["version"].clone(),
                joined(details, " · "),
                t,
            ));
        }
        let state = &app.snapshot["bootTest"];
        if state.is_object() {
            boots.push(label(json!({"key":if state["left"]==state["total"] {"sysinfo.perf.bootTestSet"} else {"sysinfo.perf.bootTestRunning"},"params":{"n":state["total"],"done":state["total"].as_i64().unwrap_or(0)-state["left"].as_i64().unwrap_or(0)}}),t,false,13));
            boots.push(row(
                "boot-state",
                vec![
                    button(
                        key("sysinfo.perf.bootTestRestart"),
                        "restart",
                        "",
                        false,
                        app.snapshot["canPower"] != true,
                    ),
                    button(
                        key("sysinfo.perf.bootTestStop"),
                        "bootCancel",
                        "",
                        false,
                        false,
                    ),
                ],
            ));
        } else {
            boots.push(label(key("sysinfo.perf.bootTestAsk"), t, true, 12));
            let mut controls: Vec<_> = [3, 5, 10]
                .into_iter()
                .map(|n| {
                    button(
                        json!(n.to_string()),
                        "bootCount",
                        &n.to_string(),
                        app.boot_count == n,
                        false,
                    )
                })
                .collect();
            controls.push(button(
                key("sysinfo.perf.bootTestStart"),
                "bootStart",
                "",
                false,
                app.snapshot["canPower"] != true,
            ));
            boots.push(row("boot-controls", controls));
        }
        body.push(group(
            "boots",
            key("sysinfo.perf.boots"),
            key("sysinfo.perf.bootsFooter"),
            boots,
            t,
        ));
    }
    scroll("performance", body)
}
fn preview(content: &str, t: &Value) -> Value {
    json!({"kind":"scroll","key":"preview","style":{"flex":1,"minHeight":160,"borderWidth":1,"borderColor":t["separator"],"backgroundColor":t["inputBg"]},
        "children":[v("lines",json!({"padding":12}),vec![json!({"kind":"text","label":content,"style":{"fontSize":11,"fontFamily":"monospace","lineHeight":1.45,"color":t["text"]}})])]})
}
fn report(app: &App) -> Value {
    let t = &app.theme;
    v(
        "report",
        json!({"flex":1,"padding":24,"gap":12}),
        vec![
            label(key("sysinfo.report.intro"), t, true, 12),
            row(
                "export-controls",
                vec![
                    button(
                        key("sysinfo.report.markdown"),
                        "format",
                        "markdown",
                        app.format == "markdown",
                        false,
                    ),
                    button(json!("JSON"), "format", "json", app.format == "json", false),
                    spacer(),
                    button(key("sysinfo.report.copy"), "copy", "", false, false),
                    button(key("sysinfo.share.button"), "share", "", false, false),
                    button(
                        key("sysinfo.report.save"),
                        "save",
                        "",
                        true,
                        app.export.is_some(),
                    ),
                ],
            ),
            preview(&app.report_text(), t),
        ],
    )
}
fn share(app: &App) -> Value {
    let t = &app.theme;
    let rows = sharing::summary(&app.share)
        .into_iter()
        .map(|(name, value)| {
            kv(
                name,
                key(&format!("sysinfo.share.row.{name}")),
                sharing::summary_caption(name, &value, &app.share),
                t,
            )
        })
        .collect();
    let mut exact = preview(&app.share_json, t);
    exact["style"]["height"] = json!(320);
    exact["style"]["flex"] = json!(0);
    scroll(
        "share",
        vec![
            title(key("sysinfo.share.title"), t),
            label(key("sysinfo.share.intro"), t, true, 12),
            group(
                "summary",
                Value::Null,
                joined(
                    vec![
                        if app.share["live"] == true {
                            key("sysinfo.share.live")
                        } else {
                            Value::Null
                        },
                        key(
                            if app.share["bench"].is_object() || app.share["desktop"].is_object() {
                                "sysinfo.share.withBench"
                            } else {
                                "sysinfo.share.noBench"
                            },
                        ),
                    ],
                    " ",
                ),
                rows,
                t,
            ),
            row(
                "consent",
                vec![
                    v(
                        "exact",
                        json!({"flex":1}),
                        vec![label(key("sysinfo.share.exact"), t, true, 12)],
                    ),
                    button(key("sysinfo.share.back"), "shareBack", "", false, false),
                    button(
                        key("sysinfo.share.go"),
                        "save",
                        "",
                        true,
                        app.export.is_some(),
                    ),
                ],
            ),
            exact,
        ],
    )
}
pub fn tree(app: &mut App) -> Value {
    let t = app.theme.clone();
    if app.scene {
        return scene(&t);
    }
    let mut tabs: Vec<_> = ["overview", "hardware", "compat", "checks", "perf", "report"]
        .into_iter()
        .map(|id| {
            button(
                key(&format!("sysinfo.tab.{id}")),
                "tab",
                id,
                app.tab == id,
                false,
            )
        })
        .collect();
    tabs.push(button(json!("↻"), "refresh", "", false, app.jobs.busy()));
    let toolbar = v(
        "toolbar",
        json!({"minHeight":44,"padding":8,"flexDirection":"row","justifyContent":"center","gap":4,"flexShrink":0,"borderBottomWidth":1,"borderBottomColor":t["separator"]}),
        tabs,
    );
    let body = if app.snapshot["report"].is_null() {
        scroll(
            "unavailable",
            vec![label(key("sysinfo.unavailable"), &t, true, 14)],
        )
    } else {
        match app.tab.as_str() {
            "hardware" => hardware(app),
            "compat" => compatibility(app),
            "checks" => checks(app),
            "perf" => performance(app),
            "report" => report(app),
            "share" => share(app),
            _ => overview(app),
        }
    };
    let mut children = vec![toolbar];
    if !app.message.is_null() {
        let mut message = vec![v(
            "status-label",
            json!({"flex":1}),
            vec![label(app.message.clone(), &t, true, 12)],
        )];
        if !app.export_dir.is_empty() {
            message.push(button(
                key("sysinfo.report.showInFiles"),
                "showFiles",
                "",
                false,
                false,
            ));
        }
        children.push(v(
            "status",
            json!({"paddingHorizontal":24,"paddingVertical":8}),
            vec![row("status-row", message)],
        ));
    }
    children.push(body);
    let rtl = app.i18n.entry(&app.i18n.lang.clone(), "meta", "direction") == Some(json!("rtl"));
    let mut root = v(
        "sysinfo",
        json!({"flex":1,"direction":if rtl {"rtl"} else {"ltr"},"backgroundColor":t["windowBg"]}),
        children,
    );
    highlight_buttons(&mut root, &t);
    root
}
fn scene(t: &Value) -> Value {
    let cards=(0..6).map(|i|v(&format!("card-{i}"),json!({"width":180,"height":96,"borderRadius":12,"padding":12,"gap":4,"backgroundColor":t["control"],"boxShadow":"0 2px 8px rgba(0,0,0,0.25)"}),
        vec![v("tile",json!({"width":28,"height":28,"borderRadius":8,"backgroundColor":(["#0a84ff","#30d158","#ff9f0a","#bf5af2","#ff453a","#64d2ff"][i])}),vec![]),
            label(key("sysinfo.perf.sceneTitle"),t,false,12),label(json!({"key":"sysinfo.perf.sceneCard","params":{"n":i+1}}),t,true,11)])).collect();
    v(
        "scene",
        json!({"flex":1,"padding":20,"gap":12,"alignItems":"center","justifyContent":"center","backgroundColor":t["windowBg"]}),
        vec![
            v(
                "square",
                json!({"width":72,"height":72,"borderRadius":16,"backgroundColor":t["accent"],"alignItems":"center","justifyContent":"center"}),
                vec![v(
                    "dot",
                    json!({"width":24,"height":24,"borderRadius":12,"backgroundColor":"#fff"}),
                    vec![],
                )],
            ),
            v(
                "cards",
                json!({"flexDirection":"row","flexWrap":"wrap","gap":12,"justifyContent":"center"}),
                cards,
            ),
        ],
    )
}
pub fn icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    let mut children = vec![v(
        "chip",
        json!({"width":s*0.5,"height":s*0.5,"borderRadius":s*0.08,"borderWidth":(s*0.04).max(1.),"borderColor":"rgba(255,255,255,0.95)","alignItems":"center","justifyContent":"center"}),
        vec![v(
            "core",
            json!({"width":s*0.22,"height":s*0.22,"borderRadius":s*0.03,"backgroundColor":"#FFD60A"}),
            vec![],
        )],
    )];
    for p in [0.32, 0.5, 0.68] {
        for side in ["top", "bottom"] {
            let mut style = json!({"position":"absolute","left":s*p-s*0.02,"width":s*0.04,"height":s*0.08,"backgroundColor":"rgba(255,255,255,0.8)"});
            style[side] = json!(s * 0.16);
            children.push(v(&format!("{p}-{side}"), style, vec![]));
        }
    }
    v(
        "icon",
        json!({"width":s,"height":s,"borderRadius":s*0.22,"alignItems":"center","justifyContent":"center","background":"linear-gradient(135deg, #B46CFF, #4F46E5)"}),
        children,
    )
}
