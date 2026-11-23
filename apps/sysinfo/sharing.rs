//! Schema allow-list and local share preview. Private UI data is never an input.
use axia_native::i18n::I18n;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::OnceLock;
fn schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../../tools/hwreport/hwreport.schema.json")).unwrap()
    })
}
pub(super) fn pick(value: &Value, spec: &Value, root: &Value, depth: usize) -> Option<Value> {
    if depth > 64 {
        return None;
    }
    if let Some(reference) = spec["$ref"].as_str() {
        return root
            .pointer(reference.strip_prefix('#')?)
            .and_then(|s| pick(value, s, root, depth + 1));
    }
    if let Some(options) = spec["anyOf"].as_array() {
        return options.iter().find_map(|s| pick(value, s, root, depth + 1));
    }
    let matches = |kind: &str| match kind {
        "null" => value.is_null(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_f64().is_some_and(|n| n.fract() == 0.),
        _ => false,
    };
    if let Some(kind) = spec["type"].as_str() {
        if !matches(kind) {
            return None;
        }
    }
    if let Some(kinds) = spec["type"].as_array() {
        if !kinds.iter().filter_map(Value::as_str).any(matches) {
            return None;
        }
    }
    if let Some(expected) = spec.get("const") {
        return (value == expected).then(|| value.clone());
    }
    if let Some(allowed) = spec["enum"].as_array() {
        return allowed.contains(value).then(|| value.clone());
    }
    if let Some(n) = value.as_f64() {
        if spec["minimum"].as_f64().is_some_and(|min| n < min)
            || spec["maximum"].as_f64().is_some_and(|max| n > max)
        {
            return None;
        }
    }
    if let Some(s) = value.as_str() {
        let max = spec["maxLength"].as_u64().unwrap_or(usize::MAX as u64) as usize;
        let value = if s.chars().count() > max {
            s.chars()
                .take(max)
                .collect::<String>()
                .trim_end()
                .to_owned()
        } else {
            s.to_owned()
        };
        if let Some(pattern) = spec["pattern"].as_str() {
            if !Regex::new(pattern).ok()?.is_match(&value) {
                return None;
            }
        }
        return Some(json!(value));
    }
    if let Some(values) = value.as_array() {
        let out: Vec<_> = values
            .iter()
            .filter_map(|v| spec.get("items").and_then(|s| pick(v, s, root, depth + 1)))
            .take(spec["maxItems"].as_u64().unwrap_or(usize::MAX as u64) as usize)
            .collect();
        return Some(json!(out));
    }
    if let Some(values) = value.as_object() {
        let mut out = serde_json::Map::new();
        for (key, value) in values {
            if out.len() >= spec["maxProperties"].as_u64().unwrap_or(usize::MAX as u64) as usize {
                break;
            }
            let field = spec["properties"].get(key).or_else(|| {
                if spec["propertyNames"]["pattern"]
                    .as_str()
                    .is_some_and(|pattern| Regex::new(pattern).map_or(true, |rx| !rx.is_match(key)))
                {
                    return None;
                }
                spec.get("additionalProperties").filter(|v| v.is_object())
            });
            if let Some(value) = field.and_then(|s| pick(value, s, root, depth + 1)) {
                out.insert(key.clone(), value);
            }
        }
        if spec["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|name| !out.contains_key(name))
        {
            return None;
        }
        return Some(Value::Object(out));
    }
    Some(value.clone())
}
fn base_version(value: &Value) -> String {
    let Some(s) = value.as_str() else {
        return String::new();
    };
    Regex::new(r"^\d+\.\d+\.\d+(?:\.\d+)?")
        .unwrap()
        .find(s)
        .map_or(String::new(), |m| m.as_str().into())
}
pub fn build(report: &Value, bench: &Value, desktop: &Value) -> Value {
    let schema = schema();
    let version = base_version(&report["software"]["lordos"]);
    let keep_bench = |value: &Value| {
        if !version.is_empty() && base_version(&value["lordos"]) == version {
            pick(value, &schema["$defs"]["bench"], schema, 0).unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    };
    let b = keep_bench(bench);
    let d = keep_bench(desktop);
    let compatibility = crate::request(&json!({"cmd":"compatibility","args":[report]}));
    let rows: Vec<_> = super::arr(&compatibility)
        .iter()
        .map(|v| json!({"id":v["id"],"state":v["state"],"note":v["note"]}))
        .collect();
    json!({"format":"lordos-hwreport","version":1,"source":"community","createdAt":report["date"],
        "lordos":report["software"]["lordos"],"live":report["software"]["live"]==true||b["live"]==true||d["live"]==true,
        "system":pick(report,&schema["$defs"]["system"],schema,0).unwrap_or(json!({})),
        "compatibility":pick(&json!(rows),&schema["properties"]["compatibility"],schema,0).unwrap_or(json!([])),"bench":b,"desktop":d})
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn summary_caption(name: &str, value: &str, report: &Value) -> Value {
    match name {
        "kind" => super::key(&format!("sysinfo.share.kind.{value}")),
        "live" => super::key(if report["live"] == true {
            "sysinfo.perf.liveYes"
        } else {
            "sysinfo.perf.liveNo"
        }),
        "boot" => {
            let sw = &report["system"]["software"];
            let mut parts = vec![super::key(&format!(
                "sysinfo.boot.{}",
                super::text(&sw["bootMedium"])
            ))];
            if sw["bootLinkMbps"].as_u64().unwrap_or(0) > 0 {
                parts.extend([
                    json!(", "),
                    json!({"key":"sysinfo.boot.mbps","params":{"n":sw["bootLinkMbps"]}}),
                ]);
            }
            if sw["bootToRam"] == true {
                parts.extend([json!(", "), super::key("sysinfo.boot.toRam")]);
            }
            json!({"parts":parts})
        }
        _ => json!(value),
    }
}
pub fn summary(report: &Value) -> Vec<(&'static str, String)> {
    let s = &report["system"];
    let gpu = super::arr(&s["gpus"])
        .iter()
        .find(|g| g["bootVga"] == true && g["renderNode"].is_string())
        .or_else(|| {
            super::arr(&s["gpus"])
                .iter()
                .find(|g| g["renderNode"].is_string())
        })
        .or_else(|| super::arr(&s["gpus"]).first());
    let gpu = gpu
        .map(|g| {
            [
                super::text(&g["vendor"]),
                super::text(&g["device"]),
                super::text(&g["driver"]),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
        })
        .unwrap_or_default();
    let kind = if !s["machine"]["virtualization"].is_null() && s["machine"]["virtualization"] != ""
    {
        "vm"
    } else if !s["raspberryPi"].is_null() || s["machine"]["firmware"] == "dt" {
        "board"
    } else if ["laptop", "handheld", "tablet"].contains(&super::text(&s["machine"]["chassis"])) {
        "laptop"
    } else if s["machine"]["chassis"] == "server" {
        "server"
    } else {
        "desktop"
    };
    let machine = super::model("machineName", json!([s]));
    let compat = super::arr(&report["compatibility"])
        .iter()
        .filter(|v| v["state"] != "absent")
        .map(|v| format!("{}={}", super::text(&v["id"]), super::text(&v["state"])))
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        ("machine", super::text(&machine).into()),
        ("kind", kind.into()),
        ("cpu", super::text(&s["cpu"]["model"]).into()),
        ("gpu", gpu),
        (
            "memory",
            format!(
                "{} GB",
                (s["memory"]["totalKB"].as_f64().unwrap_or(0.) / 1048576.).round()
            ),
        ),
        ("arch", super::text(&s["cpu"]["arch"]).into()),
        ("lordos", super::text(&report["lordos"]).into()),
        ("kernel", super::text(&s["software"]["kernel"]).into()),
        (
            "live",
            if report["live"] == true { "yes" } else { "no" }.into(),
        ),
        ("boot", super::text(&s["software"]["bootMedium"]).into()),
        ("bench", super::text(&report["bench"]["mode"]).into()),
        ("compat", compat),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .collect()
}
pub fn page(report: &Value, json_name: &str, i18n: &mut I18n) -> String {
    let rtl = i18n.entry(&i18n.lang.clone(), "meta", "direction") == Some(json!("rtl"));
    let mut out=format!("<!doctype html><html lang=\"{}\" dir=\"{}\"><head><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><style>body{{max-width:760px;margin:32px auto;padding:16px;font:16px/1.5 system-ui;color-scheme:light dark}}td,th{{text-align:start;padding:8px;vertical-align:top}}th{{width:30%}}code,td{{overflow-wrap:anywhere}}</style></head><body>",esc(&i18n.lang),if rtl {"rtl"} else {"ltr"});
    for (heading, keys) in [
        ("h1", vec!["title"]),
        ("p", vec!["lead"]),
        ("h2", vec!["summaryHead"]),
    ] {
        for key in keys {
            out.push_str(&format!(
                "<{heading}>{}</{heading}>",
                esc(&i18n.caption(&super::key(&format!("sysinfo.share.page.{key}"))))
            ));
        }
    }
    out.push_str("<table>");
    for (label, value) in summary(report) {
        out.push_str(&format!(
            "<tr><th>{}</th><td>{}</td></tr>",
            esc(&i18n.caption(&super::key(&format!("sysinfo.share.row.{label}")))),
            esc(&i18n.caption(&summary_caption(label, &value, report)))
        ));
    }
    out.push_str("</table>");
    for (heading, items) in [
        (
            "sharedHead",
            vec!["shared1", "shared2", "shared3", "shared4"],
        ),
        ("neverHead", vec!["never1", "never2", "never3"]),
        ("stepsHead", vec!["step1", "step2", "step3", "step4"]),
    ] {
        out.push_str(&format!(
            "<h2>{}</h2><ul>",
            esc(&i18n.caption(&super::key(&format!("sysinfo.share.page.{heading}"))))
        ));
        for item in items {
            out.push_str(&format!(
                "<li>{}</li>",
                esc(&i18n
                    .caption(&super::key(&format!("sysinfo.share.page.{item}")))
                    .replace("{file}", json_name))
            ));
        }
        out.push_str("</ul>");
    }
    // There is no configured public repository. Preserve the files-only share flow.
    out.push_str(&format!(
        "<p><code>{}</code></p><p>{}</p></body></html>",
        esc(json_name),
        esc(&i18n.caption(&super::key("sysinfo.share.page.footer")))
    ));
    out
}
pub fn community(snapshot: &Value, system: &Value) -> Value {
    let word = |s: &str| {
        !s.is_empty()
            && s.len() <= 40
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
    };
    if snapshot["format"] != "lordos-hwsnapshot" || snapshot["version"] != 1 {
        return json!({});
    }
    let version = base_version(&system["software"]["lordos"]);
    if version.is_empty() {
        return json!({});
    }
    let mut keys = Vec::<(String, String)>::new();
    let machine = &system["machine"];
    let clean = |s: &str, max: usize| {
        s.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(max)
            .collect::<String>()
    };
    let machine_key = if !super::text(&machine["vendor"]).is_empty()
        && !super::text(&machine["product"]).is_empty()
    {
        format!(
            "machine:{}|{}",
            clean(super::text(&machine["vendor"]), 48),
            clean(super::text(&machine["product"]), 64)
        )
    } else if !super::text(&machine["model"]).is_empty() {
        format!("model:{}", clean(super::text(&machine["model"]), 96))
    } else {
        String::new()
    };
    if !machine_key.is_empty() && super::text(&machine["virtualization"]).is_empty() {
        for component in ["display", "storage", "power", "suspend"] {
            keys.push((component.into(), machine_key.clone()));
        }
    }
    for gpu in super::arr(&system["gpus"]) {
        let hex = |v: &Value| {
            v.as_str()
                .filter(|s| {
                    !s.is_empty() && s.len() <= 8 && s.bytes().all(|c| c.is_ascii_hexdigit())
                })
                .map(str::to_owned)
        };
        if let (Some(vendor), Some(device)) = (hex(&gpu["vendorId"]), hex(&gpu["deviceId"])) {
            keys.push((
                "gpu".into(),
                format!("pci:{}:{}", vendor.to_lowercase(), device.to_lowercase()),
            ));
        } else if word(super::text(&gpu["driver"])) {
            keys.push(("gpu".into(), format!("gpu:{}", super::text(&gpu["driver"]))));
        }
    }
    for device in super::arr(&system["network"]) {
        let driver = super::text(&device["driver"]);
        if !word(driver) {
            continue;
        }
        match super::text(&device["kind"]) {
            "wifi" => keys.push(("wifi".into(), format!("wifi:{driver}"))),
            "ethernet" => keys.push(("network".into(), format!("net:{driver}"))),
            _ => {}
        }
    }
    for device in super::arr(&system["bluetooth"]) {
        if word(super::text(&device["driver"])) {
            keys.push((
                "bluetooth".into(),
                format!("bt:{}", super::text(&device["driver"])),
            ));
        }
    }
    for device in super::arr(&system["audio"]) {
        if !super::text(device).is_empty() {
            keys.push((
                "audio".into(),
                format!("audio:{}", clean(super::text(device), 64)),
            ));
        }
    }
    keys.sort();
    keys.dedup();
    let mut out = json!({});
    for (component, key) in keys {
        if let Some(device) = super::arr(&snapshot["devices"]).iter().rev().find(|d| {
            d["key"] == key && d["component"] == component && base_version(&d["lordos"]) == version
        }) {
            if out.get(&component).is_none() {
                out[&component] = json!({"works":0,"limits":0,"no":0,"reports":0,"references":0});
            }
            for field in ["works", "limits", "no", "reports", "references"] {
                out[&component][field] = json!(out[&component][field]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_add(device[field].as_u64().unwrap_or(0)));
            }
        }
    }
    out
}
