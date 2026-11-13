//! Password and passphrase generator, strength estimate, common-password check and the local
//! Have I Been Pwned lookup of axia-vault: the C ABI of server/src/vault/vgen.h, which this
//! replaced. Everything is offline. The two embedded lists (EFF words, 10,000 common
//! passwords) are borrowed from the caller, who keeps them for the life of the process; the
//! line indexes live in fixed tables, so nothing here allocates on the vault's secrets arena.
use axia_crypto::vcrypto::{sha1, vc_random};
use std::ffi::{c_char, c_int, c_long, c_void};
use std::sync::Mutex;

/// Layout of the C `VGenOpts`.
#[repr(C)]
pub struct VGenOpts {
    length: c_int,
    upper: c_int,
    lower: c_int,
    digits: c_int,
    symbols: c_int,
    no_ambiguous: c_int,
    words: c_int,
    sep: [u8; 8],
    capitalize: c_int,
    add_digit: c_int,
}

const WMAX: usize = 16384;
const CHASH: usize = 32768;

struct Lists {
    words: &'static [u8],
    word_at: [u32; WMAX],
    nwords: usize,
    common: &'static [u8],
    common_at: [u32; CHASH / 2],
    /// Lines of the common list, including those past the index (as the C code counted them).
    ncommon: usize,
    /// Open-addressing table of the first CHASH/2 common lines: index + 1, 0 = free.
    ctab: [i32; CHASH],
}

static LISTS: Mutex<Lists> = Mutex::new(Lists {
    words: &[],
    word_at: [0; WMAX],
    nwords: 0,
    common: &[],
    common_at: [0; CHASH / 2],
    ncommon: 0,
    ctab: [0; CHASH],
});

fn lists() -> std::sync::MutexGuard<'static, Lists> {
    LISTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Start of every non-empty line (up to `at.len()` of them); returns how many lines there are.
fn split_lines(buf: &[u8], at: &mut [u32]) -> usize {
    let (mut n, mut start) = (0usize, 0usize);
    for i in 0..=buf.len() {
        if i == buf.len() || buf[i] == b'\n' {
            if i > start {
                if n < at.len() {
                    at[n] = start as u32;
                }
                n += 1;
            }
            start = i + 1;
        }
    }
    n
}

/// The line starting at `from`, without its line break.
fn line(buf: &[u8], from: u32) -> &[u8] {
    let s = &buf[from as usize..];
    &s[..s.iter().position(|&c| c == b'\n' || c == b'\r').unwrap_or(s.len())]
}

fn fnv(s: &[u8]) -> u32 {
    let mut h = 2166136261u32;
    for &b in s {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

impl Lists {
    fn word(&self, i: usize) -> &[u8] {
        line(self.words, self.word_at[i])
    }
    fn index_common(&mut self) {
        self.ctab.fill(0);
        for i in 0..self.ncommon.min(CHASH / 2) {
            let mut h = fnv(line(self.common, self.common_at[i])) as usize & (CHASH - 1);
            while self.ctab[h] != 0 {
                h = (h + 1) & (CHASH - 1);
            }
            self.ctab[h] = i as i32 + 1;
        }
    }
    /// Position (1 = most common) of exactly `s` in the common list, or 0.
    fn exact(&self, s: &[u8]) -> i32 {
        if self.ncommon == 0 {
            return 0;
        }
        let mut h = fnv(s) as usize & (CHASH - 1);
        while self.ctab[h] != 0 {
            if line(self.common, self.common_at[self.ctab[h] as usize - 1]) == s {
                return self.ctab[h];
            }
            h = (h + 1) & (CHASH - 1);
        }
        0
    }
}

unsafe fn borrowed(p: *const c_char, n: usize) -> &'static [u8] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(p.cast::<u8>(), n)
    }
}

