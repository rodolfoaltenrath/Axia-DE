//! smartctl's JSON (smartmontools, `smartctl -j`) turned into one model the Disks app and the
//! Monitor can show (docs/SYSTEM.md, "SMART with smartmontools"): what sdk/smartparse.js was.
//! Reached through the launcher bridge as `{"op":"smart","cmd":…}`.
//!
//! Everything a drive sends is outside data: numbers are checked, strings are only shown as text
//! (cut at the same lengths as before; they count characters, JS counted UTF-16 units).
use serde_json::{json, Value};

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}
fn arr(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}
fn text(v: &Value, max: usize) -> String {
    v.as_str().map(|s| s.chars().take(max).collect()).unwrap_or_default()
}
fn opt(n: Option<f64>) -> Value {
    n.map_or(Value::Null, |n| json!(n))
}
/// JS `String(n)` for the numbers a drive reports.
fn js_string(n: f64) -> String {
    if n.fract() == 0. && n.abs() < 1e21 {
        format!("{}", n as i128)
    } else {
        format!("{n}")
    }
}

/// smartctl's attribute names → our plain-language strings (disks.attr.<key>). By name, not by id:
/// vendors reuse ids, and smartctl's drive database already gives each one its real meaning.
const NAME_KEYS: &[(&str, &str)] = &[
    ("Raw_Read_Error_Rate", "a1"), ("Throughput_Performance", "a2"), ("Spin_Up_Time", "a3"), ("Start_Stop_Count", "a4"),
    ("Reallocated_Sector_Ct", "a5"), ("Reallocate_NAND_Blk_Cnt", "a5"), ("Seek_Error_Rate", "a7"), ("Seek_Time_Performance", "a8"),
    ("Power_On_Hours", "a9"), ("Power_On_Hours_and_Msec", "a9"), ("Spin_Retry_Count", "a10"), ("Calibration_Retry_Count", "a11"),
    ("Power_Cycle_Count", "a12"), ("Read_Soft_Error_Rate", "a13"), ("Helium_Level", "a22"), ("Program_Fail_Count", "a171"),
    ("Program_Fail_Cnt", "a171"), ("Erase_Fail_Count", "a172"), ("Ave_Block_Erase_Count", "a173"), ("Unexpect_Power_Loss_Ct", "a174"),
    ("Unsafe_Shutdown_Count", "a174"), ("Wear_Leveling_Count", "a177"), ("Used_Rsvd_Blk_Cnt_Tot", "a179"), ("Unused_Rsvd_Blk_Cnt_Tot", "a180"),
    ("Program_Fail_Cnt_Total", "a181"), ("Erase_Fail_Count_Total", "a182"), ("Runtime_Bad_Block", "a183"), ("SATA_Downshift_Count", "a183"),
    ("End-to-End_Error", "a184"), ("Reported_Uncorrect", "a187"), ("Command_Timeout", "a188"), ("High_Fly_Writes", "a189"),
    ("Airflow_Temperature_Cel", "a190"), ("Temperature_Case", "a190"), ("G-Sense_Error_Rate", "a191"), ("Power-Off_Retract_Count", "a192"),
    ("Load_Cycle_Count", "a193"), ("Temperature_Celsius", "a194"), ("Temperature_Internal", "a194"), ("Hardware_ECC_Recovered", "a195"),
    ("Reallocated_Event_Count", "a196"), ("Current_Pending_Sector", "a197"), ("Offline_Uncorrectable", "a198"), ("UDMA_CRC_Error_Count", "a199"),
    ("CRC_Error_Count", "a199"), ("Multi_Zone_Error_Rate", "a200"), ("Percent_Lifetime_Remain", "a202"), ("Disk_Shift", "a220"),
    ("Loaded_Hours", "a222"), ("Load_Retry_Count", "a223"), ("Load_Friction", "a224"), ("Load_In_Time", "a226"), ("SSD_Life_Left", "a231"),
    ("Available_Reservd_Space", "a232"), ("Media_Wearout_Indicator", "a233"), ("Total_LBAs_Written", "a241"), ("Total_LBAs_Read", "a242"),
    ("Head_Flying_Hours", "a240"), ("Free_Fall_Sensor", "a254"),
];

