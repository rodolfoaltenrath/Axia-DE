//! CSV reader (RFC 4180) for the password importers: quoted fields with "" inside, line breaks
//! inside a field, CRLF or LF, a UTF-8 BOM at the start. The C ABI of server/src/vault/csvp.h, which
//! this replaced. Fields are copied into the secrets arena (smem): a password CSV never goes
//! through the libc heap.
use std::ffi::{c_char, c_int, c_void, CStr};
use std::{ptr, slice};

unsafe extern "C" {
    fn sm_alloc(n: usize) -> *mut c_void;
    fn sm_realloc(p: *mut c_void, n: usize) -> *mut c_void;
    fn sm_free(p: *mut c_void);
}

#[repr(C)]
pub struct CsvRow {
    f: *mut *mut c_char,
    n: c_int,
    cap: c_int,
}
#[repr(C)]
pub struct Csv {
    s: *const c_char,
    len: usize,
    p: usize,
    bad: c_int,
}

/// # Safety
/// `c` is writable and `s` covers `len` bytes, which outlive the reader.
#[no_mangle]
pub unsafe extern "C" fn csv_init(c: *mut Csv, s: *const c_char, len: usize) {
    let bom = len >= 3 && slice::from_raw_parts(s.cast::<u8>(), 3) == b"\xef\xbb\xbf";
    *c = Csv { s, len, p: if bom { 3 } else { 0 }, bad: 0 };
}

/// # Safety
/// `row` is zeroed or was filled by csv_next.
#[no_mangle]
pub unsafe extern "C" fn csv_row_free(row: *mut CsvRow) {
    let row = &mut *row;
    for i in 0..row.n {
        sm_free((*row.f.add(i as usize)).cast());
    }
    sm_free(row.f.cast());
    *row = CsvRow { f: ptr::null_mut(), n: 0, cap: 0 };
}

/// Adds the field `s[a..b]` (with `""` read as one quote if `quoted`). True when it failed.
unsafe fn push(row: &mut CsvRow, s: &[u8], a: usize, b: usize, quoted: bool) -> bool {
    if row.n == row.cap {
        let nc = if row.cap != 0 { row.cap * 2 } else { 16 };
        if nc > 4096 {
            return true;
        }
        let f = sm_realloc(row.f.cast(), std::mem::size_of::<*mut c_char>() * nc as usize).cast::<*mut c_char>();
        if f.is_null() {
            return true;
        }
        row.f = f;
        row.cap = nc;
    }
    let d = sm_alloc(b - a + 1).cast::<u8>();
    if d.is_null() {
        return true;
    }
    let mut o = 0;
    let mut i = a;
    while i < b {
        *d.add(o) = s[i];
        o += 1;
        if quoted && s[i] == b'"' && i + 1 < b && s[i + 1] == b'"' {
            i += 1;
        }
        i += 1;
    }
    *d.add(o) = 0;
    *row.f.add(row.n as usize) = d.cast();
    row.n += 1;
    false
}

/// The next row into `row` (reused: frees the previous one). 1 = row read, 0 = end, -1 = unclosed
/// quote or out of memory. Empty rows are skipped.
///
/// # Safety
/// `c` was set by csv_init and `row` is zeroed or was filled by csv_next.
#[no_mangle]
pub unsafe extern "C" fn csv_next(c: *mut Csv, row: *mut CsvRow) -> c_int {
    csv_row_free(row);
    let (c, row) = (&mut *c, &mut *row);
    let s: &[u8] = if c.len == 0 { &[] } else { slice::from_raw_parts(c.s.cast(), c.len) };
    loop {
        if c.p >= c.len {
            return 0;
        }
        // empty row
        if s[c.p] == b'\n' {
            c.p += 1;
            continue;
        }
        if s[c.p] == b'\r' {
            c.p += 1;
            if c.p < c.len && s[c.p] == b'\n' {
                c.p += 1;
            }
            continue;
        }
        break;
    }
    loop {
        let mut p = c.p;
        if p < c.len && s[p] == b'"' {
            p += 1;
            let a = p;
            loop {
                if p >= c.len {
                    c.bad = 1;
                    csv_row_free(row);
                    c.p = c.len;
                    return -1;
                }
                if s[p] == b'"' {
                    if p + 1 < c.len && s[p + 1] == b'"' {
                        p += 2;
                        continue;
                    }
                    break;
                }
                p += 1;
            }
            if push(row, s, a, p, true) {
                csv_row_free(row);
                return -1;
            }
            p += 1;
            // garbage between the closing quote and the separator: ignored (like sloppy exporters)
            while p < c.len && s[p] != b',' && s[p] != b'\n' && s[p] != b'\r' {
                p += 1;
            }
        } else {
            let a = p;
            while p < c.len && s[p] != b',' && s[p] != b'\n' && s[p] != b'\r' {
                p += 1;
            }
            if push(row, s, a, p, false) {
                csv_row_free(row);
                return -1;
            }
        }
        if p < c.len && s[p] == b',' {
            c.p = p + 1;
            if c.p >= c.len {
                if push(row, s, 0, 0, false) {
                    csv_row_free(row);
                    return -1;
                }
                return 1;
            }
            continue;
        }
        if p < c.len && s[p] == b'\r' {
            p += 1;
        }
        if p < c.len && s[p] == b'\n' {
            p += 1;
        }
        c.p = p;
        return 1;
    }
}

