//! Where a window's node goes while it is moved or resized with the pointer (docs/WINDOWS.md).
//!
//! `runtime/window_grab.rs` updates the node and calls in once per frame with the pointer; this decides
//! the node's left, top, width and height, in its parent's coordinates (the node is the frame enlarged
//! by `inset` per side):
//! - moved, the frame keeps `KEEP` px on screen and its top no higher than the work area of the
//!   display under its centre (its menu bar);
//! - resized, the frame keeps its minimum and maximum sizes, inside the screen and under the menu bar;
//! - a maximized or snapped window comes loose at its own size once the pointer has moved `DETACH`
//!   px, the point grabbed staying under the pointer at the same fraction of the width.
//!
//! At the end, the runtime reports the change from the node at the start (`delta`), and puts the node back
//! where the theme had it, for the window list to place it (runtime/window_grab.rs, `restore_nodes`).
use super::SnapRect;

/// px of the frame that always stay on screen
pub const KEEP: f64 = 48.;
/// px the pointer moves before a maximized or snapped window comes loose
pub const DETACH: f64 = 6.;
pub const MAX_AREAS: usize = 4;

/// motion's result: the node moved, its size changed, it came loose (its layer is painted again)
pub const MOVED: i32 = 1;
pub const SIZED: i32 = 2;
pub const DETACHED: i32 = 4;

/// One display's horizontal span [x, x + w) and the top of its work area, on screen.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GrabArea {
    pub x: f64,
    pub w: f64,
    pub top: f64,
}

/// A drag as it starts (server/src/ui/snap_ffi.h, AxiaGrabStart).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GrabStart {
    /// the node's left, top, width and height in its parent
    pub node: SnapRect,
    /// the parent's origin on screen
    pub origin_x: f64,
    pub origin_y: f64,
    /// where the button was pressed, on screen
    pub pointer_x: f64,
    pub pointer_y: f64,
    /// 0 moves; otherwise the xdg edges dragged (1 top, 2 bottom, 4 left, 8 right)
    pub edges: i32,
    pub inset: f64,
    /// of the frame; a maximum of 0 is none
    pub min_w: f64,
    pub min_h: f64,
    pub max_w: f64,
    pub max_h: f64,
    /// the frame goes no higher than this, on screen, unless an area under its centre says otherwise
    pub min_top: f64,
    pub nareas: i32,
    pub areas: [GrabArea; MAX_AREAS],
    /// the node's size once a maximized or snapped window comes loose; 0 keeps its size
    pub detach_w: f64,
    pub detach_h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Detach {
    No,
    Waiting,
    Done,
}

#[derive(Clone, Debug)]
pub struct Grab {
    s: GrabStart,
    /// what the pointer's offset applies to: the node at the start, at its own size once loose
    base: SnapRect,
    now: SnapRect,
    detach: Detach,
}

/// `v` kept in [lo, hi], like C's clampf: when they cross (a frame too narrow for KEEP px on a tiny
/// screen), lo below it and hi above it, never a panic.
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

impl Grab {
    pub fn new(mut s: GrabStart) -> Grab {
        s.edges &= 15;
        let fine = |v: f64| v.is_finite();
        if ![s.inset, s.min_w, s.min_h, s.max_w, s.max_h, s.min_top, s.detach_w, s.detach_h].into_iter().all(fine) {
            s.max_w = 0.;
            s.max_h = 0.;
            s.detach_w = 0.;
            s.detach_h = 0.;
        }
        let detach = if s.edges == 0 && s.detach_w > 0. && s.detach_h > 0. { Detach::Waiting } else { Detach::No };
        Grab { s, base: s.node, now: s.node, detach }
    }

    /// Still within `DETACH` px of where a maximized or snapped window was pressed: it hasn't moved.
    pub fn waiting(&self) -> bool {
        self.detach == Detach::Waiting
    }

    pub fn detached(&self) -> bool {
        self.detach == Detach::Done
    }

