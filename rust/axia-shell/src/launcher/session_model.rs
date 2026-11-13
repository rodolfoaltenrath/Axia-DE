//! Session restore's data rules (what sdk/sessionmodel.js was): what may enter the saved list,
//! what a new session does with what the last one left, the launch plan and a saved frame on
//! today's screens. Reached through the launcher bridge as `{"op":"session","cmd":…}`.
//!
//! settings.json can be edited by any program the user runs, so everything read back from it is
//! checked again here. The bridge carries plain data only: the adapter resolves callbacks (which
//! ids launch once, each display's work area) and sends their results.
use super::dock_model::parse_id;
use serde_json::{json, Map, Value};

const RESTORE_MODES: [&str; 3] = ["off", "ask", "always"];
const RESTORE_DEFAULT: &str = "always";
const MAX_WINDOWS: usize = 12; // a session never launches more than this
const MAX_DESKTOPS: f64 = 9.0; // winmodel/desktops.rs MAX_DESKTOPS says the same
// Never reopened: tools about another window, one-shot helpers and apps of the live system.
const NEVER: [&str; 5] = ["forcequit", "appabout", "installer", "sysprobe", "screenshots"];

/// JS truthiness of a JSON value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// `Math.round`: halves go up.
fn round(n: f64) -> f64 {
    (n + 0.5).floor()
}

fn text(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 80 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c)))
}
fn path(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        let units = s.encode_utf16().count();
        units > 1 && units <= 1024 && (s.starts_with('/') || s.starts_with('~')) && !s.contains('\0')
    })
}

/// Per LordOS app, the args it reads as "what this window shows" and how each must look. An app
/// or a field that isn't here reopens without them: a Store install request, a Help tour or a
/// Mail draft is an action that already happened, not a state.
fn rules(app: &str) -> &'static [(&'static str, fn(&Value) -> bool)] {
    match app {
        "settings" => &[("section", text), ("page", text)],
        "monitor" => &[("tab", text), ("disk", text)],
        "disks" => &[("disk", text)],
        "help" => &[("article", text)],
        "notes" => &[("slug", text)],
        "files" => &[("path", path), ("place", text), ("view", text)],
        "editor" | "pdf" | "photos" | "player" | "fonts" => &[("path", path)],
        _ => &[],
    }
}

fn clean_args(app: &str, args: &Value) -> Value {
    let Some(obj) = args.as_object() else { return Value::Null };
    // [[field, value]…] in the order of the rules: JSON objects come back with sorted keys, and
    // the adapter rebuilds the object in this order
    let out: Vec<Value> = rules(app)
        .iter()
        .filter_map(|(k, ok)| obj.get(*k).filter(|v| ok(v)).map(|v| json!([k, v])))
        .collect();
    if out.is_empty() {
        Value::Null
    } else {
        Value::Array(out)
    }
}

/// `{x,y,w,h}` rounded, or None.
fn clean_frame(f: &Value) -> Option<[f64; 4]> {
    let int = |k: &str| f.get(k).and_then(Value::as_f64).filter(|n| n.is_finite()).map(round);
    let (x, y, w, h) = (int("x")?, int("y")?, int("w")?, int("h")?);
    (80.0..=16384.0).contains(&w).then_some(()).filter(|_| (60.0..=16384.0).contains(&h))?;
    Some([x, y, w, h])
}

