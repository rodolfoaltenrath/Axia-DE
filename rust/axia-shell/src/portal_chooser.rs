//! The options of an `org.freedesktop.portal.FileChooser` call (`OpenFile`, `SaveFile`,
//! `SaveFiles`): the body of the D-Bus message, `ssa{sv}`, which the transport has validated.
//!
//! The rule is that a request for a file always opens the dialog. What cannot be understood or is
//! too big is left out (a filter that cannot be honored shows every file); nothing refuses the
//! request, because an application that is refused often shows nothing at all.

use std::ffi::{c_char, c_int, CStr};

const TOKEN: usize = 129;
const MODE: usize = 12;
const TITLE: usize = 1025;
const FOLDER: usize = 4096;
const NAME: usize = 16384;
/// The dialog takes at most this many bytes of patterns (apps/files/external.rs).
const PATTERNS: usize = 16384;
const CHOICES: usize = 4096;
/// More patterns than this in the filter that applies: every file is shown.
const MOST_PATTERNS: usize = 2048;
const DEPTH: u32 = 32;

/// `AxiaChooser` of server/src/ui/portal_files.c. Every string ends in a NUL.
#[repr(C)]
pub struct Chooser {
    token: [u8; TOKEN],
    mode: [u8; MODE],
    title: [u8; TITLE],
    folder: [u8; FOLDER],
    name: [u8; NAME],
    /// Lines of `g:<glob>` or `m:<mime type>`; empty shows every file.
    patterns: [u8; PATTERNS],
    /// Lines of `<id>\t<value>`: the application's extra choices, answered with their defaults.
    choices: [u8; CHOICES],
    multiple: c_int,
    filters: u32,
    /// Something of the request was left out.
    ignored: c_int,
}

/// A bounds-checked reader of the D-Bus wire format. `pos` is relative to the body, which starts
/// on an 8-byte boundary.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    big: bool,
}
impl<'a> Reader<'a> {
    fn align(&mut self, to: usize) -> Option<()> {
        let next = self.pos.checked_add(to - 1)? / to * to;
        (next <= self.bytes.len()).then(|| self.pos = next)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }
    fn u32(&mut self) -> Option<u32> {
        self.align(4)?;
        let b: [u8; 4] = self.take(4)?.try_into().ok()?;
        Some(if self.big { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
    }
    /// `s` and `o`: a length, the bytes and a NUL.
    fn string(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        let out = self.take(n)?;
        (self.take(1)? == [0]).then_some(out)
    }
    fn signature(&mut self) -> Option<&'a [u8]> {
        let n = usize::from(self.take(1)?[0]);
        let out = self.take(n)?;
        (self.take(1)? == [0]).then_some(out)
    }
    /// `ay`.
    fn bytes_array(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    /// The end of an array whose elements align to `to`.
    fn array(&mut self, to: usize) -> Option<usize> {
        let n = self.u32()? as usize;
        self.align(to)?;
        let end = self.pos.checked_add(n)?;
        (end <= self.bytes.len()).then_some(end)
    }
    /// Calls `each` for every element of an array.
    fn each(&mut self, to: usize, mut each: impl FnMut(&mut Self) -> Option<()>) -> Option<()> {
        let end = self.array(to)?;
        while self.pos < end {
            let before = self.pos;
            each(self)?;
            if self.pos <= before {
                return None;
            }
        }
        (self.pos == end).then_some(())
    }
    /// Passes over one value of the first complete type in `signature`; returns the rest.
    fn skip<'s>(&mut self, signature: &'s [u8], depth: u32) -> Option<&'s [u8]> {
        let (&kind, rest) = signature.split_first()?;
        if depth > DEPTH {
            return None;
        }
        match kind {
            b'y' => drop(self.take(1)?),
            b'n' | b'q' => {
                self.align(2)?;
                self.take(2)?;
            }
            b'b' | b'i' | b'u' | b'h' => drop(self.u32()?),
            b'x' | b't' | b'd' => {
                self.align(8)?;
                self.take(8)?;
            }
            b's' | b'o' => drop(self.string()?),
            b'g' => drop(self.signature()?),
            b'v' => {
                let inner = self.signature()?;
                self.skip(inner, depth + 1)?;
            }
            b'a' => {
                let element = *rest.first()?;
                let alignment = match element {
                    b'(' | b'{' | b'x' | b't' | b'd' => 8,
                    b'n' | b'q' => 2,
                    b'y' | b'g' | b'v' => 1,
                    _ => 4,
                };
                let after = after_type(rest, depth + 1)?;
                let end = self.array(alignment)?;
                self.pos = end;
                return Some(after);
            }
            b'(' | b'{' => {
                self.align(8)?;
                let mut inside = rest;
                while !matches!(inside.first()?, b')' | b'}') {
                    inside = self.skip(inside, depth + 1)?;
                }
                return Some(&inside[1..]);
            }
            _ => return None,
        }
        Some(rest)
    }
}
/// What follows the first complete type of `signature`.
fn after_type(signature: &[u8], depth: u32) -> Option<&[u8]> {
    let (&kind, rest) = signature.split_first()?;
    if depth > DEPTH {
        return None;
    }
    match kind {
        b'a' => after_type(rest, depth + 1),
        b'(' | b'{' => {
            let mut inside = rest;
            while !matches!(inside.first()?, b')' | b'}') {
                inside = after_type(inside, depth + 1)?;
            }
            Some(&inside[1..])
        }
        _ => Some(rest),
    }
}

