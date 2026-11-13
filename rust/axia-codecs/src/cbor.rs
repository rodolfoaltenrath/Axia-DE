//! Small, strict CBOR (RFC 8949) for CTAP2: integers, bytes, text, arrays, maps, true/false/null.
//! No floats, tags or indefinite lengths. The C ABI of server/src/vault/cbor.h, which this replaced.
//!
//! Writer: over a fixed buffer; the caller writes the map keys already in CTAP2 canonical order.
//! Reader: builds an array of items in order (no copying), each with where its subtree ends; it
//! rejects non-minimal encodings, reserved simple values, invalid UTF-8 and nesting deeper than 16.
use std::ffi::{c_char, c_int, CStr};
use std::{ptr, slice};

#[repr(C)]
pub struct CbW {
    p: *mut u8,
    n: usize,
    cap: usize,
    err: c_int,
}

pub const CB_UINT: u8 = 1;
pub const CB_NINT: u8 = 2;
pub const CB_BYTES: u8 = 3;
pub const CB_TEXT: u8 = 4;
pub const CB_ARRAY: u8 = 5;
pub const CB_MAP: u8 = 6;
pub const CB_FALSE: u8 = 7;
pub const CB_TRUE: u8 = 8;
pub const CB_NULL: u8 = 9;
pub const CB_UNDEF: u8 = 10;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CbItem {
    /// One of the CB_ constants.
    r#type: u8,
    /// UINT: the value; NINT: the value is -1 - v; ARRAY and MAP: the count.
    v: u64,
    /// BYTES and TEXT: the data.
    ptr: *const u8,
    len: usize,
    /// Index of the item following this subtree.
    next: c_int,
}

#[repr(C)]
pub struct Cb {
    it: *const CbItem,
    n: c_int,
    used: usize,
}

// ================= writer =================

unsafe fn raw(w: &mut CbW, d: &[u8]) {
    if w.err != 0 || d.len() > w.cap - w.n {
        w.err = 1;
        return;
    }
    ptr::copy_nonoverlapping(d.as_ptr(), w.p.add(w.n), d.len());
    w.n += d.len();
}

unsafe fn head(w: &mut CbW, major: u8, v: u64) {
    let m = major << 5;
    let mut b = [0u8; 9];
    let n = if v < 24 {
        b[0] = m | v as u8;
        1
    } else if v <= 0xff {
        b[0] = m | 24;
        b[1] = v as u8;
        2
    } else if v <= 0xffff {
        b[0] = m | 25;
        b[1..3].copy_from_slice(&(v as u16).to_be_bytes());
        3
    } else if v <= 0xffff_ffff {
        b[0] = m | 26;
        b[1..5].copy_from_slice(&(v as u32).to_be_bytes());
        5
    } else {
        b[0] = m | 27;
        b[1..9].copy_from_slice(&v.to_be_bytes());
        9
    };
    raw(w, &b[..n]);
}

unsafe fn bytes_of<'a>(d: *const u8, n: usize) -> &'a [u8] {
    if n == 0 || d.is_null() {
        &[]
    } else {
        slice::from_raw_parts(d, n)
    }
}

/// # Safety
/// `w` is writable and `buf` covers `cap` bytes, which outlive the writer.
#[no_mangle]
pub unsafe extern "C" fn cbw_init(w: *mut CbW, buf: *mut u8, cap: usize) {
    *w = CbW { p: buf, n: 0, cap, err: 0 };
}
/// # Safety
/// `w` was initialized and `d` covers `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn cbw_raw(w: *mut CbW, d: *const u8, n: usize) {
    raw(&mut *w, bytes_of(d, n));
}
/// # Safety
/// `w` was initialized.
#[no_mangle]
pub unsafe extern "C" fn cbw_uint(w: *mut CbW, v: u64) {
    head(&mut *w, 0, v);
}
/// # Safety
/// `w` was initialized.
#[no_mangle]
pub unsafe extern "C" fn cbw_int(w: *mut CbW, v: i64) {
    if v >= 0 {
        head(&mut *w, 0, v as u64);
    } else {
        head(&mut *w, 1, (-1 - v) as u64);
    }
}
/// # Safety
/// `w` was initialized and `d` covers `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn cbw_bytes(w: *mut CbW, d: *const u8, n: usize) {
    head(&mut *w, 2, n as u64);
    raw(&mut *w, bytes_of(d, n));
}
/// # Safety
/// `w` was initialized and `s` covers `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn cbw_textn(w: *mut CbW, s: *const c_char, n: usize) {
    head(&mut *w, 3, n as u64);
    raw(&mut *w, bytes_of(s.cast(), n));
}
/// # Safety
/// `w` was initialized and `s` is a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn cbw_text(w: *mut CbW, s: *const c_char) {
    let text = CStr::from_ptr(s).to_bytes();
    cbw_textn(w, s, text.len());
}
/// # Safety
/// `w` was initialized.
#[no_mangle]
pub unsafe extern "C" fn cbw_array(w: *mut CbW, n: usize) {
    head(&mut *w, 4, n as u64);
}
/// # Safety
/// `w` was initialized.
#[no_mangle]
pub unsafe extern "C" fn cbw_map(w: *mut CbW, n: usize) {
    head(&mut *w, 5, n as u64);
}
/// # Safety
/// `w` was initialized.
#[no_mangle]
pub unsafe extern "C" fn cbw_bool(w: *mut CbW, v: c_int) {
    raw(&mut *w, &[if v != 0 { 0xf5 } else { 0xf4 }]);
}
/// # Safety
/// `w` was initialized.
#[no_mangle]
pub unsafe extern "C" fn cbw_null(w: *mut CbW) {
    raw(&mut *w, &[0xf6]);
}

