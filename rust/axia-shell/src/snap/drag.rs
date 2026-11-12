//! The zone under the pointer while a window is moved. runtime/window_grab.rs feeds it every pointer motion (and
//! Shift changes) with no JS per frame, and tells the session only when the zone changes.
use super::{zone_at, zone_rect, SnapRect, Zone};

/// One display: its bounds and its work area (without menu bar, taskbar or Dock).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnapDisplay {
    pub bounds: SnapRect,
    pub work: SnapRect,
}

pub const MAX_DISPLAYS: usize = 8;

#[derive(Debug, Default)]
pub struct DragZones {
    displays: Vec<SnapDisplay>,
    current: Option<(Zone, usize)>,
}

impl DragZones {
    pub fn new(displays: &[SnapDisplay]) -> Self {
        let displays = displays
            .iter()
            .filter(|d| d.bounds.is_valid() && d.work.is_valid())
            .take(MAX_DISPLAYS)
            .copied()
            .collect();
        Self { displays, current: None }
    }

    /// The display under (x, y), else the nearest one (runtime's displayAt).
    fn display_at(&self, x: f64, y: f64) -> Option<usize> {
        let mut best = None;
        let mut dist = f64::INFINITY;
        for (i, d) in self.displays.iter().enumerate() {
            let b = d.bounds;
            let dx = (b.x - x).max(0.0).max(x - (b.x + b.w - 1.0));
            let dy = (b.y - y).max(0.0).max(y - (b.y + b.h - 1.0));
            let k = dx * dx + dy * dy;
            if k < dist {
                dist = k;
                best = Some(i);
            }
        }
        best
    }

    /// The pointer moved or Shift changed: true when the zone is a different one.
    pub fn update(&mut self, x: f64, y: f64, thirds: bool) -> bool {
        let next = self
            .display_at(x, y)
            .and_then(|i| zone_at(x, y, self.displays[i].work, thirds).map(|z| (z, i)));
        let changed = next != self.current;
        self.current = next;
        changed
    }

    pub fn current(&self) -> Option<(Zone, SnapRect)> {
        self.current.map(|(z, i)| (z, zone_rect(z, self.displays[i].work)))
    }
}
