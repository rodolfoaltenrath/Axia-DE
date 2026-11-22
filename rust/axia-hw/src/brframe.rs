//! Settings → Appearance → "Browsers: tabs in the title bar" (docs/WAYLAND.md, "The switch in
//! Settings"). Writes ONE preference of the preinstalled browsers in the user's own profiles, and
//! nothing else:
//!
//!   Chromium  ~/.config/chromium/{Default,Profile N}/Preferences
//!             browser.custom_chrome_frame = on, and with on extensions.theme.system_theme = 1 (the GTK
//!             theme, for LordOS's window buttons) unless the profile uses a theme extension. Only while
//!             Chromium is not running (it rewrites the file when it quits); a surgical edit of those
//!             keys, the rest of the file byte for byte, validated as JSON before and after, a backup
//!             (Preferences.lordos-backup) and an atomic rename.
//!   Firefox   ~/.config/mozilla/firefox and ~/.mozilla/firefox (and LibreWolf's), the relative profiles
//!             listed in profiles.ini: one marked line user_pref("browser.tabs.inTitlebar", 1|0) in
//!             user.js (Firefox reads it at start, so it can be written while Firefox runs). `firefox_tidy`
//!             removes the line again once Firefox has started with it (its "lock" link or .parentlock
//!             newer than user.js, the profile not open), so later choices in Firefox's own «Title Bar»
//!             stick. A user.js line the user wrote themselves is never touched ("kept").
//!
//! No program is run, no path comes from the caller: the folders are fixed under the user's home. Every
//! file is opened with O_NOFOLLOW, must be a regular file owned by the user, and is size-capped.
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::cell::Cell;
use std::ffi::CString;
use std::fmt;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

/// Chromium's Preferences.
const PREFS_MAX: usize = 32 << 20;
/// Firefox's user.js.
const USERJS_MAX: usize = 1 << 20;
/// Firefox's prefs.js.
const FFPREFS_MAX: usize = 16 << 20;
/// profiles.ini.
const INI_MAX: usize = 64 << 10;
const MAX_PROFILES: usize = 64;

const FF_PREF: &str = "browser.tabs.inTitlebar";
/// A marker in a config file.
const FF_MARK: &str = "// lordos: Settings > Appearance";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct BrfResult {
    /// The browser's data folder exists.
    pub found: i32,
    /// Profiles looked at.
    pub profiles: i32,
    /// Written now.
    pub changed: i32,
    /// Already as asked.
    pub unchanged: i32,
    /// Chromium: it is open and nothing was written (try again after it quits); Firefox: profiles that
    /// are open (the line applies when Firefox restarts).
    pub running: i32,
    /// Firefox: the user's own user.js sets the preference; left alone.
    pub kept: i32,
    /// Unreadable, malformed or unwritable: left exactly as it was.
    pub failed: i32,
}

/// What an edit did.
#[derive(Debug, PartialEq, Eq)]
pub enum Edit {
    /// The new text.
    Changed(Vec<u8>),
    Same,
    Bad,
}

// ---- strict JSON: what json_parse of the C code accepted ----

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// A JSON value read strictly: no repeated keys in an object, no `\u0000`, at most 32 levels.
struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Strict, D::Error> {
        struct V;
        fn enter<E: serde::de::Error>() -> Result<(), E> {
            let n = DEPTH.with(|c| {
                c.set(c.get() + 1);
                c.get()
            });
            if n > 32 { Err(E::custom("nested too deeply")) } else { Ok(()) }
        }
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON")
            }
            fn visit_bool<E>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(Value::Bool(v)))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(Value::from(v)))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(Value::from(v)))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Strict, E> {
                Ok(Strict(Value::from(v)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Strict, E> {
                if v.contains('\0') {
                    return Err(E::custom("\\u0000 is not accepted"));
                }
                Ok(Strict(Value::String(v.to_string())))
            }
            fn visit_unit<E>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                enter()?;
                let mut v = Vec::new();
                while let Some(Strict(x)) = a.next_element()? {
                    v.push(x);
                }
                DEPTH.with(|c| c.set(c.get() - 1));
                Ok(Strict(Value::Array(v)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                enter()?;
                let mut m = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if k.contains('\0') || m.contains_key(&k) {
                        return Err(serde::de::Error::custom("invalid or repeated key"));
                    }
                    let Strict(v) = a.next_value()?;
                    m.insert(k, v);
                }
                DEPTH.with(|c| c.set(c.get() - 1));
                Ok(Strict(Value::Object(m)))
            }
        }
        d.deserialize_any(V)
    }
}