    /// The node now.
    pub fn now(&self) -> SnapRect {
        self.now
    }

    /// The node's change since the start.
    pub fn delta(&self) -> SnapRect {
        let (a, b) = (self.s.node, self.now);
        SnapRect::new(b.x - a.x, b.y - a.y, b.w - a.w, b.h - a.h)
    }

    fn areas(&self) -> &[GrabArea] {
        &self.s.areas[..(self.s.nareas.max(0) as usize).min(MAX_AREAS)]
    }

    /// Comes loose: its own size, the point grabbed under the pointer at the same fraction of the
    /// width, the top where it was (so is the title bar).
    fn come_loose(&mut self) {
        let (s, b) = (&self.s, self.base);
        let gx = s.pointer_x - s.origin_x;
        let (fw, nw) = (b.w - 2. * s.inset, s.detach_w - 2. * s.inset);
        let rel = if fw > 0. { clamp((gx - b.x - s.inset) / fw, 0., 1.) } else { 0.5 };
        self.base = SnapRect::new((gx - s.inset - rel * nw).round(), b.y, s.detach_w, s.detach_h);
        self.detach = Detach::Done;
    }

    /// The pointer is at (px, py) on a screen of `screen_w`×`screen_h`: MOVED, SIZED and DETACHED
    /// as they happened (0: the node stays).
    pub fn motion(&mut self, px: f64, py: f64, screen_w: f64, screen_h: f64) -> i32 {
        if ![px, py, screen_w, screen_h].into_iter().all(f64::is_finite) {
            return 0;
        }
        let mut flags = 0;
        if self.detach == Detach::Waiting {
            if (px - self.s.pointer_x).abs() + (py - self.s.pointer_y).abs() < DETACH {
                return 0;
            }
            self.come_loose();
            flags |= DETACHED;
        }
        let s = &self.s;
        let (dx, dy) = ((px - s.pointer_x).round(), (py - s.pointer_y).round());
        let (b, inset) = (self.base, s.inset);
        let (sw, sh) = (screen_w - s.origin_x, screen_h - s.origin_y);
        let mut top = s.min_top - s.origin_y - inset;
        let mut r = b;
        if s.edges == 0 {
            r.x = clamp(b.x + dx, KEEP - (b.w - inset), sw - KEEP - inset);
            let cx = s.origin_x + r.x + b.w / 2.;
            if let Some(a) = self.areas().iter().find(|a| cx >= a.x && cx < a.x + a.w) {
                top = a.top - s.origin_y - inset;
            }
            r.y = clamp(b.y + dy, top, sh - KEEP - inset);
        } else {
            let (minw, minh) = (s.min_w + 2. * inset, s.min_h + 2. * inset);
            let maxw = if s.max_w > 0. { s.max_w + 2. * inset } else { f64::INFINITY };
            let maxh = if s.max_h > 0. { s.max_h + 2. * inset } else { f64::INFINITY };
            if s.edges & 8 != 0 {
                r.w = clamp(b.w + dx, minw, maxw.min(sw + inset - b.x));
            }
            if s.edges & 4 != 0 {
                r.w = clamp(b.w - dx, minw, maxw.min(b.x + b.w + inset));
                r.x = b.x + b.w - r.w;
            }
            if s.edges & 2 != 0 {
                r.h = clamp(b.h + dy, minh, maxh.min(sh + inset - b.y));
            }
            if s.edges & 1 != 0 {
                r.h = clamp(b.h - dy, minh, maxh.min(b.y + b.h - top));
                r.y = b.y + b.h - r.h;
            }
        }
        if r == self.now {
            return flags;
        }
        let sized = r.w != self.now.w || r.h != self.now.h;
        self.now = r;
        flags | MOVED | if sized { SIZED } else { 0 }
    }
}

// ---- C ABI (server/src/ui/snap_ffi.h) ----

