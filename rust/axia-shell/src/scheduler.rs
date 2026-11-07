//! The shell's timers and animation-frame queues. JavaScript retains only callback references;
//! deadlines, repeat rules, cancellation, stable ordering and wake-up decisions belong here.
//! Each QuickJS context owns one scheduler, including isolated application contexts.
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

const MAX_CALLBACKS: usize = 65_536;
const MAX_ID: u64 = (1_u64 << 53) - 1; // IDs are exact JavaScript numbers.
const MAX_WAIT_MS: f64 = i32::MAX as f64;

#[derive(Clone, Copy, Debug)]
struct Deadline {
    at: f64,
    id: u64,
}
impl PartialEq for Deadline {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Deadline {}
impl PartialOrd for Deadline {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Deadline {
    fn cmp(&self, other: &Self) -> Ordering {
        self.at.total_cmp(&other.at).then(self.id.cmp(&other.id))
    }
}

#[derive(Clone, Copy)]
struct Timer {
    at: f64,
    interval: Option<f64>,
}

#[derive(Default)]
pub struct Scheduler {
    timers: BTreeMap<u64, Timer>,
    deadlines: BTreeSet<Deadline>,
    frames: BTreeSet<u64>,
    next_timer: u64,
    next_frame: u64,
    frame_after: Option<f64>,
    presentation: u32,
    /// Set when JS enqueues new work after the loop last queried its deadlines.
    changed: bool,
}

fn delay_ms(delay: f64) -> f64 {
    if delay.is_nan() || delay <= 0. {
        0.
    } else {
        delay.min(MAX_WAIT_MS)
    }
}

impl Scheduler {
    pub fn schedule(&mut self, now: f64, delay: f64, repeating: bool) -> u64 {
        if !now.is_finite() || self.timers.len() >= MAX_CALLBACKS {
            return 0;
        }
        let delay = delay_ms(delay);
        let id = loop {
            self.next_timer = self.next_timer % MAX_ID + 1;
            if !self.timers.contains_key(&self.next_timer) {
                break self.next_timer;
            }
        };
        let timer = Timer {
            at: now + delay,
            interval: repeating.then_some(delay.max(1.)),
        };
        self.timers.insert(id, timer);
        self.deadlines.insert(Deadline { at: timer.at, id });
        self.changed = true;
        id
    }

    pub fn cancel(&mut self, id: u64) {
        if let Some(timer) = self.timers.remove(&id) {
            self.deadlines.remove(&Deadline { at: timer.at, id });
            self.changed = true;
        }
    }

    /// Snapshot the initial due set. Timers created by callbacks wait for the next tick,
    /// and a callback can still cancel another member of this snapshot before it runs.
    pub fn due(&self, now: f64) -> impl Iterator<Item = u64> + '_ {
        self.deadlines
            .iter()
            .take_while(move |d| d.at <= now)
            .map(|d| d.id)
    }

    /// Prepare a callback immediately before JS invokes it: -1 cancelled, 0 once, 1 repeating.
    /// Rescheduling first makes clearInterval(self) inside the callback work without special cases.
    pub fn fire(&mut self, id: u64, tick_time: f64) -> i32 {
        if !tick_time.is_finite() {
            return -1;
        }
        let Some(timer) = self.timers.remove(&id) else {
            return -1;
        };
        self.deadlines.remove(&Deadline { at: timer.at, id });
        if let Some(interval) = timer.interval {
            let next = Timer {
                at: tick_time + interval,
                ..timer
            };
            self.timers.insert(id, next);
            self.deadlines.insert(Deadline { at: next.at, id });
            1
        } else {
            0
        }
    }

    pub fn wait(&self, now: f64) -> f64 {
        if now.is_nan() {
            return -1.;
        }
        self.deadlines
            .first()
            .map_or(-1., |d| (d.at - now).clamp(0., MAX_WAIT_MS))
    }

