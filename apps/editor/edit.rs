//! Editing commands and the keyboard map of the text area, over `buffer::Doc`. Nothing here
//! draws: widths come from `Metrics`, and what the host must do comes back in `Reply`.
use super::buffer::{pos, Cursor, Doc, Edit, Kind, Pos};
use super::syntax::Language;

/// Widths of the monospaced text area: an ASCII character is one cell; the rest is measured.
#[derive(Clone, Copy)]
pub struct Metrics {
    pub cell: f64,
    pub measure: fn(&str) -> f64,
}
impl Metrics {
    pub fn width(&self, text: &str) -> f64 {
        if text.is_ascii() {
            text.len() as f64 * self.cell
        } else {
            (self.measure)(text)
        }
    }
    fn advance(&self, c: char) -> f64 {
        if c.is_ascii() {
            self.cell
        } else {
            (self.measure)(c.encode_utf8(&mut [0; 4]))
        }
    }
}

/// How the document is indented and commented, and how big a page is.
#[derive(Clone, Copy)]
pub struct Style<'a> {
    pub language: &'a Language,
    pub tab: usize,
    pub use_tabs: bool,
    /// Lines in a page (PageUp, PageDown).
    pub page: usize,
    pub metrics: Metrics,
}
impl Style<'_> {
    fn unit(&self) -> String {
        if self.use_tabs {
            "\t".into()
        } else {
            " ".repeat(self.tab)
        }
    }
}

/// Tabs become spaces up to the next tab stop.
pub fn expand(text: &[char], tab: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for &c in text {
        if c == '\t' {
            let n = tab - column % tab;
            out.extend(std::iter::repeat_n(' ', n));
            column += n;
        } else {
            out.push(c);
            column += 1;
        }
    }
    out
}
/// The horizontal position of `p` inside its line.
pub fn x_of(doc: &Doc, p: Pos, style: &Style) -> f64 {
    let line = doc.line(p.l);
    style.metrics.width(&expand(&line[..p.c.min(line.len())], style.tab))
}
/// The column closest to `x`.
pub fn column_at(line: &[char], x: f64, style: &Style) -> usize {
    if x <= 0. {
        return 0;
    }
    let (mut width, mut column) = (0., 0);
    for (i, &c) in line.iter().enumerate() {
        let advance = if c == '\t' {
            let n = style.tab - column % style.tab;
            column += n;
            n as f64 * style.metrics.cell
        } else {
            column += 1;
            style.metrics.advance(c)
        };
        if x < width + advance / 2. {
            return i;
        }
        width += advance;
    }
    line.len()
}

pub fn is_word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
}
fn is_space(c: Option<char>) -> bool {
    c.is_some_and(char::is_whitespace)
}
fn at(line: &[char], i: usize) -> Option<char> {
    line.get(i).copied()
}
fn before(line: &[char], i: usize) -> Option<char> {
    i.checked_sub(1).and_then(|i| at(line, i))
}
fn indentation(line: &[char]) -> usize {
    line.iter().take_while(|c| matches!(c, ' ' | '\t')).count()
}
fn closing(open: char) -> Option<char> {
    Some(match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' | '\'' | '`' => open,
        _ => return None,
    })
}
fn is_closer(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | '"' | '\'' | '`')
}

