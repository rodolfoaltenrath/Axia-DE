//! Native app packages (.lpk): read, install, list and remove. The C ABI of server/src/lpk.h,
//! which this replaced.
//!
//! An .lpk is a tar (ustar), gzip-compressed or not, with:
//!   manifest.json   id, name, version, permissions… (see docs/NATIVE-APPS.md)
//!   app.js          the already bundled app (tools/lpk.mjs), which uses React and the theme SDK
//!   icon.png        optional
//!   locales/<language>.json, *.wasm, images…   optional
//!
//! It is installed without root, in ~/.local/share/axia/apps/<id>/ (user-only). The rules: only
//! regular files and folders (no links or devices), relative paths with letters, digits, '.',
//! '-' and '_' (no "..", no components starting with '.'), at most LPK_MAX_FILES files and
//! LPK_MAX_TOTAL bytes. It is decompressed in memory with bounded codecs, written to a new
//! folder and swapped with the previous one via rename.
use std::ffi::{c_char, c_int, c_uchar, CStr, CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::{ptr, slice};

const LPK_MAX_FILES: usize = 2000;
const LPK_MAX_TOTAL: usize = 64 * 1024 * 1024; // decompressed content
const LPK_MAX_ARCHIVE: u64 = 96 * 1024 * 1024; // the compressed file
const PATH_MAX: usize = 4096;

/// Layout of the C `LpkFile`.
#[repr(C)]
pub struct LpkFile {
    path: [c_char; 224],
    data: *const c_uchar,
    size: usize,
}
/// Layout of the C `Lpk`.
#[repr(C)]
pub struct Lpk {
    raw: *mut c_uchar,
    files: *mut LpkFile,
    nfiles: c_int,
}

unsafe fn seterr(err: *mut c_char, n: usize, msg: &str) {
    if !err.is_null() && n > 0 {
        let k = msg.len().min(n - 1);
        ptr::copy_nonoverlapping(msg.as_ptr(), err.cast::<u8>(), k);
        *err.add(k) = 0;
    }
}

fn valid_id(id: &[u8]) -> bool {
    id.len() >= 2
        && id.len() <= 40
        && id[0].is_ascii_lowercase()
        && id.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn valid_path(rel: &[u8]) -> bool {
    if rel.is_empty() || rel.len() >= 224 || rel[0] == b'/' {
        return false;
    }
    let mut start = true;
    for &c in rel {
        if c == b'/' {
            if start {
                return false;
            }
            start = true;
            continue;
        }
        if start && c == b'.' {
            return false; // ".", "..", hidden
        }
        if !c.is_ascii_alphanumeric() && !b"._-".contains(&c) {
            return false;
        }
        start = false;
    }
    !start
}

/// Valid package id: [a-z][a-z0-9-]{1,39} (it is also its text namespace).
#[no_mangle]
pub unsafe extern "C" fn lpk_valid_id(id: *const c_char) -> c_int {
    (!id.is_null() && valid_id(CStr::from_ptr(id).to_bytes())) as c_int
}
/// Valid relative path inside a package.
#[no_mangle]
pub unsafe extern "C" fn lpk_valid_path(rel: *const c_char) -> c_int {
    (!rel.is_null() && valid_path(CStr::from_ptr(rel).to_bytes())) as c_int
}

fn octal(s: &[u8]) -> u64 {
    let mut v = 0u64;
    for &c in s {
        if c == 0 {
            break;
        }
        if c == b' ' {
            continue;
        }
        if !(b'0'..=b'7').contains(&c) {
            break;
        }
        v = v.wrapping_mul(8).wrapping_add((c - b'0') as u64);
    }
    v
}

/// The bytes of a tar header field up to its NUL (at most `max`).
fn field(h: &[u8], max: usize) -> &[u8] {
    let h = &h[..max.min(h.len())];
    &h[..h.iter().position(|&c| c == 0).unwrap_or(h.len())]
}

struct Tar {
    /// (path, offset of the data, size)
    files: Vec<(Vec<u8>, usize, usize)>,
}

fn parse_tar(buf: &[u8]) -> Result<Tar, &'static str> {
    let len = buf.len();
    let mut files: Vec<(Vec<u8>, usize, usize)> = Vec::new();
    let mut total = 0usize;
    let mut off = 0usize;
    while off + 512 <= len {
        let h = &buf[off..off + 512];
        if h.iter().all(|&b| b == 0) {
            break; // end of the tar
        }
        let sum = octal(&h[148..156]);
        let calc: u64 = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { b' ' as u64 } else { b as u64 }).sum();
        if sum != calc {
            return Err("lpk.corruptTar");
        }
        let size = octal(&h[124..136]);
        let typ = h[156];
        let mut name: Vec<u8> = if &h[257..262] == b"ustar" && h[345] != 0 {
            let mut n = field(&h[345..], 155).to_vec();
            n.push(b'/');
            n.extend_from_slice(field(h, 100));
            n
        } else {
            field(h, 100).to_vec()
        };
        name.truncate(259);
        let data = off + 512;
        let next = (data as u64).saturating_add(size.saturating_add(511) & !511);
        if next > len as u64 || size > LPK_MAX_TOTAL as u64 {
            return Err("lpk.corruptSize");
        }
        let size = size as usize;
        let next = next as usize;
        // (the name may hold an embedded NUL after joining: C treats it as a C string)
        if let Some(z) = name.iter().position(|&c| c == 0) {
            name.truncate(z);
        }
        while name.last() == Some(&b'/') {
            name.pop();
        }
        if name.len() > 2 && name.starts_with(b"./") {
            name.drain(..2);
        }
        if typ == b'5' || (typ == 0 && name.is_empty()) {
            off = next; // folders: created on their own
            continue;
        }
        if typ == b'x' || typ == b'g' {
            off = next; // pax headers: ignored
            continue;
        }
        if typ != b'0' && typ != 0 {
            return Err("lpk.specialFiles");
        }
        if !valid_path(&name) {
            return Err("lpk.badPath");
        }
        total += size;
        if files.len() >= LPK_MAX_FILES || total > LPK_MAX_TOTAL {
            return Err("lpk.tooLarge");
        }
        if files.iter().any(|f| f.0 == name) {
            return Err("lpk.duplicateFiles");
        }
        files.push((name, data, size));
        off = next;
    }
    if files.is_empty() {
        return Err("lpk.empty");
    }
    Ok(Tar { files })
}

