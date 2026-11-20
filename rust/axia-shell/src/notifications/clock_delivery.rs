//! Clock completions create notifications and retain typed actions in their owner.
use super::*;
use axia_native::{date_format, i18n::I18n};

// The same live rule store used by lord.fw.blocks, independent of JS snapshots.
#[cfg(not(test))]
pub(super) fn blocked() -> bool {
    extern "C" {
        fn restrict_blocks(id: *const std::ffi::c_char, flag: u32) -> i32;
    }
    unsafe { restrict_blocks(c"lordos".as_ptr(), 1 << 10) != 0 }
}
#[cfg(test)]
thread_local! { static BLOCKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
pub(super) fn blocked() -> bool {
    BLOCKED.with(std::cell::Cell::get)
}

#[derive(Clone)]
pub(super) enum Action {
    Stop,
    Snooze(Value),
    Repeat,
}
impl Action {
    pub(super) fn run(self, wall: i64) {
        axia_clock::with(|engine| match self {
            Self::Stop => {}
            Self::Snooze(id) => engine.native(&json!({"cmd":"alarmSnooze","id":id}), wall),
            Self::Repeat => {
                let seconds = engine.timer["duration"].as_f64().unwrap_or(1000.) / 1000.;
                engine.native(&json!({"cmd":"timerStart","seconds":seconds}), wall);
            }
        });
    }
}
#[derive(Default)]
pub(super) struct Delivery {
    roots: Option<(String, Option<String>)>,
    i18n: I18n,
    region: String,
    hours: String,
    revision: Value,
}
impl Delivery {
    fn configure(&mut self, input: &Value, config: &Value) {
        let roots = crate::native_view::resource_roots();
        if self.roots.as_ref() != Some(&roots) {
            self.i18n = I18n::new(&roots.0, roots.1.as_deref());
            self.roots = Some(roots);
        }
        let revision = input
            .get("localeRevision")
            .or_else(|| input.get("revision"))
            .unwrap_or(&Value::Null);
        if self.revision != *revision {
            self.i18n.invalidate();
            self.revision = revision.clone();
        }
        if let Some(lang) = input["lang"].as_str() {
            self.i18n.lang = lang.into();
        }
        if let Some(region) = input["region"].as_str() {
            self.region = region.into();
        }
        if config.is_object() {
            self.hours = config["hour24"].as_str().unwrap_or("auto").into();
        }
    }
    fn text(&mut self, key: &str) -> String {
        self.i18n.t(key, &Default::default())
    }
    fn time(&mut self, wall: i64) -> String {
        date_format::time(
            &mut self.i18n,
            &self.region,
            &self.hours,
            &axia_clock::local(wall),
            false,
        )
    }
    fn notice(&mut self, effect: &Value, wall: i64) -> (Value, Vec<Action>) {
        let stop = json!({"id":"stop","label":self.text("clock.alarm.stop"),"primary":true});
        let (title, body, other, action) = if effect["kind"] == "alarm" {
            let title = effect["alarm"]["label"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| self.text("clock.alarm.default"));
            let time = self.time(effect["due"].as_i64().unwrap_or(wall));
            let body = self.i18n.t(
                "clock.alarm.ringing",
                &serde_json::from_value(json!({"time":time})).unwrap(),
            );
            let label = self.i18n.t(
                "clock.alarm.snooze",
                &serde_json::from_value(json!({"n":9})).unwrap(),
            );
            (
                title,
                body,
                json!({"id":"snooze","label":label}),
                Action::Snooze(effect["alarm"]["id"].clone()),
            )
        } else {
            (
                self.text("clock.timer.done"),
                self.text("clock.timer.doneBody"),
                json!({"id":"again","label":self.text("clock.timer.again")}),
                Action::Repeat,
            )
        };
        (
            json!({"app":"clock","hasApp":true,"displayLabel":self.text("apps.clock"),
            "title":title,"body":body,"timeLabel":self.time(wall),"sticky":true,"ring":true,"sound":true,
            "actions":[stop,other]}),
            vec![Action::Stop, action],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Service, Instant, i64) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let assets = std::ffi::CString::new(root.to_str().unwrap()).unwrap();
        unsafe {
            crate::native_view::axia_view_init(assets.as_ptr(), std::ptr::null());
        }
        axia_clock::with(|s| *s = Default::default());
        (Service::default(), Instant::now(), axia_clock::now())
    }
    fn complete(s: &mut Service, now: Instant, wall: i64, effect: Value) -> Value {
        s.clock_reply(
            json!({"config":{"hour24":"24"},"effects":[effect]}),
            &json!({"lang":"en"}),
            wall,
            now,
        )
    }
    #[test]
    fn native_templates_keep_settings_and_actions_out_of_the_js_callback_channel() {
        let initial = sound::attempts();
        let (mut s, now, wall) = setup();
        let reply = s.clock_reply(json!({"config":{},"effects":[{"kind":"settings","patch":{"clock":{}}},{"kind":"timer"}]}), &json!({"lang":"en"}), wall, now);
        assert_eq!(
            reply["effects"],
            json!([{"kind":"settings","patch":{"clock":{}}}])
        );
        let reader = s.reader();
        let state = reader.read();
        let n = &state.shown[0].data;
        assert_eq!(n["title"], "Timer done");
        assert_eq!(n["displayLabel"], "Clock");
        assert_eq!(n["actions"][1]["id"], "again");
        assert!(n["actions"][1].get("run").is_none());
        assert!(state.shown[0].ring.is_some());
        assert!(!reply["notificationEffects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "notificationSound"));
        assert_eq!(sound::attempts(), initial + 1);
    }
    #[test]
    fn repeat_from_history_uses_latest_native_duration_and_does_not_emit_a_callback() {
        let (mut s, now, wall) = setup();
        complete(&mut s, now, wall, json!({"kind":"timer"}));
        s.command(&json!({"action":"dismiss","id":1}), now);
        axia_clock::with(|e| e.native(&json!({"cmd":"timerStart","seconds":37}), wall));
        axia_clock::with(|e| e.native(&json!({"cmd":"timerReset"}), wall));
        let reply = s.command(
            &json!({"action":"act","id":1,"index":1,"history":true}),
            now,
        );
        assert!(!reply["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "notificationAction"));
        axia_clock::with(|e| {
            assert_eq!(e.timer["duration"], 37000);
            assert_eq!(e.timer["state"], "running");
        });
        s.command(&json!({"action":"clearHistory"}), now);
        assert!(s.clock_actions.is_empty());
    }
    #[test]
    fn snooze_owns_the_alarm_identity_and_queues_settings_for_the_scheduler() {
        let (mut s, now, wall) = setup();
        axia_clock::with(|e| e.sync(&json!({"alarms":[{"id":"wake","on":false}]})));
        complete(
            &mut s,
            now,
            wall,
            json!({"kind":"alarm","alarm":{"id":"wake","label":"Morning"},"due":wall}),
        );
        assert_eq!(s.reader().read().shown[0].data["title"], "Morning");
        let reply = s.command(&json!({"action":"act","id":1,"index":1}), now);
        assert!(s.reader().read().shown.is_empty());
        assert!(!reply["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "notificationAction"));
        axia_clock::with(|e| {
            let alarm = &e.config["alarms"][0];
            assert_eq!(alarm["on"], true);
            assert!((alarm["snooze"].as_i64().unwrap() - wall - 540000).abs() < 2000);
            let pending = e.command(&json!({"cmd":"arm"}), wall);
            assert!(pending["effects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["kind"] == "settings"));
        });
        assert!(s.wait_ms(now).is_none());
    }
    #[test]
    fn clock_command_delivers_once_and_obeys_dnd_and_permission() {
        for policy in [
            json!({"prefs":{"dnd":true}}),
            json!({"prefs":{"apps":{"clock":{"allow":false}}}}),
        ] {
            let (mut s, now, wall) = setup();
            s.clock_command(
                &json!({"action":"timerStart","seconds":1,"notificationPolicy":policy}),
                wall - 2000,
                now,
            );
            let reply = s.clock_command(&json!({"action":"arm"}), wall, now);
            assert!(reply["effects"].as_array().unwrap().is_empty());
            assert!(s.reader().read().shown.is_empty());
            assert!(s.wait_ms(now).is_none());
            let n = s.reader().read().history.len();
            s.clock_command(&json!({"action":"arm"}), wall, now);
            assert_eq!(s.reader().read().history.len(), n);
        }
    }
    #[test]
    fn clock_action_storage_is_bounded_and_stop_does_not_restart_the_timer() {
        let (mut s, now, wall) = setup();
        for _ in 0..80 {
            complete(&mut s, now, wall, json!({"kind":"timer"}));
        }
        assert!(s.clock_actions.len() <= HISTORY + 1);
        let reply = s.command(&json!({"action":"act","id":80,"index":0}), now);
        assert!(!reply["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "notificationAction"));
        axia_clock::with(|e| assert_eq!(e.timer["state"], "idle"));
        s.command(&json!({"action":"clearHistory"}), now);
        assert!(s.clock_actions.len() <= MAX_SHOWN);
    }
    #[test]
    fn live_firewall_rules_block_delivery_and_cancel_an_existing_ring() {
        let initial = sound::attempts();
        let (mut s, now, wall) = setup();
        BLOCKED.with(|v| v.set(true));
        complete(&mut s, now, wall, json!({"kind":"timer"}));
        assert!(s.reader().read().history.is_empty());
        assert!(s.clock_actions.is_empty());
        assert_eq!(sound::attempts(), initial);
        BLOCKED.with(|v| v.set(false));
        complete(&mut s, now, wall, json!({"kind":"timer"}));
        assert_eq!(s.reader().read().snapshot()["ringing"], json!([1]));
        s.expire(now + RING_STEP);
        BLOCKED.with(|v| v.set(true));
        s.effects();
        assert_eq!(sound::attempts(), initial + 1);
        s.expire(now + RING_STEP * 2);
        assert!(s.reader().read().snapshot()["ringing"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(!s.effects().iter().any(|e| e["kind"] == "notificationSound"));
        assert_eq!(sound::attempts(), initial + 1);
        BLOCKED.with(|v| v.set(false));
    }
    #[test]
    fn user_catalogs_locale_revisions_and_default_alarm_labels_are_native() {
        let (mut s, now, wall) = setup();
        let roots = crate::native_view::resource_roots();
        let home =
            std::env::temp_dir().join(format!("axia-clock-locale-{}-{}", std::process::id(), wall));
        let catalog = home.join(".local/share/axia/locales/xx/clock.json");
        std::fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        std::fs::write(&catalog, r#"{"alarm.default":"Wake up","alarm.ringing":"At {time}","alarm.snooze":"Wait {n}","timer.done":"First"}"#).unwrap();
        let assets = std::ffi::CString::new(roots.0).unwrap();
        let custom = std::ffi::CString::new(home.to_str().unwrap()).unwrap();
        unsafe {
            crate::native_view::axia_view_init(assets.as_ptr(), custom.as_ptr());
        }
        let input = json!({"lang":"xx","region":"","localeRevision":1});
        s.clock_reply(json!({"config":{"hour24":"24"},"effects":[{"kind":"alarm","alarm":{"id":"wake","label":""},"due":wall}]}), &input, wall, now);
        let n = s.reader().read().shown[0].data.clone();
        let local = axia_clock::local(wall);
        assert_eq!(n["title"], "Wake up");
        assert_eq!(
            n["body"],
            format!("At {:02}:{:02}", local.tm_hour, local.tm_min)
        );
        assert_eq!(n["actions"][1]["label"], "Wait 9");
        std::fs::write(&catalog, r#"{"timer.done":"Updated"}"#).unwrap();
        s.clock_reply(
            json!({"effects":[{"kind":"timer"}]}),
            &json!({"lang":"xx","localeRevision":2}),
            wall,
            now,
        );
        assert_eq!(s.reader().read().shown[0].data["title"], "Updated");
        assert_eq!(s.reader().read().shown[0].data["body"], "Time is up.");
        std::fs::remove_dir_all(&home).unwrap();
    }
}
impl Service {
    pub(crate) fn clock_command(&mut self, input: &Value, wall: i64, now: Instant) -> Value {
        if let Some(policy) = input.get("notificationPolicy") {
            self.policy(policy);
        }
        let mut command = input.clone();
        command["cmd"] = input["action"].clone();
        let reply = axia_clock::with(|s| s.command(&command, wall));
        self.clock_reply(reply, input, wall, now)
    }
    pub(crate) fn clock_reply(
        &mut self,
        mut reply: Value,
        input: &Value,
        wall: i64,
        now: Instant,
    ) -> Value {
        self.clock_delivery.configure(input, &reply["config"]);
        let effects = reply
            .get_mut("effects")
            .map(std::mem::take)
            .unwrap_or(Value::Null);
        let mut kept = Vec::new();
        let mut notifications = Vec::new();
        for effect in effects.as_array().into_iter().flatten() {
            if effect["kind"] == "alarm" || effect["kind"] == "timer" {
                self.ring_blocked = blocked();
                let (notice, actions) = self.clock_delivery.notice(effect, wall);
                let local = axia_clock::local(wall);
                let delivered = self.command(
                    &json!({"action":"push","notice":notice,"prefs":self.ring_prefs,
                    "permission":self.clock_permission,"blocked":self.ring_blocked,"wallMs":wall,
                    "minute":local.tm_hour*60+local.tm_min}),
                    now,
                );
                if let Some(id) = delivered["result"].as_u64().filter(|id| *id > 0) {
                    self.clock_actions.insert(id, actions);
                }
                notifications.extend(
                    delivered["effects"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .cloned(),
                );
            } else {
                kept.push(effect.clone());
            }
        }
        if reply.get("effects").is_some() {
            reply["effects"] = json!(kept);
        }
        if !notifications.is_empty() {
            reply["notificationEffects"] = json!(notifications);
        }
        reply
    }
}