pub fn left(doc: &Doc, p: Pos) -> Pos {
    if p.c > 0 {
        pos(p.l, p.c - 1)
    } else if p.l > 0 {
        pos(p.l - 1, doc.len(p.l - 1))
    } else {
        p
    }
}
pub fn right(doc: &Doc, p: Pos) -> Pos {
    if p.c < doc.len(p.l) {
        pos(p.l, p.c + 1)
    } else if p.l + 1 < doc.line_count() {
        pos(p.l + 1, 0)
    } else {
        p
    }
}
pub fn word_left(doc: &Doc, p: Pos) -> Pos {
    if p.c == 0 {
        return left(doc, p);
    }
    let line = doc.line(p.l);
    let mut c = p.c;
    while c > 0 && is_space(before(line, c)) {
        c -= 1;
    }
    let word = is_word(before(line, c));
    while c > 0 && !is_space(before(line, c)) && is_word(before(line, c)) == word {
        c -= 1;
    }
    pos(p.l, c)
}
pub fn word_right(doc: &Doc, p: Pos) -> Pos {
    let line = doc.line(p.l);
    if p.c >= line.len() {
        return right(doc, p);
    }
    let mut c = p.c;
    while c < line.len() && is_space(at(line, c)) {
        c += 1;
    }
    let word = is_word(at(line, c));
    while c < line.len() && !is_space(at(line, c)) && is_word(at(line, c)) == word {
        c += 1;
    }
    pos(p.l, c)
}
/// The word at `p` (or just before it); a single character when there is no word.
pub fn word_at(doc: &Doc, p: Pos) -> (Pos, Pos) {
    let line = doc.line(p.l);
    let (mut a, mut b) = (p.c, p.c);
    if !is_word(at(line, a)) && is_word(before(line, a)) {
        a -= 1;
        b -= 1;
    }
    if !is_word(at(line, a)) {
        return (p, pos(p.l, line.len().min(p.c + 1)));
    }
    while is_word(before(line, a)) {
        a -= 1;
    }
    while is_word(at(line, b)) {
        b += 1;
    }
    (pos(p.l, a), pos(p.l, b))
}
fn line_home(doc: &Doc, p: Pos) -> Pos {
    let n = indentation(doc.line(p.l));
    pos(p.l, if p.c == n { 0 } else { n })
}
/// The start of the line after `l`, or the end of `l` when it is the last.
fn after_line(doc: &Doc, l: usize) -> Pos {
    if l + 1 < doc.line_count() {
        pos(l + 1, 0)
    } else {
        pos(l, doc.len(l))
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Motion {
    Left,
    Right,
    Up(usize),
    Down(usize),
    WordLeft,
    WordRight,
    LineHome,
    LineEnd,
    Top,
    Bottom,
}

/// Moves every cursor; with `extend` the selections grow.
pub fn move_cursors(doc: &mut Doc, motion: Motion, extend: bool, style: &Style) {
    let vertical = matches!(motion, Motion::Up(_) | Motion::Down(_));
    let moved = doc
        .cursors
        .iter()
        .map(|k| {
            // an arrow without Shift only closes a selection, on the side it points to
            if !extend && k.has_selection() && matches!(motion, Motion::Left | Motion::Right) {
                return Cursor::at(if motion == Motion::Left { k.start() } else { k.end() });
            }
            let goal = vertical.then(|| k.goal.unwrap_or_else(|| x_of(doc, k.head(), style)));
            let p = k.head();
            let target = match motion {
                Motion::Left => left(doc, p),
                Motion::Right => right(doc, p),
                Motion::WordLeft => word_left(doc, p),
                Motion::WordRight => word_right(doc, p),
                Motion::LineHome => line_home(doc, p),
                Motion::LineEnd => pos(p.l, doc.len(p.l)),
                Motion::Top => pos(0, 0),
                Motion::Bottom => doc.last_pos(),
                Motion::Up(n) if p.l < n => pos(0, 0),
                Motion::Down(n) if p.l + n >= doc.line_count() => doc.last_pos(),
                Motion::Up(n) | Motion::Down(n) => {
                    let l = if matches!(motion, Motion::Up(_)) { p.l - n } else { p.l + n };
                    pos(l, column_at(doc.line(l), goal.unwrap_or(0.), style))
                }
            };
            let p = doc.clamp(target);
            let mut next = if extend { Cursor::range(k.anchor(), p) } else { Cursor::at(p) };
            next.goal = goal;
            next
        })
        .collect();
    doc.cursors = moved;
    doc.normalize();
}

/// Typing one character or a committed composition.
pub fn type_text(doc: &mut Doc, text: &str, style: &Style, now: i64) {
    let mut chars = text.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    // typing the closer that is already there only steps over it
    if let Some(c) = single.filter(|c| is_closer(*c)) {
        if doc.cursors.iter().all(|k| !k.has_selection() && at(doc.line(k.l), k.c) == Some(c)) {
            move_cursors(doc, Motion::Right, false, style);
            return;
        }
    }
    let close = single.and_then(closing);
    if let Some(close) = close {
        if doc.cursors.iter().all(Cursor::has_selection) {
            // wrap the selections
            doc.normalize();
            let edits = doc
                .cursors
                .iter()
                .map(|k| Edit::new(k.start(), k.end(), format!("{text}{}{close}", doc.text_in(k.start(), k.end()))))
                .collect();
            doc.transact(edits, Kind::Edit, now, |_, rs, _| {
                rs.iter()
                    .map(|r| Cursor::range(pos(r.start.l, r.start.c + 1), pos(r.end.l, r.end.c - 1)))
                    .collect()
            });
            return;
        }
    }
    let quote = matches!(single, Some('"' | '\'' | '`'));
    let pair = close.filter(|_| {
        doc.cursors.iter().all(|k| {
            let line = doc.line(k.l);
            let (next, previous) = (at(line, k.c), before(line, k.c));
            if next.is_some_and(|c| !c.is_whitespace() && !matches!(c, ')' | ']' | '}' | '.' | ',' | ';' | ':')) {
                return false;
            }
            !(quote && (is_word(previous) || previous == single))
        })
    });
    match pair {
        Some(close) => {
            doc.normalize();
            let both = format!("{text}{close}");
            let edits = doc.cursors.iter().map(|k| Edit::new(k.start(), k.end(), both.clone())).collect();
            doc.transact(edits, Kind::Type, now, |_, rs, _| rs.iter().map(|r| Cursor::at(pos(r.end.l, r.end.c - 1))).collect());
        }
        None => {
            doc.insert(text, Kind::Type, now);
        }
    }
}

pub fn newline(doc: &mut Doc, style: &Style, now: i64) {
    doc.normalize();
    let unit = style.unit();
    let plain = style.language.id == "plain";
    // for each cursor: the edit, and whether the cursor goes back to the line before the closer
    let mut back = Vec::with_capacity(doc.cursors.len());
    let edits = doc
        .cursors
        .iter()
        .map(|k| {
            let (from, to) = (k.start(), k.end());
            let line = doc.line(k.l);
            let mut indent: String = line[..indentation(line).min(from.c)].iter().collect();
            let source = doc.line(from.l);
            let head: String = source[..from.c].iter().collect();
            let opens = !plain && head.trim_end().ends_with(['{', '[', '(', ':']);
            if opens && matches!(at(doc.line(to.l), to.c), Some('}' | ']' | ')')) {
                back.push(true);
                return Edit::new(from, to, format!("\n{indent}{unit}\n{indent}"));
            }
            if opens {
                indent.push_str(&unit);
            }
            back.push(false);
            Edit::new(from, to, format!("\n{indent}"))
        })
        .collect();
    doc.transact(edits, Kind::Edit, now, |doc, rs, _| {
        rs.iter()
            .zip(&back)
            .map(|(r, back)| {
                Cursor::at(if *back { pos(r.end.l - 1, doc.len(r.end.l - 1)) } else { r.end })
            })
            .collect()
    });
}

pub fn backspace(doc: &mut Doc, by_word: bool, style: &Style, now: i64) {
    if doc.cursors.iter().all(|k| !k.has_selection()) {
        // inside an empty pair: delete both
        let in_pair = doc.cursors.iter().all(|k| {
            let line = doc.line(k.l);
            before(line, k.c).and_then(closing).is_some_and(|close| at(line, k.c) == Some(close))
        });
        if in_pair {
            doc.delete_by(Kind::Delete, now, |_, p| pos(p.l, p.c - 1));
            doc.delete_by(Kind::Delete, now, |_, p| pos(p.l, p.c + 1));
            return;
        }
        // in space indentation: back to the previous tab stop
        if !by_word && !style.use_tabs && doc.cursors.iter().all(|k| k.c > 0 && doc.line(k.l)[..k.c].iter().all(|&c| c == ' ')) {
            let tab = style.tab;
            doc.delete_by(Kind::Delete, now, move |_, p| pos(p.l, p.c.saturating_sub((p.c - 1) % tab + 1)));
            return;
        }
    }
    if by_word {
        doc.delete_by(Kind::Delete, now, word_left);
    } else {
        doc.delete_by(Kind::Delete, now, left);
    }
}
pub fn delete_forward(doc: &mut Doc, by_word: bool, now: i64) {
    if by_word {
        doc.delete_by(Kind::Delete, now, word_right);
    } else {
        doc.delete_by(Kind::Delete, now, right);
    }
}

pub fn indent_lines(doc: &mut Doc, out: bool, style: &Style, now: i64) {
    let unit = style.unit();
    let tab = style.tab;
    doc.map_lines(now, |s, _| {
        if !out {
            return if s.is_empty() { s.to_owned() } else { format!("{unit}{s}") };
        }
        if let Some(rest) = s.strip_prefix('\t') {
            return rest.to_owned();
        }
        let spaces = s.len() - s.trim_start_matches(' ').len();
        s[spaces.min(tab)..].to_owned()
    });
}

fn leading(s: &str) -> &str {
    &s[..s.len() - s.trim_start_matches([' ', '\t']).len()]
}
pub fn toggle_comment(doc: &mut Doc, style: &Style, now: i64) {
    let mark = style.language.comment;
    if mark.is_empty() {
        let Some((open, close)) = style.language.block else { return };
        let commented = doc.cursor_lines().iter().all(|&l| {
            let s = doc.line_string(l);
            let s = s.trim();
            s.is_empty() || (s.starts_with(open) && s.ends_with(close))
        });
        doc.map_lines(now, |s, _| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return s.to_owned();
            }
            let indent = leading(s);
            if commented {
                let inner = trimmed.get(open.len()..trimmed.len().saturating_sub(close.len())).unwrap_or("");
                format!("{indent}{}", inner.trim())
            } else {
                format!("{indent}{open} {} {close}", &s[indent.len()..])
            }
        });
        return;
    }
    let filled: Vec<String> = doc.cursor_lines().iter().map(|&l| doc.line_string(l)).filter(|s| !s.trim().is_empty()).collect();
    let commented = !filled.is_empty() && filled.iter().all(|s| s.trim_start().starts_with(mark));
    // indentation is ASCII (spaces and tabs), so bytes are columns
    let column = filled.iter().map(|s| leading(s).len()).min().unwrap_or(0);
    doc.map_lines(now, |s, _| {
        if s.trim().is_empty() {
            return s.to_owned();
        }
        if commented {
            let Some(i) = s.find(mark) else { return s.to_owned() };
            let rest = &s[i + mark.len()..];
            format!("{}{}", &s[..i], rest.strip_prefix(' ').unwrap_or(rest))
        } else {
            format!("{}{mark} {}", &s[..column], &s[column..])
        }
    });
}

