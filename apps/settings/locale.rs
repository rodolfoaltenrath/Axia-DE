use super::*;
impl App {
    pub(super) fn language(&self) -> Value {
        let u = self.ui();
        let languages = arr(self.d("languages"));
        let lang = self.i18n.borrow().lang.clone();
        let region = if self.region.is_empty() {
            languages
                .iter()
                .find(|l| l["id"] == lang)
                .map(|l| s(&l["region"]))
                .unwrap_or("")
        } else {
            &self.region
        };
        let name = languages
            .iter()
            .find(|l| l["id"] == lang)
            .map(|l| s(&l["name"]))
            .unwrap_or(&lang);
        let mut region_rows = vec![u.row(
            if region.is_empty() { "—" } else { region },
            "",
            vec![u.button(
                t("settings.change"),
                state("regionPicker", json!(!b(self.st("regionPicker")))),
                false,
                false,
            )],
        )];
        if b(self.st("regionPicker")) {
            region_rows.push(
                self.search_list(
                    "regionSearch",
                    arr(self.d("locale.available"))
                        .iter()
                        .map(|l| {
                            let name = s(&l["name"]).trim_end_matches(".UTF-8");
                            json!([name, name, if b(&l["generated"]) { "✓" } else { "" }])
                        })
                        .collect(),
                    op("regionPick"),
                    region,
                    180.,
                    "settings.region.search",
                    300,
                ),
            );
        }
        let tm = crate::clock::local(self.now);
        let date =
            crate::native_view::date_format::date(&mut self.i18n.borrow_mut(), region, &tm, "long");
        let time = crate::native_view::date_format::time(
            &mut self.i18n.borrow_mut(),
            region,
            s(self.pref("clock.hour24")),
            &tm,
            false,
        );
        let numeric = crate::native_view::date_format::date(
            &mut self.i18n.borrow_mut(),
            region,
            &tm,
            "numeric",
        );
        let number = self.i18n.borrow().number(1234567.89, 2);
        region_rows.push(u.row(
            t("settings.region.preview"),
            "",
            vec![view(
                json!({"alignItems":"flex-end","gap":2,"paddingVertical":4}),
                vec![
                    u.text(format!("{date} · {time}"), 13., true),
                    u.text(format!("{numeric} · {number}"), 13., true),
                ],
            )],
        ));
        let zone = s(self.d("time.zone"));
        let mut zones = vec![u.row(
            zone.rsplit('/').next().unwrap_or(zone).replace('_', " "),
            zone.replace('_', " "),
            vec![u.button(
                t("settings.change"),
                state("zonePicker", json!(!b(self.st("zonePicker")))),
                false,
                false,
            )],
        )];
        if b(self.st("zonePicker")) {
            zones.push(
                self.search_list(
                    "zoneSearch",
                    arr(self.d("time.zones"))
                        .iter()
                        .map(|z| json!([z, s(z).replace('_', " ")]))
                        .collect(),
                    op("zonePick"),
                    zone,
                    180.,
                    "settings.tz.search",
                    300,
                ),
            );
        }
        let system = self.d("locale.get")["system"]["LANG"]
            .as_str()
            .unwrap_or("C.UTF-8");
        let mut packs = vec![u.row(
            name,
            "",
            vec![u.button(
                tp("settings.pack.install", json!({"lang":name})),
                op("languagePack"),
                false,
                false,
            )],
        )];
        let found = arr(self.d("languagePacks"));
        if !found.is_empty() {
            packs.push(
                u.row(
                    t("settings.pack.lordos"),
                    found
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" · "),
                    found
                        .iter()
                        .map(|path| {
                            u.button(
                                tp(
                                    "settings.pack.add",
                                    json!({"file":s(path).rsplit('/').next().unwrap_or("")}),
                                ),
                                json!({"op":"packImport","path":path}),
                                false,
                                false,
                            )
                        })
                        .collect(),
                ),
            );
        }
        let mut out = vec![
            u.section(
                t("settings.lang.title"),
                t("settings.lang.hint"),
                languages
                    .iter()
                    .map(|l| {
                        click(
                            u.row(
                                l["name"].clone(),
                                format!(
                                    "{}{}",
                                    s(&l["english"]),
                                    if b(&l["user"]) {
                                        format!(" · {}", self.tr("settings.lang.pack"))
                                    } else {
                                        String::new()
                                    }
                                ),
                                vec![u.text(if l["id"] == lang { "✓" } else { "" }, 14., false)],
                            ),
                            json!({"op":"languagePick","value":l["id"]}),
                        )
                    })
                    .collect(),
            ),
            u.section(
                t("settings.region.title"),
                t("settings.region.hint"),
                region_rows,
            ),
            u.section(t("settings.tz.title"), "", zones),
            u.section(
                t("settings.system.title"),
                tp("settings.system.hint", json!({"name":system})),
                vec![u.row(
                    "LANG",
                    system,
                    vec![u.button(
                        tp(
                            "settings.system.apply",
                            json!({"name":if region.is_empty(){"—"}else{region}}),
                        ),
                        op("systemLocale"),
                        false,
                        false,
                    )],
                )],
            ),
            u.section(t("settings.pack.title"), t("settings.pack.hint"), packs),
        ];
        if !self.admin() {
            out.push(u.note(
                t(if self.standalone() {
                    "settings.host.adminUnavailable"
                } else {
                    "settings.admin.unavailable"
                }),
                "warn",
            ));
        }
        let busy = self.d("admin.busy");
        if !busy.is_null() {
            let mut lines=vec![u.text(tp("settings.admin.running",json!({"action":self.tr(&format!("settings.admin.{}",s(&busy["action"]))),"arg":busy["arg"]})),12.,false)];
            lines.extend(
                arr(&busy["log"])
                    .iter()
                    .rev()
                    .take(6)
                    .rev()
                    .map(|line| u.text(self.i18n.borrow_mut().status(s(line)), 11., true)),
            );
            out.push(u.section("", "", lines));
        }
        stack(out)
    }
    pub(super) fn find_packs(&mut self) {
        if !self.d("languagePacks").is_null() {
            return;
        }
        let mut found = vec![];
        for dir in ["Descargas", "Downloads", "."] {
            let path = std::path::Path::new(&self.home).join(dir);
            for entry in std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
            {
                let name = entry.file_name().to_string_lossy().into_owned();
                let id = name
                    .strip_prefix("lordos-lang-")
                    .and_then(|x| x.strip_suffix(".json"));
                if id.is_some_and(|s| {
                    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || "_-".contains(c))
                }) {
                    found.push(if dir == "." {
                        name
                    } else {
                        format!("{dir}/{name}")
                    });
                }
            }
        }
        self.data["languagePacks"] = json!(found);
    }
    fn use_region(&mut self, region: &str) {
        let name = format!("{region}.UTF-8");
        let generated = arr(self.d("locale.available"))
            .iter()
            .any(|l| l["name"] == name && b(&l["generated"]));
        if generated {
            self.apply_region(region);
        } else {
            self.admin_run("locale-gen",name,false,json!({"op":"regionGenerated","region":region,"success":tp("settings.region.generated",json!({"name":region}))}));
        }
    }
    fn apply_region(&mut self, region: &str) {
        self.call(
            "locale.set",
            json!([{"LANG":format!("{region}.UTF-8")}]),
            json!({"op":"regionApplied","region":region}),
        );
    }
    pub(super) fn locale_command(&mut self, c: &Value) -> bool {
        match s(&c["op"]) {
            "zonePick" => {
                let zone = s(&c["value"]);
                if arr(self.d("time.zones")).contains(&c["value"]) {
                    self.state["zonePicker"] = json!(false);
                    self.admin_run("timezone", zone.into(), false, json!({"op":"zoneChanged"}));
                }
            }
            "languagePick" => {
                let lang = arr(self.d("languages"))
                    .iter()
                    .find(|l| l["id"] == c["value"])
                    .cloned();
                if let Some(lang) = lang {
                    self.call("language.set", json!([lang["id"]]), Value::Null);
                    if s(self.pref("region")).is_empty() && !s(&lang["region"]).is_empty() {
                        self.use_region(s(&lang["region"]));
                    }
                }
            }
            "regionPick" => {
                let region = s(&c["value"]);
                if arr(self.d("locale.available"))
                    .iter()
                    .any(|l| s(&l["name"]).trim_end_matches(".UTF-8") == region)
                {
                    self.state["regionPicker"] = json!(false);
                    self.use_region(region);
                }
            }
            "systemLocale" => {
                let region = self.region.clone();
                if !region.is_empty() {
                    self.admin_run("system-locale", format!("{region}.UTF-8"), false, json!({}));
                }
            }
            "languagePack" => {
                let lang = self
                    .i18n
                    .borrow()
                    .lang
                    .split('-')
                    .next()
                    .unwrap_or("en")
                    .to_owned();
                let region = self.region.clone();
                self.admin_run(
                    "langpack",
                    if region.starts_with(&format!("{lang}_")) {
                        region
                    } else {
                        lang
                    },
                    false,
                    json!({}),
                );
            }
            "packImport" => {
                let path = s(&c["path"]);
                if !arr(self.d("languagePacks")).contains(&c["path"]) {
                    return true;
                }
                let file = std::path::Path::new(&self.home).join(path);
                let result = std::fs::read(file)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
                if let Some(pack) = result {
                    let mut files = json!({"meta":serde_json::to_string_pretty(&pack["meta"]).unwrap_or_default()});
                    if let Some(catalogs) = pack["catalogs"].as_object() {
                        for (ns, obj) in catalogs {
                            files[ns] =
                                json!(serde_json::to_string_pretty(obj).unwrap_or_default());
                        }
                    }
                    self.call(
                        "locale.installPack",
                        json!([pack["meta"]["id"], files]),
                        json!({"op":"packInstalled","name":pack["meta"]["name"]}),
                    );
                } else {
                    self.message(false, t("settings.pack.invalid"));
                }
            }
            _ => return false,
        }
        true
    }
    pub(super) fn locale_after(&mut self, after: &Value, value: &Value) {
        match s(&after["op"]) {
            "regionGenerated" => self.apply_region(s(&after["region"])),
            "regionApplied" => {
                self.call("keyboard.reloadCompose", json!([]), Value::Null);
                self.patch("region", after["region"].clone());
            }
            "packInstalled" => {
                if value == true {
                    self.call("locale.reload", json!([]), Value::Null);
                    self.message(
                        true,
                        tp("settings.pack.imported", json!({"name":after["name"]})),
                    );
                } else {
                    self.message(false, t("settings.pack.invalid"));
                }
            }
            _ => {}
        }
    }
}
