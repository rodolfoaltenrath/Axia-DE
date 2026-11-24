//! Session capture facts independent of mounted views. Recording generations
//! reject late permission/job results; recent results retain their bounded order.
use serde_json::{json, Value};
use std::collections::VecDeque;

struct Recording {
    id: i64,
    path: String,
    start: i64,
}
#[derive(Default)]
pub struct State {
    recording: Option<Recording>,
    pending: Option<u64>,
    token: u64,
    revision: u64,
    recent: VecDeque<Value>,
}
impl State {
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    fn recent(&mut self, path: &str, video: bool, now: i64) {
        if path.is_empty() {
            return;
        }
        self.recent
            .push_front(json!({"path":path,"video":video,"time":now}));
        self.recent.truncate(20);
    }
    pub fn snapshot(&self) -> Value {
        json!({"revision":self.revision,"recording":self.recording.as_ref().map(|r|json!({"id":r.id,"path":r.path,"start":r.start})),"pendingRecord":self.pending.is_some(),"recent":self.recent})
    }
    pub fn request(&mut self, input: &Value, now: i64) -> Value {
        let mut reply = json!({});
        let token = input["token"].as_u64();
        match input["op"].as_str().unwrap_or("") {
            "recordEvent" => {
                let kind = input["type"].as_str().unwrap_or("");
                let data = &input["data"];
                let terminal = matches!(kind, "done" | "closed")
                    || kind == "state" && matches!(data["state"].as_str(), Some("done" | "failed"));
                reply = if terminal {
                    self.request(&json!({"op":"recordDone","id":input["id"],"ok":kind=="done" && data["ok"]==true,"path":data["path"]}),now)
                } else {
                    json!({"completed":false})
                };
                reply["terminal"] = json!(terminal);
            }
            "recordAction" => {
                if let Some(rec) = &self.recording {
                    reply = json!({"action":"stop","id":rec.id});
                } else if self.pending.take().is_some() {
                    self.changed();
                    reply = json!({"action":"cancel"});
                } else {
                    self.token = self.token.wrapping_add(1);
                    self.pending = Some(self.token);
                    self.changed();
                    reply = json!({"action":"start","token":self.token});
                }
            }
            "recordCurrent" => {
                reply["valid"] = json!(self.pending.is_some() && self.pending == token)
            }
            "recordReady" => {
                let id = input["id"].as_i64().unwrap_or(0);
                let accepted = id > 0 && self.pending.is_some() && self.pending == token;
                if accepted {
                    self.pending = None;
                    self.recording = Some(Recording {
                        id,
                        path: input["path"].as_str().unwrap_or("").into(),
                        start: now,
                    });
                    self.changed();
                }
                reply["accepted"] = json!(accepted);
            }
            "recordFailed" => {
                if self.pending.is_some() && self.pending == token {
                    self.pending = None;
                    self.changed();
                }
            }
            "recordDone" => {
                let completed = self
                    .recording
                    .as_ref()
                    .is_some_and(|r| Some(r.id) == input["id"].as_i64());
                if completed {
                    let rec = self.recording.take().unwrap();
                    if input["ok"] == true {
                        self.recent(
                            input["path"]
                                .as_str()
                                .filter(|p| !p.is_empty())
                                .unwrap_or(&rec.path),
                            true,
                            now,
                        );
                    }
                    self.changed();
                }
                reply["completed"] = json!(completed);
            }
            "recentAdd" => {
                if let Some(path) = input["path"].as_str().filter(|p| !p.is_empty()) {
                    self.recent(path, input["video"] == true, now);
                    self.changed();
                }
            }
            _ => {}
        }
        reply["state"] = self.snapshot();
        reply
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_events_require_an_explicit_success_before_adding_a_video() {
        for (kind, data, success) in [
            ("state", json!({"state":"failed"}), false),
            ("state", json!({"state":"done"}), false),
            ("closed", json!({"crashed":true}), false),
            ("done", json!({"ok":false}), false),
            ("done", json!({"ok":true,"path":"/finished.mp4"}), true),
        ] {
            let mut state = State::default();
            let token = state.request(&json!({"op":"recordAction"}), 0)["token"].clone();
            state.request(
                &json!({"op":"recordReady","token":token,"id":7,"path":"/pending.mp4"}),
                1,
            );
            let progress=state.request(&json!({"op":"recordEvent","id":7,"type":"error","data":{"message":"media.noHelper"}}),2);
            assert_eq!(progress["terminal"], false);
            assert!(progress["state"]["recording"].is_object());
            let result = state.request(
                &json!({"op":"recordEvent","id":7,"type":kind,"data":data}),
                3,
            );
            assert_eq!(result["terminal"], true);
            assert_eq!(result["completed"], true);
            assert!(result["state"]["recording"].is_null());
            assert_eq!(state.recent.len(), usize::from(success));
            let duplicate = state.request(
                &json!({"op":"recordEvent","id":7,"type":"done","data":{"ok":true}}),
                4,
            );
            assert_eq!(duplicate["completed"], false);
            assert_eq!(state.recent.len(), usize::from(success));
        }
    }
    #[test]
    fn cancelled_permission_results_cannot_replace_a_new_recording() {
        let mut state = State::default();
        let old = state.request(&json!({"op":"recordAction"}), 1)["token"].clone();
        assert_eq!(
            state.request(&json!({"op":"recordAction"}), 2)["action"],
            "cancel"
        );
        let new = state.request(&json!({"op":"recordAction"}), 3)["token"].clone();
        assert_ne!(old, new);
        assert_eq!(
            state.request(&json!({"op":"recordReady","token":old,"id":4}), 4)["accepted"],
            false
        );
        state.request(&json!({"op":"recordFailed","token":old}), 5);
        assert_eq!(
            state.request(&json!({"op":"recordCurrent","token":new}), 6)["valid"],
            true
        );
        assert_eq!(
            state.request(
                &json!({"op":"recordReady","token":new,"id":5,"path":"/new.mp4"}),
                7
            )["accepted"],
            true
        );
        assert_eq!(state.request(&json!({"op":"recordAction"}), 8)["id"], 5);
        assert_eq!(
            state.request(
                &json!({"op":"recordDone","id":4,"ok":true,"path":"/old.mp4"}),
                9
            )["completed"],
            false
        );
        let done = state.request(&json!({"op":"recordDone","id":5,"ok":true}), 10);
        assert!(done["state"]["recording"].is_null());
        assert_eq!(done["state"]["recent"][0]["path"], "/new.mp4");
        state.request(&json!({"op":"recordDone","id":5,"ok":true}), 11);
        assert_eq!(state.recent.len(), 1);
    }
    #[test]
    fn history_is_bounded_ordered_and_has_native_completion_times() {
        let mut state = State::default();
        for i in 0..25 {
            state.request(&json!({"op":"recentAdd","path":format!("/{i}.png")}), i);
        }
        assert_eq!(state.recent.len(), 20);
        assert_eq!(state.recent[0]["time"], 24);
        assert_eq!(state.recent[19]["path"], "/5.png");
        let revision = state.revision;
        state.request(&json!({"op":"recentAdd","path":""}), 26);
        assert_eq!(state.revision, revision);
    }
}
