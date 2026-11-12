//! Profiles and native terminal configuration. No terminal sessions or
//! secrets are retained by this model; persistence remains a shell effect.
#[path = "data.rs"]
mod data;
#[path = "keys.rs"]
mod keys;
use serde_json::{json, Value};

fn merge(base: &Value, patch: &Value) -> Value {
    let mut out = base.as_object().cloned().unwrap_or_default();
    if let Some(patch) = patch.as_object() {
        out.extend(patch.clone());
    }
    Value::Object(out)
}
fn truth(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.) != 0.,
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
pub fn settings(raw: &Value) -> Value {
    let mut out = merge(data::defaults(), raw);
    let profiles = raw["profiles"]
        .as_array()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| data::defaults()["profiles"].as_array().unwrap());
    out["profiles"] = json!(profiles
        .iter()
        .map(|p| merge(data::profile(), p))
        .collect::<Vec<_>>());
    out
}
fn profile(raw: &Value, id: &Value) -> Value {
    let settings = settings(raw);
    let profiles = settings["profiles"].as_array().unwrap();
    profiles
        .iter()
        .find(|p| p["id"] == *id)
        .or_else(|| {
            profiles
                .iter()
                .find(|p| p["id"] == settings["defaultProfile"])
        })
        .unwrap_or(&profiles[0])
        .clone()
}
fn scheme(raw: &Value, p: &Value) -> Value {
    if p["colors"] == "custom" && truth(&p["custom"]) {
        return p["custom"].clone();
    }
    let name = p["colors"].as_str().unwrap_or("");
    if truth(&raw["schemes"][name]) {
        return raw["schemes"][name].clone();
    }
    let presets = data::presets().as_array().unwrap();
    presets
        .iter()
        .find(|entry| entry[0] == name)
        .unwrap_or(&presets[0])[1]
        .clone()
}
// parseInt accepts a hexadecimal prefix and stops at the first non-hex digit.
fn hex(v: &Value) -> u32 {
    let s = v
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string());
    let s = s.replacen('#', "", 1);
    let s: String = s.chars().take(6).collect();
    let s = s.trim_start();
    let (negative, s) = if let Some(s) = s.strip_prefix('-') {
        (true, s)
    } else {
        (false, s.strip_prefix('+').unwrap_or(s))
    };
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    let digits: String = s.chars().take_while(char::is_ascii_hexdigit).collect();
    let n = u32::from_str_radix(&digits, 16).unwrap_or(0);
    if negative {
        n.wrapping_neg()
    } else {
        n
    }
}
fn config(raw: &Value, p: &Value, extra: &Value) -> Value {
    let s = scheme(raw, p);
    let mut out = json!({});
    for key in [
        "fontSize",
        "lineSpacing",
        "padding",
        "minimumContrast",
        "cursorStyle",
        "wordChars",
    ] {
        if let Some(v) = p.get(key) {
            out[key] = v.clone();
        }
    }
    out["palette"] = json!(s["ansi"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(hex)
        .collect::<Vec<_>>());
    for (target, source) in [
        ("foreground", "fg"),
        ("background", "bg"),
        ("cursor", "cursor"),
        ("selection", "selection"),
    ] {
        out[target] = json!(hex(&s[source]));
    }
    out["link"] = json!(if truth(&s["link"]) {
        hex(&s["link"])
    } else {
        0x2b7ff7
    });
    out["cursorGuideColor"] = json!(if truth(&s["guide"]) {
        hex(&s["guide"])
    } else {
        0xa6e8ff
    });
    out["backgroundOpacity"] = json!(1. - p["transparency"].as_f64().unwrap_or(0.));
    for key in [
        "boldBright",
        "useBoldFont",
        "useItalic",
        "cursorBlink",
        "cursorGuide",
        "blinkText",
        "altSendsEscape",
        "mouseReporting",
        "copyOnSelect",
        "alternateScroll",
        "ligatures",
    ] {
        out[key] = json!(i32::from(truth(&p[key])));
    }
    out["timestamps"] = json!(i32::from(truth(&extra["timestamps"])));
    out["bidi"] = json!(i32::from(p["bidi"] != false));
    for key in ["font", "nonAsciiFont"] {
        out[key] = if truth(&p[key]) {
            p[key].clone()
        } else {
            json!("")
        };
    }
    for key in ["cursorText", "selectionText", "bold"] {
        if truth(&s[key]) {
            out[key] = json!(hex(&s[key]));
        }
    }
    if truth(&extra["fontDelta"]) {
        out["fontSize"] = json!((p["fontSize"].as_f64().unwrap_or(13.)
            + extra["fontDelta"].as_f64().unwrap_or(0.))
        .clamp(6., 72.));
    }
    out
}
pub fn request(input: &Value) -> Value {
    if let Some(reply) = keys::request(input) {
        return reply;
    }
    let raw = &input["settings"];
    match input["cmd"].as_str().unwrap_or("") {
        "catalog" => json!({"presets":data::presets(),"profile":data::profile()}),
        "settings" => settings(raw),
        "profile" => profile(raw, &input["id"]),
        "scheme" => scheme(raw, &input["profile"]),
        "config" => config(raw, &input["profile"], &input["extra"]),
        _ => json!({"error":"app.invalidRequest"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_defaults_and_fallbacks_preserve_custom_fields() {
        let raw = json!({"profiles":[{"id":"a","fontSize":17},{"id":"b","badge":"x"}],"defaultProfile":"b","future":42});
        let t = settings(&raw);
        assert_eq!(t["future"], 42);
        assert_eq!(t["profiles"][0]["fontSize"], 17);
        assert_eq!(t["profiles"][0]["colors"], "lordos");
        assert_eq!(profile(&raw, &json!("missing"))["id"], "b");
        assert_eq!(profile(&raw, &json!("a"))["id"], "a");
        assert_eq!(
            settings(&json!({"profiles":[]}))["profiles"][0],
            *data::profile()
        );
    }
    #[test]
    fn scheme_order_custom_overrides_and_native_configuration() {
        assert_eq!(data::presets()[0][0], "lordos");
        assert_eq!(data::presets()[1][0], "iterm");
        let raw = json!({"schemes":{"lordos":{"name":"User override"}}});
        assert_eq!(scheme(&raw, data::profile())["name"], "User override");
        let mut p = data::profile().clone();
        p["colors"] = json!("missing");
        p["transparency"] = json!(0.25);
        let cfg = config(
            &Value::Null,
            &p,
            &json!({"fontDelta":100,"timestamps":true}),
        );
        assert_eq!(cfg["foreground"], 0xe5e5ea);
        assert_eq!(cfg["fontSize"], 72.);
        assert_eq!(cfg["backgroundOpacity"], 0.75);
        assert_eq!(cfg["palette"].as_array().unwrap().len(), 16);
        assert_eq!(cfg["timestamps"], 1);
    }
}
