use serde_json::{json, Value};
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn n(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or(0.)
}
pub fn icon(size: f64) -> Value {
    let size = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    let px = [16, 32, 64, 128, 256, 512, 1024]
        .into_iter()
        .find(|p| *p as f64 >= if size > 32. { size * 2. } else { size })
        .unwrap_or(1024);
    view(
        json!({"width":size,"height":size,"borderRadius":size*0.22,"boxShadow":if size>24. {format!("0 {}px {}px rgba(0,0,0,0.3)",size*0.04,size*0.12)} else {"none".into()}}),
        vec![
            json!({"kind":"image","src":format!("asset:icons/axia-icon-{px}.png"),"style":{"width":size,"height":size}}),
        ],
    )
}
fn number(value: f64, digits: usize, context: &Value) -> String {
    let fixed = format!("{value:.digits$}");
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 && c != '-' && !out.ends_with('-') {
            out.push_str(context["group"].as_str().unwrap_or(","));
        }
        out.push(c);
    }
    if !frac.is_empty() {
        out.push_str(context["decimal"].as_str().unwrap_or("."));
        out.push_str(frac);
    }
    out
}
fn bytes(mut value: f64, context: &Value) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut i = 0;
    while value >= 1024. && i < 4 {
        value /= 1024.;
        i += 1;
    }
    let label = if value < 10. && i > 0 {
        number(value, 1, context)
    } else {
        format!("{:.0}", value.round())
    };
    format!("{} {}", label, units[i])
}
fn duration(value: f64) -> String {
    let s = value.max(0.) as u64;
    let (d, h, m) = (s / 86400, s % 86400 / 3600, s % 3600 / 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    }
}
fn row(k: &str, v: Value, t: &Value) -> Value {
    view(
        json!({"flexDirection":"row","alignItems":"center","minHeight":20,"gap":10}),
        vec![
            text(
                key(k),
                json!({"width":118,"fontSize":12,"color":t["textDim"],"textAlign":"end","whiteSpace":"nowrap","overflow":"hidden"}),
            ),
            view(
                json!({"flex":1,"minWidth":0,"overflow":"hidden"}),
                vec![text(
                    v,
                    json!({"fontSize":12,"fontWeight":500,"color":t["text"],"whiteSpace":"nowrap"}),
                )],
            ),
        ],
    )
}
fn badge(color: &str, k: &str, t: &Value) -> Value {
    view(
        json!({"flexDirection":"row","alignItems":"center","gap":5,"paddingHorizontal":8,"paddingVertical":2,"borderRadius":10,"backgroundColor":if t["mode"]=="dark" {"rgba(255,255,255,0.08)"} else {"rgba(0,0,0,0.05)"}}),
        vec![
            view(
                json!({"width":7,"height":7,"borderRadius":4,"backgroundColor":color}),
                vec![],
            ),
            text(
                key(k),
                json!({"fontSize":11,"color":t["text"],"whiteSpace":"nowrap"}),
            ),
        ],
    )
}
pub fn tree(input: &Value) -> Value {
    let c = &input["context"];
    let t = &c["theme"];
    let info = &input["services"]["system"]["info"];
    let st = &input["services"]["$root"]["stats"];
    let code = &input["services"]["code"]["status"];
    let state = s(code, "state");
    let refused = code["refused"].as_array().is_some_and(|v| !v.is_empty());
    let (color, status) = if refused || state == "invalid" {
        ("#ff453a", "about.codeInvalid")
    } else if state == "verified" {
        ("#30d158", "about.codeVerified")
    } else {
        ("#ff9f0a", "about.codeDevelopment")
    };
    let channel = s(info, "channel");
    let channel = match channel {
        "beta" => key("about.channelBeta"),
        "alpha" => key("about.channelAlpha"),
        "rc" => key("about.channelRc"),
        "dev" => key("about.channelDev"),
        other => json!(other
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string() + &other[c.len_utf8()..])
            .unwrap_or_default()),
    };
    let version = info["version"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| c["version"].as_str())
        .unwrap_or("");
    let mut children = vec![
        icon(64.),
        view(
            json!({"marginTop":12}),
            vec![view(
                json!({"flexDirection":"row","alignItems":"center","direction":"ltr"}),
                vec![text(
                    json!("Axia"),
                    json!({"fontFamily":"brand","fontWeight":800,"fontSize":22,"color":if t["mode"]=="light" {"#12122A"} else {"#ffffff"},"whiteSpace":"nowrap"}),
                )],
            )],
        ),
        text(
            if s(info, "channel").is_empty() {
                json!({"key":"about.version","params":{"v":version}})
            } else {
                json!({"key":"about.versionChannel","params":{"v":version,"channel":channel}})
            },
            json!({"fontSize":12,"color":t["textDim"],"marginTop":2}),
        ),
    ];
    if !s(info, "build").is_empty() {
        children.push(text(
            json!({"key":"about.build","params":{"b":info["build"]}}),
            json!({"fontSize":11,"color":t["textDim"],"marginTop":1}),
        ));
    }
    children.push(view(
        json!({"flexDirection":"row","gap":6,"marginTop":8}),
        vec![
            badge("#0a84ff", "about.systemApp", t),
            badge(color, status, t),
        ],
    ));
    let note = if state == "verified" && !refused {
        json!({"key":"about.codeVerifiedNote","params":{"count":code["files"]}})
    } else if state == "development" {
        key("about.codeDevNote")
    } else {
        Value::Null
    };
    if !note.is_null() {
        children.push(text(note,json!({"fontSize":11,"color":if state=="development" {json!("#ff9f0a")} else {t["textDim"].clone()},"marginTop":4,"textAlign":"center"})));
    }
    children.push(text(key("about.tagline"),json!({"fontSize":12,"color":t["textDim"],"marginTop":10,"maxWidth":320,"textAlign":"center","lineHeight":17})));
    let cpu = s(info, "cpu")
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace("CPU", "")
        .replace("Processor", "");
    static REV: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = REV.get_or_init(|| regex::Regex::new(r" Rev [\d.]+$").unwrap());
    let cpu = re
        .replace(&cpu, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let cpu: String = cpu.chars().take(36).collect();
    let mut rows = vec![
        row("about.os", info["os"].clone(), t),
        row(
            "about.kernel",
            json!(format!("{} ({})", s(info, "kernel"), s(info, "arch"))),
            t,
        ),
        row(
            "about.host",
            json!({"key":"about.hostValue","params":{"host":info["hostname"],"user":info["user"]}}),
            t,
        ),
        row(
            "about.cpu",
            json!({"key":"about.cpuValue","params":{"cpu":if cpu.is_empty(){key("about.cpuUnknown")}else{json!(cpu)},"count":info["cores"]}}),
            t,
        ),
        row(
            "about.memory",
            json!({"key":"shell.cc.memDetail","params":{"used":bytes((n(info,"memTotalKB")-n(info,"memAvailableKB"))*1024.,c),"total":bytes(n(info,"memTotalKB")*1024.,c)}}),
            t,
        ),
    ];
    if input["services"]["fan"]["caseConfig"] == "argon-one-v5" {
        rows.push(row("about.case", key("about.caseArgonV5"), t));
    }
    rows.push(row("about.uptime", json!(duration(n(info, "uptime"))), t));
    if n(info, "bootSeconds") > 0. {
        rows.push(row("about.bootTime",json!({"key":if info["bootShown"]=="session" {"about.bootToDesktop"}else{"about.bootToLogin"},"params":{"s":number(n(info,"bootSeconds"),3,c)}}),t));
    }
    rows.push(view(json!({"height":1,"backgroundColor":t["separator"],"marginVertical":8,"marginHorizontal":24}),vec![]));
    rows.extend([row("about.server",json!("axia-server (C · DRM/KMS · evdev)"),t),row("about.engine",json!(format!("QuickJS + React {}",s(c,"reactVersion"))),t),row("about.codeLabel",key(status),t),row("about.display",json!(format!("{} × {}",c["screen"]["width"],c["screen"]["height"])),t),row("about.ram",json!(format!("{} MB (JS {} MB)",number(n(st,"rssKB")/1024.,1,c),number(n(st,"jsKB")/1024.,1,c))),t),row("about.render",json!({"key":"about.renderValue","params":{"ms":number(n(st,"layoutMs")+n(st,"paintMs"),1,c),"count":st["nodes"]}}),t)]);
    children.push(view(
        json!({"alignSelf":"stretch","marginTop":16,"gap":2}),
        rows,
    ));
    children.push(view(json!({"marginTop":12}),vec![json!({"kind":"button","small":true,"label":key("about.systemReport"),"action":["report",""]})]));
    children.push(view(json!({"flex":1,"minHeight":12}), vec![]));
    children.push(view(json!({"alignSelf":"stretch","flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":24}),vec![icon(24.),text(key("about.credits"),json!({"flex":1,"fontSize":11,"color":t["textDim"],"textAlign":"start","lineHeight":1.4}))]));
    view(
        json!({"flex":1,"alignItems":"center","paddingHorizontal":20,"paddingTop":20,"paddingBottom":16}),
        children,
    )
}
