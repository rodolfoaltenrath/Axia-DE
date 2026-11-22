use super::*;
impl App {
    pub(super) fn privacy(&mut self) -> Value {
        let u = self.ui();
        let mut out = vec![u.note(t("privacy.intro"), "")];
        let indicators = arr(self.d("privacy.indicators"));
        if !indicators.is_empty() {
            out.push(
                u.section(
                    "",
                    "",
                    indicators
                        .iter()
                        .map(|i| {
                            u.row(
                                format!(
                                    "{}: {}",
                                    self.tr(&format!("privacy.inUse.{}", s(&i["kind"]))),
                                    arr(&i["apps"])
                                        .iter()
                                        .filter_map(Value::as_str)
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                                "",
                                vec![],
                            )
                        })
                        .collect(),
                ),
            );
        }
        if self.privacy.is_empty() {
            for manifest in arr(&crate::native_apps::manifests()) {
                for section in arr(&manifest["privacy"]) {
                    if let Some(mut app) = crate::native_view::app(s(&manifest["id"])) {
                        app.resources(&self.assets, Some(&self.home));
                        self.privacy.push((s(&section["section"]).into(), app));
                    }
                }
            }
        }
        for index in 0..self.privacy.len() {
            let section = self.privacy[index].0.clone();
            let mut services = self.d("services").clone();
            if !services.is_object() {
                services = json!({});
            }
            services["fw"] = json!({"status":self.d("fw.status"),"state":self.d("fw.state"),"poll":self.d("fw.poll"),"lists":self.d("fw.lists")});
            let input = json!({"cmd":"sync","args":{"section":section},"context":{"theme":self.theme.as_ref(),"width":self.st("contentWidth"),"decimal":self.i18n.borrow().decimal,"group":self.i18n.borrow().group},"settings":self.settings,"services":services,"catalog":self.d("catalog"),"lang":self.i18n.borrow().lang});
            let reply = self.privacy[index].1.request(&input);
            if !reply["tree"].is_null() {
                self.data[format!("privacyTree{index}")] = wrap(reply["tree"].clone(), index);
            }
            self.collect_privacy(index, reply);
            out.push(self.d(&format!("privacyTree{index}")).clone());
        }
        for (perm, glyph, color) in [
            ("camera", "camera", "#30d158"),
            ("microphone", "microphone", "#ff9f0a"),
            ("screen", "▭", "#0a84ff"),
            ("files", "folder", "#30b0c7"),
            ("network", "⇅", "#0a84ff"),
            ("clipboard", "⎘", "#5e5ce6"),
            ("notifications", "◉", "#ff375f"),
            ("apps", "▦", "#8e8e93"),
            ("listen", "⇄", "#34c759"),
            ("usb", "⇆", "#8e8e93"),
            ("bluetooth", "ᛒ", "#0a84ff"),
        ] {
            let decisions = arr(self.d("perms.list"));
            let mut apps = std::collections::BTreeMap::new();
            for a in arr(self.d("packages.list")) {
                if arr(&a["permissions"]).contains(&json!(perm)) {
                    apps.insert(s(&a["id"]).to_owned(), s(&a["name"]).to_owned());
                }
            }
            for d in decisions.iter().filter(|d| d["perm"] == perm) {
                let id = s(&d["app"]);
                let name = arr(self.d("registry"))
                    .iter()
                    .find(|a| a["id"] == id)
                    .map(|a| s(&a["name"]))
                    .unwrap_or(id);
                apps.insert(id.into(), name.into());
            }
            if apps.is_empty() {
                continue;
            }
            let rows = apps
                .into_iter()
                .map(|(id, name)| {
                    let choice = decisions
                        .iter()
                        .find(|d| d["perm"] == perm && d["app"] == id)
                        .map(|d| d["state"].clone())
                        .unwrap_or(json!("ask"));
                    u.row(
                        name,
                        "",
                        vec![u.segmented(
                            choice,
                            vec![
                                json!(["allow", t("privacy.allowShort")]),
                                json!(["ask", t("privacy.askShort")]),
                                json!(["deny", t("privacy.denyShort")]),
                            ],
                            json!({"op":"call","name":"perms.set","args":[id,perm]}),
                        )],
                    )
                })
                .collect();
            let section = u.section(
                t(&format!("privacy.perm.{perm}")),
                t(&format!("privacy.permHint.{perm}")),
                rows,
            );
            out.push(u.section_icon(section, system::badge(glyph, color, 22.)));
        }
        stack(out)
    }
    pub(super) fn collect_privacy(&mut self, index: usize, reply: Value) {
        let mut effects = arr(&reply["effects"]).to_vec();
        if !reply["effect"].is_null() {
            effects.push(reply["effect"].clone());
        }
        for mut effect in effects {
            effect["__settingsPrivacy"] = json!(index);
            self.effects.push_back(effect);
        }
    }
    pub(super) fn privacy_action(&mut self, v: &Value) {
        let Ok(action) = serde_json::from_str::<Value>(s(&v["key"])) else {
            return;
        };
        let index = n(&action["index"]) as usize;
        if index >= self.privacy.len() {
            return;
        }
        let mut input = v.clone();
        input["cmd"] = action["action"][0].clone();
        input["key"] = action["action"][1].clone();
        let reply = self.privacy[index].1.request(&input);
        self.collect_privacy(index, reply);
    }
    pub(super) fn defaultapps(&self) -> Value {
        let u = self.ui();
        let mut rows = vec![];
        for id in [
            "browser",
            "mail",
            "music",
            "video",
            "images",
            "pdf",
            "text",
            "terminal",
            "screenshots",
        ] {
            let info = self.d(&format!("default:{id}"));
            let candidates = arr(self.d(&format!("candidates:{id}")));
            let current = if info["auto"] == false {
                s(&info["id"])
            } else {
                ""
            };
            let mut options: Vec<Value> = candidates
                .iter()
                .map(|a| {
                    let desktop = s(&a["id"]);
                    let app = if b(&a["native"]) {
                        desktop
                            .strip_prefix("lordos-")
                            .and_then(|v| v.strip_suffix(".desktop"))
                            .unwrap_or(desktop)
                    } else {
                        arr(self.d("catalog"))
                            .iter()
                            .find(|v| format!("{}.desktop", s(&v["desktopId"])) == desktop)
                            .map(|v| s(&v["id"]))
                            .unwrap_or(desktop)
                    };
                    json!([a["id"],a["name"],{"kind":"appIcon","app":app,"size":16}])
                })
                .collect();
            if !current.is_empty() && !options.iter().any(|o| o[0] == current) {
                options.push(json!([
                    current,
                    tp(
                        "prefs.defaults.other",
                        json!({"id":current.trim_end_matches(".desktop")})
                    )
                ]));
            }
            if id != "terminal" || current.is_empty() {
                let first = if info["auto"] == true {
                    let name = candidates
                        .iter()
                        .find(|c| c["id"] == info["id"])
                        .map(|c| s(&c["name"]))
                        .unwrap_or(s(&info["id"]));
                    tp("prefs.defaults.automatic", json!({"name":name}))
                } else if !current.is_empty() {
                    t("prefs.defaults.automaticPlain")
                } else {
                    t("prefs.defaults.choose")
                };
                options.insert(0, json!(["", first]));
            }
            rows.push(u.row(
                t(&format!("prefs.defaults.cat.{id}")),
                if candidates.is_empty() && info.is_null() {
                    t("prefs.defaults.none")
                } else {
                    json!("")
                },
                if candidates.is_empty() && info.is_null() {
                    vec![]
                } else {
                    vec![u.pop(
                        &format!("default-{id}"),
                        json!(current),
                        options,
                        json!({"op":"defaultPick","category":id}),
                        230.,
                        false,
                    )]
                },
            ));
        }
        for (row, (glyph, color)) in rows.iter_mut().zip([
            ("◎", "#0a84ff"),
            ("✉", "#30a0ff"),
            ("♪", "#ff375f"),
            ("▶", "#bf5af2"),
            ("▣", "#ff9f0a"),
            ("⎙", "#ff453a"),
            ("✎", "#8e8e93"),
            (">_", "#2c2c2e"),
            ("⬚", "#30b0c7"),
        ]) {
            *row = u.row_icon(row.take(), system::badge(glyph, color, 26.));
        }
        let mut out = vec![u.section("", t("prefs.defaults.footer"), rows)];
        if !self.d("defaults.screenshotLauncher").is_null() {
            out.push(u.note(t("prefs.defaults.screenshotsExternal"), "warn"));
        }
        out.push(u.note(t("prefs.defaults.filesHint"), ""));
        stack(out)
    }
    pub(super) fn default_command(&mut self, c: &Value) -> bool {
        if c["op"] != "defaultPick" {
            return false;
        }
        let category = s(&c["category"]);
        if ![
            "browser",
            "mail",
            "music",
            "video",
            "images",
            "pdf",
            "text",
            "terminal",
            "screenshots",
        ]
        .contains(&category)
        {
            return true;
        }
        if s(&c["value"]).is_empty() {
            self.call(
                "defaults.reset",
                json!([category]),
                json!({"op":"defaultPicked"}),
            );
        } else {
            self.call(
                "defaults.set",
                json!([category, c["value"]]),
                json!({"op":"defaultPicked"}),
            );
        }
        true
    }
}
fn wrap(mut node: Value, index: usize) -> Value {
    if !node.is_object() {
        return node;
    }
    for key in ["action", "change", "submit", "cancel"] {
        if node[key].is_array() {
            node[key] = json!([
                "privacyAction",
                json!({"index":index,"action":node[key]}).to_string()
            ]);
        }
    }
    if let Some(events) = node["events"].as_object_mut() {
        for (_, a) in events {
            *a = json!([
                "privacyAction",
                json!({"index":index,"action":a}).to_string()
            ]);
        }
    }
    if let Some(children) = node["children"].as_array_mut() {
        for child in children {
            *child = wrap(child.take(), index);
        }
    }
    node
}