/// The value of a strictly valid JSON text, `None` if it is not.
fn strict(text: &[u8]) -> Option<Value> {
    DEPTH.with(|c| c.set(0));
    serde_json::from_slice::<Strict>(text).ok().map(|s| s.0)
}

// ---- JSON: just enough scanning to find a member's value span (the text was validated first, and the
// result is validated again) ----

fn sk_ws(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// `s[i] == '"'`: the index after the closing quote, 0 if unterminated.
fn sk_str(s: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < s.len() {
        if s[i] == b'\\' {
            i += 2;
            continue;
        }
        if s[i] == b'"' {
            return i + 1;
        }
        i += 1;
    }
    0
}

/// The index after the value at `s[i]`, 0 if it isn't one.
fn sk_val(s: &[u8], mut i: usize, depth: u32) -> usize {
    if i >= s.len() || depth > 64 {
        return 0;
    }
    if s[i] == b'"' {
        return sk_str(s, i);
    }
    if s[i] == b'{' || s[i] == b'[' {
        let obj = s[i] == b'{';
        let close = if obj { b'}' } else { b']' };
        i = sk_ws(s, i + 1);
        if i < s.len() && s[i] == close {
            return i + 1;
        }
        loop {
            if obj {
                if i >= s.len() || s[i] != b'"' {
                    return 0;
                }
                i = sk_str(s, i);
                if i == 0 {
                    return 0;
                }
                i = sk_ws(s, i);
                if i >= s.len() || s[i] != b':' {
                    return 0;
                }
                i = sk_ws(s, i + 1);
            }
            i = sk_val(s, i, depth + 1);
            if i == 0 {
                return 0;
            }
            i = sk_ws(s, i);
            if i < s.len() && s[i] == b',' {
                i = sk_ws(s, i + 1);
                continue;
            }
            if i < s.len() && s[i] == close {
                return i + 1;
            }
            return 0;
        }
    }
    let st = i;
    while i < s.len() && (s[i].is_ascii_lowercase() || s[i].is_ascii_digit() || matches!(s[i], b'-' | b'+' | b'.' | b'E')) {
        i += 1;
    }
    if i > st { i } else { 0 }
}

/// Where the member `key` (compared byte for byte) of the object at `s[o] == '{'` is: `Ok(Some((start, end)))`
/// of its value, `Ok(None)` if missing, `Err` if malformed. The bool says the object has no members.
fn find_member(s: &[u8], o: usize, key: &str) -> Result<(Option<(usize, usize)>, bool), ()> {
    let key = key.as_bytes();
    let mut i = sk_ws(s, o + 1);
    let empty = i < s.len() && s[i] == b'}';
    if empty {
        return Ok((None, true));
    }
    loop {
        if i >= s.len() || s[i] != b'"' {
            return Err(());
        }
        let ke = sk_str(s, i);
        if ke == 0 {
            return Err(());
        }
        let is_match = ke - i - 2 == key.len() && &s[i + 1..ke - 1] == key;
        i = sk_ws(s, ke);
        if i >= s.len() || s[i] != b':' {
            return Err(());
        }
        i = sk_ws(s, i + 1);
        let e = sk_val(s, i, 1);
        if e == 0 {
            return Err(());
        }
        if is_match {
            return Ok((Some((i, e)), false));
        }
        i = sk_ws(s, e);
        if i < s.len() && s[i] == b',' {
            i = sk_ws(s, i + 1);
            continue;
        }
        if i < s.len() && s[i] == b'}' {
            return Ok((None, false));
        }
        return Err(());
    }
}

enum Set {
    Changed,
    Same,
    Bad,
}

/// `b[from..to]` replaced with `text`.
fn splice(b: &mut Vec<u8>, from: usize, to: usize, text: &[u8]) {
    b.splice(from..to, text.iter().copied());
}

/// keys[0].keys[1]… = `lit` (a JSON literal): replaced if present, otherwise inserted as the first member of
/// the deepest object that exists.
fn set_path(b: &mut Vec<u8>, keys: &[&str], lit: &str) -> Set {
    let mut o = sk_ws(b, 0);
    if o >= b.len() || b[o] != b'{' {
        return Set::Bad;
    }
    for k in 0..keys.len() {
        let Ok((found, empty)) = find_member(b, o, keys[k]) else { return Set::Bad };
        if let Some((vs, ve)) = found {
            if k == keys.len() - 1 {
                if &b[vs..ve] == lit.as_bytes() {
                    return Set::Same;
                }
                splice(b, vs, ve, lit.as_bytes());
                return Set::Changed;
            }
            if b[vs] != b'{' {
                return Set::Bad; // not an object: don't guess
            }
            o = vs;
            continue;
        }
        let mut ins = String::new();
        for (j, key) in keys.iter().enumerate().skip(k) {
            ins.push_str(&format!("\"{key}\":{}", if j == keys.len() - 1 { lit } else { "{" }));
        }
        for _ in k..keys.len() - 1 {
            ins.push('}');
        }
        if !empty {
            ins.push(',');
        }
        splice(b, o + 1, o + 1, ins.as_bytes());
        return Set::Changed;
    }
    Set::Bad
}

/// Chromium's Preferences with the frame (and the GTK theme) switched `on` or off.
pub fn chrome_edit(input: &[u8], on: bool) -> Edit {
    if input.is_empty() || input.len() > PREFS_MAX {
        return Edit::Bad;
    }
    let Some(v) = strict(input) else { return Edit::Bad };
    let Some(root) = v.as_object() else { return Edit::Bad };
    // a theme extension the user installed: kept
    let theme_ext = root.get("extensions").and_then(|e| e.get("theme")).and_then(|t| t.get("id")).and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    let mut b = input.to_vec();
    let mut r = set_path(&mut b, &["browser", "custom_chrome_frame"], if on { "true" } else { "false" });
    let mut changed = matches!(r, Set::Changed);
    if !matches!(r, Set::Bad) && on && !theme_ext {
        r = set_path(&mut b, &["extensions", "theme", "system_theme"], "1");
        changed |= matches!(r, Set::Changed);
    }
    match r {
        Set::Bad => return Edit::Bad,
        _ if !changed => return Edit::Same,
        _ => {}
    }
    // the result must parse and say what was asked
    let ok = strict(&b).is_some_and(|v| v.get("browser").and_then(|b| b.get("custom_chrome_frame")).and_then(Value::as_bool) == Some(on));
    if ok { Edit::Changed(b) } else { Edit::Bad }
}

// ---- Firefox's user.js / prefs.js lines ----

/// Is the line a user_pref of `FF_PREF`? `(value 0/1, true/false too; -1 if unreadable, ours if marked)`.
fn ff_line(l: &[u8]) -> Option<(i32, bool)> {
    let blank = |c: u8| c == b' ' || c == b'\t';
    let skip = |mut i: usize| {
        while i < l.len() && blank(l[i]) {
            i += 1;
        }
        i
    };
    let mut i = skip(0);
    const UP: &[u8] = b"user_pref(";
    if !l[i..].starts_with(UP) {
        return None;
    }
    i = skip(i + UP.len());
    if i >= l.len() || (l[i] != b'"' && l[i] != b'\'') {
        return None;
    }
    let q = l[i];
    i += 1;
    let kl = FF_PREF.len();
    if l.len() - i < kl + 1 || &l[i..i + kl] != FF_PREF.as_bytes() || l[i + kl] != q {
        return None;
    }
    i = skip(i + kl + 1);
    if i >= l.len() || l[i] != b',' {
        return None;
    }
    i = skip(i + 1);
    let rest = &l[i..];
    let val = if rest.first().is_some_and(u8::is_ascii_digit) {
        (rest[0] != b'0') as i32
    } else if rest.starts_with(b"true") {
        1
    } else if rest.starts_with(b"false") {
        0
    } else {
        -1
    };
    let ours = l.windows(FF_MARK.len()).any(|w| w == FF_MARK.as_bytes());
    Some((val, ours))
}

/// The lines of a text as the C loops walk them: each without its newline, the last one possibly without.
fn lines(t: &[u8]) -> impl Iterator<Item = (&[u8], bool)> {
    let mut i = 0;
    std::iter::from_fn(move || {
        if i >= t.len() {
            return None;
        }
        let e = t[i..].iter().position(|&c| c == b'\n');
        let len = e.unwrap_or(t.len() - i);
        let l = &t[i..i + len];
        i += len + 1;
        Some((l, e.is_some()))
    })
}

/// The value prefs.js holds for browser.tabs.inTitlebar, or -1 if it isn't set there.
pub fn prefs_value(t: &[u8]) -> i32 {
    let mut v = -1;
    for (l, _) in lines(t) {
        if let Some((val, _)) = ff_line(l) {
            if val >= 0 {
                v = val;
            }
        }
    }
    v
}

/// The value of LordOS's marked line in user.js, or -1.
pub fn userjs_ours(t: &[u8]) -> i32 {
    let mut v = -1;
    for (l, _) in lines(t) {
        if let Some((val, true)) = ff_line(l) {
            v = val;
        }
    }
    v
}

/// user.js: `value` 0/1 sets LordOS's marked line, -1 removes it. The bool is true if a line the user wrote
/// sets the preference (then nothing is changed: `Same`).
pub fn userjs_edit(input: &[u8], value: i32) -> (Edit, bool) {
    if input.len() > USERJS_MAX || input.contains(&0) {
        return (Edit::Bad, false);
    }
    let mut h: Vec<u8> = Vec::new();
    for (l, nl) in lines(input) {
        match ff_line(l) {
            Some((_, false)) => return (Edit::Same, true), // the user's own line: theirs
            Some((_, true)) => {}
            None => {
                h.extend_from_slice(l);
                if nl {
                    h.push(b'\n');
                }
            }
        }
    }
    if value >= 0 {
        if h.last().is_some_and(|&c| c != b'\n') {
            h.push(b'\n');
        }
        h.extend_from_slice(format!("user_pref(\"{FF_PREF}\", {}); {FF_MARK}, removed once Firefox has read it\n", (value != 0) as i32).as_bytes());
    }
    if h == input {
        return (Edit::Same, false);
    }
    (Edit::Changed(h), false)
}

// ---- files ----

fn c(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

fn uid() -> u32 {
    // SAFETY: getuid cannot fail.
    unsafe { libc::getuid() }
}

fn openat(dfd: RawFd, name: &str, flags: i32, mode: u32) -> Option<OwnedFd> {
    // SAFETY: a valid C string and a descriptor.
    let fd = unsafe { libc::openat(dfd, c(name).as_ptr(), flags | libc::O_CLOEXEC, mode) };
    // SAFETY: a new descriptor that nobody else owns.
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
}

fn fstat(fd: RawFd) -> Option<libc::stat> {
    // SAFETY: fstat fills the struct.
    unsafe {
        let mut s: libc::stat = std::mem::zeroed();
        (libc::fstat(fd, &mut s) == 0).then_some(s)
    }
}

/// Why a read failed: ENOENT, EINVAL (not a regular file of ours, or too big) or EIO.
type Errno = i32;

/// `name` in `dfd`: a regular file owned by the user, at most `max` bytes, opened without following links.
fn read_at(dfd: RawFd, name: &str, max: usize) -> Result<(Vec<u8>, libc::stat), Errno> {
    let Some(fd) = openat(dfd, name, libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK, 0) else {
        return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EIO));
    };
    let Some(s) = fstat(fd.as_raw_fd()) else { return Err(libc::EINVAL) };
    if s.st_mode & libc::S_IFMT != libc::S_IFREG || s.st_uid != uid() || s.st_size as usize > max {
        return Err(libc::EINVAL);
    }
    let mut b = vec![0u8; s.st_size as usize];
    let mut got = 0;
    while got < b.len() {
        // SAFETY: the buffer has room for what is asked.
        let r = unsafe { libc::read(fd.as_raw_fd(), b[got..].as_mut_ptr().cast(), b.len() - got) };
        if r < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if r <= 0 {
            break;
        }
        got += r as usize;
    }
    if got != b.len() {
        return Err(libc::EIO);
    }
    Ok((b, s))
}