fn key_of(name: &str) -> Option<&'static str> {
    NAME_KEYS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

/// The distinct keys, in the order of the table.
fn attr_keys() -> Vec<&'static str> {
    let mut out: Vec<&str> = Vec::new();
    for &(_, k) in NAME_KEYS {
        if !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

/// "Unknown_SSD_Attribute" → "Unknown SSD Attribute"
fn pretty_attr_name(n: &Value) -> String {
    text(n, 64).replace('_', " ").trim().to_owned()
}

/// A count attribute's raw number: smartctl's raw string starts with it ("12 (Min/Max 0/40)").
fn raw_count(a: &Value) -> Option<f64> {
    let s = text(&a["raw"]["string"], 64);
    let s = s.trim();
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    let next = s[digits.len()..].chars().next();
    let boundary = next.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
    if !digits.is_empty() && digits.len() <= 15 && boundary {
        return digits.parse().ok();
    }
    num(&a["raw"]["value"]).filter(|v| *v >= 0.)
}

fn parse_attrs(j: &Value) -> Vec<Value> {
    arr(&j["ata_smart_attributes"]["table"])
        .iter()
        .filter(|a| a.is_object() && num(&a["id"]).is_some_and(|id| id > 0. && id < 256.))
        .take(64)
        .map(|a| {
            let name = text(&a["name"], 64);
            let raw = {
                let t = text(&a["raw"]["string"], 64);
                if !t.is_empty() {
                    t
                } else {
                    num(&a["raw"]["value"]).map(js_string).unwrap_or_default()
                }
            };
            json!({
                "id": a["id"], "name": name, "key": key_of(&name),
                "value": opt(num(&a["value"])), "worst": opt(num(&a["worst"])), "thresh": opt(num(&a["thresh"])),
                "raw": raw, "count": opt(raw_count(a)),
                "prefail": a["flags"]["prefailure"] == true,
                "failed": match a["when_failed"].as_str() { Some("now") => "now", Some("past") => "past", _ => "" },
            })
        })
        .collect()
}

fn by_key<'a>(attrs: &'a [Value], keys: &[&str]) -> Option<&'a Value> {
    attrs.iter().find(|a| a["key"].as_str().is_some_and(|k| keys.contains(&k)))
}

/// The percentage of the rated life already used (SSD wear), from the most reliable source available.
fn wear_of(j: &Value, attrs: &[Value], nvme: &Value) -> Value {
    if let Some(used) = num(&nvme["used"]) {
        return json!({"used": used, "source": "nvme"});
    }
    for p in arr(&j["ata_device_statistics"]["pages"]) {
        for e in arr(&p["table"]) {
            if text(&e["name"], 200).to_lowercase().contains("percentage used endurance indicator") {
                if let Some(v) = num(&e["value"]).filter(|v| (0. ..=255.).contains(v)) {
                    return json!({"used": v, "source": "devstat"});
                }
            }
        }
    }
    if let Some(v) = num(&j["scsi_percentage_used_endurance_indicator"]) {
        return json!({"used": v, "source": "scsi"});
    }
    // normalized values that count down from 100 as the flash wears out
    if let Some(a) = by_key(attrs, &["a231", "a233", "a202", "a177"]) {
        if let Some(v) = num(&a["value"]).filter(|v| (0. ..=100.).contains(v)) {
            return json!({"used": 100. - v, "source": format!("attr{}", a["id"])});
        }
    }
    Value::Null
}

