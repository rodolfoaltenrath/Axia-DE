//! Native animations of a node's paint-only properties: scale, opacity and transform origin.
//!
//! The shell asks once ("shrink this window towards the Dock in 260 ms"); every frame the compositor
//! steps the animator and writes the values straight into the node's style, without a React render.
//! Only properties that skip layout are animated, so each frame is a repaint of the node's cached
//! raster (server/src/ui/node.c `transform_only`), never a relayout of the window and its app.
//!
//! Ownership: the main loop's thread only (the C side calls in from axia-server's single UI thread).
use std::cell::RefCell;

/// Values written into a node. `origin_*` are pixels from the node's border box top-left corner;
/// `origin_set == 0` means the default origin (the centre).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimProps {
    pub scale_x: f32,
    pub scale_y: f32,
    pub opacity: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub translate_x: f32,
    pub translate_y: f32,
    pub origin_set: u8,
}

impl Default for AnimProps {
    fn default() -> Self {
        Self { scale_x: 1., scale_y: 1., opacity: 1., origin_x: 0., origin_y: 0., translate_x: 0., translate_y: 0., origin_set: 0 }
    }
}

/// One node's values for this frame. `done` is set on the last frame of its animation.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct AnimFrame {
    pub node: i32,
    pub id: u32,
    pub props: AnimProps,
    pub done: u8,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Easing {
    Linear = 0,
    /// Quadratic ease-in: slow start, the window "falls" into the Dock.
    In = 1,
    /// Cubic ease-out: fast start, gentle stop (windows appearing, desktops sliding).
    Out = 2,
    /// Quadratic ease-out, used by the Dock sliding at the display edge.
    OutQuad = 3,
}

impl Easing {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::In,
            2 => Self::Out,
            3 => Self::OutQuad,
            _ => Self::Linear,
        }
    }
    fn apply(self, p: f32) -> f32 {
        let p = p.clamp(0., 1.);
        match self {
            Self::Linear => p,
            Self::In => p * p,
            Self::Out => 1. - (1. - p).powi(3),
            Self::OutQuad => 1. - (1. - p).powi(2),
        }
    }
}

/// How long minimizing takes, and how small and faint the window gets at the Dock.
pub const MINIMIZE_MS: f64 = 260.;
const MINIMIZE_SCALE: f32 = 0.1;
const MINIMIZE_OPACITY: f32 = 0.3;

struct Anim {
    id: u32,
    node: i32,
    t0: f64,
    ms: f64,
    easing: Easing,
    from: AnimProps,
    to: AnimProps,
}

impl Anim {
    fn sample(&self, now: f64) -> (AnimProps, bool) {
        let a = self;
        let p = if a.ms <= 0. { 1. } else { ((now - a.t0) / a.ms) as f32 };
        let done = p >= 1.;
        let e = a.easing.apply(p);
        let props = AnimProps {
            scale_x: lerp(a.from.scale_x, a.to.scale_x, e),
            scale_y: lerp(a.from.scale_y, a.to.scale_y, e),
            opacity: lerp(a.from.opacity, a.to.opacity, e).clamp(0., 1.),
            origin_x: lerp(a.from.origin_x, a.to.origin_x, e),
            origin_y: lerp(a.from.origin_y, a.to.origin_y, e),
            translate_x: lerp(a.from.translate_x, a.to.translate_x, e),
            translate_y: lerp(a.from.translate_y, a.to.translate_y, e),
            origin_set: a.from.origin_set | a.to.origin_set,
        };
        (props, done)
    }
}

#[derive(Default)]
pub struct Animator {
    next: u32,
    active: Vec<Anim>,
    /// A completion callback may enqueue React work after the loop read its JS timer deadline.
    completion_ready: bool,
    /// What each animated node had before its first animation, restored by `clear`.
    base: Vec<(i32, AnimProps)>,
}

fn lerp(a: f32, b: f32, e: f32) -> f32 {
    a + (b - a) * e
}