/// Atomic replace of `name` in `dfd`: a new file next to it (O_EXCL, never through a link), synced, then
/// renamed over it.
fn write_at(dfd: RawFd, name: &str, data: &[u8], mode: u32) -> bool {
    let tmp = format!("{name}.lordos-tmp");
    if tmp.len() >= 256 {
        return false;
    }
    // SAFETY: valid C strings and descriptors throughout.
    unsafe {
        libc::unlinkat(dfd, c(&tmp).as_ptr(), 0);
        let Some(fd) = openat(dfd, &tmp, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW, 0o600) else { return false };
        let mut ok = libc::fchmod(fd.as_raw_fd(), mode & 0o777) == 0;
        let mut put = 0;
        while ok && put < data.len() {
            let w = libc::write(fd.as_raw_fd(), data[put..].as_ptr().cast(), data.len() - put);
            if w < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if w <= 0 {
                ok = false;
            } else {
                put += w as usize;
            }
        }
        ok = ok && libc::fsync(fd.as_raw_fd()) == 0;
        let closed = libc::close(std::os::fd::IntoRawFd::into_raw_fd(fd)) == 0;
        ok = closed && ok;
        if !ok || libc::renameat(dfd, c(&tmp).as_ptr(), dfd, c(name).as_ptr()) != 0 {
            libc::unlinkat(dfd, c(&tmp).as_ptr(), 0);
            return false;
        }
        libc::fsync(dfd);
    }
    true
}

