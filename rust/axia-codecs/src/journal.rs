//! Our own reader for systemd journal files (format from systemd/src/libsystemd/sd-journal/
//! journal-def.h, versions 187 to 257): the C ABI of server/src/logs/journal.h, which this
//! replaced. Used by the Logs app (lord.logs, as the user: it reads only the files that the
//! kernel and journald's ACLs let it read) and by the helper's logs-system action.
//!
//! The format, in short (all little-endian, objects aligned to 8):
//!   "LPKSHHRH" header with incompatible flags (XZ=1, LZ4=2, KEYED_HASH=4, ZSTD=8, COMPACT=16),
//!   n_entries and entry_array_offset;
//!   ENTRY_ARRAY: [header 16][next array 8][entry offsets 8 (or 4 in compact mode)];
//!   ENTRY: [header 16][seqnum][realtime][monotonic][boot_id 16][xor_hash] + items: offset of the
//!          DATA (+ hash) — 16 bytes per item, or 4 in compact mode;
//!   DATA:  [header 16][hash][next_hash][next_field][entry][entry_array][n_entries]
//!          (+ tail_entry_array_offset and n in compact mode) and the "FIELD=value" payload,
//!          possibly compressed (XZ, LZ4 with the size in the first 8 bytes, or ZSTD).
//!
//! Safety: never mmap (if journald shrinks the file, mmap would raise SIGBUS): pread with a small
//! block cache. Every offset is checked against the file size, the array chain must always move
//! forward (no loops) and sizes are capped. ZSTD has no decoder of our own yet: those fields are
//! shown as «[zstd-compressed]».
use std::ffi::{c_char, c_int, c_long, c_uint, c_void, CStr, CString};
use std::fs::File;
use std::io::ErrorKind;
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
use std::{ptr, slice};

const BS: u64 = 65536; // cache block
const NB: usize = 32; // blocks (2 MB per open file)
const DC: usize = 8192; // cache of short fields by offset
const HDR_MIN: u64 = 208; // systemd >= 187 headers
const MAX_ENTRY_SIZE: u64 = 1 << 20;
const MAX_PAYLOAD: u64 = 1 << 20; // compressed or not: anything bigger is not read
pub const JMSG_MAX: usize = 16384;

const INC_XZ: u32 = 1;
const INC_LZ4: u32 = 2;
const INC_ZSTD: u32 = 8;
const INC_COMPACT: u32 = 16;
const INC_KNOWN: u32 = 31;
const OBJ_DATA: u8 = 1;
const OBJ_ENTRY: u8 = 3;
const OBJ_ENTRY_ARRAY: u8 = 6;
const OF_XZ: u8 = 1;
const OF_LZ4: u8 = 2;
const OF_ZSTD: u8 = 4;
#[derive(Clone, Copy, PartialEq)]
enum F {
    None,
    Msg,
    Prio,
    Ident,
    Comm,
    Unit,
    UUnit,
    Pid,
    Uid,
    Boot,
    Transport,
}

/// Layout of the C `JEntry`.
#[repr(C)]
pub struct JEntry {
    realtime: u64,
    seqnum: u64,
    priority: c_int,
    pid: c_int,
    uid: c_int,
    kernel: c_int,
    ident: [u8; 64],
    comm: [u8; 32],
    unit: [u8; 96],
    boot: [u8; 33],
    message: *mut c_char,
}

/// Layout of the C `JQuery`.
#[repr(C)]
pub struct JQuery {
    since: u64,
    until: u64,
    prio: c_int,
    kernel: c_int,
    r#match: [u8; 96],
    grep: [u8; 128],
    limit: usize,
}

fn le64(p: &[u8]) -> u64 {
    u64::from_le_bytes(p[..8].try_into().unwrap())
}
fn le32(p: &[u8]) -> u32 {
    u32::from_le_bytes(p[..4].try_into().unwrap())
}

#[derive(Default)]
struct Blk {
    base: u64,
    len: usize,
    valid: bool,
    buf: Vec<u8>,
}
#[derive(Clone, Copy)]
struct DCache {
    off: u64,
    id: F,
    val: [u8; 95],
}

pub struct JFile {
    file: File,
    size: u64,
    compact: bool,
    ent: Vec<u64>,
    blk: Vec<Blk>,
    dc: Vec<DCache>,
}

