use super::*;
impl Service {
    pub(super) fn sync_presentation(&mut self) {
        let locale = crate::locale::reader(&self.home, &self.assets);
        let locale = locale.read();
        if self.output_revision != Some(locale.revision) {
            self.output_i18n.invalidate();
            self.output_revision = Some(locale.revision);
        }
        self.output_i18n.lang = locale.lang.clone();
    }
    pub(super) fn shortcut(
        &mut self,
        windows: &Session,
        input: &Value,
        wall: i64,
        now: Instant,
    ) -> Value {
        let key = match input["key"].as_str() {
            Some(
                key @ ("capture.cancel" | "capture.full" | "capture.area" | "capture.selector"
                | "capture.panel"),
            ) => key,
            _ => return json!({"error":"app.invalidRequest"}),
        };
        let snapshot = self.state.snapshot();
        let active = snapshot["recording"].is_object() || snapshot["pendingRecord"] == true;
        let external = self.preferences.as_ref().and_then(|prefs| {
            axia_app_screenshots::shortcuts::external(&prefs.read()["screenshotApp"])
        });
        let count = windows
            .reader()
            .read()
            .window("screenshots#count")
            .is_some();
        let enabled = if key == "capture.cancel" {
            count
        } else {
            external.is_some() || input["media"] == true || key == "capture.panel" && active
        };
        if input["action"] == "shortcutUsable" {
            return json!({"result":{"enabled":enabled}});
        }
        if !enabled {
            return json!({"effects":[]});
        }
        if key == "capture.cancel" {
            let ticket = windows
                .reader()
                .read()
                .window("screenshots#count")
                .and_then(|w| w.args()["captureTicket"].as_u64());
            return if let Some(ticket) = ticket {
                self.command(
                    windows,
                    &json!({"action":"cancel","ticket":ticket}),
                    wall,
                    now,
                )
            } else {
                json!({"effects":[{"kind":"capturePipeline","op":"close","id":"screenshots#count"}]})
            };
        }
        if key == "capture.panel" && active {
            return self.command(
                windows,
                &json!({"action":"start","mode":"record"}),
                wall,
                now,
            );
        }
        if let Some(app) = external {
            return json!({"effects":[{"kind":"capturePipeline","op":"launch","app":app}]});
        }
        if key == "capture.panel" {
            return json!({"effects":[{"kind":"capturePipeline","op":"open","app":"screenshots","args":{}}]});
        }
        self.command(
            windows,
            &json!({"action":"start","mode":if key=="capture.full"{"full"}else{"area"}}),
            wall,
            now,
        )
    }
    pub(super) fn refused(
        &mut self,
        windows: &Session,
        input: &Value,
        wall: i64,
        now: Instant,
    ) -> Value {
        let Some(pending) = self.pending.as_mut().filter(|p| {
            Some(p.ticket) == input["ticket"].as_u64() && p.phase == Phase::Media && !p.reported
        }) else {
            return json!({"effects":[]});
        };
        pending.reported = true;
        self.sync_presentation();
        let notice =
            axia_app_screenshots::feedback::refusal(&mut self.output_i18n, &input["error"]);
        let service = windows.notification_service();
        let mut service = service.borrow_mut();
        let prefs = service.command(&json!({"action":"appPreferences","app":"screenshots"}), now)
            ["result"]
            .clone();
        let local = axia_clock::local(wall);
        let dnd = service.command(
            &json!({"action":"policy","wallMs":wall,"minute":local.tm_hour*60+local.tm_min}),
            now,
        )["result"]
            == true;
        let reply = service.command(&json!({"action":"push","notice":notice}), now);
        let id = reply["result"].as_u64().unwrap_or(0);
        if id > 0 {
            self.refusal_ids.insert(id);
            while self.refusal_ids.len() > 64 {
                self.refusal_ids.pop_first();
            }
        }
        let mut effects =
            vec![json!({"kind":"capturePipeline","op":"notifications","effects":reply["effects"]})];
        if id == 0 || dnd || prefs["style"] == "none" {
            effects.push(json!({"kind":"capturePipeline","op":"toast","text":notice["body"]}));
        }
        json!({"effects":effects})
    }
}
