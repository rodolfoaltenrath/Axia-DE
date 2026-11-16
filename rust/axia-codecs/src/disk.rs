//! Disks, partitions and mounts without root, for the Disks app. C ABI of
//! server/src/disk/disk.h (the former disk.c).
//!
//! Sources, none of which needs root:
//!   /sys/block/<disk>/…                 size, removable, read-only, rotational, model
//!   /run/udev/data/b<major>:<minor>     what udev already probed: file system, label, UUID,
//!                                       partition table, bus (without opening the device)
//!   /proc/self/mountinfo, /proc/swaps   where each one is mounted
//! The system disk is recognized right here.
//!
//! This is a faithful port: the C code read these files with `fgets` into
//! fixed buffers and `sscanf` and returned C strings, so the quirks callers see
//! (a long line split in pieces, tokens cut at their buffer size, values that
//! stop at a NUL, the order of `readdir`) are kept on purpose.
use libc::{c_char, c_int};
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering::Relaxed};

const PATH_MAX: usize = 4096;
/// Size of the `fgets` buffers of the C code.
const MOUNTINFO_LINE: usize = PATH_MAX * 2 + 512;
const SWAPS_LINE: usize = 512;
const UDEV_LINE: usize = 1024;
/// The set of system disks lives in a `char[1024]` in C.
const SET_CAP: usize = 1024;

// ---- C strings ----

fn z(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

unsafe fn cstr<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        b""
    } else {
        std::ffi::CStr::from_ptr(p).to_bytes()
    }
}

/// `snprintf(out, n, "%s", s)`.
unsafe fn put(out: *mut c_char, n: usize, s: &[u8]) {
    if out.is_null() || n == 0 {
        return;
    }
    let k = z(s).len().min(n - 1);
    std::ptr::copy_nonoverlapping(s.as_ptr(), out.cast::<u8>(), k);
    *out.add(k) = 0;
}

fn trunc(mut v: Vec<u8>, cap: usize) -> Vec<u8> {
    v.truncate(cap.saturating_sub(1));
    v
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() {
        return Some(0);
    }
    h.windows(n.len()).position(|w| w == n)
}

/// `atoll`: leading blanks, a sign and digits; saturates like strtoll.
fn atoll(s: &[u8]) -> i64 {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut v: i128 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + (s[i] - b'0') as i128).min(i64::MAX as i128 + 1);
        i += 1;
    }
    let v = if neg { -v } else { v };
    v.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// The pieces `fgets(buf, cap, f)` returns while reading `data`: up to cap-1
/// bytes, ending after a newline.
fn chunks(data: &[u8], cap: usize) -> impl Iterator<Item = &[u8]> {
    let mut pos = 0;
    std::iter::from_fn(move || {
        if pos >= data.len() {
            return None;
        }
        let limit = (pos + cap - 1).min(data.len());
        let end = data[pos..limit].iter().position(|&c| c == b'\n').map_or(limit, |k| pos + k + 1);
        let c = &data[pos..end];
        pos = end;
        Some(c)
    })
}

/// Just enough of `sscanf` for the formats the C code used.
struct Scan<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Scan<'a> {
    fn new(s: &'a [u8]) -> Self {
        Scan { s: z(s), i: 0 }
    }
    fn ws(&mut self) {
        while self.i < self.s.len() && is_space(self.s[self.i]) {
            self.i += 1;
        }
    }
    /// `%d` / `%u` (value as strtoul gives it, negative wrapping).
    fn num(&mut self) -> Option<u64> {
        self.ws();
        let mut neg = false;
        if self.i < self.s.len() && (self.s[self.i] == b'+' || self.s[self.i] == b'-') {
            neg = self.s[self.i] == b'-';
            self.i += 1;
        }
        let d0 = self.i;
        let mut v: u64 = 0;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add((self.s[self.i] - b'0') as u64);
            self.i += 1;
        }
        if self.i == d0 {
            return None;
        }
        Some(if neg { v.wrapping_neg() } else { v })
    }
    fn lit(&mut self, c: u8) -> bool {
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    /// `%<max>s`.
    fn word(&mut self, max: usize) -> Option<&'a [u8]> {
        self.ws();
        let a = self.i;
        while self.i < self.s.len() && !is_space(self.s[self.i]) && self.i - a < max {
            self.i += 1;
        }
        (self.i > a).then(|| &self.s[a..self.i])
    }
}

// ---- file system access (a root prefix only for tests) ----

#[derive(Clone)]
pub(crate) struct Fs {
    root: Vec<u8>,
}

#[cfg(feature = "disk-test-root")]
static TEST_ROOT: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

/// Test builds only (feature `disk-test-root`): resolve every path below `root`.
#[cfg(feature = "disk-test-root")]
#[no_mangle]
pub unsafe extern "C" fn axia_disk_test_root(root: *const c_char) {
    *TEST_ROOT.lock().unwrap_or_else(|e| e.into_inner()) = cstr(root).to_vec();
}

fn fs() -> Fs {
    #[cfg(feature = "disk-test-root")]
    {
        Fs { root: TEST_ROOT.lock().unwrap_or_else(|e| e.into_inner()).clone() }
    }
    #[cfg(not(feature = "disk-test-root"))]
    {
        Fs { root: Vec::new() }
    }
}

fn valid_name(n: &[u8]) -> bool {
    let n = z(n);
    !n.is_empty() && n.len() <= 31 && n[0].is_ascii_lowercase() && n.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// The names the system-disk detection follows: block device names, and device-mapper ones
/// (dm-0, the root of a LUKS or LVM install), which disk_valid_name refuses because the actions
/// never take them. Without this a mount on dm-0 never reached the disks under it.
fn follows(n: &[u8]) -> bool {
    valid_name(n) || (n.len() <= 31 && n.strip_prefix(b"dm-").is_some_and(|d| !d.is_empty() && d.iter().all(u8::is_ascii_digit)))
}

fn access(p: &Path, mode: c_int) -> bool {
    match std::ffi::CString::new(p.as_os_str().as_bytes()) {
        Ok(c) => unsafe { libc::access(c.as_ptr(), mode) == 0 },
        Err(_) => false,
    }
}

/// The base name of a real path (`strrchr(real, '/') + 1`).
fn base_of(p: &std::path::Path) -> Option<Vec<u8>> {
    let b = p.as_os_str().as_bytes();
    Some(b[b.iter().rposition(|&c| c == b'/')? + 1..].to_vec())
}

fn blockdev(p: &[u8]) -> Option<(u32, u32)> {
    let md = fs::metadata(OsStr::from_bytes(p)).ok()?;
    if !md.file_type().is_block_device() {
        return None;
    }
    Some((libc::major(md.rdev()), libc::minor(md.rdev())))
}

/// `\ooo` escapes of mountinfo and /proc/swaps (the result stops at a NUL, like the C string).
fn unescape_octal(s: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let at = |k: usize| s.get(i + k).copied().unwrap_or(0);
        if s[i] == b'\\' && (b'0'..=b'3').contains(&at(1)) && (b'0'..=b'7').contains(&at(2)) && (b'0'..=b'7').contains(&at(3)) {
            o.push((at(1) - b'0') * 64 + (at(2) - b'0') * 8 + (at(3) - b'0'));
            i += 4;
        } else {
            o.push(s[i]);
            i += 1;
        }
    }
    z(&o).to_vec()
}