/// Registers the embedded lists, one entry per line. They must outlive every later call.
///
/// # Safety
/// `w` and `c` cover `wlen` / `clen` bytes for the life of the process.
#[no_mangle]
pub unsafe extern "C" fn vg_set_lists(w: *const c_char, wlen: usize, c: *const c_char, clen: usize) {
    let mut l = lists();
    l.words = borrowed(w, wlen);
    l.common = borrowed(c, clen);
    let (words, common) = (l.words, l.common);
    l.nwords = split_lines(words, &mut l.word_at).min(WMAX);
    l.ncommon = split_lines(common, &mut l.common_at);
    l.index_common();
}

#[no_mangle]
pub extern "C" fn vg_word_count() -> usize {
    lists().nwords
}

/// Uniform in `0..n` without modulo bias; `None` if the system has no randomness.
fn rnd_below(n: u32) -> Option<u32> {
    if n <= 1 {
        return Some(0);
    }
    let lim = u32::MAX - u32::MAX % n;
    loop {
        let mut r = 0u32;
        if unsafe { vc_random((&mut r as *mut u32).cast::<c_void>(), 4) } != 0 {
            return None;
        }
        if r < lim {
            return Some(r % n);
        }
    }
}

fn generate(o: &VGenOpts, out: &mut [u8], bits: &mut f64) -> Option<()> {
    let cap = out.len();
    if o.words > 0 {
        let l = lists();
        if l.nwords == 0 {
            return None;
        }
        let nw = o.words.clamp(3, 12);
        let dig_at = if o.add_digit != 0 { rnd_below(nw as u32)? as i32 } else { -1 };
        let sep = &o.sep[..o.sep.iter().position(|&c| c == 0).unwrap_or(8)];
        let mut k = 0usize;
        for i in 0..nw {
            let w = l.word(rnd_below(l.nwords as u32)? as usize);
            let sl = if i > 0 { sep.len() } else { 0 };
            if k + sl + w.len() + 2 >= cap {
                return None;
            }
            out[k..k + sl].copy_from_slice(&sep[..sl]);
            k += sl;
            out[k..k + w.len()].copy_from_slice(w);
            if o.capitalize != 0 && !w.is_empty() {
                out[k] = out[k].to_ascii_uppercase();
            }
            k += w.len();
            if i == dig_at {
                out[k] = b'0' + rnd_below(10)? as u8;
                k += 1;
            }
        }
        out[k] = 0;
        *bits = nw as f64 * (l.nwords as f64).log2()
            + if o.add_digit != 0 { 10f64.log2() + (nw as f64).log2() } else { 0.0 };
        return Some(());
    }
    const SETS: [&[u8]; 4] = [
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        b"abcdefghijklmnopqrstuvwxyz",
        b"0123456789",
        b"!#$%&*+-=?@^_~.:,;()[]{}<>/|",
    ];
    let on = [o.upper != 0, o.lower != 0, o.digits != 0, o.symbols != 0];
    let mut pool = [0u8; 160];
    let mut pn = 0usize;
    let mut cls = [[0u8; 40]; 4];
    let mut clen = [0usize; 4];
    let mut ncls = 0usize;
    for (s, set) in SETS.iter().enumerate() {
        if !on[s] {
            continue;
        }
        for &c in set.iter() {
            if o.no_ambiguous != 0 && b"Il1O0o|".contains(&c) {
                continue;
            }
            pool[pn] = c;
            pn += 1;
            cls[ncls][clen[ncls]] = c;
            clen[ncls] += 1;
        }
        ncls += 1;
    }
    if pn == 0 {
        return None;
    }
    let len = o.length.clamp(4, 128) as usize;
    if len + 1 > cap {
        return None;
    }
    let mut res = Some(());
    'tries: for _ in 0..1000 {
        for i in 0..len {
            match rnd_below(pn as u32) {
                Some(r) => out[i] = pool[r as usize],
                None => {
                    res = None;
                    break 'tries;
                }
            }
        }
        out[len] = 0;
        // at least one of each requested class
        let ok = (0..ncls).all(|c| out[..len].iter().any(|b| cls[c][..clen[c]].contains(b)));
        if ok || len < ncls {
            break;
        }
    }
    *bits = len as f64 * (pn as f64).log2();
    pool.fill(0);
    cls.iter_mut().for_each(|c| c.fill(0));
    res
}

