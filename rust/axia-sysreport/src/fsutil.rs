//! Small reads of sysfs, procfs and friends, as the collector makes them: a leaf is opened with
//! O_NOFOLLOW, an attribute is cut to its first line and to printable ASCII, directories are listed
//! in name order (natural: "card2" before "card10") so two runs on one machine give the same report.
use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;

/// The first `size - 1` bytes (a C buffer of `size`).
pub fn fit(s: &str, size: usize) -> String {
    if s.len() < size {
        return s.to_string();
    }
    let mut n = size.saturating_sub(1);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s[..n].to_string()
}

fn printable(c: u8) -> char {
    if (0x20..0x7f).contains(&c) { c as char } else { '?' }
}

/// A whole small file (at most `max` bytes); the leaf is not followed if it is a link.
pub fn read_file(path: &str, max: usize) -> Option<Vec<u8>> {
    let f = fs::OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(path).ok()?;
    let mut b = Vec::new();
    f.take(max as u64).read_to_end(&mut b).ok()?;
    Some(b)
}

/// Like `read_file` but a link at the leaf is followed (procfs files).
pub fn read_file_following(path: &str, max: usize) -> Option<Vec<u8>> {
    let f = fs::File::open(path).ok()?;
    let mut b = Vec::new();
    f.take(max as u64).read_to_end(&mut b).ok()?;
    Some(b)
}

/// The same, as text up to its first NUL (a C string) with invalid bytes as `?`.
pub fn read_text(path: &str, max: usize) -> Option<String> {
    let b = read_file(path, max)?;
    let b = &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())];
    Some(b.iter().map(|&c| if c < 0x80 { c as char } else { '?' }).collect())
}

/// The first line of an attribute: trimmed, printable ASCII only, at most `size - 1` characters.
/// "" when missing.
pub fn read_attr(path: &str, size: usize) -> String {
    read_attr_limit(path, size, 511)
}

/// `read_attr` reading at most `limit` bytes of the file (the periph reader looks at 1023).
pub fn read_attr_limit(path: &str, size: usize, limit: u64) -> String {
    let Some(f) = fs::OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW).open(path).ok() else { return String::new() };
    let mut buf = Vec::new();
    if f.take(limit).read_to_end(&mut buf).is_err() || buf.is_empty() {
        return String::new();
    }
    let mut i = 0;
    while i < buf.len() && (buf[i] == b' ' || buf[i] == b'\t') {
        i += 1;
    }
    let mut out = String::new();
    while i < buf.len() && buf[i] != 0 && buf[i] != b'\n' && out.len() + 1 < size {
        out.push(printable(buf[i]));
        i += 1;
    }
    while out.ends_with([' ', '\t']) {
        out.pop();
    }
    out
}

pub fn attr(root: &str, dir: &str, name: &str, size: usize) -> String {
    read_attr(&format!("{root}{dir}/{name}"), size)
}

/// strtoll(s, &end, 0): blanks, a sign, then decimal, 0x hexadecimal or leading-0 octal; `None` when no
/// digits were read.
pub fn strtoll0(s: &str) -> Option<i64> {
    let b = s.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b').as_bytes();
    let (neg, mut i) = match b.first() {
        Some(b'-') => (true, 1),
        Some(b'+') => (false, 1),
        _ => (false, 0),
    };
    let mut radix = 10u32;
    if b.len() > i + 1 && b[i] == b'0' && (b[i + 1] | 0x20) == b'x' && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit) {
        radix = 16;
        i += 2;
    } else if b.get(i) == Some(&b'0') {
        radix = 8;
    }
    let start = i;
    let mut v: i64 = 0;
    while i < b.len() {
        let Some(d) = (b[i] as char).to_digit(radix) else { break };
        v = v.saturating_mul(radix as i64).saturating_add(d as i64);
        i += 1;
    }
    if i == start {
        return None;
    }
    Some(if neg { -v } else { v })
}

pub fn attr_num(root: &str, dir: &str, name: &str, def: i64) -> i64 {
    let v = attr(root, dir, name, 64);
    if v.is_empty() { def } else { strtoll0(&v).unwrap_or(def) }
}

/// atoi / strtol(s, NULL, 10) on the digits at the start.
pub fn atoi(s: &str) -> i64 {
    let s = s.trim_start();
    let (neg, d) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let v: i64 = d.bytes().take_while(u8::is_ascii_digit).fold(0i64, |a, c| a.saturating_mul(10).saturating_add((c - b'0') as i64));
    if neg { -v } else { v }
}

pub fn exists(root: &str, path: &str) -> bool {
    fs::symlink_metadata(format!("{root}{path}")).is_ok()
}

/// A hex ID attribute ("0x10de" or "10de") as `digits` lowercase hex digits; "" if it is not hex.
pub fn hex_attr(root: &str, dir: &str, name: &str, digits: usize) -> String {
    let v = attr(root, dir, name, 32);
    let s = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")).unwrap_or(&v);
    if s.is_empty() || s.len() > digits || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return String::new();
    }
    format!("{}{}", "0".repeat(digits - s.len()), s.to_ascii_lowercase())
}