impl Animator {
    /// Starts an animation of `node`, replacing one already running on it. `base` is what the node
    /// shows now; the first animation's base is kept until `clear`.
    pub fn start(
        &mut self,
        node: i32,
        now: f64,
        ms: f64,
        easing: Easing,
        from: AnimProps,
        to: AnimProps,
        base: AnimProps,
    ) -> u32 {
        if node <= 0 {
            return 0;
        }
        self.active.retain(|a| a.node != node);
        if !self.base.iter().any(|(n, _)| *n == node) {
            self.base.push((node, base));
        }
        self.next = self.next.wrapping_add(1).max(1);
        let ms = if ms.is_finite() && ms > 0. { ms.min(10_000.) } else { 0. };
        self.active.push(Anim { id: self.next, node, t0: now, ms, easing, from, to });
        self.next
    }

    /// The values of up to `max` running animations at `now`; finished ones are reported once,
    /// with `done`, at their end values, and stop running (the values stay until `clear`).
    pub fn step(&mut self, now: f64, max: usize) -> Vec<AnimFrame> {
        let mut out = Vec::new();
        let mut finished = Vec::new();
        for a in self.active.iter().take(max) {
            let (props, done) = a.sample(now);
            out.push(AnimFrame { node: a.node, id: a.id, props, done: done as u8 });
            if done {
                finished.push(a.id);
            }
        }
        self.completion_ready |= !finished.is_empty();
        self.active.retain(|a| !finished.contains(&a.id));
        out
    }

    /// Slide the Dock without re-entering React per frame. A reversal starts exactly where
    /// the previous animation is at `now`, and always targets the original unshifted style.
    pub fn dock_visibility(&mut self, node: i32, now: f64, edge: u8, distance: f32,
        shown: bool, immediate: bool, current: AnimProps) -> Option<(u32, AnimProps)> {
        if node <= 0 || edge > 2 || !distance.is_finite() || distance < 0. || !now.is_finite() {
            return None;
        }
        let base = self.base.iter().find(|(n, _)| *n == node).map(|(_, p)| *p).unwrap_or(current);
        let mut to = base;
        if !shown {
            match edge {
                0 => to.translate_y += distance, // bottom
                1 => to.translate_x -= distance, // left
                _ => to.translate_x += distance, // right
            }
        }
        let from = if immediate { to } else {
            self.active.iter().find(|a| a.node == node).map(|a| a.sample(now).0).unwrap_or(current)
        };
        let id = self.start(node, now, if immediate { 0. } else { DOCK_HIDE_MS }, Easing::OutQuad, from, to, base);
        Some((id, from))
    }

    /// Completion notices run JS during painting, after the event loop last queried timers.
    /// Wake once immediately so any newly scheduled UI work is processed before an older timer.
    pub fn timeout(&mut self, now: f64, next_frame: f64, software_paced: bool,
        waiting_for_flip: bool, refresh_hz: u32, js_frames: bool) -> f64 {
        if std::mem::take(&mut self.completion_ready) { return 0.; }
        frame_wake_delay(self.active() || js_frames, software_paced, waiting_for_flip, now, next_frame, refresh_hz)
    }

    /// Stops `node`'s animation and returns what it showed before it, to be written back.
    pub fn clear(&mut self, node: i32) -> Option<AnimProps> {
        self.active.retain(|a| a.node != node);
        let i = self.base.iter().position(|(n, _)| *n == node)?;
        Some(self.base.swap_remove(i).1)
    }

    /// The node is gone (its id may be reused): drop everything about it.
    pub fn forget(&mut self, node: i32) {
        self.active.retain(|a| a.node != node);
        self.base.retain(|(n, _)| *n != node);
    }

    pub fn active(&self) -> bool {
        !self.active.is_empty()
    }
}

