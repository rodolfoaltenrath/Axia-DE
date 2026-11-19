//! Graceful quit: select a live group once, resolve each remaining target when used.
//! JS still runs close guards and broker IPC until their services migrate.
use crate::{forcequit::App, winmodel::Session};
use serde_json::{json, Value};
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct GracefulQuit {
    ticket: u64,
    key: String,
    ids: VecDeque<String>,
    package_pending: bool,
    error: String,
}
impl GracefulQuit {
    pub fn request(&mut self, input: &Value, windows: &Session, app: &App) -> Value {
        if input["cmd"] == "quitApp" {
            // Keep tickets exactly representable in the transitional JS adapter.
            if self.ticket >= 9_007_199_254_740_991 {
                return json!({"error":"app.invalidRequest"});
            }
            self.ticket += 1;
            self.ids.clear();
            self.error.clear();
            self.package_pending = false;
            let groups = app.command_groups();
            let reader = windows.reader();
            let state = reader.read();
            let Some(group) = super::selected_group(&groups, &state, input) else {
                return self.finished();
            };
            self.key.clone_from(&group.key);
            self.ids.extend(group.windows.iter().map(|w| w.id.clone()));
            let package = group
                .windows
                .iter()
                .filter_map(|w| state.window(&w.id))
                .find_map(|w| {
                    (w.args()["pkg"] == true)
                        .then(|| w.args()["appId"].as_str())
                        .flatten()
                        .filter(|id| !id.is_empty())
                        .map(str::to_owned)
                });
            if let Some(package) = package {
                self.package_pending = true;
                return json!({"ticket":self.ticket,"effect":{"kind":"quitPackage","app":package}});
            }
        } else {
            if input["ticket"].as_u64() != Some(self.ticket) {
                return json!({"effect":null});
            }
            if self.package_pending {
                self.package_pending = false;
                if input["result"] == true {
                    self.ids.clear();
                    return self.finished();
                }
            } else if let Some(error) = input["result"].as_str().filter(|error| !error.is_empty()) {
                if self.error.is_empty() {
                    self.error = error.to_owned();
                }
            }
        }
        self.advance(windows, app)
    }
    fn finished(&self) -> Value {
        json!({"ticket":self.ticket,"effect":null,"error":self.error})
    }
    fn advance(&mut self, windows: &Session, app: &App) -> Value {
        let groups = app.command_groups();
        let group = groups.iter().find(|group| group.key == self.key);
        let reader = windows.reader();
        let state = reader.read();
        while let Some(id) = self.ids.pop_front() {
            if !group.is_some_and(|group| group.windows.iter().any(|w| w.id == id)) {
                continue;
            }
            let Some(window) = state.window(&id) else {
                continue;
            };
            let surface = id
                .starts_with("linux#")
                .then(|| {
                    window.args()["surface"]
                        .as_i64()
                        .and_then(|surface| i32::try_from(surface).ok())
                        .filter(|surface| *surface > 0)
                })
                .flatten();
            let effect = surface.map_or_else(
                || json!({"kind":"requestCloseWindow","id":id}),
                |surface| json!({"kind":"closeSurface","surface":surface}),
            );
            return json!({"ticket":self.ticket,"effect":effect});
        }
        self.finished()
    }
}