// ================= reader =================

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
    it: &'a mut [CbItem],
    n: usize,
}

fn utf8_ok(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c < 0x80 {
            i += 1;
            continue;
        }
        let (k, mut cp) = match c {
            0xc2..=0xdf => (1, u32::from(c & 0x1f)),
            0xe0..=0xef => (2, u32::from(c & 0x0f)),
            0xf0..=0xf4 => (3, u32::from(c & 0x07)),
            _ => return false,
        };
        for j in 1..=k {
            if i + j >= s.len() || s[i + j] & 0xc0 != 0x80 {
                return false;
            }
            cp = cp << 6 | u32::from(s[i + j] & 0x3f);
        }
        if (k == 2 && cp < 0x800) || (k == 3 && !(0x10000..=0x10ffff).contains(&cp)) || (0xd800..0xe000).contains(&cp) {
            return false;
        }
        i += k + 1;
    }
    true
}

impl Reader<'_> {
    /// One item and its subtree; true when it is malformed.
    fn item(&mut self, depth: u32) -> bool {
        if depth > 16 || self.n >= self.it.len() || self.p >= self.d.len() {
            return true;
        }
        let ib = self.d[self.p];
        self.p += 1;
        let (major, ai) = (ib >> 5, ib & 31);
        let v: u64 = if ai < 24 {
            u64::from(ai)
        } else if ai <= 27 {
            let k = 1usize << (ai - 24);
            if k > self.d.len() - self.p {
                return true;
            }
            let v = self.d[self.p..self.p + k].iter().fold(0u64, |v, b| v << 8 | u64::from(*b));
            self.p += k;
            // minimal (canonical) encoding: if it fit in fewer bytes, it is invalid
            if (ai == 24 && v < 24) || (ai == 25 && v <= 0xff) || (ai == 26 && v <= 0xffff) || (ai == 27 && v <= 0xffff_ffff) {
                return true;
            }
            v
        } else {
            return true; // 28-30 reserved, 31 indefinite
        };
        let k = self.n;
        self.n += 1;
        let mut it = CbItem { r#type: 0, v: 0, ptr: ptr::null(), len: 0, next: 0 };
        match major {
            0 => {
                it.r#type = CB_UINT;
                it.v = v;
            }
            1 => {
                it.r#type = CB_NINT;
                it.v = v;
            }
            2 | 3 => {
                if v > (self.d.len() - self.p) as u64 {
                    return true;
                }
                it.r#type = if major == 2 { CB_BYTES } else { CB_TEXT };
                let d = self.d;
                let data = &d[self.p..self.p + v as usize];
                it.ptr = data.as_ptr();
                it.len = data.len();
                self.p += v as usize;
                if major == 3 && !utf8_ok(data) {
                    return true;
                }
            }
            4 | 5 => {
                it.r#type = if major == 4 { CB_ARRAY } else { CB_MAP };
                it.v = v;
                let sub = if major == 4 { v } else { v.wrapping_mul(2) };
                // each item takes at least one byte
                if v > self.it.len() as u64 || sub > (self.d.len() - self.p) as u64 {
                    return true;
                }
                self.it[k] = it; // the children may read the array of items
                for _ in 0..sub {
                    if self.item(depth + 1) {
                        return true;
                    }
                }
            }
            6 => return true, // tags: not in CTAP2
            _ => {
                if ai >= 24 {
                    return true; // floats and 1-byte simple values
                }
                it.r#type = match v {
                    20 => CB_FALSE,
                    21 => CB_TRUE,
                    22 => CB_NULL,
                    23 => CB_UNDEF,
                    _ => return true,
                };
            }
        }
        it.next = self.n as c_int;
        self.it[k] = it;
        false
    }
}

