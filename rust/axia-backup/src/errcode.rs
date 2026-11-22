//! Stable, translatable error codes (server/src/errcode.h): "code key=value key=value".
//! The UI translates `errors.<code>`; nothing that reaches it is prose.

/// The code line for `code` and its parameters, as `lerr_set` writes it into a buffer of `cap`
/// bytes (the NUL included): a parameter that does not fit is dropped, the code always stays.
pub fn lerr(cap: usize, code: &str, params: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    if cap == 0 {
        return out;
    }
    out.extend_from_slice(code.as_bytes());
    if out.len() >= cap {
        out.truncate(cap - 1);
        return out;
    }
    for (key, value) in params {
        let mut piece = Vec::with_capacity(key.len() + value.len() + 2);
        piece.push(b' ');
        piece.extend_from_slice(key.as_bytes());
        piece.push(b'=');
        for &ch in *value {
            if ch < 0x20 || ch == 0x7f || ch == b'%' || ch == b' ' || ch == b'=' || ch == b'"' {
                piece.push(b'%');
                piece.extend_from_slice(format!("{ch:02X}").as_bytes());
            } else {
                piece.push(ch);
            }
        }
        if out.len() + piece.len() < cap {
            out.extend_from_slice(&piece);
        }
    }
    out
}

/// A code with no parameters, in the usual 256-byte error buffer.
pub fn code(code: &str) -> Vec<u8> {
    lerr(256, code, &[])
}

extern "C" {
    fn strerrorname_np(errnum: libc::c_int) -> *const libc::c_char;
}

/// errno as a parameter value ("ENOENT"), or its number when glibc has no name for it.
pub fn errno_name(e: i32) -> Vec<u8> {
    let name = unsafe { strerrorname_np(e) };
    if name.is_null() {
        return e.to_string().into_bytes();
    }
    unsafe { std::ffi::CStr::from_ptr(name) }.to_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_are_escaped_and_dropped_when_they_do_not_fit() {
        assert_eq!(lerr(256, "backup.badPath", &[]), b"backup.badPath");
        assert_eq!(
            lerr(256, "backup.createFailed", &[("path", b"/a b=c%\"\n"), ("errno", b"ENOENT")]),
            b"backup.createFailed path=/a%20b%3Dc%25%22%0A errno=ENOENT"
        );
        // 16 bytes with the NUL: "backup.x" (8) + " k=" (3) + 4 value bytes = 15 fits, 5 does not
        assert_eq!(lerr(16, "backup.x", &[("k", b"abcd")]), b"backup.x k=abcd");
        assert_eq!(lerr(16, "backup.x", &[("k", b"abcde")]), b"backup.x");
        // the code itself is cut to the buffer
        assert_eq!(lerr(6, "backup.badPath", &[]), b"backu");
        assert_eq!(errno_name(libc::ENOENT), b"ENOENT");
    }
}