fn clean_entry(e: &Value, zones: &[Value]) -> Value {
    let Some(id) = e.get("id").filter(|v| v.is_string()) else { return Value::Null };
    let (Some((kind, key)), Some([x, y, w, h])) = (parse_id(id), clean_frame(&e["f"])) else { return Value::Null };
    if kind == "shell" || NEVER.contains(&key) {
        return Value::Null;
    }
    let mut out = Map::new();
    out.insert("id".into(), id.clone());
    out.insert("f".into(), json!({"x":x,"y":y,"w":w,"h":h}));
    if truthy(&e["max"]) {
        out.insert("max".into(), json!(1));
    }
    if truthy(&e["min"]) {
        out.insert("min".into(), json!(1));
    }
    if zones.contains(&e["snap"]) && !e["snap"].is_null() {
        out.insert("snap".into(), e["snap"].clone());
    }
    if let Some(d) = e["desk"].as_f64().filter(|d| d.fract() == 0.0 && *d > 0.0 && *d < MAX_DESKTOPS) {
        out.insert("desk".into(), json!(d as i64));
    }
    if kind == "app" {
        let args = clean_args(key, &e["args"]);
        if !args.is_null() {
            out.insert("args".into(), args);
        }
    }
    Value::Object(out)
}

fn fit_frame(input: &Value) -> Option<Value> {
    let n = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64);
    let f = &input["f"];
    let (fx, fy, fw, fh) = (n(f, "x")?, n(f, "y")?, n(f, "w")?, n(f, "h")?);
    let displays = input["displays"].as_array()?;
    let areas = input["areas"].as_array()?;
    let on = |d: &Value| -> Option<bool> {
        let (x, y, w, h) = (n(d, "x")?, n(d, "y")?, n(d, "w")?, n(d, "h")?);
        Some(fx < x + w - 48.0 && fx + fw > x + 48.0 && fy < y + h - 24.0 && fy + 24.0 > y)
    };
    let hit = displays.iter().position(|d| on(d) == Some(true));
    let main = input["main"].as_u64().unwrap_or(0) as usize;
    // area(d || displays[main] || displays[0]): the adapter measured every display
    let a = match hit {
        Some(i) => areas.get(i)?,
        None if main < displays.len() => areas.get(main)?,
        None if !displays.is_empty() => areas.first()?,
        None => &input["fallbackArea"],
    };
    let (ax, ay, aw, ah) = (n(a, "x")?, n(a, "y")?, n(a, "w")?, n(a, "h")?);
    let (w, h) = (fw.min(aw), fh.min(ah));
    if hit.is_none() {
        return Some(json!({"x":round(ax + (aw - w) / 2.0),"y":round(ay + (ah - h) / 2.0),"w":w,"h":h}));
    }
    // the same limits a window has when the screen changes (windows.js): partly off the edge stays so
    Some(json!({
        "x": fx.max(ax - w + 96.0).min(ax + aw - 96.0),
        "y": fy.max(ay).min(ay + ah - h.min(48.0)),
        "w": w, "h": h,
    }))
}

