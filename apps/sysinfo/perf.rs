//! Benchmark interpretation. Keep per-category comparisons, signed thresholds,
//! sandbox limits and measured firmware events separate from one another.
#[path = "perf_data.rs"]
mod data;
use serde_json::{json, Value};
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
fn ratio(res: &Value, reference: &Value, id: &str) -> Value {
    let a = &res["results"][id];
    let b = &reference["results"][id];
    let (Some(av), Some(bv)) = (
        a["value"].as_f64().filter(|v| *v > 0.),
        b["value"].as_f64().filter(|v| *v > 0.),
    ) else {
        return Value::Null;
    };
    if !b["lordos"].is_null() && b["lordos"] != res["lordos"] {
        return Value::Null;
    }
    json!(if truth(&a["lowerBetter"]) {
        bv / av
    } else {
        av / bv
    })
}
fn categories(res: &Value, reference: &Value) -> Value {
    let mut out = json!({});
    for cat in arr(&data::catalog()["categories"]) {
        let values: Vec<_> = arr(&cat["ids"])
            .iter()
            .filter_map(|id| ratio(res, reference, text(id)).as_f64())
            .collect();
        out[text(&cat["id"])] = if values.is_empty() {
            Value::Null
        } else {
            json!((values.iter().map(|v| v.ln()).sum::<f64>() / values.len() as f64).exp())
        };
    }
    out
}
fn pi_firmware(res: &Value) -> Value {
    let pf = &res["piFirmware"];
    if !pf.is_object() && !pf.is_array() {
        return Value::Null;
    }
    json!({"flags":pf["flags"]==true,"voltageAlarm":pf["voltageAlarm"]==true,"tempLimitC":pf["tempLimitC"].as_f64(),
        "events":arr(&data::catalog()["events"]).iter().filter(|id|pf[text(id)]==true).collect::<Vec<_>>()})
}
fn warn(state: &mut &'static str, notes: &mut Vec<Value>, id: &str, vars: Value) {
    notes.push(json!({"id":id,"vars":vars}));
    if *state == "pass" {
        *state = "warn";
    }
}
fn stability(res: &Value) -> Value {
    if !truth(res) {
        return json!({"state":"unknown","notes":[]});
    }
    let mut state = "pass";
    let mut notes = Vec::new();
    if res["status"] == "failed" {
        state = "fail";
        notes.push(json!({"id":"failed","vars":{"why":text(&res["stopReason"])}}));
    } else if res["status"] == "stopped" {
        warn(
            &mut state,
            &mut notes,
            match text(&res["stopReason"]) {
                "thermal" => "stoppedThermal",
                "throttled" => "stoppedThrottled",
                _ => "stopped",
            },
            json!({}),
        );
    }
    let failed = res["skipped"]
        .as_object()
        .map(|o| o.values().filter(|v| **v == "failed").count())
        .unwrap_or(0);
    if failed > 0 {
        warn(
            &mut state,
            &mut notes,
            "measureFailed",
            json!({"count":failed}),
        );
    }
    if let (Some(b), Some(a)) = (
        res["before"]["throttleEvents"].as_f64(),
        res["after"]["throttleEvents"].as_f64(),
    ) {
        if a > b {
            warn(&mut state, &mut notes, "throttleSeen", json!({"count":a-b}));
        }
    }
    let pf = pi_firmware(res);
    if !pf.is_null() {
        let events = arr(&pf["events"]);
        let has = |id: &str| events.contains(&json!(id));
        if ["capped", "throttled", "softLimit"]
            .iter()
            .any(|id| has(id))
        {
            warn(&mut state, &mut notes, "piThrottled", json!({}));
        } else if has("tempLimit") {
            warn(
                &mut state,
                &mut notes,
                "piTempLimit",
                json!({"temp":pf["tempLimitC"]}),
            );
        }
        if has("underVoltage") {
            warn(&mut state, &mut notes, "underVoltage", json!({}));
        }
    } else {
        let bits = res["after"]["rpiThrottled"].as_i64().unwrap_or(0);
        if bits & 7 != 0 {
            warn(&mut state, &mut notes, "piThrottled", json!({}));
        }
        if bits & 1 != 0 {
            warn(&mut state, &mut notes, "underVoltage", json!({}));
        }
    }
    if truth(&res["results"]["sustained.start"]) && truth(&res["results"]["sustained.end"]) {
        let start = res["results"]["sustained.start"]["value"]
            .as_f64()
            .unwrap_or(f64::NAN);
        let end = res["results"]["sustained.end"]["value"]
            .as_f64()
            .unwrap_or(f64::NAN);
        let drop = 1. - end / start;
        if drop > 0.1 {
            warn(
                &mut state,
                &mut notes,
                "sustainedDrop",
                json!({"pct":(drop*100.).round()}),
            );
        }
    }
    if !res["sustained"]["throttleAtS"].is_null() {
        let mut vars = json!({"s":res["sustained"]["throttleAtS"]});
        if let Some(temp) = res["sustained"].get("throttleTempC") {
            vars["temp"] = temp.clone();
        }
        warn(&mut state, &mut notes, "sustainedThrottle", vars);
    }
    if truth(&res["quiet"]) && !truth(&res["quiet"]["quiet"]) {
        warn(
            &mut state,
            &mut notes,
            "notQuiet",
            json!({"pct":res["quiet"]["busyPct"].as_f64().map(f64::round)}),
        );
    }
    if res["sandboxTries"].is_array() {
        let tries = arr(&res["sandboxTries"]);
        let by = |id: &str| tries.iter().rev().find(|t| t["id"] == id);
        let leaks: Vec<_> = arr(&data::catalog()["sandbox"])
            .iter()
            .filter(|id| by(text(id)).is_none_or(|t| t["denied"] != true))
            .collect();
        let no_landlock = res["sandbox"].as_i64().is_some_and(|bits| bits & 2 == 0);
        let partial: Vec<_> = leaks
            .iter()
            .copied()
            .filter(|id| {
                no_landlock
                    && arr(&data::catalog()["landlock"]).contains(id)
                    && by(text(id)).is_some()
            })
            .collect();
        let real: Vec<_> = leaks
            .iter()
            .copied()
            .filter(|id| !partial.contains(id))
            .collect();
        if !real.is_empty() {
            state = "fail";
            notes.push(json!({"id":"sandboxLeaks","vars":{"ids":real}}));
        }
        if !partial.is_empty() {
            warn(
                &mut state,
                &mut notes,
                "sandboxPartial",
                json!({"ids":partial}),
            );
        }
        if leaks.is_empty() {
            notes.push(json!({"id":"sandboxHeld","vars":{"count":arr(&data::catalog()["sandbox"]).len()},"info":true}));
        }
    }
    json!({"state":state,"notes":notes})
}
fn compatibility(rows: &Value) -> Value {
    let mut count = json!({"works":0,"limits":0,"no":0,"absent":0,"unchecked":0});
    for row in arr(rows) {
        let state = text(&row["state"]);
        if let Some(n) = count[state].as_u64() {
            count[state] = json!(n + 1);
        }
    }
    let state = if count["no"].as_u64().unwrap() > 0 {
        "fail"
    } else if count["limits"].as_u64().unwrap() > 0 {
        "warn"
    } else if !arr(rows).is_empty() {
        "pass"
    } else {
        "unknown"
    };
    json!({"state":state,"count":count})
}
fn desktop(res: &Value) -> Value {
    let r = &res["results"];
    let Some(budget) = res["frames"]["budgetMs"].as_f64().filter(|n| *n != 0.) else {
        return json!({"state":"unknown","notes":[]});
    };
    if !truth(&r["comp.p99"]) && !truth(&r["desk.p99"]) {
        return json!({"state":"unknown","notes":[]});
    }
    let mut state = "pass";
    let mut notes = Vec::new();
    for phase in ["comp", "desk"] {
        let p99 = &r[format!("{phase}.p99")];
        let missed = &r[format!("{phase}.missed")];
        if !truth(p99) {
            continue;
        }
        let mut vars =
            json!({"budget":budget,"pct":if truth(missed){missed["value"].clone()}else{json!(0)}});
        if let Some(value) = p99.get("value") {
            vars["p99"] = value.clone();
        }
        let over = p99["value"].as_f64().is_some_and(|v| v > budget);
        let id = format!("{phase}{}", if over { "Over" } else { "Fits" });
        if over {
            warn(&mut state, &mut notes, &id, vars.clone());
        } else {
            notes.push(json!({"id":id,"vars":vars}));
        }
        let pct = missed["value"].as_f64().unwrap_or(0.);
        if pct > data::catalog()["missedFail"].as_f64().unwrap() {
            state = "fail";
            notes.push(json!({"id":format!("{phase}MissedMany"),"vars":vars}));
        } else if pct > data::catalog()["missedWarn"].as_f64().unwrap() {
            warn(&mut state, &mut notes, &format!("{phase}Missed"), vars);
        }
        let in50 = &r[format!("{phase}.in50")];
        let in99 = &r[format!("{phase}.in99")];
        if truth(in50) && truth(in99) {
            let mut vars = json!({});
            if let Some(v) = in50.get("value") {
                vars["in50"] = v.clone();
            }
            if let Some(v) = in99.get("value") {
                vars["in99"] = v.clone();
            }
            let slow = in99["value"]
                .as_f64()
                .is_some_and(|n| n > budget * data::catalog()["inputWarn"].as_f64().unwrap());
            let id = format!("{phase}Input{}", if slow { "Slow" } else { "Ok" });
            if slow {
                warn(&mut state, &mut notes, &id, vars);
            } else {
                notes.push(json!({"id":id,"vars":vars}));
            }
        }
    }
    json!({"state":state,"notes":notes})
}
fn condition_diffs(a: &Value, b: &Value) -> Value {
    if !truth(a) || !truth(b) {
        return json!([]);
    }
    let mut out = Vec::new();
    for (id, path) in [
        ("power", "/before/power"),
        ("governor", "/before/governor"),
        ("profile", "/before/platformProfile"),
        ("gpuKind", "/gpuKind"),
        ("storage", "/storage/kind"),
        ("threads", "/threads"),
        ("live", "/live"),
        ("lordos", "/lordos"),
        ("kernel", "/kernel"),
        ("composition", "/session/composition"),
    ] {
        let x = a.pointer(path);
        let y = b.pointer(path);
        if x == y || x.is_none_or(Value::is_null) && y.is_none_or(Value::is_null) {
            continue;
        }
        let mut row = json!({"id":id});
        if let Some(x) = x {
            row["now"] = x.clone();
        }
        if let Some(y) = y {
            row["before"] = y.clone();
        }
        out.push(row);
    }
    json!(out)
}
pub fn request(input: &Value) -> Option<Value> {
    let a = &input["args"];
    Some(match input["cmd"].as_str().unwrap_or("") {
        "perfCatalog" => data::catalog().clone(),
        "ratio" => ratio(&a[0], &a[1], text(&a[2])),
        "categoryRatios" => categories(&a[0], &a[1]),
        "comparableReferences" => json!(arr(&data::catalog()["references"])
            .iter()
            .filter(|r| truth(&a[0]) && r["lordbench"] == a[0]["lordbench"])
            .collect::<Vec<_>>()),
        "piFirmware" => pi_firmware(&a[0]),
        "stability" => stability(&a[0]),
        "compatibilityVerdict" => compatibility(&a[0]),
        "desktopVerdict" => desktop(&a[0]),
        "conditionDiffs" => condition_diffs(&a[0], &a[1]),
        "changes" => {
            let now = &a[0];
            let prev = &a[1];
            if !truth(now)
                || !truth(prev)
                || now["lordbench"] != prev["lordbench"]
                || now["mode"] != prev["mode"]
            {
                Value::Null
            } else {
                let mut out = json!({});
                if let Some(results) = now["results"].as_object() {
                    for id in results.keys() {
                        if let Some(r) = ratio(now, prev, id).as_f64() {
                            out[id] = json!(r - 1.);
                        }
                    }
                }
                out
            }
        }
        "sustainedSeries" => json!(arr(&a[0]["sustained"]["samples"])
            .iter()
            .filter(|s| arr(s).len() == 3 && s[1].as_f64().is_some_and(|v| v >= 0.))
            .collect::<Vec<_>>()),
        "refreshBudget" => {
            let hz = arr(&a[0]["session"]["displays"])
                .iter()
                .filter_map(|d| d["mode"]["refresh"].as_f64().filter(|v| *v > 0.))
                .reduce(f64::min);
            json!(hz.map(|hz| 1_000_000. / hz))
        }
        _ => return None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn same(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(a), Value::Number(b)) => {
                let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
                (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.)
            }
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
            }
            (Value::Object(a), Value::Object(b)) => {
                a.len() == b.len() && a.iter().all(|(k, a)| b.get(k).is_some_and(|b| same(a, b)))
            }
            _ => a == b,
        }
    }
    #[test]
    fn performance_matches_original_verdicts_and_measurements() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../rust/axia-shell/tests/fixtures/sysinfo_performance.json"
        ))
        .unwrap();
        for (i, c) in arr(&cases).iter().enumerate() {
            let got = request(&c["input"]).unwrap();
            assert!(
                same(&got, &c["expected"]),
                "performance reference {i}: {}\ngot {got}\nwant {}",
                c["input"],
                c["expected"]
            );
        }
    }
}
