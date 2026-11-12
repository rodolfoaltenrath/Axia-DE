//! Window snapping (docs/WINDOWS.md, "Snapping").
//!
//! The geometry of the zones, the zone under the pointer while a window is dragged, the windows that
//! resize together when a snapped window's inner edge is dragged, and what Snap Assist offers after a
//! snap. Everything is pure and works on frames in screen pixels; the Rust runtime
//! (`runtime/window_grab.rs`) owns drag state and updates Rust-owned nodes and the JS session owns the window list.
mod assist;
mod drag;
mod ffi;
mod grab;
mod resize;
#[cfg(test)]
mod tests;

pub use assist::assist_zones;
pub(crate) use grab::{Grab, GrabStart, GrabArea, MOVED, SIZED, DETACHED};
pub use drag::{DragZones, SnapDisplay};
pub use resize::{LinkedResize, SnapMember};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnapRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl SnapRect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn overlaps(&self, o: &SnapRect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
    pub fn is_valid(&self) -> bool {
        [self.x, self.y, self.w, self.h].iter().all(|v| v.is_finite()) && self.w >= 0.0 && self.h >= 0.0
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Max,
    LeftThird,
    CenterThird,
    RightThird,
    LeftTwoThirds,
    RightTwoThirds,
}

impl Zone {
    /// In the order of their indices across the C ABI (and of `lord.snap.zones`).
    pub const ALL: [Zone; 14] = [
        Zone::Left,
        Zone::Right,
        Zone::Top,
        Zone::Bottom,
        Zone::TopLeft,
        Zone::TopRight,
        Zone::BottomLeft,
        Zone::BottomRight,
        Zone::Max,
        Zone::LeftThird,
        Zone::CenterThird,
        Zone::RightThird,
        Zone::LeftTwoThirds,
        Zone::RightTwoThirds,
    ];
    const NAMES: [&'static str; 14] = [
        "left",
        "right",
        "top",
        "bottom",
        "top-left",
        "top-right",
        "bottom-left",
        "bottom-right",
        "max",
        "left-third",
        "center-third",
        "right-third",
        "left-two-thirds",
        "right-two-thirds",
    ];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
    pub fn index(self) -> i32 {
        self as i32
    }
    pub fn from_index(i: i32) -> Option<Zone> {
        usize::try_from(i).ok().and_then(|i| Self::ALL.get(i).copied())
    }
    pub fn from_name(name: &str) -> Option<Zone> {
        Self::NAMES.iter().position(|n| *n == name).map(|i| Self::ALL[i])
    }
    fn is_third(self) -> bool {
        matches!(
            self,
            Zone::LeftThird | Zone::CenterThird | Zone::RightThird | Zone::LeftTwoThirds | Zone::RightTwoThirds
        )
    }
}

/// JavaScript's Math.round, which the layouts were tuned with: halves go up.
fn round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// The rectangle of `zone` inside work area `a`. Halves, quarters and thirds round to whole pixels
/// and share their middle lines, so two snapped windows never overlap or leave a gap.
pub fn zone_rect(zone: Zone, a: SnapRect) -> SnapRect {
    let hw = round(a.w / 2.0);
    let hh = round(a.h / 2.0);
    let t1 = round(a.w / 3.0);
    let t2 = round(2.0 * a.w / 3.0);
    let r = SnapRect::new;
    match zone {
        Zone::Left => r(a.x, a.y, hw, a.h),
        Zone::Right => r(a.x + hw, a.y, a.w - hw, a.h),
        Zone::Top => r(a.x, a.y, a.w, hh),
        Zone::Bottom => r(a.x, a.y + hh, a.w, a.h - hh),
        Zone::TopLeft => r(a.x, a.y, hw, hh),
        Zone::TopRight => r(a.x + hw, a.y, a.w - hw, hh),
        Zone::BottomLeft => r(a.x, a.y + hh, hw, a.h - hh),
        Zone::BottomRight => r(a.x + hw, a.y + hh, a.w - hw, a.h - hh),
        Zone::LeftThird => r(a.x, a.y, t1, a.h),
        Zone::CenterThird => r(a.x + t1, a.y, t2 - t1, a.h),
        Zone::RightThird => r(a.x + t2, a.y, a.w - t2, a.h),
        Zone::LeftTwoThirds => r(a.x, a.y, t2, a.h),
        Zone::RightTwoThirds => r(a.x + t1, a.y, a.w - t1, a.h),
        Zone::Max => a,
    }
}

/// How close to a side of the work area the pointer must be (or past it, over a taskbar there).
pub const EDGE: f64 = 8.0;

/// The zone for a window dropped with the pointer at (x, y) in work area `a`, or None. At the left or
/// right side a half, in the top or bottom fifth of that side a quarter, at the top edge the whole
/// area. With `thirds` (Shift held) the sides give the outer thirds and the top edge the middle one.
pub fn zone_at(x: f64, y: f64, a: SnapRect, thirds: bool) -> Option<Zone> {
    if !a.is_valid() || !x.is_finite() || !y.is_finite() {
        return None;
    }
    let left = x <= a.x + EDGE;
    let right = !left && x >= a.x + a.w - 1.0 - EDGE;
    if left || right {
        if thirds {
            return Some(if left { Zone::LeftThird } else { Zone::RightThird });
        }
        let band = (a.h / 5.0).max(48.0);
        let top = y < a.y + band;
        let bottom = !top && y > a.y + a.h - band;
        return Some(match (left, top, bottom) {
            (true, true, _) => Zone::TopLeft,
            (true, _, true) => Zone::BottomLeft,
            (true, _, _) => Zone::Left,
            (false, true, _) => Zone::TopRight,
            (false, _, true) => Zone::BottomRight,
            (false, _, _) => Zone::Right,
        });
    }
    if y <= a.y + EDGE {
        return Some(if thirds { Zone::CenterThird } else { Zone::Max });
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Zone(Zone),
    Restore,
    Minimize,
}

/// Super+arrows: where `dir` takes a window that is at `at` (a zone, Max, or None when free).
pub fn snap_next(at: Option<Zone>, dir: Dir) -> Option<Next> {
    use Dir::*;
    use Next::{Minimize, Restore};
    let z = Next::Zone;
    match at {
        None => Some(match dir {
            Left => z(Zone::Left),
            Right => z(Zone::Right),
            Up => z(Zone::Max),
            Down => Minimize,
        }),
        Some(at) if at.is_third() => Some(match dir {
            Left => z(Zone::Left),
            Right => z(Zone::Right),
            Up => z(Zone::Max),
            Down => Restore,
        }),
        Some(Zone::Max) => match dir {
            Left => Some(z(Zone::Left)),
            Right => Some(z(Zone::Right)),
            Down => Some(Restore),
            Up => None,
        },
        Some(Zone::Left) => match dir {
            Right => Some(Restore),
            Up => Some(z(Zone::TopLeft)),
            Down => Some(z(Zone::BottomLeft)),
            Left => None,
        },
        Some(Zone::Right) => match dir {
            Left => Some(Restore),
            Up => Some(z(Zone::TopRight)),
            Down => Some(z(Zone::BottomRight)),
            Right => None,
        },
        Some(Zone::TopLeft) => match dir {
            Right => Some(z(Zone::TopRight)),
            Up => Some(z(Zone::Max)),
            Down => Some(z(Zone::Left)),
            Left => None,
        },
        Some(Zone::TopRight) => match dir {
            Left => Some(z(Zone::TopLeft)),
            Up => Some(z(Zone::Max)),
            Down => Some(z(Zone::Right)),
            Right => None,
        },
        Some(Zone::BottomLeft) => match dir {
            Right => Some(z(Zone::BottomRight)),
            Up => Some(z(Zone::Left)),
            Down => Some(Minimize),
            Left => None,
        },
        Some(Zone::BottomRight) => match dir {
            Left => Some(z(Zone::BottomLeft)),
            Up => Some(z(Zone::Right)),
            Down => Some(Minimize),
            Right => None,
        },
        // the top and bottom halves come only from the Window menu: the arrows leave them alone
        Some(_) => None,
    }
}
