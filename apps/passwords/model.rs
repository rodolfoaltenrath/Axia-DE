//! Vault presentation metadata and category policy. Entry secrets remain in
//! axia-vault; this module never requests, caches, logs or persists them.
#[path = "data.rs"]
mod data;
use serde_json::{json, Value};

fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn builtin(id: &Value) -> Option<&'static Value> {
    array(data::builtin())
        .iter()
        .chain(array(data::system()))
        .find(|c| c["id"] == *id)
}
fn categories(users: &Value) -> Value {
    let mut out = Vec::new();
    for cat in array(data::builtin()) {
        let mut c = cat.clone();
        c["builtin"] = json!(true);
        out.push(c);
    }
    for cat in array(users) {
        if cat.is_object()
            && cat["id"].as_str().is_some_and(|s| !s.is_empty())
            && builtin(&cat["id"]).is_none()
        {
            let mut c = cat.clone();
            c["builtin"] = json!(false);
            out.push(c);
        }
    }
    json!(out)
}
fn fields(cat: &Value, cats: &Value) -> Value {
    let mut current = cat;
    // A user can import cyclic category parents. Bound traversal without
    // changing the inheritance of valid category trees.
    for _ in 0..64 {
        if !array(&current["fields"]).is_empty() {
            return current["fields"].clone();
        }
        if let Some(base) = builtin(&current["template"]) {
            return base["fields"].clone();
        }
        if let Some(parent) = current["parent"].as_str().filter(|s| !s.is_empty()) {
            if let Some(base) = array(cats).iter().find(|c| c["id"] == parent) {
                current = base;
                continue;
            }
        }
        break;
    }
    data::builtin()[0]["fields"].clone()
}
fn in_cat(entry: &Value, id: &Value, cats: &Value) -> bool {
    if entry == id {
        return true;
    }
    let mut c = array(cats).iter().find(|c| c["id"] == *entry);
    for _ in 0..16 {
        let Some(parent) = c
            .and_then(|c| c["parent"].as_str())
            .filter(|s| !s.is_empty())
        else {
            break;
        };
        if *id == parent {
            return true;
        }
        c = array(cats).iter().find(|c| c["id"] == parent);
    }
    false
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
fn selected(entry: &Value, sel: &str, cats: &Value) -> bool {
    if sel == "login" {
        return true;
    }
    if sel == "trash" {
        return truth(&entry["trash"]);
    }
    if truth(&entry["trash"]) {
        return false;
    }
    match sel {
        "all" => !truth(&entry["arch"]),
        "fav" => truth(&entry["fav"]),
        "archived" => truth(&entry["arch"]),
        _ if sel.starts_with("cat:") => in_cat(&entry["cat"], &json!(&sel[4..]), cats),
        _ if sel.starts_with("tag:") => array(&entry["tags"]).contains(&json!(&sel[4..])),
        _ => true,
    }
}
pub fn request(input: &Value) -> Value {
    match input["cmd"].as_str().unwrap_or("") {
        "catalog" => {
            json!({"builtin":data::builtin(),"system":data::system(),"types":data::types(),"colors":data::colors()})
        }
        "builtin" => builtin(&input["id"]).cloned().unwrap_or(Value::Null),
        "categories" => categories(&input["users"]),
        "fields" => fields(&input["category"], &input["categories"]),
        "inCategory" => json!(in_cat(&input["entry"], &input["id"], &input["categories"])),
        "filter" => json!(array(&input["entries"])
            .iter()
            .enumerate()
            .filter(|(_, e)| selected(
                e,
                input["selection"].as_str().unwrap_or("all"),
                &input["categories"]
            ))
            .map(|(i, _)| i)
            .collect::<Vec<_>>()),
        "summary" => {
            let entries = array(&input["entries"]);
            let live: Vec<_> = entries.iter().filter(|e| !truth(&e["trash"])).collect();
            let mut counts = json!({});
            for cat in array(&input["categories"]) {
                if let Some(id) = cat["id"].as_str() {
                    counts[id] = json!(live
                        .iter()
                        .filter(|e| in_cat(&e["cat"], &cat["id"], &input["categories"]))
                        .count());
                }
            }
            let mut tags = serde_json::Map::new();
            for e in &live {
                let mut seen = std::collections::BTreeSet::new();
                for tag in array(&e["tags"]).iter().filter_map(Value::as_str) {
                    if seen.insert(tag) {
                        let count = tags.get(tag).and_then(Value::as_u64).unwrap_or(0);
                        tags.insert(tag.into(), json!(count + 1));
                    }
                }
            }
            json!({"all":live.iter().filter(|e|!truth(&e["arch"])).count(),"fav":live.iter().filter(|e|truth(&e["fav"])).count(),
                "codes":live.iter().filter(|e|truth(&e["totp"])).count(),"archived":live.iter().filter(|e|truth(&e["arch"])).count(),
                "trash":entries.len()-live.len(),"categories":counts,"tags":tags})
        }
        _ => json!({"error":"app.invalidRequest"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn categories_and_fields_preserve_order_overrides_and_inheritance() {
        let cats = categories(
            &json!([null,{}, {"id":"internet","name":"override"},{"id":"child","parent":"internet"}]),
        );
        assert_eq!(array(&cats).len(), 17);
        assert_eq!(cats[0]["id"], "general");
        assert_eq!(cats[16]["builtin"], false);
        assert_eq!(fields(&cats[16], &cats), data::builtin()[1]["fields"]);
        assert!(in_cat(&json!("child"), &json!("internet"), &cats));
        assert!(!in_cat(&json!("child"), &json!("general"), &cats));
        let custom = json!({"fields":[{"k":"c1","t":"hidden"}],"template":"internet"});
        assert_eq!(fields(&custom, &cats), custom["fields"]);
        let cycle = json!([{"id":"a","parent":"b"},{"id":"b","parent":"a"}]);
        assert_eq!(fields(&cycle[0], &cycle), data::builtin()[0]["fields"]);
    }
    #[test]
    fn lists_keep_archived_favorites_tags_and_trash_rules() {
        let entries = json!([{"cat":"wifi"},{"cat":"wifi","fav":true,"arch":true,"tags":["work"]},{"cat":"wifi","fav":true,"trash":true}]);
        for (selection, expected) in [
            ("all", json!([0])),
            ("fav", json!([1])),
            ("trash", json!([2])),
            ("cat:wifi", json!([0, 1])),
            ("tag:work", json!([1])),
            ("login", json!([0, 1, 2])),
        ] {
            assert_eq!(
                request(&json!({"cmd":"filter","entries":entries,"selection":selection})),
                expected
            );
        }
    }
}
