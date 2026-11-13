//! pci.ids / usb.ids lookups (hwdata). The format: a vendor line "vvvv  Name" at the
//! start of a line, its devices on the lines below as "\tdddd  Name" (subsystems
//! "\t\t…" are skipped), and, in pci.ids, classes as "C cc  Name" with subclasses
//! "\tss  Name". Comments start with '#'. The file is read once into memory (at most
//! 8 MB) and searched in place. C ABI of server/src/sysreport/ids.h.
use crate::lanparse::cstr;
use std::ffi::{c_char, c_int};
use std::io::Read;
use std::os::unix::fs::MetadataExt;

const IDS_MAX: u64 = 8 << 20;

#[repr(C)]
pub struct IdsDb {
    text: *mut c_char,
    len: usize,
}

/// The name after "<hex>  " up to the end of the line, printable ASCII only, cut to
/// size - 1. Like the C, the copied bytes are kept and the trimmed length is where the
/// NUL goes: (bytes written, length).
fn copy_name(s: &[u8], size: usize) -> (Vec<u8>, usize) {
    let start = s.iter().position(|&c| c != b' ').unwrap_or(s.len());
    let out: Vec<u8> = s[start..]
        .iter()
        .take_while(|&&c| c != b'\n')
        .take(size - 1)
        .map(|&c| if (0x20..0x7f).contains(&c) { c } else { b'?' })
        .collect();
    let mut n = out.len();
    while n > 0 && out[n - 1] == b' ' {
        n -= 1;
    }
    (out, n)
}

type Name = (Vec<u8>, usize);

/// The line starts (after `tabs` tabs) with the hex id followed by a space: the rest after the id.
fn id_line<'a>(line: &'a [u8], tabs: usize, id: &[u8]) -> Option<&'a [u8]> {
    if line.len() < tabs || line[..tabs].iter().any(|&c| c != b'\t') {
        return None;
    }
    let s = &line[tabs..];
    if s.first() == Some(&b'\t') || s.len() <= id.len() {
        return None;
    }
    if !s[..id.len()].eq_ignore_ascii_case(id) || s[id.len()] != b' ' {
        return None;
    }
    Some(&s[id.len()..])
}

fn hexlen(s: &[u8]) -> usize {
    if s.iter().all(u8::is_ascii_hexdigit) { s.len() } else { 0 }
}

/// Lines of the text with what comes right after each one (C reads p[1] past a short line).
fn lines(text: &[u8]) -> impl Iterator<Item = (&[u8], u8)> {
    let mut p = 0;
    std::iter::from_fn(move || {
        if p >= text.len() {
            return None;
        }
        let e = text[p..].iter().position(|&c| c == b'\n').map_or(text.len(), |k| p + k);
        let line = &text[p..e];
        let second = text.get(p + 1).copied().unwrap_or(0);
        p = e + 1;
        Some((line, second))
    })
}

pub fn lookup(text: &[u8], vendor: &[u8], device: Option<&[u8]>, vsize: usize, dsize: usize) -> (Name, Name) {
    let (mut vname, mut dname) = ((Vec::new(), 0), (Vec::new(), 0));
    let dl = device.map_or(0, hexlen);
    if hexlen(vendor) != 4 {
        return (vname, dname);
    }
    let device = device.unwrap_or(b"");
    let mut in_vendor = false;
    for (line, second) in lines(text) {
        let first = line.first().copied();
        if first.is_some_and(|c| c != b'#' && c != b'\t') {
            if in_vendor {
                break; // past the vendor's devices
            }
            if first == Some(b'C') && second == b' ' {
                break; // the class section follows the vendors
            }
            if let Some(name) = id_line(line, 0, vendor) {
                vname = copy_name(name, vsize);
                if dl != 4 {
                    break;
                }
                in_vendor = true;
            }
        } else if in_vendor {
            if let Some(name) = id_line(line, 1, device) {
                dname = copy_name(name, dsize);
                break;
            }
        }
    }
    (vname, dname)
}

pub fn pci_class(text: &[u8], cls: &[u8], csize: usize, ssize: usize) -> (Name, Name) {
    let (mut cname, mut sname) = ((Vec::new(), 0), (Vec::new(), 0));
    if hexlen(cls) < 4 {
        return (cname, sname);
    }
    let mut in_class = false;
    for (line, _) in lines(text) {
        if line.len() > 2 && line[0] == b'C' && line[1] == b' ' {
            if in_class {
                break;
            }
            if let Some(name) = id_line(&line[2..], 0, &cls[..2]) {
                cname = copy_name(name, csize);
                in_class = true;
            }
        } else if in_class && line.first().is_some_and(|&c| c != b'#' && c != b'\t') {
            break;
        } else if in_class {
            if let Some(name) = id_line(line, 1, &cls[2..4]) {
                sname = copy_name(name, ssize);
                break;
            }
        }
    }
    (cname, sname)
}

