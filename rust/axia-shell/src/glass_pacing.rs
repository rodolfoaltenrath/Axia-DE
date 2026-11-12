//! How often the glass over app windows is refreshed for the apps' own frames (server/src/ui/paint.c,
//! `paint_external_glass_damage`).
//!
//! A frosted panel (the menu bar, the Dock, an open panel) blurs what is below it, app windows
//! included. When an app draws a new frame near one, the panel is painted again on the CPU: its box
//! and the margin it samples are cleared and repainted, the app pixels behind it are read back from
//! the GPU, blurred and scaled up. A profile of a desktop with a browser and an editor open had a
//! quarter of the compositor's main thread there, once per app frame: a video or a scrolling page
//! next to the menu bar repainted it 60 to 165 times a second, for a tint that moves with the app.
//!
//! Those refreshes are now at most `MIN_INTERVAL_MS` apart. The first one after a quiet moment is
//! immediate; while an app keeps drawing, the boxes wait and go together in the next refresh, which
//! the main loop wakes up for. Changes of the shell itself (the panel opening, its own content) don't
//! come through here and stay immediate.
use std::cell::RefCell;

/// 30 refreshes a second at most.
pub const MIN_INTERVAL_MS: f64 = 1000. / 30.;
const EPSILON: f64 = 1e-6;

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct Pacing {
    last: Option<f64>,
    pending: bool,
}

impl Pacing {
    /// An app frame touched some glass at `now`: true to refresh it now, false to keep it for later.
    pub fn request(&mut self, now: f64) -> bool {
        if !now.is_finite() {
            return true;
        }
        match self.last {
            Some(l) if now - l < MIN_INTERVAL_MS && now >= l => {
                self.pending = true;
                false
            }
            _ => {
                self.last = Some(now);
                self.pending = false;
                true
            }
        }
    }

    /// True once the boxes kept for later are due (they are refreshed now).
    pub fn tick(&mut self, now: f64) -> bool {
        if !self.pending || !now.is_finite() {
            return false;
        }
        match self.last {
            // EPSILON: waking exactly at `timeout` must count as due, whatever the rounding of
            // last + interval - now + now
            Some(l) if now >= l && now - l < MIN_INTERVAL_MS - EPSILON => false,
            _ => {
                self.last = Some(now);
                self.pending = false;
                true
            }
        }
    }

    /// Milliseconds until `tick` is due, or -1 with nothing waiting.
    pub fn timeout(&self, now: f64) -> f64 {
        match (self.pending, self.last) {
            (true, Some(l)) if now.is_finite() => (l + MIN_INTERVAL_MS - now).clamp(0., MIN_INTERVAL_MS),
            (true, _) => 0.,
            _ => -1.,
        }
    }
}

thread_local! {
    static PACING: RefCell<Pacing> = RefCell::new(Pacing::default());
}

/// UI thread. 1: refresh the glass now; 0: it is kept until `axia_glass_pacing_tick`.
#[no_mangle]
pub extern "C" fn axia_glass_pacing_request(now_ms: f64) -> i32 {
    PACING.with(|p| p.borrow_mut().request(now_ms)) as i32
}

/// UI thread, every loop. 1: the kept glass is due now.
#[no_mangle]
pub extern "C" fn axia_glass_pacing_tick(now_ms: f64) -> i32 {
    PACING.with(|p| p.borrow_mut().tick(now_ms)) as i32
}

/// Milliseconds until the kept glass is due; -1 if none is kept.
#[no_mangle]
pub extern "C" fn axia_glass_pacing_timeout(now_ms: f64) -> f64 {
    PACING.with(|p| p.borrow().timeout(now_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_frame_is_immediate_and_a_stream_is_paced() {
        let mut p = Pacing::default();
        assert!(p.request(1000.));
        assert_eq!(p.timeout(1000.), -1.);
        // an app drawing at 165 Hz, and the main loop also waking when `timeout` says
        let mut refreshed = 1;
        let (mut t, mut frame) = (1000., 1000.);
        let mut steps = 0;
        while t < 2000. {
            steps += 1;
            assert!(steps < 10_000, "the loop must always move on");
            let due = p.timeout(t);
            let next_frame = frame + 1000. / 165.;
            if due >= 0. && t + due < next_frame {
                t += due;
            } else {
                t = next_frame;
                frame = next_frame;
                if p.request(t) {
                    refreshed += 1;
                }
            }
            if p.tick(t) {
                refreshed += 1;
            }
        }
        assert!((29..=31).contains(&refreshed), "{refreshed} refreshes in a second");
    }

    #[test]
    fn what_waits_is_never_lost() {
        let mut p = Pacing::default();
        assert!(p.request(0.));
        assert!(!p.request(5.));
        assert!((p.timeout(5.) - (MIN_INTERVAL_MS - 5.)).abs() < 1e-9);
        assert!(!p.tick(20.));
        assert!(p.tick(MIN_INTERVAL_MS));
        assert!(!p.tick(MIN_INTERVAL_MS + 1.), "once");
        assert_eq!(p.timeout(40.), -1.);
        // a quiet moment, then an app frame: immediate again
        assert!(p.request(500.));
    }

    #[test]
    fn odd_clocks_never_hold_glass_forever() {
        let mut p = Pacing::default();
        assert!(p.request(f64::NAN));
        p.request(100.);
        // the clock went back: refresh rather than wait for it to catch up
        assert!(p.request(50.));
        p.request(60.);
        assert!(p.timeout(f64::NAN) == 0.);
        assert!(p.tick(10.), "a clock before the last refresh: due");
    }
}
