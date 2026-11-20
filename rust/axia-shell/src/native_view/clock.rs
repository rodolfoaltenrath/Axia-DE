//! Clock deadlines and the bar indicator borrow the resident Rust clock engine.
use axia_clock as clock;
use axia_native::{date_format, i18n::I18n, NativeApp};
use axia_state::Subscription;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub struct Scheduler {
    changes: Subscription,
    until: Option<Instant>,
    last: Value,
    mounted: bool,
    notifications: Option<std::rc::Rc<std::cell::RefCell<crate::notifications::Service>>>,
    locale: Value,
}
impl Default for Scheduler {
    fn default() -> Self {
        Self {
            changes: clock::subscribe(),
            until: None,
            last: Value::Null,
            mounted: false,
            notifications: None,
            locale: Value::Null,
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
    fn sync_at(&mut self, wall: i64, now: Instant) -> Value {
        let mut reply = clock::with(|s| s.command(&json!({"cmd":"arm"}), wall));
        if let Some(service) = &self.notifications {
            reply = service
                .borrow_mut()
                .clock_reply(reply, &self.locale, wall, now);
        }
        self.changes.take();
        crate::clock_settings::take();
        self.until = reply["wait"]
            .as_u64()
            .map(|ms| now + Duration::from_millis(ms));
        let state = json!({"timer":reply["timer"],"stopwatch":reply["stopwatch"],"nextAlarm":reply["nextAlarm"],"config":reply["config"]});
        let effects = if self.last != state
            || reply["effects"].as_array().is_some_and(|e| !e.is_empty())
            || reply["notificationEffects"]
                .as_array()
                .is_some_and(|e| !e.is_empty())
        {
            self.last = state;
            json!([{"kind":"clockReply","reply":reply}])
        } else {
            json!([])
        };
        if std::mem::replace(&mut self.mounted, true) {
            json!({"same":true,"effects":effects})
        } else {
            json!({"tree":{"kind":"view","style":{"pointerEvents":"none"}},"effects":effects})
        }
    }
}
impl NativeApp for Scheduler {
    fn invalidated(&self) -> bool {
        self.changes.pending() || crate::clock_settings::pending()
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.until.map(|until| {
            until
                .saturating_duration_since(Instant::now())
                .as_secs_f64()
                * 1000.
                + 0.01
        })
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        self.refresh_ms()
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] == "sync" {
            self.locale = json!({"lang":input["lang"],"region":input["context"]["region"],
                "localeRevision":input["context"]["localeRevision"]});
            self.sync_at(clock::now(), Instant::now())
        } else {
            json!({})
        }
    }
}

fn tab(timer: &Value, next: Option<i64>, now: i64) -> Option<&'static str> {
    if timer["state"] != "idle" {
        Some("timer")
    } else if next.is_some_and(|at| at.saturating_sub(now) < 86_400_000) {
        Some("alarms")
    } else {
        None
    }
}
fn timer_caption(ms: i64) -> String {
    let sec = ms.max(0).saturating_add(999) / 1000;
    if sec >= 3600 {
        format!("{}:{:02}:{:02}", sec / 3600, sec / 60 % 60, sec % 60)
    } else {
        format!("{}:{:02}", sec / 60 % 60, sec % 60)
    }
}
fn clock_mark(timer: bool, color: &Value) -> Value {
    let mut children = vec![
        json!({"kind":"view","style":{"position":"absolute","left":0,"top":2,"width":12,"height":12,"borderRadius":6,"border":format!("1px solid {}",color.as_str().unwrap_or("#fff"))}}),
        json!({"kind":"view","style":{"position":"absolute","left":5.5,"top":4,"width":1,"height":4,"backgroundColor":color}}),
        json!({"kind":"view","style":{"position":"absolute","left":6,"top":7,"width":3,"height":1,"backgroundColor":color}}),
    ];
    if timer {
        children.push(json!({"kind":"view","style":{"position":"absolute","left":4,"top":0,"width":4,"height":1,"backgroundColor":color}}));
    } else {
        for left in [0, 9] {
            children.push(json!({"kind":"view","style":{"position":"absolute","left":left,"top":0,"width":3,"height":1,"backgroundColor":color}}));
        }
    }
    json!({"kind":"view","key":"clock-mark","style":{"width":12,"height":14},"children":children})
}
pub struct Status {
    changes: Subscription,
    i18n: I18n,
    until: Option<Instant>,
    last: Value,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            changes: clock::subscribe(),
            i18n: I18n::default(),
            until: None,
            last: Value::Null,
        }
    }
}
impl Status {
    fn sync_at(&mut self, input: &Value, now: i64, mono: Instant) -> Value {
        self.changes.take();
        let tree = clock::with(|s| {
            let next = s.next_alarm(now).map(|(at, _)| at);
            let selected = tab(&s.timer, next, now);
            let running = s.timer["state"] == "running";
            self.until = if running {
                Some(mono + Duration::from_secs(1))
            } else if selected.is_none() {
                next.map(|at| {
                    mono + Duration::from_millis(
                        at.saturating_sub(now)
                            .saturating_sub(86_400_000)
                            .saturating_add(1)
                            .clamp(1, 600_000) as u64,
                    )
                })
            } else {
                None
            };
            if let Some(selected) = selected {
                self.i18n.lang = input["lang"].as_str().unwrap_or("en").to_owned();
                let caption = if selected == "timer" {
                    timer_caption(clock::timer_left(&s.timer, now))
                } else {
                    date_format::time(
                        &mut self.i18n,
                        input["context"]["region"]
                            .as_str()
                            .or_else(|| input["region"].as_str())
                            .unwrap_or(""),
                        s.config["hour24"].as_str().unwrap_or(""),
                        &clock::local(next.unwrap_or(now)),
                        false,
                    )
                };
                let timer = selected == "timer";
                let weight = if timer { 600. } else { 400. };
                let text_width = if timer {
                    caption.chars().count() as f64 * 7.5
                } else {
                    super::measure_text(&caption, 12., weight)
                };
                let width = text_width.ceil() + 32.;
                json!({"kind":"view","key":"clock-status","style":{"height":input["args"]["height"].as_f64().unwrap_or(22.),"width":width,
                "paddingHorizontal":7,"gap":6,"flexDirection":"row","alignItems":"center"},"children":[clock_mark(timer,&input["args"]["color"]),{"kind":"text","key":"caption","label":caption,
                "style":{"fontSize":12,"fontWeight":weight,"color":input["args"]["color"],"fontFamily":if timer {"monospace"} else {"sans-serif"},"whiteSpace":"nowrap"}}],
                "events":{"onClick":["open"]}})
            } else {
                json!({"kind":"view","key":"clock-status","style":{"width":0,"height":0}})
            }
        });
        if tree == self.last {
            json!({"same":true})
        } else {
            self.last = tree.clone();
            json!({"tree":tree})
        }
    }
}
impl NativeApp for Status {
    fn resources(&mut self, assets: &str, home: Option<&str>) {
        self.i18n = I18n::new(assets, home);
    }
    fn invalidated(&self) -> bool {
        self.changes.pending()
    }
    fn refresh_ms(&self) -> Option<f64> {
        self.until.map(|until| {
            until
                .saturating_duration_since(Instant::now())
                .as_secs_f64()
                * 1000.
                + 0.01
        })
    }
    fn background_refresh_ms(&self) -> Option<f64> {
        self.refresh_ms()
    }
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str() {
            Some("sync") => self.sync_at(input, clock::now(), Instant::now()),
            Some("open") => {
                let now = clock::now();
                let selected =
                    clock::with(|s| tab(&s.timer, s.next_alarm(now).map(|(at, _)| at), now));
                selected.map(|tab|json!({"effects":[{"kind":"openApp","app":"clock","args":{"tab":tab}}]})).unwrap_or_else(||json!({}))
            }
            _ => json!({}),
        }
    }
}

#[cfg(test)]
#[path = "clock_tests.rs"]
mod tests;