/// Writes a password or passphrase (`o.words > 0`) into `out` (`cap` bytes) and its entropy
/// in bits into `*bits`. 0 on success.
///
/// # Safety
/// `o` is valid, `out` covers `cap` bytes and `bits` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn vg_generate(o: *const VGenOpts, out: *mut c_char, cap: usize, bits: *mut f64) -> c_int {
    if o.is_null() || out.is_null() {
        return -1;
    }
    let mut b = 0.0;
    let r = generate(&*o, std::slice::from_raw_parts_mut(out.cast::<u8>(), cap), &mut b);
    if r.is_some() && !bits.is_null() {
        *bits = b;
    }
    if r.is_some() {
        0
    } else {
        -1
    }
}

/// Position in the common list (1 = the most common) or 0: also without capitals, without
/// trailing digits and symbols, and without "leet" (p4ssw0rd → password).
fn common_rank(pw: &[u8]) -> i32 {
    let n = pw.len();
    let l = lists();
    if n == 0 || n > 64 || l.ncommon == 0 {
        return 0;
    }
    let mut low = [0u8; 72];
    let mut unleet = [0u8; 72];
    for i in 0..n {
        low[i] = pw[i].to_ascii_lowercase();
    }
    let mut r = l.exact(pw);
    if r == 0 {
        r = l.exact(&low[..n]);
    }
    // without the trailing part people usually add: password1!, qwerty2024
    let mut m = n;
    while m > 3 && (low[m - 1].is_ascii_digit() || low[m - 1].is_ascii_punctuation()) {
        m -= 1;
    }
    if r == 0 && m < n {
        r = l.exact(&low[..m]);
    }
    if r == 0 {
        for i in 0..m {
            unleet[i] = match low[i] {
                b'0' => b'o',
                b'1' => b'i',
                b'3' => b'e',
                b'4' | b'@' => b'a',
                b'5' | b'$' => b's',
                b'7' => b't',
                c => c,
            };
        }
        r = l.exact(&unleet[..m]);
        if r == 0 {
            // 1 can also be l
            for i in 0..m {
                if low[i] == b'1' {
                    unleet[i] = b'l';
                }
            }
            r = l.exact(&unleet[..m]);
        }
    }
    low.fill(0);
    unleet.fill(0);
    r
}

#[no_mangle]
pub unsafe extern "C" fn vg_common_rank(pw: *const c_char) -> c_int {
    if pw.is_null() {
        return 0;
    }
    common_rank(std::ffi::CStr::from_ptr(pw).to_bytes())
}

const ROWS: [&[u8]; 6] = [b"1234567890", b"qwertyuiop", b"asdfghjkl", b"zxcvbnm", b"qwertzuiop", b"azertyuiop"];

fn keyboard_adjacent(a: u8, b: u8) -> bool {
    let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    ROWS.iter().any(|r| match (r.iter().position(|&c| c == a), r.iter().position(|&c| c == b)) {
        (Some(p), Some(q)) => p.abs_diff(q) == 1,
        _ => false,
    })
}

