//! Dock identifiers, pinned-list policy and quick-action validation.
//! JS supplies callback results and retains callback/object identity; all policy
//! runs here through the existing context-owned launcher transport.
use serde_json::{json, Value};

const FIRST: &str = "app:files";
const LOCKED: [&str; 3] = [FIRST, "shell:launcher", "app:settings"];

fn valid_key(kind: &str, key: &str) -> bool {
    let bytes = key.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    match kind {
        "app" | "lpk" => {
            (if kind == "app" { 1..=40 } else { 2..=40 }).contains(&bytes.len())
                && first.is_ascii_lowercase()
                && bytes
                    .iter()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        }
        "desktop" => {
            bytes.len() <= 62
                && (first.is_ascii_alphanumeric() || *first == b'_')
                && bytes
                    .iter()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c))
                && !key.contains("..")
                && !key.starts_with("lpk-")
        }
        "shell" => key == "launcher",
        _ => false,
    }
}

fn parse(id: &Value) -> Option<(&str, &str)> {
    let (kind, key) = id.as_str()?.split_once(':')?;
    valid_key(kind, key).then_some((kind, key))
}
/// The (kind, key) of a typed dock id, for the session rules.
#[allow(dead_code)] // the Dock's node example does not include the session model
pub(super) fn parse_id(id: &Value) -> Option<(&str, &str)> {
    parse(id)
}

fn dock_id(kind: &Value, key: &Value) -> Value {
    match (kind.as_str(), key.as_str()) {
        (Some(kind), Some(key)) if valid_key(kind, key) => json!(format!("{kind}:{key}")),
        _ => Value::Null,
    }
}

fn typed(id: &Value, kind: &Value) -> Value {
    if parse(id).is_some() {
        return id.clone();
    }
    match id.as_str() {
        Some(id) if !id.is_empty() && !id.contains(':') => dock_id(kind, &json!(id)),
        _ => Value::Null,
    }
}

fn files_first(list: &mut Vec<Value>) {
    if let Some(i) = list.iter().position(|id| id == FIRST) {
        if i > 0 {
            let id = list.remove(i);
            list.insert(0, id);
        }
    }
}

fn insert(list: &mut Vec<Value>, id: Value, index: &Value, shown: &[Value]) {
    let mut k = index
        .as_f64()
        .filter(|n| n.is_finite())
        .map(|n| (n + 0.5).floor().max(0.) as usize)
        .unwrap_or(shown.len());
    if shown.first().is_some_and(|id| id == FIRST) {
        k = k.max(1);
    }
    let at = shown
        .get(k)
        .and_then(|id| list.iter().position(|x| x == id))
        .unwrap_or(list.len());
    list.insert(at, id);
    files_first(list);
}

fn action_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 48
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c))
}

fn args_valid(args: &Value) -> bool {
    let Some(args) = args.as_object() else {
        return false;
    };
    args.iter().all(|(key, v)| {
        let b = key.as_bytes();
        !b.is_empty()
            && b.len() <= 32
            && key != "nonce"
            && (b[0].is_ascii_alphabetic() || b[0] == b'_')
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
            && (v.is_null() || v.is_string() || v.is_number() || v.is_boolean())
    })
}