fn parse_nvme(j: &Value) -> Value {
    let h = &j["nvme_smart_health_information_log"];
    if !h.is_object() {
        return Value::Null;
    }
    let units = |v: &Value| num(v).map_or(Value::Null, |n| json!(n * 512_000.)); // data units are 1000 × 512 bytes
    json!({
        "criticalWarning": num(&h["critical_warning"]).unwrap_or(0.),
        "temp": opt(num(&h["temperature"])),
        "spare": opt(num(&h["available_spare"])), "spareThreshold": opt(num(&h["available_spare_threshold"])),
        "used": opt(num(&h["percentage_used"])),
        "read": units(&h["data_units_read"]), "written": units(&h["data_units_written"]),
        "hostReads": opt(num(&h["host_reads"])), "hostWrites": opt(num(&h["host_writes"])),
        "busyMinutes": opt(num(&h["controller_busy_time"])),
        "cycles": opt(num(&h["power_cycles"])), "hours": opt(num(&h["power_on_hours"])),
        "unsafeShutdowns": opt(num(&h["unsafe_shutdowns"])), "mediaErrors": opt(num(&h["media_errors"])),
        "errorLogEntries": opt(num(&h["num_err_log_entries"])),
        "warningTempMinutes": opt(num(&h["warning_temp_time"])), "criticalTempMinutes": opt(num(&h["critical_comp_time"])),
        "sensors": arr(&h["temperature_sensors"]).iter().filter_map(num).take(8).collect::<Vec<_>>(),
    })
}

/// ATA status 0xF?: a test in progress.
fn ata_test_running(v: Option<f64>) -> bool {
    v.is_some_and(|v| (v as i64) >> 4 == 15)
}

fn parse_self_test(j: &Value, nvme: &Value) -> Value {
    let data = &j["ata_smart_data"];
    let polling = &data["self_test"]["polling_minutes"];
    let st = &data["self_test"]["status"];
    let caps = &data["capabilities"];
    let mut res = json!({
        "supported": false, "running": false, "remaining": null,
        "polling": {"short": opt(num(&polling["short"])), "long": opt(num(&polling["extended"]))},
    });
    if caps.is_object() {
        res["supported"] = json!(truthy(&caps["self_tests_supported"]));
    }
    if st.is_object() {
        let running = ata_test_running(num(&st["value"]));
        res["running"] = json!(running);
        if running {
            res["remaining"] = opt(num(&st["remaining_percent"]).or(Some(f64::from((num(&st["value"]).unwrap_or(0.) as i64 & 15) as i32) * 10.)));
        }
        res["last"] = json!({"result": text(&st["string"], 120), "passed": st["passed"] != false});
    }
    let nl = &j["nvme_self_test_log"];
    if !nvme.is_null() || nl.is_object() {
        let o = &j["nvme_optional_admin_commands"];
        res["supported"] = json!(nl.is_object() || (o.is_object() && truthy(&o["self_test"])));
        if num(&nl["current_self_test_operation"]["value"]).is_some_and(|v| v != 0.) {
            res["running"] = json!(true);
            res["remaining"] = num(&nl["current_self_test_completion_percent"]).map_or(Value::Null, |done| json!((100. - done).max(0.)));
        }
        let ext = num(&j["nvme_extended_self_test_time"]).or_else(|| num(&j["nvme_self_test_time"]["extended"]));
        if let Some(e) = ext {
            res["polling"]["long"] = json!(e);
        }
    }
    res
}

/// JS truthiness for the flags a drive reports.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn nvme_result(r: i64) -> Option<&'static str> {
    Some(match r {
        0 => "completed",
        1 => "abortedCommand",
        2 => "abortedReset",
        3 => "abortedRemoved",
        4 => "abortedFormat",
        5 => "fatal",
        6 => "failedUnknownSegment",
        7 => "failedSegments",
        8 => "abortedUnknown",
        9 => "abortedSanitize",
        _ => return None,
    })
}