/// Strength 0 (very weak) – 4 (very strong) and the estimated bits (a zxcvbn-style estimate:
/// repeats, sequences, keyboard runs, years and the common-password list lower it).
fn strength(pw: &[u8]) -> (i32, f64) {
    let n = pw.len();
    if n == 0 {
        return (0, 0.0);
    }
    let (mut lo, mut up, mut di, mut sy, mut other) = (0, 0, 0, 0, 0);
    for &c in pw {
        if c >= 0x80 {
            other = 1;
        } else if c.is_ascii_lowercase() {
            lo = 1;
        } else if c.is_ascii_uppercase() {
            up = 1;
        } else if c.is_ascii_digit() {
            di = 1;
        } else {
            sy = 1;
        }
    }
    let pool = (lo * 26 + up * 26 + di * 10 + sy * 33 + other * 100) as f64;
    let per = pool.max(10.0).log2();
    let mut bits = 0.0;
    for i in 0..n {
        if i > 0 {
            let d = pw[i] as i8 as i32 - pw[i - 1] as i8 as i32;
            if pw[i] == pw[i - 1] {
                bits += 1.0; // aaaa
                continue;
            }
            if d == 1 || d == -1 {
                bits += 1.5; // abcd, 4321
                continue;
            }
            if keyboard_adjacent(pw[i], pw[i - 1]) {
                bits += 2.0; // qwerty
                continue;
            }
        }
        bits += per;
    }
    // years (19xx/20xx) and dates count for little
    for i in 0..n.saturating_sub(3) {
        if (pw[i..].starts_with(b"19") || pw[i..].starts_with(b"20"))
            && pw[i + 2].is_ascii_digit()
            && pw[i + 3].is_ascii_digit()
        {
            bits -= 2.0 * per - 7.0;
            break;
        }
    }
    let rank = common_rank(pw);
    if rank != 0 {
        bits = (rank as f64 + 1.0).log2() + 1.0;
    }
    if bits < 0.0 {
        bits = 0.0;
    }
    let score = if rank != 0 || bits < 28.0 {
        0
    } else if bits < 36.0 {
        1
    } else if bits < 60.0 {
        2
    } else if bits < 80.0 {
        3
    } else {
        4
    };
    (score, bits)
}

/// # Safety
/// `pw` is a C string; `bits_out` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn vg_strength(pw: *const c_char, bits_out: *mut f64) -> c_int {
    let (s, b) = strength(if pw.is_null() { &[] } else { std::ffi::CStr::from_ptr(pw).to_bytes() });
    if !bits_out.is_null() {
        *bits_out = b;
    }
    s
}