/// Stateless operations also work in the isolated app host: no filesystem or
/// session capability is used. The existing launcher bridge bounds JSON input.
pub(super) fn request(input: &Value) -> Result<Value, &'static str> {
    let cmd = input["cmd"].as_str().unwrap_or("");
    let id = &input["id"];
    let array = |name: &str| input[name].as_array().cloned().unwrap_or_default();
    match cmd {
        "constants" => Ok(
            json!({"kinds":["app","lpk","desktop","shell"],"first":FIRST,
            "launcher":"shell:launcher","locked":LOCKED}),
        ),
        "parse" => Ok(parse(id)
            .map(|(kind, key)| json!({"kind":kind,"key":key}))
            .unwrap_or(Value::Null)),
        "id" => Ok(dock_id(&input["kind"], &input["key"])),
        "typed" => Ok(typed(id, &input["kind"])),
        "normalize" => {
            let Some(list) = input["list"].as_array() else {
                return Ok(json!({"list":[],"changed":!input["list"].is_null()}));
            };
            let kinds = array("kinds");
            let mut out = Vec::new();
            for (i, id) in list.iter().enumerate() {
                let value = typed(id, kinds.get(i).unwrap_or(&Value::Null));
                if !value.is_null() && !out.contains(&value) {
                    out.push(value);
                }
            }
            Ok(json!({"changed":out != *list,"list":out}))
        }
        "locked" => Ok(json!(array("locked").contains(id))),
        "movable" => Ok(json!(id != FIRST)),
        "move" | "pin" | "unpin" | "ensure" => {
            let mut list = array("list");
            match cmd {
                "move" | "pin" => {
                    let exists = list.contains(id);
                    if (cmd == "move" && (!exists || id == FIRST))
                        || (cmd == "pin"
                            && (parse(id).is_none() || (exists && input["omitted"] == true)))
                    {
                        return Ok(json!(list));
                    }
                    if exists && id == FIRST {
                        return Ok(json!(list));
                    }
                    list.retain(|x| x != id);
                    insert(&mut list, id.clone(), &input["index"], &array("shown"));
                }
                "unpin" => {
                    if !array("locked").contains(id) {
                        list.retain(|x| x != id);
                    }
                }
                "ensure" => {
                    let defaults = array("defaults");
                    for id in array("required") {
                        if list.contains(&id) || parse(&id).is_none() {
                            continue;
                        }
                        let at = defaults
                            .iter()
                            .position(|x| x == &id)
                            .map(|d| {
                                defaults[..d]
                                    .iter()
                                    .rev()
                                    .find_map(|x| list.iter().position(|y| y == x).map(|i| i + 1))
                                    .unwrap_or(0)
                            })
                            .unwrap_or(list.len());
                        list.insert(at, id);
                    }
                    files_first(&mut list);
                }
                _ => unreachable!(),
            }
            Ok(json!(list))
        }
        "actions" => {
            let mut identities = Vec::new();
            let mut selected = Vec::new();
            let max = input["max"].as_f64().unwrap_or(f64::INFINITY);
            for (i, a) in array("candidates").iter().enumerate() {
                if !action_id(a["id"].as_str().unwrap_or(""))
                    || identities.contains(&a["identity"])
                    || a["label"].as_str().unwrap_or("").is_empty()
                    || !args_valid(&a["args"])
                {
                    continue;
                }
                identities.push(a["identity"].clone());
                selected.push(i);
                if selected.len() as f64 >= max {
                    break;
                }
            }
            Ok(json!(selected))
        }
        _ => Err("unknown dock operation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_are_bounded_ascii_and_namespace_specific() {
        for id in [
            "app:files",
            "lpk:notes",
            "desktop:org.mozilla.firefox",
            "shell:launcher",
        ] {
            assert!(parse(&json!(id)).is_some());
        }
        for id in [
            "app:A",
            "lpk:a",
            "desktop:a..b",
            "desktop:lpk-notes",
            "app:á",
            "shell:trash",
            "app:x\n",
        ] {
            assert!(parse(&json!(id)).is_none(), "{id:?}");
        }
        assert!(valid_key("desktop", &"x".repeat(62)));
        assert!(!valid_key("desktop", &"x".repeat(63)));
    }
    #[test]
    fn hidden_items_do_not_count_and_files_stays_first() {
        let r = request(
            &json!({"cmd":"pin","list":[FIRST,"lpk:gone","app:last"],"id":"app:new",
            "index":0,"shown":[FIRST,"app:last"]}),
        )
        .unwrap();
        assert_eq!(r, json!([FIRST, "lpk:gone", "app:new", "app:last"]));
    }
    #[test]
    fn action_arguments_reject_reserved_nested_and_non_ascii_keys() {
        assert!(args_valid(
            &json!({"section":"network","enabled":true,"empty":null})
        ));
        for args in [
            json!({"nonce":1}),
            json!({"nested":{}}),
            json!({"arr":[]}),
            json!({"á":1}),
        ] {
            assert!(!args_valid(&args));
        }
    }
    #[test]
    fn malformed_operations_fail_without_session_state() {
        assert!(request(&json!({})).is_err());
        assert_eq!(
            request(&json!({"cmd":"normalize","list":null})).unwrap(),
            json!({"list":[],"changed":false})
        );
    }
}
