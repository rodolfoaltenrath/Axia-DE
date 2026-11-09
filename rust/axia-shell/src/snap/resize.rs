//! Snapped windows that share an edge resize together: dragging the line between them moves every
//! window on either side of it, as one divider. Member 0 is the window being resized; the others
//! are the windows snapped on the same display. A dragged edge on the border of the work area has
//! nobody on its other side and resizes only member 0.
use super::SnapRect;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnapMember {
    pub frame: SnapRect,
    pub min_w: f64,
    pub min_h: f64,
    /// 0 = no maximum
    pub max_w: f64,
    pub max_h: f64,
}

pub const MAX_MEMBERS: usize = 9;
const TOP: i32 = 1;
const BOTTOM: i32 = 2;
const LEFT: i32 = 4;
const RIGHT: i32 = 8;
/// Edges this close count as the same line (frames are whole pixels; this absorbs rounding).
const TOLERANCE: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

fn near(r: &SnapRect, a: Axis) -> f64 {
    if a == Axis::X { r.x } else { r.y }
}
fn far(r: &SnapRect, a: Axis) -> f64 {
    if a == Axis::X { r.right() } else { r.bottom() }
}
fn min_of(m: &SnapMember, a: Axis) -> f64 {
    (if a == Axis::X { m.min_w } else { m.min_h }).max(1.0)
}
fn max_of(m: &SnapMember, a: Axis) -> Option<f64> {
    let v = if a == Axis::X { m.max_w } else { m.max_h };
    (v > 0.0).then_some(v)
}
fn set_span(r: &mut SnapRect, a: Axis, start: f64, end: f64) {
    if a == Axis::X {
        r.x = start;
        r.w = end - start;
    } else {
        r.y = start;
        r.h = end - start;
    }
}

/// A line between windows: `before` end on it (their right or bottom edge), `after` start on it.
#[derive(Debug)]
struct Divider {
    axis: Axis,
    pos: f64,
    before: Vec<usize>,
    after: Vec<usize>,
    lo: f64,
    hi: f64,
}

impl Divider {
    fn find(members: &[SnapMember], axis: Axis, line: f64) -> Option<Divider> {
        let on = |v: f64| (v - line).abs() <= TOLERANCE;
        let before: Vec<usize> = (0..members.len()).filter(|&i| on(far(&members[i].frame, axis))).collect();
        let after: Vec<usize> = (0..members.len()).filter(|&i| on(near(&members[i].frame, axis))).collect();
        if before.is_empty() || after.is_empty() {
            return None;
        }
        // every window keeps its minimum (and maximum) size on its side of the line
        let mut lo = f64::NEG_INFINITY;
        let mut hi = f64::INFINITY;
        for &i in &before {
            let m = &members[i];
            lo = lo.max(near(&m.frame, axis) + min_of(m, axis));
            if let Some(max) = max_of(m, axis) {
                hi = hi.min(near(&m.frame, axis) + max);
            }
        }
        for &i in &after {
            let m = &members[i];
            hi = hi.min(far(&m.frame, axis) - min_of(m, axis));
            if let Some(max) = max_of(m, axis) {
                lo = lo.max(far(&m.frame, axis) - max);
            }
        }
        // a window already outside its limits doesn't make the line jump when the drag starts
        lo = lo.min(line);
        hi = hi.max(line);
        Some(Divider { axis, pos: line, before, after, lo, hi })
    }

    fn apply(&self, start: &[SnapRect], delta: f64, out: &mut [SnapRect]) {
        let p = (self.pos + delta).clamp(self.lo, self.hi).round();
        for &i in &self.before {
            set_span(&mut out[i], self.axis, near(&start[i], self.axis), p);
        }
        for &i in &self.after {
            set_span(&mut out[i], self.axis, p, far(&start[i], self.axis));
        }
    }
}

#[derive(Debug)]
pub struct LinkedResize {
    members: Vec<SnapMember>,
    area: SnapRect,
    edges: i32,
    x: Option<Divider>,
    y: Option<Divider>,
}

impl LinkedResize {
    /// None when no dragged edge of member 0 is shared with another window: the plain resize applies.
    pub fn new(edges: i32, members: &[SnapMember], area: SnapRect) -> Option<Self> {
        let members: Vec<SnapMember> = members.iter().take(MAX_MEMBERS).copied().collect();
        if members.len() < 2 || !area.is_valid() || members.iter().any(|m| !m.frame.is_valid()) {
            return None;
        }
        let l = members[0].frame;
        let x = if edges & RIGHT != 0 {
            Divider::find(&members, Axis::X, l.right())
        } else if edges & LEFT != 0 {
            Divider::find(&members, Axis::X, l.x)
        } else {
            None
        };
        let y = if edges & BOTTOM != 0 {
            Divider::find(&members, Axis::Y, l.bottom())
        } else if edges & TOP != 0 {
            Divider::find(&members, Axis::Y, l.y)
        } else {
            None
        };
        if x.is_none() && y.is_none() {
            return None;
        }
        Some(Self { members, area, edges, x, y })
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether member `i` changes with this drag (member 0 always does).
    pub fn moves(&self, i: usize) -> bool {
        i == 0
            || [&self.x, &self.y]
                .into_iter()
                .flatten()
                .any(|d| d.before.contains(&i) || d.after.contains(&i))
    }

    /// The frames of every member with the pointer moved by (dx, dy) since the drag started.
    pub fn apply(&self, dx: f64, dy: f64) -> Vec<SnapRect> {
        let start: Vec<SnapRect> = self.members.iter().map(|m| m.frame).collect();
        let mut out = start.clone();
        let dx = if dx.is_finite() { dx } else { 0.0 };
        let dy = if dy.is_finite() { dy } else { 0.0 };
        match &self.x {
            Some(d) => d.apply(&start, dx, &mut out),
            None => self.free(Axis::X, dx, &mut out[0]),
        }
        match &self.y {
            Some(d) => d.apply(&start, dy, &mut out),
            None => self.free(Axis::Y, dy, &mut out[0]),
        }
        out
    }

    /// A dragged edge of member 0 with nobody behind it (on the border of the work area): only it
    /// resizes, inside its limits and the work area.
    fn free(&self, axis: Axis, delta: f64, r: &mut SnapRect) {
        let (low_edge, high_edge) = if axis == Axis::X { (LEFT, RIGHT) } else { (TOP, BOTTOM) };
        let m = &self.members[0];
        let (s, e) = (near(&m.frame, axis), far(&m.frame, axis));
        let (area_s, area_e) = (near(&self.area, axis), far(&self.area, axis));
        let min = min_of(m, axis);
        let max = max_of(m, axis).unwrap_or(f64::INFINITY);
        if self.edges & high_edge != 0 {
            let end = (e + delta).min(area_e.max(e)).clamp(s + min, s + max.max(min));
            set_span(r, axis, s, end);
        } else if self.edges & low_edge != 0 {
            let start = (s + delta).max(area_s.min(s)).clamp(e - max.max(min), e - min);
            set_span(r, axis, start, e);
        }
    }
}