/// base/sub as a folder owned by the user (base itself may be a link the user made).
fn open_root(base: &str, sub: &str) -> Option<OwnedFd> {
    let p = format!("{base}/{sub}");
    if !base.starts_with('/') || p.len() >= 4096 {
        return None;
    }
    // SAFETY: a valid C string.
    let fd = unsafe { libc::open(c(&p).as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
    if fd < 0 {
        return None;
    }
    // SAFETY: a new descriptor that nobody else owns.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    fstat(fd.as_raw_fd()).filter(|s| s.st_uid == uid()).map(|_| fd)
}

/// Is `pid` one of the user's processes whose name starts with one of `names`?
fn pid_is(pid: i64, names: &[&str]) -> bool {
    if pid <= 1 {
        return false;
    }
    let owned = std::fs::metadata(format!("/proc/{pid}")).is_ok_and(|m| std::os::unix::fs::MetadataExt::uid(&m) == uid());
    if !owned {
        return false;
    }
    let Ok(comm) = std::fs::read(format!("/proc/{pid}/comm")) else { return false };
    let comm = &comm[..comm.len().min(63)];
    let comm = comm.split(|&c| c == b'\n').next().unwrap_or(b"");
    !comm.is_empty() && names.iter().any(|n| comm.starts_with(n.as_bytes()))
}

const CHROMIUM_NAMES: [&str; 2] = ["chromium", "chrome"];
const FIREFOX_NAMES: [&str; 2] = ["firefox", "GeckoMain"];

fn readlinkat(dfd: RawFd, name: &str, max: usize) -> Result<Vec<u8>, Errno> {
    let mut b = vec![0u8; max];
    // SAFETY: the buffer is ours.
    let k = unsafe { libc::readlinkat(dfd, c(name).as_ptr(), b.as_mut_ptr().cast(), max) };
    if k < 0 {
        return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EIO));
    }
    b.truncate(k as usize);
    Ok(b)
}