/// ID_FS_LABEL_ENC: unusual characters come as \xNN (output of at most cap-1 bytes).
fn unescape_udev(s: &[u8], cap: usize) -> Vec<u8> {
    let s = z(s);
    let mut o = Vec::new();
    let mut i = 0;
    while i < s.len() && o.len() + 1 < cap {
        let at = |k: usize| s.get(i + k).copied().unwrap_or(0);
        if s[i] == b'\\' && at(1) == b'x' && at(2).is_ascii_hexdigit() && at(3).is_ascii_hexdigit() {
            o.push(((at(2) as char).to_digit(16).unwrap_or(0) * 16 + (at(3) as char).to_digit(16).unwrap_or(0)) as u8);
            i += 4;
        } else {
            o.push(s[i]);
            i += 1;
        }
    }
    o
}

struct Mnt {
    maj: u32,
    min: u32,
    point: Vec<u8>,
    source: Vec<u8>,
}

/// One mountinfo line: "%*d %*d %u:%u %*s %4095s" up to " - ", then "%31s %255s".
fn parse_mnt(line: &[u8]) -> Option<Mnt> {
    let line = z(line);
    let mut sc = Scan::new(line);
    sc.num()?;
    sc.num()?;
    let maj = sc.num()? as u32;
    if !sc.lit(b':') {
        return None;
    }
    let min = sc.num()? as u32;
    sc.word(usize::MAX)?;
    let point = sc.word(PATH_MAX - 1)?;
    let dash = find(line, b" - ")?;
    let mut sc2 = Scan::new(&line[dash + 3..]);
    sc2.word(31)?;
    let source = sc2.word(255)?;
    Some(Mnt { maj, min, point: unescape_octal(point), source: source.to_vec() })
}

/// Device number of a mount: the one in mountinfo, or (btrfs, 0:N) the one of its /dev/… source.
fn mount_dev(m: &Mnt) -> Option<(u32, u32)> {
    if m.maj != 0 {
        return Some((m.maj, m.min));
    }
    if !m.source.starts_with(b"/dev/") {
        return None;
    }
    blockdev(&m.source)
}

const SYSTEM_POINTS: [&[u8]; 10] = [
    b"/",
    b"/usr",
    b"/var",
    b"/home",
    b"/boot",
    b"/boot/efi",
    b"/efi",
    b"/run/live/medium",
    b"/lib/live/mount/medium",
    b"/run/initramfs/live",
];

impl Fs {
    fn path(&self, parts: &[&[u8]]) -> PathBuf {
        let mut p = Vec::new();
        for s in parts {
            p.extend_from_slice(s);
        }
        p.truncate(PATH_MAX - 1);
        let mut full = self.root.clone();
        full.extend_from_slice(&p);
        PathBuf::from(OsStr::from_bytes(&full))
    }

    /// One `read` of at most cap-1 bytes, trailing blanks and newlines removed
    /// (the model comes with trailing spaces). The result may hold NULs; callers
    /// use it as a C string.
    fn read_small(&self, path: &Path, cap: usize) -> Option<Vec<u8>> {
        let mut f = fs::File::open(path).ok()?;
        let mut buf = vec![0u8; cap.saturating_sub(1)];
        let r = f.read(&mut buf).ok()?;
        buf.truncate(r);
        while matches!(buf.last(), Some(b'\n' | b' ')) {
            buf.pop();
        }
        Some(buf)
    }

    fn read_num(&self, path: &Path) -> i64 {
        self.read_small(path, 32).map_or(-1, |b| atoll(z(&b)))
    }

    /// /sys/class/block/<name>/<rel>
    fn sysfs(&self, name: &[u8], rel: &[u8], cap: usize) -> Option<Vec<u8>> {
        self.read_small(&self.path(&[b"/sys/class/block/", z(name), b"/", rel]), cap)
    }

    fn dev_numbers(&self, name: &[u8]) -> Option<(u32, u32)> {
        let b = self.sysfs(name, b"dev", 32)?;
        let mut sc = Scan::new(&b);
        let ma = sc.num()? as u32;
        if !sc.lit(b':') {
            return None;
        }
        Some((ma, sc.num()? as u32))
    }

    fn parent(&self, name: &[u8]) -> Option<Vec<u8>> {
        let name = z(name);
        if !access(&self.path(&[b"/sys/class/block/", name, b"/partition"]), libc::F_OK) {
            return None;
        }
        let real = fs::canonicalize(self.path(&[b"/sys/class/block/", name, b"/.."])).ok()?;
        let b = base_of(&real)?;
        valid_name(&b).then_some(b)
    }

    fn size(&self, name: &[u8]) -> u64 {
        let s = self.read_num(&self.path(&[b"/sys/class/block/", z(name), b"/size"]));
        if s > 0 {
            (s as u64).wrapping_mul(512)
        } else {
            0
        }
    }

    /// The value of a udev database property, cut to cap-1 bytes.
    fn udev_prop(&self, name: &[u8], key: &[u8], cap: usize) -> Option<Vec<u8>> {
        let (ma, mi) = self.dev_numbers(name)?;
        let p = self.path(&[format!("/run/udev/data/b{ma}:{mi}").as_bytes()]);
        let data = fs::read(p).ok()?;
        let key = z(key);
        for line in chunks(&data, UDEV_LINE) {
            if line.len() < 2 + key.len() || line[0] != b'E' || line[1] != b':' || &line[2..2 + key.len()] != key {
                continue;
            }
            if line.get(2 + key.len()) != Some(&b'=') {
                continue;
            }
            let v = &line[3 + key.len()..];
            let end = v.iter().position(|&c| c == 0 || c == b'\n').unwrap_or(v.len());
            return Some(trunc(v[..end].to_vec(), cap));
        }
        None
    }

    fn fs_label(&self, name: &[u8], cap: usize) -> Vec<u8> {
        match self.udev_prop(name, b"ID_FS_LABEL_ENC", 512) {
            Some(v) => unescape_udev(&v, cap),
            None => self.udev_prop(name, b"ID_FS_LABEL", cap).unwrap_or_default(),
        }
    }

    /// Calls f for each mount until it returns true.
    fn each_mount(&self, mut f: impl FnMut(&Mnt) -> bool) {
        let Ok(data) = fs::read(self.path(&[b"/proc/self/mountinfo"])) else { return };
        for line in chunks(&data, MOUNTINFO_LINE) {
            if let Some(m) = parse_mnt(line) {
                if f(&m) {
                    break;
                }
            }
        }
    }

    /// The mount points of a device joined by newlines, and how many there are. With
    /// `cap`, a point is only added while it fits in a buffer of that size, but it
    /// is counted either way (and the newline separator follows the count).
    fn mounts(&self, name: &[u8], cap: Option<usize>) -> (Vec<u8>, i32) {
        let Some((qa, qi)) = self.dev_numbers(name) else { return (Vec::new(), 0) };
        let mut out = Vec::new();
        let mut count = 0;
        self.each_mount(|m| {
            if mount_dev(m) != Some((qa, qi)) {
                return false;
            }
            if cap.is_some_and(|n| out.len() + m.point.len() + 2 < n) {
                if count != 0 {
                    out.push(b'\n');
                }
                out.extend_from_slice(&m.point);
            }
            count += 1;
            false
        });
        (out, count)
    }

    /// Adds to set (space-separated names) the disks under name: for a partition, its disk;
    /// for dm/md, the disks of its slaves (recursively).
    fn base_disks(&self, name: &[u8], set: &mut Vec<u8>, depth: u32) {
        if depth > 8 || !follows(name) {
            return;
        }
        let name = self.parent(name).unwrap_or_else(|| z(name).to_vec());
        let mut slaves = 0;
        if let Ok(rd) = fs::read_dir(self.path(&[b"/sys/class/block/", &name, b"/slaves"])) {
            for e in rd.flatten() {
                let n = e.file_name();
                if n.as_bytes().first() == Some(&b'.') {
                    continue;
                }
                slaves += 1;
                self.base_disks(n.as_bytes(), set, depth + 1);
            }
        }
        if slaves != 0 {
            return;
        }
        let key = [b" ", name.as_slice(), b" "].concat();
        if find(set, &key).is_none() && set.len() + name.len() + 2 < SET_CAP {
            set.extend_from_slice(&name);
            set.push(b' ');
        }
    }

