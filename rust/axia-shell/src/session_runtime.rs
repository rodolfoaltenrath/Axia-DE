//! Resident context services. The compositor drives them even with no scene hosts.
use crate::winmodel::{Session, WeakSession};
use std::{cell::RefCell, time::Instant};

#[path = "session_clock.rs"]
pub(crate) mod clock;

#[path = "session_displays.rs"]
pub(crate) mod displays;

#[path = "session_preferences.rs"]
pub(crate) mod preferences;

thread_local! { static CONTEXTS: RefCell<Vec<WeakSession>> = const { RefCell::new(Vec::new()) }; }

pub(crate) fn register(owner: WeakSession) {
    CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        contexts.retain(|context| context.upgrade().is_some());
        if !contexts.iter().any(|context| context.ptr_eq(&owner)) { contexts.push(owner); }
    });
}
fn each(mut visit: impl FnMut(Session)) {
    CONTEXTS.with(|contexts| {
        contexts.borrow_mut().retain(|context| context.upgrade().is_some());
        let count = contexts.borrow().len();
        for index in 0..count {
            let session = contexts.borrow().get(index).and_then(WeakSession::upgrade);
            // Never lend the registry across a model tick, and allocate no snapshot per frame.
            if let Some(session) = session { visit(session); }
        }
    });
}
pub(crate) fn tick(now: Instant) {
    each(|mut session| session.service_tick(now));
}
pub(crate) fn key(code: i32, value: i32, key: &str, flags: i32) -> i32 {
    let mut used = 0;
    let enabled = crate::keygrab::enabled();
    each(|mut session| {
        if used == 0 {
            used = session.physical_key(code,value,key,flags,enabled);
            if enabled { session.sync_key_grabs(); }
        }
    });
    used
}
pub(crate) fn timeout(now: Instant) -> Option<f64> {
    let mut wait: Option<f64> = None;
    each(|session| {
        if let Some(next) = session.service_wait(now) { wait = Some(wait.map_or(next, |wait|wait.min(next))); }
    });
    wait
}
pub(crate) fn pending() -> bool {
    let mut pending = false;
    each(|session| pending |= session.service_pending());
    pending
}

#[cfg(test)]
#[path = "session_runtime_tests.rs"]
mod tests;