fn strtol10(s: &[u8]) -> (i64, usize) {
    let mut i = 0;
    let neg = s.first() == Some(&b'-');
    if matches!(s.first(), Some(b'-' | b'+')) {
        i = 1;
    }
    let st = i;
    let mut v = 0i64;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.saturating_mul(10).saturating_add((s[i] - b'0') as i64);
        i += 1;
    }
    if i == st { (0, 0) } else { (if neg { -v } else { v }, i) }
}

/// Chromium's SingletonLock is a link to "<host>-<pid>". Anything unexpected counts as running.
fn chromium_locked(dfd: RawFd) -> bool {
    let t = match readlinkat(dfd, "SingletonLock", 299) {
        Ok(t) => t,
        Err(e) => return e != libc::ENOENT,
    };
    let Some(dash) = t.iter().rposition(|&c| c == b'-') else { return true };
    let mut host = [0u8; 256];
    // SAFETY: gethostname fills the buffer.
    if unsafe { libc::gethostname(host.as_mut_ptr().cast(), host.len()) } != 0 {
        return true;
    }
    host[255] = 0;
    let host = &host[..host.iter().position(|&c| c == 0).unwrap_or(255)];
    if &t[..dash] != host {
        return true; // another computer
    }
    let (pid, n) = strtol10(&t[dash + 1..]);
    if n != t.len() - dash - 1 || pid <= 0 {
        return true;
    }
    pid_is(pid, &CHROMIUM_NAMES) // a dead or unrelated pid: a stale lock
}

