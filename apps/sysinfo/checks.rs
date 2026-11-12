//! Configured/effective/measured checks. Captions remain localization keys;
//! an available mechanism never becomes an effective-test claim here.
use serde_json::{json, Value};
fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
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
fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn row(id: &str, paper: &str, level: &str, state: &str, note: Option<&str>, vars: Value) -> Value {
    json!({"id":id,"paper":paper,"level":level,"state":state,"note":note,"vars":vars})
}
const KERNEL: &[&str] = &[
    "fsHome",
    "fsSystem",
    "cgroupEscape",
    "netInet",
    "netUnix",
    "ptrace",
    "userns",
    "mount",
    "exec",
];
const HELPER: &[&str] = &["helperSocket", "helperExec", "helperFork", "helperData"];
const BROKER: &[&str] = &[
    "brokerFsEscape",
    "brokerClipboard",
    "brokerFido",
    "brokerMail",
];

pub fn evaluate(c: &Value) -> Value {
    let r = &c["report"];
    let f = &c["facts"];
    let sw = &r["software"];
    let s = &r["session"];
    let mut out = Vec::new();
    let uid = f["uid"].as_i64().unwrap_or(-1);
    out.push(if uid < 0 {
        row(
            "serverUser",
            "§2.2",
            "configured",
            "unknown",
            None,
            json!({}),
        )
    } else {
        let ok = uid > 0 && f["euid"].as_i64().unwrap_or(0) > 0;
        row(
            "serverUser",
            "§2.2",
            "configured",
            if ok { "pass" } else { "fail" },
            Some(if ok { "serverUserOk" } else { "serverUserRoot" }),
            json!({"uid":uid}),
        )
    });
    let ll = &sw["landlockAbi"];
    out.push(if ll.is_null() {
        row("landlock", "§6.4", "configured", "unknown", None, json!({}))
    } else if ll.as_i64().unwrap_or(0) >= 6 {
        row(
            "landlock",
            "§6.4",
            "configured",
            "pass",
            Some("landlockOk"),
            json!({"abi":ll}),
        )
    } else if ll.as_i64().unwrap_or(0) > 0 {
        row(
            "landlock",
            "§6.4",
            "configured",
            "warn",
            Some("landlockOld"),
            json!({"abi":ll,"min":6}),
        )
    } else {
        row(
            "landlock",
            "§6.4",
            "configured",
            "fail",
            Some("landlockNone"),
            json!({}),
        )
    });
    let service = &sw["services"]["axia-netperm"];
    let (state, note) = if service.is_null() {
        ("unknown", None)
    } else if !truth(service) {
        ("fail", Some("netpermOff"))
    } else if sw["kernelConfig"]["CONFIG_CGROUP_BPF"] == "n" || !truth(&sw["bpfLsm"]) {
        (
            "warn",
            Some(if !truth(&sw["bpfLsm"]) {
                "netpermNoLsm"
            } else {
                "netpermNoCgroupBpf"
            }),
        )
    } else {
        ("pass", Some("netpermOk"))
    };
    out.push(row("netperm", "§6.6", "configured", state, note, json!({})));
    let code = &c["code"];
    let refused = array(&code["refused"]);
    out.push(if !refused.is_empty() {
        row(
            "code",
            "§6.9",
            "effective",
            "fail",
            Some("codeRefused"),
            json!({"count":refused.len()}),
        )
    } else if code["state"] == "verified" {
        row(
            "code",
            "§6.9",
            "effective",
            "pass",
            Some("codeVerified"),
            json!({"count":code["files"].as_u64().unwrap_or(0)}),
        )
    } else if code["state"] == "development" {
        row(
            "code",
            "§6.9",
            "effective",
            "warn",
            Some("codeDevelopment"),
            json!({}),
        )
    } else {
        row("code", "§6.9", "effective", "unknown", None, json!({}))
    });
    let cg = text(&f["cgroup"]);
    let own = cg
        .strip_suffix("/server")
        .and_then(|s| s.rsplit_once("/lordos-session-"))
        .is_some_and(|(_, tail)| !tail.is_empty() && tail.bytes().all(|c| c.is_ascii_digit()));
    out.push(if f["leafRoot"].is_null() || f["parentRoot"].is_null() {
        row("cgroups", "§11.4", "configured", "unknown", None, json!({}))
    } else if !own {
        row(
            "cgroups",
            "§11.4",
            "configured",
            "info",
            Some("cgroupsNotSession"),
            json!({}),
        )
    } else {
        let ok = truth(&f["leafRoot"]) && truth(&f["parentRoot"]);
        row(
            "cgroups",
            "§11.4",
            "configured",
            if ok { "pass" } else { "fail" },
            Some(if ok { "cgroupsOk" } else { "cgroupsUser" }),
            json!({"scopes":f["appScopes"].as_u64().unwrap_or(0)}),
        )
    });
    if s["composition"] == "gpu" {
        out.push(row(
            "gpu",
            "§7.3",
            "configured",
            "pass",
            Some("gpuOn"),
            json!({"renderer":text(&s["renderer"])}),
        ));
    } else {
        let comp = text(&s["composition"]);
        let state = if matches!(comp, "headless" | "no-display" | "capture") {
            "info"
        } else {
            "warn"
        };
        let reason = if state == "info" {
            comp
        } else if !text(&s["gpuInit"]).is_empty() {
            text(&s["gpuInit"])
        } else if !comp.is_empty() {
            comp
        } else {
            "unknown"
        };
        out.push(row(
            "gpu",
            "§7.3",
            "configured",
            state,
            Some(&format!("gpuNot.{reason}")),
            json!({}),
        ));
    }
    if truth(&c["stats"]["rssKB"]) {
        out.push(row(
            "memory",
            "§7.1",
            "measured",
            "info",
            Some("memoryNow"),
            json!({"mb":(c["stats"]["rssKB"].as_f64().unwrap_or(0.)/1024.).round()}),
        ));
    }
    json!(out)
}
pub fn probe(result: &Value) -> Value {
    let p = &result["probe"];
    let proc = &result["proc"];
    let tries = array(&p["tries"]);
    // Object.fromEntries in the previous model uses the last duplicate id.
    let by = |id: &str| tries.iter().rev().find(|t| t["id"] == id);
    let leaks = |ids: &[&str]| {
        ids.iter()
            .filter(|id| by(id).is_none_or(|t| t["denied"] != true))
            .copied()
            .collect::<Vec<_>>()
            .join(", ")
    };
    let own = by("ownData").is_some_and(|t| t["denied"] == false);
    let kernel = leaks(KERNEL);
    let caps = text(&proc["capEff"]);
    let proc_ok = proc["noNewPrivs"] == "1"
        && proc["seccomp"] == "2"
        && !caps.is_empty()
        && caps.bytes().all(|c| c == b'0');
    let mut out = vec![if !own {
        row(
            "appSandbox",
            "§6.4",
            "effective",
            "unknown",
            Some("probeBroken"),
            json!({}),
        )
    } else if !kernel.is_empty() {
        row(
            "appSandbox",
            "§6.4",
            "effective",
            "fail",
            Some("sandboxLeaks"),
            json!({"list":kernel}),
        )
    } else if !proc_ok {
        row(
            "appSandbox",
            "§6.4",
            "effective",
            "warn",
            Some("sandboxProc"),
            json!({"nnp":if truth(&proc["noNewPrivs"]){text(&proc["noNewPrivs"])}else{"?"},"seccomp":if truth(&proc["seccomp"]){text(&proc["seccomp"])}else{"?"}}),
        )
    } else {
        row(
            "appSandbox",
            "§6.4",
            "effective",
            "pass",
            Some("sandboxOk"),
            json!({"count":KERNEL.len()}),
        )
    }];
    if by("helperRan").is_some() {
        out.push(row(
            "helperSandbox",
            "§6.7",
            "effective",
            "unknown",
            Some("helperNotRun"),
            json!({}),
        ));
    } else {
        let h = leaks(HELPER);
        let ok = h.is_empty();
        out.push(row(
            "helperSandbox",
            "§6.7",
            "effective",
            if ok { "pass" } else { "fail" },
            Some(if ok { "helperOk" } else { "sandboxLeaks" }),
            if ok {
                json!({"count":HELPER.len()})
            } else {
                json!({"list":h})
            },
        ));
    }
    let b = leaks(BROKER);
    let ok = b.is_empty();
    out.push(row(
        "broker",
        "§6.5",
        "effective",
        if ok { "pass" } else { "fail" },
        Some(if ok { "brokerOk" } else { "sandboxLeaks" }),
        if ok {
            json!({"count":BROKER.len()})
        } else {
            json!({"list":b})
        },
    ));
    let cg = text(&proc["cgroup"]);
    let scoped = cg.ends_with("/app-lordos-lpk-sysprobe.scope");
    let mem = text(&proc["memoryMax"]);
    let mb = if !mem.is_empty() && mem.bytes().all(|c| c.is_ascii_digit()) {
        (mem.parse::<f64>().unwrap_or(0.) / 1048576.).round()
    } else {
        0.
    };
    out.push(if cg.is_empty() {
        row("appScope", "§6.4", "configured", "unknown", None, json!({}))
    } else {
        let ok = scoped && mb != 0.;
        row(
            "appScope",
            "§6.4",
            "configured",
            if ok { "pass" } else { "warn" },
            Some(if scoped {
                if mb != 0. {
                    "scopeOk"
                } else {
                    "scopeNoCap"
                }
            } else {
                "scopeMissing"
            }),
            json!({"mb":mb}),
        )
    });
    json!(out)
}
pub fn bench(b: &Value) -> Value {
    if b["tool"] != "axia-testbench" || !b["checks"].is_array() {
        return Value::Null;
    }
    let checks = array(&b["checks"]);
    let fails: Vec<_> = checks.iter().filter(|c| c["state"] == "fail").collect();
    let passes = checks.iter().filter(|c| c["state"] == "pass").count();
    let mut out = vec![row(
        "wayland",
        "§3.4",
        "effective",
        if checks.is_empty() {
            "unknown"
        } else if fails.is_empty() {
            "pass"
        } else {
            "fail"
        },
        Some(if fails.is_empty() {
            "benchPasses"
        } else {
            "benchFails"
        }),
        json!({"pass":passes,"fail":fails.len(),"list":fails.iter().map(|c|text(&c["id"])).collect::<Vec<_>>().join(", ")}),
    )];
    let gl = &b["gl"];
    out.push(if truth(&gl["renderer"]){row("renderer3d","§7.3","effective",if text(&gl["kind"]).contains("SOFTWARE"){"warn"}else{"pass"},Some("benchRenderer"),json!({"renderer":gl["renderer"],"ms":(gl["drawMs"].as_f64().unwrap_or(0.)*100.).round()/100.}))}
        else if truth(&gl["error"]){row("renderer3d","§7.3","effective","unknown",Some("benchNo3d"),json!({"error":gl["error"]}))}
        else {row("renderer3d","§7.3","effective","unknown",Some("benchNo3dRun"),json!({}))});
    if truth(&b["presentation"]) {
        let shown = b["presented"].as_f64().unwrap_or(0.);
        let refresh = if truth(&b["refreshHz"]) {
            &b["refreshHz"]
        } else {
            &b["outputRefreshHz"]
        };
        out.push(row("presented","§7.2","measured",if shown!=0.{"info"}else{"warn"},Some(if shown!=0.{"benchPresented"}else{"benchNotPresented"}),json!({"shown":shown,"discarded":b["discarded"].as_f64().unwrap_or(0.),"hz":(refresh.as_f64().unwrap_or(0.)*100.).round()/100.,"fps":(b["fps"].as_f64().unwrap_or(0.)*10.).round()/10.})));
    }
    let lat = &b["latency"];
    if truth(&lat["n"]) {
        out.push(row(
            "latency",
            "§7.2",
            "measured",
            "info",
            Some("benchLatency"),
            json!({"ms":lat["avgMs"].as_f64().map(|v|(v*10.).round()/10.),"n":lat["n"]}),
        ));
    }
    json!(out)
}
pub fn request(input: &Value) -> Value {
    match input["cmd"].as_str().unwrap_or("") {
        "catalog" => {
            json!({"limits":{"landlockAbi":6},"pending":["netpermEffective"],"probeChecks":["appSandbox","helperSandbox","broker","appScope"]})
        }
        "evaluate" => evaluate(&input["value"]),
        "evaluateProbe" => probe(&input["value"]),
        "evaluateBench" => bench(&input["value"]),
        _ => json!({"error":"app.invalidRequest"}),
    }
}