/// Reads an .lpk (tar or tar.gz). Rejects links, devices, paths with "..", absolute ones or with
/// odd characters, and those that exceed the limits. `err` receives a text for the user.
#[no_mangle]
pub unsafe extern "C" fn lpk_open(path: *const c_char, out: *mut Lpk, err: *mut c_char, errlen: usize) -> c_int {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    if out.is_null() || path.is_null() {
        return -1;
    }
    ptr::write(out, Lpk { raw: ptr::null_mut(), files: ptr::null_mut(), nfiles: 0 });
    let read = || -> Option<Vec<u8>> {
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(OsStr::from_bytes(CStr::from_ptr(path).to_bytes()))
            .ok()?;
        let md = f.metadata().ok()?;
        if !md.is_file() || md.size() == 0 || md.size() > LPK_MAX_ARCHIVE {
            return None;
        }
        let mut v = Vec::with_capacity(md.size() as usize);
        f.read_to_end(&mut v).ok()?;
        (v.len() as u64 == md.size()).then_some(v)
    };
    let Some(mut buf) = read() else {
        seterr(err, errlen, "lpk.readFailed");
        return -1;
    };
    if buf.len() >= 2 && buf[0] == 0x1f && buf[1] == 0x8b {
        // gzip (RFC 1952) → raw bytes
        match crate::compression::decode(&buf, LPK_MAX_TOTAL + LPK_MAX_FILES * 1024, true) {
            Some(raw) => buf = raw,
            None => {
                seterr(err, errlen, "lpk.corruptGzip");
                return -1;
            }
        }
    }
    let tar = match parse_tar(&buf) {
        Ok(t) => t,
        Err(m) => {
            seterr(err, errlen, m);
            return -1;
        }
    };
    let raw = libc::malloc(buf.len().max(1)) as *mut c_uchar;
    let files = libc::calloc(LPK_MAX_FILES, std::mem::size_of::<LpkFile>()) as *mut LpkFile;
    if raw.is_null() || files.is_null() {
        libc::free(raw.cast());
        libc::free(files.cast());
        seterr(err, errlen, "sys.outOfMemory");
        return -1;
    }
    ptr::copy_nonoverlapping(buf.as_ptr(), raw, buf.len());
    for (i, (name, data, size)) in tar.files.iter().enumerate() {
        let f = &mut *files.add(i);
        ptr::copy_nonoverlapping(name.as_ptr(), f.path.as_mut_ptr().cast::<u8>(), name.len());
        f.path[name.len()] = 0;
        f.data = raw.add(*data);
        f.size = *size;
    }
    *out = Lpk { raw, files, nfiles: tar.files.len() as c_int };
    0
}