/// Is Chromium of this user running (its profile lock is held by a live Chromium)?
pub fn chromium_running(cfg: &str) -> bool {
    open_root(cfg, "chromium").is_some_and(|d| chromium_locked(d.as_raw_fd()))
}

/// A Firefox profile is open: a lock on .parentlock, or the "lock" link "<ip>:+<pid>" of a live Firefox.
fn ff_locked(pfd: RawFd) -> bool {
    if let Some(fd) = openat(pfd, ".parentlock", libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK, 0) {
        // SAFETY: F_GETLK on our descriptor with a zeroed flock.
        let locked = unsafe {
            let mut fl: libc::flock = std::mem::zeroed();
            fl.l_type = libc::F_WRLCK as libc::c_short;
            fl.l_whence = libc::SEEK_SET as libc::c_short;
            libc::fcntl(fd.as_raw_fd(), libc::F_GETLK, &mut fl) == 0 && fl.l_type != libc::F_UNLCK as libc::c_short
        };
        if locked {
            return true;
        }
    }
    if let Ok(t) = readlinkat(pfd, "lock", 127) {
        if let Some(plus) = t.iter().rposition(|&c| c == b'+') {
            let (pid, n) = strtol10(&t[plus + 1..]);
            if n == t.len() - plus - 1 && pid_is(pid, &FIREFOX_NAMES) {
                return true;
            }
        }
    }
    false
}

fn chromium_profile_name(n: &str) -> bool {
    n == "Default" || (n.starts_with("Profile ") && n.len() > 8 && n.len() <= 12 && n[8..].bytes().all(|c| c.is_ascii_digit()))
}

/// The profile folder names under Chromium's folder (Default, Profile N), sorted.
fn chromium_profiles(dfd: RawFd) -> Option<Vec<String>> {
    // SAFETY: fdopendir takes a duplicate that it owns.
    let mut names = Vec::new();
    unsafe {
        let dup = libc::dup(dfd);
        if dup < 0 {
            return None;
        }
        let d = libc::fdopendir(dup);
        if d.is_null() {
            libc::close(dup);
            return None;
        }
        loop {
            let e = libc::readdir(d);
            if e.is_null() {
                break;
            }
            let n = std::ffi::CStr::from_ptr((*e).d_name.as_ptr()).to_string_lossy().into_owned();
            if chromium_profile_name(&n) {
                names.push(n);
            }
        }
        libc::closedir(d);
    }
    names.sort();
    names.truncate(MAX_PROFILES);
    Some(names)
}

/// Switches the frame on every profile of Chromium (only while it is not running).
pub fn chromium_apply(cfg: &str, on: bool) -> BrfResult {
    let mut r = BrfResult::default();
    let Some(dfd) = open_root(cfg, "chromium") else { return r };
    r.found = 1;
    let d = dfd.as_raw_fd();
    if chromium_locked(d) {
        r.running = 1;
        return r;
    }
    let Some(names) = chromium_profiles(d) else {
        r.failed += 1;
        return r;
    };
    for name in names {
        if r.running != 0 {
            break; // it started meanwhile: the rest waits
        }
        let Some(pfd) = openat(d, &name, libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW, 0) else { continue };
        let p = pfd.as_raw_fd();
        let (data, st) = match read_at(p, "Preferences", PREFS_MAX) {
            Ok(x) => x,
            Err(e) => {
                if e != libc::ENOENT {
                    r.profiles += 1; // no Preferences yet: not a profile in use
                    r.failed += 1;
                }
                continue;
            }
        };
        r.profiles += 1;
        match chrome_edit(&data, on) {
            Edit::Bad => r.failed += 1,
            Edit::Same => r.unchanged += 1,
            Edit::Changed(_) if chromium_locked(d) => r.running = 1, // it started meanwhile: later
            Edit::Changed(out) => {
                if !write_at(p, "Preferences.lordos-backup", &data, st.st_mode) || !write_at(p, "Preferences", &out, st.st_mode) {
                    r.failed += 1;
                } else {
                    r.changed += 1;
                }
            }
        }
    }
    r
}

