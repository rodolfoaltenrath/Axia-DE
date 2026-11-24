//! Session-owned capture scheduling. Views display countdowns; they do not own
//! the work or its deadline. SDK effects only execute media/window transports.
use crate::winmodel::Session;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
#[path = "capture_policy.rs"]
mod policy;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Hide,
    Countdown,
    Settle,
    Media,
}
struct Pending {
    ticket: u64,
    mode: String,
    record: Option<u64>,
    restore_record: Option<i64>,
    reported: bool,
    phase: Phase,
    due: Option<Instant>,
}
#[derive(Default)]
pub(crate) struct Service {
    state: axia_app_screenshots::session::State,
    preferences: Option<axia_state::Reader<Value>>,
    pending: Option<Pending>,
    generation: u64,
    home: String,
    assets: String,
    output_i18n: axia_native::i18n::I18n,
    output_revision: Option<u64>,
    outputs: axia_app_screenshots::output::Outputs,
    record_outputs: std::collections::BTreeMap<i64, u64>,
    refusal_ids: std::collections::BTreeSet<u64>,
}
impl Service {
    pub(crate) fn bind_settings(&mut self, home: &str) {
        self.home = crate::settings::profile(home);
        self.assets = crate::native_view::resource_roots().0;
        self.output_i18n = axia_native::i18n::I18n::new(&self.assets, Some(&self.home));
        self.output_revision = None;
        self.preferences = Some(crate::settings::with(
            &crate::settings::profile(home),
            |s| s.reader(),
        ));
    }
    pub(crate) fn output(&mut self, input: &Value, wall: i64) -> Value {
        match input["op"].as_str() {
            Some("begin") => {
                self.sync_presentation();
                let prefs = self
                    .preferences
                    .as_ref()
                    .map(|p| p.read()["screenshots"].clone())
                    .unwrap_or(Value::Null);
                self.outputs
                    .begin(input, &prefs, &mut self.output_i18n, wall)
            }
            Some("next") => self.outputs.next(input),
            Some("complete") => {
                let mut reply = self.outputs.complete(input);
                if reply["kind"] == "image" && reply["ok"] == true && reply["path"].is_string() {
                    self.state
                        .request(&json!({"op":"recentAdd","path":reply["path"]}), wall);
                }
                reply["capture"] = self.state.snapshot();
                reply
            }
            Some("release") => {
                if let Some(id) = input["id"].as_u64() {
                    self.outputs.release(id);
                }
                json!({"ok":true})
            }
            _ => json!({"error":"app.invalidRequest"}),
        }
    }
    pub(crate) fn session(&mut self, input: &Value, wall: i64) -> Value {
        if input["op"] == "recordReady"
            && !self.pending.as_ref().is_some_and(|p| {
                p.phase == Phase::Media && p.record.is_some() && p.record == input["token"].as_u64()
            })
        {
            return json!({"accepted":false,"state":self.state.snapshot()});
        }
        let mut reply = self.state.request(input, wall);
        if input["op"] == "recordReady" && reply["accepted"] == true {
            if let (Some(id), Some(output)) = (input["id"].as_i64(), input["output"].as_u64()) {
                self.record_outputs.insert(id, output);
            }
        }
        if input["op"] == "recordEvent" && reply["terminal"] == true
            || input["op"] == "recordDone" && reply["completed"] == true
        {
            if let Some(output) = input["id"]
                .as_i64()
                .and_then(|id| self.record_outputs.remove(&id))
            {
                self.outputs.release(output);
            }
        }
        if matches!(input["op"].as_str(), Some("recordEvent" | "recordDone"))
            && reply["completed"] == true
        {
            reply["effects"] =
                json!([{"kind":"capturePipeline","op":"close","id":"screenshots#rec"}]);
        }
        if matches!(input["op"].as_str(), Some("recordReady" | "recordFailed"))
            && self
                .pending
                .as_ref()
                .is_some_and(|p| p.record.is_some() && p.record == input["token"].as_u64())
            && (reply["accepted"] == true || input["op"] == "recordFailed")
        {
            self.pending = None;
        }
        reply
    }
    fn effect(op: &str, pending: &Pending) -> Value {
        json!({"kind":"capturePipeline","op":op,"ticket":pending.ticket,"mode":pending.mode,"recordToken":pending.record})
    }
    fn cancel(&mut self, wall: i64, effects: &mut Vec<Value>) {
        if let Some(pending) = self.pending.take() {
            if let Some(token) = pending.record {
                self.state
                    .request(&json!({"op":"recordFailed","token":token}), wall);
            }
            effects.push(Self::effect("closeCountdown", &pending));
            self.restore(&pending, effects);
        }
    }
    fn restore(&self, pending: &Pending, effects: &mut Vec<Value>) {
        if let Some(id) = pending.restore_record {
            effects.push(json!({"kind":"capturePipeline","op":"restoreRecording","ticket":pending.ticket,"id":id}));
        }
    }
    pub(crate) fn command(
        &mut self,
        windows: &Session,
        input: &Value,
        wall: i64,
        now: Instant,
    ) -> Value {
        let mut effects = Vec::new();
        let mut result = json!({});
        match input["action"].as_str().unwrap_or("") {
            "bindings" => return json!({"result":axia_app_screenshots::shortcuts::bindings()}),
            "shortcutUsable" | "shortcut" => return self.shortcut(windows, input, wall, now),
            "refused" => return self.refused(windows, input, wall, now),
            "noticeAction" => {
                let valid = input["id"]
                    .as_u64()
                    .is_some_and(|id| self.refusal_ids.contains(&id))
                    && input["index"] == 0;
                return json!({"effects":if valid {vec![json!({"kind":"capturePipeline","op":"open","app":"settings","args":{"section":"privacy","nonce":wall}})]}else{Vec::new()}});
            }
            "start" => {
                let Some(mode) = input["mode"]
                    .as_str()
                    .filter(|m| ["full", "area", "window", "record"].contains(m))
                else {
                    return json!({"error":"app.invalidRequest"});
                };
                let record = if mode == "record" {
                    let reply = self.state.request(&json!({"op":"recordAction"}), wall);
                    match reply["action"].as_str() {
                        Some("stop") => {
                            return json!({"capture":self.state.snapshot(),"effects":[{"kind":"capturePipeline","op":"stop","id":reply["id"]}]});
                        }
                        Some("cancel") => {
                            self.cancel(wall, &mut effects);
                            return json!({"capture":self.state.snapshot(),"effects":effects});
                        }
                        _ => reply["token"].as_u64(),
                    }
                } else {
                    None
                };
                let restore_record = self.pending.as_mut().and_then(|p| p.restore_record.take());
                self.cancel(wall, &mut effects);
                // Cancel the old generation before installing this request. The
                // recordAction token above differs from every earlier token.
                let Some(ticket) = self
                    .generation
                    .checked_add(1)
                    .filter(|v| *v <= 9_007_199_254_740_991)
                else {
                    if let Some(token) = record {
                        self.state
                            .request(&json!({"op":"recordFailed","token":token}), wall);
                    }
                    return json!({"capture":self.state.snapshot(),"effects":effects,"error":"app.invalidRequest"});
                };
                self.generation = ticket;
                let state = windows.reader();
                let state = state.read();
                let restore_record = restore_record.or_else(|| {
                    state
                        .window("screenshots#rec")
                        .and_then(|_| self.state.snapshot()["recording"]["id"].as_i64())
                });
                let ids = state
                    .windows()
                    .filter(|w| w.app() == "screenshots" && !w.id().starts_with("screenshots#edit"))
                    .map(|w| w.id().to_owned())
                    .collect::<Vec<_>>();
                effects.push(json!({"kind":"capturePipeline","op":"hide","ids":ids}));
                self.pending = Some(Pending {
                    ticket,
                    mode: mode.into(),
                    record,
                    restore_record,
                    reported: false,
                    phase: Phase::Hide,
                    due: Some(
                        now + Duration::from_millis(if mode == "record" { 220 } else { 160 }),
                    ),
                });
                result["ticket"] = json!(ticket);
            }
            "valid" | "accept" => {
                let valid = self.pending.as_ref().is_some_and(|p| {
                    Some(p.ticket) == input["ticket"].as_u64() && p.phase == Phase::Media
                });
                result["valid"] = json!(valid);
                if valid
                    && input["action"] == "accept"
                    && self.pending.as_ref().is_some_and(|p| p.record.is_none())
                {
                    let pending = self.pending.take().unwrap();
                    self.restore(&pending, &mut effects);
                }
            }
            "restoreRecording" => {
                result["valid"] = json!(
                    self.pending.is_none()
                        && input["ticket"].as_u64() == Some(self.generation)
                        && input["id"].is_i64()
                        && input["id"] == self.state.snapshot()["recording"]["id"]
                );
            }
            "cancel" | "countClosed" => {
                if self.pending.as_ref().is_some_and(|p| {
                    Some(p.ticket) == input["ticket"].as_u64()
                        && (input["action"] == "cancel" || p.phase == Phase::Countdown)
                }) {
                    self.cancel(wall, &mut effects);
                }
            }
            _ => return json!({"error":"app.invalidRequest"}),
        }
        json!({"capture":self.state.snapshot(),"effects":effects,"result":result})
    }
    pub(crate) fn wait_ms(&self, now: Instant) -> Option<f64> {
        self.pending
            .as_ref()?
            .due
            .map(|due| due.saturating_duration_since(now).as_secs_f64() * 1000.)
    }
    pub(crate) fn allows(&self, effect: &Value) -> bool {
        let phase = match effect["op"].as_str() {
            Some("work") => Phase::Media,
            Some("countdown") => Phase::Countdown,
            _ => return true,
        };
        self.pending
            .as_ref()
            .is_some_and(|p| Some(p.ticket) == effect["ticket"].as_u64() && p.phase == phase)
    }
    pub(crate) fn tick(&mut self, windows: &Session, wall: i64, now: Instant) -> Vec<Value> {
        let Some(pending) = self
            .pending
            .as_mut()
            .filter(|p| p.due.is_some_and(|due| due <= now))
        else {
            return Vec::new();
        };
        match pending.phase {
            Phase::Hide => {
                let secs = self
                    .preferences
                    .as_ref()
                    .map(|p| p.read()["screenshots"]["timer"].as_u64().unwrap_or(0))
                    .unwrap_or(0);
                if [5, 10].contains(&secs) {
                    pending.phase = Phase::Countdown;
                    pending.due = Some(now + Duration::from_secs(secs));
                    let mut effect = Self::effect("countdown", pending);
                    effect["secs"] = json!(secs);
                    effect["start"] = json!(wall);
                    effect["returnWindow"] = json!(windows.reader().read().focused_window());
                    let displays = crate::native_view::services::displays::reader();
                    let displays = displays.read();
                    let display = displays
                        .iter()
                        .find(|d| d["main"] == true)
                        .or_else(|| displays.first());
                    let number = |k: &str, default: f64| {
                        display.and_then(|d| d[k].as_f64()).unwrap_or(default)
                    };
                    effect["frame"] = json!({"x":(number("x",0.)+number("w",760.)/2.-60.).round(),"y":(number("y",0.)+number("h",700.)/2.-60.).round(),"w":120,"h":120});
                    vec![effect]
                } else {
                    pending.phase = Phase::Media;
                    pending.due = None;
                    vec![Self::effect("work", pending)]
                }
            }
            Phase::Countdown => {
                pending.phase = Phase::Settle;
                pending.due = Some(now + Duration::from_millis(180));
                vec![Self::effect("closeCountdown", pending)]
            }
            Phase::Settle => {
                pending.phase = Phase::Media;
                pending.due = None;
                vec![Self::effect("work", pending)]
            }
            Phase::Media => Vec::new(),
        }
    }
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