pub(super) fn request(input: &Value) -> Result<Value, &'static str> {
    match input["cmd"].as_str().unwrap_or("") {
        "constants" => Ok(json!({"modes":RESTORE_MODES,"default":RESTORE_DEFAULT,"max":MAX_WINDOWS,"never":NEVER})),
        "cleanArgs" => Ok(clean_args(input["app"].as_str().unwrap_or(""), &input["args"])),
        "cleanFrame" => Ok(clean_frame(&input["f"]).map_or(Value::Null, |[x, y, w, h]| json!({"x":x,"y":y,"w":w,"h":h}))),
        "cleanEntry" => Ok(clean_entry(&input["e"], input["zones"].as_array().map_or(&[], Vec::as_slice))),
        "cleanList" => {
            let zones = input["zones"].as_array().map_or(&[][..], Vec::as_slice);
            let list: Vec<Value> = input["list"]
                .as_array()
                .map(|l| l.iter().map(|e| clean_entry(e, zones)).filter(|e| !e.is_null()).collect())
                .unwrap_or_default();
            let skip = list.len().saturating_sub(MAX_WINDOWS);
            Ok(Value::Array(list.into_iter().skip(skip).collect()))
        }
        // What a new session does with what the last one left. An unclean end (power cut, crash)
        // always asks first, unless restore is off: the same apps may be why it fell.
        "restoreDecision" => {
            let mode = input["mode"].as_str().unwrap_or("");
            let decision = if !truthy(&input["count"]) || mode == "off" || !RESTORE_MODES.contains(&mode) {
                "none"
            } else if !truthy(&input["clean"]) || mode == "ask" {
                "ask"
            } else {
                "run"
            };
            Ok(json!(decision))
        }
        // The launches of a saved list, in order (bottom window first). `open`: dock id → windows
        // already open: that many entries are skipped, nothing doubles. `once`: ids that bring back
        // all their windows from a single launch. Returns {index, launch} per step.
        "planRestore" => {
            let ids: Vec<&str> = input["ids"].as_array().ok_or("ids")?.iter().map(|v| v.as_str().unwrap_or("")).collect();
            let mut left: Vec<(&str, i64)> = input["open"]
                .as_array()
                .map(|o| o.iter().filter_map(|p| Some((p.get(0)?.as_str()?, p.get(1)?.as_i64()?))).collect())
                .unwrap_or_default();
            let once: Vec<&str> = input["once"].as_array().map_or(vec![], |o| o.iter().filter_map(Value::as_str).collect());
            let mut started: Vec<&str> = Vec::new();
            let mut steps = Vec::new();
            for (index, id) in ids.iter().enumerate() {
                if let Some(slot) = left.iter_mut().find(|(k, _)| k == id) {
                    if slot.1 > 0 {
                        slot.1 -= 1;
                        continue;
                    }
                }
                let again = started.contains(id);
                started.push(id);
                steps.push(json!({"index":index,"launch":!(again && once.contains(id))}));
            }
            Ok(Value::Array(steps))
        }
        // A saved frame on today's screens: kept on the display it was on, or centered on the main
        // one when that display is gone.
        "fitFrame" => fit_frame(input).ok_or("fitFrame"),
        _ => Err("unknown operation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(v: Value) -> Value {
        request(&v).unwrap()
    }
    const F: &str = r#"{"x":10,"y":20,"w":400,"h":300}"#;

    #[test]
    fn args_keep_only_listed_fields() {
        assert_eq!(clean_args("files", &json!({"path":"/home/u/Docs","nonce":9})), json!([["path","/home/u/Docs"]]));
        assert_eq!(clean_args("store", &json!({"install":"/tmp/evil.deb"})), Value::Null);
        assert_eq!(clean_args("editor", &json!({"path":"a/b"})), Value::Null);
        assert_eq!(clean_args("settings", &json!({"section":"a b"})), Value::Null);
        assert_eq!(clean_args("files", &json!({"path":"~/Docs"})), json!([["path","~/Docs"]]));
        assert_eq!(clean_args("settings", &json!({"page":"vpn","section":"network"})), json!([["section","network"],["page","vpn"]]));
        assert_eq!(clean_args("files", &json!(["x"])), Value::Null);
        assert_eq!(clean_args("files", &json!({"path":"/a\u{0}b"})), Value::Null);
    }

    #[test]
    fn frames_round_like_js() {
        let f = |s: &str| clean_frame(&serde_json::from_str(s).unwrap());
        assert_eq!(f(r#"{"x":1.4,"y":2.6,"w":300,"h":200}"#), Some([1.0, 3.0, 300.0, 200.0]));
        assert_eq!(f(r#"{"x":-2.5,"y":0,"w":300,"h":200}"#), Some([-2.0, 0.0, 300.0, 200.0]));
        assert_eq!(f(r#"{"x":"0","y":0,"w":300,"h":200}"#), None);
        assert_eq!(f(r#"{"x":0,"y":0,"w":1,"h":1}"#), None);
        assert_eq!(f(r#"{"x":0,"y":0,"w":100000,"h":200}"#), None);
        assert_eq!(f(r#"{"x":0,"y":0,"w":80,"h":60}"#), Some([0.0, 0.0, 80.0, 60.0]));
    }

    #[test]
    fn entries_and_the_cap() {
        let f: Value = serde_json::from_str(F).unwrap();
        let z = [json!("left")];
        let fo = json!({"x":10.0,"y":20.0,"w":400.0,"h":300.0}); // rounded frames are numbers with a fraction part
        assert!(!clean_entry(&json!({"id":"app:files","f":f}), &z).is_null());
        for bad in ["linux:foo", "shell:launcher", "desktop:../../bin/sh", "app:forcequit", "app:screenshots"] {
            assert!(clean_entry(&json!({"id":bad,"f":f}), &z).is_null(), "{bad}");
        }
        let e = clean_entry(&json!({"id":"desktop:foo","f":f,"exec":"rm","max":1,"min":0,"snap":"left","desk":3,"args":{"path":"/x"}}), &z);
        assert_eq!(e, json!({"id":"desktop:foo","f":fo,"max":1,"snap":"left","desk":3}));
        assert_eq!(clean_entry(&json!({"id":"app:files","f":f,"snap":"nowhere","desk":9}), &z), json!({"id":"app:files","f":fo}));
        let many: Vec<Value> = (0..13).map(|i| json!({"id": if i == 12 { "app:notes" } else { "app:files" }, "f": f})).collect();
        let out = r(json!({"cmd":"cleanList","list":many}));
        assert_eq!(out.as_array().unwrap().len(), MAX_WINDOWS);
        assert_eq!(out[MAX_WINDOWS - 1]["id"], "app:notes");
        assert_eq!(r(json!({"cmd":"cleanList","list":{"a":1}})), json!([]));
    }

    #[test]
    fn decisions_and_plans() {
        let d = |m: &str, c: bool, n: i64| r(json!({"cmd":"restoreDecision","mode":m,"clean":c,"count":n}));
        assert_eq!(d("always", true, 2), "run");
        assert_eq!(d("ask", true, 2), "ask");
        assert_eq!(d("off", true, 2), "none");
        assert_eq!(d("always", false, 2), "ask");
        assert_eq!(d("off", false, 2), "none");
        assert_eq!(d("always", true, 0), "none");
        assert_eq!(d("sometimes", true, 2), "none");
        let p = |ids: Value, open: Value, once: Value| r(json!({"cmd":"planRestore","ids":ids,"open":open,"once":once}));
        assert_eq!(p(json!(["a", "b"]), json!([]), json!([])), json!([{"index":0,"launch":true},{"index":1,"launch":true}]));
        assert_eq!(p(json!(["a", "a", "b"]), json!([["a", 1]]), json!([])), json!([{"index":1,"launch":true},{"index":2,"launch":true}]));
        assert_eq!(p(json!(["a", "a"]), json!([]), json!(["a"])), json!([{"index":0,"launch":true},{"index":1,"launch":false}]));
    }

    #[test]
    fn frames_on_changed_screens() {
        let d = json!([{"x":0,"y":0,"w":1280,"h":800}]);
        let area = json!([{"x":0,"y":24,"w":1280,"h":720}]);
        let fit = |f: Value| r(json!({"cmd":"fitFrame","f":f,"displays":d,"areas":area,"main":0}));
        let same = |a: Value, b: Value| a.as_object().unwrap().iter().all(|(k, v)| v.as_f64() == b[k].as_f64());
        assert!(same(fit(json!({"x":100,"y":100,"w":400,"h":300})), json!({"x":100,"y":100,"w":400,"h":300})));
        assert!(same(fit(json!({"x":0,"y":0,"w":4000,"h":3000})), json!({"x":0,"y":24,"w":1280,"h":720})));
        assert!(same(fit(json!({"x":1220,"y":100,"w":400,"h":300})), json!({"x":1184,"y":100,"w":400,"h":300})));
        assert!(same(fit(json!({"x":3000,"y":100,"w":400,"h":300})), json!({"x":440,"y":234,"w":400,"h":300})));
        assert!(same(fit(json!({"x":10,"y":0,"w":400,"h":300})), json!({"x":10,"y":24,"w":400,"h":300})));
    }
}
