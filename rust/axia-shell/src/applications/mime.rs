use super::*;

pub(super) fn valid(m: &[u8]) -> bool {
    if !(3..128).contains(&m.len()) {
        return false;
    }
    let parts: Vec<_> = m.split(|b| *b == b'/').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && (p[0].is_ascii_lowercase() || p[0].is_ascii_digit())
                && p.iter()
                    .all(|b| b.is_ascii_alphanumeric() || b".+_-".contains(b))
        })
}
pub(super) fn desktop_valid(id: &[u8]) -> bool {
    (9..160).contains(&id.len())
        && id.ends_with(b".desktop")
        && !b".-".contains(&id[0])
        && !id.windows(2).any(|w| w == b"..")
        && id
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
}
fn trim(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|b| !b" \t".contains(b))
        .unwrap_or(s.len());
    let s = &s[start..];
    let end = s
        .iter()
        .rposition(|b| !b" \t\r".contains(b))
        .map_or(0, |i| i + 1);
    &s[..end]
}
fn group(line: &[u8]) -> Option<&[u8]> {
    let line = trim(line);
    if line.len() >= 2 && line[0] == b'[' && line.last() == Some(&b']') {
        Some(&line[1..line.len() - 1])
    } else {
        None
    }
}
fn key<'a>(line: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let line = trim(line);
    let rest = line.strip_prefix(name)?;
    let start = rest
        .iter()
        .position(|b| !b" \t".contains(b))
        .unwrap_or(rest.len());
    Some(trim(rest[start..].strip_prefix(b"=")?))
}
fn find<'a>(text: &'a [u8], grp: &[u8], name: &[u8]) -> Option<&'a [u8]> {
    let mut inside = false;
    for line in text.split(|b| *b == b'\n') {
        if let Some(g) = group(line) {
            inside = g == grp;
        } else if inside {
            if let Some(v) = key(line, name) {
                return Some(v);
            }
        }
    }
    None
}
pub(super) fn entries<'a>(
    text: &'a [u8],
    grp: &[u8],
    name: &[u8],
) -> impl Iterator<Item = &'a [u8]> {
    find(text, grp, name)
        .unwrap_or_default()
        .split(|b| *b == b';')
        .filter(|s| desktop_valid(s))
}
pub(super) fn removed(text: &[u8], name: &[u8], id: &[u8]) -> bool {
    find(text, b"Removed Associations", name).is_some_and(|s| list_has(s, id))
}
fn put_key(out: &mut Vec<u8>, name: &[u8], id: &[u8], old: &[u8], keep: bool) {
    out.extend_from_slice(name);
    out.push(b'=');
    out.extend_from_slice(id);
    out.push(b';');
    if keep {
        for v in old
            .split(|b| *b == b';')
            .filter(|v| !v.is_empty() && *v != id)
        {
            out.extend_from_slice(v);
            out.push(b';');
        }
    }
    out.push(b'\n');
}
fn set_list(text: &[u8], grp: &[u8], name: &[u8], id: &[u8], keep: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut inside, mut done, mut held) = (false, false, 0);
    for raw in text.split_inclusive(|b| *b == b'\n') {
        let line = raw.strip_suffix(b"\n").unwrap_or(raw);
        let hdr = group(line);
        if hdr.is_some() && inside && !done {
            put_key(&mut out, name, id, b"", false);
            done = true;
        }
        if let Some(g) = hdr {
            inside = g == grp;
        }
        if hdr.is_none() && inside && !done && trim(line).is_empty() {
            held += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(b'\n', held));
        held = 0;
        if hdr.is_none() && inside && !done {
            if let Some(v) = key(line, name) {
                put_key(&mut out, name, id, v, keep);
                done = true;
                continue;
            }
        }
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    if inside && !done {
        put_key(&mut out, name, id, b"", false);
        done = true;
    }
    out.extend(std::iter::repeat_n(b'\n', held));
    if !done {
        if !out.is_empty() {
            out.push(b'\n');
        }
        out.push(b'[');
        out.extend_from_slice(grp);
        out.extend_from_slice(b"]\n");
        put_key(&mut out, name, id, b"", false);
    }
    out
}
pub(super) fn set(text: &[u8], name: &[u8], id: &[u8]) -> Vec<u8> {
    set_list(
        &set_list(text, b"Default Applications", name, id, false),
        b"Added Associations",
        name,
        id,
        true,
    )
}
pub(super) fn unset(text: &[u8], name: &[u8]) -> Vec<u8> {
    let mut inside = false;
    let mut out = Vec::new();
    for raw in text.split_inclusive(|b| *b == b'\n') {
        let line = raw.strip_suffix(b"\n").unwrap_or(raw);
        if let Some(g) = group(line) {
            inside = g == b"Default Applications" || g == b"Added Associations";
        } else if inside && key(line, name).is_some() {
            continue;
        }
        out.extend_from_slice(raw);
    }
    out
}
pub(super) fn guess(globs: &[u8], name: &[u8], capacity: usize) -> Option<Vec<u8>> {
    let base = name.rsplit(|b| *b == b'/').next()?;
    if name.is_empty() || base.len() >= 256 {
        return None;
    }
    let mut best = (-1, 0, false);
    let mut found = None;
    for line in globs
        .split(|b| *b == b'\n')
        .filter(|s| !s.is_empty() && s[0] != b'#' && s.len() < 512)
    {
        let f: Vec<_> = line.splitn(4, |b| *b == b':').collect();
        if f.len() < 3 {
            continue;
        }
        // atoi-compatible prefix (including signs) used by the previous reader.
        let weight = cstring(f[0]);
        let w = unsafe { libc::atoi(weight.as_ptr()) };
        let cs = f.get(3).is_some_and(|v| v.windows(2).any(|w| w == b"cs"));
        let rank = (w, f[2].len(), cs);
        if w < 0
            || rank <= best
            || !valid(f[1])
            || f[1].len() >= capacity
            || (!cs && f[2].len() >= 256)
        {
            continue;
        }
        let pattern = cstring(&if cs {
            f[2].to_vec()
        } else {
            f[2].to_ascii_lowercase()
        });
        let input = cstring(&if cs {
            base.to_vec()
        } else {
            base.to_ascii_lowercase()
        });
        if unsafe { libc::fnmatch(pattern.as_ptr(), input.as_ptr(), 0) } == 0 {
            best = rank;
            found = Some(f[1].to_vec());
        }
    }
    found
}