fn shifted(k: &Cursor, by: isize) -> Cursor {
    let add = |l: usize| (l as isize + by).max(0) as usize;
    Cursor { l: add(k.l), al: add(k.al), ..*k }
}
pub fn duplicate_lines(doc: &mut Doc, now: i64) {
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for l in doc.cursor_lines() {
        match groups.last_mut() {
            Some(g) if g.1 + 1 == l => g.1 = l,
            _ => groups.push((l, l)),
        }
    }
    let edits = groups
        .iter()
        .map(|&(a, b)| {
            let end = pos(b, doc.len(b));
            Edit::new(end, end, format!("\n{}", doc.text_in(pos(a, 0), end)))
        })
        .collect();
    let before = doc.cursors.clone();
    doc.transact(edits, Kind::Edit, now, |_, _, _| {
        before
            .iter()
            .map(|k| {
                let shift: usize = match groups.iter().position(|&(a, b)| k.l >= a && k.l <= b) {
                    Some(g) => groups[..=g].iter().map(|&(a, b)| b - a + 1).sum(),
                    None => 0,
                };
                shifted(k, shift as isize)
            })
            .collect()
    });
}

pub fn move_lines(doc: &mut Doc, up: bool, now: i64) {
    let lines = doc.cursor_lines();
    let (Some(&a), Some(&b)) = (lines.first(), lines.last()) else { return };
    if (up && a == 0) || (!up && b + 1 >= doc.line_count()) {
        return;
    }
    let block = doc.text_in(pos(a, 0), pos(b, doc.len(b)));
    let before = doc.cursors.clone();
    let (edit, by) = if up {
        let previous = doc.line_string(a - 1);
        (Edit::new(pos(a - 1, 0), pos(b, doc.len(b)), format!("{block}\n{previous}")), -1)
    } else {
        let next = doc.line_string(b + 1);
        (Edit::new(pos(a, 0), pos(b + 1, doc.len(b + 1)), format!("{next}\n{block}")), 1)
    };
    doc.transact(vec![edit], Kind::Edit, now, |_, _, _| before.iter().map(|k| shifted(k, by)).collect());
}