fn parse_self_tests(j: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let log = &j["ata_smart_self_test_log"];
    let table = if arr(&log["extended"]["table"]).is_empty() { arr(&log["standard"]["table"]) } else { arr(&log["extended"]["table"]) };
    for e in table.iter().take(32).filter(|e| e.is_object()) {
        let t = text(&e["type"]["string"], 200).to_lowercase();
        out.push(json!({
            "type": if t.contains("extended") || t.contains("long") { "long" } else if t.contains("short") { "short" } else { "other" },
            "typeText": text(&e["type"]["string"], 60),
            "result": text(&e["status"]["string"], 120),
            "passed": e["status"]["passed"] != false,
            "hours": opt(num(&e["lifetime_hours"])),
        }));
    }
    for e in arr(&j["nvme_self_test_log"]["table"]).iter().take(32).filter(|e| e.is_object()) {
        let code = num(&e["self_test_code"]["value"]);
        let r = num(&e["self_test_result"]["value"]);
        out.push(json!({
            "type": if code == Some(1.) { "short" } else if code == Some(2.) { "long" } else { "other" },
            "typeText": text(&e["self_test_code"]["string"], 60),
            "result": text(&e["self_test_result"]["string"], 120),
            "resultKey": r.and_then(|r| nvme_result(r as i64)),
            "passed": r == Some(0.),
            "hours": opt(num(&e["power_on_hours"])),
        }));
    }
    out
}

fn parse_errors(j: &Value, nvme: &Value) -> Value {
    let log = &j["ata_smart_error_log"];
    let part = if log["extended"].is_object() { &log["extended"] } else { &log["summary"] };
    if part.is_object() {
        return json!({
            "count": num(&part["count"]).unwrap_or(0.),
            "list": arr(&part["table"]).iter().filter(|e| e.is_object()).take(16)
                .map(|e| json!({"n": opt(num(&e["error_number"])), "hours": opt(num(&e["lifetime_hours"])), "text": text(&e["error_description"], 160)}))
                .collect::<Vec<_>>(),
        });
    }
    let ne = &j["nvme_error_information_log"];
    if ne.is_object() || !nvme.is_null() {
        return json!({
            "count": num(&nvme["errorLogEntries"]).or_else(|| num(&ne["unread"])).unwrap_or(0.),
            "list": arr(&ne["table"]).iter().filter(|e| e.is_object()).take(16)
                .map(|e| json!({"n": opt(num(&e["error_count"])), "hours": null, "text": text(&e["status_field"]["string"], 160)}))
                .collect::<Vec<_>>(),
        });
    }
    json!({"count": 0, "list": []})
}

fn parse_temp_history(j: &Value) -> Value {
    let h = &j["ata_sct_temperature_history"];
    if !h.is_object() {
        return Value::Null;
    }
    let values: Vec<Value> = arr(&h["table"]).iter().take(1024).map(|v| num(v).filter(|v| *v > -40. && *v < 150.).map_or(Value::Null, |v| json!(v))).collect();
    if values.iter().all(Value::is_null) {
        return Value::Null;
    }
    json!({"interval": num(&h["logging_interval_minutes"]).or_else(|| num(&h["sampling_period_minutes"])).unwrap_or(1.), "values": values})
}

fn temp_limits(j: &Value) -> (Option<f64>, Option<f64>) {
    let t = &j["temperature"];
    let sct = &j["ata_sct_temperature_history"]["temperature"];
    let max = num(&t["op_limit_max"]).or_else(|| num(&sct["op_limit_max"])).or_else(|| num(&j["nvme_warning_composite_temperature_threshold"]));
    let crit = num(&t["critical_limit_max"]).or_else(|| num(&sct["limit_max"])).or_else(|| num(&j["nvme_critical_composite_temperature_threshold"]));
    let ok = |v: Option<f64>| v.filter(|v| (30. ..=150.).contains(v));
    (ok(max), ok(crit))
}

