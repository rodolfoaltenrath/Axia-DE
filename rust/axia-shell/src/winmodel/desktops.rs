//! The list of desktops (docs/DESKTOPS.md). A desktop is a stable number; `list` is their order on
//! screen and `active` the one showing. Which window shows where stays with the window list
//! (sdk/desktopmodel.js).
use super::{get, num, same};
use serde_json::{json, Value};

/// sdk/desktopmodel.js MAX_DESKTOPS says the same.
pub const MAX_DESKTOPS: usize = 9;

fn list_of(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

fn index_of(list: &[Value], id: &Value) -> Option<usize> {
    list.iter().position(|d| same(d, id))
}

/// A new desktop at the end, with an id not in the list. id null when there are already MAX_DESKTOPS.
pub fn add(state: &Value) -> Value {
    let list = list_of(get(state, "list"));
    if list.len() >= MAX_DESKTOPS {
        return json!({ "state": state, "id": null });
    }
    let id = list.iter().filter_map(Value::as_f64).fold(0.0, f64::max) + 1.0;
    let mut next = state.clone();
    let mut ids = list.to_vec();
    ids.push(num(id));
    if let Some(o) = next.as_object_mut() {
        o.insert("list".into(), Value::Array(ids));
    }
    json!({ "state": next, "id": num(id) })
}

/// The desktop whose windows move in when `id` closes: the next one, or the previous for the last.
pub fn next_desktop(list: &[Value], id: &Value) -> Option<Value> {
    let i = index_of(list, id)?;
    if i + 1 < list.len() {
        return Some(list[i + 1].clone());
    }
    (i > 0).then(|| list[i - 1].clone())
}

/// Removes a desktop. target: where its windows go (null: nothing changed, the last desktop stays).
pub fn remove(state: &Value, id: &Value) -> Value {
    let list = list_of(get(state, "list"));
    let Some(target) = next_desktop(list, id) else {
        return json!({ "state": state, "target": null });
    };
    let active = get(state, "active");
    let rest: Vec<Value> = list.iter().filter(|d| !same(d, id)).cloned().collect();
    let active = if same(active, id) { target.clone() } else { active.clone() };
    json!({ "state": { "list": rest, "active": active }, "target": target })
}

/// step: -1 previous, 1 next. No wrap: the first and the last desktop are ends.
pub fn neighbor(list: &Value, active: &Value, step: f64) -> Value {
    let list = list_of(list);
    let i = index_of(list, active).map_or(-1.0, |i| i as f64) + step;
    if i >= 0.0 && i.fract() == 0.0 && (i as usize) < list.len() {
        list[i as usize].clone()
    } else {
        Value::Null
    }
}

/// Left and Right follow the layout: the strip is mirrored in right-to-left languages. Swiping is
/// the opposite (it brings what is on that side, as a page does).
pub fn arrow_step(dir: &Value, rtl: bool) -> f64 {
    if (dir.as_str() == Some("left")) != rtl {
        -1.0
    } else {
        1.0
    }
}

fn whole(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite() && n.fract() == 0.0)
}

/// A saved number of desktops, from a file any program of the user can edit.
pub fn clean_count(v: &Value) -> f64 {
    whole(v).filter(|n| (1.0..=MAX_DESKTOPS as f64).contains(n)).unwrap_or(1.0)
}

/// A saved position in the list (0 is the first desktop).
pub fn clean_index(v: &Value) -> f64 {
    whole(v).filter(|n| *n >= 0.0 && *n < MAX_DESKTOPS as f64).unwrap_or(0.0)
}
