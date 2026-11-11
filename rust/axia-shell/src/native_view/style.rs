//! React-style objects to the runtime's properties, for trees that Rust mounts directly. The
//! conversion itself is `crate::css_style`, the one the React renderer uses too (lord.css), so a
//! style looks the same in a native view and in a JS app.
use crate::runtime::native::Prop;
use serde_json::Value;
use std::collections::BTreeMap;
pub type Props = BTreeMap<String, Prop>;

fn pairs(style: &Value, out: &mut Vec<Value>) {
    match style {
        Value::Array(list) => list.iter().for_each(|s| pairs(s, out)),
        Value::Object(map) => out.extend(map.iter().map(|(k, v)| Value::Array(vec![Value::String(k.clone()), v.clone()]))),
        _ => {}
    }
}

/// A style object, or nested arrays of them (later ones win), as the engine's properties.
pub fn normalize(style: &Value) -> Props {
    let mut list = Vec::new();
    pairs(style, &mut list);
    crate::css_style::normalize(&list)
        .into_iter()
        .filter_map(|(k, v)| {
            let prop = match v {
                Value::Number(n) => Prop::Num(n.as_f64()?),
                Value::String(s) => Prop::Str(s),
                Value::Array(g) if g.len() == 3 => Prop::Gradient(g[0].as_f64()?, g[1].as_f64()?, g[2].as_f64()?),
                _ => return None,                     // null: nothing to set (a reset in diff)
            };
            Some((k, prop))
        })
        .collect()
}
/// The properties to send: removed keys reset (`None`), changed ones set.
pub fn diff<'a>(prev: &'a Props, next: &'a Props) -> Vec<(&'a str, Option<&'a Prop>)> {
    let mut out: Vec<_> = prev
        .keys()
        .filter(|k| !next.contains_key(*k))
        .map(|k| (k.as_str(), None))
        .collect();
    out.extend(
        next.iter()
            .filter(|(k, v)| prev.get(*k) != Some(v))
            .map(|(k, v)| (k.as_str(), Some(v))),
    );
    out
}
