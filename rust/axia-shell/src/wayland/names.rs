//! The names a client gives its windows (title, app id), which go on to the shell inside JSON.
//! Clients send whatever bytes they like, and C buffers cut by byte: either one left half a
//! character behind, the shell refused the whole message, and a title (or the news that a window
//! had opened) was lost.
use std::ffi::{c_char, CStr};

/// Writes `text` and a NUL into `out`, cut between pieces: `pieces` are never split.
fn write<'a>(out: &mut [u8], pieces: impl Iterator<Item = std::borrow::Cow<'a, str>>) {
    let Some(room) = out.len().checked_sub(1) else { return };
    let mut used = 0;
    for piece in pieces {
        if used + piece.len() > room {
            break;
        }
        out[used..used + piece.len()].copy_from_slice(piece.as_bytes());
        used += piece.len();
    }
    out[used] = 0;
}
/// The characters of `bytes`; what is not UTF-8 becomes U+FFFD.
fn characters(bytes: &[u8]) -> impl Iterator<Item = char> + '_ {
    bytes.utf8_chunks().flat_map(|chunk| chunk.valid().chars().chain((!chunk.invalid().is_empty()).then_some('\u{fffd}')))
}
fn copy(out: &mut [u8], text: &[u8]) {
    write(out, characters(text).map(|c| c.to_string().into()));
}
/// The inside of a JSON string.
fn escape(out: &mut [u8], text: &[u8]) {
    write(
        out,
        characters(text).map(|c| match c {
            '"' => "\\\"".into(),
            '\\' => "\\\\".into(),
            c if c < ' ' => format!("\\u{:04x}", u32::from(c)).into(),
            c => c.to_string().into(),
        }),
    );
}

unsafe fn buffers<'a>(out: *mut c_char, size: usize, text: *const c_char) -> Option<(&'a mut [u8], &'a [u8])> {
    if out.is_null() || size == 0 {
        return None;
    }
    let text = if text.is_null() { &[][..] } else { CStr::from_ptr(text).to_bytes() };
    Some((std::slice::from_raw_parts_mut(out.cast(), size), text))
}
/// # Safety
/// `out` has `size` writable bytes that do not overlap `text`, a NUL-terminated string or null.
#[no_mangle]
pub unsafe extern "C" fn axia_wl_copy_name(out: *mut c_char, size: usize, text: *const c_char) {
    if let Some((out, text)) = buffers(out, size, text) {
        copy(out, text);
    }
}
/// # Safety
/// As `axia_wl_copy_name`.
#[no_mangle]
pub unsafe extern "C" fn axia_wl_json_name(out: *mut c_char, size: usize, text: *const c_char) {
    if let Some((out, text)) = buffers(out, size, text) {
        escape(out, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(f: fn(&mut [u8], &[u8]), size: usize, text: &[u8]) -> String {
        let mut out = vec![0xaa; size];
        f(&mut out, text);
        CStr::from_bytes_until_nul(&out).unwrap().to_str().expect("valid UTF-8").to_owned()
    }
    #[test]
    fn names_are_cut_between_characters_and_invalid_bytes_are_replaced() {
        for size in 1..20 {
            let cut = run(copy, size, "ação 😀 fim".as_bytes());
            assert!(cut.len() < size && "ação 😀 fim".starts_with(&cut), "{size}: {cut}");
        }
        assert_eq!(run(copy, 64, b"caf\xe9 latin-1"), "caf\u{fffd} latin-1");
        assert_eq!(run(copy, 64, b"half \xc3"), "half \u{fffd}");
        assert_eq!(run(copy, 1, b"x"), "");
    }
    #[test]
    fn json_names_always_parse_whatever_their_length() {
        let hostile = "Relatório \"final\" \\ linha\n\u{1}fim 😀".repeat(20);
        for size in 1..200 {
            let json = format!("\"{}\"", run(escape, size, hostile.as_bytes()));
            let parsed: String = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{size}: {e}: {json}"));
            assert!(hostile.starts_with(&parsed));
        }
    }
}