fn pread_all(file: &File, buf: &mut [u8], off: u64) -> Option<()> {
    let mut got = 0;
    while got < buf.len() {
        match file.read_at(&mut buf[got..], off + got as u64) {
            Ok(0) => return None,
            Ok(n) => got += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(())
}

impl JFile {
    /// Reads `out.len()` bytes at `off`, via the cache if small. None if it runs past the file
    /// (or it shrank).
    fn rd(&mut self, mut off: u64, out: &mut [u8]) -> Option<()> {
        let len = out.len() as u64;
        if off > self.size || len > self.size - off {
            return None;
        }
        if len > BS / 4 {
            return pread_all(&self.file, out, off);
        }
        let mut done = 0usize;
        while done < out.len() {
            let b = off / BS;
            let k = &mut self.blk[(b % NB as u64) as usize];
            if !k.valid || k.base != b * BS {
                if k.buf.is_empty() {
                    k.buf = vec![0; BS as usize];
                }
                let n = loop {
                    match self.file.read_at(&mut k.buf, b * BS) {
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(_) => break 0,
                        Ok(n) => break n,
                    }
                };
                if n == 0 {
                    k.valid = false;
                    return None;
                }
                k.base = b * BS;
                k.len = n;
                k.valid = true;
            }
            let inb = (off - k.base) as usize;
            if inb >= k.len {
                return None;
            }
            let n = (k.len - inb).min(out.len() - done);
            out[done..done + n].copy_from_slice(&k.buf[inb..inb + n]);
            done += n;
            off += n as u64;
        }
        Some(())
    }

    fn open(path: &CStr) -> Option<JFile> {
        use std::os::unix::ffi::OsStrExt;
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(path.to_bytes()));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NOCTTY)
            .open(p)
            .ok()?;
        let md = file.metadata().ok()?;
        if !md.is_file() || md.size() < HDR_MIN {
            return None;
        }
        let size = md.size();
        let mut h = [0u8; 272];
        let want = (size as usize).min(h.len());
        pread_all(&file, &mut h[..want], 0)?;
        if &h[..8] != b"LPKSHHRH" {
            return None;
        }
        let inc = le32(&h[12..]);
        if inc & !INC_KNOWN != 0 {
            return None; // something we don't know: don't guess
        }
        let compact = inc & INC_COMPACT != 0;
        let (hsize, mut n_entries, mut arr) = (le64(&h[88..]), le64(&h[152..]), le64(&h[176..]));
        if hsize < HDR_MIN || hsize > size {
            return None;
        }
        // cap: no more entries than fit in the file (64 bytes minimum each)
        n_entries = n_entries.min(size / 64);
        let mut f = JFile {
            file,
            size,
            compact,
            ent: Vec::new(),
            blk: (0..NB).map(|_| Blk::default()).collect(),
            dc: vec![DCache { off: 0, id: F::None, val: [0; 95] }; DC],
        };
        let isz: u64 = if compact { 4 } else { 8 };
        let mut prev = 0u64;
        let mut arrays = 0u32;
        let mut items: Vec<u8> = Vec::new();
        while arr != 0 && (f.ent.len() as u64) < n_entries {
            let mut oh = [0u8; 24];
            arrays += 1;
            if arr <= prev
                || arr & 7 != 0
                || arr < hsize
                || arrays > 1 << 20
                || f.rd(arr, &mut oh).is_none()
                || oh[0] != OBJ_ENTRY_ARRAY
            {
                break;
            }
            let osize = le64(&oh[8..]);
            if osize < 24 || osize > f.size - arr || osize > (64 << 20) {
                break;
            }
            let n = ((osize - 24) / isz) as usize;
            items.resize(n * isz as usize, 0);
            if n != 0 && f.rd(arr + 24, &mut items).is_none() {
                break;
            }
            let mut end = false;
            let mut i = 0;
            while i < n && (f.ent.len() as u64) < n_entries {
                let e = if compact { le32(&items[i * 4..]) as u64 } else { le64(&items[i * 8..]) };
                i += 1;
                if e == 0 {
                    end = true; // preallocated array: the rest is empty
                    break;
                }
                if e & 7 != 0 || e < hsize || e > f.size - 64 {
                    continue;
                }
                f.ent.push(e);
            }
            if end {
                break;
            }
            prev = arr;
            arr = le64(&oh[16..]);
        }
        Some(f)
    }

    fn entry_header(&mut self, idx: usize) -> Option<([u8; 64], u64)> {
        let off = *self.ent.get(idx)?;
        let mut eh = [0u8; 64];
        self.rd(off, &mut eh)?;
        if eh[0] != OBJ_ENTRY {
            return None;
        }
        let size = le64(&eh[8..]);
        if size < 64 || size > MAX_ENTRY_SIZE || size > self.size - off {
            return None;
        }
        Some((eh, size))
    }

    fn realtime(&mut self, idx: usize) -> u64 {
        self.entry_header(idx).map_or(0, |(eh, _)| le64(&eh[24..]))
    }
}

// ---- LZ4 block ----