    fn name_of_dev(&self, ma: u32, mi: u32) -> Vec<u8> {
        let Ok(real) = fs::canonicalize(self.path(&[format!("/sys/dev/block/{ma}:{mi}").as_bytes()])) else { return Vec::new() };
        base_of(&real).filter(|b| follows(b)).unwrap_or_default()
    }

    /// " sda nvme0n1 " — the disks that hold /, /usr, /var, /home, /boot, the live medium or swap.
    fn system_set(&self) -> Vec<u8> {
        let mut set = b" ".to_vec();
        self.each_mount(|m| {
            if !SYSTEM_POINTS.contains(&m.point.as_slice()) {
                return false;
            }
            if let Some((ma, mi)) = mount_dev(m) {
                let name = self.name_of_dev(ma, mi);
                if !name.is_empty() {
                    self.base_disks(&name, &mut set, 0);
                }
            }
            false
        });
        // active swaps (the first line is the header)
        if let Ok(data) = fs::read(self.path(&[b"/proc/swaps"])) {
            for line in chunks(&data, SWAPS_LINE).skip(1) {
                let Some(file) = Scan::new(line).word(PATH_MAX - 1) else { continue };
                if let Some((ma, mi)) = blockdev(&unescape_octal(file)) {
                    let name = self.name_of_dev(ma, mi);
                    if !name.is_empty() {
                        self.base_disks(&name, &mut set, 0);
                    }
                }
            }
        }
        trunc(set, SET_CAP)
    }

    fn is_system(&self, disk: &[u8]) -> bool {
        if !valid_name(disk) {
            return true;
        }
        find(&self.system_set(), &[b" ", z(disk), b" "].concat()).is_some()
    }

    fn is_removable(&self, disk: &[u8]) -> bool {
        if self.sysfs(disk, b"removable", 16).is_some_and(|b| z(&b) == b"1") {
            return true;
        }
        if self.udev_prop(disk, b"ID_BUS", 32).is_some_and(|b| b == b"usb") {
            return true;
        }
        if self.udev_prop(disk, b"ID_PATH", 256).is_some_and(|b| find(&b, b"-usb-").is_some()) {
            return true;
        }
        z(disk).starts_with(b"mmcblk")
    }

    fn transport(&self, name: &[u8]) -> Vec<u8> {
        let bus = self.udev_prop(name, b"ID_BUS", 32).unwrap_or_default();
        let path = self.udev_prop(name, b"ID_PATH", 256).unwrap_or_default();
        let name = z(name);
        let t: &[u8] = if bus == b"usb" || find(&path, b"-usb-").is_some() {
            b"usb"
        } else if name.starts_with(b"nvme") {
            b"nvme"
        } else if name.starts_with(b"mmcblk") {
            b"mmc"
        } else if name.starts_with(b"loop") {
            b"loop"
        } else if name.starts_with(b"vd") {
            b"virtio"
        } else if bus == b"ata" || find(&path, b"-ata-").is_some() {
            b"sata"
        } else if !bus.is_empty() {
            &bus
        } else {
            b"scsi"
        };
        t.to_vec()
    }

    /// The model: sysfs, else udev; a loop device shows its backing file.
    fn model(&self, name: &[u8], cap: usize) -> Vec<u8> {
        let mut model = match self.sysfs(name, b"device/model", cap) {
            Some(m) => z(&m).to_vec(),
            None => self.udev_prop(name, b"ID_MODEL", cap).unwrap_or_default(),
        };
        if z(name).starts_with(b"loop") {
            // read_small clears the buffer even when the file is missing
            model = self
                .read_small(&self.path(&[b"/sys/block/", z(name), b"/loop/backing_file"]), cap)
                .map_or_else(Vec::new, |b| z(&b).to_vec());
        }
        model
    }
}

// ---- JSON ----

/// Length of the valid UTF-8 sequence at the start of s (0 if there is none). The ranges are
/// RFC 3629's: no overlong forms, no surrogates, nothing above U+10FFFF.
fn utf8_len(s: &[u8]) -> usize {
    let Some(&c) = s.first() else { return 0 };
    let (n, lo, hi) = match c {
        0x00..=0x7f => return 1,
        0xc2..=0xdf => (2, 0x80, 0xbf),
        0xe0 => (3, 0xa0, 0xbf),
        0xed => (3, 0x80, 0x9f),
        0xe1..=0xef => (3, 0x80, 0xbf),
        0xf0 => (4, 0x90, 0xbf),
        0xf4 => (4, 0x80, 0x8f),
        0xf1..=0xf3 => (4, 0x80, 0xbf),
        _ => return 0,
    };
    if s.len() < n || !(lo..=hi).contains(&s[1]) || s[2..n].iter().any(|b| b & 0xc0 != 0x80) {
        return 0;
    }
    n
}

struct Js(Vec<u8>);

impl Js {
    fn raw(&mut self, s: &str) {
        self.0.extend_from_slice(s.as_bytes());
    }
    /// `"key":"value",`. Quotes, backslashes and control characters are escaped as the C code did.
    /// Unlike it, a byte that is not UTF-8 becomes U+FFFD: QuickJS refuses the whole list otherwise
    /// ("SyntaxError: invalid UTF-8 sequence"), and a FAT label in a legacy code page, which udev
    /// hands over as \xE9, was enough to make every disk disappear from the Disks app.
    fn str(&mut self, key: &str, s: &[u8]) {
        self.0.push(b'"');
        self.0.extend_from_slice(key.as_bytes());
        self.0.extend_from_slice(b"\":\"");
        let s = z(s);
        let mut i = 0;
        while i < s.len() {
            let c = s[i];
            match c {
                b'"' | b'\\' => self.0.extend_from_slice(&[b'\\', c]),
                0..=0x1f => self.0.extend_from_slice(format!("\\u{:04x}", c).as_bytes()),
                0x20..=0x7f => self.0.push(c),
                _ => {
                    let k = utf8_len(&s[i..]);
                    if k == 0 {
                        self.0.extend_from_slice("\u{fffd}".as_bytes());
                    } else {
                        self.0.extend_from_slice(&s[i..i + k]);
                        i += k - 1;
                    }
                }
            }
            i += 1;
        }
        self.0.extend_from_slice(b"\",");
    }
}

fn skip_disk(fs: &Fs, n: &[u8]) -> bool {
    const PRE: [&[u8]; 9] = [b"ram", b"zram", b"dm-", b"md", b"sr", b"fd", b"nbd", b"mtdblock", b"zd"];
    if PRE.iter().any(|p| n.starts_with(p)) {
        return true;
    }
    if n.starts_with(b"loop") {
        // only loops backed by a file (tests, images)
        return !access(&fs.path(&[b"/sys/block/", n, b"/loop/backing_file"]), libc::F_OK);
    }
    false
}