#[no_mangle]
pub unsafe extern "C" fn lpk_find(p: *const Lpk, path: *const c_char) -> *const LpkFile {
    if p.is_null() || path.is_null() || (*p).files.is_null() {
        return ptr::null();
    }
    let want = CStr::from_ptr(path);
    for i in 0..(*p).nfiles.max(0) as usize {
        let f = (*p).files.add(i);
        if CStr::from_ptr((*f).path.as_ptr()) == want {
            return f;
        }
    }
    ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn lpk_free(p: *mut Lpk) {
    if let Some(p) = p.as_mut() {
        libc::free(p.raw.cast());
        libc::free(p.files.cast());
        *p = Lpk { raw: ptr::null_mut(), files: ptr::null_mut(), nfiles: 0 };
    }
}

static mut ROOT: [c_char; PATH_MAX] = [0; PATH_MAX];

/// Installed apps folder: ~/.local/share/axia/apps (created if missing).
#[no_mangle]
pub unsafe extern "C" fn lpk_root() -> *const c_char {
    let Some(home) = std::env::var_os("HOME") else { return ptr::null() };
    let mut last = Vec::new();
    for part in ["/.local", "/.local/share", "/.local/share/axia", "/.local/share/axia/apps"] {
        let mut p = home.as_bytes().to_vec();
        p.extend_from_slice(part.as_bytes());
        let Ok(c) = CString::new(p.clone()) else { return ptr::null() };
        if libc::mkdir(c.as_ptr(), 0o755) != 0 && *libc::__errno_location() != libc::EEXIST {
            return ptr::null();
        }
        last = p;
    }
    if last.len() >= PATH_MAX {
        return ptr::null();
    }
    let root = ptr::addr_of_mut!(ROOT).cast::<c_char>();
    ptr::copy_nonoverlapping(last.as_ptr().cast::<c_char>(), root, last.len());
    *root.add(last.len()) = 0;
    root
}

/// Removes a file or a whole folder tree (symbolic links are removed, never followed).
fn rm_tree(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        Ok(md) if md.is_dir() => std::fs::remove_dir_all(path).is_ok(),
        Ok(_) => std::fs::remove_file(path).is_ok(),
    }
}

/// Creates the intermediate folders of `rel` inside `base`.
fn make_parents(base: &Path, rel: &[u8]) -> bool {
    let mut cur = base.to_path_buf();
    let parts: Vec<&[u8]> = rel.split(|&c| c == b'/').collect();
    for part in &parts[..parts.len() - 1] {
        cur.push(OsStr::from_bytes(part));
        let c = CString::new(cur.as_os_str().as_bytes()).unwrap();
        unsafe {
            if libc::mkdir(c.as_ptr(), 0o755) != 0 && *libc::__errno_location() != libc::EEXIST {
                return false;
            }
        }
        match std::fs::symlink_metadata(&cur) {
            Ok(md) if md.is_dir() => {}
            _ => return false,
        }
    }
    true
}

fn write_file(path: &Path, data: &[u8]) -> bool {
    let c = CString::new(path.as_os_str().as_bytes()).unwrap();
    unsafe {
        let fd = libc::open(
            c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o644,
        );
        if fd < 0 {
            return false;
        }
        let mut w = 0usize;
        while w < data.len() {
            let r = libc::write(fd, data.as_ptr().add(w).cast(), data.len() - w);
            if r < 0 && *libc::__errno_location() == libc::EINTR {
                continue;
            }
            if r <= 0 {
                break;
            }
            w += r as usize;
        }
        libc::close(fd) == 0 && w == data.len()
    }
}

