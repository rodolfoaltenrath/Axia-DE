//! Unicode multiline editor: wrapped visual rows, history, Markdown commands and emoji completion.
use super::emoji;
use serde_json::{json, Value};
#[derive(Clone, Default)]
struct Snapshot {
    body: String,
    a: usize,
    b: usize,
}
#[derive(Clone, Debug)]
pub struct Row {
    pub start: usize,
    pub end: usize,
    pub line: usize,
}
#[derive(Default)]
pub struct Editor {
    pub body: String,
    pub a: usize,
    pub b: usize,
    pub scroll: f64,
    pub focused: bool,
    pub width: f64,
    pub height: f64,
    pub size: f64,
    pub padding: f64,
    pub x: f64,
    pub y: f64,
    pub rows: Vec<Row>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last: i64,
    kind: String,
    goal: Option<f64>,
    pub suggestion: Option<(usize, Vec<emoji::Emoji>, usize)>,
    pub drag: Option<usize>,
    clicks: (i64, usize),
}
fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}
fn slice(s: &str, a: usize, b: usize) -> String {
    s.chars().skip(a).take(b.saturating_sub(a)).collect()
}
fn regex(s: &str) -> regex::Regex {
    regex::Regex::new(s).unwrap()
}
pub fn advance(c: char, size: f64) -> f64 {
    let mut encoded = [0; 4];
    crate::native_view::measure_text(
        if c == '\t' {
            "    "
        } else {
            c.encode_utf8(&mut encoded)
        },
        size,
        1400.,
    )
    .max(size * 0.1)
}
pub fn width(s: &str, size: f64) -> f64 {
    s.chars().map(|c| advance(c, size)).sum()
}
pub fn wrap(text: &str, width: f64, size: f64) -> Vec<Row> {
    let mut rows = vec![];
    let mut off = 0;
    for (line, s) in text.split('\n').enumerate() {
        let cs = chars(s);
        if cs.is_empty() {
            rows.push(Row {
                start: off,
                end: off,
                line,
            });
        } else {
            let (mut start, mut w, mut space) = (0, 0., None);
            for (i, c) in cs.iter().enumerate() {
                let adv = advance(*c, size);
                if w + adv > width && i > start {
                    let cut = space.filter(|n| *n >= start).map_or(i, |n| n + 1);
                    rows.push(Row {
                        start: off + start,
                        end: off + cut,
                        line,
                    });
                    start = cut;
                    w = cs[cut..i].iter().map(|c| advance(*c, size)).sum();
                    space = None;
                }
                if *c == ' ' {
                    space = Some(i);
                }
                w += adv;
            }
            rows.push(Row {
                start: off + start,
                end: off + cs.len(),
                line,
            });
        }
        off += cs.len() + 1;
    }
    rows
}
impl Editor {
    pub fn reset(&mut self, body: &str) {
        *self = Self {
            body: body.into(),
            a: body.chars().count(),
            b: body.chars().count(),
            size: 14.,
            padding: 14.,
            width: 400.,
            height: 300.,
            ..Self::default()
        };
        self.layout();
    }
    pub fn layout(&mut self) {
        self.rows = wrap(
            &self.body,
            (self.width - self.padding * 2. - 6.).max(40.),
            self.size,
        );
        self.scroll = self.scroll.clamp(0., self.max_scroll());
    }
    pub fn lh(&self) -> f64 {
        (self.size * 1.55).round()
    }
    pub fn max_scroll(&self) -> f64 {
        (self.rows.len() as f64 * self.lh() - self.height + self.padding * 2. + self.lh()).max(0.)
    }
    pub fn range(&self) -> (usize, usize) {
        (self.a.min(self.b), self.a.max(self.b))
    }
    pub fn row_of(&self, off: usize) -> usize {
        for (i, r) in self.rows.iter().enumerate() {
            let next = self.rows.get(i + 1);
            if off >= r.start
                && (off < r.end
                    || off == r.end && next.is_none_or(|n| n.line != r.line || n.start > off))
            {
                return i;
            }
            if off == r.end && next.is_some_and(|n| n.line == r.line && n.start == off) {
                return i + 1;
            }
        }
        self.rows.len().saturating_sub(1)
    }
    pub fn caret_x(&self, off: usize) -> f64 {
        self.rows
            .get(self.row_of(off))
            .map_or(0., |r| width(&slice(&self.body, r.start, off), self.size))
    }
    pub fn at(&self, x: f64, y: f64) -> usize {
        let r = (y / self.lh()).floor().max(0.) as usize;
        let Some(row) = self.rows.get(r.min(self.rows.len().saturating_sub(1))) else {
            return 0;
        };
        let mut w = 0.;
        for (i, c) in self
            .body
            .chars()
            .skip(row.start)
            .take(row.end - row.start)
            .enumerate()
        {
            let a = advance(c, self.size);
            if x < w + a / 2. {
                return row.start + i;
            }
            w += a;
        }
        row.end
    }
    pub fn visible(&mut self) {
        let y = self.row_of(self.b) as f64 * self.lh();
        let h = (self.height - self.padding * 2.).max(self.lh());
        if y < self.scroll {
            self.scroll = y;
        } else if y + self.lh() > self.scroll + h {
            self.scroll = y + self.lh() - h;
        }
        self.scroll = self.scroll.clamp(0., self.max_scroll());
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            body: self.body.clone(),
            a: self.a,
            b: self.b,
        }
    }
    fn commit(&mut self, body: String, a: usize, b: usize, kind: &str, now: i64) {
        if kind != "type" || self.kind != kind || now - self.last > 900 {
            self.undo.push(self.snapshot());
            if self.undo.len() > 300 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last = now;
        self.kind = kind.into();
        self.body = body;
        self.a = a;
        self.b = b;
        self.layout();
        self.visible();
        let before = slice(&self.body, self.b.saturating_sub(26), self.b);
        self.suggestion = regex(r"(^|[\s(])(:[a-z0-9_+\-]{2,24})$")
            .captures(&before)
            .and_then(|c| {
                let list = emoji::search(&c[2][1..])
                    .into_iter()
                    .take(6)
                    .collect::<Vec<_>>();
                (!list.is_empty()).then_some((self.b - c[2].chars().count(), list, 0))
            });
    }
    pub fn replace(
        &mut self,
        from: usize,
        to: usize,
        insert: &str,
        kind: &str,
        caret: Option<usize>,
        now: i64,
    ) {
        let cs = chars(&self.body);
        let a = from.min(cs.len());
        let b = to.max(a).min(cs.len());
        let body = format!(
            "{}{}{}",
            cs[..a].iter().collect::<String>(),
            insert,
            cs[b..].iter().collect::<String>()
        );
        let end = caret.unwrap_or(a + insert.chars().count());
        self.commit(body, end, end, kind, now);
    }
    pub fn insert(&mut self, text: &str, now: i64) {
        let (a, b) = self.range();
        self.replace(a, b, &text.replace("\r\n", "\n"), "edit", None, now);
    }
    fn move_to(&mut self, at: usize, extend: bool) {
        self.b = at.min(self.body.chars().count());
        if !extend {
            self.a = self.b;
        }
        self.suggestion = None;
        self.visible();
    }
    fn line_start(&self, at: usize) -> usize {
        let cs = chars(&self.body);
        cs[..at.min(cs.len())]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |i| i + 1)
    }
    fn line_end(&self, at: usize) -> usize {
        let cs = chars(&self.body);
        cs.iter()
            .enumerate()
            .skip(at)
            .find(|(_, c)| **c == '\n')
            .map_or(cs.len(), |(i, _)| i)
    }
    fn word(&self, at: usize, right: bool) -> usize {
        let cs = chars(&self.body);
        let is = |c: char| c.is_alphanumeric() || c == '_';
        let mut i = at.min(cs.len());
        if right {
            while i < cs.len() && !is(cs[i]) {
                i += 1;
            }
            while i < cs.len() && is(cs[i]) {
                i += 1;
            }
        } else {
            while i > 0 && !is(cs[i - 1]) {
                i -= 1;
            }
            while i > 0 && is(cs[i - 1]) {
                i -= 1;
            }
        }
        i
    }
    fn lines(&mut self, kind: &str, now: i64) {
        let (a, b) = self.range();
        let a = self.line_start(a);
        let b = self.line_end(b);
        let inner = slice(&self.body, a, b);
        let strip = regex(r"^(\s*)(#{1,6}\s+|[-*+]\s+\[[ xX]\]\s+|[-*+]\s+|\d+[.)]\s+)?");
        let out = inner
            .split('\n')
            .enumerate()
            .map(|(i, l)| {
                if kind == "indent" {
                    return format!("  {l}");
                }
                if kind == "outdent" {
                    return regex(r"^ {1,2}").replace(l, "").into_owned();
                }
                let m = strip.captures(l).unwrap();
                let prefix = m.get(2).map_or("", |m| m.as_str());
                let body = &l[m[0].len()..];
                if kind.starts_with('h') {
                    let n = kind[1..].parse::<usize>().unwrap_or(1);
                    let mark = "#".repeat(n);
                    return if prefix.trim() == mark {
                        body.into()
                    } else {
                        format!("{mark} {body}")
                    };
                }
                let same = match kind {
                    "task" => regex(r"\[[ xX]\]").is_match(prefix),
                    "num" => prefix.starts_with(|c: char| c.is_ascii_digit()),
                    _ => regex(r"^[-*+]\s+$").is_match(prefix),
                };
                let mark = if same {
                    String::new()
                } else {
                    match kind {
                        "task" => "- [ ] ".into(),
                        "num" => format!("{}. ", i + 1),
                        _ => "- ".into(),
                    }
                };
                format!("{}{mark}{body}", &m[1])
            })
            .collect::<Vec<_>>()
            .join("\n");
        let body = format!(
            "{}{}{}",
            slice(&self.body, 0, a),
            out,
            slice(&self.body, b, self.body.chars().count())
        );
        let end = a + out.chars().count();
        self.commit(body, a, end, "edit", now);
    }
    pub fn format(&mut self, kind: &str, now: i64) {
        let (a, b) = self.range();
        match kind {
            "bold" | "italic" => {
                let mark = if kind == "bold" { "**" } else { "*" };
                let inner = slice(&self.body, a, b);
                let n = mark.chars().count();
                if inner.starts_with(mark) && inner.ends_with(mark) && inner.len() >= mark.len() * 2
                {
                    let inner = &inner[mark.len()..inner.len() - mark.len()];
                    let body = format!(
                        "{}{}{}",
                        slice(&self.body, 0, a),
                        inner,
                        slice(&self.body, b, self.body.chars().count())
                    );
                    self.commit(body, a, b - n * 2, "edit", now);
                } else {
                    let body = format!(
                        "{}{}{}{}{}",
                        slice(&self.body, 0, a),
                        mark,
                        inner,
                        mark,
                        slice(&self.body, b, self.body.chars().count())
                    );
                    self.commit(body, a + n, b + n, "edit", now);
                }
            }
            "link" => {
                let inner = slice(&self.body, a, b);
                let link = format!("[{inner}](url)");
                self.replace(
                    a,
                    b,
                    &link,
                    "edit",
                    Some(if a == b { a + 1 } else { b + 3 }),
                    now,
                );
            }
            "code" => {
                let inner = slice(&self.body, a, b);
                let pre = if a == self.line_start(a) { "" } else { "\n" };
                self.replace(
                    a,
                    b,
                    &format!("{pre}```\n{inner}\n```\n"),
                    "edit",
                    Some(a + pre.len() + 4 + inner.chars().count()),
                    now,
                );
            }
            _ => self.lines(kind, now),
        }
    }
    fn newline(&mut self, now: i64) {
        let (a, b) = self.range();
        let start = self.line_start(a);
        let line = slice(&self.body, start, a);
        if let Some(c) =
            regex(r"^(\s*)([-*+]\s+\[[ xX]\]\s+|[-*+]\s+|(\d+)([.)])\s+|>\s?)").captures(&line)
        {
            if line.trim() == c[0].trim() && a == b && self.line_end(a) == a {
                self.replace(start, a, "", "edit", None, now);
                return;
            }
            let mut mark = regex(r"\[[xX]\]").replace(&c[2], "[ ]").into_owned();
            if let Some(n) = c.get(3) {
                mark = format!("{}{} ", n.as_str().parse::<u64>().unwrap_or(0) + 1, &c[4]);
            }
            self.insert(&format!("\n{}{mark}", &c[1]), now);
        } else {
            let indent = regex(r"^\s*").find(&line).unwrap().as_str();
            self.insert(&format!("\n{indent}"), now);
        }
    }
    pub fn accept(&mut self, index: usize, now: i64) {
        if let Some((start, list, _)) = self.suggestion.take() {
            if let Some(e) = list.get(index) {
                self.replace(
                    start,
                    self.range().1,
                    &format!("{} ", e.ch),
                    "edit",
                    None,
                    now,
                );
                self.suggestion = None;
            }
        }
    }
    pub fn key(&mut self, key: &str, chr: &str, flags: i32, now: i64) -> Option<Value> {
        let ctrl = flags & (2 | 16) != 0;
        let shift = flags & 1 != 0;
        let alt = flags & 4 != 0;
        let base = key.to_lowercase();
        let (a, b) = self.range();
        if let Some((_, items, index)) = &mut self.suggestion {
            match key {
                "ArrowDown" => {
                    *index = (*index + 1) % items.len();
                    return None;
                }
                "ArrowUp" => {
                    *index = (*index + items.len() - 1) % items.len();
                    return None;
                }
                "Enter" | "Tab" => {
                    let i = *index;
                    self.accept(i, now);
                    return None;
                }
                "Escape" => {
                    self.suggestion = None;
                    return None;
                }
                _ => {}
            }
        }
        if ctrl && !alt {
            match base.as_str() {
                "z" | "y" => {
                    let redo = base == "y" || shift;
                    let current = self.snapshot();
                    let s = if redo {
                        self.redo.pop()
                    } else {
                        self.undo.pop()
                    };
                    if let Some(s) = s {
                        if redo {
                            self.undo.push(current);
                        } else {
                            self.redo.push(current);
                        }
                        self.body = s.body;
                        self.a = s.a;
                        self.b = s.b;
                        self.last = 0;
                        self.layout();
                        self.visible();
                    }
                    return None;
                }
                "a" => {
                    self.a = 0;
                    self.b = self.body.chars().count();
                    return None;
                }
                "c" if !shift => {
                    return if a < b {
                        Some(json!({"copy":slice(&self.body,a,b)}))
                    } else {
                        None
                    }
                }
                "x" if a < b => {
                    let copy = slice(&self.body, a, b);
                    if a < b {
                        self.replace(a, b, "", "edit", None, now);
                    }
                    return Some(json!({"copy":copy}));
                }
                "v" => return Some(json!({"paste":true})),
                "b" => {
                    self.format("bold", now);
                    return None;
                }
                "i" => {
                    self.format("italic", now);
                    return None;
                }
                "k" => {
                    self.format("link", now);
                    return None;
                }
                "1" | "2" | "3" => {
                    self.format(&format!("h{base}"), now);
                    return None;
                }
                "7" | "8" | "9" if shift => {
                    self.format(
                        match base.as_str() {
                            "7" => "num",
                            "8" => "bullet",
                            _ => "task",
                        },
                        now,
                    );
                    return None;
                }
                "c" if shift => {
                    self.format("code", now);
                    return None;
                }
                _ => {}
            }
        }
        let extend = shift;
        let n = self.body.chars().count();
        match key {
            "ArrowLeft" => self.move_to(
                if ctrl || alt {
                    self.word(self.b, false)
                } else if !shift && a < b {
                    a
                } else {
                    self.b.saturating_sub(1)
                },
                extend,
            ),
            "ArrowRight" => self.move_to(
                if ctrl || alt {
                    self.word(self.b, true)
                } else if !shift && a < b {
                    b
                } else {
                    (self.b + 1).min(n)
                },
                extend,
            ),
            "ArrowUp" | "ArrowDown" => {
                if ctrl {
                    self.move_to(if key == "ArrowUp" { 0 } else { n }, extend);
                } else {
                    let r = self.row_of(self.b) as i64 + if key == "ArrowUp" { -1 } else { 1 };
                    let goal = self.goal.unwrap_or(self.caret_x(self.b));
                    self.move_to(
                        if r < 0 {
                            0
                        } else if r >= self.rows.len() as i64 {
                            n
                        } else {
                            self.at(goal, r as f64 * self.lh() + 1.)
                        },
                        extend,
                    );
                    self.goal = Some(goal);
                }
            }
            "Home" => self.move_to(
                if ctrl {
                    0
                } else {
                    self.rows[self.row_of(self.b)].start
                },
                extend,
            ),
            "End" => self.move_to(
                if ctrl {
                    n
                } else {
                    self.rows[self.row_of(self.b)].end
                },
                extend,
            ),
            "Backspace" => self.replace(
                if a == b {
                    if ctrl {
                        self.word(a, false)
                    } else {
                        a.saturating_sub(1)
                    }
                } else {
                    a
                },
                b,
                "",
                "type",
                None,
                now,
            ),
            "Delete" => self.replace(
                a,
                if a == b { (b + 1).min(n) } else { b },
                "",
                "type",
                None,
                now,
            ),
            "Enter" => self.newline(now),
            "Tab" => {
                let line = slice(&self.body, self.line_start(a), self.line_end(a));
                if regex(r"^\s*([-*+]|\d+[.)])\s").is_match(&line) || a < b {
                    self.lines(if shift { "outdent" } else { "indent" }, now);
                } else {
                    self.insert("  ", now);
                }
            }
            "Escape" => {
                if a < b {
                    self.a = self.b;
                } else {
                    return Some(json!({"escape":true}));
                }
            }
            _ => {
                if !ctrl
                    && (!alt || flags & 8 != 0)
                    && !chr.is_empty()
                    && chr.chars().all(|c| c >= ' ')
                {
                    self.replace(a, b, chr, "type", None, now);
                }
            }
        }
        if key != "ArrowUp" && key != "ArrowDown" {
            self.goal = None;
        }
        None
    }
    pub fn down(&mut self, x: f64, y: f64, shift: bool, now: i64) {
        self.focused = true;
        let p = self.at(
            x - self.x - self.padding,
            y - self.y - self.padding + self.scroll,
        );
        let count = if now - self.clicks.0 < 400 {
            self.clicks.1 + 1
        } else {
            1
        };
        self.clicks = (now, count);
        self.suggestion = None;
        if count == 2 {
            let cs = chars(&self.body);
            let mut a = p;
            let mut b = p;
            while a > 0 && (cs[a - 1].is_alphanumeric() || cs[a - 1] == '_') {
                a -= 1;
            }
            while b < cs.len() && (cs[b].is_alphanumeric() || cs[b] == '_') {
                b += 1;
            }
            self.a = a;
            self.b = b;
        } else if count >= 3 {
            self.a = self.line_start(p);
            self.b = self.line_end(p);
        } else {
            if !shift {
                self.a = p;
            }
            self.b = p;
            self.drag = Some(self.a);
        }
    }
    pub fn drag_to(&mut self, x: f64, y: f64) {
        if let Some(a) = self.drag {
            self.a = a;
            self.b = self.at(
                x - self.x - self.padding,
                y - self.y - self.padding + self.scroll,
            );
            self.visible();
        }
    }
    pub fn render(&self, now: i64) -> Value {
        let first = (self.scroll / self.lh()).floor().max(0.) as usize;
        let mut nodes = vec![];
        let (a, b) = self.range();
        let mut fenced = false;
        let lines: Vec<_> = self
            .body
            .split('\n')
            .map(|l| {
                let fence = regex(r"^(`{3,}|~{3,})").is_match(l);
                let code = fenced || fence;
                if fence {
                    fenced = !fenced;
                }
                (l, code)
            })
            .collect();
        for (r, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(first.saturating_sub(2))
            .take((self.height / self.lh()).ceil() as usize + 6)
        {
            let y = self.padding + r as f64 * self.lh() - self.scroll;
            let cs = slice(&self.body, row.start, row.end);
            if b > a && row.end >= a && row.start <= b {
                let left = width(&slice(&self.body, row.start, a.max(row.start)), self.size);
                let right = width(&slice(&self.body, row.start, b.min(row.end)), self.size)
                    + if b > row.end { self.size * 0.4 } else { 0. };
                if right > left {
                    nodes.push(json!({"kind":"view","style":{"position":"absolute","left":self.padding+left,"top":y,"width":right-left,"height":self.lh(),"backgroundColor":"rgba(10,95,208,0.22)"}}));
                }
            }
            let (line, code) = lines.get(row.line).copied().unwrap_or(("", false));
            let heading = regex(r"^#{1,6}\s").is_match(line);
            nodes.push(json!({"kind":"text","label":cs,"style":{"position":"absolute","left":self.padding,"top":y+(self.lh()-self.size*1.3)/2.,"fontFamily":"monospace","fontSize":self.size,"fontWeight":if heading{700}else{400},"color":if code{"#5b3fa0"}else if heading{"#151515"}else if line.trim_start().starts_with('>'){"#6b6b6b"}else{"#2a2a2a"},"whiteSpace":"nowrap"}}));
        }
        if self.focused && (now / 530) % 2 == 0 {
            nodes.push(json!({"kind":"view","style":{"position":"absolute","left":self.padding+self.caret_x(self.b)-1.,"top":self.padding+self.row_of(self.b)as f64*self.lh()-self.scroll+2.,"width":2,"height":self.lh()-4.,"backgroundColor":"#0a5fd0"}}));
        }
        if let Some((_, list, index)) = &self.suggestion {
            let items:Vec<_>=list.iter().enumerate().map(|(i,e)|json!({"kind":"view","events":{"onMouseDown":["acceptEmoji",i]},"style":{"height":24,"paddingHorizontal":6,"flexDirection":"row","gap":8,"borderRadius":5,"backgroundColor":if i==*index{"#0a5fd0"}else{"transparent"}},"children":[{"kind":"text","label":e.ch,"style":{"fontSize":15}},{"kind":"text","label":format!(":{}:",e.name),"style":{"fontSize":12,"color":if i==*index{"#fff"}else{"#222"}}}]})).collect();
            nodes.push(json!({"kind":"view","style":{"position":"absolute","left":(self.padding+self.caret_x(self.b)).min((self.width-200.).max(0.)),"top":self.padding+(self.row_of(self.b)+1)as f64*self.lh()-self.scroll+2.,"width":196,"padding":4,"borderRadius":8,"backgroundColor":"#fff","borderWidth":1,"borderColor":"rgba(0,0,0,0.15)"},"children":items}));
        }
        json!({"kind":"view","key":"note-editor","style":{"flex":1,"overflow":"hidden","direction":"ltr"},"events":{"onMouseDown":["editorDown"],"onWheel":["editorWheel"]},"children":nodes})
    }
}