/// Reads one item from `d`. `items` is the caller's array (`max` of them). With `allow_trailing`
/// whatever follows is left over (`used` says how many bytes it took); otherwise it must end
/// exactly. 0 = ok.
///
/// # Safety
/// `c` is writable, `d` covers `len` bytes and `items` covers `max` items.
#[no_mangle]
pub unsafe extern "C" fn cb_parse(c: *mut Cb, d: *const u8, len: usize, items: *mut CbItem, max: c_int, allow_trailing: c_int) -> c_int {
    let c = &mut *c;
    c.it = items;
    c.n = 0;
    c.used = 0;
    if max <= 0 || items.is_null() {
        return -1;
    }
    let mut r = Reader { d: bytes_of(d, len), p: 0, it: slice::from_raw_parts_mut(items, max as usize), n: 0 };
    if r.item(0) || (allow_trailing == 0 && r.p != len) {
        return -1;
    }
    c.n = r.n as c_int;
    c.used = r.p;
    0
}

unsafe fn items<'a>(c: *const Cb) -> &'a [CbItem] {
    let c = &*c;
    if c.it.is_null() || c.n <= 0 {
        &[]
    } else {
        slice::from_raw_parts(c.it, c.n as usize)
    }
}

/// The type of item `i` (0 if there is none).
///
/// # Safety
/// `c` was filled by cb_parse.
#[no_mangle]
pub unsafe extern "C" fn cb_type(c: *const Cb, i: c_int) -> c_int {
    usize::try_from(i).ok().and_then(|i| items(c).get(i)).map_or(0, |it| c_int::from(it.r#type))
}

fn int_of(it: Option<&CbItem>) -> Option<i64> {
    let it = it?;
    match it.r#type {
        CB_UINT if it.v <= i64::MAX as u64 => Some(it.v as i64),
        CB_NINT if it.v <= i64::MAX as u64 => Some(-1 - it.v as i64),
        _ => None,
    }
}

/// An UINT or NINT that fits in an int64: 0 = ok.
///
/// # Safety
/// `c` was filled by cb_parse and `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cb_int(c: *const Cb, i: c_int, out: *mut i64) -> c_int {
    match int_of(usize::try_from(i).ok().and_then(|i| items(c).get(i))) {
        Some(v) => {
            *out = v;
            0
        }
        None => -1,
    }
}

fn text_eq(it: Option<&CbItem>, s: &[u8]) -> bool {
    it.is_some_and(|it| it.r#type == CB_TEXT && unsafe { bytes_of(it.ptr, it.len) } == s)
}

/// # Safety
/// `c` was filled by cb_parse and `s` is a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn cb_text_eq(c: *const Cb, i: c_int, s: *const c_char) -> c_int {
    c_int::from(text_eq(usize::try_from(i).ok().and_then(|i| items(c).get(i)), CStr::from_ptr(s).to_bytes()))
}

/// Walks the pairs of map `map`; `want` says if a key is the one, and the value's index comes back.
fn map_find(all: &[CbItem], map: c_int, want: impl Fn(&CbItem) -> bool) -> c_int {
    let Some(m) = usize::try_from(map).ok().and_then(|m| all.get(m)).filter(|m| m.r#type == CB_MAP) else { return -1 };
    let mut k = map as usize + 1;
    for _ in 0..m.v {
        let Some(key) = all.get(k) else { return -1 };
        if want(key) {
            return (k + 1) as c_int;
        }
        match all.get(k + 1) {
            Some(value) => k = value.next as usize,
            None => return -1,
        }
    }
    -1
}

/// The index of the value under integer key `key`, or -1.
///
/// # Safety
/// `c` was filled by cb_parse.
#[no_mangle]
pub unsafe extern "C" fn cb_map_int(c: *const Cb, map: c_int, key: i64) -> c_int {
    map_find(items(c), map, |k| int_of(Some(k)) == Some(key))
}

/// The index of the value under text key `key`, or -1.
///
/// # Safety
/// `c` was filled by cb_parse and `key` is a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn cb_map_text(c: *const Cb, map: c_int, key: *const c_char) -> c_int {
    let key = CStr::from_ptr(key).to_bytes();
    map_find(items(c), map, |k| text_eq(Some(k), key))
}