fn ieq_trim(a: &[u8], b: &[u8]) -> bool {
    let is_blank = |c: &u8| *c == b' ' || *c == b'\t';
    let start = a.iter().take_while(|c| is_blank(c)).count();
    let a = &a[start..];
    let end = a.len() - a.iter().rev().take_while(|c| is_blank(c)).count();
    let a = &a[..end];
    a.eq_ignore_ascii_case(b)
}

/// The header column with that name (case-insensitive, ignoring surrounding spaces), or -1.
///
/// # Safety
/// `hdr` was filled by csv_next and `name` is a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn csv_col(hdr: *const CsvRow, name: *const c_char) -> c_int {
    let (hdr, name) = (&*hdr, CStr::from_ptr(name).to_bytes());
    for i in 0..hdr.n {
        if ieq_trim(CStr::from_ptr(*hdr.f.add(i as usize)).to_bytes(), name) {
            return i;
        }
    }
    -1
}

/// Field `i` of the row, or "" if it does not exist.
///
/// # Safety
/// `row` was filled by csv_next.
#[no_mangle]
pub unsafe extern "C" fn csv_get(row: *const CsvRow, i: c_int) -> *const c_char {
    let row = &*row;
    if i >= 0 && i < row.n {
        *row.f.add(i as usize)
    } else {
        c"".as_ptr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn rows(text: &[u8]) -> Vec<Vec<String>> {
        let mut c = Csv { s: ptr::null(), len: 0, p: 0, bad: 0 };
        let mut row = CsvRow { f: ptr::null_mut(), n: 0, cap: 0 };
        csv_init(&mut c, text.as_ptr().cast(), text.len());
        let mut out = Vec::new();
        loop {
            match csv_next(&mut c, &mut row) {
                1 => out.push((0..row.n).map(|i| CStr::from_ptr(csv_get(&row, i)).to_string_lossy().into_owned()).collect()),
                0 => break,
                _ => {
                    out.push(vec!["<error>".into()]);
                    break;
                }
            }
        }
        csv_row_free(&mut row);
        out
    }

    #[test]
    fn reads_quotes_line_breaks_bom_and_empty_rows() {
        unsafe {
            let r = rows(b"\xef\xbb\xbfname,pass\r\n\r\n\"a,b\",\"say \"\"hi\"\"\"\n\"two\nlines\",x\r\n,\n");
            assert_eq!(r[0], ["name", "pass"]);
            assert_eq!(r[1], ["a,b", "say \"hi\""]);
            assert_eq!(r[2], ["two\nlines", "x"]);
            assert_eq!(r[3], ["", ""], "an empty first field and the one after the comma");
        }
    }
    #[test]
    fn an_open_quote_is_an_error_and_a_trailing_comma_is_an_empty_field() {
        unsafe {
            assert_eq!(rows(b"a,\"never closed")[0], ["<error>"]);
            assert_eq!(rows(b"a,b,")[0], ["a", "b", ""]);
            assert_eq!(rows(b"\"q\"junk,z")[0], ["q", "z"], "garbage after a closing quote is ignored");
        }
    }
    #[test]
    fn header_columns_ignore_case_and_surrounding_spaces() {
        unsafe {
            let mut c = Csv { s: ptr::null(), len: 0, p: 0, bad: 0 };
            let mut row = CsvRow { f: ptr::null_mut(), n: 0, cap: 0 };
            let text = b" URL ,\tUser Name\t,Password";
            csv_init(&mut c, text.as_ptr().cast(), text.len());
            assert_eq!(csv_next(&mut c, &mut row), 1);
            assert_eq!((csv_col(&row, c"url".as_ptr()), csv_col(&row, c"USER name".as_ptr()), csv_col(&row, c"password".as_ptr())), (0, 1, 2));
            assert_eq!(csv_col(&row, c"nope".as_ptr()), -1);
            assert_eq!(CStr::from_ptr(csv_get(&row, 9)).to_bytes(), b"");
            csv_row_free(&mut row);
        }
    }
}