/// Minimizing: the window shrinks towards the Dock while fading. Translating by `e·(dock − centre)`
/// while scaling by `1 − 0.9·e` around the centre is the same as scaling around one fixed point,
/// `centre + (dock − centre) / 0.9`, so the whole effect is a scale: no translate, no relayout.
pub fn minimize_props(x: f32, y: f32, w: f32, h: f32, dock_x: f32, dock_y: f32, base: AnimProps) -> (AnimProps, AnimProps) {
    let k = 1. - MINIMIZE_SCALE;
    let (cx, cy) = (x + w / 2., y + h / 2.);
    let origin_x = cx + (dock_x - cx) / k - x;
    let origin_y = cy + (dock_y - cy) / k - y;
    let from = AnimProps { scale_x: base.scale_x, scale_y: base.scale_y, opacity: base.opacity, origin_x, origin_y, origin_set: 1, ..base };
    let to = AnimProps {
        scale_x: base.scale_x * MINIMIZE_SCALE,
        scale_y: base.scale_y * MINIMIZE_SCALE,
        opacity: base.opacity * MINIMIZE_OPACITY,
        ..from
    };
    (from, to)
}

/// Switching desktops: the windows that appear slide in from the side of the move while fading in.
pub const DESKTOP_SLIDE_MS: f64 = 170.;
const DESKTOP_SLIDE_PX: f32 = 56.;

/// `step` +1 moves to the next desktop (the windows come from the right; from the left in RTL).
pub fn slide_props(step: i32, rtl: bool, base: AnimProps) -> (AnimProps, AnimProps) {
    let dx = step.signum() as f32 * DESKTOP_SLIDE_PX * if rtl { -1. } else { 1. };
    (AnimProps { translate_x: base.translate_x + dx, opacity: 0., ..base }, base)
}

/// Dock auto-hide and removal timings. Geometry is computed here, never in a JS frame loop.
pub const DOCK_HIDE_MS: f64 = 180.;
pub const DOCK_POOF_MS: f64 = 320.;

/// Removal effect: part 0 is the icon; parts 1..=5 are expanding puffs around it.
/// Each node has a fixed `size` square centred on the removal point. Only its cached
/// raster's scale, translation and opacity change, so neither icons nor circles relayout.
pub fn dock_poof_props(size: f32, part: u8, base: AnimProps) -> Option<(AnimProps, AnimProps)> {
    if !size.is_finite() || size <= 0. || part > 5 { return None; }
    if part == 0 {
        return Some((base, AnimProps { scale_x: base.scale_x * 0.4,
            scale_y: base.scale_y * 0.4, opacity: 0., ..base }));
    }
    let angle = f32::from(part - 1) * std::f32::consts::TAU / 5.;
    let (dy, dx) = angle.sin_cos();
    let from = AnimProps { scale_x: base.scale_x * 0.22, scale_y: base.scale_y * 0.22,
        opacity: base.opacity * 0.85, translate_x: base.translate_x + dx * size * 0.25,
        translate_y: base.translate_y + dy * size * 0.25, ..base };
    let to = AnimProps { scale_x: base.scale_x * 0.42, scale_y: base.scale_y * 0.42,
        opacity: 0., translate_x: base.translate_x + dx * size,
        translate_y: base.translate_y + dy * size, ..base };
    Some((from, to))
}

thread_local! { static ANIMATOR: RefCell<Animator> = RefCell::new(Animator::default()); }

unsafe fn props(p: *const AnimProps) -> AnimProps {
    if p.is_null() { AnimProps::default() } else { *p }
}

#[no_mangle]
pub unsafe extern "C" fn axia_anim_start(
    node: i32,
    now: f64,
    ms: f64,
    easing: u8,
    from: *const AnimProps,
    to: *const AnimProps,
    base: *const AnimProps,
) -> u32 {
    let (from, to, base) = (props(from), props(to), props(base));
    ANIMATOR.with(|a| a.borrow_mut().start(node, now, ms, Easing::from_u8(easing), from, to, base))
}