/// A drag's state, or NULL without a start.
#[no_mangle]
pub unsafe extern "C" fn axia_grab_new(start: *const GrabStart) -> *mut Grab {
    match start.as_ref() {
        Some(s) => Box::into_raw(Box::new(Grab::new(*s))),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn axia_grab_free(g: *mut Grab) {
    if !g.is_null() {
        drop(Box::from_raw(g));
    }
}

/// The pointer moved: the flags of `Grab::motion`, and the node in `out` when MOVED is set.
#[no_mangle]
pub unsafe extern "C" fn axia_grab_motion(g: *mut Grab, px: f64, py: f64, screen_w: f64, screen_h: f64, out: *mut SnapRect) -> i32 {
    let Some(g) = g.as_mut() else { return 0 };
    let flags = g.motion(px, py, screen_w, screen_h);
    if let Some(out) = out.as_mut() {
        *out = g.now();
    }
    flags
}

/// 1 while a maximized or snapped window hasn't come loose yet.
#[no_mangle]
pub unsafe extern "C" fn axia_grab_waiting(g: *const Grab) -> i32 {
    g.as_ref().is_some_and(Grab::waiting) as i32
}

/// The node's change since the start in `delta`; returns 1 if it came loose.
#[no_mangle]
pub unsafe extern "C" fn axia_grab_end(g: *const Grab, delta: *mut SnapRect) -> i32 {
    let Some(g) = g.as_ref() else { return 0 };
    if let Some(d) = delta.as_mut() {
        *d = g.delta();
    }
    g.detached() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1280×800 screen with a 28 px menu bar; the node of a 600×400 frame at (100, 100) with 6 px
    /// of borders.
    fn start(edges: i32) -> GrabStart {
        GrabStart {
            node: SnapRect::new(94., 94., 612., 412.),
            pointer_x: 400.,
            pointer_y: 110.,
            edges,
            inset: 6.,
            min_w: 160.,
            min_h: 100.,
            min_top: 28.,
            nareas: 1,
            areas: [GrabArea { x: 0., w: 1280., top: 28. }, GrabArea::default(), GrabArea::default(), GrabArea::default()],
            ..GrabStart::default()
        }
    }

    #[test]
    fn a_move_follows_the_pointer() {
        let mut g = Grab::new(start(0));
        assert_eq!(g.motion(400., 110., 1280., 800.), 0, "no movement yet");
        assert_eq!(g.motion(450.4, 140.6, 1280., 800.), MOVED);
        assert_eq!(g.now(), SnapRect::new(144., 125., 612., 412.));
        assert_eq!(g.delta(), SnapRect::new(50., 31., 0., 0.));
        assert_eq!(g.motion(450.4, 140.6, 1280., 800.), 0, "same place");
    }

    #[test]
    fn the_title_bar_never_goes_under_the_menu_bar() {
        let mut g = Grab::new(start(0));
        g.motion(400., -500., 1280., 800.);
        assert_eq!(g.now().y + 6., 28., "the frame's top at the work area's top");
        // a second display without a menu bar, to the right
        let mut s = start(0);
        s.nareas = 2;
        s.areas[1] = GrabArea { x: 1280., w: 1920., top: 0. };
        let mut g = Grab::new(s);
        g.motion(400. + 1500., -500., 3200., 1080.);
        assert_eq!(g.now().y + 6., 0.);
        // a parent not at the origin
        let mut s = start(0);
        s.origin_y = 10.;
        s.node.y = 84.;
        let mut g = Grab::new(s);
        g.motion(400., -500., 1280., 800.);
        assert_eq!(10. + g.now().y + 6., 28.);
    }

    #[test]
    fn some_of_the_frame_stays_on_screen() {
        let mut g = Grab::new(start(0));
        g.motion(-5000., 5000., 1280., 800.);
        let f = g.now();
        assert_eq!(f.x + f.w - 6., KEEP, "its right edge");
        assert_eq!(f.y + 6., 800. - KEEP, "its top");
        g.motion(5000., 110., 1280., 800.);
        assert_eq!(g.now().x + 6., 1280. - KEEP);
    }

    #[test]
    fn resizing_keeps_the_limits() {
        let mut g = Grab::new(start(8 | 2));
        assert_eq!(g.motion(500., 160., 1280., 800.), MOVED | SIZED);
        assert_eq!(g.now(), SnapRect::new(94., 94., 712., 462.));
        g.motion(-2000., -2000., 1280., 800.);
        assert_eq!((g.now().w, g.now().h), (172., 112.), "the minimum frame and its borders");
        g.motion(5000., 5000., 1280., 800.);
        assert_eq!(g.now().x + g.now().w - 6., 1280., "the right edge at the screen's");
        assert_eq!(g.now().y + g.now().h - 6., 800.);

        let mut s = start(1 | 4);
        s.max_w = 700.;
        let mut g = Grab::new(s);
        g.motion(-2000., -2000., 1280., 800.);
        let f = g.now();
        assert_eq!(f.y + 6., 28., "the top edge stops at the menu bar");
        assert_eq!(f.y + f.h, 94. + 412., "the bottom stays");
        assert_eq!(f.w, 712., "the maximum width");
        assert_eq!(f.x + f.w, 94. + 612., "the right edge stays");
    }

    #[test]
    fn a_maximized_window_comes_loose_at_its_own_size() {
        // maximized, without borders: the frame is the work area, 1280×772 under the menu bar
        let mut s = start(0);
        s.node = SnapRect::new(0., 28., 1280., 772.);
        s.inset = 0.;
        s.pointer_x = 640.;
        s.pointer_y = 40.;
        s.detach_w = 600.;
        s.detach_h = 400.;
        let mut g = Grab::new(s);
        assert!(g.waiting());
        assert_eq!(g.motion(642., 42., 1280., 800.), 0, "within DETACH px");
        assert_eq!(g.now(), s.node);
        assert_eq!(g.motion(644., 44., 1280., 800.), DETACHED | MOVED | SIZED);
        assert!(g.detached() && !g.waiting());
        // the middle of its bar under the pointer, the top where it was (+ the pointer's 4 px)
        assert_eq!(g.now(), SnapRect::new(344., 32., 600., 400.));
        assert_eq!(g.delta(), SnapRect::new(344., 4., -680., -372.));
        // a resize never detaches
        let mut s2 = s;
        s2.edges = 8;
        assert!(!Grab::new(s2).waiting());
    }

    #[test]
    fn crossed_bounds_do_not_panic() {
        // a frame too narrow for KEEP px on a tiny screen: the room for x is crossed (lo > hi)
        let mut s = start(0);
        s.node = SnapRect::new(0., 28., 20., 400.);
        let mut g = Grab::new(s);
        g.motion(900., 500., 50., 800.);
        assert_eq!(g.now().x, 50. - KEEP - 6., "hi, as clampf gave");
        assert_eq!(Grab::new(start(0)).motion(f64::NAN, 1., 1280., 800.), 0);
    }

    #[test]
    fn the_ffi_follows_the_drag() {
        let s = start(0);
        unsafe {
            let g = axia_grab_new(&s);
            let mut out = SnapRect::default();
            assert_eq!(axia_grab_motion(g, 410., 120., 1280., 800., &mut out), MOVED);
            assert_eq!(out, SnapRect::new(104., 104., 612., 412.));
            let mut d = SnapRect::default();
            assert_eq!(axia_grab_end(g, &mut d), 0);
            assert_eq!(d, SnapRect::new(10., 10., 0., 0.));
            assert_eq!(axia_grab_waiting(g), 0);
            axia_grab_free(g);
            assert!(axia_grab_new(std::ptr::null()).is_null());
            assert_eq!(axia_grab_motion(std::ptr::null_mut(), 1., 1., 1., 1., std::ptr::null_mut()), 0);
        }
    }
}