/// Installs (or replaces) the package in lpk_root()/<id>, atomically.
#[no_mangle]
pub unsafe extern "C" fn lpk_install(p: *const Lpk, id: *const c_char, err: *mut c_char, errlen: usize) -> c_int {
    let root = lpk_root();
    if root.is_null() || p.is_null() || id.is_null() || !valid_id(CStr::from_ptr(id).to_bytes()) {
        seterr(err, errlen, "lpk.installHome");
        return -1;
    }
    let root = Path::new(OsStr::from_bytes(CStr::from_ptr(root).to_bytes())).to_path_buf();
    let id = OsStr::from_bytes(CStr::from_ptr(id).to_bytes());
    let dst = root.join(id);
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(id);
    let mut old_name = tmp_name.clone();
    tmp_name.push(".new");
    old_name.push(".old");
    let (tmp, old) = (root.join(tmp_name), root.join(old_name));
    rm_tree(&tmp);
    rm_tree(&old);
    let tmp_c = CString::new(tmp.as_os_str().as_bytes()).unwrap();
    if libc::mkdir(tmp_c.as_ptr(), 0o755) != 0 {
        seterr(err, errlen, "lpk.mkdirFailed");
        return -1;
    }
    for i in 0..(*p).nfiles.max(0) as usize {
        let f = &*(*p).files.add(i);
        let rel = CStr::from_ptr(f.path.as_ptr()).to_bytes();
        let path = tmp.join(OsStr::from_bytes(rel));
        let data: &[u8] = if f.size == 0 { &[] } else { slice::from_raw_parts(f.data, f.size) };
        if path.as_os_str().len() >= PATH_MAX || !make_parents(&tmp, rel) || !write_file(&path, data) {
            rm_tree(&tmp);
            seterr(err, errlen, "lpk.writeFailed");
            return -1;
        }
    }
    let had = std::fs::symlink_metadata(&dst).is_ok();
    if had && std::fs::rename(&dst, &old).is_err() {
        rm_tree(&tmp);
        seterr(err, errlen, "lpk.replaceFailed");
        return -1;
    }
    if std::fs::rename(&tmp, &dst).is_err() {
        if had {
            let _ = std::fs::rename(&old, &dst);
        }
        rm_tree(&tmp);
        seterr(err, errlen, "lpk.installFailed");
        return -1;
    }
    if had {
        rm_tree(&old);
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn lpk_remove(id: *const c_char) -> c_int {
    let root = lpk_root();
    if root.is_null() || id.is_null() || !valid_id(CStr::from_ptr(id).to_bytes()) {
        return -1;
    }
    let dst = Path::new(OsStr::from_bytes(CStr::from_ptr(root).to_bytes())).join(OsStr::from_bytes(CStr::from_ptr(id).to_bytes()));
    match std::fs::symlink_metadata(&dst) {
        Ok(md) if md.is_dir() => {
            if rm_tree(&dst) {
                0
            } else {
                -1
            }
        }
        _ => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, data) in files {
            let mut h = [0u8; 512];
            h[..name.len()].copy_from_slice(name.as_bytes());
            h[100..107].copy_from_slice(b"0000644");
            h[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
            h[156] = b'0';
            h[257..262].copy_from_slice(b"ustar");
            let sum: u32 = h.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u32 }).sum();
            h[148..155].copy_from_slice(format!("{:06o}\0", sum).as_bytes());
            h[155] = b' ';
            out.extend_from_slice(&h);
            out.extend_from_slice(data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0; 1024]);
        out
    }

    #[test]
    fn names_and_ids() {
        assert!(valid_id(b"clock") && valid_id(b"a-1") && !valid_id(b"A1") && !valid_id(b"a") && !valid_id(b"1a"));
        assert!(!valid_id(&[b'a'; 41]));
        for ok in ["app.js", "locales/en.json", "wasm/vida.wasm", "a_b-c.d"] {
            assert!(valid_path(ok.as_bytes()), "{ok}");
        }
        for bad in ["", "/etc/passwd", "../x", "a/../b", ".hidden", "a/.b", "a//b", "a/", "a b", "é"] {
            assert!(!valid_path(bad.as_bytes()), "{bad}");
        }
        assert!(!valid_path(&[b'a'; 224]));
    }

    #[test]
    fn reads_a_tar() {
        let t = tar(&[("manifest.json", b"{}"), ("./app.js", b"x = 1"), ("locales/es.json", b"")]);
        let r = parse_tar(&t).unwrap();
        let names: Vec<_> = r.files.iter().map(|f| String::from_utf8(f.0.clone()).unwrap()).collect();
        assert_eq!(names, ["manifest.json", "app.js", "locales/es.json"]);
        assert_eq!(&t[r.files[1].1..r.files[1].1 + r.files[1].2], b"x = 1");
    }

    #[test]
    fn rejects_bad_tars() {
        assert_eq!(parse_tar(&tar(&[("a/../b", b"x")])).err(), Some("lpk.badPath"));
        assert_eq!(parse_tar(&tar(&[("a", b"x"), ("a", b"y")])).err(), Some("lpk.duplicateFiles"));
        assert_eq!(parse_tar(&[0; 1024]).err(), Some("lpk.empty"));
        let mut t = tar(&[("a", b"x")]);
        t[0] ^= 1;
        assert_eq!(parse_tar(&t).err(), Some("lpk.corruptTar"));
        let mut t = tar(&[("a", b"x")]);
        t[156] = b'2'; // symbolic link
        // fix the checksum so only the type is wrong
        let sum: u32 = t[..512].iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u32 }).sum();
        t[148..155].copy_from_slice(format!("{:06o}\0", sum).as_bytes());
        assert_eq!(parse_tar(&t).err(), Some("lpk.specialFiles"));
        let t = tar(&[("a", &[7u8; 2000])]);
        assert_eq!(parse_tar(&t[..1024]).err(), Some("lpk.corruptSize"));
    }
}