/// Selects the word under the cursor, or adds the next occurrence of the selection. Returns the
/// start of what was added, which the view must show.
pub fn select_next(doc: &mut Doc) -> Option<Pos> {
    let last = doc.primary();
    if !last.has_selection() {
        let (from, to) = word_at(doc, last.head());
        *doc.cursors.last_mut()? = Cursor::range(from, to);
        return Some(to);
    }
    let needle: Vec<char> = doc.text_in(last.start(), last.end()).chars().collect();
    if needle.is_empty() || needle.contains(&'\n') {
        return None;
    }
    let from = last.end();
    for i in 0..=doc.line_count() {
        let l = (from.l + i) % doc.line_count();
        let start = if i == 0 { from.c } else { 0 };
        let line = doc.line(l);
        let Some(c) = line.get(start..).and_then(|tail| tail.windows(needle.len()).position(|w| w == needle)).map(|c| c + start)
        else {
            continue;
        };
        if doc.cursors.iter().any(|k| k.start() == pos(l, c)) {
            return None;
        }
        doc.cursors.push(Cursor::range(pos(l, c), pos(l, c + needle.len())));
        doc.normalize();
        return Some(pos(l, c));
    }
    None
}

/// What "copy" puts on the clipboard: the selections, or the whole line of a lone cursor.
pub fn copy_text(doc: &Doc) -> String {
    let selected: Vec<String> =
        doc.cursors.iter().filter(|k| k.has_selection()).map(|k| doc.text_in(k.start(), k.end())).collect();
    if !selected.is_empty() {
        return selected.join("\n");
    }
    match doc.cursors.as_slice() {
        [only] => format!("{}\n", doc.line_string(only.l)),
        _ => String::new(),
    }
}
/// Cuts and returns what goes to the clipboard. Without a selection, the whole line.
pub fn cut(doc: &mut Doc, now: i64) -> String {
    if !doc.any_selection() {
        let l = doc.cursors[0].l;
        let text = format!("{}\n", doc.line_string(l));
        doc.cursors = vec![Cursor::range(pos(l, 0), after_line(doc, l))];
        doc.delete_by(Kind::Edit, now, |_, p| p);
        return text;
    }
    let text = copy_text(doc);
    doc.delete_by(Kind::Edit, now, |_, p| p);
    text
}
/// With as many cursors as pasted lines, each cursor takes one line.
pub fn paste(doc: &mut Doc, text: &str, now: i64) {
    let text = text.replace("\r\n", "\n");
    if text.is_empty() {
        return;
    }
    let parts: Vec<&str> = text.strip_suffix('\n').unwrap_or(&text).split('\n').collect();
    doc.normalize();
    if doc.cursors.len() > 1 && parts.len() == doc.cursors.len() {
        let mut next = parts.iter();
        doc.insert_with(Kind::Edit, now, |_, _| next.next().copied().unwrap_or("").to_owned());
    } else {
        doc.insert(&text, Kind::Edit, now);
    }
}
pub fn select_all(doc: &mut Doc) {
    doc.cursors = vec![Cursor::range(pos(0, 0), doc.last_pos())];
}
pub fn select_line(doc: &mut Doc) {
    doc.cursors = doc
        .cursors
        .iter()
        .map(|k| Cursor::range(pos(k.start().l, 0), after_line(doc, k.end().l)))
        .collect();
    doc.normalize();
}
pub fn single_cursor(doc: &mut Doc) {
    doc.cursors = vec![doc.primary()];
}
/// Moves the only cursor to `from`, or selects from `from` to `to`.
pub fn go_to(doc: &mut Doc, from: Pos, to: Option<Pos>) {
    let from = doc.clamp(from);
    doc.cursors = vec![match to {
        Some(to) => Cursor::range(from, doc.clamp(to)),
        None => Cursor::at(from),
    }];
}
/// Tab: indent the selected lines, or fill up to the next tab stop.
pub fn tab_key(doc: &mut Doc, shift: bool, style: &Style, now: i64) {
    if shift {
        indent_lines(doc, true, style, now);
    } else if doc.cursors.iter().any(|k| k.has_selection() && k.l != k.al) {
        indent_lines(doc, false, style, now);
    } else if style.use_tabs {
        type_text(doc, "\t", style, now);
    } else {
        let tab = style.tab;
        doc.insert_with(Kind::Type, now, |doc, k| " ".repeat(tab - expand(&doc.line(k.l)[..k.c], tab).chars().count() % tab));
    }
}