/// LZ4 block decoding (the format journald uses, without the size prefix). Bytes written or None.
fn lz4_block(inp: &[u8], out: &mut [u8]) -> Option<usize> {
    let (mut ip, mut op) = (0usize, 0usize);
    let (ie, cap) = (inp.len(), out.len());
    while ip < ie {
        let tok = inp[ip] as usize;
        ip += 1;
        let mut lit = tok >> 4;
        if lit == 15 {
            loop {
                if ip >= ie {
                    return None;
                }
                let b = inp[ip] as usize;
                ip += 1;
                lit += b;
                if !(b == 255 && lit < 1 << 30) {
                    break;
                }
            }
        }
        if lit > ie - ip || lit > cap - op {
            return None;
        }
        out[op..op + lit].copy_from_slice(&inp[ip..ip + lit]);
        ip += lit;
        op += lit;
        if ip >= ie {
            break; // the last sequence has only literals
        }
        if ie - ip < 2 {
            return None;
        }
        let dist = inp[ip] as usize | (inp[ip + 1] as usize) << 8;
        ip += 2;
        if dist == 0 || dist > op {
            return None;
        }
        let mut ml = tok & 15;
        if ml == 15 {
            loop {
                if ip >= ie {
                    return None;
                }
                let b = inp[ip] as usize;
                ip += 1;
                ml += b;
                if !(b == 255 && ml < 1 << 30) {
                    break;
                }
            }
        }
        ml += 4;
        if ml > cap - op {
            return None;
        }
        for _ in 0..ml {
            out[op] = out[op - dist]; // may overlap
            op += 1;
        }
    }
    Some(op)
}

#[no_mangle]
pub unsafe extern "C" fn lz4_block_decode(inp: *const u8, len: usize, out: *mut u8, cap: usize) -> c_long {
    if (inp.is_null() && len != 0) || (out.is_null() && cap != 0) {
        return -1;
    }
    let i: &[u8] = if len == 0 { &[] } else { slice::from_raw_parts(inp, len) };
    let o: &mut [u8] = if cap == 0 { &mut [] } else { slice::from_raw_parts_mut(out, cap) };
    lz4_block(i, o).map_or(-1, |n| n as c_long)
}

// ---- fields ----

/// The field named before the `=` of `p` and the position of the `=`.
fn field_id(p: &[u8]) -> Option<(F, usize)> {
    let eq = p.iter().position(|&c| c == b'=')?;
    let id = match &p[..eq] {
        b"MESSAGE" => F::Msg,
        b"PRIORITY" => F::Prio,
        b"SYSLOG_IDENTIFIER" => F::Ident,
        b"_COMM" => F::Comm,
        b"_SYSTEMD_UNIT" => F::Unit,
        b"_SYSTEMD_USER_UNIT" => F::UUnit,
        b"_PID" => F::Pid,
        b"_UID" => F::Uid,
        b"_BOOT_ID" => F::Boot,
        b"_TRANSPORT" => F::Transport,
        _ => F::None,
    };
    Some((id, eq))
}

/// Copies at most `dst.len() - 1` bytes, control characters shown as '?'.
fn copy_val(dst: &mut [u8], v: &[u8]) -> usize {
    let n = v.len().min(dst.len() - 1);
    for i in 0..n {
        dst[i] = if (v[i] >= 32 && v[i] != 127) || v[i] >= 128 { v[i] } else { b'?' };
    }
    dst[n] = 0;
    n
}

fn parse_int(s: &[u8]) -> c_int {
    if s.is_empty() {
        return -1;
    }
    let mut v: i64 = 0;
    for &c in s {
        if !c.is_ascii_digit() || v > 100_000_000 {
            return -1;
        }
        v = v * 10 + (c - b'0') as i64;
    }
    v as c_int
}

/// `snprintf(dst, sizeof dst, "%s", v)`.
fn put(dst: &mut [u8], v: &[u8]) {
    let n = v.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&v[..n]);
    dst[n] = 0;
}
fn is_empty(s: &[u8]) -> bool {
    s[0] == 0
}

fn set_field(e: &mut JEntry, id: F, v: &[u8]) {
    match id {
        F::Prio => {
            if v.len() == 1 && (b'0'..=b'7').contains(&v[0]) {
                e.priority = (v[0] - b'0') as c_int;
            }
        }
        F::Ident => put(&mut e.ident, v),
        F::Comm => put(&mut e.comm, v),
        F::Unit => {
            if is_empty(&e.unit) {
                put(&mut e.unit, v)
            }
        }
        F::UUnit => put(&mut e.unit, v), // the user unit wins
        F::Pid => e.pid = parse_int(v),
        F::Uid => e.uid = parse_int(v),
        F::Boot => put(&mut e.boot, v),
        F::Transport => e.kernel = (v == b"kernel") as c_int,
        F::Msg | F::None => {}
    }
}

unsafe fn set_message(e: &mut JEntry, v: &[u8]) {
    let cut = v.len() > JMSG_MAX;
    let n = v.len().min(JMSG_MAX);
    let m = libc::malloc(n + 4) as *mut u8;
    if m.is_null() {
        return;
    }
    ptr::copy_nonoverlapping(v.as_ptr(), m, n);
    let mut total = n;
    if cut {
        ptr::copy_nonoverlapping("\u{2026}".as_ptr(), m.add(n), 3); // «…»
        total += 3;
    }
    *m.add(total) = 0;
    libc::free(e.message.cast());
    e.message = m.cast();
}

