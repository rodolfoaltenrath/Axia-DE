//! Hardware presentation policy. Collector data stays separate from the
//! on-screen-only peripheral names and serial-number queries.
#[path = "checks.rs"]
mod checks;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::OnceLock;
fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn truth(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().unwrap_or(0.) != 0.,
        _ => true,
    }
}
fn or(v: &Value, fallback: Value) -> Value {
    if truth(v) {
        v.clone()
    } else {
        fallback
    }
}
fn reason(s: &Value) -> Value {
    if s["composition"] == "gpu" {
        Value::Null
    } else if truth(&s["composition"]) && s["composition"] != "gpu-off" {
        s["composition"].clone()
    } else {
        or(&s["gpuInit"], json!("unknown"))
    }
}
fn card(g: &Value) -> Value {
    let name = [text(&g["vendor"]), text(&g["device"])]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let name = if name.is_empty() {
        format!(
            "{}:{}",
            or(&g["vendorId"], json!("?")).as_str().unwrap_or("?"),
            or(&g["deviceId"], json!("?")).as_str().unwrap_or("?")
        )
    } else {
        name
    };
    let kms = g["connectors"].as_i64().unwrap_or(0) > 0;
    json!([
        {"id":"device","state":"yes","vars":{"name":name}},
        {"id":"driver","state":if truth(&g["driver"]){"yes"}else{"no"},"vars":{"driver":or(&g["driver"],json!(""))}},
        {"id":"kms","state":if kms{"yes"}else{"no"},"note":if kms{None}else{Some("renderOnly")},"vars":{"connected":or(&g["connected"],json!(0)),"connectors":or(&g["connectors"],json!(0))}},
        {"id":"render","state":if truth(&g["renderNode"]){"yes"}else{"no"}}
    ])
}
fn session(s: &Value) -> Value {
    let on = s["composition"] == "gpu";
    let reason = reason(s);
    let renderer = text(&s["renderer"]).to_lowercase();
    let soft = ["llvmpipe", "softpipe", "swrast"]
        .iter()
        .any(|needle| renderer.contains(needle));
    json!([
        {"id":"egl","state":if on{"yes"}else{"no"},"note":if on{if soft{Some("software")}else{None}}else if reason=="software"{Some("softwareRefused")}else{None},"vars":{"renderer":or(&s["renderer"],json!(""))}},
        {"id":"composition","state":if on{"yes"}else{"no"},"note":if on{Value::Null}else{reason}},
        {"id":"scanout","state":if on{"yes"}else{"no"},"vars":{"count":or(&s["scanouts"],json!(0))},"note":if on{Some(if truth(&s["scanouts"]){"scanoutSeen"}else{"scanoutNone"})}else{None}},
        {"id":"dmabuf","state":if on&&truth(&s["dmabuf"]){"yes"}else{"no"}},
        {"id":"decode","state":"unchecked"},{"id":"encode","state":"unchecked"},{"id":"vulkan","state":"unchecked"}
    ])
}
fn compatibility(r: &Value) -> Value {
    let mut out = Vec::new();
    let mut add = |id: &str, state: &str, note: Option<String>, vars: Value| {
        out.push(json!({"id":id,"state":state,"note":note,"vars":vars}))
    };
    let s = &r["session"];
    if s["composition"] == "gpu" {
        add(
            "gpu",
            "works",
            Some("gpuComposes".into()),
            json!({"renderer":or(&s["renderer"],json!(""))}),
        );
    } else {
        add(
            "gpu",
            "limits",
            Some(if !arr(&r["gpus"]).is_empty() {
                format!("cpu.{}", text(&reason(s)))
            } else {
                "noGpu".into()
            }),
            json!({}),
        );
    }
    let ds = arr(&r["displays"]);
    let no_edid = ds.iter().any(|d| !truth(&d["nativeWidth"]));
    if !ds.is_empty() {
        add(
            "display",
            if no_edid { "limits" } else { "works" },
            no_edid.then(|| "noEdid".into()),
            json!({"count":ds.len()}),
        );
    } else {
        let headless = s["composition"] == "headless";
        add(
            "display",
            if headless { "unchecked" } else { "absent" },
            headless.then(|| "headless".into()),
            json!({}),
        );
    }
    let disks = arr(&r["storage"]);
    add(
        "storage",
        if disks.is_empty() { "absent" } else { "works" },
        None,
        json!({"count":disks.len()}),
    );
    let nets: Vec<_> = arr(&r["network"])
        .iter()
        .filter(|n| matches!(text(&n["kind"]), "ethernet" | "wifi"))
        .collect();
    if nets.is_empty() {
        add("network", "absent", None, json!({}));
    } else {
        let up = nets.iter().any(|n| truth(&n["up"]));
        add(
            "network",
            if up { "works" } else { "limits" },
            (!up).then(|| "notConnected".into()),
            json!({}),
        );
    }
    let wifi: Vec<_> = arr(&r["network"])
        .iter()
        .filter(|n| n["kind"] == "wifi")
        .collect();
    if wifi.is_empty() {
        add("wifi", "absent", None, json!({}));
    } else {
        let ok = wifi.iter().all(|n| truth(&n["driver"]));
        add(
            "wifi",
            if ok { "works" } else { "limits" },
            (!ok).then(|| "noDriver".into()),
            json!({"driver":or(&wifi[0]["driver"],json!(""))}),
        );
    }
    if let Some(bt) = arr(&r["bluetooth"]).first() {
        let ok = truth(&bt["driver"]);
        add(
            "bluetooth",
            if ok { "works" } else { "limits" },
            (!ok).then(|| "noDriver".into()),
            json!({"driver":or(&bt["driver"],json!(""))}),
        );
    } else {
        add("bluetooth", "absent", None, json!({}));
    }
    let audio = arr(&r["audio"]);
    add(
        "audio",
        if audio.is_empty() { "absent" } else { "works" },
        None,
        json!({"count":audio.len()}),
    );
    if let Some(bat) = arr(&r["power"]).iter().find(|p| p["kind"] == "battery") {
        let known = !bat["health"].is_null();
        let worn = known && bat["health"].as_f64().unwrap_or(100.) < 70.;
        let vars = bat
            .get("health")
            .map_or_else(|| json!({}), |health| json!({"health":health}));
        add(
            "power",
            if worn { "limits" } else { "works" },
            if worn {
                Some("batteryWorn".into())
            } else if known {
                Some("batteryHealth".into())
            } else {
                None
            },
            vars,
        );
    } else {
        add("power", "absent", None, json!({}));
    }
    let sw = &r["software"];
    let ll = &sw["landlockAbi"];
    let mut missing = Vec::new();
    if ll.as_i64() == Some(0) {
        missing.push("landlock");
    }
    if !truth(&sw["bpfLsm"]) {
        missing.push("bpfLsm");
    }
    if !truth(&sw["cgroup2"]) {
        missing.push("cgroup2");
    }
    if ll.is_null() && missing.is_empty() {
        add("isolation", "works", None, json!({}));
    } else {
        add(
            "isolation",
            if missing.is_empty() {
                "works"
            } else {
                "limits"
            },
            missing.first().map(|m| format!("missing.{m}")),
            json!({"abi":or(ll,json!(0))}),
        );
    }
    add("suspend", "unchecked", None, json!({}));
    json!(out)
}
fn hardware(r: &Value) -> Value {
    let keys = [
        "computer",
        "cpu",
        "memory",
        "graphics",
        "displays",
        "storage",
        "network",
        "audio",
        "bluetooth",
        "peripherals",
        "power",
        "thermal",
        "pci",
        "usb",
        "raspberryPi",
        "software",
    ];
    json!(keys
        .into_iter()
        .filter(|key| match *key {
            "computer" | "cpu" | "memory" | "peripherals" | "software" => true,
            "graphics" => !arr(&r["gpus"]).is_empty() || truth(&r["session"]),
            "raspberryPi" => truth(&r[*key]),
            _ => !arr(&r[*key]).is_empty(),
        })
        .collect::<Vec<_>>())
}
fn pi_board(v: &Value) -> Value {
    let s = text(v).trim_start();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    let digits: String = s.chars().take_while(char::is_ascii_hexdigit).collect();
    let Some(code) = u32::from_str_radix(&digits, 16)
        .ok()
        .filter(|v| v & (1 << 23) != 0)
    else {
        return Value::Null;
    };
    let product = match (code >> 4) & 0xff {
        4 => "Raspberry Pi 2 Model B",
        8 => "Raspberry Pi 3 Model B",
        9 => "Raspberry Pi Zero",
        12 => "Raspberry Pi Zero W",
        13 => "Raspberry Pi 3 Model B+",
        14 => "Raspberry Pi 3 Model A+",
        17 => "Raspberry Pi 4 Model B",
        18 => "Raspberry Pi Zero 2 W",
        19 => "Raspberry Pi 400",
        20 => "Raspberry Pi Compute Module 4",
        21 => "Raspberry Pi Compute Module 4S",
        23 => "Raspberry Pi 5 Model B",
        24 => "Raspberry Pi Compute Module 5",
        25 => "Raspberry Pi 500",
        26 => "Raspberry Pi Compute Module 5 Lite",
        _ => "",
    };
    let chip = ["BCM2835", "BCM2836", "BCM2837", "BCM2711", "BCM2712"]
        .get(((code >> 12) & 15) as usize)
        .copied()
        .unwrap_or("");
    let maker = [
        "Sony UK",
        "Egoman",
        "Embest",
        "Sony Japan",
        "Embest",
        "Stadium",
    ]
    .get(((code >> 16) & 15) as usize)
    .copied()
    .unwrap_or("");
    let mem = [0.25, 0.5, 1., 2., 4., 8., 16.]
        .get(((code >> 20) & 7) as usize)
        .copied()
        .unwrap_or(0.);
    json!({"product":product,"chip":chip,"maker":maker,"memoryGB":mem,"version":format!("1.{}",code&15)})
}
fn flags(v: &Value) -> Value {
    if v.is_null() || v.as_f64().unwrap_or(0.) < 0. {
        return Value::Null;
    }
    let n = v.as_u64().unwrap_or(0) as u32;
    let keys = [
        "underVoltageNow",
        "cappedNow",
        "throttledNow",
        "softLimitNow",
        "underVoltage",
        "capped",
        "throttled",
        "softLimit",
    ];
    let mut out = json!({});
    for (i, key) in keys.into_iter().enumerate() {
        out[key] = json!(n & (1 << if i < 4 { i } else { i + 12 }) != 0);
    }
    out
}
fn pi_status(pi: &Value) -> Value {
    let f = flags(&pi["throttled"]);
    let recent = pi["underVoltageAlarm"].is_boolean();
    let mut power = None;
    let mut throttling = None;
    if !f.is_null() {
        power = Some(if f["underVoltageNow"] == true {
            "underVoltageNow"
        } else if f["underVoltage"] == true {
            if recent {
                "underVoltageRecent"
            } else {
                "underVoltageSeen"
            }
        } else if recent {
            "powerOkRecent"
        } else {
            "powerOk"
        });
        throttling = Some(
            if ["throttledNow", "cappedNow", "softLimitNow"]
                .iter()
                .any(|k| f[*k] == true)
            {
                "throttledNow"
            } else if ["throttled", "capped", "softLimit"]
                .iter()
                .any(|k| f[*k] == true)
            {
                if recent {
                    "throttledRecent"
                } else {
                    "throttledSeen"
                }
            } else if recent {
                "throttleOkRecent"
            } else {
                "throttleOk"
            },
        );
    } else if recent {
        power = Some(if pi["underVoltageAlarm"] == true {
            "underVoltageRecent"
        } else {
            "powerOkRecent"
        });
    }
    json!({"power":power,"throttling":throttling})
}
fn peripherals(local: &Value, bt: &Value) -> Value {
    static ADDRESS: OnceLock<Regex> = OnceLock::new();
    let address =
        ADDRESS.get_or_init(|| Regex::new(r"(?i)^[0-9a-f]{2}([:-][0-9a-f]{2}){5}$").unwrap());
    let battery = |p: &Value| {
        if p["battery"].as_f64().is_some_and(|b| b >= 0.) {
            p["battery"].clone()
        } else {
            Value::Null
        }
    };
    let mut out:Vec<_>=arr(local).iter().map(|p|json!({"name":or(&p["name"],json!("")),"kind":or(&p["kind"],json!("other")),"connection":or(&p["connection"],json!("usb")),"battery":battery(p),"connected":true})).collect();
    for d in arr(&bt["devices"]) {
        if !truth(&d["paired"]) && !truth(&d["connected"]) {
            continue;
        }
        let icon = text(&d["icon"]);
        let kind = if icon.contains("keyboard") {
            "keyboard"
        } else if icon.contains("mouse") {
            "mouse"
        } else if icon.contains("tablet") {
            "tablet"
        } else if icon.contains("gaming") || icon.contains("joystick") {
            "gamepad"
        } else if ["audio", "headset", "headphone"]
            .iter()
            .any(|s| icon.contains(s))
        {
            "audio"
        } else if icon.contains("phone") {
            "phone"
        } else {
            "other"
        };
        let name = text(&d["name"]);
        out.push(json!({"name":if address.is_match(name){""}else{name},"kind":kind,"connection":"bluetooth","battery":battery(d),"connected":truth(&d["connected"])}));
    }
    let kinds = [
        "keyboard", "mouse", "touchpad", "receiver", "gamepad", "tablet", "input", "audio",
    ];
    out.sort_by_key(|p| {
        kinds
            .iter()
            .position(|k| p["kind"] == *k)
            .unwrap_or(kinds.len())
    });
    json!(out)
}
fn serials(lines: &Value) -> Value {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^serial (system|board|device) ([A-Za-z0-9 ._:/-]{1,63})$").unwrap()
    });
    let mut out: Vec<Value> = Vec::new();
    for line in arr(lines).iter().filter_map(Value::as_str) {
        if let Some(c) = re.captures(line) {
            if !out.iter().any(|p| p["kind"] == &c[1]) {
                out.push(json!({"kind":&c[1],"value":&c[2]}));
            }
        }
    }
    json!(out)
}
fn memory(lines: &Value) -> Value {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re=RE.get_or_init(||Regex::new(r"^mem ([0-9]{1,9})\|([A-Za-z0-9]{0,11})\|([0-9]{1,6})\|([0-9]{1,6})\|([\x20-\x7e]{0,31})\|([\x20-\x7e]{0,31})\|([\x20-\x7e]{0,31})$").unwrap());
    let mut out = Vec::new();
    for line in arr(lines).iter().filter_map(Value::as_str) {
        if let Some(c) = re.captures(line) {
            if [5, 6, 7].iter().any(|i| c[*i].contains('|')) {
                continue;
            }
            out.push(json!({"sizeMB":c[1].parse::<u64>().unwrap(),"type":&c[2],"speed":c[3].parse::<u64>().unwrap(),"configured":c[4].parse::<u64>().unwrap(),"slot":&c[5],"bank":&c[6],"maker":&c[7]}));
            if out.len() >= 64 {
                break;
            }
        }
    }
    json!(out)
}
pub fn request(input: &Value) -> Value {
    let a = &input["args"];
    match input["cmd"].as_str().unwrap_or("") {
        "compositionReason" => reason(&a[0]),
        "cardLayers" => card(&a[0]),
        "sessionLayers" => session(&a[0]),
        "compatibility" => compatibility(&a[0]),
        "hardwareSections" => hardware(&a[0]),
        "piBoard" => pi_board(&a[0]),
        "throttleFlags" => flags(&a[0]),
        "piStatus" => pi_status(&a[0]),
        "peripherals" => peripherals(&a[0], &a[1]),
        "parseSerialLines" => serials(&a[0]),
        "parseMemoryLines" => memory(&a[0]),
        "machineName" => {
            let m = &a[0]["machine"];
            let name = [text(&m["vendor"]), text(&m["product"])]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            json!(if name.is_empty() {
                text(&m["model"]).to_owned()
            } else {
                name
            })
        }
        _ => checks::request(input),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn normalize(v: &Value) -> Value {
        match v {
            Value::Number(n) => json!(n.as_f64().unwrap()),
            Value::Array(a) => json!(a.iter().map(normalize).collect::<Vec<_>>()),
            Value::Object(o) => {
                Value::Object(o.iter().map(|(k, v)| (k.clone(), normalize(v))).collect())
            }
            _ => v.clone(),
        }
    }
    #[test]
    fn hardware_matches_pre_migration_javascript() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../rust/axia-shell/tests/fixtures/sysinfo_hardware.json"
        ))
        .unwrap();
        for (i, c) in arr(&cases).iter().enumerate() {
            assert_eq!(
                normalize(&request(&c["input"])),
                normalize(&c["expected"]),
                "hardware reference {i}: {}",
                c["input"]
            );
        }
    }
}
