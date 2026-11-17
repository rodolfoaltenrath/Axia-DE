//! Calendar's civil-date ranges, occurrence layout and lossless editor policy.
use crate::native_view::civil_date as date;
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub const HOUR: f64 = 48.;
pub const GUTTER: f64 = 56.;
pub const COLORS: [&str; 8] = [
    "#ff3b30", "#ff9500", "#ffcc00", "#34c759", "#30b0c7", "#007aff", "#af52de", "#a2845e",
];
pub const ALERTS: [i64; 7] = [0, 5, 10, 15, 30, 60, 1440];
pub const FREQS: [&str; 4] = ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"];
pub fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn num(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}
pub fn key(e: &Value) -> String {
    format!("{}/{}/{}", s(&e["cal"]), s(&e["uid"]), num(&e["start"]))
}
pub fn range(view: &str, cursor: i64) -> (i64, i64) {
    let first = match view {
        "day" => date::day(cursor),
        "month" => date::week(date::month(cursor, 0)),
        _ => date::week(cursor),
    };
    (
        first,
        date::add_days(
            first,
            match view {
                "day" => 1,
                "month" => 42,
                _ => 7,
            },
        ),
    )
}
pub fn event_days(e: &Value) -> Vec<i64> {
    let all_day = e["allDay"] == true;
    let start = num(&e["start"]);
    let end = num(&e["end"]);
    let first = if all_day {
        date::parse(s(&e["sdate"])).unwrap_or_else(|| date::day(start))
    } else {
        date::day(start)
    };
    let last = if all_day {
        date::parse(s(&e["edate"])).unwrap_or_else(|| date::day(end))
    } else {
        start.max(end.saturating_sub(1))
    };
    let mut out = vec![];
    let mut day = first;
    for _ in 0..367 {
        if if all_day {
            day >= last && !out.is_empty()
        } else {
            day > last
        } {
            break;
        }
        out.push(day);
        let next = date::add_days(day, 1);
        if next <= day {
            break;
        }
        day = next;
    }
    out
}
fn title_key(title: &str) -> String {
    use std::ffi::{c_char, c_void, CStr};
    #[link(name = "glib-2.0")]
    extern "C" {
        fn g_utf8_normalize(p: *const c_char, len: isize, mode: i32) -> *mut c_char;
        fn g_free(p: *mut c_void);
    }
    unsafe {
        let p = g_utf8_normalize(title.as_ptr().cast(), title.len() as isize, 1);
        if p.is_null() {
            return title.into();
        }
        let text = CStr::from_ptr(p).to_string_lossy().into_owned();
        g_free(p.cast());
        text
    }
}
pub fn by_day(events: &[Value]) -> BTreeMap<String, Vec<usize>> {
    let mut days: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in events.iter().enumerate() {
        for day in event_days(e) {
            days.entry(date::ymd(day)).or_default().push(i);
        }
    }
    for list in days.values_mut() {
        list.sort_by_cached_key(|i| {
            let e = &events[*i];
            (
                e["allDay"] != true,
                num(&e["start"]),
                title_key(s(&e["title"])),
            )
        });
    }
    days
}
#[derive(Debug, Clone)]
pub struct Segment {
    pub index: usize,
    pub top: f64,
    pub bottom: f64,
    pub col: usize,
    pub cols: usize,
}
pub fn layout(day: i64, events: &[Value], indices: &[usize]) -> Vec<Segment> {
    let first = date::day(day);
    let end = date::add_days(day, 1);
    let mut segs: Vec<_> = indices
        .iter()
        .copied()
        .filter(|i| events[*i]["allDay"] != true)
        .map(|index| {
            let e = &events[index];
            let start = num(&e["start"]);
            let stop = num(&e["end"])
                .max(start.saturating_add(15 * 60000))
                .min(end);
            Segment {
                index,
                top: (start.max(first) - first) as f64 / 60000.,
                bottom: (stop - first) as f64 / 60000.,
                col: 0,
                cols: 0,
            }
        })
        .collect();
    segs.sort_by(|a, b| {
        a.top
            .total_cmp(&b.top)
            .then_with(|| b.bottom.total_cmp(&a.bottom))
    });
    let mut group_start = 0;
    let mut ends: Vec<f64> = vec![];
    let mut group_end = -1.;
    for i in 0..segs.len() {
        if segs[i].top >= group_end {
            for seg in &mut segs[group_start..i] {
                seg.cols = ends.len();
            }
            group_start = i;
            ends.clear();
            group_end = -1.;
        }
        let seg = &mut segs[i];
        let col = ends
            .iter()
            .position(|end| *end <= seg.top)
            .unwrap_or(ends.len());
        if col == ends.len() {
            ends.push(0.);
        }
        ends[col] = seg.bottom.max(seg.top + 20.);
        seg.col = col;
        group_end = group_end.max(ends[col]);
    }
    for seg in &mut segs[group_start..] {
        seg.cols = ends.len();
    }
    segs
}
pub fn color(e: &Value, cals: &[Value]) -> String {
    e["color"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cals.iter()
                .find(|c| c["id"] == e["cal"])
                .and_then(|c| c["color"].as_str())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("#007aff")
        .into()
}
pub fn tint(color: &str, dark: bool) -> String {
    if color.len() == 7
        && color.starts_with('#')
        && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
    {
        format!("{color}{}", if dark { "4d" } else { "2e" })
    } else {
        "rgba(0,122,255,0.2)".into()
    }
}
pub fn default_cal(cals: &[Value]) -> Option<&Value> {
    cals.iter()
        .filter(|c| c["events"] == true)
        .find(|c| c["hidden"] != true)
        .or_else(|| cals.iter().find(|c| c["events"] == true))
}
pub fn new_event(cals: &[Value], day: i64, minutes: Option<i32>, now: i64) -> Value {
    let min = minutes
        .unwrap_or_else(|| ((crate::clock::local(now).tm_hour + 1) * 60).min(23 * 60))
        .clamp(0, 1439);
    let start = date::at_minutes(day, min);
    json!({"title":"","start":start,"end":start+3600000,"allDay":false,"alarms":[],"cal":default_cal(cals).map(|c|s(&c["id"])).unwrap_or("")})
}
pub fn draft(e: &Value, cals: &[Value]) -> Value {
    let all = e["allDay"] == true;
    let start = if all {
        date::parse(s(&e["sdate"])).unwrap_or_else(|| date::day(num(&e["start"])))
    } else {
        num(&e["start"])
    };
    let end = if all {
        date::parse(s(&e["edate"])).unwrap_or_else(|| date::day(num(&e["end"])))
    } else {
        num(&e["end"])
    };
    let end_day = if all {
        date::add_days(end, -1)
    } else {
        date::day(end)
    };
    json!({"title":e["title"].as_str().unwrap_or(""),"loc":e["loc"].as_str().unwrap_or(""),"notes":e["notes"].as_str().unwrap_or(""),"allDay":all,"sDay":date::day(start),"eDay":end_day.max(date::day(start)),"sMin":if all{540}else{date::minutes(start)},"eMin":if all{600}else{date::minutes(end)},"rrule":e["rrule"].as_str().unwrap_or(""),"alarm":e["alarms"][0].as_i64().unwrap_or(-1),"cal":e["cal"].as_str().filter(|s|!s.is_empty()).or_else(||cals.first().map(|c|s(&c["id"]))).unwrap_or("")})
}
pub fn set_start_day(d: &mut Value, day: i64) {
    let days = ((num(&d["eDay"]) - num(&d["sDay"])) as f64 / 86400000.).round() as i32;
    let start = date::day(day);
    d["sDay"] = json!(start);
    d["eDay"] = json!(date::add_days(start, days));
}
pub fn set_start_min(d: &mut Value, min: i32) {
    let same = d["sDay"] == d["eDay"];
    let mut end = num(&d["eMin"]) + (min as i64 - num(&d["sMin"]));
    if !same || end >= 1440 || end <= min as i64 {
        end = if same {
            1439.min(min as i64 + 60)
        } else {
            num(&d["eMin"])
        };
    }
    d["sMin"] = json!(min);
    d["eMin"] = json!(end);
}
pub fn recurring(e: &Value, new: bool) -> bool {
    !new && !s(&e["rrule"]).is_empty() && num(&e["occ"]) != 0
}
pub fn build(e: &Value, d: &Value, new: bool, series_start: Option<i64>) -> Value {
    let all = d["allDay"] == true;
    let s_day = num(&d["sDay"]);
    let e_day = num(&d["eDay"]).max(s_day);
    let mut start = if all {
        date::day(s_day)
    } else {
        date::at_minutes(s_day, num(&d["sMin"]) as i32)
    };
    let mut end = if all {
        date::add_days(e_day, 1)
    } else {
        date::at_minutes(e_day, num(&d["eMin"]) as i32)
    };
    if end <= start {
        end = start + 3600000;
    }
    if recurring(e, new) {
        let len = end - start;
        start = series_start.unwrap_or(num(&e["start"])) + (start - num(&e["start"]));
        end = start + len;
    }
    let alarm = num(&d["alarm"]);
    let mut alarms = vec![];
    if alarm >= 0 {
        alarms.push(json!(alarm));
        alarms.extend(
            e["alarms"]
                .as_array()
                .into_iter()
                .flatten()
                .skip(1)
                .cloned(),
        );
    }
    let mut event = json!({"title":s(&d["title"]).trim(),"loc":s(&d["loc"]).trim(),"notes":d["notes"],"allDay":all,"start":start,"end":end,"rrule":d["rrule"],"alarms":alarms});
    if !new {
        event["uid"] = e["uid"].clone();
    }
    for k in ["url", "status"] {
        if !s(&e[k]).is_empty() {
            event[k] = e[k].clone();
        }
    }
    if all {
        event["sdate"] = json!(date::ymd(start));
        event["edate"] = json!(date::ymd(end));
    } else if !s(&e["tz"]).is_empty() {
        event["tz"] = e["tz"].clone();
    }
    if e["att"].as_array().is_some_and(|a| !a.is_empty()) {
        event["att"] = e["att"].clone();
        if e["org"].is_object() {
            event["orgEmail"] = e["org"]["email"].clone();
            event["orgName"] = e["org"]["name"].clone();
        }
    }
    event
}
pub fn save_args(e: &Value, d: &Value, new: bool, first: Option<i64>) -> Value {
    let mut args = json!({"cal":d["cal"],"event":build(e,d,new,first)});
    if !new && !s(&e["cal"]).is_empty() && e["cal"] != d["cal"] {
        args["from"] = e["cal"].clone();
    }
    args
}
pub fn participation(ps: &str) -> (&'static str, &'static str, &'static str) {
    match ps.to_uppercase().as_str() {
        "ACCEPTED" => ("✓", "#34c759", "accepted"),
        "DECLINED" => ("✕", "#ff3b30", "declined"),
        "TENTATIVE" => ("?", "#ff9500", "tentative"),
        "DELEGATED" => ("→", "#8e8e93", "delegated"),
        _ => ("•", "#8e8e93", "pending"),
    }
}
#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