/// The value as a C string (<= 95 characters): trailing part of a decoded `FIELD=value`.
fn val96(v: &[u8]) -> ([u8; 96], usize) {
    let mut b = [0u8; 96];
    let n = copy_val(&mut b, v);
    (b, n)
}

impl JFile {
    /// A compressed field: it is decompressed whole (capped) and parsed.
    unsafe fn compressed_field(&mut self, e: &mut JEntry, poff: u64, plen: u64, flags: u8, zstd: &mut u32) {
        if flags & OF_ZSTD != 0 {
            *zstd += 1;
            return;
        }
        if plen > MAX_PAYLOAD {
            return;
        }
        let mut inp = vec![0u8; plen as usize];
        if self.rd(poff, &mut inp).is_none() {
            return;
        }
        let out: Option<Vec<u8>> = if flags & OF_XZ != 0 {
            crate::xz::decode(&inp, MAX_PAYLOAD as usize)
        } else if flags & OF_LZ4 != 0 && plen > 8 {
            let want = le64(&inp);
            if want != 0 && want <= MAX_PAYLOAD {
                let mut o = vec![0u8; want as usize];
                lz4_block(&inp[8..], &mut o).map(|n| {
                    o.truncate(n);
                    o
                })
            } else {
                None
            }
        } else {
            None
        };
        let Some(out) = out else { return };
        match field_id(&out) {
            Some((F::Msg, eq)) => set_message(e, &out[eq + 1..]),
            Some((id, eq)) if id != F::None => {
                let (v, n) = val96(&out[eq + 1..]);
                set_field(e, id, &v[..n]);
            }
            _ => {}
        }
    }

    /// Decodes entry `idx` into `out`; false if it is broken.
    unsafe fn entry(&mut self, idx: usize, out: &mut JEntry) -> bool {
        *out = JEntry {
            realtime: 0,
            seqnum: 0,
            priority: -1,
            pid: -1,
            uid: -1,
            kernel: 0,
            ident: [0; 64],
            comm: [0; 32],
            unit: [0; 96],
            boot: [0; 33],
            message: ptr::null_mut(),
        };
        let Some((eh, size)) = self.entry_header(idx) else { return false };
        let off = self.ent[idx];
        out.seqnum = le64(&eh[16..]);
        out.realtime = le64(&eh[24..]);
        let isz: u64 = if self.compact { 4 } else { 16 };
        let n = (((size - 64) / isz) as usize).min(4096);
        let mut items = vec![0u8; n * isz as usize];
        if n != 0 && self.rd(off + 64, &mut items).is_none() {
            return false;
        }
        let pstart: u64 = if self.compact { 72 } else { 64 };
        let mut zstd = 0u32;
        for i in 0..n {
            let d = if self.compact { le32(&items[i * 4..]) as u64 } else { le64(&items[i * 16..]) };
            if d == 0 || d & 7 != 0 || d > self.size - 16 {
                continue;
            }
            let ci = ((d >> 3) % DC as u64) as usize;
            if self.dc[ci].off == d {
                let c = self.dc[ci];
                if c.id != F::None {
                    let l = c.val.iter().position(|&b| b == 0).unwrap_or(95);
                    set_field(out, c.id, &c.val[..l]);
                }
                continue;
            }
            let mut dh = [0u8; 16];
            if self.rd(d, &mut dh).is_none() || dh[0] != OBJ_DATA {
                continue;
            }
            let osize = le64(&dh[8..]);
            if osize < pstart || osize > self.size - d {
                continue;
            }
            let (plen, poff) = (osize - pstart, d + pstart);
            let flags = dh[1];
            if flags & (OF_XZ | OF_LZ4 | OF_ZSTD) != 0 {
                self.compressed_field(out, poff, plen, flags, &mut zstd);
                continue;
            }
            let mut head = [0u8; 128];
            let hl = (plen as usize).min(head.len());
            if self.rd(poff, &mut head[..hl]).is_none() {
                continue;
            }
            let Some((id, eq)) = field_id(&head[..hl]) else { continue };
            if id == F::Msg {
                let ml = ((plen as usize) - eq - 1).min(JMSG_MAX + 1);
                let mut m = vec![0u8; ml];
                if self.rd(poff + eq as u64 + 1, &mut m).is_some() {
                    set_message(out, &m);
                }
                continue;
            }
            let c = &mut self.dc[ci];
            c.off = d;
            c.id = id;
            c.val[0] = 0;
            if id != F::None {
                let n = copy_val(&mut c.val, &head[eq + 1..hl]);
                let v = c.val;
                set_field(out, id, &v[..n]);
            }
        }
        if out.message.is_null() {
            let s = CString::new(if zstd > 0 { "[zstd-compressed]" } else { "" }).unwrap();
            out.message = libc::strdup(s.as_ptr());
        }
        !out.message.is_null()
    }
}

