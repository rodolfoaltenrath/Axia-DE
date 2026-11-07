//! Whether the display driver really waits for vblank before completing a page flip.
//!
//! A flip waits for vblank, half a period on average. A virtual card (vmwgfx, some VMs) completes
//! every flip at once, and then the compositor must pace frames itself or it draws hundreds of
//! frames a second. A real driver also completes a flip early when the frame was submitted just
//! before vblank, which at high refresh rates is common (165 Hz: an eighth of the period is
//! 0.76 ms). So only a long run of early flips counts, and one flip that waited proves there is a
//! vblank and starts the count over: a false start undoes itself on the next real flip.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Consecutive early flips before the compositor paces by itself (about 0.2 s at 165 Hz).
pub const SOFT_AFTER: u32 = 30;

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct FlipPacing {
    early: u32,
}

impl FlipPacing {
    /// Records one completed flip. Returns true when this flip switched software pacing on.
    pub fn observe(&mut self, submit_ns: u64, flip_ns: u64, period_ns: u64) -> bool {
        // The kernel's timestamp is the vblank's; one before the submit is a flip that completed
        // in the vblank it was submitted in: no evidence either way.
        if submit_ns == 0 || flip_ns < submit_ns || period_ns == 0 {
            return false;
        }
        let was = self.soft();
        if flip_ns - submit_ns < period_ns / 8 {
            self.early = (self.early + 1).min(SOFT_AFTER);
        } else {
            self.early = 0;
        }
        !was && self.soft()
    }

    pub fn soft(&self) -> bool {
        self.early >= SOFT_AFTER
    }
}

// axia-server's main loop is single-threaded; atomics keep the statics safe without a lock.
static EARLY: AtomicU32 = AtomicU32::new(0);
static TOLD: AtomicBool = AtomicBool::new(false);

/// One completed flip of the main display. Returns 1 the first time software pacing switches on
/// (the caller logs it once per session), else 0.
#[no_mangle]
pub extern "C" fn axia_flip_observe(submit_ns: u64, flip_ns: u64, period_ns: u64) -> i32 {
    let mut p = FlipPacing { early: EARLY.load(Ordering::Relaxed) };
    let switched = p.observe(submit_ns, flip_ns, period_ns);
    EARLY.store(p.early, Ordering::Relaxed);
    (switched && !TOLD.swap(true, Ordering::Relaxed)) as i32
}

/// 1 while the compositor must pace frames itself (the driver doesn't wait for vblank).
#[no_mangle]
pub extern "C" fn axia_flip_soft() -> i32 {
    (FlipPacing { early: EARLY.load(Ordering::Relaxed) }).soft() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    const P165: u64 = 1_000_000_000 / 165;

    #[test]
    fn virtual_card_switches_to_software_pacing() {
        let mut p = FlipPacing::default();
        let mut switched = 0;
        for i in 0..SOFT_AFTER + 10 {
            let t = 1_000_000 + u64::from(i) * P165;
            switched += p.observe(t, t + 50_000, P165) as u32;
        }
        assert!(p.soft());
        assert_eq!(switched, 1);
    }

    #[test]
    fn late_submits_at_165hz_never_switch_while_some_flips_wait() {
        // A real amdgpu at 165 Hz: frames that take 5.5 ms complete their flip 0.5 ms later
        // (counted as early), but every few frames one waits for vblank.
        let mut p = FlipPacing::default();
        for i in 0..10_000u64 {
            let t = 1_000_000 + i * P165;
            let wait = if i % 20 == 19 { P165 / 2 } else { 500_000 };
            assert!(!p.observe(t, t + wait, P165));
        }
        assert!(!p.soft());
    }

    #[test]
    fn one_waiting_flip_leaves_software_pacing() {
        let mut p = FlipPacing::default();
        for i in 0..SOFT_AFTER {
            let t = u64::from(i + 1) * P165;
            p.observe(t, t + 10_000, P165);
        }
        assert!(p.soft());
        p.observe(10 * P165, 10 * P165 + P165 / 2, P165);
        assert!(!p.soft());
    }

    #[test]
    fn flips_without_evidence_are_ignored() {
        let mut p = FlipPacing::default();
        assert!(!p.observe(0, 5, P165));
        assert!(!p.observe(100, 50, P165));
        assert!(!p.observe(100, 101, 0));
        assert_eq!(p, FlipPacing::default());
    }
}