/// How many Chromium profiles say custom_chrome_frame true / false (unset ones aren't counted).
pub fn chromium_state(cfg: &str) -> (i32, i32) {
    let (mut on, mut off) = (0, 0);
    let Some(dfd) = open_root(cfg, "chromium") else { return (0, 0) };
    for name in chromium_profiles(dfd.as_raw_fd()).unwrap_or_default() {
        let Some(pfd) = openat(dfd.as_raw_fd(), &name, libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW, 0) else { continue };
        let Ok((data, _)) = read_at(pfd.as_raw_fd(), "Preferences", PREFS_MAX) else { continue };
        let Some(v) = strict(&data) else { continue };
        match v.get("browser").and_then(|b| b.get("custom_chrome_frame")).and_then(Value::as_bool) {
            Some(true) => on += 1,
            Some(false) => off += 1,
            None => {}
        }
    }
    (on, off)
}

// ---- Firefox profiles ----

fn ff_path_ok(p: &str) -> bool {
    !p.is_empty() && p.len() <= 200 && p != "." && p != ".." && !p.bytes().any(|c| c < 0x20 || c == b'/' || c == 0x7f)
}

/// Every relative profile listed in each root's profiles.ini, once. Returns how many roots exist.
fn ff_each(cfg: &str, home: &str, f: &mut dyn FnMut(RawFd)) -> usize {
    let roots: [(&str, &str); 5] = [(cfg, "mozilla/firefox"), (home, ".mozilla/firefox"), (cfg, "librewolf/librewolf"), (cfg, "librewolf"), (home, ".librewolf")];
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut found = 0;
    for (base, sub) in roots {
        let Some(rfd) = open_root(base, sub) else { continue };
        found += 1;
        let ini = read_at(rfd.as_raw_fd(), "profiles.ini", INI_MAX).map(|x| x.0).ok();
        let mut flush = |path: &str, in_profile: bool, relative: bool, seen: &mut Vec<(u64, u64)>| {
            if in_profile && relative && ff_path_ok(path) && seen.len() < MAX_PROFILES {
                if let Some(pfd) = openat(rfd.as_raw_fd(), path, libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW, 0) {
                    if let Some(st) = fstat(pfd.as_raw_fd()).filter(|s| s.st_uid == uid()) {
                        let id = (st.st_dev as u64, st.st_ino as u64);
                        if !seen.contains(&id) {
                            seen.push(id);
                            f(pfd.as_raw_fd());
                        }
                    }
                }
            }
        };
        if let Some(ini) = ini {
            let (mut path, mut in_profile, mut relative) = (String::new(), false, false);
            for (raw, _) in lines(&ini) {
                let line = &raw[..raw.len().min(299)];
                let line = &line[..line.iter().position(|&c| c == b'\r').unwrap_or(line.len())];
                if line.first() == Some(&b'[') {
                    flush(&path, in_profile, relative, &mut seen);
                    in_profile = line.starts_with(b"[Profile");
                    relative = false;
                    path.clear();
                } else if in_profile && line.starts_with(b"Path=") && line.len() - 5 < 256 {
                    path = String::from_utf8_lossy(&line[5..]).into_owned();
                } else if in_profile && line.starts_with(b"IsRelative=") {
                    relative = &line[11..] == b"1";
                }
            }
            flush(&path, in_profile, relative, &mut seen); // the last section
        }
    }
    found
}