// ---- C ABI: files ----

#[no_mangle]
pub unsafe extern "C" fn jf_open(path: *const c_char) -> *mut JFile {
    if path.is_null() {
        return ptr::null_mut();
    }
    match JFile::open(CStr::from_ptr(path)) {
        Some(f) => Box::into_raw(Box::new(f)),
        None => ptr::null_mut(),
    }
}
#[no_mangle]
pub unsafe extern "C" fn jf_close(f: *mut JFile) {
    if !f.is_null() {
        drop(Box::from_raw(f));
    }
}
#[no_mangle]
pub unsafe extern "C" fn jf_count(f: *const JFile) -> usize {
    f.as_ref().map_or(0, |f| f.ent.len())
}
#[no_mangle]
pub unsafe extern "C" fn jf_compact(f: *const JFile) -> c_int {
    f.as_ref().map_or(0, |f| f.compact as c_int)
}
/// µs since 1970 of entry `idx` (0 = the oldest), without decoding its fields; 0 if broken.
#[no_mangle]
pub unsafe extern "C" fn jf_realtime(f: *mut JFile, idx: usize) -> u64 {
    f.as_mut().map_or(0, |f| f.realtime(idx))
}
/// Decodes entry `idx`. 0 ok (free with jentry_free), -1 broken.
#[no_mangle]
pub unsafe extern "C" fn jf_entry(f: *mut JFile, idx: usize, out: *mut JEntry) -> c_int {
    if out.is_null() {
        return -1;
    }
    // the C caller's buffer may be uninitialized: build the value apart and move it in
    let mut e: JEntry = std::mem::zeroed();
    let ok = match f.as_mut() {
        Some(f) => f.entry(idx, &mut e),
        None => false,
    };
    if !ok {
        libc::free(e.message.cast());
        e.message = ptr::null_mut();
    }
    ptr::write(out, e);
    if ok {
        0
    } else {
        -1
    }
}
#[no_mangle]
pub unsafe extern "C" fn jentry_free(e: *mut JEntry) {
    if let Some(e) = e.as_mut() {
        libc::free(e.message.cast());
        e.message = ptr::null_mut();
    }
}

// ---- JSON ----

struct JOut<'a> {
    b: &'a mut [u8],
    n: usize,
    full: bool,
}
impl JOut<'_> {
    fn put(&mut self, s: &[u8]) {
        if self.full || s.len() > self.b.len() - self.n - 1 {
            self.full = true;
            return;
        }
        self.b[self.n..self.n + s.len()].copy_from_slice(s);
        self.n += s.len();
    }
    /// Text as a JSON string: control characters escaped and invalid UTF-8 → U+FFFD.
    fn esc(&mut self, s: &[u8], max: usize) {
        let s = &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())];
        let (mut i, mut used) = (0usize, 0usize);
        while i < s.len() && used < max {
            let c = s[i] as u32;
            let len = if c < 0x80 {
                1
            } else if c & 0xe0 == 0xc0 {
                2
            } else if c & 0xf0 == 0xe0 {
                3
            } else if c & 0xf8 == 0xf0 {
                4
            } else {
                0
            };
            let at = |k: usize| s.get(i + k).copied().unwrap_or(0);
            let mut ok = len > 0 && (len == 1 || c >= 0xc2) && c <= 0xf4;
            for k in 1..len {
                if at(k) & 0xc0 != 0x80 {
                    ok = false;
                }
            }
            if ok && len == 3 && ((c == 0xe0 && at(1) < 0xa0) || (c == 0xed && at(1) >= 0xa0)) {
                ok = false;
            }
            if ok && len == 4 && ((c == 0xf0 && at(1) < 0x90) || (c == 0xf4 && at(1) >= 0x90)) {
                ok = false;
            }
            if !ok {
                self.put("\u{fffd}".as_bytes());
                i += 1;
                used += 1;
                continue;
            }
            if len > 1 {
                self.put(&s[i..i + len]);
                i += len;
                used += len;
                continue;
            }
            match c as u8 {
                b'"' | b'\\' => self.put(&[b'\\', c as u8]),
                b'\n' => self.put(b"\\n"),
                b'\t' => self.put(b"\\t"),
                x if x < 32 || x == 127 => self.put(format!("\\u{:04x}", x).as_bytes()),
                x => self.put(&[x]),
            }
            i += 1;
            used += 1;
        }
        if i < s.len() {
            self.put("\u{2026}".as_bytes());
        }
    }
}

