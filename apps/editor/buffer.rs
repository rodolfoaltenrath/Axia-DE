//! The editor document: lines, multiple cursors, edits and undo.
//!
//! A position is a line and a column in characters. A cursor is the head (`l`, `c`), the
//! selection anchor (`al`, `ac`) and the desired horizontal pixel position for moving up and
//! down (`goal`). Without a selection the anchor is the head.
//!
//! Every change goes through `raw_apply`: sorted, non-overlapping replacements applied back to
//! front, which report where each one ended up. Each transaction stores its inverse, so undo and
//! redo are just other transactions. Consecutive keystrokes become one step.

const GROUP_MS: i64 = 900;
const UNDO_MAX: usize = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub l: usize,
    pub c: usize,
}
pub fn pos(l: usize, c: usize) -> Pos {
    Pos { l, c }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cursor {
    pub l: usize,
    pub c: usize,
    pub al: usize,
    pub ac: usize,
    pub goal: Option<f64>,
}
impl Cursor {
    pub fn at(p: Pos) -> Self {
        Self { l: p.l, c: p.c, al: p.l, ac: p.c, goal: None }
    }
    /// A selection from `anchor` to `head`.
    pub fn range(anchor: Pos, head: Pos) -> Self {
        Self { l: head.l, c: head.c, al: anchor.l, ac: anchor.c, goal: None }
    }
    pub fn head(&self) -> Pos {
        pos(self.l, self.c)
    }
    pub fn anchor(&self) -> Pos {
        pos(self.al, self.ac)
    }
    pub fn has_selection(&self) -> bool {
        self.head() != self.anchor()
    }
    pub fn start(&self) -> Pos {
        self.head().min(self.anchor())
    }
    pub fn end(&self) -> Pos {
        self.head().max(self.anchor())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub from: Pos,
    pub to: Pos,
    pub text: String,
}
impl Edit {
    pub fn new(from: Pos, to: Pos, text: impl Into<String>) -> Self {
        Self { from, to, text: text.into() }
    }
}
/// Where the text of an edit ended up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Applied {
    pub start: Pos,
    pub end: Pos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Type,
    Delete,
    /// Never joins the step before it, and closes the one it opens.
    Edit,
}

struct Transaction {
    edits: Vec<Edit>,
    before: Vec<Cursor>,
    after: Vec<Cursor>,
    serial_before: u64,
    serial_after: u64,
}
struct Group {
    kind: Kind,
    time: i64,
    steps: Vec<Transaction>,
}

pub struct Doc {
    lines: Vec<Vec<char>>,
    /// "\n" or "\r\n".
    pub eol: &'static str,
    pub cursors: Vec<Cursor>,
    undo: Vec<Group>,
    redo: Vec<Group>,
    /// Changes with every edit.
    pub version: u64,
    /// State number: undo and redo go back to earlier states.
    serial: u64,
    saved_serial: u64,
    next_serial: u64,
    /// The first line touched since the highlighter last looked (`usize::MAX`: none).
    pub first_changed: usize,
    /// The next change starts a new undo step.
    sealed: bool,
}

impl Doc {
    pub fn new(text: &str) -> Self {
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let lines = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).chars().collect())
            .collect::<Vec<Vec<char>>>();
        Self {
            lines,
            eol,
            cursors: vec![Cursor::default()],
            undo: Vec::new(),
            redo: Vec::new(),
            version: 0,
            serial: 0,
            saved_serial: 0,
            next_serial: 1,
            first_changed: 0,
            sealed: true,
        }
    }
    pub fn dirty(&self) -> bool {
        self.serial != self.saved_serial
    }
    pub fn mark_saved(&mut self) {
        self.saved_serial = self.serial;
    }
    /// A change without history (the line ending).
    pub fn touch(&mut self) {
        self.serial = self.next_serial;
        self.next_serial += 1;
        self.version += 1;
    }
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push_str(self.eol);
            }
            out.extend(line.iter());
        }
        out
    }
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }
    pub fn line(&self, l: usize) -> &[char] {
        self.lines.get(l).map_or(&[], Vec::as_slice)
    }
    pub fn line_string(&self, l: usize) -> String {
        self.line(l).iter().collect()
    }
    pub fn len(&self, l: usize) -> usize {
        self.line(l).len()
    }
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
    pub fn last_pos(&self) -> Pos {
        let l = self.lines.len() - 1;
        pos(l, self.len(l))
    }
    pub fn clamp(&self, p: Pos) -> Pos {
        let l = p.l.min(self.lines.len() - 1);
        pos(l, p.c.min(self.lines[l].len()))
    }
    pub fn text_in(&self, a: Pos, b: Pos) -> String {
        if a.l == b.l {
            return self.lines[a.l][a.c..b.c].iter().collect();
        }
        let mut out: String = self.lines[a.l][a.c..].iter().collect();
        for line in &self.lines[a.l + 1..b.l] {
            out.push('\n');
            out.extend(line.iter());
        }
        out.push('\n');
        out.extend(self.lines[b.l][..b.c].iter());
        out
    }
    /// The primary cursor: the last one.
    pub fn primary(&self) -> Cursor {
        self.cursors.last().copied().unwrap_or_default()
    }
    pub fn any_selection(&self) -> bool {
        self.cursors.iter().any(Cursor::has_selection)
    }

    /// Sorts the cursors and merges the ones that touch.
    pub fn normalize(&mut self) {
        let mut sorted = self.cursors.clone();
        sorted.sort_by_key(Cursor::start);
        let mut out: Vec<Cursor> = Vec::with_capacity(sorted.len());
        for k in sorted {
            match out.last_mut() {
                Some(prev) if k.start() <= prev.end() && (k.has_selection() || prev.has_selection() || k.head() == prev.head()) => {
                    let (s, e) = (prev.start().min(k.start()), prev.end().max(k.end()));
                    let forward = prev.head() >= prev.anchor();
                    *prev = if forward { Cursor::range(s, e) } else { Cursor::range(e, s) };
                }
                _ => out.push(k),
            }
        }
        self.cursors = out;
    }

    /// Applies sorted, non-overlapping edits. Returns where each one is now and the inverse.
    fn raw_apply(&mut self, edits: &[Edit]) -> (Vec<Applied>, Vec<Edit>) {
        let removed: Vec<String> = edits.iter().map(|e| self.text_in(e.from, e.to)).collect();
        for e in edits.iter().rev() {
            let mut parts: Vec<Vec<char>> = e.text.split('\n').map(|p| p.chars().collect()).collect();
            let mut first = self.lines[e.from.l][..e.from.c].to_vec();
            first.append(&mut parts[0]);
            parts[0] = first;
            let after = self.lines[e.to.l][e.to.c..].to_vec();
            parts.last_mut().expect("split gives a part").extend(after);
            self.lines.splice(e.from.l..=e.to.l, parts);
        }
        let mut results = Vec::with_capacity(edits.len());
        let (mut dl, mut dc, mut last_line) = (0isize, 0isize, usize::MAX);
        let add = |n: usize, d: isize| (n as isize + d).max(0) as usize;
        for e in edits {
            let sl = add(e.from.l, dl);
            let sc = add(e.from.c, if e.from.l == last_line { dc } else { 0 });
            let count = e.text.split('\n').count();
            let tail = e.text.rsplit('\n').next().unwrap_or("").chars().count();
            let el = sl + count - 1;
            let ec = if count == 1 { sc + tail } else { tail };
            dl += count as isize - 1 - (e.to.l - e.from.l) as isize;
            dc = ec as isize - e.to.c as isize;
            last_line = e.to.l;
            results.push(Applied { start: pos(sl, sc), end: pos(el, ec) });
        }
        let inverse = results.iter().zip(removed).map(|(r, text)| Edit { from: r.start, to: r.end, text }).collect();
        self.version += 1;
        if let Some(first) = edits.first() {
            self.first_changed = self.first_changed.min(first.from.l);
        }
        (results, inverse)
    }

    /// A transaction: applies the edits, keeps the inverse and leaves the cursors `place` says.
    pub fn transact(
        &mut self,
        edits: Vec<Edit>,
        kind: Kind,
        now: i64,
        place: impl FnOnce(&Doc, &[Applied], &[Edit]) -> Vec<Cursor>,
    ) -> bool {
        let edits: Vec<Edit> = edits.into_iter().filter(|e| !e.text.is_empty() || e.from != e.to).collect();
        if edits.is_empty() {
            return false;
        }
        let before = self.cursors.clone();
        let serial_before = self.serial;
        let (results, inverse) = self.raw_apply(&edits);
        self.serial = self.next_serial;
        self.next_serial += 1;
        let cursors = place(self, &results, &edits);
        self.cursors = cursors;
        self.normalize();
        let mut step = Transaction { edits: inverse, before, after: self.cursors.clone(), serial_before, serial_after: self.serial };
        match self.undo.last_mut() {
            Some(last) if !self.sealed && last.kind == kind && kind != Kind::Edit && now - last.time < GROUP_MS => {
                // the group becomes a single state
                step.serial_before = last.steps[0].serial_before;
                last.steps.push(step);
                last.time = now;
            }
            _ => self.undo.push(Group { kind, time: now, steps: vec![step] }),
        }
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.sealed = kind == Kind::Edit;
        true
    }
    pub fn seal(&mut self) {
        self.sealed = true;
    }
    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(group) => {
                let back = self.revert(group);
                self.redo.push(back);
                true
            }
            None => false,
        }
    }
    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(group) => {
                let back = self.revert(group);
                self.undo.push(back);
                true
            }
            None => false,
        }
    }
    fn revert(&mut self, group: Group) -> Group {
        let mut back = Vec::with_capacity(group.steps.len());
        let state = group.steps[0].serial_before;
        for step in group.steps.into_iter().rev() {
            let (_, inverse) = self.raw_apply(&step.edits);
            self.cursors = step.before.clone();
            back.push(Transaction {
                edits: inverse,
                before: step.after,
                after: step.before,
                serial_before: step.serial_after,
                serial_after: step.serial_before,
            });
        }
        // `back` is in the order applied here, so reverting it replays the first change first
        self.serial = state;
        self.sealed = true;
        Group { kind: group.kind, time: 0, steps: back }
    }

    // ---- high-level edits, for every cursor ----

    /// Replaces each selection with `text_for(cursor)`; each cursor ends after its text.
    pub fn insert_with(&mut self, kind: Kind, now: i64, mut text_for: impl FnMut(&Doc, &Cursor) -> String) -> bool {
        self.normalize();
        let edits = self.cursors.iter().map(|k| Edit::new(k.start(), k.end(), text_for(self, k))).collect();
        self.transact(edits, kind, now, |_, rs, _| rs.iter().map(|r| Cursor::at(r.end)).collect())
    }
    pub fn insert(&mut self, text: &str, kind: Kind, now: i64) -> bool {
        self.insert_with(kind, now, |_, _| text.to_owned())
    }
    /// Deletes each selection, or from the cursor to `to(cursor)` when there is none.
    pub fn delete_by(&mut self, kind: Kind, now: i64, to: impl Fn(&Doc, Pos) -> Pos) -> bool {
        self.normalize();
        let mut clean: Vec<Edit> = Vec::new();
        for k in &self.cursors {
            let (from, end) = if k.has_selection() {
                (k.start(), k.end())
            } else {
                let t = self.clamp(to(self, k.head()));
                (t.min(k.head()), t.max(k.head()))
            };
            // two cursors deleting the same text must not overlap
            match clean.last_mut() {
                Some(prev) if from < prev.to => prev.to = prev.to.max(end),
                _ => clean.push(Edit::new(from, end, "")),
            }
        }
        self.transact(clean, kind, now, |_, rs, _| rs.iter().map(|r| Cursor::at(r.start)).collect())
    }
    /// The lines the cursors touch, sorted and without repeats.
    pub fn cursor_lines(&self) -> Vec<usize> {
        let mut out = std::collections::BTreeSet::new();
        for k in &self.cursors {
            let (s, e) = (k.start(), k.end());
            let last = if e.c == 0 && e.l > s.l { e.l - 1 } else { e.l };
            out.extend(s.l..=last);
        }
        out.into_iter().collect()
    }
    /// Rewrites whole lines (indentation, comments), keeping the selections.
    pub fn map_lines(&mut self, now: i64, change: impl Fn(&str, usize) -> String) -> bool {
        let mut edits = Vec::new();
        for l in self.cursor_lines() {
            let old = self.line_string(l);
            let new = change(&old, l);
            if new != old {
                edits.push(Edit::new(pos(l, 0), pos(l, self.len(l)), new));
            }
        }
        if edits.is_empty() {
            return false;
        }
        let before = self.cursors.clone();
        self.transact(edits, Kind::Edit, now, move |_, rs, es| {
            let shift = |p: Pos| match es.iter().position(|e| e.from.l == p.l) {
                None => p,
                Some(i) => {
                    let delta = rs[i].end.c as isize - es[i].to.c as isize;
                    let wanted = if p.c == 0 && delta < 0 { 0 } else { p.c as isize + delta };
                    pos(p.l, wanted.clamp(0, rs[i].end.c as isize) as usize)
                }
            };
            before.iter().map(|k| Cursor::range(shift(k.anchor()), shift(k.head()))).collect()
        })
    }
}