/// Minimizes the node whose border box is (x, y, w, h) on screen towards (dock_x, dock_y).
#[no_mangle]
pub unsafe extern "C" fn axia_anim_minimize(
    node: i32,
    now: f64,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    dock_x: f32,
    dock_y: f32,
    base: *const AnimProps,
) -> u32 {
    if !(w > 0. && h > 0.) {
        return 0;
    }
    let base = props(base);
    let (from, to) = minimize_props(x, y, w, h, dock_x, dock_y, base);
    ANIMATOR.with(|a| a.borrow_mut().start(node, now, MINIMIZE_MS, Easing::In, from, to, base))
}

/// Slides the node in for a desktop switch (`step` ±1, `rtl` the layout direction).
#[no_mangle]
pub unsafe extern "C" fn axia_anim_slide(node: i32, now: f64, step: i32, rtl: bool, base: *const AnimProps) -> u32 {
    if step == 0 {
        return 0;
    }
    let base = props(base);
    let (from, to) = slide_props(step, rtl, base);
    ANIMATOR.with(|a| a.borrow_mut().start(node, now, DESKTOP_SLIDE_MS, Easing::Out, from, to, base))
}

/// The Dock's visibility target. Writes the initial frame for immediate application by C.
#[no_mangle]
pub unsafe extern "C" fn axia_anim_dock_visibility(node: i32, now: f64, edge: u8, distance: f32,
    shown: bool, immediate: bool, current: *const AnimProps, initial: *mut AnimProps) -> u32 {
    let result = ANIMATOR.with(|a| a.borrow_mut().dock_visibility(node, now, edge, distance, shown, immediate, props(current)));
    match result {
        Some((id, from)) => { if !initial.is_null() { *initial = from; } id }
        None => 0,
    }
}

/// One fixed-raster part of the Dock's removal effect.
#[no_mangle]
pub unsafe extern "C" fn axia_anim_dock_poof(node: i32, now: f64, size: f32, part: u8,
    base: *const AnimProps, initial: *mut AnimProps) -> u32 {
    if node <= 0 || !now.is_finite() { return 0; }
    let base = props(base);
    let Some((from, to)) = dock_poof_props(size, part, base) else { return 0; };
    if !initial.is_null() { *initial = from; }
    ANIMATOR.with(|a| a.borrow_mut().start(node, now, DOCK_POOF_MS, Easing::Linear, from, to, base))
}

/// Headless/virtually paced outputs have no hardware vblank to wake the poll loop.
/// KMS normally wakes on a pending page flip; if nothing was submitted (for example an
/// off-screen effect), a deadline still lets that animation finish and release its callback.
pub fn frame_wake_delay(active: bool, software_paced: bool, waiting_for_flip: bool,
    now: f64, next_frame: f64, refresh_hz: u32) -> f64 {
    if !active || !now.is_finite() { return -1.; }
    if software_paced {
        return if next_frame.is_finite() { (next_frame - now).max(0.) } else { -1. };
    }
    if waiting_for_flip { return -1.; }
    let hz = if refresh_hz == 0 { 60 } else { refresh_hz.min(1000) };
    1000. / f64::from(hz)
}

#[no_mangle]
pub extern "C" fn axia_anim_timeout(now: f64, next_frame: f64, software_paced: bool,
    waiting_for_flip: bool, refresh_hz: u32, js_frames: bool) -> f64 {
    ANIMATOR.with(|a| a.borrow_mut().timeout(now, next_frame, software_paced, waiting_for_flip, refresh_hz, js_frames))
}

/// Fills `out` with up to `max` frames; returns how many.
#[no_mangle]
pub unsafe extern "C" fn axia_anim_step(now: f64, out: *mut AnimFrame, max: usize) -> usize {
    if out.is_null() || max == 0 {
        return 0;
    }
    let frames = ANIMATOR.with(|a| a.borrow_mut().step(now, max));
    for (i, f) in frames.iter().enumerate() {
        *out.add(i) = *f;
    }
    frames.len()
}