/// `\b(STANDBY|SLEEP|IDLE(_[A-Z])?) mode\b`
fn is_standby(s: &str) -> bool {
    let b = s.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    for (at, _) in s.match_indices(" mode") {
        let end = at + 5;
        if end < b.len() && word(b[end]) {
            continue;
        }
        let head = &s[..at];
        for name in ["STANDBY", "SLEEP", "IDLE"] {
            let mut tails = vec![name.to_owned()];
            if name == "IDLE" {
                tails.extend(('A'..='Z').map(|c| format!("IDLE_{c}")));
            }
            for full in tails {
                if head.ends_with(&full) {
                    let start = head.len() - full.len();
                    if start == 0 || !word(head.as_bytes()[start - 1]) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Health from what the drive says about itself; never guessed when there is no data.
fn judge(m: &Value) -> (String, Vec<String>) {
    let (mut fail, mut warn): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    if m["passed"] == false {
        fail.push("smartFailed");
    }
    for a in arr(&m["attrs"]) {
        let (failed, prefail) = (a["failed"].as_str().unwrap_or(""), a["prefail"] == true);
        if failed == "now" && prefail {
            fail.push("prefailNow");
        } else if failed == "now" {
            warn.push("oldAgeNow");
        } else if failed == "past" && prefail {
            warn.push("prefailPast");
        }
    }
    let gt0 = |v: &Value| num(v).is_some_and(|n| n > 0.);
    if gt0(&m["reallocated"]) {
        warn.push("reallocated");
    }
    if gt0(&m["pending"]) {
        warn.push("pending");
    }
    if gt0(&m["uncorrectable"]) {
        warn.push("uncorrectable");
    }
    if gt0(&m["grownDefects"]) {
        warn.push("grownDefects");
    }
    let n = &m["nvme"];
    if n.is_object() {
        let w = num(&n["criticalWarning"]).unwrap_or(0.) as i64;
        if w & 0x04 != 0 {
            fail.push("nvmeReliability");
        }
        if w & 0x08 != 0 {
            fail.push("nvmeReadOnly");
        }
        let low_spare = matches!((num(&n["spare"]), num(&n["spareThreshold"])), (Some(s), Some(t)) if s < t);
        if w & 0x01 != 0 || low_spare {
            warn.push("nvmeSpare");
        }
        if w & 0x02 != 0 {
            warn.push("nvmeTemp");
        }
        if w & 0x10 != 0 {
            warn.push("nvmeBackup");
        }
        if gt0(&n["mediaErrors"]) {
            warn.push("mediaErrors");
        }
    }
    if num(&m["wear"]["used"]).is_some_and(|u| u >= 90.) {
        warn.push("worn");
    }
    if m["passed"].is_null() && (num(&m["exit"]).unwrap_or(0.) as i64) & 8 != 0 {
        fail.push("smartFailed"); // smartctl: "DISK FAILING"
    }
    let mut reasons: Vec<String> = Vec::new();
    for r in fail.iter().chain(warn.iter()) {
        if !reasons.iter().any(|x| x == r) {
            reasons.push((*r).to_owned());
        }
    }
    let known = !m["passed"].is_null() || n.is_object() || !arr(&m["attrs"]).is_empty();
    let health = if !fail.is_empty() { "failing" } else if !warn.is_empty() { "warning" } else if known { "ok" } else { "unknown" };
    (health.to_owned(), reasons)
}

fn parse_smartctl(j: &Value, exit: &Value) -> Value {
    let empty = json!({});
    let j = if j.is_object() { j } else { &empty };
    let sc = &j["smartctl"];
    let messages: Vec<Value> = arr(&sc["messages"]).iter().filter(|m| m.is_object()).map(|m| json!({"text": text(&m["string"], 200), "severity": text(&m["severity"], 16)})).take(8).collect();
    let status = num(&sc["exit_status"]).or_else(|| num(exit)).unwrap_or(0.);
    let protocol = text(&j["device"]["protocol"], 16);
    let kind = match protocol.as_str() {
        "NVMe" => Some("nvme"),
        "ATA" => Some("ata"),
        "SCSI" => Some("scsi"),
        _ => None,
    };
    let nvme = parse_nvme(j);
    let attrs = parse_attrs(j);
    let support = &j["smart_support"];
    let smart_avail = if support.is_object() { support["available"] != false } else { !nvme.is_null() || !attrs.is_empty() };
    let standby = !j["smart_status"].is_object() && messages.iter().any(|m| is_standby(m["text"].as_str().unwrap_or("")));
    let temp = num(&j["temperature"]["current"]).or_else(|| num(&nvme["temp"]));
    let (tmax, tcrit) = temp_limits(j);
    let attr_count = |k: &str| by_key(&attrs, &[k]).map_or(Value::Null, |a| a["count"].clone());
    let model = {
        let m = text(&j["model_name"], 80);
        let m = if m.is_empty() { text(&j["scsi_model_name"], 80) } else { m };
        if m.is_empty() {
            [text(&j["scsi_vendor"], 40), text(&j["scsi_product"], 40)].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ")
        } else {
            m
        }
    };
    let firmware = {
        let f = text(&j["firmware_version"], 40);
        if f.is_empty() { text(&j["scsi_revision"], 40) } else { f }
    };
    let passed = if j["smart_status"].is_object() && !(support.is_object() && support["available"] == false) { json!(j["smart_status"]["passed"] != false) } else { Value::Null };
    let mut m = json!({
        "kind": kind,
        "model": model,
        "serial": text(&j["serial_number"], 64),
        "firmware": firmware,
        "capacity": opt(num(&j["user_capacity"]["bytes"]).or_else(|| num(&j["nvme_total_capacity"]))),
        "rotation": opt(num(&j["rotation_rate"])),
        "supported": kind.is_some() && smart_avail,
        "enabled": if support.is_object() { support["enabled"] != false } else { true },
        "standby": standby,
        // a device that says it has no SMART (an emulated disk) gets no verdict, whatever -H printed
        "passed": passed,
        "temp": opt(temp.filter(|t| *t > -40. && *t < 150. && !(kind == Some("scsi") && *t == 0.))),
        "tempMax": opt(tmax), "tempCrit": opt(tcrit),
        "hours": opt(num(&j["power_on_time"]["hours"]).or_else(|| num(&nvme["hours"]))),
        "cycles": opt(num(&j["power_cycle_count"]).or_else(|| num(&nvme["cycles"]))),
        "reallocated": attr_count("a5"),
        "pending": attr_count("a197"),
        "uncorrectable": attr_count("a198"),
        "grownDefects": opt(num(&j["scsi_grown_defect_list"])),
        "attrs": attrs,
        "nvme": nvme.clone(),
        "wear": null,
        "read": nvme["read"].clone(),
        "written": nvme["written"].clone(),
        "selfTest": parse_self_test(j, &nvme),
        "selfTests": parse_self_tests(j),
        "errors": parse_errors(j, &nvme),
        "tempHistory": parse_temp_history(j),
        "messages": messages,
        "exit": status,
        "health": "unknown",
        "reasons": [],
    });
    if nvme.is_null() {
        let attrs = arr(&m["attrs"]).to_vec();
        if let Some(n) = by_key(&attrs, &["a241"]).and_then(|a| num(&a["count"])) {
            m["written"] = json!(n * 512.);
        }
        if let Some(n) = by_key(&attrs, &["a242"]).and_then(|a| num(&a["count"])) {
            m["read"] = json!(n * 512.);
        }
    }
    m["wear"] = wear_of(j, &arr(&m["attrs"]).to_vec(), &nvme);
    let (health, reasons) = judge(&m);
    m["health"] = json!(health);
    m["reasons"] = json!(reasons);
    m
}

/// The little summary the Monitor keeps: `{health, reasons, temp, wear, hours, standby, supported, t}`.
fn summarize(m: &Value, t: &Value) -> Value {
    json!({"health": m["health"], "reasons": m["reasons"], "temp": m["temp"], "wear": if m["wear"].is_object() { m["wear"]["used"].clone() } else { Value::Null },
        "hours": m["hours"], "standby": m["standby"], "supported": m["supported"], "t": t})
}

/// The summary from LordOS's own SMART reader (disk-smart: smart.c), used when smartmontools is missing.
fn summary_from_own(s: &Value, t: &Value) -> Value {
    if !s.is_object() {
        return Value::Null;
    }
    let health = match s["health"].as_str() {
        Some("ok") => "ok",
        Some("warning") => "warning",
        Some("failing") => "failing",
        _ => "unknown",
    };
    json!({
        "health": health, "reasons": [],
        "temp": opt(num(&s["temp"]).filter(|v| *v >= 0.)),
        "wear": if s["kind"] == "nvme" { opt(num(&s["used"])) } else { Value::Null },
        "hours": opt(num(&s["hours"]).filter(|v| *v >= 0.)),
        "standby": false, "supported": true, "t": t,
    })
}

pub(super) fn request(input: &Value) -> Result<Value, &'static str> {
    Ok(match input["cmd"].as_str().unwrap_or("") {
        "constants" => json!({"attrKeys": attr_keys()}),
        "pretty" => json!(pretty_attr_name(&input["name"])),
        "parse" => parse_smartctl(&input["json"], &input["exit"]),
        "judge" => {
            let (health, reasons) = judge(&input["model"]);
            json!({"health": health, "reasons": reasons})
        }
        "summarize" => summarize(&input["model"], &input["t"]),
        "summaryFromOwn" => summary_from_own(&input["own"], &input["t"]),
        _ => return Err("unknown smart command"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standby_messages_are_found_by_word() {
        assert!(is_standby("Device is in STANDBY mode, exit(2)"));
        assert!(is_standby("Device is in IDLE_B mode"));
        assert!(is_standby("SLEEP mode"));
        assert!(!is_standby("NOSTANDBY mode"));
        assert!(!is_standby("STANDBY modes"));
        assert!(!is_standby("IDLE_a mode"));
    }
    #[test]
    fn a_nvme_log_gives_health_wear_and_reasons() {
        let j = json!({"smartctl": {"exit_status": 0}, "device": {"protocol": "NVMe"}, "smart_status": {"passed": true},
            "nvme_smart_health_information_log": {"critical_warning": 1, "percentage_used": 93, "available_spare": 5, "available_spare_threshold": 10,
                "data_units_read": 10, "temperature": 41, "media_errors": 2}});
        let m = parse_smartctl(&j, &json!(0));
        assert_eq!(m["kind"], "nvme");
        assert_eq!(m["health"], "warning");
        assert_eq!(m["reasons"], json!(["nvmeSpare", "mediaErrors", "worn"]));
        assert_eq!(m["wear"], json!({"used": 93., "source": "nvme"}));
        assert_eq!(m["read"].as_f64(), Some(5_120_000.));
    }
    #[test]
    fn an_ata_failure_and_a_missing_verdict() {
        let j = json!({"device": {"protocol": "ATA"}, "smart_status": {"passed": false},
            "ata_smart_attributes": {"table": [{"id": 5, "name": "Reallocated_Sector_Ct", "value": 100, "worst": 100, "thresh": 36,
                "raw": {"value": 12, "string": "12"}, "flags": {"prefailure": true}, "when_failed": ""}]}});
        let m = parse_smartctl(&j, &json!(0));
        assert_eq!((m["health"].as_str(), m["reallocated"].as_f64()), (Some("failing"), Some(12.)));
        assert_eq!(m["reasons"], json!(["smartFailed", "reallocated"]));
        let none = parse_smartctl(&json!({}), &json!(0));
        assert_eq!((none["health"].as_str(), none["supported"].as_bool()), (Some("unknown"), Some(false)));
        let exit8 = parse_smartctl(&json!({"smartctl": {"exit_status": 8}}), &json!(0));
        assert_eq!(exit8["health"], "failing");
    }
    #[test]
    fn raw_counts_and_names_read_like_the_old_regexes() {
        assert_eq!(raw_count(&json!({"raw": {"string": "12 (Min/Max 0/40)"}})), Some(12.));
        assert_eq!(raw_count(&json!({"raw": {"string": "12abc", "value": 7}})), Some(7.));
        assert_eq!(raw_count(&json!({"raw": {"value": -1}})), None);
        assert_eq!(pretty_attr_name(&json!("Unknown_SSD_Attribute ")), "Unknown SSD Attribute");
        assert!(attr_keys().contains(&"a194") && attr_keys().iter().filter(|k| **k == "a5").count() == 1);
    }
}