/// The last component of a link (a device's "driver", "subsystem"), [A-Za-z0-9_-] only.
pub fn link_base(root: &str, dir: &str, name: &str, size: usize) -> String {
    let Ok(l) = fs::read_link(format!("{root}{dir}/{name}")) else { return String::new() };
    let l = l.as_os_str().as_encoded_bytes();
    if l.is_empty() {
        return String::new();
    }
    let l = &l[..l.len().min(511)];
    let b = l.rsplit(|&c| c == b'/').next().unwrap_or(l);
    b.iter().filter(|c| c.is_ascii_alphanumeric() || **c == b'_' || **c == b'-').take(size - 1).map(|&c| c as char).collect()
}

/// "card2" sorts before "card10"; names with another text before the digits sort as text.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let digits = |s: &str| s.bytes().position(|c| c.is_ascii_digit()).unwrap_or(s.len());
    let (la, lb) = (digits(a), digits(b));
    if la == lb && a[..la] == b[..lb] && la < a.len() && lb < b.len() {
        let (na, nb) = (atoi(&a[la..]), atoi(&b[lb..]));
        if na != nb {
            return na.cmp(&nb);
        }
    }
    a.cmp(b)
}

/// The names in `<root><dir>` (no dot entries, at most 4096), in natural order.
pub fn list_dir(root: &str, dir: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if let Ok(d) = fs::read_dir(format!("{root}{dir}")) {
        for e in d.flatten() {
            if v.len() >= 4096 {
                break;
            }
            if let Ok(n) = e.file_name().into_string() {
                if !n.starts_with('.') {
                    v.push(n);
                }
            }
        }
    }
    // an insertion sort, so equal and odd cases keep the order they always had
    for i in 1..v.len() {
        let mut j = i;
        while j > 0 && natural_cmp(&v[j - 1], &v[j]) == std::cmp::Ordering::Greater {
            v.swap(j - 1, j);
            j -= 1;
        }
    }
    v
}

/// "key<spaces>: value" in /proc/cpuinfo-style text (the first one); "" when missing.
pub fn field(txt: &str, key: &str, size: usize) -> String {
    for line in txt.split('\n') {
        let Some(rest) = line.strip_prefix(key) else { continue };
        if !matches!(rest.bytes().next(), Some(b' ' | b'\t' | b':')) {
            continue;
        }
        let Some(colon) = line.find(':') else { continue };
        let v = line[colon + 1..].trim_start_matches([' ', '\t']);
        let mut out: String = v.bytes().take(size - 1).map(printable).collect();
        while out.ends_with(' ') {
            out.pop();
        }
        return out;
    }
    String::new()
}

/// os-release: KEY=value or KEY="value".
pub fn osr_field(txt: &str, key: &str, size: usize) -> String {
    for line in txt.split('\n') {
        let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix('=')) else { continue };
        let (q, v) = match rest.bytes().next() {
            Some(c @ (b'"' | b'\'')) => (Some(c), &rest[1..]),
            _ => (None, rest),
        };
        return v.bytes().take_while(|&c| Some(c) != q).take(size - 1).map(printable).collect();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_like_strtoll_with_base_zero() {
        assert_eq!((strtoll0("0x10"), strtoll0("010"), strtoll0("-5"), strtoll0("  42abc"), strtoll0("x")), (Some(16), Some(8), Some(-5), Some(42), None));
        assert_eq!((strtoll0("0"), strtoll0("0x"), strtoll0("+7"), strtoll0("")), (Some(0), Some(0), Some(7), None));
        assert_eq!((atoi("12abc"), atoi(" -3"), atoi("x"), atoi("")), (12, -3, 0, 0));
    }

    #[test]
    fn names_sort_naturally() {
        let mut v = ["card10", "card2", "card1", "renderD128", "card1-HDMI"].map(String::from).to_vec();
        for i in 1..v.len() {
            let mut j = i;
            while j > 0 && natural_cmp(&v[j - 1], &v[j]).is_gt() {
                v.swap(j - 1, j);
                j -= 1;
            }
        }
        assert_eq!(v, ["card1", "card1-HDMI", "card2", "card10", "renderD128"]);
    }

    #[test]
    fn cpuinfo_and_os_release_fields() {
        let t = "processor\t: 0\nmodel name\t: AMD  Ryzen\u{e9} 7  \nflags\t\t: a b\nmodel namex: no\n";
        assert_eq!(field(t, "model name", 96), "AMD  Ryzen?? 7");
        assert_eq!(field(t, "flags", 160), "a b");
        assert_eq!(field(t, "missing", 8), "");
        assert_eq!(field(t, "model name", 6), "AMD");
        let o = "NAME=\"Deb\"\nPRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\nID=debian\nVERSION_ID='13'\n";
        assert_eq!((osr_field(o, "PRETTY_NAME", 96).as_str(), osr_field(o, "ID", 24).as_str(), osr_field(o, "VERSION_ID", 24).as_str()), ("Debian GNU/Linux 13 (trixie)", "debian", "13"));
        assert_eq!(osr_field(o, "NAME", 96), "Deb");
        assert_eq!(fit("abcdef", 4), "abc");
        assert_eq!(fit("ab", 4), "ab");
    }
}
