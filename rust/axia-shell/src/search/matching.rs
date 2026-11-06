use std::ffi::{c_char, c_void, CStr, CString};
#[link(name = "glib-2.0")]
extern "C" {
    fn g_utf8_normalize(s: *const c_char, n: isize, mode: i32) -> *mut c_char;
    fn g_free(p: *mut c_void);
}
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .split('\0')
        .map(|s| {
            let c = CString::new(s).unwrap();
            unsafe {
                let p = g_utf8_normalize(c.as_ptr(), s.len() as isize, 0);
                if p.is_null() {
                    return s.to_owned();
                }
                let s = CStr::from_ptr(p)
                    .to_string_lossy()
                    .chars()
                    .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
                    .collect::<String>();
                g_free(p.cast());
                s
            }
        })
        .collect::<Vec<_>>()
        .join("\0")
}
pub fn score(a: &str, q: &str, fuzzy: bool) -> f64 {
    if a.is_empty() || q.is_empty() {
        return 0.;
    }
    if a == q {
        return 100.;
    }
    if a.starts_with(q) {
        return 80.;
    }
    if a.match_indices(q).any(|(at, _)| {
        at == 0
            || a[..at].chars().next_back().is_some_and(|c| {
                (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}' || "._-/(".contains(c)
            })
    }) {
        return 60.;
    }
    if a.contains(q) {
        return 40.;
    }
    if !fuzzy || q.chars().count() < 2 {
        return 0.;
    }
    let mut chars = q.chars();
    let mut want = chars.next();
    for c in a.chars() {
        if Some(c) == want {
            want = chars.next();
            if want.is_none() {
                return 15.;
            }
        }
    }
    0.
}
pub fn best(q: &str, fields: &[String]) -> f64 {
    fields
        .iter()
        .enumerate()
        .map(|(i, s)| score(s, q, i == 0) * if i == 0 { 1. } else { 0.7 })
        .fold(0., f64::max)
}
pub fn trimmed(s: &str) -> &str {
    s.trim_matches(|c: char| c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}'))
}
