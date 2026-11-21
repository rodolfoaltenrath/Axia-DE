//! Regional date/time captions shared by native applications. Match the SDK's
//! format catalogs and 12/24-hour override; the caller supplies local time.
use super::i18n::I18n;
use serde_json::{Map, Value};
fn value(i18n: &mut I18n, lang: &str, key: &str) -> Value {
    i18n.entry(lang, "format", key)
        .or_else(|| i18n.entry(lang.split(['-', '_']).next().unwrap_or(lang), "format", key))
        .or_else(|| i18n.entry("en", "format", key))
        .unwrap_or(Value::Null)
}
pub fn date(i18n: &mut I18n, region: &str, t: &libc::tm, style: &str) -> String {
    let lang = i18n.lang.clone();
    let regional = if region.is_empty() {
        &lang
    } else {
        region.split('_').next().unwrap_or(&lang)
    };
    let names = value(i18n, &lang, "date");
    let base = value(i18n, regional, "date");
    let regions = value(i18n, regional, "regions");
    let rdate = if regions[region]["date"].is_object() {
        &regions[region]["date"]
    } else {
        &base
    };
    let preferred = if style == "numeric" || regional == lang {
        rdate[style].as_str()
    } else {
        None
    };
    let mut pattern = preferred
        .or_else(|| names[style].as_str())
        .unwrap_or("{dd}/{mm}/{yyyy}")
        .to_owned();
    for (key, v) in [
        ("day", t.tm_mday.to_string()),
        ("dd", format!("{:02}", t.tm_mday)),
        ("mm", format!("{:02}", t.tm_mon + 1)),
        ("yyyy", (t.tm_year + 1900).to_string()),
    ] {
        pattern = pattern.replace(&format!("{{{key}}}"), &v);
    }
    for (key, catalog, index) in [
        ("weekday", "weekdays", t.tm_wday),
        ("wd", "weekdaysShort", t.tm_wday),
        ("month", "months", t.tm_mon),
        ("mon", "monthsShort", t.tm_mon),
    ] {
        let names = value(i18n, &lang, catalog);
        let fallback = if key == "month" || key == "mon" {
            index + 1
        } else {
            index
        }
        .to_string();
        pattern = pattern.replace(
            &format!("{{{key}}}"),
            names[index.max(0) as usize].as_str().unwrap_or(&fallback),
        );
    }
    pattern
}
pub fn time(
    i18n: &mut I18n,
    region: &str,
    hour_override: &str,
    t: &libc::tm,
    seconds: bool,
) -> String {
    let lang = i18n.lang.clone();
    let regional = if region.is_empty() {
        &lang
    } else {
        region.split('_').next().unwrap_or(&lang)
    };
    let regions = value(i18n, regional, "regions");
    let hour12 = match hour_override {
        "12" => true,
        "24" => false,
        _ => regions[region]["hour12"]
            .as_bool()
            .unwrap_or_else(|| value(i18n, regional, "hour12").as_bool().unwrap_or(false)),
    };
    let tail = if seconds {
        format!(":{:02}", t.tm_sec)
    } else {
        String::new()
    };
    if !hour12 {
        return format!("{:02}:{:02}{tail}", t.tm_hour, t.tm_min);
    }
    let key = if t.tm_hour < 12 { "am" } else { "pm" };
    let source = if regional == lang { regional } else { &lang };
    let period = value(i18n, source, key)
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if t.tm_hour < 12 {
                "AM".into()
            } else {
                "PM".into()
            }
        });
    format!(
        "{}:{:02}{tail} {period}",
        if t.tm_hour % 12 == 0 {
            12
        } else {
            t.tm_hour % 12
        },
        t.tm_min
    )
}
// SDK compatibility resolves complete catalogs; existing native app helpers above
// retain their per-field fallback for partial language packs.
/// Resolved once per language/region/clock revision by the session owner.
pub struct Formats {
    names: Map<String, Value>,
    date: Map<String, Value>,
    hour12: bool,
    am: String,
    pm: String,
    pub decimal: String,
    pub group: String,
}
impl Formats {
    pub fn new(i18n: &mut I18n, region: &str, hours: &str) -> Self {
        let lang = i18n.lang.clone();
        let names = i18n.catalog(&lang, "format").cloned();
        let names = names
            .or_else(|| i18n.catalog("en", "format").cloned())
            .unwrap_or_default();
        let regional = if region.is_empty() {
            lang.as_str()
        } else {
            region.split('_').next().unwrap_or(&lang)
        };
        let mut base = i18n
            .catalog(regional, "format")
            .cloned()
            .unwrap_or_else(|| names.clone());
        if let Some(overrides) = base
            .get("regions")
            .and_then(|v| v.get(region))
            .and_then(Value::as_object)
            .cloned()
        {
            base.extend(overrides);
        }
        let own = regional == lang;
        let mut date = names
            .get("date")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(numeric) = base
            .get("date")
            .and_then(|v| v.get("numeric"))
            .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        {
            date.insert("numeric".into(), numeric.clone());
        }
        if own {
            if let Some(regional_date) = base.get("date").and_then(Value::as_object) {
                date.extend(regional_date.clone());
            }
        }
        let text = |values: &Map<String, Value>, key: &str, fallback: &str| {
            values
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback)
                .to_owned()
        };
        let period = |key: &str| {
            if own {
                text(&base, key, if key == "am" { "AM" } else { "PM" })
            } else {
                text(
                    &names,
                    key,
                    &text(&base, key, if key == "am" { "AM" } else { "PM" }),
                )
            }
        };
        Self {
            date,
            hour12: match hours {
                "12" => true,
                "24" => false,
                _ => base.get("hour12").and_then(Value::as_bool).unwrap_or(false),
            },
            am: period("am"),
            pm: period("pm"),
            decimal: text(&base, "decimal", "."),
            group: base
                .get("group")
                .and_then(Value::as_str)
                .unwrap_or(",")
                .into(),
            names,
        }
    }
    pub fn name(&self, month: bool, index: i32, short: bool) -> String {
        let key = match (month, short) {
            (true, true) => "monthsShort",
            (true, false) => "months",
            (false, true) => "weekdaysShort",
            (false, false) => "weekdays",
        };
        self.names
            .get(key)
            .and_then(Value::as_array)
            .and_then(|a| usize::try_from(index).ok().and_then(|n| a.get(n)))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| (i64::from(index) + i64::from(month)).to_string())
    }
    pub fn date(&self, t: &libc::tm, style: &str) -> String {
        let pattern = self
            .date
            .get(style)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("{dd}/{mm}/{yyyy}");
        // One pass: translated names containing braces are data, never another template.
        let vars: Map<String, Value> = [
            ("day", t.tm_mday.to_string()),
            ("dd", format!("{:02}", t.tm_mday)),
            ("mm", format!("{:02}", t.tm_mon + 1)),
            ("yyyy", (i64::from(t.tm_year) + 1900).to_string()),
            ("weekday", self.name(false, t.tm_wday, false)),
            ("wd", self.name(false, t.tm_wday, true)),
            ("month", self.name(true, t.tm_mon, false)),
            ("mon", self.name(true, t.tm_mon, true)),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), Value::from(v)))
        .collect();
        super::i18n::fill(pattern, &vars)
    }
    pub fn invalid_date(&self, style: &str) -> String {
        let pattern = self
            .date
            .get(style)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("{dd}/{mm}/{yyyy}");
        let vars = ["weekday", "wd", "day", "dd", "month", "mon", "mm", "yyyy"]
            .into_iter()
            .map(|k| (k.into(), Value::from("NaN")))
            .collect();
        super::i18n::fill(pattern, &vars)
    }
    pub fn invalid_time(&self, seconds: bool) -> String {
        let tail = if seconds { ":NaN" } else { "" };
        if self.hour12 {
            format!("12:NaN{tail} {}", self.pm)
        } else {
            format!("NaN:NaN{tail}")
        }
    }
    pub fn time(&self, t: &libc::tm, seconds: bool) -> String {
        let tail = if seconds {
            format!(":{:02}", t.tm_sec)
        } else {
            String::new()
        };
        if !self.hour12 {
            return format!("{:02}:{:02}{tail}", t.tm_hour, t.tm_min);
        }
        format!(
            "{}:{:02}{tail} {}",
            if t.tm_hour % 12 == 0 {
                12
            } else {
                t.tm_hour % 12
            },
            t.tm_min,
            if t.tm_hour < 12 { &self.am } else { &self.pm }
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regional_formats_match_frozen_sdk_captions() {
        let cases: Value =
            serde_json::from_str(include_str!("../tests/fixtures/native_dates.json")).unwrap();
        let mut i18n = I18n::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"), None);
        for c in cases.as_array().unwrap() {
            i18n.lang = c["lang"].as_str().unwrap().into();
            let region = c["region"].as_str().unwrap();
            let mut t: libc::tm = unsafe { std::mem::zeroed() };
            t.tm_year = 126;
            t.tm_mon = 10;
            t.tm_mday = 13;
            t.tm_wday = 5;
            t.tm_hour = c["hour"].as_i64().unwrap() as i32;
            t.tm_min = 7;
            t.tm_sec = 9;
            for style in ["short", "long", "numeric"] {
                assert_eq!(
                    date(&mut i18n, region, &t, style),
                    c[style].as_str().unwrap(),
                    "{c}"
                );
            }
            for seconds in [false, true] {
                assert_eq!(
                    time(
                        &mut i18n,
                        region,
                        c["override"].as_str().unwrap(),
                        &t,
                        seconds
                    ),
                    c[if seconds { "timeSeconds" } else { "time" }]
                        .as_str()
                        .unwrap(),
                    "{c}"
                );
            }
        }
    }
}