#[no_mangle]
pub unsafe extern "C" fn mime_valid(m: *const c_char) -> i32 {
    valid(bytes(m)) as i32
}
#[no_mangle]
pub unsafe extern "C" fn desktop_id_valid(id: *const c_char) -> i32 {
    desktop_valid(bytes(id)) as i32
}
#[no_mangle]
pub unsafe extern "C" fn mimeapps_entry(
    t: *const c_char,
    g: *const c_char,
    m: *const c_char,
    i: i32,
    out: *mut c_char,
    n: usize,
) -> i32 {
    if t.is_null() || g.is_null() || i < 0 {
        return -1;
    }
    entries(bytes(t), bytes(g), bytes(m))
        .filter(|v| v.len() < n)
        .nth(i as usize)
        .map_or(-1, |v| output(out, n, v))
}
#[no_mangle]
pub unsafe extern "C" fn mimeapps_default(
    t: *const c_char,
    m: *const c_char,
    out: *mut c_char,
    n: usize,
) -> i32 {
    mimeapps_entry(t, c"Default Applications".as_ptr(), m, 0, out, n)
}
#[no_mangle]
pub unsafe extern "C" fn mimeapps_removed(
    t: *const c_char,
    m: *const c_char,
    id: *const c_char,
) -> i32 {
    removed(bytes(t), bytes(m), bytes(id)) as i32
}
#[no_mangle]
pub unsafe extern "C" fn mimeapps_set_default(
    t: *const c_char,
    m: *const c_char,
    id: *const c_char,
) -> *mut c_char {
    if !valid(bytes(m)) || !desktop_valid(bytes(id)) {
        return std::ptr::null_mut();
    }
    allocated(&set(bytes(t), bytes(m), bytes(id)))
}
#[no_mangle]
pub unsafe extern "C" fn mimeapps_unset_default(t: *const c_char, m: *const c_char) -> *mut c_char {
    if !valid(bytes(m)) {
        return std::ptr::null_mut();
    }
    allocated(&unset(bytes(t), bytes(m)))
}
#[no_mangle]
pub unsafe extern "C" fn mime_guess(
    t: *const c_char,
    name: *const c_char,
    out: *mut c_char,
    n: usize,
) -> i32 {
    guess(bytes(t), bytes(name), n).map_or(-1, |v| output(out, n, &v))
}
