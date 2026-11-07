//! Main-loop stall watch: names the phase of axia-server's loop that held the screen.
//!
//! The session log showed gaps of 250-360 ms between frames with nothing saying where they went.
//! The C loop times a few phases (the theme's timers, rendering, dispatching what poll() returned,
//! the event queues) and reports each duration here; a phase that takes longer than
//! `THRESHOLD_MS` is logged, at most once per phase every `QUIET_S`, with how many more happened
//! meanwhile and the worst of them, so a stuttering session can't flood the log.
use std::cell::RefCell;
use std::time::Instant;

pub const THRESHOLD_MS: f64 = 50.;
pub const QUIET_S: f64 = 10.;
const PHASES: [&str; 4] = ["theme timers (JS)", "rendering", "dispatching input, apps and devices", "event queues (JS)"];

#[derive(Default, Clone, Copy)]
struct Phase {
    logged_at: Option<f64>,
    more: u32,
    worst: f64,
}

#[derive(Default)]
pub struct StallWatch {
    phases: [Phase; PHASES.len()],
}

impl StallWatch {
    /// One phase took `ms` at `now_s` (seconds, monotonic). Returns the line to log, if any.
    pub fn note(&mut self, phase: usize, ms: f64, now_s: f64) -> Option<String> {
        let name = PHASES.get(phase)?;
        if !ms.is_finite() || ms < THRESHOLD_MS {
            return None;
        }
        let p = &mut self.phases[phase];
        if p.logged_at.is_some_and(|t| now_s - t < QUIET_S) {
            p.more += 1;
            p.worst = p.worst.max(ms);
            return None;
        }
        let tail = if p.more > 0 {
            format!(" ({} more over {THRESHOLD_MS:.0} ms since the last line, the worst {:.0} ms)", p.more, p.worst)
        } else {
            String::new()
        };
        *p = Phase { logged_at: Some(now_s), more: 0, worst: 0. };
        Some(format!("stall: {name} held the screen for {ms:.0} ms{tail}"))
    }
}

thread_local! {
    static WATCH: RefCell<StallWatch> = RefCell::new(StallWatch::default());
    static START: Instant = Instant::now();
}

/// axia-server's loop: `phase` 0 theme timers, 1 rendering, 2 dispatching poll()'s fds, 3 event queues.
#[no_mangle]
pub extern "C" fn axia_stall_note(phase: u32, ms: f64) {
    if ms < THRESHOLD_MS {
        return;                             // the common case: nothing to look up
    }
    let now_s = START.with(|s| s.elapsed().as_secs_f64());
    if let Some(line) = WATCH.with(|w| w.borrow_mut().note(phase as usize, ms, now_s)) {
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_phases_are_never_logged() {
        let mut w = StallWatch::default();
        for i in 0..1000 {
            assert_eq!(w.note(1, 49.9, f64::from(i)), None);
        }
        assert_eq!(w.note(1, f64::NAN, 0.), None);
        assert_eq!(w.note(9, 500., 0.), None, "an unknown phase");
    }

    #[test]
    fn a_stall_is_named_then_quiet_and_summarized() {
        let mut w = StallWatch::default();
        assert_eq!(w.note(1, 312., 100.).unwrap(), "stall: rendering held the screen for 312 ms");
        assert_eq!(w.note(1, 80., 101.), None);
        assert_eq!(w.note(1, 340., 105.), None);
        // another phase has its own quiet period
        assert!(w.note(0, 60., 105.).unwrap().starts_with("stall: theme timers (JS)"));
        assert_eq!(
            w.note(1, 70., 111.).unwrap(),
            "stall: rendering held the screen for 70 ms (2 more over 50 ms since the last line, the worst 340 ms)"
        );
        assert_eq!(w.note(1, 55., 125.).unwrap(), "stall: rendering held the screen for 55 ms");
    }
}