/// 1 and the node's values from before its animation in `out`, or 0 if it had none.
#[no_mangle]
pub unsafe extern "C" fn axia_anim_clear(node: i32, out: *mut AnimProps) -> i32 {
    match ANIMATOR.with(|a| a.borrow_mut().clear(node)) {
        Some(p) if !out.is_null() => {
            *out = p;
            1
        }
        Some(_) => 1,
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn axia_anim_forget(node: i32) {
    ANIMATOR.with(|a| a.borrow_mut().forget(node));
}

#[no_mangle]
pub extern "C" fn axia_anim_active() -> i32 {
    ANIMATOR.with(|a| a.borrow().active()) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// The point (px, py) of the node, after scaling by s around the origin, on screen.
    fn mapped(x: f32, y: f32, p: &AnimProps, px: f32, py: f32) -> (f32, f32) {
        let (ox, oy) = (x + p.origin_x, y + p.origin_y);
        (ox + (px - ox) * p.scale_x, oy + (py - oy) * p.scale_y)
    }

    #[test]
    fn minimize_matches_the_old_translate_and_scale_effect() {
        let (x, y, w, h) = (300., 200., 800., 600.);
        let (dx, dy) = (960., 1044.);
        let mut a = Animator::default();
        let (from, to) = minimize_props(x, y, w, h, dx, dy, AnimProps::default());
        a.start(7, 0., MINIMIZE_MS, Easing::In, from, to, AnimProps::default());
        for ms in [0., 65., 130., 195., 260.] {
            let f = a.step(ms, 8);
            let p = f[0].props;
            // the old effect (themes/default): e = t², translate e·(dock − centre), scale 1 − 0.9e
            let t = (ms / MINIMIZE_MS) as f32;
            let e = t * t;
            let (cx, cy) = (x + w / 2., y + h / 2.);
            let want = (cx + (dx - cx) * e, cy + (dy - cy) * e);
            let got = mapped(x, y, &p, cx, cy);
            assert!(close(got.0, want.0) && close(got.1, want.1), "{ms} ms: {got:?} vs {want:?}");
            assert!(close(p.scale_x, 1. - 0.9 * e));
            assert!(close(p.opacity, 1. - 0.7 * e));
            assert_eq!(f[0].done, (ms >= MINIMIZE_MS) as u8);
        }
        assert!(!a.active());
        assert!(a.step(300., 8).is_empty());
    }

    #[test]
    fn clear_restores_the_values_from_before_the_first_animation() {
        let mut a = Animator::default();
        let base = AnimProps { opacity: 0.8, ..AnimProps::default() };
        let (from, to) = minimize_props(0., 0., 100., 100., 50., 500., base);
        a.start(3, 0., 100., Easing::Linear, from, to, base);
        // a second animation of the same node replaces the first and keeps the original base
        a.start(3, 50., 100., Easing::Linear, to, from, to);
        assert_eq!(a.step(60., 8).len(), 1);
        assert_eq!(a.clear(3), Some(base));
        assert!(!a.active());
        assert_eq!(a.clear(3), None);
    }

    #[test]
    fn forget_drops_a_destroyed_node_and_bad_input_is_ignored() {
        let mut a = Animator::default();
        let p = AnimProps::default();
        assert_eq!(a.start(0, 0., 100., Easing::Linear, p, p, p), 0);
        let id = a.start(9, 0., f64::NAN, Easing::Linear, p, p, p);
        assert!(id > 0);
        let f = a.step(0., 8);
        assert_eq!(f[0].done, 1, "a non-finite duration ends at once");
        a.start(9, 0., 100., Easing::Out, p, p, p);
        a.forget(9);
        assert!(!a.active());
        assert_eq!(a.clear(9), None);
    }

    #[test]
    fn step_respects_max_and_keeps_the_rest_running() {
        let mut a = Animator::default();
        let p = AnimProps::default();
        for n in 1..=5 {
            a.start(n, 0., 100., Easing::Linear, p, p, p);
        }
        assert_eq!(a.step(10., 2).len(), 2);
        assert_eq!(a.step(10., 64).len(), 5);
        assert!(a.active());
    }

    #[test]
    fn desktop_slide_matches_the_old_effect_and_ends_where_it_was() {
        let base = AnimProps { translate_x: 4., ..AnimProps::default() };
        let mut a = Animator::default();
        let (from, to) = slide_props(1, false, base);
        a.start(5, 0., DESKTOP_SLIDE_MS, Easing::Out, from, to, base);
        for ms in [0., 42.5, 85., 127.5, 170.] {
            let p = a.step(ms, 8)[0].props;
            // the old effect (sdk/desktops.js): e = 1 − (1 − t)³, translateX dx·(1 − e), opacity e
            let t = (ms / DESKTOP_SLIDE_MS) as f32;
            let e = 1. - (1. - t).powi(3);
            assert!(close(p.translate_x, 4. + 56. * (1. - e)) && close(p.opacity, e), "{ms} ms: {p:?}");
            assert_eq!((p.scale_x, p.origin_set), (1., 0), "nothing else moves");
        }
        assert_eq!(slide_props(-1, false, base).0.translate_x, 4. - 56.);
        assert_eq!(slide_props(1, true, base).0.translate_x, 4. - 56., "right to left");
        assert_eq!(a.clear(5), Some(base));
    }

    #[test]
    fn dock_visibility_reverses_between_frames_without_jumping() {
        let mut a = Animator::default();
        let base = AnimProps { translate_x: 4., translate_y: 7., opacity: 0.8, ..AnimProps::default() };
        let (_, initial) = a.dock_visibility(11, 0., 0, 80., false, false, base).unwrap();
        assert_eq!(initial, base);
        let last_frame = a.step(30., 64)[0].props;
        let (_, reversal) = a.dock_visibility(11, 90., 0, 80., true, false, last_frame).unwrap();
        // Halfway through quadratic ease-out: 75% hidden, even if the last frame was earlier.
        assert!(close(reversal.translate_y, 7. + 60.));
        assert_eq!(a.step(90., 64)[0].props, reversal);
        let end = a.step(270., 64)[0];
        assert_eq!(end.props, base);
        assert_eq!(end.done, 1);
        assert!(!a.active());
        assert_eq!(a.clear(11), Some(base));
    }

    #[test]
    fn dock_visibility_edges_reduced_motion_and_configuration_changes() {
        for (edge, x, y) in [(0, 0., 80.), (1, -80., 0.), (2, 80., 0.)] {
            let mut a = Animator::default();
            let base = AnimProps::default();
            let (_, initial) = a.dock_visibility(4, 0., edge, 80., false, true, base).unwrap();
            assert_eq!((initial.translate_x, initial.translate_y), (x, y));
            let f = a.step(0., 64)[0];
            assert_eq!(f.done, 1, "initial hidden state and reduced motion need no timed frames");
            assert_eq!(f.props, initial);
            // Changing edge/size must target the original style rather than accumulating offsets.
            a.dock_visibility(4, 1., 2, 100., false, true, initial).unwrap();
            let new = a.step(1., 64)[0].props;
            assert_eq!((new.translate_x, new.translate_y), (100., 0.));
            a.dock_visibility(4, 2., edge, 100., true, true, new).unwrap();
            assert_eq!(a.step(2., 64)[0].props, base);
            assert_eq!(a.clear(4), Some(base));
        }
    }

    #[test]
    fn invalid_dock_requests_do_not_interrupt_an_animation() {
        let mut a = Animator::default();
        let p = AnimProps::default();
        let (id, _) = a.dock_visibility(4, 0., 0, 80., false, false, p).unwrap();
        for (node, now, edge, distance) in [(0, 0., 0, 80.), (4, f64::NAN, 0, 80.),
            (4, 0., 3, 80.), (4, 0., 0, -1.), (4, 0., 0, f32::INFINITY)] {
            assert!(a.dock_visibility(node, now, edge, distance, false, false, p).is_none());
        }
        assert_eq!(a.step(180., 64)[0].id, id);
        for (size, part) in [(0., 0), (f32::NAN, 1), (54., 6)] {
            assert!(dock_poof_props(size, part, p).is_none());
        }
    }

    #[test]
    fn dock_removal_has_linear_fixed_raster_trajectories_and_finishes() {
        let size = 60.;
        let base = AnimProps { opacity: 0.8, translate_x: 7., translate_y: 9., ..AnimProps::default() };
        let mut a = Animator::default();
        for part in 0..=5 {
            let (from, to) = dock_poof_props(size, part, base).unwrap();
            a.start(i32::from(part) + 1, 0., DOCK_POOF_MS, Easing::Linear, from, to, base);
        }
        let frames = a.step(160., 64);
        assert_eq!(frames.len(), 6);
        assert!(close(frames[0].props.scale_x, 0.7));
        assert!(close(frames[0].props.opacity, 0.4));
        for (i, f) in frames.iter().enumerate().skip(1) {
            let p = f.props;
            let angle = (i - 1) as f32 * std::f32::consts::TAU / 5.;
            assert!(close(p.translate_x, 7. + angle.cos() * size * 0.625));
            assert!(close(p.translate_y, 9. + angle.sin() * size * 0.625));
            assert!(close(p.scale_x, 0.32) && close(p.scale_y, 0.32));
            assert!(close(p.opacity, 0.34));
        }
        let last = a.step(320., 64);
        assert!(last.iter().all(|f| f.done == 1 && f.props.opacity == 0.));
        assert!(!a.active());
        assert!(a.step(400., 64).is_empty());
        for node in 1..=6 { a.forget(node); assert_eq!(a.clear(node), None); }
    }

    #[test]
    fn completion_wakes_new_js_work_even_after_the_last_animation() {
        let mut a = Animator::default();
        let p = AnimProps::default();
        a.start(9, 0., 100., Easing::Linear, p, p, p);
        assert_eq!(a.timeout(0., 16., true, false, 60, false), 16.);
        assert_eq!(a.step(100., 64)[0].done, 1);
        assert!(!a.active());
        assert_eq!(a.timeout(100., 116., true, false, 60, false), 0., "completion scheduled JS during painting");
        assert_eq!(a.timeout(100., 116., true, false, 60, false), -1., "only one wake, never idle polling");
        a.start(10, 100., 0., Easing::Linear, p, p, p);
        a.step(100., 64);
        assert_eq!(a.timeout(100., 0., false, true, 165, false), 0., "KMS completion also wakes pending JS");
        assert_eq!(a.timeout(100., 0., false, false, 165, false), -1.);
    }

    #[test]
    fn native_frames_wake_software_pacing_without_js_timers() {
        assert_eq!(frame_wake_delay(true, true, false, 100., 116., 60), 16.);
        assert_eq!(frame_wake_delay(true, true, false, 120., 116., 60), 0.);
        assert_eq!(frame_wake_delay(false, true, false, 100., 116., 60), -1., "no idle polling");
        assert_eq!(frame_wake_delay(true, false, true, 100., 116., 165), -1., "KMS supplies vblank");
        assert_eq!(frame_wake_delay(true, false, false, 100., 0., 165), 1000. / 165., "no submitted flip: animations still complete");
        assert_eq!(frame_wake_delay(false, false, false, 100., 0., 165), -1., "no idle wake-ups");
        assert_eq!(frame_wake_delay(true, false, false, 100., 0., 0), 1000. / 60., "unknown refresh");
        assert_eq!(frame_wake_delay(true, true, false, f64::NAN, 116., 60), -1.);
        assert_eq!(frame_wake_delay(true, true, false, 100., f64::INFINITY, 60), -1.);
    }

    #[test]
    fn easings_start_at_zero_and_end_at_one() {
        for e in [Easing::Linear, Easing::In, Easing::Out, Easing::OutQuad] {
            assert!(close(e.apply(0.), 0.) && close(e.apply(1.), 1.) && close(e.apply(2.), 1.));
        }
        assert!(Easing::In.apply(0.5) < 0.5 && Easing::Out.apply(0.5) > 0.5);
    }
}
