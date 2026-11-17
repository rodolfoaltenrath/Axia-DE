//! Contact-card policy shared by the native list, editor and PIM jobs.
//! Unknown vCard properties survive edits; book + UID is the identity everywhere.
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    ffi::{c_char, c_void, CStr},
};

#[link(name = "glib-2.0")]
extern "C" {
    fn g_utf8_normalize(s: *const c_char, len: isize, mode: i32) -> *mut c_char;
    fn g_free(p: *mut c_void);
}

pub const KINDS: [&str; 4] = ["phones", "emails", "urls", "addrs"];
pub fn string(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn rows<'a>(v: &'a Value, field: &str) -> impl Iterator<Item = &'a Value> {
    v[field].as_array().into_iter().flatten()
}
pub fn key(c: &Value) -> String {
    format!("{}/{}", string(&c["book"]), string(&c["uid"]))
}
pub fn display_name(c: &Value) -> String {
    for candidate in [
        string(&c["fn"]).to_owned(),
        full_name(c),
        string(&c["org"]).to_owned(),
        string(&c["emails"][0]["value"]).to_owned(),
    ] {
        if !candidate.is_empty() {
            return candidate;
        }
    }
    String::new()
}
fn full_name(c: &Value) -> String {
    ["prefix", "given", "middle", "family", "suffix"]
        .iter()
        .map(|k| string(&c["n"][k]))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn initials(c: &Value) -> String {
    let s: String = ["given", "family"]
        .iter()
        .filter_map(|k| string(&c["n"][k]).trim().chars().next())
        .collect();
    if !s.is_empty() {
        return s.to_uppercase();
    }
    display_name(c)
        .trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}
pub fn photo_color(c: &Value) -> &'static str {
    let hash = display_name(c)
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    [
        "#ff6b6b", "#f7a440", "#e0b400", "#4cb860", "#36a3d9", "#6c7ae0", "#b06ad9", "#e05f9b",
        "#8e8e93",
    ][hash as usize % 9]
}
pub fn sort_key(c: &Value) -> String {
    let s = ["family", "given"]
        .iter()
        .map(|k| string(&c["n"][k]))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if s.is_empty() {
        display_name(c).trim().to_owned()
    } else {
        s.trim().to_owned()
    }
}
// QuickJS localeCompare uses NFC code points, without an Intl collator. Match
// that ordering rather than silently introducing machine-dependent locales.
fn normalized(s: &str) -> String {
    unsafe {
        let p = g_utf8_normalize(s.as_ptr().cast(), s.len() as isize, 1);
        if p.is_null() {
            return s.into();
        }
        let value = CStr::from_ptr(p).to_string_lossy().into_owned();
        g_free(p.cast());
        value
    }
}
pub fn letter(c: &Value) -> String {
    sort_key(c)
        .chars()
        .next()
        .filter(|c| c.is_alphabetic())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "#".into())
}
pub fn matches(c: &Value, query: &str) -> bool {
    let mut fields = vec![
        display_name(c),
        string(&c["org"]).into(),
        string(&c["nick"]).into(),
    ];
    for kind in ["emails", "phones"] {
        fields.extend(rows(c, kind).map(|r| string(&r["value"]).to_owned()));
    }
    let hay = fields.join("\n").to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| hay.contains(word))
}
pub fn groups(all: &[Value]) -> Vec<&Value> {
    let mut result: Vec<_> = all.iter().filter(|c| c["kind"] == "group").collect();
    result.sort_by_cached_key(|c| normalized(&display_name(c)));
    result
}
pub fn source_group<'a>(all: &'a [Value], source: &str) -> Option<&'a Value> {
    let id = source.strip_prefix("group:")?;
    all.iter().find(|c| c["kind"] == "group" && key(c) == id)
}
pub fn source_book<'a>(all: &'a [Value], source: &'a str) -> Option<&'a str> {
    source
        .strip_prefix("book:")
        .or_else(|| source_group(all, source).map(|g| string(&g["book"])))
}
pub fn visible<'a>(all: &'a [Value], source: &str, query: &str) -> Vec<&'a Value> {
    let group = source_group(all, source);
    let book = source_book(all, source);
    let members: HashSet<_> = group
        .into_iter()
        .flat_map(|g| rows(g, "members"))
        .filter_map(Value::as_str)
        .collect();
    let mut result: Vec<_> = all
        .iter()
        .filter(|c| c["kind"] != "group")
        .filter(|c| book.is_none_or(|b| c["book"] == b))
        .filter(|c| group.is_none() || members.contains(string(&c["uid"])))
        .filter(|c| matches(c, query))
        .collect();
    result.sort_by_cached_key(|c| normalized(&sort_key(c)));
    result
}
/// None tells the controller to create a local address book before the write.
pub fn target_book<'a>(books: &'a [Value], selected: Option<&str>) -> Option<&'a str> {
    selected
        .and_then(|id| books.iter().find(|b| b["id"] == id))
        .or_else(|| books.iter().find(|b| b["kind"] == "local"))
        .or_else(|| books.first())
        .map(|b| string(&b["id"]))
}
pub fn blank() -> Value {
    json!({"n":{"given":"","family":""},"org":"","title":"","phones":[],"emails":[],"urls":[],"addrs":[],"notes":"","kind":"individual"})
}
pub fn field_types(kind: &str) -> &'static [&'static str] {
    if kind == "phones" {
        &["cell", "home", "work", "main", "other"]
    } else {
        &["home", "work", "other"]
    }
}
pub fn field_type(kind: &str, raw: &str) -> String {
    let lower = raw.to_lowercase();
    let parts: Vec<_> = lower
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    if let Some(p) = parts
        .iter()
        .find(|p| field_types(kind).contains(p) || (kind == "phones" && **p == "mobile"))
    {
        return if *p == "mobile" {
            "cell".into()
        } else {
            (*p).into()
        };
    }
    parts
        .into_iter()
        .find(|p| !["pref", "voice", "internet"].contains(p))
        .unwrap_or("other")
        .into()
}
/// Trim editable fields while retaining all imported vCard extensions. Empty
/// rows are omitted. A contact needs a usable name, organization or address.
pub fn build(draft: &Value) -> Result<Value, &'static str> {
    let mut c = if draft.is_object() {
        draft.clone()
    } else {
        blank()
    };
    if !c["n"].is_object() {
        c["n"] = json!({});
    }
    for k in ["given", "family", "middle", "prefix", "suffix"] {
        if !c["n"][k].is_null() {
            c["n"][k] = json!(string(&c["n"][k]).trim());
        }
    }
    for k in ["org", "title"] {
        c[k] = json!(string(&c[k]).trim());
    }
    for kind in KINDS {
        let fields: &[&str] = if kind == "addrs" {
            &["street", "city", "region", "code", "country"]
        } else {
            &["value"]
        };
        let cleaned: Vec<_> = rows(&c, kind)
            .filter(|r| r.is_object())
            .filter_map(|r| {
                let mut row = r.clone();
                if string(&row["type"]).is_empty() {
                    row["type"] = json!(if kind == "addrs" { "home" } else { "other" });
                }
                for field in fields {
                    row[field] = json!(string(&row[field]).trim());
                }
                fields
                    .iter()
                    .any(|field| !string(&row[field]).is_empty())
                    .then_some(row)
            })
            .collect();
        c[kind] = json!(cleaned);
    }
    let name = [
        full_name(&c),
        string(&c["org"]).into(),
        string(&c["emails"][0]["value"]).into(),
        string(&c["phones"][0]["value"]).into(),
    ]
    .into_iter()
    .find(|s| !s.is_empty())
    .ok_or("contacts.needName")?;
    c["fn"] = json!(name);
    c.as_object_mut().unwrap().remove("_new");
    Ok(c)
}
/// UIDs are scoped to their address book. Cross-book namesakes must never be
/// shown as members or written into the other book's group.
pub fn is_member(group: &Value, contact: &Value) -> bool {
    group["book"] == contact["book"] && rows(group, "members").any(|m| m == &contact["uid"])
}
pub fn toggle_member(group: &Value, contact: &Value) -> Option<Value> {
    if group["book"] != contact["book"] || string(&contact["uid"]).is_empty() {
        return None;
    }
    let mut changed = group.clone();
    let exists = is_member(group, contact);
    let mut members: Vec<_> = rows(group, "members")
        .filter(|m| !exists || *m != &contact["uid"])
        .cloned()
        .collect();
    if !exists {
        members.push(contact["uid"].clone());
    }
    changed["members"] = json!(members);
    Some(changed)
}

