//! Context-owned notification delivery, history, grouping and monotonic expiry.
use axia_state::{Reader, Store};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const MAX_SHOWN: usize = 4;
const HISTORY: usize = 50;

#[derive(Clone)]
pub struct Notice {
    pub data: Value,
    until: Option<Instant>,
}
impl Notice {
    pub fn id(&self) -> u64 {
        self.data["id"].as_u64().unwrap()
    }
    fn group_key(&self) -> &str {
        if self.data["app"] == "" {
            self.data["label"].as_str().unwrap_or("")
        } else {
            self.data["app"].as_str().unwrap_or("")
        }
    }
}
#[derive(Default)]
pub struct State {
    pub shown: Vec<Notice>,
    pub history: Vec<Value>,
    pub ungrouped: bool,
    pub bottom: bool,
}
impl State {
    pub fn grouped(&self) -> Vec<(&Notice, usize)> {
        let mut out: Vec<(&Notice, usize)> = Vec::new();
        for notice in &self.shown {
            if let Some((_, more)) = out
                .iter_mut()
                .find(|(n, _)| !self.ungrouped && n.group_key() == notice.group_key())
            {
                *more += 1;
            } else {
                out.push((notice, 0));
            }
        }
        if self.bottom {
            out.reverse();
        }
        out
    }
    fn snapshot(&self) -> Value {
        json!({"shown":self.shown.iter().map(|n|&n.data).collect::<Vec<_>>(),"history":self.history,
            "placement":if self.bottom {"bottom"} else {"top"}})
    }
}

#[derive(Default)]
pub struct Service {
    state: Store<State>,
    next: u64,
    sent: Option<u64>,
    effects: Vec<Value>,
}

// Wall time is used for user schedules/history; elapsed banner time is monotonic.
pub fn dnd(prefs: &Value, wall_ms: f64, minute: f64) -> bool {
    if prefs["dnd"] == true || prefs["dnd"].as_f64().is_some_and(|until| wall_ms < until) {
        return true;
    }
    let schedule = &prefs["schedule"];
    if schedule["on"] != true {
        return false;
    }
    let (Some(from), Some(to)) = (schedule["from"].as_f64(), schedule["to"].as_f64()) else {
        return false;
    };
    let len = (to - from).rem_euclid(1440.);
    len > 0. && (minute - from).rem_euclid(1440.) < len
}

