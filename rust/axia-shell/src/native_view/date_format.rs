//! Regional date/time captions shared by native applications. Match the SDK's
//! format catalogs and 12/24-hour override; the caller supplies local time.
use super::i18n::I18n;
use serde_json::Value;
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regional_formats_match_frozen_sdk_captions() {
        let cases: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/native_dates.json")).unwrap();
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