/// C `strtol(s, NULL, 10)` for the count after the colon of a HIBP line.
fn strtol10(s: &[u8]) -> c_long {
    let s = &s[s.iter().take_while(|c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')).count()..];
    let (neg, d) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut v: i64 = 0;
    for &c in d.iter().take_while(|c| c.is_ascii_digit()) {
        v = v.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    if neg {
        -v
    } else {
        v
    }
}

/// strncasecmp(a, b, 40) < 0, = 0, > 0 as an ordering.
fn cmp40(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    for i in 0..40 {
        let (x, y) = (a[i].to_ascii_lowercase(), b[i].to_ascii_lowercase());
        if x != y {
            return x.cmp(&y);
        }
        if x == 0 {
            break;
        }
    }
    std::cmp::Ordering::Equal
}

/// Local HIBP file ("pwned-passwords-sha1-ordered-by-hash", lines `HASH:count`): how many times
/// `pw` appears, 0 if not, -1 if the file is unusable. Binary search by file offset.
///
/// # Safety
/// `fd` is an open file; `pw` is a C string.
#[no_mangle]
pub unsafe extern "C" fn vg_hibp_lookup(fd: c_int, pw: *const c_char) -> c_long {
    use std::cmp::Ordering::*;
    if pw.is_null() {
        return -1;
    }
    let mut st: libc::stat = std::mem::zeroed();
    if libc::fstat(fd, &mut st) != 0 || st.st_size < 42 {
        return -1;
    }
    let mut h = [0u8; 20];
    sha1(pw.cast::<c_void>(), std::ffi::CStr::from_ptr(pw).to_bytes().len(), h.as_mut_ptr());
    let mut hex = [0u8; 40];
    for (i, b) in h.iter().enumerate() {
        hex[2 * i] = b"0123456789ABCDEF"[(b >> 4) as usize];
        hex[2 * i + 1] = b"0123456789ABCDEF"[(b & 15) as usize];
    }
    let (mut lo, mut hi) = (0i64, st.st_size as i64);
    let mut buf = [0u8; 256];
    let rd = |buf: &mut [u8; 256], off: i64| -> isize {
        let r = libc::pread(fd, buf.as_mut_ptr().cast::<c_void>(), 255, off as libc::off_t);
        if r > 0 {
            buf[r as usize] = 0;
        }
        r
    };
    let mut iter = 0;
    while iter < 64 && lo < hi {
        iter += 1;
        let mid = lo + (hi - lo) / 2;
        // go to the start of the line after mid (or mid if it is 0)
        let mut r = rd(&mut buf, mid);
        if r <= 0 {
            break;
        }
        let mut start = mid;
        if mid != 0 {
            let Some(nl) = buf[..r as usize].iter().position(|&c| c == b'\n') else {
                hi = mid;
                continue;
            };
            start = mid + nl as i64 + 1;
            r = rd(&mut buf, start);
            if r <= 0 {
                hi = mid;
                continue;
            }
        }
        if r < 41 {
            hi = mid;
            continue;
        }
        match cmp40(&buf, &hex) {
            Equal => {
                let end = buf.iter().position(|&c| c == 0).unwrap_or(255);
                return if buf[40] == b':' { strtol10(&buf[41..end]) } else { 1 };
            }
            Less => lo = start + 1,
            Greater => hi = mid,
        }
    }
    // the search may skip the first line: check it separately
    let r = rd(&mut buf, 0);
    if r >= 41 && cmp40(&buf, &hex) == Equal {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(255);
        return strtol10(&buf[41..end]);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::io::Write;
    use std::os::fd::AsRawFd;

    static WORDS: &[u8] = include_bytes!("../../../third_party/wordlists/eff_large_wordlist.txt");
    static COMMON: &[u8] = include_bytes!("../../../third_party/wordlists/10k-most-common.txt");

    fn setup() {
        unsafe { vg_set_lists(WORDS.as_ptr().cast(), WORDS.len(), COMMON.as_ptr().cast(), COMMON.len()) };
    }
    fn opts(f: impl FnOnce(&mut VGenOpts)) -> VGenOpts {
        let mut o = VGenOpts {
            length: 20,
            upper: 0,
            lower: 0,
            digits: 0,
            symbols: 0,
            no_ambiguous: 0,
            words: 0,
            sep: [0; 8],
            capitalize: 0,
            add_digit: 0,
        };
        f(&mut o);
        o
    }
    fn gen(o: &VGenOpts) -> (String, f64) {
        let mut out = [0 as c_char; 256];
        let mut bits = 0.0;
        assert_eq!(unsafe { vg_generate(o, out.as_mut_ptr(), 256, &mut bits) }, 0);
        (unsafe { std::ffi::CStr::from_ptr(out.as_ptr()) }.to_str().unwrap().to_string(), bits)
    }
    fn strength_of(s: &str) -> i32 {
        let c = CString::new(s).unwrap();
        unsafe { vg_strength(c.as_ptr(), std::ptr::null_mut()) }
    }
    fn rank(s: &str) -> i32 {
        let c = CString::new(s).unwrap();
        unsafe { vg_common_rank(c.as_ptr()) }
    }

    #[test]
    fn layout_matches_c() {
        assert_eq!(std::mem::size_of::<VGenOpts>(), 44);
    }

    #[test]
    fn password_and_passphrase() {
        setup();
        assert_eq!(vg_word_count(), 7776);
        let o = opts(|o| {
            o.length = 24;
            o.upper = 1;
            o.lower = 1;
            o.digits = 1;
            o.symbols = 1;
            o.no_ambiguous = 1;
        });
        for _ in 0..200 {
            let (pw, bits) = gen(&o);
            assert_eq!(pw.len(), 24);
            assert!(pw.bytes().any(|b| b"ABCDEFGHJKLMNPQRSTUVWXYZ".contains(&b)));
            assert!(pw.bytes().any(|b| b"abcdefghijkmnpqrstuvwxyz".contains(&b)));
            assert!(pw.bytes().any(|b| b"23456789".contains(&b)));
            assert!(!pw.bytes().any(|b| b"Il1O0o|".contains(&b)));
            assert!(bits > 140.0);
        }
        assert!(strength_of(&gen(&o).0) >= 3);
        let p = opts(|o| {
            o.words = 6;
            o.capitalize = 1;
            o.add_digit = 1;
            o.sep[0] = b'.';
        });
        let (pw, bits) = gen(&p);
        assert_eq!(pw.matches('.').count(), 5);
        assert!(bits > 77.0);
        assert!(pw.split('.').all(|w| w.chars().next().unwrap().is_ascii_uppercase()));
        assert_eq!(pw.bytes().filter(u8::is_ascii_digit).count(), 1);
        // too small a buffer, no class selected
        let mut tiny = [0 as c_char; 8];
        assert_eq!(unsafe { vg_generate(&p, tiny.as_mut_ptr(), 8, std::ptr::null_mut()) }, -1);
        let none = opts(|_| {});
        assert_eq!(unsafe { vg_generate(&none, tiny.as_mut_ptr(), 8, std::ptr::null_mut()) }, -1);
        // short passwords are clamped to 4
        assert_eq!(gen(&opts(|o| { o.length = 1; o.lower = 1; })).0.len(), 4);
        // the randomness is uniform enough to cover the whole pool
        let mut seen = std::collections::HashSet::new();
        for _ in 0..300 {
            seen.extend(gen(&opts(|o| { o.length = 128; o.digits = 1; })).0.bytes());
        }
        assert_eq!(seen.len(), 10);
    }

    #[test]
    fn strength_and_common() {
        setup();
        assert!(rank("password") > 0);
        assert!(rank("P4ssw0rd!") > 0);
        assert_eq!(rank("password"), rank("PASSWORD"));
        assert_eq!(rank(&"x".repeat(65)), 0);
        assert_eq!(strength_of("password"), 0);
        assert_eq!(strength_of("qwerty123"), 0);
        assert!(strength_of("aaaaaaaaaaaa") <= 1);
        assert!(strength_of("Tr0ub4dour&3-kangaroo-Zebra") >= 3);
        assert_eq!(strength_of(""), 0);
        let mut bits = 0.0;
        let c = CString::new("").unwrap();
        unsafe { vg_strength(c.as_ptr(), &mut bits) };
        assert_eq!(bits, 0.0);
    }

    #[test]
    fn hibp_file() {
        let mut lines: Vec<(String, u32)> = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta"]
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let c = CString::new(*w).unwrap();
                let mut h = [0u8; 20];
                unsafe { sha1(c.as_ptr().cast(), w.len(), h.as_mut_ptr()) };
                (h.iter().map(|b| format!("{b:02X}")).collect(), 100 + i as u32)
            })
            .collect();
        lines.sort();
        let mut f = tempfile_in_tmp();
        for (h, n) in &lines {
            writeln!(f.0, "{h}:{n}").unwrap();
        }
        for (i, w) in ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta"].iter().enumerate() {
            let c = CString::new(*w).unwrap();
            assert_eq!(unsafe { vg_hibp_lookup(f.0.as_raw_fd(), c.as_ptr()) }, 100 + i as c_long, "{w}");
        }
        let c = CString::new("not-there").unwrap();
        assert_eq!(unsafe { vg_hibp_lookup(f.0.as_raw_fd(), c.as_ptr()) }, 0);
        let tiny = tempfile_in_tmp();
        assert_eq!(unsafe { vg_hibp_lookup(tiny.0.as_raw_fd(), c.as_ptr()) }, -1);
    }

    struct Tmp(std::fs::File);
    fn tempfile_in_tmp() -> Tmp {
        let p = std::env::temp_dir().join(format!("vgen-hibp-{}-{:?}", std::process::id(), std::thread::current().id()));
        let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&p).unwrap();
        std::fs::remove_file(&p).unwrap();
        Tmp(f)
    }
}
