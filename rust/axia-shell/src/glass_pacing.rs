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
//!
//! The hardware cursor is paced the same way (`axia_cursor_pacing_*`). Every report of the mouse
//! moved the cursor plane with an ioctl: a 1000 Hz mouse made a thousand of them a second, half a
//! second of the main thread in the kernel for thirty seconds of use, while the plane only takes a new
//! position at the display's next vblank. The first move after a pause is immediate; the following
//! ones wait at most half a frame and go together, with the latest position.
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
    /// Work came at `now`: true to do it now, false to keep it for `tick`, with `interval` ms at
    /// least between two of them.
    pub fn request_every(&mut self, now: f64, interval: f64) -> bool {
        if !now.is_finite() {
            return true;
        }
        match self.last {
            Some(l) if now - l < interval && now >= l => {
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

    /// True once what was kept is due (it is done now).
    pub fn tick_every(&mut self, now: f64, interval: f64) -> bool {
        if !self.pending || !now.is_finite() {
            return false;
        }
        match self.last {
            // EPSILON: waking exactly at `timeout` must count as due, whatever the rounding of
            // last + interval - now + now
            Some(l) if now >= l && now - l < interval - EPSILON => false,
            _ => {
                self.last = Some(now);
                self.pending = false;
                true
            }
        }
    }

    /// Milliseconds until `tick` is due, or -1 with nothing waiting.
    pub fn timeout_every(&self, now: f64, interval: f64) -> f64 {
        match (self.pending, self.last) {
            (true, Some(l)) if now.is_finite() => (l + interval - now).clamp(0., interval),
            (true, _) => 0.,
            _ => -1.,
        }
    }

    /// The glass's: at most 30 a second.
    pub fn request(&mut self, now: f64) -> bool {
        self.request_every(now, MIN_INTERVAL_MS)
    }
    pub fn tick(&mut self, now: f64) -> bool {
        self.tick_every(now, MIN_INTERVAL_MS)
    }
    pub fn timeout(&self, now: f64) -> f64 {
        self.timeout_every(now, MIN_INTERVAL_MS)
    }
}

/// Half of the display's frame (`hz` its refresh; 60 if unknown): the cursor plane gets at least two
/// positions a frame, the latest one within half a frame of the mouse.
pub fn cursor_interval(hz: f64) -> f64 {
    let hz = if hz.is_finite() && hz >= 1. { hz.min(1000.) } else { 60. };
    500. / hz
}

thread_local! {
    static PACING: RefCell<Pacing> = RefCell::new(Pacing::default());
    static CURSOR: RefCell<Pacing> = RefCell::new(Pacing::default());
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

/// UI thread, on a mouse report. 1: move the cursor plane now; 0: kept until
/// `axia_cursor_pacing_tick` (`hz`: the refresh of the display under the pointer).
#[no_mangle]
pub extern "C" fn axia_cursor_pacing_request(now_ms: f64, hz: f64) -> i32 {
    CURSOR.with(|p| p.borrow_mut().request_every(now_ms, cursor_interval(hz))) as i32
}

/// UI thread, every loop. 1: the kept move is due now.
#[no_mangle]
pub extern "C" fn axia_cursor_pacing_tick(now_ms: f64, hz: f64) -> i32 {
    CURSOR.with(|p| p.borrow_mut().tick_every(now_ms, cursor_interval(hz))) as i32
}

/// Milliseconds until the kept move is due; -1 if none is kept.
#[no_mangle]
pub extern "C" fn axia_cursor_pacing_timeout(now_ms: f64, hz: f64) -> f64 {
    CURSOR.with(|p| p.borrow().timeout_every(now_ms, cursor_interval(hz)))
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
    fn a_1000_hz_mouse_moves_the_cursor_plane_at_most_twice_a_frame() {
        for hz in [60., 165.] {
            let mut p = Pacing::default();
            let iv = cursor_interval(hz);
            let (mut moves, mut t, mut report, mut steps) = (0, 0., 0., 0);
            let mut worst_wait: f64 = 0.;
            let mut waiting_since: Option<f64> = None;
            while t < 1000. {
                steps += 1;
                assert!(steps < 100_000);
                let due = p.timeout_every(t, iv);
                if due >= 0. && t + due < report + 1. {
                    t += due;
                } else {
                    report += 1.;
                    t = report;
                    if p.request_every(t, iv) {
                        moves += 1;
                        waiting_since = None; // this move carries the latest position
                    } else {
                        waiting_since.get_or_insert(t);
                    }
                }
                if p.tick_every(t, iv) {
                    moves += 1;
                    worst_wait = worst_wait.max(t - waiting_since.take().unwrap_or(t));
                }
            }
            assert!(moves as f64 <= 2. * hz + 2., "{moves} moves at {hz} Hz");
            assert!(moves as f64 >= 2. * hz - 2., "{moves} moves at {hz} Hz");
            assert!(worst_wait <= iv + 1e-6, "a position waited {worst_wait} ms at {hz} Hz");
        }
        assert_eq!(cursor_interval(f64::NAN), 500. / 60.);
        assert_eq!(cursor_interval(0.), 500. / 60.);
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
