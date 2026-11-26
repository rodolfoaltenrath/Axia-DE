//! Find and replace in the document, and the fuzzy score of "Go to File".
use super::buffer::{pos, Doc, Edit, Pos};
use regex::{Captures, Regex, RegexBuilder};

pub const MAX_MATCHES: usize = 5000;

#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub line: usize,
    /// Column and length in characters.
    pub column: usize,
    pub len: usize,
    /// Byte offset in the line, to find the capture groups again.
    byte: usize,
}
impl Match {
    pub fn from(&self) -> Pos {
        pos(self.line, self.column)
    }
    pub fn to(&self) -> Pos {
        pos(self.line, self.column + self.len)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub text: String,
    pub regex: bool,
    pub case_sensitive: bool,
}

#[derive(Default)]
pub struct Found {
    pub list: Vec<Match>,
    /// The pattern could not be compiled.
    pub bad_pattern: bool,
    pattern: Option<Regex>,
    regex: bool,
}

/// Matches never cross lines; empty matches do not count.
pub fn find(doc: &Doc, query: &Query) -> Found {
    if query.text.is_empty() {
        return Found::default();
    }
    let source = if query.regex { query.text.clone() } else { regex::escape(&query.text) };
    let Ok(pattern) = RegexBuilder::new(&source).case_insensitive(!query.case_sensitive).size_limit(1 << 20).build() else {
        return Found { bad_pattern: true, ..Found::default() };
    };
    let mut list = Vec::new();
    'lines: for l in 0..doc.line_count() {
        let line = doc.line_string(l);
        // columns are counted forward from the previous match
        let (mut byte, mut column) = (0, 0);
        for m in pattern.find_iter(&line).filter(|m| !m.is_empty()) {
            column += line[byte..m.start()].chars().count();
            byte = m.start();
            list.push(Match { line: l, column, len: m.as_str().chars().count(), byte });
            if list.len() >= MAX_MATCHES {
                break 'lines;
            }
        }
    }
    Found { list, bad_pattern: false, pattern: Some(pattern), regex: query.regex }
}

/// `$&`, `$1`…`$99`, `$<name>` and `$$`, as the previous editor wrote replacements.
fn expand(template: &str, groups: &Captures, line: &str) -> String {
    let whole = groups.get(0).expect("group 0 is the match");
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let digit = |s: &str, i: usize| s.as_bytes().get(i).filter(|b| b.is_ascii_digit()).map(|b| usize::from(b - b'0'));
        let used = match rest.chars().next() {
            Some('$') => {
                out.push('$');
                1
            }
            Some('&') => {
                out.push_str(whole.as_str());
                1
            }
            Some('`') => {
                out.push_str(&line[..whole.start()]);
                1
            }
            Some('\'') => {
                out.push_str(&line[whole.end()..]);
                1
            }
            Some('<') => match rest.find('>') {
                Some(end) => {
                    out.push_str(groups.name(&rest[1..end]).map_or("", |m| m.as_str()));
                    end + 1
                }
                None => {
                    out.push('$');
                    0
                }
            },
            _ => {
                let count = groups.len();
                let one = digit(rest, 0);
                let two = one.zip(digit(rest, 1)).map(|(a, b)| a * 10 + b);
                match (two.filter(|n| *n >= 1 && *n < count), one.filter(|n| *n >= 1 && *n < count)) {
                    (Some(n), _) => {
                        out.push_str(groups.get(n).map_or("", |m| m.as_str()));
                        2
                    }
                    (None, Some(n)) => {
                        out.push_str(groups.get(n).map_or("", |m| m.as_str()));
                        1
                    }
                    _ => {
                        out.push('$');
                        0
                    }
                }
            }
        };
        rest = &rest[used..];
    }
    out.push_str(rest);
    out
}

impl Found {
    /// The edit that replaces one match.
    pub fn edit(&self, doc: &Doc, m: &Match, replacement: &str) -> Edit {
        let text = match &self.pattern {
            Some(pattern) if self.regex => {
                let line = doc.line_string(m.line);
                match pattern.captures_at(&line, m.byte) {
                    Some(groups) => expand(replacement, &groups, &line),
                    None => replacement.to_owned(),
                }
            }
            _ => replacement.to_owned(),
        };
        Edit::new(m.from(), m.to(), text)
    }
    /// The match at or after the primary cursor (the first when there is none).
    pub fn current(&self, doc: &Doc) -> usize {
        let p = doc.primary();
        self.list
            .iter()
            .position(|m| m.line > p.l || (m.line == p.l && m.column + m.len >= p.c))
            .unwrap_or(0)
    }
    pub fn next(&self, doc: &Doc) -> Option<&Match> {
        let after = doc.primary().end();
        self.list.iter().find(|m| m.from() >= after).or_else(|| self.list.first())
    }
    pub fn previous(&self, doc: &Doc) -> Option<&Match> {
        let before = doc.primary().start();
        self.list.iter().rev().find(|m| m.from() < before).or_else(|| self.list.last())
    }
    /// The match that is exactly the primary selection.
    pub fn selected(&self, doc: &Doc) -> Option<&Match> {
        let k = doc.primary();
        self.list.iter().find(|m| k.has_selection() && m.from() == k.start() && m.to() == k.end())
    }
}

/// Letters in order, worth more when consecutive and at the start of a word. `None`: no match.
pub fn fuzzy(query: &str, candidate: &str) -> Option<f64> {
    let candidate: Vec<char> = candidate.chars().map(lower).collect();
    let (mut score, mut next, mut previous) = (0., 0, None::<usize>);
    for c in query.chars().map(lower) {
        let k = next + candidate.get(next..)?.iter().position(|&x| x == c)?;
        score += if previous.is_some_and(|p| p + 1 == k) { 5. } else { 1. };
        if k == 0 || matches!(candidate[k - 1], '/' | '.' | '_' | ' ' | '-') {
            score += 4.;
        }
        previous = Some(k);
        next = k + 1;
    }
    Some(score - candidate.len() as f64 * 0.01)
}
fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}