#[derive(Debug, PartialEq)]
pub enum Row {
    Heading(String),
    Contact(usize),
}
pub struct ListLayout {
    pub rows: Vec<(f64, Row)>,
    pub height: f64,
}
impl ListLayout {
    pub fn new(contacts: &[&Value]) -> Self {
        let mut rows = vec![];
        let mut height = 0.;
        let mut previous = String::new();
        for (i, c) in contacts.iter().enumerate() {
            let section = letter(c);
            if section != previous {
                previous = section.clone();
                rows.push((height, Row::Heading(section)));
                height += 24.;
            }
            rows.push((height, Row::Contact(i)));
            height += 30.;
        }
        Self { rows, height }
    }
    pub fn viewport(&self, offset: f64, height: f64) -> std::ops::Range<usize> {
        let offset = if offset.is_finite() {
            offset.max(0.)
        } else {
            0.
        };
        let height = if height.is_finite() {
            height.max(0.)
        } else {
            0.
        };
        if height == 0. {
            return 0..0;
        }
        let start = self.rows.partition_point(|(y, row)| {
            *y + if matches!(row, Row::Heading(_)) {
                24.
            } else {
                30.
            } <= offset
        });
        let end = self.rows.partition_point(|(y, _)| *y < offset + height);
        start.min(end)..end
    }
    pub fn reveal(&self, index: usize, offset: f64, height: f64) -> f64 {
        let Some((y, _)) = self.rows.iter().find(|(_, r)| *r == Row::Contact(index)) else {
            return offset;
        };
        if *y < offset {
            *y
        } else if *y + 30. > offset + height {
            (*y + 30. - height).max(0.)
        } else {
            offset
        }
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