    pub fn request_frame(&mut self) -> u64 {
        if self.frames.len() >= MAX_CALLBACKS {
            return 0;
        }
        let id = loop {
            self.next_frame = self.next_frame % MAX_ID + 1;
            if !self.frames.contains(&self.next_frame) {
                break self.next_frame;
            }
        };
        self.frames.insert(id);
        id
    }
    /// Isolated apps wait for Wayland presentation. With no submitted frame (or before
    /// configuration), fall back to 60 Hz rather than repeatedly running RAF after JS jobs.
    pub fn frame_wait(&self, now: f64, waiting: bool, presentation: u32) -> f64 {
        if self.frames.is_empty() || waiting || !now.is_finite() {
            return -1.;
        }
        if presentation != self.presentation {
            return 0.;
        }
        self.frame_after.map_or(0., |after| (after - now).max(0.))
    }
    pub fn frame_ready(&mut self, now: f64, waiting: bool, presentation: u32) -> bool {
        if self.frame_wait(now, waiting, presentation) != 0. {
            return false;
        }
        self.frame_after = Some(now + 1000. / 60.);
        self.presentation = presentation;
        true
    }
    pub fn cancel_frame(&mut self, id: u64) {
        self.frames.remove(&id);
    }
    pub fn take_frames(&mut self) -> Vec<u64> {
        let frames = self.frames.iter().copied().collect();
        self.frames.clear();
        frames
    }
    pub fn reset(&mut self) {
        self.timers.clear();
        self.deadlines.clear();
        self.frames.clear();
        self.frame_after = None;
        self.changed = true;
    }
    /// One extra loop after JS queued work during rendering; never poll an idle context.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
}

// Opaque state belongs to the QuickJS object's finalizer. Calls and destruction happen on its
// single UI thread; no JS values cross this boundary, so GC ownership stays with QuickJS.
#[no_mangle]
pub extern "C" fn axia_scheduler_new() -> *mut Scheduler {
    Box::into_raw(Box::new(Scheduler::default()))
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_free(state: *mut Scheduler) {
    if !state.is_null() {
        drop(Box::from_raw(state));
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_add(
    state: *mut Scheduler,
    now: f64,
    delay: f64,
    repeating: bool,
) -> u64 {
    state
        .as_mut()
        .map_or(0, |s| s.schedule(now, delay, repeating))
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_cancel(state: *mut Scheduler, id: u64) {
    if let Some(s) = state.as_mut() {
        s.cancel(id);
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_due(
    state: *const Scheduler,
    now: f64,
    out: *mut u64,
    capacity: usize,
) -> usize {
    let Some(s) = state.as_ref() else {
        return 0;
    };
    let mut n = 0;
    for id in s.due(now) {
        if !out.is_null() && n < capacity {
            *out.add(n) = id;
        }
        n += 1;
    }
    n
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_fire(state: *mut Scheduler, id: u64, now: f64) -> i32 {
    state.as_mut().map_or(-1, |s| s.fire(id, now))
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_wait(state: *const Scheduler, now: f64) -> f64 {
    state.as_ref().map_or(-1., |s| s.wait(now))
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_frame(state: *mut Scheduler) -> u64 {
    state.as_mut().map_or(0, Scheduler::request_frame)
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_cancel_frame(state: *mut Scheduler, id: u64) {
    if let Some(s) = state.as_mut() {
        s.cancel_frame(id);
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_frame_count(state: *const Scheduler) -> usize {
    state.as_ref().map_or(0, |s| s.frames.len())
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_take_frames(
    state: *mut Scheduler,
    out: *mut u64,
    capacity: usize,
) -> usize {
    let Some(s) = state.as_mut() else {
        return 0;
    };
    if out.is_null() || capacity < s.frames.len() {
        return s.frames.len();
    }
    let frames = s.take_frames();
    for (i, id) in frames.iter().enumerate() {
        *out.add(i) = *id;
    }
    frames.len()
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_frame_wait(
    state: *const Scheduler,
    now: f64,
    waiting: bool,
    presentation: u32,
) -> f64 {
    state
        .as_ref()
        .map_or(-1., |s| s.frame_wait(now, waiting, presentation))
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_frame_ready(
    state: *mut Scheduler,
    now: f64,
    waiting: bool,
    presentation: u32,
) -> bool {
    state
        .as_mut()
        .is_some_and(|s| s.frame_ready(now, waiting, presentation))
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_reset(state: *mut Scheduler) {
    if let Some(s) = state.as_mut() {
        s.reset();
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_scheduler_changed(state: *mut Scheduler) -> bool {
    state.as_mut().is_some_and(Scheduler::take_changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn due_order_is_deadline_then_id_and_creation_waits_for_next_tick() {
        let mut s = Scheduler::default();
        let late = s.schedule(0., 20., false);
        let first = s.schedule(0., 10., false);
        let second = s.schedule(0., 10., false);
        let batch: Vec<_> = s.due(30.).collect();
        assert_eq!(batch, [first, second, late]);
        assert_eq!(s.fire(first, 30.), 0);
        let created = s.schedule(30., 0., false);
        s.cancel(second);
        assert_eq!(s.fire(second, 30.), -1);
        assert_eq!(s.fire(late, 30.), 0);
        assert_eq!(s.due(30.).collect::<Vec<_>>(), [created]);
    }
    #[test]
    fn intervals_reschedule_from_tick_with_a_minimum_of_one_millisecond() {
        let mut s = Scheduler::default();
        let id = s.schedule(5., 0., true);
        assert_eq!(s.fire(id, 10.), 1);
        assert_eq!(s.wait(10.), 1.);
        assert!(s.due(10.).next().is_none());
        s.cancel(id);
        assert_eq!(s.wait(11.), -1.);
    }
    #[test]
    fn cancelling_the_earliest_deadline_never_leaves_a_stale_wait() {
        let mut s = Scheduler::default();
        let first = s.schedule(0., 1., false);
        s.schedule(0., 100., false);
        s.cancel(first);
        assert_eq!(s.wait(0.), 100.);
        s.cancel(first);
        assert_eq!(s.wait(101.), 0.);
    }
    #[test]
    fn frames_use_an_independent_snapshot_and_id_space() {
        let mut s = Scheduler::default();
        assert_eq!(s.schedule(0., 0., false), 1);
        let a = s.request_frame();
        let b = s.request_frame();
        assert_eq!(a, 1);
        s.cancel_frame(b);
        let batch = s.take_frames();
        let next = s.request_frame();
        s.cancel_frame(a);
        assert_eq!(batch, [a]);
        assert_eq!(s.take_frames(), [next]);
    }
    #[test]
    fn invalid_delays_and_ids_are_bounded_without_spinning() {
        let mut s = Scheduler::default();
        for delay in [f64::NAN, -5., f64::NEG_INFINITY] {
            let id = s.schedule(10., delay, false);
            assert_eq!(s.wait(10.), 0.);
            s.cancel(id);
        }
        s.schedule(0., f64::INFINITY, false);
        assert_eq!(s.wait(0.), MAX_WAIT_MS);
        assert_eq!(s.schedule(f64::NAN, 0., false), 0);
        s.next_timer = MAX_ID;
        assert_ne!(s.schedule(0., 0., false), 0);
        s.reset();
        assert_eq!(s.wait(0.), -1.);
    }
    #[test]
    fn contexts_reset_and_changes_are_consumed_once() {
        let mut a = Scheduler::default();
        let b = Scheduler::default();
        a.schedule(0., 10., false);
        a.request_frame();
        assert!(a.take_changed());
        assert!(!a.take_changed());
        assert_eq!(b.wait(0.), -1.);
        assert!(b.frames.is_empty());
        a.reset();
        assert!(a.frames.is_empty());
        assert!(a.deadlines.is_empty());
        assert!(a.take_changed());
        assert!(!a.take_changed());
    }
    #[test]
    fn isolated_app_frames_wait_for_presentation_and_never_spin_on_js_jobs() {
        let mut s = Scheduler::default();
        s.request_frame();
        assert!(
            !s.take_changed(),
            "RAF alone must not cause an unpaced zero-time poll loop"
        );
        assert!(s.frame_ready(0., false, 0));
        s.take_frames();
        s.request_frame();
        for now in [0., 1., 6., 12.] {
            assert!(
                !s.frame_ready(now, false, 0),
                "no presentation and before software deadline"
            );
            assert!(s.frame_wait(now, false, 0) > 0.);
        }
        assert!(
            !s.frame_ready(20., true, 0),
            "a submitted Wayland frame is still pending"
        );
        assert_eq!(s.frame_wait(20., true, 0), -1.);
        assert!(s.frame_ready(20., false, 1));
        s.take_frames();
        s.request_frame();
        // At 165 Hz, a real callback releases the next frame without a 60 Hz software cap.
        assert!(s.frame_ready(26., false, 2));
        s.take_frames();
        s.request_frame();
        assert!(!s.frame_ready(27., false, 2));
        assert!(
            s.frame_ready(43., false, 2),
            "with no submitted frame, software cadence resumes"
        );
    }

    #[test]
    fn queue_limit_preserves_existing_callbacks() {
        let mut s = Scheduler::default();
        for _ in 0..MAX_CALLBACKS {
            assert_ne!(s.schedule(0., 100., false), 0);
        }
        assert_eq!(s.schedule(0., 0., false), 0);
        assert_eq!(s.due(100.).count(), MAX_CALLBACKS);
    }
}
