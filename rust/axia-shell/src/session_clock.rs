//! Resident clock scheduling; presentation and theme mounts do not own deadlines.
use axia_clock as clock;
use axia_state::Subscription;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub struct Scheduler {
    changes: Subscription,
    until: Option<Instant>,
    last: Value,
    notifications: Option<std::rc::Rc<std::cell::RefCell<crate::notifications::Service>>>,
}
impl Default for Scheduler {
    fn default() -> Self {
        Self {
            changes: clock::subscribe(),
            until: None,
            last: Value::Null,
            notifications: None,
        }
    }
}
impl Scheduler {
    pub fn new(
        notifications: std::rc::Rc<std::cell::RefCell<crate::notifications::Service>>,
    ) -> Self {
        Self {
            notifications: Some(notifications),
            ..Default::default()
        }
    }
    pub(crate) fn tick_at(&mut self, wall: i64, now: Instant) -> Vec<Value> {
        let mut reply = clock::with(|s| s.command(&json!({"cmd":"arm"}), wall));
        if let Some(service) = &self.notifications {
            reply = service
                .borrow_mut()
                .clock_reply(reply, &Value::Null, wall, now);
        }
        self.changes.take();
        crate::clock_settings::take();
        self.until = reply["wait"]
            .as_u64()
            .map(|ms| now + Duration::from_millis(ms));
        let state = json!({"timer":reply["timer"],"stopwatch":reply["stopwatch"],"nextAlarm":reply["nextAlarm"],"config":reply["config"]});
        if self.last != state
            || reply["effects"].as_array().is_some_and(|e| !e.is_empty())
            || reply["notificationEffects"]
                .as_array()
                .is_some_and(|e| !e.is_empty())
        {
            self.last = state;
            vec![json!({"kind":"clockReply","reply":reply})]
        } else { Vec::new() }
    }
    pub(crate) fn invalidated(&self) -> bool {
        self.last.is_null() || self.changes.pending() || crate::clock_settings::pending()
    }
    pub(crate) fn wait_ms(&self, now: Instant) -> Option<f64> {
        if self.invalidated() { return Some(0.); }
        self.until.map(|until| until.saturating_duration_since(now).as_secs_f64() * 1000.)
    }
}

#[cfg(test)]
#[path = "session_clock_tests.rs"]
mod tests;