/// File system and mount fields of a device (disk or partition).
fn fs_fields(fs: &Fs, b: &mut Js, name: &[u8]) {
    let prop = |k: &[u8]| fs.udev_prop(name, k, 512).unwrap_or_default();
    b.str("fstype", &prop(b"ID_FS_TYPE"));
    b.str("label", &fs.fs_label(name, 256));
    b.str("uuid", &prop(b"ID_FS_UUID"));
    b.str("fsversion", &prop(b"ID_FS_VERSION"));
    let (mounts, _) = fs.mounts(name, Some(2048));
    let first = &mounts[..mounts.iter().position(|&c| c == b'\n').unwrap_or(mounts.len())];
    b.str("mountpoint", first);
    let point = z(first);
    // SAFETY: statvfs fills a caller-owned struct.
    let mut sv: libc::statvfs = unsafe { std::mem::zeroed() };
    let ok = !point.is_empty()
        && std::ffi::CString::new(point).is_ok_and(|c| unsafe { libc::statvfs(c.as_ptr(), &mut sv) } == 0);
    if ok {
        let fr = sv.f_frsize as u64;
        b.raw(&format!(
            "\"used\":{},\"avail\":{},\"fssize\":{},",
            (sv.f_blocks as u64).wrapping_sub(sv.f_bfree as u64).wrapping_mul(fr),
            (sv.f_bavail as u64).wrapping_mul(fr),
            (sv.f_blocks as u64).wrapping_mul(fr)
        ));
    } else {
        b.raw("\"used\":null,\"avail\":null,\"fssize\":null,");
    }
}

fn list_json(fs: &Fs) -> Option<Vec<u8>> {
    let rd = fs::read_dir(fs.path(&[b"/sys/block"])).ok()?;
    let sys = fs.system_set();
    let mut b = Js(b"[".to_vec());
    let mut first = true;
    for e in rd.flatten() {
        let n = e.file_name();
        let n = n.as_bytes();
        if n.first() == Some(&b'.') || !valid_name(n) || skip_disk(fs, n) {
            continue;
        }
        let size = fs.size(n);
        if size == 0 {
            continue;
        }
        b.raw(if first { "{" } else { ",{" });
        first = false;
        b.str("name", n);
        b.str("model", &fs.model(n, 256));
        let vendor = fs.sysfs(n, b"device/vendor", 128).unwrap_or_default();
        b.str("vendor", &vendor);
        b.str("serial", &fs.udev_prop(n, b"ID_SERIAL_SHORT", 512).unwrap_or_default());
        b.str("transport", &fs.transport(n));
        b.str("table", &fs.udev_prop(n, b"ID_PART_TABLE_TYPE", 512).unwrap_or_default());
        let flag = |rel: &[u8]| fs.sysfs(n, rel, 8).is_some_and(|v| z(&v) == b"1");
        let tf = |v: bool| if v { "true" } else { "false" };
        b.raw(&format!(
            "\"size\":{},\"removable\":{},\"ro\":{},\"rotational\":{},\"system\":{},",
            size,
            tf(fs.is_removable(n)),
            tf(flag(b"ro")),
            tf(flag(b"queue/rotational")),
            tf(find(&sys, &[b" ", &n[..n.len().min(31)], b" "].concat()).is_some())
        ));
        fs_fields(fs, &mut b, n);
        // partitions
        let mut parts: Vec<(Vec<u8>, i32)> = Vec::new();
        if let Ok(pd) = fs::read_dir(fs.path(&[b"/sys/block/", n])) {
            for pe in pd.flatten() {
                if parts.len() >= 128 {
                    break;
                }
                let pn = pe.file_name();
                let pn = pn.as_bytes();
                if !valid_name(pn) {
                    continue;
                }
                let num = fs.read_num(&fs.path(&[b"/sys/block/", n, b"/", pn, b"/partition"]));
                if num <= 0 {
                    continue;
                }
                parts.push((pn[..pn.len().min(31)].to_vec(), num as i32));
            }
        }
        parts.sort_by_key(|p| p.1);
        b.raw("\"partitions\":[");
        for (i, (pn, number)) in parts.iter().enumerate() {
            b.raw(if i != 0 { ",{" } else { "{" });
            b.str("name", pn);
            let label = unescape_udev(&fs.udev_prop(pn, b"ID_PART_ENTRY_NAME", 512).unwrap_or_default(), 256);
            b.str("partlabel", &label);
            b.str("parttype", &fs.udev_prop(pn, b"ID_PART_ENTRY_TYPE", 512).unwrap_or_default());
            fs_fields(fs, &mut b, pn);
            let start = fs.read_num(&fs.path(&[b"/sys/block/", &n[..n.len().min(31)], b"/", pn, b"/start"]));
            b.raw(&format!(
                "\"number\":{},\"size\":{},\"start\":{}}}",
                number,
                fs.size(pn),
                if start < 0 { 0 } else { start.wrapping_mul(512) }
            ));
        }
        b.raw("]}");
    }
    b.raw("]");
    Some(b.0)
}

fn tools_json() -> String {
    let ok = |p: &str| std::ffi::CString::new(p).is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0) as i32;
    format!(
        "{{\"fat32\":{},\"exfat\":{},\"ext4\":{},\"ntfs\":{},\"smartctl\":{}}}",
        ok("/usr/sbin/mkfs.fat"),
        ok("/usr/sbin/mkfs.exfat"),
        ok("/usr/sbin/mkfs.ext4"),
        ok("/usr/sbin/mkfs.ntfs"),
        ok("/usr/sbin/smartctl")
    )
}

unsafe fn malloc_copy(s: &[u8]) -> *mut c_char {
    let p = libc::malloc(s.len() + 1).cast::<u8>();
    if !p.is_null() {
        std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        *p.add(s.len()) = 0;
    }
    p.cast()
}

// ---- notifications: mountinfo (POLLPRI) + udev (netlink, group 2) in a single epoll ----

static EP: AtomicI32 = AtomicI32::new(-1);
static MFD: AtomicI32 = AtomicI32::new(-1);
static UFD: AtomicI32 = AtomicI32::new(-1);
static LAST_MOUNTS: AtomicU64 = AtomicU64::new(0);

/// Fingerprint of mountinfo (FNV-1a). Polling /proc/self/mountinfo "consumes" the event: the
/// main loop's poll on the epoll already uses it up and epoll_wait then doesn't report it. So
/// on every notification the content (a few KB) is compared with the previous one.
fn mounts_hash(fs: &Fs) -> u64 {
    let Ok(mut f) = fs::File::open(fs.path(&[b"/proc/self/mountinfo"])) else { return 0 };
    let mut h: u64 = 1469598103934665603;
    let mut buf = [0u8; 8192];
    while let Ok(r) = f.read(&mut buf) {
        if r == 0 {
            break;
        }
        for &c in &buf[..r] {
            h = (h ^ c as u64).wrapping_mul(1099511628211);
        }
    }
    h
}

unsafe fn epoll_add(ep: c_int, fd: c_int, events: u32) {
    let mut ev = libc::epoll_event { events, u64: fd as u64 };
    libc::epoll_ctl(ep, libc::EPOLL_CTL_ADD, fd, &mut ev);
}

// ---- C ABI ----