/// A command of the menus and shortcuts that works on the text. `None`: not an editing one.
pub fn command(doc: &mut Doc, name: &str, style: &Style, now: i64) -> Option<Reply> {
    let mut reply = Reply::changed(true);
    match name {
        "undo" => {
            if !doc.undo() {
                reply = Reply::default();
            }
        }
        "redo" => {
            if !doc.redo() {
                reply = Reply::default();
            }
        }
        "copy" => {
            let text = copy_text(doc);
            reply = Reply { copy: (!text.is_empty()).then_some(text), ..Reply::default() };
        }
        "cut" => reply.copy = Some(cut(doc, now)),
        "paste" => reply = Reply { paste: true, ..Reply::default() },
        "selectAll" => select_all(doc),
        "selectNext" => reply.reveal = select_next(doc),
        "toggleComment" => toggle_comment(doc, style, now),
        "duplicateLines" => duplicate_lines(doc, now),
        "indent" => indent_lines(doc, false, style, now),
        "outdent" => indent_lines(doc, true, style, now),
        "moveUp" => move_lines(doc, true, now),
        "moveDown" => move_lines(doc, false, now),
        "selectLine" => select_line(doc),
        "singleCursor" => single_cursor(doc),
        _ => return None,
    }
    if reply.seal {
        doc.seal();
    }
    Some(reply)
}