impl Service {
    pub fn reader(&self) -> Reader<State> {
        self.state.reader()
    }
    pub fn wait_ms(&self, now: Instant) -> Option<f64> {
        self.state
            .read()
            .shown
            .iter()
            .filter_map(|n| n.until)
            .min()
            .map(|until| until.saturating_duration_since(now).as_secs_f64() * 1000. + 0.01)
    }
    pub fn configure(&self, ungrouped: bool) {
        self.state.update(|s| {
            let changed = s.ungrouped != ungrouped;
            s.ungrouped = ungrouped;
            ((), changed)
        });
    }
    pub fn expire(&mut self, now: Instant) {
        let ids: Vec<_> = self
            .state
            .read()
            .shown
            .iter()
            .filter(|n| n.until.is_some_and(|until| now >= until))
            .map(Notice::id)
            .collect();
        if ids.is_empty() {
            return;
        }
        self.state.update(|s| {
            s.shown.retain(|n| !ids.contains(&n.id()));
            ((), true)
        });
        self.effects.extend(
            ids.into_iter()
                .map(|id| json!({"kind":"notificationClose","id":id,"reason":"closed"})),
        );
    }
    fn dismiss(&mut self, id: u64, reason: &str) {
        let removed = self.state.update(|s| {
            let before = s.shown.len();
            s.shown.retain(|n| n.id() != id);
            let changed = before != s.shown.len();
            (changed, changed)
        });
        if removed {
            self.effects
                .push(json!({"kind":"notificationClose","id":id,"reason":reason}));
        }
    }
    pub fn effects(&mut self) -> Vec<Value> {
        let mut effects = Vec::new();
        if self.sent != Some(self.state.revision()) {
            self.sent = Some(self.state.revision());
            effects.push(json!({"kind":"notificationState","state":self.state.read().snapshot()}));
        }
        effects.append(&mut self.effects);
        effects
    }
    pub fn command(&mut self, input: &Value, now: Instant) -> Value {
        self.expire(now);
        let result = match input["action"].as_str() {
            Some("policy") => json!(dnd(
                &input["prefs"],
                input["wallMs"].as_f64().unwrap_or(0.),
                input["minute"].as_f64().unwrap_or(0.)
            )),
            Some("push") => {
                let n = &input["notice"];
                let prefs = &input["prefs"];
                let ap = &input["appPrefs"];
                let app = n["app"].as_str().unwrap_or("");
                // Remember attempted apps even when permission/firewall denies delivery.
                let mut seen: Vec<Value> = prefs["seen"].as_array().cloned().unwrap_or_default();
                if !app.is_empty() && !seen.iter().any(|id| id == app) {
                    seen.insert(0, json!(app));
                    seen.truncate(64);
                    let mut patch = prefs.clone();
                    if !patch.is_object() {
                        patch = json!({});
                    }
                    patch["seen"] = json!(seen);
                    self.effects
                        .push(json!({"kind":"settings","patch":{"notifications":patch}}));
                }
                if ap["allow"] == false || input["blocked"] == true {
                    json!(0)
                } else {
                    self.next = self.next.checked_add(1).expect("notification ID exhausted");
                    let id = self.next;
                    let actions: Vec<_>=n["actions"].as_array().into_iter().flatten().take(3).map(|a|json!({"id":a["id"],"label":a["label"],"primary":a["primary"]==true})).collect();
                    let sticky = n["sticky"] == true || ap["style"] == "alert";
                    let data = json!({"id":id,"app":app,"label":n["label"].as_str().unwrap_or(""),
                        "displayLabel":n["displayLabel"].as_str().unwrap_or(""),"hasApp":n["hasApp"]==true,
                        "title":n["title"].as_str().unwrap_or(""),"body":n["body"].as_str().unwrap_or(""),
                        "actions":actions,"sticky":sticky,"time":input["wallMs"],"timeLabel":n["timeLabel"].as_str().unwrap_or("")});
                    let visible = ap["style"] != "none"
                        && !dnd(
                            prefs,
                            input["wallMs"].as_f64().unwrap_or(0.),
                            input["minute"].as_f64().unwrap_or(0.),
                        );
                    let timeout = n["timeout"]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .unwrap_or(6000.)
                        .clamp(1500., i32::MAX as f64);
                    self.state.update(|s| {
                        s.history.insert(0, data.clone());
                        s.history.truncate(HISTORY);
                        if visible {
                            s.shown.insert(
                                0,
                                Notice {
                                    data,
                                    until: (!sticky)
                                        .then(|| now + Duration::from_millis(timeout as u64)),
                                },
                            );
                            s.shown.truncate(MAX_SHOWN);
                        }
                        ((), true)
                    });
                    if visible && n["sound"] != false && ap["sound"] != false {
                        self.effects.push(json!({"kind":"notificationSound"}));
                    }
                    json!(id)
                }
            }
            Some("dismiss") => {
                let Some(id) = input["id"].as_u64() else {
                    return json!({"error":"app.invalidRequest"});
                };
                self.dismiss(id, input["reason"].as_str().unwrap_or("closed"));
                Value::Null
            }
            Some("act") => {
                let (Some(id), Some(index)) = (input["id"].as_u64(), input["index"].as_u64())
                else {
                    return json!({"error":"app.invalidRequest"});
                };
                let action = {
                    let s = self.state.read();
                    let notice = s
                        .shown
                        .iter()
                        .find(|n| n.id() == id)
                        .map(|n| &n.data)
                        .or_else(|| {
                            (input["history"] == true)
                                .then(|| s.history.iter().find(|n| n["id"] == id))
                                .flatten()
                        });
                    notice
                        .and_then(|n| n["actions"].as_array())
                        .and_then(|a| a.get(usize::try_from(index).ok()?))
                        .cloned()
                };
                if let Some(action) = action {
                    self.dismiss(
                        id,
                        action["id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .unwrap_or("action"),
                    );
                    self.effects
                        .push(json!({"kind":"notificationAction","id":id,"index":index}));
                }
                Value::Null
            }
            Some("clearHistory") => {
                self.state.update(|s| {
                    let changed = !s.history.is_empty();
                    s.history.clear();
                    ((), changed)
                });
                Value::Null
            }
            Some("placement") => {
                self.state.update(|s| {
                    let bottom = input["placement"] == "bottom";
                    let changed = s.bottom != bottom;
                    s.bottom = bottom;
                    ((), changed)
                });
                Value::Null
            }
            Some("snapshot") => self.state.read().snapshot(),
            _ => return json!({"error":"app.invalidRequest"}),
        };
        json!({"result":result,"effects":self.effects()})
    }
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;