/// One entry as JSON `{"t":µs,"s":seq,"p":n,"pid":n,"uid":n,"k":0|1,"id":"","u":"","b":"","m":""}`.
/// The message is cut to `msgmax` bytes. Returns the length written (0 if it doesn't fit).
#[no_mangle]
pub unsafe extern "C" fn jentry_json(e: *const JEntry, buf: *mut c_char, cap: usize, msgmax: usize) -> usize {
    if e.is_null() || buf.is_null() || cap < 2 {
        return 0;
    }
    let e = &*e;
    let mut o = JOut { b: slice::from_raw_parts_mut(buf.cast::<u8>(), cap), n: 0, full: false };
    o.put(
        format!(
            "{{\"t\":{},\"s\":{},\"p\":{},\"pid\":{},\"uid\":{},\"k\":{},\"id\":\"",
            e.realtime, e.seqnum, e.priority, e.pid, e.uid, e.kernel
        )
        .as_bytes(),
    );
    o.esc(if e.ident[0] != 0 { &e.ident } else { &e.comm }, 64);
    o.put(b"\",\"u\":\"");
    o.esc(&e.unit, 96);
    o.put(b"\",\"b\":\"");
    o.esc(&e.boot, 8);
    o.put(b"\",\"m\":\"");
    let msg: &[u8] = if e.message.is_null() { b"" } else { CStr::from_ptr(e.message).to_bytes() };
    o.esc(msg, msgmax);
    o.put(b"\"}");
    if o.full {
        return 0;
    }
    let n = o.n;
    o.b[n] = 0;
    n
}

// ---- queries ----

fn valid_match(s: &[u8]) -> bool {
    !s.is_empty() && s.len() <= 95 && s.iter().all(|&c| c.is_ascii_alphanumeric() || b"@._:-".contains(&c))
}
#[no_mangle]
pub unsafe extern "C" fn jq_valid_match(s: *const c_char) -> c_int {
    (!s.is_null() && valid_match(CStr::from_ptr(s).to_bytes())) as c_int
}

fn parse_u64(s: &[u8]) -> Option<u64> {
    if s.is_empty() || s.len() > 19 || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(s.iter().fold(0u64, |v, &c| v * 10 + (c - b'0') as u64))
}