/// What the host does after a key or a command.
#[derive(Debug, Default, PartialEq)]
pub struct Reply {
    /// The text or the cursors changed: scroll to the cursor and draw again.
    pub changed: bool,
    /// The change closes the undo step.
    pub seal: bool,
    pub copy: Option<String>,
    pub paste: bool,
    /// A position to bring into view instead of the primary cursor.
    pub reveal: Option<Pos>,
    /// A window command: the shortcut letter (or "escape") and whether Shift was down.
    pub window: Option<(String, bool)>,
}
impl Reply {
    pub fn changed(seal: bool) -> Self {
        Reply { changed: true, seal, ..Reply::default() }
    }
}

pub struct Key<'a> {
    pub name: &'a str,
    /// What the key types ("" for none).
    pub text: &'a str,
    /// The key without modifiers, for shortcuts.
    pub base: &'a str,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// The keyboard of the text area. `None`: the key is not the editor's.
pub fn key(doc: &mut Doc, k: &Key, style: &Style, now: i64) -> Option<Reply> {
    let primary = k.ctrl || k.meta;
    let shift = k.shift;
    let motion = |doc: &mut Doc, m: Motion| {
        move_cursors(doc, m, shift, style);
        doc.seal();
        Some(Reply::changed(true))
    };
    let run = |doc: &mut Doc, name: &str| command(doc, name, style, now);
    if primary && !k.alt {
        let base = k.base.to_lowercase();
        let editing = match (base.as_str(), shift) {
            ("z", false) => Some("undo"),
            ("z", true) | ("y", _) => Some("redo"),
            ("c", _) => Some("copy"),
            ("x", _) => Some("cut"),
            ("v", _) => Some("paste"),
            ("a", _) => Some("selectAll"),
            ("d", false) => Some("selectNext"),
            ("d", true) => Some("duplicateLines"),
            ("/", _) => Some("toggleComment"),
            ("]", _) => Some("indent"),
            ("[", _) => Some("outdent"),
            ("l", true) => Some("selectLine"),
            _ => None,
        };
        if let Some(name) = editing {
            return run(doc, name);
        }
        return match k.name {
            "ArrowLeft" if k.meta => motion(doc, Motion::LineHome),
            "ArrowRight" if k.meta => motion(doc, Motion::LineEnd),
            "ArrowUp" if k.meta => motion(doc, Motion::Top),
            "ArrowDown" if k.meta => motion(doc, Motion::Bottom),
            "Home" => motion(doc, Motion::Top),
            "End" => motion(doc, Motion::Bottom),
            "ArrowLeft" => motion(doc, Motion::WordLeft),
            "ArrowRight" => motion(doc, Motion::WordRight),
            "Backspace" => {
                backspace(doc, true, style, now);
                Some(Reply::changed(false))
            }
            "Delete" => {
                delete_forward(doc, true, now);
                Some(Reply::changed(false))
            }
            _ if !base.is_empty() => Some(Reply { window: Some((base, shift)), ..Reply::default() }),
            _ => None,
        };
    }
    if k.alt && !primary {
        match k.name {
            "ArrowUp" => return run(doc, "moveUp"),
            "ArrowDown" => return run(doc, "moveDown"),
            "ArrowLeft" => return motion(doc, Motion::WordLeft),
            "ArrowRight" => return motion(doc, Motion::WordRight),
            "Backspace" => {
                backspace(doc, true, style, now);
                return Some(Reply::changed(false));
            }
            _ => {}
        }
    }
    let page = style.page.max(1);
    match k.name {
        "ArrowLeft" => motion(doc, Motion::Left),
        "ArrowRight" => motion(doc, Motion::Right),
        "ArrowUp" => motion(doc, Motion::Up(1)),
        "ArrowDown" => motion(doc, Motion::Down(1)),
        "PageUp" => motion(doc, Motion::Up(page)),
        "PageDown" => motion(doc, Motion::Down(page)),
        "Home" => motion(doc, Motion::LineHome),
        "End" => motion(doc, Motion::LineEnd),
        "Backspace" => {
            backspace(doc, false, style, now);
            Some(Reply::changed(false))
        }
        "Delete" => {
            delete_forward(doc, false, now);
            Some(Reply::changed(false))
        }
        "Enter" => {
            newline(doc, style, now);
            doc.seal();
            Some(Reply::changed(true))
        }
        "Tab" => {
            tab_key(doc, shift, style, now);
            Some(Reply::changed(false))
        }
        "Escape" => {
            if doc.cursors.len() > 1 {
                run(doc, "singleCursor")
            } else if doc.any_selection() {
                doc.cursors = vec![Cursor::at(doc.cursors[0].head())];
                doc.seal();
                Some(Reply::changed(true))
            } else {
                Some(Reply { window: Some(("escape".into(), false)), ..Reply::default() })
            }
        }
        _ if !k.text.is_empty() && !k.ctrl && !k.meta && k.text.chars().all(|c| c >= ' ') => {
            type_text(doc, k.text, style, now);
            let seal = k.text == " " || k.text == "\n";
            if seal {
                doc.seal();
            }
            Some(Reply::changed(seal))
        }
        _ => None,
    }
}
