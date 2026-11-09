//! Read-only Linux hwmon and power-supply inputs. Missing sensors stay missing.
use super::{entries, number, read};
use serde_json::{json, Value};
use std::path::Path;
pub fn integer(text: &str) -> Option<i64> {
    let s = text.strip_suffix('\n').unwrap_or(text);
    if s.len() > 11
        || s.is_empty()
        || s.bytes()
            .enumerate()
            .any(|(i, b)| !b.is_ascii_digit() && !(i == 0 && b == b'-'))
    {
        return None;
    }
    s.parse().ok()
}
fn input(path: &Path) -> Option<f64> {
    integer(&read(path, 64)).map(|v| v as f64)
}
pub fn collect(root: &Path) -> Value {
    let mut temps = Vec::new();
    let mut fans = Vec::new();
    let mut chips = Vec::new();
    let mut supplies = Vec::new();
    for dir in entries(&root.join("sys/class/hwmon")).into_iter().take(32) {
        let name = read(&dir.join("name"), 128).trim().to_owned();
        let dev = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut rails = Vec::new();
        for path in entries(&dir).into_iter().take(512) {
            let file = path.file_name().unwrap_or_default().to_string_lossy();
            if let Some(key) = file.strip_suffix("_input") {
                let Some(v) = input(&path) else { continue };
                let label = read(&dir.join(format!("{key}_label")), 128)
                    .trim()
                    .to_owned();
                if key
                    .strip_prefix("temp")
                    .is_some_and(|s| s.parse::<u32>().is_ok())
                    && (-40000. ..=200000.).contains(&v)
                {
                    temps.push(json!({"dev":dev,"chip":name,"label":if label.is_empty(){key}else{&label},"value":v/1000.,"max":input(&dir.join(format!("{key}_max"))).map(|n|n/1000.),"crit":input(&dir.join(format!("{key}_crit"))).map(|n|n/1000.)}));
                } else if key
                    .strip_prefix("fan")
                    .is_some_and(|s| s.parse::<u32>().is_ok())
                    && (0. ..=100000.).contains(&v)
                {
                    fans.push(json!({"dev":dev,"chip":name,"label":if label.is_empty(){key}else{&label},"rpm":v}));
                } else if let Some(i) = key
                    .strip_prefix("in")
                    .and_then(|s| s.parse::<u32>().ok())
                    .filter(|i| *i < 256)
                {
                    if v.abs() <= 30000. && (!label.is_empty() || v != 0.) {
                        rails.push(json!({"i":i,"label":label,"v":v/1000.}));
                    }
                }
            }
        }
        rails.sort_by_key(|r| r["i"].as_u64().unwrap_or(0));
        if !rails.is_empty() {
            chips.push(json!({"dev":dev,"name":name,"in":rails}));
        }
    }
    if temps.is_empty() {
        for dir in entries(&root.join("sys/class/thermal"))
            .into_iter()
            .take(32)
        {
            if let Some(v) = input(&dir.join("temp")).filter(|v| (-40000. ..=200000.).contains(v)) {
                temps.push(json!({"label":read(&dir.join("type"),128).trim(),"value":v/1000.}));
            }
        }
    }
    for dir in entries(&root.join("sys/class/power_supply"))
        .into_iter()
        .take(32)
    {
        let Some(v) = input(&dir.join("voltage_now")).filter(|n| *n > 0. && *n <= 30000000.) else {
            continue;
        };
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        supplies.push(json!({"name":name,"v":v/1e6,"min":input(&dir.join("voltage_min_design")).map(|n|n/1e6),"battery":read(&dir.join("type"),128).trim()=="Battery","online":input(&dir.join("online")),"a":input(&dir.join("current_now")).map(|n|n/1e6),"capacity":input(&dir.join("capacity"))}));
    }
    let alarm = entries(&root.join("sys/class/hwmon"))
        .iter()
        .flat_map(|d| entries(d))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with("_alarm"))
        })
        .filter_map(|p| input(&p))
        .any(|v| v > 0.);
    let cpu = temps
        .iter()
        .find(|t| {
            ["coretemp", "k10temp", "zenpower", "cpu_thermal"]
                .contains(&t["chip"].as_str().unwrap_or(""))
        })
        .or_else(|| temps.first())
        .and_then(|t| t["value"].as_f64());
    json!({"temp":cpu,"fan":fans.first().and_then(|f|f["rpm"].as_f64()),"temps":temps,"fans":fans,"voltage":{"board":false,"alarm":if alarm{1}else{-1},"sinceBoot":-1,"chips":chips,"supplies":supplies}})
}
pub fn disk_temperature(root: &Path, name: &str, awake: bool) -> Value {
    if name.is_empty()
        || name.len() > 31
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return json!({"sensor":null,"temp":null,"tempIdle":!awake});
    }
    let dev = root.join("sys/block").join(name).join("device");
    let real = std::fs::canonicalize(&dev).unwrap_or(dev);
    let mut devices = vec![real.clone()];
    if let Some(nvme) = name.strip_prefix("nvme") {
        for entry in entries(&real) {
            let n = entry.file_name().unwrap_or_default().to_string_lossy();
            if n.strip_prefix("nvme")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            {
                if let Ok(path) = std::fs::canonicalize(entry) {
                    devices.push(path);
                }
            }
        }
        let digits: String = nvme.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            if let Ok(path) =
                std::fs::canonicalize(root.join("sys/class/nvme").join(format!("nvme{digits}")))
            {
                devices.push(path);
            }
        }
    }
    for dir in entries(&root.join("sys/class/hwmon")).into_iter().take(32) {
        let sensor = read(&dir.join("name"), 128).trim().to_owned();
        if !["nvme", "drivetemp"].contains(&sensor.as_str()) {
            continue;
        }
        let path = std::fs::canonicalize(dir.join("device")).unwrap_or_default();
        if path.as_os_str().is_empty() || !devices.iter().any(|real| real == &path) {
            continue;
        }
        let temp = if awake {
            input(&dir.join("temp1_input"))
                .filter(|n| (-40000. ..150000.).contains(n))
                .map(|n| n / 1000.)
        } else {
            None
        };
        let limit = |suffix: &str| {
            input(&dir.join(format!("temp1_{suffix}")))
                .filter(|n| (30000. ..=150000.).contains(n))
                .map(|n| n / 1000.)
        };
        let max = limit("max");
        let crit = limit("crit").filter(|c| max.is_none_or(|m| *c >= m));
        return json!({"sensor":sensor,"temp":temp,"tempMax":max,"tempCrit":crit,"tempIdle":!awake});
    }
    json!({"sensor":null,"temp":null,"tempIdle":!awake})
}
pub fn pressure(text: &str) -> Value {
    let mut out = json!({});
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(kind) = fields.next() else { continue };
        let mut v = json!({});
        for pair in fields {
            if let Some((k, n)) = pair.split_once('=') {
                if let Ok(n) = n.parse::<f64>() {
                    if n.is_finite() {
                        v[k] = json!(n)
                    }
                }
            }
        }
        out[kind] = v;
    }
    out
}
pub fn load(text: &str) -> Value {
    let v: Vec<_> = text.split_whitespace().collect();
    let loads: Vec<f64> = v.iter().take(3).map(|v| v.parse().unwrap_or(0.)).collect();
    let tasks = v
        .get(3)
        .and_then(|v| v.split_once('/'))
        .map(|(a, b)| (a.parse::<u64>().unwrap_or(0), b.parse::<u64>().unwrap_or(0)))
        .unwrap_or_default();
    json!({"load":loads,"tasks":{"running":tasks.0,"total":tasks.1}})
}
pub fn uptime(root: &Path) -> f64 {
    number(
        &format!("uptime {}", read(&root.join("proc/uptime"), 128)),
        "uptime",
    )
}
