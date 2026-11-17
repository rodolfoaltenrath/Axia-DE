//! Local civil dates shared by native PIM windows. Calendar arithmetic uses
//! local calendar fields, so moving by one day is safe across DST changes.
//! Yearless birthdays use the leap year 2000.
pub fn timestamp(mut t: libc::tm) -> i64 {
    t.tm_isdst = -1;
    unsafe { libc::mktime(&mut t) as i64 * 1000 }
}
pub fn day(ms: i64) -> i64 {
    let mut t = crate::clock::local(ms);
    t.tm_hour = 0;
    t.tm_min = 0;
    t.tm_sec = 0;
    timestamp(t)
}
pub fn add_days(ms: i64, n: i32) -> i64 {
    let mut t = crate::clock::local(day(ms));
    t.tm_mday += n;
    timestamp(t)
}
pub fn at_minutes(ms: i64, minutes: i32) -> i64 {
    let mut t = crate::clock::local(day(ms));
    t.tm_hour = minutes / 60;
    t.tm_min = minutes % 60;
    timestamp(t)
}
pub fn minutes(ms: i64) -> i32 {
    let t = crate::clock::local(ms);
    t.tm_hour * 60 + t.tm_min
}
pub fn week(ms: i64) -> i64 {
    let d = day(ms);
    add_days(d, -((crate::clock::local(d).tm_wday + 6) % 7))
}
pub fn month(ms: i64, n: i32) -> i64 {
    let mut t = crate::clock::local(day(ms));
    t.tm_mday = 1;
    t.tm_mon += n;
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
pub fn parse(s: &str) -> Option<i64> {
    let full = if let Some(s) = s.strip_prefix("--") {
        match s.len() {
            4 => format!("2000-{}-{}", s.get(..2)?, s.get(2..)?),
            5 => format!("2000-{s}"),
            _ => return None,
        }
    } else {
        s.to_owned()
    };
    let b = full.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return None;
    }
    let (year, mon, day) = (
        full[..4].parse::<i32>().ok()?,
        full[5..7].parse::<i32>().ok()?,
        full[8..].parse::<i32>().ok()?,
    );
    let mut t: libc::tm = unsafe { std::mem::zeroed() };
    t.tm_year = year - 1900;
    t.tm_mon = mon - 1;
    t.tm_mday = day;
    let ms = timestamp(t);
    let checked = crate::clock::local(ms);
    (checked.tm_year == year - 1900 && checked.tm_mon == mon - 1 && checked.tm_mday == day)
        .then_some(ms)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn birthdays_preserve_yearless_leap_days_and_reject_invalid_dates() {
        assert_eq!(ymd(parse("--0229").unwrap()), "2000-02-29");
        assert_eq!(parse("--02-29"), parse("--0229"));
        assert_eq!(ymd(parse("1990-01-01").unwrap()), "1990-01-01");
        for bad in [
            "2025-02-29",
            "2026-13-01",
            "2026-00-01",
            "--0230",
            "--x229",
            "2026-1-01",
            "ééééé",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }
    #[test]
    fn month_navigation_uses_first_day_and_monday_grid() {
        let d = parse("2026-03-31").unwrap();
        assert_eq!(ymd(month(d, -1)), "2026-02-01");
        assert_eq!(ymd(month(d, 1)), "2026-04-01");
        let first = month(d, 0);
        let start = add_days(first, -((crate::clock::local(first).tm_wday + 6) % 7));
        assert_eq!(ymd(start), "2026-02-23");
    }
}
