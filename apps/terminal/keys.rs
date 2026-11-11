//! Physical shortcuts and key mappings. Resolution preserves factory order,
//! user overrides, profile precedence and the byte strings sent to the PTY.
#[path = "key_data.rs"]
mod data;
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
fn combo(e: &Value) -> Value {
    let key = e["key"].as_str().unwrap_or("");
    let code = e["code"].as_i64().unwrap_or(-1).to_string();
    let key = if key.encode_utf16().count() > 1 && key != "Meta" {
        key
    } else {
        data::code_keys()[&code].as_str().unwrap_or("")
    };
    if key.is_empty() {
        return Value::Null;
    }
    let mut parts = Vec::new();
    for (flag, label) in [
        ("meta", "Super"),
        ("ctrl", "Ctrl"),
        ("alt", "Alt"),
        ("shift", "Shift"),
    ] {
        if truth(&e[flag]) {
            parts.push(label);
        }
    }
    parts.push(key);
    json!(parts.join("+"))
}
fn bindings(custom: &Value) -> Vec<Value> {
    let mut entries = array(data::bindings()).to_vec();
    for entry in array(custom) {
        if array(entry).len() != 2 {
            continue;
        }
        if let Some(old) = entries.iter_mut().find(|old| old[0] == entry[0]) {
            *old = entry.clone();
        } else {
            entries.push(entry.clone());
        }
    }
    entries
}
fn action(e: &Value, custom: &Value) -> Value {
    let combo = combo(e);
    if combo.is_null() {
        return Value::Null;
    }
    let mut action = Value::Null;
    for i in 1..=9 {
        if combo == format!("Super+{i}") {
            action = json!(format!("tab{i}"));
        }
    }
    for entry in bindings(custom) {
        if array(&entry[1]).contains(&combo) {
            action = entry[0].clone();
        }
    }
    action
}
fn mapping(e: &Value, profile: &Value, global: &Value) -> Value {
    let c = combo(e);
    if c.is_null() {
        return Value::Null;
    }
    array(&profile["keyMap"])
        .iter()
        .chain(array(global))
        .find(|m| m["combo"] == c)
        .cloned()
        .unwrap_or(Value::Null)
}
fn js_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}
fn hex_prefix(s: &str) -> Option<i64> {
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
    i64::from_str_radix(&digits, 16)
        .ok()
        .map(|n| if negative { -n } else { n })
}
fn bytes(mapping: &Value) -> Value {
    let param = if truth(&mapping["param"]) {
        js_string(&mapping["param"])
    } else {
        String::new()
    };
    match mapping["action"].as_str().unwrap_or("") {
        "hex" => {
            json!({"kind":"hex","bytes":param.split(|c:char|c.is_whitespace()||c==',').filter(|s|!s.is_empty()).filter_map(hex_prefix).filter(|v|(0..256).contains(v)).collect::<Vec<_>>()})
        }
        "escape" => {
            json!({"kind":"text","units":format!("\u{1b}{param}").encode_utf16().collect::<Vec<_>>()})
        }
        "text" => {
            let chars: Vec<_> = param.chars().collect();
            let mut out = Vec::<u16>::new();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] != '\\'
                    || i + 1 >= chars.len()
                    || matches!(chars[i + 1], '\n' | '\r' | '\u{2028}' | '\u{2029}')
                {
                    out.extend(chars[i].encode_utf16(&mut [0; 2]).iter().copied());
                    i += 1;
                    continue;
                }
                i += 1;
                let c = chars[i];
                if c == 'x'
                    && i + 2 < chars.len()
                    && chars[i + 1..=i + 2].iter().all(char::is_ascii_hexdigit)
                {
                    let hex: String = chars[i + 1..=i + 2].iter().collect();
                    out.push(u16::from_str_radix(&hex, 16).unwrap());
                    i += 3;
                    continue;
                }
                if c == 'u' && i + 2 < chars.len() && chars[i + 1] == '{' {
                    let digits: String = chars[i + 2..]
                        .iter()
                        .take_while(|c| c.is_ascii_hexdigit())
                        .collect();
                    let end = i + 2 + digits.len();
                    if !digits.is_empty() && chars.get(end) == Some(&'}') {
                        let Ok(cp) = u32::from_str_radix(&digits, 16) else {
                            return json!({"error":"invalidCodePoint"});
                        };
                        if cp > 0x10ffff {
                            return json!({"error":"invalidCodePoint"});
                        }
                        if cp <= 0xffff {
                            out.push(cp as u16);
                        } else {
                            let n = cp - 0x10000;
                            out.extend([0xd800 + (n >> 10) as u16, 0xdc00 + (n & 0x3ff) as u16]);
                        }
                        i = end + 1;
                        continue;
                    }
                }
                let c = match c {
                    'n' | 'r' => '\r',
                    't' => '\t',
                    'e' => '\u{1b}',
                    'a' => '\u{7}',
                    'b' => '\u{8}',
                    c => c,
                };
                out.extend(c.encode_utf16(&mut [0; 2]).iter().copied());
                i += 1;
            }
            json!({"kind":"text","units":out})
        }
        _ => Value::Null,
    }
}
fn combo_label(input: &Value) -> Value {
    let mut out = String::new();
    for key in input.as_str().unwrap_or("").split('+') {
        if let Some(symbol) = data::symbols()[key].as_str() {
            out.push_str(symbol);
        } else if key.encode_utf16().count() == 1 {
            out.push_str(&key.to_uppercase());
        } else {
            out.push_str(key);
        }
    }
    json!(out)
}
pub fn request(input: &Value) -> Option<Value> {
    Some(match input["cmd"].as_str().unwrap_or("") {
        "keyCatalog" => json!({"bindings":data::bindings()}),
        "combo" => combo(&input["event"]),
        "bindings" => json!(bindings(&input["shortcuts"])),
        "action" => action(&input["event"], &input["shortcuts"]),
        "keyMapping" => mapping(&input["event"], &input["profile"], &input["keyMap"]),
        "mappingBytes" => bytes(&input["mapping"]),
        "comboLabel" => combo_label(&input["value"]),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_match_original_javascript_including_collision_order_and_bytes() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../rust/axia-shell/tests/fixtures/terminal_keys.json"
        ))
        .unwrap();
        for (i, case) in array(&cases).iter().enumerate() {
            assert_eq!(
                request(&case["input"]).unwrap(),
                case["expected"],
                "key reference {i}: {}",
                case["input"]
            );
        }
    }
    #[test]
    fn invalid_unicode_reports_a_range_error_without_panicking() {
        assert_eq!(
            bytes(&json!({"action":"text","param":"\\u{110000}"})),
            json!({"error":"invalidCodePoint"})
        );
        assert_eq!(
            bytes(&json!({"action":"text","param":"\\u{ffffffffffffffffffff}"})),
            json!({"error":"invalidCodePoint"})
        );
    }
}