fn clean(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok().filter(|s| !s.contains('\0'))
}
fn file_name(bytes: &[u8]) -> Option<&str> {
    clean(bytes).filter(|s| !s.is_empty() && s.len() <= 255 && !s.contains(['/', '\n']) && *s != "." && *s != "..")
}
/// A NUL-terminated absolute path sent as bytes.
fn path(bytes: &[u8]) -> Option<&str> {
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    clean(bytes).filter(|s| s.starts_with('/') && s.len() < FOLDER)
}
/// The longest start of `text` that fits in `most` bytes.
fn cut(text: &str, most: usize) -> &str {
    let mut end = text.len().min(most);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The patterns of one filter, `(sa(us))`, as the dialog's lines.
#[derive(Default)]
struct Filter {
    lines: Vec<String>,
    /// One of the patterns accepts every file.
    all: bool,
    incomplete: bool,
}
fn filter(r: &mut Reader) -> Option<Filter> {
    r.align(8)?;
    r.string()?;
    let mut out = Filter::default();
    r.each(8, |r| {
        r.align(8)?;
        let kind = r.u32()?;
        let value = r.string()?;
        match clean(value).filter(|v| !v.is_empty() && v.len() <= 1024 && !v.contains('\n')) {
            Some(v) if kind <= 1 => {
                out.all |= matches!((kind, v), (0, "*" | "*.*") | (1, "*/*" | "application/octet-stream"));
                out.lines.push(format!("{}:{v}", if kind == 0 { 'g' } else { 'm' }));
            }
            _ => out.incomplete = true,
        }
        Some(())
    })?;
    Some(out)
}

#[derive(Default)]
struct Options {
    token: String,
    title: String,
    folder: String,
    name: String,
    files: Vec<String>,
    multiple: bool,
    directory: bool,
    filters: Vec<Filter>,
    current: Option<Filter>,
    choices: Vec<(String, String)>,
    ignored: bool,
}

/// `(ssa(ss)s)`: id, label, the options and the one selected at first.
fn choice(r: &mut Reader, out: &mut Options) -> Option<()> {
    r.align(8)?;
    let id = r.string()?;
    r.string()?;
    let mut first = None;
    r.each(8, |r| {
        r.align(8)?;
        let option = r.string()?;
        r.string()?;
        first.get_or_insert(option);
        Some(())
    })?;
    let initial = r.string()?;
    // without options it is a check box
    let value = Some(initial).filter(|v| !v.is_empty()).or(first).unwrap_or(b"false");
    let plain = |b: &[u8]| clean(b).filter(|s| !s.contains(['\t', '\n']) && s.len() <= 256).map(str::to_owned);
    match (plain(id), plain(value)) {
        (Some(id), Some(value)) if !id.is_empty() => out.choices.push((id, value)),
        _ => out.ignored = true,
    }
    Some(())
}

fn option(r: &mut Reader, out: &mut Options) -> Option<()> {
    r.align(8)?;
    let key = r.string()?;
    let signature = r.signature()?;
    match (key, signature) {
        (b"handle_token", b"s") => {
            let token = r.string()?;
            match clean(token).filter(|t| !t.is_empty() && t.len() < TOKEN && t.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')) {
                Some(token) => out.token = token.to_owned(),
                None => out.ignored = true,
            }
        }
        (b"multiple", b"b") => out.multiple = r.u32()? != 0,
        (b"directory", b"b") => out.directory = r.u32()? != 0,
        (b"current_name", b"s") => match file_name(r.string()?) {
            Some(name) => out.name = name.to_owned(),
            None => out.ignored = true,
        },
        (b"current_folder", b"ay") => match path(r.bytes_array()?) {
            Some(folder) => out.folder = folder.to_owned(),
            None => out.ignored = true,
        },
        (b"current_file", b"ay") => match path(r.bytes_array()?).and_then(|p| p.rsplit_once('/')) {
            Some((folder, name)) => {
                out.folder = if folder.is_empty() { "/".to_owned() } else { folder.to_owned() };
                if let Some(name) = file_name(name.as_bytes()) {
                    out.name = name.to_owned();
                }
            }
            None => out.ignored = true,
        },
        (b"filters", b"a(sa(us))") => r.each(8, |r| {
            out.filters.push(filter(r)?);
            Some(())
        })?,
        (b"current_filter", b"(sa(us))") => out.current = Some(filter(r)?),
        (b"files", b"aay") => r.each(4, |r| {
            let bytes = r.bytes_array()?;
            match file_name(bytes.strip_suffix(&[0]).unwrap_or(bytes)) {
                Some(name) if out.files.len() < 128 => out.files.push(name.to_owned()),
                _ => out.ignored = true,
            }
            Some(())
        })?,
        (b"choices", b"a(ssa(ss)s)") => r.each(8, |r| choice(r, out))?,
        // "modal", "accept_label" and whatever comes later
        _ => drop(r.skip(signature, 0)?),
    }
    Some(())
}

/// What of the request could be read; a broken tail leaves what came before it.
fn read(body: &[u8], big: bool) -> Options {
    let mut r = Reader { bytes: body, pos: 0, big };
    let mut out = Options::default();
    let complete = (|| {
        r.string()?; // the parent window
        let title = r.string()?;
        out.title = String::from_utf8_lossy(title).replace('\0', "");
        r.each(8, |r| option(r, &mut out))
    })();
    out.ignored |= complete.is_none();
    out
}

/// The lines of the filter that applies: the current one, otherwise all of them together (the
/// dialog has no filter menu, and hiding a file the application accepts is worse than showing one
/// it does not). Empty shows every file.
fn patterns(options: &mut Options) -> String {
    let chosen: Vec<&Filter> = match options.current.as_ref().filter(|f| !f.lines.is_empty()) {
        Some(current) => vec![current],
        None => options.filters.iter().collect(),
    };
    if chosen.iter().any(|f| f.all) {
        return String::new();
    }
    if chosen.iter().any(|f| f.incomplete) {
        options.ignored = true;
        return String::new();
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = String::new();
    for line in chosen.iter().flat_map(|f| &f.lines) {
        if !seen.insert(line.as_str()) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
        if seen.len() > MOST_PATTERNS || out.len() >= PATTERNS {
            options.ignored = true;
            return String::new();
        }
    }
    out
}

fn put(into: &mut [u8], text: &str) {
    let text = cut(text, into.len() - 1).as_bytes();
    into[..text.len()].copy_from_slice(text);
    into[text.len()..].fill(0);
}

fn fill(out: &mut Chooser, member: &str, body: &[u8], big: bool) {
    let mut options = read(body, big);
    let lines = patterns(&mut options);
    let mode = match member {
        "SaveFiles" => "saveFiles",
        "SaveFile" => "save",
        _ if options.directory => "folder",
        _ => "open",
    };
    let name = if mode == "saveFiles" {
        // whole names only: as many as fit
        let mut names = String::new();
        for file in &options.files {
            if names.len() + file.len() + 2 > NAME {
                options.ignored = true;
                break;
            }
            if !names.is_empty() {
                names.push('\n');
            }
            names.push_str(file);
        }
        names
    } else {
        options.name.clone()
    };
    let mut choices = String::new();
    for (id, value) in &options.choices {
        let line = format!("{id}\t{value}\n");
        if choices.len() + line.len() >= CHOICES {
            options.ignored = true;
            break;
        }
        choices.push_str(&line);
    }
    put(&mut out.token, &options.token);
    put(&mut out.mode, mode);
    put(&mut out.title, &options.title);
    put(&mut out.folder, &options.folder);
    put(&mut out.name, &name);
    put(&mut out.patterns, &lines);
    put(&mut out.choices, &choices);
    out.multiple = c_int::from(options.multiple);
    out.filters = options.filters.len().min(u32::MAX as usize) as u32;
    out.ignored = c_int::from(options.ignored);
}

/// # Safety
/// `member` is a NUL-terminated string, `body` has `len` readable bytes (or is null with `len` 0)
/// and `out` points to an `AxiaChooser` that may be written.
#[no_mangle]
pub unsafe extern "C" fn axia_portal_chooser(member: *const c_char, body: *const u8, len: usize, big_endian: c_int, out: *mut Chooser) {
    if member.is_null() || out.is_null() {
        return;
    }
    let member = CStr::from_ptr(member).to_str().unwrap_or("OpenFile");
    let body = if body.is_null() { &[][..] } else { std::slice::from_raw_parts(body, len) };
    fill(&mut *out, member, body, big_endian != 0);
}

#[cfg(test)]
#[path = "portal_chooser_tests.rs"]
mod tests;