fn parse_query(text: &[u8], q: &mut JQuery, max_limit: usize) -> Option<()> {
    q.prio = -1;
    q.limit = max_limit.min(1000);
    let mut p = text;
    while !p.is_empty() {
        let nl = p.iter().position(|&c| c == b'\n');
        let n = nl.unwrap_or(p.len());
        if n >= 256 {
            return None;
        }
        let line = &p[..n];
        p = &p[n + nl.is_some() as usize..];
        if line.is_empty() {
            continue;
        }
        let eq = line.iter().position(|&c| c == b'=')?;
        let (k, v) = (&line[..eq], &line[eq + 1..]);
        match k {
            b"since" | b"until" => {
                let mut u = parse_u64(v)?;
                if u != 0 && u < 1_000_000_000_000 {
                    u *= 1_000_000; // seconds → µs
                }
                if k == b"since" {
                    q.since = u
                } else {
                    q.until = u
                }
            }
            b"prio" => {
                if v.len() != 1 || !(b'0'..=b'7').contains(&v[0]) {
                    return None;
                }
                q.prio = (v[0] - b'0') as c_int;
            }
            b"kernel" => {
                if v != b"0" && v != b"1" {
                    return None;
                }
                q.kernel = (v[0] == b'1') as c_int;
            }
            b"match" => {
                if !v.is_empty() && !valid_match(v) {
                    return None;
                }
                put(&mut q.r#match, v);
            }
            b"grep" => {
                if v.len() > 127 || v.iter().any(|&c| c < 32 || c == 127) {
                    return None;
                }
                put(&mut q.grep, v);
            }
            b"limit" => {
                let u = parse_u64(v)?;
                if u == 0 || u > max_limit as u64 {
                    return None;
                }
                q.limit = u as usize;
            }
            _ => return None,
        }
    }
    Some(())
}

/// Parses a text query, one «key=value» per line (since, until, prio, match, grep, kernel,
/// limit). since/until in seconds or µs (> 1e12). Returns 0 if everything is valid.
#[no_mangle]
pub unsafe extern "C" fn jq_parse(text: *const c_char, q: *mut JQuery, max_limit: usize) -> c_int {
    if text.is_null() || q.is_null() {
        return -1;
    }
    ptr::write(
        q,
        JQuery { since: 0, until: 0, prio: 0, kernel: 0, r#match: [0; 96], grep: [0; 128], limit: 0 },
    );
    if parse_query(CStr::from_ptr(text).to_bytes(), &mut *q, max_limit).is_some() {
        0
    } else {
        -1
    }
}

fn cstr_bytes(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}

fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

unsafe fn matches(q: &JQuery, e: &JEntry) -> bool {
    if q.prio >= 0 && (e.priority < 0 || e.priority > q.prio) {
        return false; // no priority: no match
    }
    if q.kernel != 0 && e.kernel == 0 {
        return false;
    }
    let m = cstr_bytes(&q.r#match);
    if !m.is_empty() {
        let (ident, comm, unit) = (cstr_bytes(&e.ident), cstr_bytes(&e.comm), cstr_bytes(&e.unit));
        let ok = ident == m
            || comm == m
            || unit == m
            || (unit.len() == m.len() + 8 && unit.starts_with(m) && unit.ends_with(b".service"));
        if !ok {
            return false;
        }
    }
    let g = cstr_bytes(&q.grep);
    if !g.is_empty() {
        let msg: &[u8] = if e.message.is_null() { b"" } else { CStr::from_ptr(e.message).to_bytes() };
        if !contains_ci(msg, g) {
            return false;
        }
    }
    true
}

/// Runs the query over several files; entries come out in chronological order (oldest first).
/// Returns the count or -1; free with jq_free.
#[no_mangle]
pub unsafe extern "C" fn jq_run(paths: *const *const c_char, npaths: c_int, q: *const JQuery, out: *mut *mut JEntry) -> c_long {
    if q.is_null() || out.is_null() || (paths.is_null() && npaths > 0) {
        return -1;
    }
    let q = &*q;
    let limit = if q.limit != 0 { q.limit } else { 1000 };
    let mut all: Vec<JEntry> = Vec::new();
    for p in 0..npaths.max(0) as usize {
        let path = *paths.add(p);
        let Some(mut f) = (if path.is_null() { None } else { JFile::open(CStr::from_ptr(path)) }) else { continue };
        let mut found = 0usize;
        let mut i = f.ent.len();
        while i > 0 && found < limit {
            i -= 1;
            let rt = f.realtime(i);
            if rt == 0 {
                continue;
            }
            if q.until != 0 && rt >= q.until {
                continue;
            }
            if q.since != 0 && rt <= q.since {
                break;
            }
            let mut e: JEntry = std::mem::zeroed();
            if !f.entry(i, &mut e) {
                libc::free(e.message.cast());
                continue;
            }
            if !matches(q, &e) {
                libc::free(e.message.cast());
                continue;
            }
            all.push(e);
            found += 1;
        }
    }
    all.sort_by(|a, b| a.realtime.cmp(&b.realtime).then(a.seqnum.cmp(&b.seqnum)));
    if all.len() > limit {
        // the newest ones
        let drop = all.len() - limit;
        for e in all.drain(..drop) {
            libc::free(e.message.cast());
        }
    }
    let n = all.len();
    if n == 0 {
        *out = ptr::null_mut();
        return 0;
    }
    let arr = libc::malloc(n * std::mem::size_of::<JEntry>()) as *mut JEntry;
    if arr.is_null() {
        for e in all {
            libc::free(e.message.cast());
        }
        return -1;
    }
    ptr::copy_nonoverlapping(all.as_ptr(), arr, n);
    *out = arr;
    n as c_long
}

#[no_mangle]
pub unsafe extern "C" fn jq_free(e: *mut JEntry, n: c_long) {
    if e.is_null() {
        return;
    }
    for i in 0..n.max(0) as usize {
        libc::free((*e.add(i)).message.cast());
    }
    libc::free(e.cast());
}

// ---- files ----

fn name_ok(name: &[u8], prefix: &[u8]) -> bool {
    if !name.starts_with(prefix) {
        return false;
    }
    let rest = &name[prefix.len()..];
    if rest != b".journal" && rest.first() != Some(&b'@') {
        return false;
    }
    (name.len() > 8 && name.ends_with(b".journal")) || (name.len() > 9 && name.ends_with(b".journal~"))
}

/// Journal files of that scope (1 = the user's, 2 = the system's) in /var/log/journal and
/// /run/log/journal that this process can read. Returns how many (malloc'd paths).
#[no_mangle]
pub unsafe extern "C" fn jq_files(scope: c_int, uid: c_uint, paths: *mut *mut *mut c_char) -> c_int {
    use std::os::unix::ffi::OsStrExt;
    if paths.is_null() {
        return 0;
    }
    *paths = ptr::null_mut();
    let mid = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
    let mid = mid.lines().next().unwrap_or("");
    if mid.len() != 32 || !mid.bytes().all(|c| c.is_ascii_hexdigit()) {
        return 0;
    }
    let prefix = if scope == 1 { format!("user-{uid}") } else { "system".to_string() };
    let mut found: Vec<CString> = Vec::new();
    'roots: for root in ["/var/log/journal", "/run/log/journal"] {
        let dir = format!("{root}/{mid}");
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for de in rd.flatten() {
            let name = de.file_name();
            if !name_ok(name.as_bytes(), prefix.as_bytes()) {
                continue;
            }
            let path = de.path();
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            if !md.file_type().is_file() {
                continue;
            }
            let Ok(c) = CString::new(path.as_os_str().as_bytes()) else { continue };
            if libc::access(c.as_ptr(), libc::R_OK) != 0 {
                continue;
            }
            found.push(c);
            if found.len() >= 512 {
                continue 'roots;
            }
        }
    }
    if found.is_empty() {
        return 0;
    }
    let list = libc::malloc(found.len() * std::mem::size_of::<*mut c_char>()) as *mut *mut c_char;
    if list.is_null() {
        return 0;
    }
    let mut n = 0;
    for c in &found {
        let d = libc::strdup(c.as_ptr());
        if d.is_null() {
            break;
        }
        *list.add(n) = d;
        n += 1;
    }
    *paths = list;
    n as c_int
}

#[no_mangle]
pub unsafe extern "C" fn jq_files_free(paths: *mut *mut c_char, n: c_int) {
    if paths.is_null() {
        return;
    }
    for i in 0..n.max(0) as usize {
        libc::free((*paths.add(i)).cast::<c_void>());
    }
    libc::free(paths.cast());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str, max: usize) -> Option<JQuery> {
        let c = CString::new(text).unwrap();
        let mut q = std::mem::MaybeUninit::<JQuery>::uninit();
        let rc = unsafe { jq_parse(c.as_ptr(), q.as_mut_ptr(), max) };
        (rc == 0).then(|| unsafe { q.assume_init() })
    }
    fn json(e: &JEntry, msgmax: usize) -> String {
        let mut b = vec![0 as c_char; 4096];
        let n = unsafe { jentry_json(e, b.as_mut_ptr(), b.len(), msgmax) };
        String::from_utf8(b[..n].iter().map(|&c| c as u8).collect()).unwrap()
    }

    #[test]
    fn layouts_match_c() {
        assert_eq!(std::mem::size_of::<JEntry>(), 8 + 8 + 16 + 64 + 32 + 96 + 33 + 7 + 8);
        assert_eq!(std::mem::size_of::<JQuery>(), 248 + 8);
    }

    #[test]
    fn lz4_literals_and_overlapping_match() {
        // token 0x4f: 4 literals "lz4 ", match length 15+4 with distance 4 (overlapping copy)
        let src = [0x4f, b'l', b'z', b'4', b' ', 4, 0, 0];
        let mut out = [0u8; 64];
        let n = lz4_block(&src, &mut out).unwrap();
        assert_eq!(&out[..n], b"lz4 lz4 lz4 lz4 lz4 lz4");
        assert!(lz4_block(&src, &mut [0u8; 10]).is_none(), "output cap");
        assert!(lz4_block(&[0x10, b'a', 9, 0], &mut out).is_none(), "distance past the start");
        assert!(lz4_block(&[0xf0], &mut out).is_none(), "truncated length");
    }

    #[test]
    fn query_text() {
        let a = q("since=1700000000\nprio=3\nmatch=sshd.service\ngrep=Failed\nkernel=1\nlimit=50\n", 5000).unwrap();
        assert_eq!((a.since, a.prio, a.kernel, a.limit), (1_700_000_000_000_000, 3, 1, 50));
        assert_eq!(cstr_bytes(&a.r#match), b"sshd.service");
        assert_eq!(q("", 5000).unwrap().limit, 1000);
        assert_eq!(q("", 20).unwrap().limit, 20);
        for bad in ["prio=8", "kernel=2", "limit=0", "limit=6000", "match=a b", "grep=\u{1}", "since=12x", "nokey", "x=1"] {
            assert!(q(bad, 5000).is_none(), "{bad}");
        }
        assert!(q(&format!("grep={}", "a".repeat(128)), 5000).is_none());
        assert!(q(&format!("match={}", "a".repeat(96)), 5000).is_none());
    }

    #[test]
    fn json_escapes_and_cuts() {
        let mut e: JEntry = unsafe { std::mem::zeroed() };
        e.priority = 6;
        put(&mut e.ident, b"sshd");
        put(&mut e.boot, b"0123456789abcdef");
        let m = CString::new("a\"b\\c\n\t\u{1}\u{7f}\u{e9}").unwrap();
        e.message = unsafe { libc::strdup(m.as_ptr()) };
        let j = json(&e, 100);
        assert!(j.contains(r#""id":"sshd""#) && j.contains("\"b\":\"01234567\u{2026}\""), "{j}");
        assert!(j.contains(r#""m":"a\"b\\c\n\t\u0001\u007fé""#), "{j}");
        // invalid UTF-8 → U+FFFD; cut message → …
        unsafe { libc::free(e.message.cast()) };
        e.message = unsafe { libc::strdup(b"ab\xffcdef\0".as_ptr().cast()) };
        assert!(json(&e, 100).contains("ab\u{fffd}cdef"));
        assert!(json(&e, 4).contains("ab\u{fffd}c\u{2026}"));
        let mut tiny = [0 as c_char; 20];
        assert_eq!(unsafe { jentry_json(&e, tiny.as_mut_ptr(), 20, 100) }, 0, "does not fit");
        unsafe { libc::free(e.message.cast()) };
    }
}
