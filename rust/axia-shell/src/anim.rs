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
    pub origin_set: u8,
}

impl Default for AnimProps {
    fn default() -> Self {
        Self { scale_x: 1., scale_y: 1., opacity: 1., origin_x: 0., origin_y: 0., origin_set: 0 }
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
}

impl Easing {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::In,
            2 => Self::Out,
            _ => Self::Linear,
        }
    }
    fn apply(self, p: f32) -> f32 {
        let p = p.clamp(0., 1.);
        match self {
            Self::Linear => p,
            Self::In => p * p,
            Self::Out => 1. - (1. - p).powi(3),
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

#[derive(Default)]
pub struct Animator {
    next: u32,
    active: Vec<Anim>,
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
            let p = if a.ms <= 0. { 1. } else { ((now - a.t0) / a.ms) as f32 };
            let done = p >= 1.;
            let e = a.easing.apply(p);
            let props = AnimProps {
                scale_x: lerp(a.from.scale_x, a.to.scale_x, e),
                scale_y: lerp(a.from.scale_y, a.to.scale_y, e),
                opacity: lerp(a.from.opacity, a.to.opacity, e).clamp(0., 1.),
                origin_x: lerp(a.from.origin_x, a.to.origin_x, e),
                origin_y: lerp(a.from.origin_y, a.to.origin_y, e),
                origin_set: a.from.origin_set | a.to.origin_set,
            };
            out.push(AnimFrame { node: a.node, id: a.id, props, done: done as u8 });
            if done {
                finished.push(a.id);
            }
        }
        self.active.retain(|a| !finished.contains(&a.id));
        out
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
    let from = AnimProps { scale_x: base.scale_x, scale_y: base.scale_y, opacity: base.opacity, origin_x, origin_y, origin_set: 1 };
    let to = AnimProps {
        scale_x: base.scale_x * MINIMIZE_SCALE,
        scale_y: base.scale_y * MINIMIZE_SCALE,
        opacity: base.opacity * MINIMIZE_OPACITY,
        ..from
    };
    (from, to)
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
    fn easings_start_at_zero_and_end_at_one() {
        for e in [Easing::Linear, Easing::In, Easing::Out] {
            assert!(close(e.apply(0.), 0.) && close(e.apply(1.), 1.) && close(e.apply(2.), 1.));
        }
        assert!(Easing::In.apply(0.5) < 0.5 && Easing::Out.apply(0.5) > 0.5);
    }
}