/// The k-th element of array `arr`, or -1.
///
/// # Safety
/// `c` was filled by cb_parse.
#[no_mangle]
pub unsafe extern "C" fn cb_at(c: *const Cb, arr: c_int, k: c_int) -> c_int {
    let all = items(c);
    let Some(a) = usize::try_from(arr).ok().and_then(|a| all.get(a)).filter(|a| a.r#type == CB_ARRAY) else { return -1 };
    if k < 0 || k as u64 >= a.v {
        return -1;
    }
    let mut i = arr as usize + 1;
    for _ in 0..k {
        match all.get(i) {
            Some(it) => i = it.next as usize,
            None => return -1,
        }
    }
    i as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(d: &[u8], max: usize, trailing: bool) -> Option<(Vec<CbItem>, Cb)> {
        let mut items = vec![CbItem { r#type: 0, v: 0, ptr: ptr::null(), len: 0, next: 0 }; max];
        let mut c = Cb { it: ptr::null(), n: 0, used: 0 };
        let rc = unsafe { cb_parse(&mut c, d.as_ptr(), d.len(), items.as_mut_ptr(), max as c_int, trailing as c_int) };
        (rc == 0).then_some((items, c))
    }

    #[test]
    fn the_writer_makes_the_shortest_head_and_stops_at_its_buffer() {
        let mut buf = [0u8; 64];
        let mut w = CbW { p: ptr::null_mut(), n: 0, cap: 0, err: 0 };
        unsafe {
            cbw_init(&mut w, buf.as_mut_ptr(), buf.len());
            cbw_uint(&mut w, 23);
            cbw_uint(&mut w, 24);
            cbw_uint(&mut w, 0x1_0000);
            cbw_int(&mut w, -1);
            cbw_int(&mut w, -25);
            cbw_text(&mut w, c"id".as_ptr());
            cbw_bool(&mut w, 1);
            cbw_null(&mut w);
        }
        assert_eq!(&buf[..w.n], &[0x17, 0x18, 0x18, 0x1a, 0, 1, 0, 0, 0x20, 0x38, 24, 0x62, b'i', b'd', 0xf5, 0xf6]);
        assert_eq!(w.err, 0);
        let mut small = [0u8; 2];
        unsafe {
            cbw_init(&mut w, small.as_mut_ptr(), 2);
            cbw_bytes(&mut w, [1u8, 2, 3].as_ptr(), 3);
        }
        assert_eq!((w.err, w.n), (1, 1), "the head fit, the content did not: the error latches");
        unsafe { cbw_null(&mut w) };
        assert_eq!(w.n, 1, "and nothing more is written");
    }
    #[test]
    fn a_map_is_read_in_place_and_looked_up_by_key() {
        // {1: "a", -2: [true, null], "k": h'0102'}
        let d = [0xa3, 0x01, 0x61, b'a', 0x21, 0x82, 0xf5, 0xf6, 0x61, b'k', 0x42, 1, 2];
        let (items, c) = parse(&d, 16, false).unwrap();
        assert_eq!(c.used, d.len());
        unsafe {
            assert_eq!(cb_type(&c, 0), i32::from(CB_MAP));
            assert_eq!(cb_type(&c, -1), 0);
            let one = cb_map_int(&c, 0, 1);
            assert_eq!(cb_text_eq(&c, one, c"a".as_ptr()), 1);
            let two = cb_map_int(&c, 0, -2);
            assert_eq!(cb_type(&c, cb_at(&c, two, 1)), i32::from(CB_NULL));
            assert_eq!(cb_at(&c, two, 2), -1);
            let k = cb_map_text(&c, 0, c"k".as_ptr());
            assert_eq!(items[k as usize].len, 2);
            assert_eq!(cb_map_int(&c, 0, 9), -1);
            let mut v = 0i64;
            assert_eq!(cb_int(&c, cb_map_int(&c, 0, 1), &mut v), -1, "a text value is no integer");
            assert_eq!((cb_int(&c, cb_map_int(&c, 0, 1) - 1, &mut v), v), (0, 1), "its key is");
        }
    }
    #[test]
    fn what_ctap2_forbids_is_refused() {
        for bad in [
            &[0x18, 0x05][..],            // overlong: 5 in two bytes
            &[0x19, 0x00, 0xff],          // overlong
            &[0x1f],                      // indefinite
            &[0xc0, 0x00],                // a tag
            &[0xf9, 0, 0],                // a float
            &[0xf8, 0x20],                // a 1-byte simple value
            &[0xf0],                      // simple 16
            &[0x62, 0xc3, 0x28],          // invalid UTF-8
            &[0x62, 0xed, 0xa0],          // a surrogate (cut)
            &[0x82, 0x01],                // an array that ends early
            &[0x42, 0x01],                // bytes that end early
            &[],
        ] {
            assert!(parse(bad, 8, false).is_none(), "{bad:x?}");
        }
        assert!(parse(&[0x01, 0x02], 8, false).is_none(), "trailing bytes");
        assert_eq!(parse(&[0x01, 0x02], 8, true).unwrap().1.used, 1);
        let mut deep = vec![0x81u8; 20];
        deep.push(0x00);
        assert!(parse(&deep, 64, false).is_none(), "nesting deeper than 16");
        assert!(parse(&[0x83, 1, 2, 3], 2, false).is_none(), "more items than the caller's array");
    }
}
