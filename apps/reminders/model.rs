//! Local-calendar semantics and smart-list ordering from the original app.
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
pub const RED: &str = "#ff3b30";
pub const COLORS: [&str; 8] = [
    "#ff3b30", "#ff9500", "#ffcc00", "#34c759", "#30b0c7", "#007aff", "#af52de", "#a2845e",
];
pub const SMART: [(&str, &str); 5] = [
    ("today", "#007aff"),
    ("scheduled", RED),
    ("all", "#5b5b60"),
    ("flagged", "#ff9500"),
    ("completed", "#8e8e93"),
];
pub fn string(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn key(it: &Value) -> String {
    format!("{}/{}", string(&it["list"]), string(&it["uid"]))
}
pub fn timestamp(mut t: libc::tm) -> i64 {
    t.tm_isdst = -1;
    unsafe { libc::mktime(&mut t) as i64 * 1000 }
}
pub fn midnight(ms: i64) -> i64 {
    let mut t = crate::clock::local(ms);
    t.tm_hour = 0;
    t.tm_min = 0;
    t.tm_sec = 0;
    timestamp(t)
}
pub fn at_minutes(day: i64, minutes: i32) -> i64 {
    let mut t = crate::clock::local(day);
    t.tm_hour = minutes / 60;
    t.tm_min = minutes % 60;
    t.tm_sec = 0;
    timestamp(t)
}
pub fn add_days(ms: i64, days: i32) -> i64 {
    let mut t = crate::clock::local(ms);
    t.tm_mday += days;
    timestamp(t)
}
pub fn month(ms: i64, delta: i32) -> i64 {
    let mut t = crate::clock::local(midnight(ms));
    t.tm_mday = 1;
    t.tm_mon += delta;
    timestamp(t)
}
pub fn ymd(ms: i64) -> String {
    let t = crate::clock::local(ms);
    format!(
        "{:04}-{:02}-{:02}",
        t.tm_year + 1900,
        t.tm_mon + 1,
        t.tm_mday
    )
}
pub fn parse_day(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b[..10]
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return None;
    }
    let mut t: libc::tm = unsafe { std::mem::zeroed() };
    t.tm_year = s[..4].parse::<i32>().ok()? - 1900;
    t.tm_mon = s[5..7].parse::<i32>().ok()? - 1;
    t.tm_mday = s[8..10].parse().ok()?;
    Some(timestamp(t))
}
pub fn due_day(it: &Value) -> Option<String> {
    let due = it["due"].as_i64()?;
    Some(
        if it["dueAllDay"] == true && !string(&it["dueDate"]).is_empty() {
            string(&it["dueDate"]).into()
        } else {
            ymd(due)
        },
    )
}
pub fn today(it: &Value, now: i64) -> bool {
    due_day(it).is_some_and(|d| d <= ymd(now))
}
pub fn overdue(it: &Value, now: i64) -> bool {
    it["done"] != true
        && it["due"].as_i64().is_some_and(|due| {
            if it["dueAllDay"] == true {
                due_day(it).unwrap_or_default() < ymd(now)
            } else {
                due < now
            }
        })
}
pub fn matches(sel: &str, it: &Value, now: i64) -> bool {
    match sel {
        "today" => today(it, now),
        "scheduled" => it["due"].is_number(),
        "flagged" => it["flag"] == true,
        "completed" => it["done"] == true,
        "all" => true,
        _ => sel.strip_prefix("list:").is_some_and(|id| it["list"] == id),
    }
}
pub fn marks(prio: i64) -> &'static str {
    match prio {
        1 => "!!!",
        2..=5 => "!!",
        6.. => "!",
        _ => "",
    }
}
pub fn counts(items: &[Value], now: i64) -> Value {
    let mut c = json!({"today":0,"scheduled":0,"all":0,"flagged":0,"completed":0,"lists":{}});
    for it in items {
        if it["done"] == true {
            c["completed"] = json!(c["completed"].as_u64().unwrap() + 1);
            continue;
        }
        for (name, include) in [
            ("all", true),
            ("scheduled", it["due"].is_number()),
            ("today", today(it, now)),
            ("flagged", it["flag"] == true),
        ] {
            if include {
                c[name] = json!(c[name].as_u64().unwrap() + 1);
            }
        }
        let id = string(&it["list"]);
        c["lists"][id] = json!(c["lists"][id].as_u64().unwrap_or(0) + 1);
    }
    c
}
pub fn sort_items(sel: &str, items: Vec<Value>) -> Vec<(Value, usize)> {
    let (mut open, mut done): (Vec<_>, Vec<_>) =
        items.into_iter().partition(|it| it["done"] != true);
    if !sel.starts_with("list:") {
        open.sort_by_key(|it| it["due"].as_i64().unwrap_or(i64::MAX));
    }
    done.sort_by_key(|it| std::cmp::Reverse(it["doneAt"].as_i64().unwrap_or(0)));
    let uids: HashSet<_> = open.iter().map(|it| string(&it["uid"])).collect();
    let mut children: HashMap<&str, Vec<&Value>> = HashMap::new();
    for it in &open {
        let parent = string(&it["parent"]);
        if !parent.is_empty() && uids.contains(parent) {
            children.entry(parent).or_default().push(it);
        }
    }
    let mut out = vec![];
    for it in &open {
        let p = string(&it["parent"]);
        if !p.is_empty() && uids.contains(p) {
            continue;
        }
        out.push((it.clone(), 0));
        for child in children.get(string(&it["uid"])).into_iter().flatten() {
            out.push(((*child).clone(), 1));
        }
    }
    out.extend(done.into_iter().map(|it| (it, 0)));
    out
}
pub struct Section {
    pub id: String,
    pub title: Value,
    pub color: String,
    pub rows: Vec<(Value, usize)>,
}
pub fn sections(
    sel: &str,
    lists: &[Value],
    items: &[Value],
    show_done: bool,
    now: i64,
) -> Vec<Section> {
    let shown: Vec<_> = items
        .iter()
        .filter(|it| {
            matches(sel, it, now) && (show_done || sel == "completed" || it["done"] != true)
        })
        .cloned()
        .collect();
    if sel == "all" {
        return lists
            .iter()
            .filter_map(|list| {
                let rows = sort_items(
                    sel,
                    shown
                        .iter()
                        .filter(|it| it["list"] == list["id"])
                        .cloned()
                        .collect(),
                );
                (!rows.is_empty()).then(|| Section {
                    id: string(&list["id"]).into(),
                    title: list["name"].clone(),
                    color: string(&list["color"]).into(),
                    rows,
                })
            })
            .collect();
    }
    if sel == "scheduled" {
        let mut days: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for it in shown {
            days.entry(due_day(&it).unwrap_or_default())
                .or_default()
                .push(it);
        }
        let today = ymd(now);
        let mut out: Vec<Section> = vec![];
        for (day, items) in days {
            let rows = sort_items(sel, items);
            out.push(Section {
                id: day.clone(),
                title: if day < today {
                    json!({"key":"reminders.overdue"})
                } else if day == today {
                    json!({"key":"reminders.smart.today"})
                } else {
                    json!({"day":day})
                },
                color: if day < today {
                    RED.into()
                } else {
                    String::new()
                },
                rows,
            });
        }
        let overdue = out.iter().take_while(|s| s.id < today).count();
        if overdue > 1 {
            let mut merged = Section {
                id: "overdue".into(),
                title: out[0].title.clone(),
                color: RED.into(),
                rows: vec![],
            };
            for s in out.drain(..overdue) {
                merged.rows.extend(s.rows);
            }
            out.insert(0, merged);
        }
        return out;
    }
    vec![Section {
        id: "one".into(),
        title: Value::Null,
        color: String::new(),
        rows: sort_items(sel, shown),
    }]
}