#[no_mangle]
pub unsafe extern "C" fn disk_valid_name(name: *const c_char) -> c_int {
    valid_name(cstr(name)) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn disk_parent(name: *const c_char, disk: *mut c_char, n: usize) -> c_int {
    match fs().parent(cstr(name)) {
        Some(d) => {
            put(disk, n, &d);
            1
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn disk_size(name: *const c_char) -> u64 {
    fs().size(cstr(name))
}

#[no_mangle]
pub unsafe extern "C" fn disk_udev_prop(name: *const c_char, key: *const c_char, out: *mut c_char, n: usize) -> c_int {
    put(out, n, b"");
    match fs().udev_prop(cstr(name), cstr(key), n) {
        Some(v) => {
            put(out, n, &v);
            0
        }
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn disk_fs_label(name: *const c_char, out: *mut c_char, n: usize) {
    put(out, n, &fs().fs_label(cstr(name), n));
}

#[no_mangle]
pub unsafe extern "C" fn disk_mounts(name: *const c_char, out: *mut c_char, n: usize) -> c_int {
    put(out, n, b"");
    let (text, count) = fs().mounts(cstr(name), if out.is_null() { None } else { Some(n) });
    put(out, n, &text);
    count
}

#[no_mangle]
pub unsafe extern "C" fn disk_is_system(disk: *const c_char) -> c_int {
    fs().is_system(cstr(disk)) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn disk_is_removable(disk: *const c_char) -> c_int {
    fs().is_removable(cstr(disk)) as c_int
}

/// The disk's identity as lord.disks.list() shows it: the model (sysfs or udev; a loop
/// device's backing file) and the short serial (udev, may be empty).
#[no_mangle]
pub unsafe extern "C" fn disk_identity(name: *const c_char, model: *mut c_char, mn: usize, serial: *mut c_char, sn: usize) -> c_int {
    let name = cstr(name);
    if !valid_name(name) || mn == 0 || sn == 0 {
        return -1;
    }
    let fs = fs();
    put(model, mn, &fs.model(name, mn));
    put(serial, sn, &fs.udev_prop(name, b"ID_SERIAL_SHORT", sn).unwrap_or_default());
    0
}

#[no_mangle]
pub unsafe extern "C" fn disk_list_json() -> *mut c_char {
    match list_json(&fs()) {
        Some(j) => malloc_copy(&j),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn disk_tools_json() -> *mut c_char {
    malloc_copy(tools_json().as_bytes())
}

#[no_mangle]
pub extern "C" fn disk_watch_fd() -> c_int {
    EP.load(Relaxed)
}

#[no_mangle]
pub unsafe extern "C" fn disk_watch(on: c_int) -> c_int {
    if on == 0 {
        for a in [&EP, &MFD, &UFD] {
            let fd = a.swap(-1, Relaxed);
            if fd >= 0 {
                libc::close(fd);
            }
        }
        return 0;
    }
    if EP.load(Relaxed) >= 0 {
        return 0;
    }
    let ep = libc::epoll_create1(libc::EPOLL_CLOEXEC);
    if ep < 0 {
        return -1;
    }
    EP.store(ep, Relaxed);
    let fs = fs();
    LAST_MOUNTS.store(mounts_hash(&fs), Relaxed);
    let mp = fs.path(&[b"/proc/self/mountinfo"]);
    let mfd = std::ffi::CString::new(mp.as_os_str().as_bytes()).map_or(-1, |c| libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC));
    MFD.store(mfd, Relaxed);
    if mfd >= 0 {
        epoll_add(ep, mfd, (libc::EPOLLPRI | libc::EPOLLERR) as u32);
    }
    // events udevd sends after processing the device (its database is already up to date)
    let ufd = libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, libc::NETLINK_KOBJECT_UEVENT);
    UFD.store(ufd, Relaxed);
    if ufd >= 0 {
        let mut sa: libc::sockaddr_nl = std::mem::zeroed();
        sa.nl_family = libc::AF_NETLINK as u16;
        sa.nl_groups = 2;
        if libc::bind(ufd, (&sa as *const libc::sockaddr_nl).cast(), std::mem::size_of::<libc::sockaddr_nl>() as u32) != 0 {
            libc::close(ufd);
            UFD.store(-1, Relaxed);
        } else {
            epoll_add(ep, ufd, libc::EPOLLIN as u32);
        }
    }
    0
}

/// Reads what arrived; 1 if disks or mounts changed (JS must be told).
#[no_mangle]
pub unsafe extern "C" fn disk_watch_dispatch() -> c_int {
    let ep = EP.load(Relaxed);
    if ep < 0 {
        return 0;
    }
    let ufd = UFD.load(Relaxed);
    let mut evs = [libc::epoll_event { events: 0, u64: 0 }; 4];
    let n = libc::epoll_wait(ep, evs.as_mut_ptr(), 4, 0);
    let mut changed = false;
    for ev in evs.iter().take(n.max(0) as usize) {
        let data = ev.u64;
        if data as u32 as i32 != ufd {
            continue;
        }
        let mut buf = [0u8; 8192];
        loop {
            let r = libc::recv(ufd, buf.as_mut_ptr().cast(), buf.len(), 0);
            if r <= 0 {
                break;
            }
            if find(&buf[..r as usize], b"SUBSYSTEM=block").is_some() {
                changed = true;
            }
        }
    }
    let h = mounts_hash(&fs());
    if h != LAST_MOUNTS.load(Relaxed) {
        LAST_MOUNTS.store(h, Relaxed);
        changed = true;
    }
    changed as c_int
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::AtomicUsize;

    static N: AtomicUsize = AtomicUsize::new(0);

    /// A synthetic /sys, /proc and /run/udev tree in a temporary directory.
    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new() -> Tree {
            let root = std::env::temp_dir().join(format!("axia-disk-test-{}-{}", std::process::id(), N.fetch_add(1, Relaxed)));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Tree { root }
        }
        fn fs(&self) -> Fs {
            Fs { root: self.root.as_os_str().as_bytes().to_vec() }
        }
        fn at(&self, rel: &str) -> PathBuf {
            let p = self.root.join(rel.trim_start_matches('/'));
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            p
        }
        fn file(&self, rel: &str, data: impl AsRef<[u8]>) {
            fs::write(self.at(rel), data).unwrap();
        }
        fn link(&self, target: &str, rel: &str) {
            symlink(target, self.at(rel)).unwrap();
        }
        /// A whole disk the way sysfs lays it out: the real directory under devices/, symlinked
        /// from block/, class/block/ and dev/block/<major>:<minor>.
        fn disk(&self, name: &str, maj: u32, min: u32, sectors: u64) {
            let base = format!("sys/devices/pci0/block/{name}");
            self.file(&format!("{base}/dev"), format!("{maj}:{min}\n"));
            self.file(&format!("{base}/size"), format!("{sectors}\n"));
            self.file(&format!("{base}/removable"), "0\n");
            self.file(&format!("{base}/ro"), "0\n");
            self.file(&format!("{base}/queue/rotational"), "0\n");
            self.link(&format!("../devices/pci0/block/{name}"), &format!("sys/block/{name}"));
            self.link(&format!("../../devices/pci0/block/{name}"), &format!("sys/class/block/{name}"));
            self.link(&format!("../../devices/pci0/block/{name}"), &format!("sys/dev/block/{maj}:{min}"));
        }
        fn part(&self, disk: &str, name: &str, number: u32, dev: (u32, u32), sectors: u64, start: u64) {
            let (maj, min) = dev;
            let base = format!("sys/devices/pci0/block/{disk}/{name}");
            self.file(&format!("{base}/dev"), format!("{maj}:{min}\n"));
            self.file(&format!("{base}/size"), format!("{sectors}\n"));
            self.file(&format!("{base}/partition"), format!("{number}\n"));
            self.file(&format!("{base}/start"), format!("{start}\n"));
            self.link(&format!("../../devices/pci0/block/{disk}/{name}"), &format!("sys/class/block/{name}"));
            self.link(&format!("../../devices/pci0/block/{disk}/{name}"), &format!("sys/dev/block/{maj}:{min}"));
        }
        fn udev(&self, maj: u32, min: u32, text: impl AsRef<[u8]>) {
            self.file(&format!("run/udev/data/b{maj}:{min}"), text);
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn s(v: &[u8]) -> String {
        String::from_utf8_lossy(v).into_owned()
    }

    #[test]
    fn names_follow_the_kernel() {
        for g in ["sda", "sda1", "nvme0n1", "nvme0n1p2", "mmcblk0p1", "vdb", "loop3", "a", "abcdefghijklmnopqrstuvwxyzabcde"] {
            assert!(valid_name(g.as_bytes()), "accepts {g}");
        }
        for b in ["", "1sda", "SDA", "sd-a", "dm-0", "sda/", "../sda", "sda ", "sda\n", "abcdefghijklmnopqrstuvwxyzabcdef", "sd\u{e1}"] {
            assert!(!valid_name(b.as_bytes()), "refuses {b:?}");
        }
        assert!(valid_name(b"sda\0trailing"), "a C string ends at its NUL");
    }

    #[test]
    fn numbers_and_scanning_follow_libc() {
        assert_eq!(atoll(b"  12abc"), 12);
        assert_eq!(atoll(b"-7"), -7);
        assert_eq!(atoll(b"+5"), 5);
        assert_eq!(atoll(b"abc"), 0);
        assert_eq!(atoll(b"99999999999999999999999"), i64::MAX);
        assert_eq!(atoll(b"-99999999999999999999999"), i64::MIN);
        let mut sc = Scan::new(b" 8:\t1 rest");
        assert_eq!((sc.num(), sc.lit(b':'), sc.num()), (Some(8), true, Some(1)));
        assert_eq!(sc.word(2), Some(&b"re"[..]), "%2s stops after two characters");
        assert_eq!(sc.word(8), Some(&b"st"[..]), "and the next one continues from there");
        assert_eq!(Scan::new(b"x").num(), None);
        assert_eq!(Scan::new(b"+").num(), None);
        assert_eq!(Scan::new(b"-1").num(), Some(u64::MAX), "a negative %u wraps like strtoul");
    }

    #[test]
    fn fgets_pieces() {
        let data = b"ab\ncd\n\nlast";
        assert_eq!(chunks(data, 1024).collect::<Vec<_>>(), [&b"ab\n"[..], b"cd\n", b"\n", b"last"]);
        // at most cap-1 bytes per piece, so a long line comes back in several
        assert_eq!(chunks(b"abcdefg\nxy", 4).collect::<Vec<_>>(), [&b"abc"[..], b"def", b"g\n", b"xy"]);
    }

    #[test]
    fn read_small_reads_once_and_trims() {
        let t = Tree::new();
        t.file("sys/x/model", "SATA SSD        \n");
        t.file("sys/x/big", "0123456789\n");
        t.file("sys/x/empty", "");
        let f = t.fs();
        assert_eq!(f.read_small(&f.path(&[b"/sys/x/model"]), 256).unwrap(), b"SATA SSD");
        assert_eq!(f.read_small(&f.path(&[b"/sys/x/big"]), 5).unwrap(), b"0123", "a buffer of 5 holds 4 bytes");
        assert_eq!(f.read_small(&f.path(&[b"/sys/x/empty"]), 8).unwrap(), b"");
        assert!(f.read_small(&f.path(&[b"/sys/x/missing"]), 8).is_none());
        assert_eq!(f.read_num(&f.path(&[b"/sys/x/missing"])), -1);
    }

    #[test]
    fn udev_properties() {
        let t = Tree::new();
        t.disk("sda", 8, 0, 100);
        t.udev(
            8,
            0,
            "S:disk/by-id/x\nE:ID_FS_TYPE_X=nope\nE:ID_FS_TYPE=ext4\nE:ID_BUS\n\
             E:ID_PATH=pci-0000:00:17.0-ata-1\nE:ID_MODEL=a\0b\nE:ID_FS_UUID=\n",
        );
        let f = t.fs();
        assert_eq!(f.udev_prop(b"sda", b"ID_FS_TYPE", 512).unwrap(), b"ext4", "an exact key, not a longer one");
        assert_eq!(f.udev_prop(b"sda", b"ID_FS_TYPE", 3).unwrap(), b"ex", "cut to the buffer");
        assert_eq!(f.udev_prop(b"sda", b"ID_FS_UUID", 512).unwrap(), b"", "present but empty is not missing");
        assert!(f.udev_prop(b"sda", b"ID_BUS", 512).is_none(), "no '=' is no property");
        assert!(f.udev_prop(b"sda", b"ID_NOPE", 512).is_none());
        assert_eq!(f.udev_prop(b"sda", b"ID_MODEL", 512).unwrap(), b"a", "the value stops at a NUL");
        assert!(f.udev_prop(b"nothere", b"ID_FS_TYPE", 512).is_none());
        // the C code read the file with a 1024-byte fgets: a line that long is split, and the
        // second piece is looked at as a line of its own
        let mut long = b"E:ID_PATH=".to_vec();
        long.resize(1023, b'y');
        long.extend_from_slice(b"E:ID_BUS=usb\n");
        t.udev(8, 0, &long);
        assert_eq!(f.udev_prop(b"sda", b"ID_BUS", 512).unwrap(), b"usb");
    }

    #[test]
    fn labels_are_decoded() {
        let t = Tree::new();
        t.disk("sda", 8, 0, 100);
        t.part("sda", "sda1", 1, (8, 1), 50, 2048);
        t.udev(8, 0, "E:ID_FS_LABEL_ENC=Caf\\xc3\\xa9\\x20\\x41\\xE9\\x00zzz\nE:ID_FS_LABEL=plain\n");
        t.udev(8, 1, "E:ID_FS_LABEL=plain only\n");
        let f = t.fs();
        // \xNN of either case decoded, the result of \x00 ends the string
        assert_eq!(z(&f.fs_label(b"sda", 256)), b"Caf\xc3\xa9 A\xe9");
        assert_eq!(f.fs_label(b"sda1", 256), b"plain only", "without the encoded form the plain label is used");
        assert_eq!(f.fs_label(b"sda", 4), b"Caf", "cut to the buffer");
        assert_eq!(unescape_udev(b"a\\x4", 16), b"a\\x4", "an incomplete escape stays as it is");
        assert_eq!(unescape_udev(b"\\\\x41", 16), b"\\A", "a backslash then \\x41");
    }

    #[test]
    fn mountinfo_lines() {
        let m = parse_mnt(b"36 35 8:1 / /media/user/USB\\040STICK rw,nosuid shared:1 - vfat /dev/sda1 rw,uid=1000\n").unwrap();
        assert_eq!((m.maj, m.min), (8, 1));
        assert_eq!(m.point, b"/media/user/USB STICK");
        assert_eq!(m.source, b"/dev/sda1");
        // blanks between fields are free, the colon of major:minor is not
        assert!(parse_mnt(b"1  2 \t 8:1 /  /x rw - ext4 /dev/sda1").is_some());
        assert!(parse_mnt(b"1 2 8 1 / /x rw - ext4 /dev/sda1").is_none());
        assert!(parse_mnt(b"1 2 8;1 / /x rw - ext4 /dev/sda1").is_none());
        assert!(parse_mnt(b"1 2 8:1 / /x rw ext4 /dev/sda1").is_none(), "no ' - ' separator");
        assert!(parse_mnt(b"1 2 8:1 / /x rw - ext4").is_none(), "no source");
        assert!(parse_mnt(b"garbage").is_none());
        // %31s cuts a long file system name; the rest of it is read as the source
        let long = format!("1 2 8:1 / /x rw - {} /dev/sda1", "f".repeat(40));
        let m = parse_mnt(long.as_bytes()).unwrap();
        assert_eq!(m.source, "f".repeat(9).as_bytes());
        // octal escapes: \134 is a backslash, a NUL ends the name, out-of-range ones stay
        assert_eq!(unescape_octal(b"/a\\134b"), b"/a\\b");
        assert_eq!(unescape_octal(b"/a\\000b"), b"/a");
        assert_eq!(unescape_octal(b"/x\\400"), b"/x\\400");
        assert_eq!(unescape_octal(b"/x\\1"), b"/x\\1");
    }

    #[test]
    fn mounts_of_a_device() {
        let t = Tree::new();
        t.disk("sda", 8, 0, 1000);
        t.part("sda", "sda1", 1, (8, 1), 500, 2048);
        t.file(
            "proc/self/mountinfo",
            "1 0 8:1 / /mnt/a rw - ext4 /dev/sda1 rw\n2 0 8:1 /sub /mnt/b rw - ext4 /dev/sda1 rw\n\
             3 0 8:2 / /mnt/c rw - ext4 /dev/sda2 rw\n4 0 0:5 / /mnt/d rw - btrfs /dev/null rw\n",
        );
        let f = t.fs();
        assert_eq!(f.mounts(b"sda1", Some(4096)), (b"/mnt/a\n/mnt/b".to_vec(), 2));
        assert_eq!(f.mounts(b"sda1", None), (Vec::new(), 2), "counting only");
        assert_eq!(f.mounts(b"sda", Some(4096)), (Vec::new(), 0));
        assert_eq!(f.mounts(b"nothere", Some(4096)), (Vec::new(), 0));
        // a point is added only if it and a margin of two bytes fit; it is counted either way,
        // and the separator follows the count
        assert_eq!(f.mounts(b"sda1", Some(8)), (Vec::new(), 2));
        assert_eq!(f.mounts(b"sda1", Some(9)), (b"/mnt/a".to_vec(), 2));
        assert_eq!(f.mounts(b"sda1", Some(14)), (b"/mnt/a".to_vec(), 2), "6 + 6 + 2 must be below the size");
        assert_eq!(f.mounts(b"sda1", Some(15)), (b"/mnt/a\n/mnt/b".to_vec(), 2));
    }

    #[test]
    fn the_system_disk() {
        let t = Tree::new();
        t.disk("sda", 8, 0, 1000);
        t.part("sda", "sda2", 2, (8, 2), 500, 2048);
        t.disk("sdb", 8, 16, 1000);
        t.disk("sdc", 8, 32, 1000);
        t.part("sdc", "sdc1", 1, (8, 33), 500, 2048);
        t.disk("sdd", 8, 48, 1000);
        t.disk("sde", 8, 64, 1000);
        // dm-0 (LVM) on top of two partitions, mounted at /home
        t.disk("dm-0", 253, 0, 1000);
        t.link("../../../../../devices/pci0/block/sdc/sdc1", "sys/class/block/dm-0/slaves/sdc1");
        t.file(
            "proc/self/mountinfo",
            "1 0 8:2 / / rw - ext4 /dev/sda2 rw\n2 0 8:16 / /data rw - ext4 /dev/sdb rw\n3 0 253:0 / /home rw - ext4 /dev/mapper/x rw\n",
        );
        let f = t.fs();
        let set = f.system_set();
        assert_eq!(s(&set), " sda sdc ", "the partition's disk, and the disk under the dm device; /data is not a system point");
        assert!(f.is_system(b"sda") && f.is_system(b"sdc"));
        assert!(!f.is_system(b"sdb") && !f.is_system(b"sdd") && !f.is_system(b"sde"));
        assert!(f.is_system(b"not a name"), "anything that is not a block device name is refused");
        assert!(f.is_system(b"dm-0"));
        // swaps: the header is skipped; a regular file or a missing one holds no disk
        t.file("proc/swaps", "Filename\t\tType\tSize\tUsed\tPriority\n/swapfile\tfile\t10\t0\t-2\n/dev/nothere\tpartition\t10\t0\t-2\n");
        assert_eq!(s(&f.system_set()), " sda sdc ");
    }

    #[test]
    fn partitions_parent_and_removable() {
        let t = Tree::new();
        t.disk("sda", 8, 0, 1000);
        t.part("sda", "sda1", 1, (8, 1), 500, 2048);
        t.disk("sdb", 8, 16, 1000);
        t.file("sys/devices/pci0/block/sdb/removable", "1\n");
        t.disk("sdc", 8, 32, 1000);
        t.udev(8, 32, "E:ID_BUS=usb\n");
        t.disk("sdd", 8, 48, 1000);
        t.udev(8, 48, "E:ID_PATH=pci-0000:00:14.0-usb-0:1:1.0-scsi-0:0:0:0\n");
        t.disk("mmcblk0", 179, 0, 1000);
        let f = t.fs();
        assert_eq!(f.parent(b"sda1").unwrap(), b"sda");
        assert!(f.parent(b"sda").is_none(), "a whole disk has no parent");
        assert!(f.parent(b"nothere").is_none());
        assert!(!f.is_removable(b"sda"));
        assert!(f.is_removable(b"sdb"), "removable=1");
        assert!(f.is_removable(b"sdc"), "over USB");
        assert!(f.is_removable(b"sdd"), "a USB path");
        assert!(f.is_removable(b"mmcblk0"), "a card");
        assert_eq!(f.size(b"sda"), 1000 * 512);
        assert_eq!(f.size(b"nothere"), 0);
    }

    #[test]
    fn the_list() {
        let t = Tree::new();
        t.disk("sda", 8, 0, 2_000_000);
        t.file("sys/devices/pci0/block/sda/device/model", "Samsung SSD 980   \n");
        t.file("sys/devices/pci0/block/sda/device/vendor", "ATA     \n");
        t.udev(8, 0, "E:ID_SERIAL_SHORT=S123\nE:ID_PART_TABLE_TYPE=gpt\nE:ID_PATH=pci-0000:00:17.0-ata-1\n");
        // partition numbers decide the order, not the directory order; one is hidden from the list by number 0
        t.part("sda", "sda2", 2, (8, 2), 1000, 4096);
        t.part("sda", "sda1", 1, (8, 1), 1000, 2048);
        t.part("sda", "sda9", 0, (8, 9), 1000, 8192);
        t.udev(
            8,
            1,
            "E:ID_FS_TYPE=vfat\nE:ID_FS_LABEL_ENC=EFI\\x20\"q\\\\\nE:ID_FS_UUID=AAAA-BBBB\n\
             E:ID_PART_ENTRY_NAME=EFI\\x20System\nE:ID_PART_ENTRY_TYPE=c12a\n",
        );
        t.udev(8, 2, "E:ID_FS_TYPE=ext4\nE:ID_FS_VERSION=1.0\n");
        // not listed: ram disks, dm, a loop device without a backing file, an empty disk
        t.disk("zram0", 251, 0, 1000);
        t.disk("dm-0", 253, 0, 1000);
        t.disk("loop0", 7, 0, 1000);
        t.disk("loop1", 7, 1, 1000);
        t.file("sys/devices/pci0/block/loop1/loop/backing_file", "/home/u/disk.img\n");
        t.disk("sdz", 8, 400, 0);
        t.file(
            "proc/self/mountinfo",
            "1 0 8:1 / /proc rw - vfat /dev/sda1 rw\n2 0 8:2 / /nonexistent-axia rw - ext4 /dev/sda2 rw\n\
             3 0 8:2 / /second rw - ext4 /dev/sda2 rw\n",
        );
        let json = list_json(&t.fs()).unwrap();
        let j = s(&json);
        assert!(j.starts_with("[{\"name\":\"loop1\",") || j.starts_with("[{\"name\":\"sda\",") || j.contains("{\"name\":\"sda\","), "{j}");
        for gone in ["zram0", "dm-0", "\"loop0\"", "sdz"] {
            assert!(!j.contains(gone), "{gone} is not listed: {j}");
        }
        assert!(j.contains("\"name\":\"loop1\",\"model\":\"/home/u/disk.img\""), "a loop device shows its backing file: {j}");
        assert!(j.contains("\"transport\":\"loop\""), "{j}");
        let sda = &j[j.find("{\"name\":\"sda\"").unwrap()..];
        let head = r#"{"name":"sda","model":"Samsung SSD 980","vendor":"ATA","serial":"S123","transport":"sata","table":"gpt","size":1024000000,"#;
        assert!(sda.starts_with(head), "{sda}");
        assert!(sda.contains("\"removable\":false,\"ro\":false,\"rotational\":false,\"system\":false,"), "{sda}");
        let p1 = sda.find("\"name\":\"sda1\"").unwrap();
        let p2 = sda.find("\"name\":\"sda2\"").unwrap();
        assert!(p1 < p2, "partitions are in number order");
        assert!(!sda.contains("sda9"), "partition number 0 is not a partition");
        // the label is EFI, a space, a quote, q and two backslashes: \x20 decodes, \\ does not
        let p1 = concat!(
            r#""partlabel":"EFI System","parttype":"c12a","fstype":"vfat","label":"EFI \"q\\\\","uuid":"AAAA-BBBB","fsversion":"","#,
            r#""mountpoint":"/proc","used":0,"avail":0,"fssize":0,"number":1,"size":512000,"start":1048576}"#
        );
        assert!(sda.contains(p1), "{sda}");
        let p2 = r#""fsversion":"1.0","mountpoint":"/nonexistent-axia","used":null,"avail":null,"fssize":null,"number":2,"#;
        assert!(sda.contains(p2), "only the first mount point is listed: {sda}");
        assert_eq!(j.matches("\"partitions\":[").count(), 2);
    }

    #[test]
    fn at_most_128_partitions() {
        let t = Tree::new();
        t.disk("sda", 8, 0, 1_000_000);
        for n in 1..=130 {
            t.part("sda", &format!("sda{n}"), n, (8, n), 100, 2048 + n as u64);
        }
        let j = s(&list_json(&t.fs()).unwrap());
        assert_eq!(j.matches("\"partlabel\"").count(), 128);
    }

    #[test]
    fn invalid_utf8_never_reaches_the_json() {
        // The Disks app parses the list with QuickJS, which refuses a byte that is not UTF-8 in a
        // string (SyntaxError: invalid UTF-8 sequence) and then every disk disappears. A FAT label
        // in a legacy code page reaches us as \xE9 in udev's ENC form.
        let t = Tree::new();
        t.disk("sda", 8, 0, 2_000_000);
        t.file("sys/devices/pci0/block/sda/device/model", b"Mod\xe9l \xff\xfe");
        t.udev(8, 0, b"E:ID_SERIAL_SHORT=ser\xc3\n");
        t.part("sda", "sda1", 1, (8, 1), 1000, 2048);
        t.udev(8, 1, "E:ID_FS_LABEL_ENC=CAF\\xc9 \\xe2\\x82\nE:ID_PART_ENTRY_NAME=\\xed\\xa0\\x80\n");
        let json = list_json(&t.fs()).unwrap();
        let j = std::str::from_utf8(&json).expect("the list is valid UTF-8");
        assert!(j.contains("\"model\":\"Mod\u{fffd}l \u{fffd}\u{fffd}\""), "{j}");
        assert!(j.contains("\"serial\":\"ser\u{fffd}\""), "{j}");
        assert!(j.contains("\"label\":\"CAF\u{fffd} \u{fffd}\u{fffd}\""), "{j}");
        assert!(j.contains("\"partlabel\":\"\u{fffd}\u{fffd}\u{fffd}\""), "a UTF-8 encoded surrogate is not valid either: {j}");
        let mut b = Js(Vec::new());
        b.str("k", "caf\u{e9} \u{1f600} \u{fffd} \u{7f}\t\"\\".as_bytes());
        let want = "\"k\":\"caf\u{e9} \u{1f600} \u{fffd} \u{7f}\\u0009\\\"\\\\\",";
        assert_eq!(s(&b.0), want, "valid text, controls and quotes are escaped, nothing else changes");
    }

    #[test]
    fn utf8_sequences_agree_with_std() {
        fn first_char_len(s: &[u8]) -> usize {
            let n = s.len().min(4);
            match std::str::from_utf8(&s[..n]) {
                Ok(t) => t.chars().next().map_or(0, char::len_utf8),
                Err(e) if e.valid_up_to() > 0 => std::str::from_utf8(&s[..e.valid_up_to()]).unwrap().chars().next().unwrap().len_utf8(),
                Err(_) => 0,
            }
        }
        for a in 0..=255u8 {
            assert_eq!(utf8_len(&[a]), first_char_len(&[a]), "{a:#x}");
            for b in 0..=255u8 {
                assert_eq!(utf8_len(&[a, b]), first_char_len(&[a, b]), "{a:#x} {b:#x}");
                for c in [0x00, 0x7f, 0x80, 0x8f, 0x90, 0x9f, 0xa0, 0xbf, 0xc0] {
                    assert_eq!(utf8_len(&[a, b, c]), first_char_len(&[a, b, c]), "{a:#x} {b:#x} {c:#x}");
                    for d in [0x80, 0xbf, 0x7f, 0xc0] {
                        assert_eq!(utf8_len(&[a, b, c, d]), first_char_len(&[a, b, c, d]), "{a:#x} {b:#x} {c:#x} {d:#x}");
                    }
                }
            }
        }
        assert_eq!(utf8_len(b""), 0);
    }

    #[test]
    fn identity_and_model() {
        let t = Tree::new();
        t.disk("sda", 8, 0, 1000);
        t.file("sys/devices/pci0/block/sda/device/model", "Disk  \n");
        t.udev(8, 0, "E:ID_MODEL=FromUdev\nE:ID_SERIAL_SHORT=ABC\n");
        t.disk("sdb", 8, 16, 1000);
        t.udev(8, 16, "E:ID_MODEL=FromUdev\n");
        t.disk("loop0", 7, 0, 1000);
        t.file("sys/devices/pci0/block/loop0/device/model", "ignored\n");
        let f = t.fs();
        assert_eq!(f.model(b"sda", 256), b"Disk", "sysfs first");
        assert_eq!(f.model(b"sdb", 256), b"FromUdev", "then udev");
        assert_eq!(f.model(b"loop0", 256), b"", "a loop device shows its backing file, or nothing");
        assert_eq!(f.model(b"sda", 3), b"Di");
        assert_eq!(f.udev_prop(b"sda", b"ID_SERIAL_SHORT", 64).unwrap(), b"ABC");
    }

    #[test]
    fn watch_opens_and_closes() {
        // process-wide state: this is the only test that touches it
        unsafe {
            assert_eq!(disk_watch_fd(), -1);
            assert_eq!(disk_watch_dispatch(), 0, "nothing to dispatch while closed");
            assert_eq!(disk_watch(1), 0);
            let fd = disk_watch_fd();
            assert!(fd >= 0);
            assert_eq!(disk_watch(1), 0);
            assert_eq!(disk_watch_fd(), fd, "opening twice keeps the first");
            assert!(disk_watch_dispatch() == 0 || disk_watch_dispatch() == 1);
            assert_eq!(disk_watch(0), 0);
            assert_eq!(disk_watch_fd(), -1);
            assert_eq!(disk_watch(0), 0, "closing twice is fine");
        }
    }

    #[test]
    fn tools_and_hash() {
        let j = tools_json();
        assert!(j.starts_with("{\"fat32\":") && j.ends_with('}') && j.contains(",\"smartctl\":"), "{j}");
        let t = Tree::new();
        t.file("proc/self/mountinfo", "a\n");
        let f = t.fs();
        let h = mounts_hash(&f);
        assert_ne!(h, 0);
        t.file("proc/self/mountinfo", "b\n");
        assert_ne!(mounts_hash(&f), h, "a different mountinfo has a different fingerprint");
        assert_eq!(mounts_hash(&Fs { root: b"/nonexistent-axia".to_vec() }), 0, "no mountinfo, no fingerprint");
    }
}
