//! tmux in control mode (`tmux -CC`), the part that is only text and trees: the protocol lines
//! (`%begin`/`%end` blocks and notifications), tmux's layout strings and their translation into the
//! pane tree of a tab. The window that owns the session does the I/O. A port of `tmux.js`.
use crate::workspace::{Dir, Node, Sid};
use std::collections::VecDeque;

/// Nesting deeper than this is not a layout tmux would send.
const DEPTH_MAX: usize = 64;

/// A window's layout: panes with their size, or rows/columns of layouts.
#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Pane { w: i64, h: i64, pane: i64 },
    Split { w: i64, h: i64, dir: Dir, kids: Vec<Layout> },
}
impl Layout {
    pub fn panes(&self) -> Vec<i64> {
        match self {
            Layout::Pane { pane, .. } => vec![*pane],
            Layout::Split { kids, .. } => kids.iter().flat_map(Layout::panes).collect(),
        }
    }
    fn size(&self, dir: Dir) -> i64 {
        match (self, dir) {
            (Layout::Pane { w, .. } | Layout::Split { w, .. }, Dir::Row) => *w,
            (Layout::Pane { h, .. } | Layout::Split { h, .. }, Dir::Column) => *h,
        }
    }
}
fn number(b: &[u8], i: &mut usize) -> i64 {
    if *i >= b.len() {
        return 0;
    }
    let start = *i;
    while *i < b.len() && b[*i].is_ascii_digit() {
        *i += 1;
    }
    std::str::from_utf8(&b[start..*i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0)
}
fn node(b: &[u8], i: &mut usize, depth: usize) -> Option<Layout> {
    if depth > DEPTH_MAX {
        return None;
    }
    // "WxH,X,Y" and then either ",pane" or a bracketed list of children
    let w = number(b, i);
    *i += 1;
    let h = number(b, i);
    *i += 1;
    number(b, i);
    *i += 1;
    number(b, i);
    if b.get(*i) == Some(&b',') {
        *i += 1;
        return Some(Layout::Pane { w, h, pane: number(b, i) });
    }
    let open = *b.get(*i)?;
    let (dir, close) = match open {
        b'{' => (Dir::Row, b'}'),
        b'[' => (Dir::Column, b']'),
        _ => return None,
    };
    *i += 1;
    let mut kids = vec![node(b, i, depth + 1)?];
    while b.get(*i) == Some(&b',') {
        *i += 1;
        kids.push(node(b, i, depth + 1)?);
    }
    if b.get(*i) == Some(&close) {
        *i += 1;
    }
    Some(Layout::Split { w, h, dir, kids })
}
/// `b25d,160x48,0,0{80x48,0,0,1,79x48,81,0,2}`: the checksum, then the tree.
pub fn parse_layout(text: &str) -> Option<Layout> {
    let b = text.as_bytes();
    let mut i = text.find(',')? + 1;
    node(b, &mut i, 0)
}
fn build(kids: &[Layout], dir: Dir, sid_of: &dyn Fn(i64) -> Option<Sid>, next_id: &mut dyn FnMut() -> String) -> Option<Node> {
    match kids {
        [] => None,
        [only] => to_tree(only, sid_of, next_id),
        [first, rest @ ..] => {
            // tmux lists n children; the tab holds binary splits, each worth the share of its size
            let total = kids.iter().map(|k| k.size(dir)).sum::<i64>().max(1);
            let ratio = (first.size(dir) as f64 / total as f64).clamp(0.1, 0.9);
            let id = next_id();
            Some(Node::Split { id, dir, ratio, a: Box::new(to_tree(first, sid_of, next_id)?), b: Box::new(build(rest, dir, sid_of, next_id)?) })
        }
    }
}
/// The pane tree of a layout; None while some pane has no session yet.
pub fn to_tree(layout: &Layout, sid_of: &dyn Fn(i64) -> Option<Sid>, next_id: &mut dyn FnMut() -> String) -> Option<Node> {
    match layout {
        Layout::Pane { pane, .. } => sid_of(*pane).map(Node::Leaf),
        Layout::Split { dir, kids, .. } => build(kids, *dir, sid_of, next_id),
    }
}
/// The size tmux must give a window so that its panes fill the tab: panes plus one cell per divider.
pub fn window_size(root: &Node, leaf: &dyn Fn(Sid) -> (i64, i64)) -> (i64, i64) {
    match root {
        Node::Leaf(sid) => leaf(*sid),
        Node::Split { dir, a, b, .. } => {
            let ((aw, ah), (bw, bh)) = (window_size(a, leaf), window_size(b, leaf));
            match dir {
                Dir::Row => (aw + 1 + bw, ah.max(bh)),
                Dir::Column => (aw.max(bw), ah + 1 + bh),
            }
        }
    }
}

/// What a command sent to tmux asked for, to know what to do with its answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Ignore,
    Windows,
    Capture { pane: i64 },
    Cursor { pane: i64, lines: Vec<String> },
}
/// What the lines tmux sent mean.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Answer(Reply, Vec<String>),
    Failed(String),
    Layout { window: String, layout: String },
    ListWindows,
    WindowClosed(String),
    WindowRenamed { window: String, name: String },
    PaneChanged { window: String, pane: i64 },
    Exit(String),
}
/// The conversation with one `tmux -CC`: answers come in the order the commands went out.
#[derive(Default)]
pub struct Protocol {
    queue: VecDeque<Reply>,
    block: Option<Vec<String>>,
}
impl Protocol {
    /// `resumed`: the session was already in control mode when the window started, so no answer is
    /// owed to the command that launched tmux.
    pub fn new(resumed: bool) -> Self {
        Self { queue: if resumed { VecDeque::new() } else { VecDeque::from([Reply::Ignore]) }, block: None }
    }
    /// A command went out; its answer is `reply`.
    pub fn expect(&mut self, reply: Reply) {
        self.queue.push_back(reply);
    }
    /// The server hands over whole lines, each ended by a newline: that last newline ends a line,
    /// it does not start a blank one.
    pub fn feed(&mut self, text: &str) -> Vec<Item> {
        let mut out = vec![];
        for line in text.strip_suffix('\n').unwrap_or(text).split('\n') {
            if let Some(lines) = self.block.as_mut() {
                if line.starts_with("%end ") || line.starts_with("%error ") {
                    let lines = std::mem::take(lines);
                    self.block = None;
                    let reply = self.queue.pop_front();
                    if line.starts_with("%error") {
                        out.push(Item::Failed(if lines.is_empty() { "error".to_owned() } else { lines.join(" ") }));
                    } else if let Some(reply) = reply {
                        out.push(Item::Answer(reply, lines));
                    }
                } else {
                    lines.push(line.to_owned());
                }
                continue;
            }
            if line.is_empty() {
                continue;
            }
            let mut words = line.split(' ');
            let kind = words.next().unwrap_or("");
            let first = words.next().unwrap_or("").to_owned();
            let rest: Vec<&str> = words.collect();
            match kind {
                "%begin" => self.block = Some(vec![]),
                "%layout-change" => out.push(Item::Layout { window: first, layout: rest.first().copied().unwrap_or("").to_owned() }),
                "%window-add" | "%unlinked-window-add" | "%sessions-changed" | "%session-changed" => out.push(Item::ListWindows),
                "%window-close" | "%unlinked-window-close" => {
                    out.push(Item::WindowClosed(first));
                    out.push(Item::ListWindows);
                }
                "%window-renamed" => out.push(Item::WindowRenamed { window: first, name: rest.join(" ") }),
                "%window-pane-changed" => {
                    if let Some(pane) = rest.first().and_then(|p| p.strip_prefix('%')).and_then(|p| p.parse().ok()) {
                        out.push(Item::PaneChanged { window: first, pane });
                    }
                }
                "%exit" => out.push(Item::Exit(if rest.is_empty() { first } else { format!("{first} {}", rest.join(" ")) })),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(w: i64, h: i64, pane: i64) -> Layout {
        Layout::Pane { w, h, pane }
    }
    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("s{n}")
        }
    }

    #[test]
    fn a_layout_string_is_a_tree_of_panes() {
        assert_eq!(parse_layout("b25d,160x48,0,0,5"), Some(leaf(160, 48, 5)));
        let two = parse_layout("b25d,160x48,0,0{80x48,0,0,1,79x48,81,0,2}").unwrap();
        assert_eq!(two, Layout::Split { w: 160, h: 48, dir: Dir::Row, kids: vec![leaf(80, 48, 1), leaf(79, 48, 2)] });
        let nested = parse_layout("x,160x48,0,0[160x24,0,0,1,160x23,0,25{80x23,0,25,2,79x23,81,25,3}]").unwrap();
        assert_eq!(nested.panes(), [1, 2, 3]);
        assert!(matches!(&nested, Layout::Split { dir: Dir::Column, kids, .. } if kids.len() == 2));
    }

    #[test]
    fn broken_layouts_are_refused_not_guessed() {
        for text in ["", "no comma", "x,", "x,160x48,0,0", "x,160x48,0,0{", "x,160x48,0,0{80x48,0,0,1,", "x,160x48,0,0?1"] {
            assert_eq!(parse_layout(text), None, "{text:?}");
        }
        let deep = format!("x,{}", "1x1,0,0[".repeat(200));
        assert_eq!(parse_layout(&deep), None, "nesting is bounded");
    }

    #[test]
    fn many_children_become_binary_splits_sharing_their_size() {
        let layout = parse_layout("x,100x10,0,0{25x10,0,0,1,25x10,26,0,2,50x10,52,0,3}").unwrap();
        let tree = to_tree(&layout, &|pane| Some(pane + 100), &mut ids()).unwrap();
        let Node::Split { dir, ratio, a, b, .. } = &tree else { panic!("a split") };
        assert_eq!((*dir, **a == Node::Leaf(101)), (Dir::Row, true));
        assert!((ratio - 0.25).abs() < 1e-9, "the first child is a quarter of the width");
        let Node::Split { ratio: second, a: a2, b: b2, .. } = &**b else { panic!("the rest is a split") };
        assert!((second - 0.333_333).abs() < 1e-3, "and the second a third of what is left");
        assert_eq!((**a2).clone(), Node::Leaf(102));
        assert_eq!((**b2).clone(), Node::Leaf(103));
        assert_eq!(tree.leaves(), [101, 102, 103]);
    }

    #[test]
    fn ratios_stay_in_range_and_a_pane_without_a_session_defers_the_tree() {
        let layout = parse_layout("x,100x10,0,0{1x10,0,0,1,98x10,2,0,2}").unwrap();
        let Node::Split { ratio, .. } = to_tree(&layout, &|p| Some(p), &mut ids()).unwrap() else { panic!() };
        assert_eq!(ratio, 0.1, "a sliver is not allowed to vanish");
        assert_eq!(to_tree(&layout, &|pane| (pane != 2).then_some(pane), &mut ids()), None);
    }

    #[test]
    fn the_window_size_adds_a_cell_for_each_divider() {
        let layout = parse_layout("x,100x20,0,0{49x20,0,0,1,50x20,50,0,2}").unwrap();
        let tree = to_tree(&layout, &|p| Some(p), &mut ids()).unwrap();
        assert_eq!(window_size(&tree, &|sid| if sid == 1 { (49, 20) } else { (50, 19) }), (100, 20));
        let column = Node::Split { id: "s".into(), dir: Dir::Column, ratio: 0.5, a: Box::new(Node::Leaf(1)), b: Box::new(Node::Leaf(2)) };
        assert_eq!(window_size(&column, &|_| (80, 10)), (80, 21));
    }

    #[test]
    fn answers_pair_with_the_commands_in_order_and_the_launch_answer_is_skipped() {
        let mut p = Protocol::new(false);
        p.expect(Reply::Windows);
        p.expect(Reply::Capture { pane: 3 });
        let items = p.feed("%begin 1 1 0\n%end 1 1 0\n%begin 2 2 0\n@1\tlayout\tname\n%end 2 2 0\n%begin 3 3 0\nline one\nline two\n%end 3 3 0\n");
        assert_eq!(
            items,
            [
                Item::Answer(Reply::Ignore, vec![]),
                Item::Answer(Reply::Windows, vec!["@1\tlayout\tname".into()]),
                Item::Answer(Reply::Capture { pane: 3 }, vec!["line one".into(), "line two".into()]),
            ]
        );
        let mut resumed = Protocol::new(true);
        resumed.expect(Reply::Windows);
        assert_eq!(resumed.feed("%begin 1 1 0\n%end 1 1 0\n"), [Item::Answer(Reply::Windows, vec![])], "nothing is owed to a launch command that was not sent");
    }

    #[test]
    fn a_block_may_span_several_batches_and_an_error_is_reported_with_its_text() {
        let mut p = Protocol::new(true);
        p.expect(Reply::Ignore);
        assert_eq!(p.feed("%begin 1 1 0\nno such pane\n"), []);
        assert_eq!(p.feed("%error 1 1 0\n"), [Item::Failed("no such pane".into())]);
        p.expect(Reply::Ignore);
        assert_eq!(p.feed("%begin 2 2 0\n%error 2 2 0\n"), [Item::Failed("error".into())]);
    }

    #[test]
    fn notifications_become_items() {
        let mut p = Protocol::new(true);
        let items = p.feed(
            "%layout-change @2 b25d,80x24,0,0,7 b25d,80x24,0,0,7 *\n%window-add @3\n%window-close @2\n%window-renamed @1 my window\n%window-pane-changed @1 %4\n%output %1 hi\n%exit server exited\n%exit\n\n",
        );
        assert_eq!(
            items,
            [
                Item::Layout { window: "@2".into(), layout: "b25d,80x24,0,0,7".into() },
                Item::ListWindows,
                Item::WindowClosed("@2".into()),
                Item::ListWindows,
                Item::WindowRenamed { window: "@1".into(), name: "my window".into() },
                Item::PaneChanged { window: "@1".into(), pane: 4 },
                Item::Exit("server exited".into()),
                Item::Exit(String::new()),
            ]
        );
    }
}