/// Sets the preference in every Firefox profile (the user's own line is kept).
pub fn firefox_apply(cfg: &str, home: &str, on: bool) -> BrfResult {
    let mut r = BrfResult::default();
    let value = on as i32;
    let found = ff_each(cfg, home, &mut |pfd| {
        r.profiles += 1;
        let open_now = ff_locked(pfd);
        let (u, st, existed) = match read_at(pfd, "user.js", USERJS_MAX) {
            Ok((u, st)) => (u, Some(st), true),
            Err(e) if e == libc::ENOENT => (Vec::new(), None, false),
            Err(_) => {
                r.failed += 1;
                return;
            }
        };
        let (e, own) = userjs_edit(&u, value);
        if own {
            r.kept += 1;
            return;
        }
        match e {
            Edit::Bad => r.failed += 1,
            Edit::Same => r.unchanged += 1,
            Edit::Changed(out) => {
                // no line needed when prefs.js already holds the value and there was none of ours
                let prefs = if userjs_ours(&u) < 0 { read_at(pfd, "prefs.js", FFPREFS_MAX).ok().map(|x| x.0) } else { None };
                if prefs.is_some_and(|p| prefs_value(&p) == value) {
                    r.unchanged += 1;
                    return;
                }
                let mode = st.map_or(0o600, |s| s.st_mode);
                let backup_failed = existed && !u.is_empty() && !write_at(pfd, "user.js.lordos-backup", &u, mode);
                if backup_failed || !write_at(pfd, "user.js", &out, if existed { mode } else { 0o600 }) {
                    r.failed += 1;
                } else {
                    r.changed += 1;
                    if open_now {
                        r.running += 1;
                    }
                }
            }
        }
    });
    r.found = (found > 0) as i32;
    r
}

fn newer(a: &libc::timespec, b: &libc::timespec) -> bool {
    a.tv_sec > b.tv_sec || (a.tv_sec == b.tv_sec && a.tv_nsec > b.tv_nsec)
}

/// Has Firefox started this profile since `t`? Firefox makes the "lock" link and .parentlock when it opens a
/// profile (and leaves them when it quits), so their times are its last start. prefs.js is no proof: Firefox
/// also writes it when it quits, after a line written while it ran.
fn ff_started_after(pfd: RawFd, t: &libc::timespec) -> bool {
    let at = |name: &str| {
        // SAFETY: fstatat fills the struct.
        unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            (libc::fstatat(pfd, c(name).as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) == 0).then_some(st)
        }
    };
    let ts = |s: &libc::stat| libc::timespec { tv_sec: s.st_mtime, tv_nsec: s.st_mtime_nsec };
    if at("lock").is_some_and(|s| newer(&ts(&s), t)) {
        return true;
    }
    at(".parentlock").is_some_and(|s| s.st_mode & libc::S_IFMT == libc::S_IFREG && newer(&ts(&s), t))
}

/// Removes LordOS's marked lines Firefox has already read. How many.
pub fn firefox_tidy(cfg: &str, home: &str) -> i32 {
    let mut removed = 0;
    ff_each(cfg, home, &mut |pfd| {
        let Ok((u, us)) = read_at(pfd, "user.js", USERJS_MAX) else { return };
        let mtime = libc::timespec { tv_sec: us.st_mtime, tv_nsec: us.st_mtime_nsec };
        if userjs_ours(&u) < 0 || ff_locked(pfd) || !ff_started_after(pfd, &mtime) {
            return;
        }
        // Firefox has started with the line since it was written: the value is its own now
        if let (Edit::Changed(out), _) = userjs_edit(&u, -1) {
            let done = if out.is_empty() {
                // SAFETY: a valid C string and descriptor.
                unsafe { libc::unlinkat(pfd, c("user.js").as_ptr(), 0) == 0 }
            } else {
                write_at(pfd, "user.js", &out, us.st_mode)
            };
            if done {
                removed += 1;
            }
        }
    });
    removed
}

/// {"chromium":{…},"firefox":{…}} for lord.browserFrame.apply.
pub fn result_json(chromium: &BrfResult, firefox: &BrfResult) -> String {
    let one = |name: &str, r: &BrfResult| {
        format!(
            "\"{name}\":{{\"found\":{},\"profiles\":{},\"changed\":{},\"unchanged\":{},\"running\":{},\"kept\":{},\"failed\":{}}}",
            r.found != 0, r.profiles, r.changed, r.unchanged, r.running, r.kept, r.failed
        )
    };
    format!("{{{},{}}}", one("chromium", chromium), one("firefox", firefox))
}
