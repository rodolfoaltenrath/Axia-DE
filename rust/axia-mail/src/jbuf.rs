//! The C `JBuf` JSON builder (`server/src/doc/wire.h`), driven from Rust with
//! the growth rules and limit of `wire.c`, so buffers stay interchangeable
//! with the C `jb_*` functions and are released with `jb_free`/`free`.

use core::ffi::{c_char, c_int};

/// `DOC_JSON_MAX` (server/src/doc/proto.h) plus the builder's slack.
const JB_LIMIT: usize = 16 * 1024 * 1024 + 1024;

#[repr(C)]
pub struct JBuf {
    pub p: *mut c_char,
    pub n: usize,
    pub cap: usize,
    pub oom: c_int,
}

impl JBuf {
    pub(crate) fn new() -> JBuf {
        JBuf {
            p: std::ptr::null_mut(),
            n: 0,
            cap: 0,
            oom: 0,
        }
    }

    fn grow(&mut self, extra: usize) -> bool {
        if self.oom != 0 {
            return false;
        }
        if self.n + extra < self.cap {
            return true;
        }
        let mut cap = if self.cap != 0 { self.cap } else { 256 };
        while cap < self.n + extra + 1 {
            cap *= 2;
        }
        if cap > JB_LIMIT {
            self.oom = 1;
            return false;
        }
        let p = unsafe { libc::realloc(self.p.cast(), cap) }.cast::<c_char>();
        if p.is_null() {
            self.oom = 1;
            return false;
        }
        self.p = p;
        self.cap = cap;
        true
    }

    /// `jb_put`.
    pub(crate) fn put(&mut self, s: &[u8]) {
        if !self.grow(s.len()) {
            return;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), self.p.add(self.n).cast(), s.len());
            self.n += s.len();
            *self.p.add(self.n) = 0;
        }
    }

    /// `jb_raw` (stops at a NUL like strlen).
    pub(crate) fn raw(&mut self, s: &[u8]) {
        self.put(crate::c::z(s))
    }

    /// `jb_printf`.
    pub(crate) fn fmt(&mut self, a: core::fmt::Arguments) {
        match a.as_str() {
            Some(s) => self.put(s.as_bytes()),
            None => self.put(a.to_string().as_bytes()),
        }
    }

    /// `jb_strn`: a JSON string, invalid UTF-8 as U+FFFD.
    pub(crate) fn strn(&mut self, s: &[u8]) {
        self.put(b"\"");
        let mut i = 0;
        while i < s.len() {
            let ch = s[i];
            if ch == b'"' || ch == b'\\' {
                self.put(&[b'\\', ch]);
                i += 1;
                continue;
            }
            if ch < 0x20 {
                match ch {
                    b'\n' => self.put(b"\\n"),
                    b'\t' => self.put(b"\\t"),
                    b'\r' => self.put(b"\\r"),
                    _ => self.fmt(format_args!("\\u{:04x}", ch)),
                }
                i += 1;
                continue;
            }
            let k = wire_utf8_len(&s[i..]);
            if k == 0 {
                self.put(b"\xef\xbf\xbd");
                i += 1;
                continue;
            }
            self.put(&s[i..i + k]);
            i += k;
        }
        self.put(b"\"");
    }

    /// `jb_str` (C-string semantics).
    pub(crate) fn str(&mut self, s: &[u8]) {
        self.strn(crate::c::z(s))
    }

    /// The text written so far (empty if nothing was).
    pub(crate) fn as_bytes(&self) -> &[u8] {
        if self.p.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.p.cast(), self.n) }
        }
    }

    /// `jb_free`.
    pub(crate) fn free(&mut self) {
        unsafe { libc::free(self.p.cast()) };
        *self = JBuf::new();
    }
}

/// wire.c's UTF-8 check (overlongs and surrogates rejected by lead/second byte).
fn wire_utf8_len(s: &[u8]) -> usize {
    let c = s[0];
    if c < 0x80 {
        return 1;
    }
    let k = if (0xf0..=0xf4).contains(&c) {
        4
    } else if c >= 0xe0 {
        3
    } else if (0xc2..0xe0).contains(&c) {
        2
    } else {
        0
    };
    // Like wire.c, 0xf5..=0xff fall into the 3-byte branch.
    if k == 0 || k > s.len() {
        return 0;
    }
    if s[1..k].iter().any(|&d| d & 0xc0 != 0x80) {
        return 0;
    }
    if k == 3 && c == 0xe0 && s[1] < 0xa0 {
        return 0;
    }
    if k == 3 && c == 0xed && s[1] >= 0xa0 {
        return 0;
    }
    if k == 4 && c == 0xf0 && s[1] < 0x90 {
        return 0;
    }
    if k == 4 && c == 0xf4 && s[1] >= 0x90 {
        return 0;
    }
    k
}

/// The `JBuf` behind a C pointer (None for NULL).
pub(crate) unsafe fn from_ptr<'a>(b: *mut JBuf) -> Option<&'a mut JBuf> {
    unsafe { b.as_mut() }
}
