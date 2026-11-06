//! Unicode preparation belongs to the catalog, using GLib already linked by
//! the desktop. No JavaScript sorting/search keys or additional crate needed.
use std::ffi::{c_char, c_void, CStr, CString};

#[link(name = "glib-2.0")]
extern "C" {
    fn g_utf8_normalize(text: *const c_char, length: isize, mode: i32) -> *mut c_char;
    fn g_free(memory: *mut c_void);
}

pub(super) fn nfc(text: &str) -> String {
    // GLib strings terminate at NUL even with an explicit length. Preserve
    // embedded NULs as boundaries, including in ABI/metadata regression tests.
    text.split('\0')
        .map(|part| {
            let input = CString::new(part).unwrap();
            unsafe {
                let result = g_utf8_normalize(input.as_ptr(), part.len() as isize, 1); // G_NORMALIZE_NFC
                if result.is_null() {
                    return part.to_owned();
                }
                let value = CStr::from_ptr(result).to_string_lossy().into_owned();
                g_free(result.cast());
                value
            }
        })
        .collect::<Vec<_>>()
        .join("\0")
}

pub(super) fn query(text: &str) -> String {
    // ECMAScript TrimString whitespace (Rust additionally treats NEL as space).
    text.trim_matches(|c| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
        )
    })
    .to_lowercase()
}