unsafe fn text<'a>(db: *const IdsDb) -> Option<&'a [u8]> {
    let db = db.as_ref()?;
    (!db.text.is_null()).then(|| std::slice::from_raw_parts(db.text.cast::<u8>(), db.len))
}

unsafe fn out(p: *mut c_char, size: usize, (s, n): &Name) {
    if !p.is_null() && size > 0 {
        std::ptr::copy_nonoverlapping(s.as_ptr(), p.cast::<u8>(), s.len());
        *p.add(*n) = 0;
    }
}

const EMPTY: Name = (Vec::new(), 0);

/// # Safety
/// `db` points to an IdsDb and `path` is a C string.
#[no_mangle]
pub unsafe extern "C" fn ids_open(db: *mut IdsDb, path: *const c_char) -> c_int {
    let db = &mut *db;
    db.text = std::ptr::null_mut();
    db.len = 0;
    let path = std::ffi::OsStr::from_encoded_bytes_unchecked(cstr(path));
    let Ok(mut f) = std::fs::File::open(path) else { return -1 };
    let Ok(meta) = f.metadata() else { return -1 };
    if !meta.is_file() || meta.size() == 0 || meta.size() > IDS_MAX {
        return -1;
    }
    let mut data = Vec::with_capacity(meta.size() as usize);
    let _ = (&mut f).take(meta.size()).read_to_end(&mut data);
    if data.is_empty() {
        return -1;
    }
    let p = libc::malloc(data.len() + 1).cast::<u8>();
    if p.is_null() {
        return -1;
    }
    std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
    *p.add(data.len()) = 0;
    db.text = p.cast();
    db.len = data.len();
    0
}

/// # Safety
/// `db` was filled by ids_open (or is empty).
#[no_mangle]
pub unsafe extern "C" fn ids_close(db: *mut IdsDb) {
    let db = &mut *db;
    libc::free(db.text.cast());
    db.text = std::ptr::null_mut();
    db.len = 0;
}

/// # Safety
/// `db` is valid; the ids are NULL or C strings; outs are NULL or have room for their sizes.
#[no_mangle]
pub unsafe extern "C" fn ids_lookup(
    db: *const IdsDb,
    vendor: *const c_char,
    device: *const c_char,
    vname: *mut c_char,
    vsize: usize,
    dname: *mut c_char,
    dsize: usize,
) {
    out(vname, vsize, &EMPTY);
    out(dname, dsize, &EMPTY);
    let Some(t) = text(db) else { return };
    let dev = (!device.is_null()).then(|| cstr(device));
    let (v, d) = lookup(t, cstr(vendor), dev, vsize.max(1), dsize.max(1));
    out(vname, vsize, &v);
    out(dname, dsize, &d);
}

/// # Safety
/// As ids_lookup.
#[no_mangle]
pub unsafe extern "C" fn ids_pci_class(
    db: *const IdsDb,
    cls: *const c_char,
    cname: *mut c_char,
    csize: usize,
    sname: *mut c_char,
    ssize: usize,
) {
    out(cname, csize, &EMPTY);
    out(sname, ssize, &EMPTY);
    let Some(t) = text(db) else { return };
    let (c, s) = pci_class(t, cstr(cls), csize.max(1), ssize.max(1));
    out(cname, csize, &c);
    out(sname, ssize, &s);
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &[u8] = b"# comment\n10de  NVIDIA Corporation\n\t2684  AD102 [GeForce RTX 4090]\n\t\t10de 167c  sub\n8086  Intel\n\t46a6  Alder Lake\nC 03  Display controller\n\t00  VGA compatible controller\n\t02  3D controller\n";

    #[test]
    fn lookups() {
        let s = |(v, n): Name| v[..n].to_vec();
        let (v, d) = lookup(T, b"10DE", Some(b"2684"), 64, 64);
        assert_eq!((s(v), s(d)), (b"NVIDIA Corporation".to_vec(), b"AD102 [GeForce RTX 4090]".to_vec()));
        assert_eq!(s(lookup(T, b"10de", Some(b"46a6"), 64, 64).1), b"");
        assert_eq!(s(lookup(T, b"8086", None, 6, 64).0), b"Intel");
        assert_eq!(s(lookup(T, b"8086", None, 4, 64).0), b"Int");
        let (c, sub) = pci_class(T, b"030200", 64, 64);
        assert_eq!((s(c), s(sub)), (b"Display controller".to_vec(), b"3D controller".to_vec()));
        assert_eq!(s(pci_class(T, b"03x0", 64, 64).0), b"");
    }
}
