use super::*;
impl App {
    pub(super) fn zone_items(&self) -> Vec<String> {
        let needle = s(self.st("zoneSearch"))
            .trim()
            .to_lowercase()
            .replace(' ', "_");
        let mut zones: Vec<_> = arr(self.d("time.zones"))
            .iter()
            .filter_map(Value::as_str)
            .filter(|zone| needle.is_empty() || zone.to_lowercase().contains(&needle))
            .map(String::from)
            .collect();
        zones.sort();
        zones.truncate(400);
        zones
    }
    fn zone_picker(&self, current: &str) -> Value {
        let u = self.ui();
        view(
            json!({"gap":8,"paddingVertical":8}),
            vec![
                json!({"kind":"input","key":"zone-search","value":self.st("zoneSearch").as_str().unwrap_or(""),"change":["field","zoneSearch"],"placeholder":t("prefs.time.searchZone"),"autoFocus":true}),
                view(
                    json!({"height":220,"borderRadius":8,"border":format!("1px solid {}",s(&u.color("separator")))}),
                    vec![
                        json!({"kind":"scroll","style":{"padding":4,"flex":1},"children":self.zone_items().iter().map(|zone|{let on=zone==current;let(city,region)=zone.rsplit_once('/').map(|(r,c)|(c,r)).unwrap_or((zone.as_str(),""));let mut name=u.text(city.replace('_'," "),12.,false);merge(&mut name["style"],&json!({"flex":1,"whiteSpace":"nowrap"}));let area=u.text(region.replace('/'," / ").replace('_'," "),11.,true);let offset=self.d(&format!("zoneOffset:{zone}"))["offset"].as_i64().map(offset_text).unwrap_or_default();let mut offset=u.text(offset,11.,true);merge(&mut offset["style"],&json!({"width":84,"textAlign":"end","direction":"ltr"}));let mut labels=vec![name,area,offset];if on{for label in &mut labels{label["style"]["color"]=json!("#fff");}}click(view(json!({"height":26,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":8,"borderRadius":5,"backgroundColor":if on{u.color("accent")}else{json!("transparent")}}),labels),json!({"op":"zonePick","value":zone}))}).collect::<Vec<_>>()}),
                    ],
                ),
            ],
        )
    }
    pub(super) fn bluetooth(&self) -> Value {
        let u = self.ui();
        let st = self.d("bt.state");
        let a = &st["adapter"];
        let icon = system::badge("ᛒ", "#0a6cd6", 44.);
        if st.is_null() || !b(&st["available"]) || a.is_null() {
            return stack(vec![u.header(
                icon,
                t("prefs.bt.title"),
                t(if b(&st["available"]) {
                    "prefs.bt.noAdapter"
                } else {
                    "prefs.bt.noService"
                }),
                vec![],
            )]);
        }
        let powered = b(&a["powered"]);
        let mut out=vec![u.header(icon,t("prefs.bt.title"),if powered{tp("prefs.bt.visibleAs",json!({"name":a["name"].as_str().filter(|s|!s.is_empty()).unwrap_or(s(&a["address"]))}))}else{t("prefs.bt.offHint")},vec![u.toggle(powered,call("bt.power",json!([!powered])))])];
        let error = s(self.st("btError"));
        if error == "blocked" {
            out.push(u.section(
                "",
                "",
                vec![u.row(
                    t("prefs.bt.err.blocked"),
                    "",
                    vec![u.button(t("prefs.bt.unblock"), op("btUnblock"), false, false)],
                )],
            ));
        } else if !error.is_empty() {
            out.push(u.note(t(&format!("prefs.bt.err.{error}")), "warn"));
        }
        if powered {
            let mut mine: Vec<_> = arr(&st["devices"])
                .iter()
                .filter(|d| b(&d["paired"]))
                .collect();
            mine.sort_by_key(|d| (!b(&d["connected"]), s(&d["name"]).to_lowercase()));
            let mut near: Vec<_> = arr(&st["devices"])
                .iter()
                .filter(|d| !b(&d["paired"]) && (b(&d["named"]) || b(&d["busy"])))
                .collect();
            near.sort_by(|x, y| {
                y["rssi"]
                    .as_f64()
                    .unwrap_or(-200.)
                    .total_cmp(&x["rssi"].as_f64().unwrap_or(-200.))
            });
            let devices = |list: Vec<&Value>, empty: &str| {
                if list.is_empty() {
                    vec![u.row(t(empty), "", vec![])]
                } else {
                    list.into_iter().map(|d| self.bt_device(d)).collect()
                }
            };
            out.push(u.section(
                t("prefs.bt.myDevices"),
                "",
                devices(mine, "prefs.bt.noDevices"),
            ));
            out.push(u.section(
                t(if b(&a["discovering"]) {
                    "prefs.bt.nearbySearching"
                } else {
                    "prefs.bt.nearby"
                }),
                t("prefs.bt.nearbyHint"),
                devices(
                    near,
                    if b(&a["discovering"]) {
                        "prefs.bt.searching"
                    } else {
                        "prefs.bt.noneNearby"
                    },
                ),
            ));
            if !b(&a["discovering"]) {
                out.push(actions(vec![u.button(
                    t("prefs.bt.searchAgain"),
                    op("btSearch"),
                    false,
                    false,
                )]));
            }
            out.push(u.note(t("prefs.bt.audioHint"), ""));
            if !b(&st["agent"]) {
                out.push(u.note(t("prefs.bt.noAgent"), "warn"));
            }
        }
        stack(out)
    }
    fn bt_device(&self, d: &Value) -> Value {
        let u = self.ui();
        let paired = b(&d["paired"]);
        let busy = b(&d["busy"]);
        let mut controls = vec![];
        if !busy {
            controls.push(u.button(
                t(if paired && b(&d["connected"]) {
                    "prefs.bt.disconnect"
                } else {
                    "prefs.bt.connect"
                }),
                call(
                    if !paired {
                        "bt.pair"
                    } else if b(&d["connected"]) {
                        "bt.disconnect"
                    } else {
                        "bt.connect"
                    },
                    json!([d["path"]]),
                ),
                false,
                false,
            ));
        } else if !paired {
            controls.push(u.text(t("prefs.bt.pairing"), 12., true));
        }
        if paired {
            controls.push(u.button(
                t("prefs.bt.forget"),
                call("bt.forget", json!([d["path"]])),
                false,
                false,
            ));
        }
        let hint = if paired {
            format!(
                "{}{}",
                self.tr(if busy {
                    "prefs.bt.working"
                } else if b(&d["connected"]) {
                    "prefs.bt.connected"
                } else {
                    "prefs.bt.notConnected"
                }),
                if n(&d["battery"]) >= 0. {
                    format!(
                        " · {}",
                        self.caption(&tp("prefs.bt.battery", json!({"pct":d["battery"]})))
                    )
                } else {
                    String::new()
                }
            )
        } else if !d["rssi"].is_null() {
            self.caption(&tp("prefs.bt.signal", json!({"dbm":d["rssi"]})))
        } else {
            String::new()
        };
        let mut row = u.row(d["name"].clone(), hint, controls);
        let icon = s(&d["icon"]);
        let glyph = if icon.contains("audio") || icon.contains("head") {
            "♪"
        } else if icon.contains("keyboard") {
            "⌨"
        } else if icon.contains("mouse") || icon.contains("tablet") {
            "◖"
        } else if icon.contains("gaming") || icon.contains("joystick") {
            "✜"
        } else if icon.contains("phone") {
            "▯"
        } else if icon.contains("computer") {
            "▭"
        } else if icon.contains("camera") {
            "◉"
        } else if icon.contains("printer") {
            "⎙"
        } else {
            "ᛒ"
        };
        row["children"].as_array_mut().unwrap().insert(0,view(json!({"width":28,"height":28,"borderRadius":14,"backgroundColor":u.color("control"),"alignItems":"center","justifyContent":"center"}),vec![u.text(glyph,14.,false)]));
        row
    }
    pub(super) fn sound(&self) -> Value {
        let u = self.ui();
        let st = self.d("sound.state");
        if !b(&st["connected"]) {
            return stack(vec![u.note(t("settings.sound.unavailable"), "warn")]);
        }
        let sinks = arr(&st["sinks"]);
        let mics: Vec<_> = arr(&st["sources"])
            .iter()
            .filter(|d| d["monitorOf"] == -1)
            .cloned()
            .collect();
        let default = |list: &[Value]| {
            list.iter()
                .find(|d| b(&d["default"]))
                .or_else(|| list.first())
                .cloned()
        };
        let mut out = vec![self.sound_devices(
            "sink",
            sinks,
            "settings.sound.output",
            "settings.sound.noOutputs",
        )];
        if let Some(dev) = default(sinks) {
            let mut rows = vec![u.row(
                t("settings.sound.volume"),
                "",
                vec![self.volume("sink", &dev, 170.)],
            )];
            if arr(&dev["ports"]).len() > 1 {
                rows.push(u.row(
                    t("settings.sound.port"),
                    "",
                    vec![u.segmented(
                            dev["activePort"].clone(),
                            arr(&dev["ports"])
                                .iter()
                                .map(|p| {
                                    json!([
                                        p["name"],
                                        p["description"]
                                            .as_str()
                                            .filter(|s| !s.is_empty())
                                            .unwrap_or(s(&p["name"]))
                                    ])
                                })
                                .collect(),
                            json!({"op":"soundPort","kind":"sink","index":dev["index"]}),
                        )],
                ));
            }
            rows.push(u.row(
                t("settings.sound.test"),
                "",
                vec![u.button(
                    t("settings.sound.play"),
                    call("sound.play", json!(["notify"])),
                    false,
                    false,
                )],
            ));
            out.push(u.section("", "", rows));
        }
        out.push(self.sound_devices(
            "source",
            &mics,
            "settings.sound.input",
            "settings.sound.noInputs",
        ));
        if let Some(dev) = default(&mics) {
            let peak = self.d("sound.peak");
            let level = if peak["source"] == dev["index"] {
                n(&peak["level"]).min(1.)
            } else {
                0.
            };
            out.push(u.section("","",vec![u.row(t("settings.sound.volume"),"",vec![self.volume("source",&dev,170.)]),u.row(t("settings.sound.level"),"",vec![view(json!({"flexDirection":"row","gap":3,"direction":"ltr"}),(0..16).map(|i|view(json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":if(i as f64)<(level*16.).round(){json!(if i>=13{"#ff453a"}else{"#30d158"})}else{u.color("control")}}),vec![])).collect())])]));
        }
        let apps = arr(&st["inputs"]);
        out.push(u.section(
            t("settings.sound.apps"),
            "",
            if apps.is_empty() {
                vec![u.disabled(u.row(t("settings.sound.noApps"), "", vec![]), true)]
            } else {
                apps.iter()
                    .map(|a| {
                        u.row(
                            a["app"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .unwrap_or(s(&a["name"])),
                            "",
                            vec![if b(&a["hasVolume"]) {
                                self.volume("input", a, 140.)
                            } else {
                                u.toggle(
                                    !b(&a["mute"]),
                                    call(
                                        "sound.mute",
                                        json!(["input", a["index"], !b(&a["mute"])]),
                                    ),
                                )
                            }],
                        )
                    })
                    .collect()
            },
        ));
        stack(out)
    }
    fn sound_devices(&self, kind: &str, list: &[Value], title: &str, empty: &str) -> Value {
        let u = self.ui();
        u.section(
            t(title),
            "",
            if list.is_empty() {
                vec![u.disabled(u.row(t(empty), "", vec![]), true)]
            } else {
                list.iter()
                    .map(|d| {
                        click(
                            u.row(
                                d["description"]
                                    .as_str()
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or(s(&d["name"])),
                                "",
                                vec![u.text(if b(&d["default"]) { "✓" } else { "" }, 14., false)],
                            ),
                            if b(&d["default"]) {
                                Value::Null
                            } else {
                                call("sound.default", json!([kind, d["name"]]))
                            },
                        )
                    })
                    .collect()
            },
        )
    }
    fn volume(&self, kind: &str, d: &Value, width: f64) -> Value {
        let u = self.ui();
        view(
            json!({"flexDirection":"row","alignItems":"center","gap":8,"direction":"ltr"}),
            vec![
                u.slider(
                    &format!("{kind}-{}", d["index"]),
                    n(&d["level"]).min(1.),
                    0.,
                    1.,
                    0.01,
                    width,
                    json!({"op":"soundVolume","kind":kind,"index":d["index"]}),
                ),
                u.text(format!("{} %", (n(&d["level"]) * 100.).round()), 12., true),
                u.toggle(
                    !b(&d["mute"]),
                    call("sound.mute", json!([kind, d["index"], !b(&d["mute"])])),
                ),
            ],
        )
    }
    pub(super) fn power(&self) -> Value {
        let u = self.ui();
        let battery = self.d("power.state");
        let has = b(&battery["hasBattery"]);
        let mut out = vec![];
        if has {
            let main = arr(&battery["supplies"])
                .iter()
                .find(|s| b(&s["battery"]) && !b(&s["device"]))
                .cloned()
                .unwrap_or(Value::Null);
            let minutes = n(&battery["minutesLeft"]) as i64;
            let status = if b(&battery["charging"]) {
                if minutes > 0 {
                    tp(
                        "prefs.power.chargingLeft",
                        json!({"time":self.caption(&duration(minutes))}),
                    )
                } else {
                    t("prefs.power.charging")
                }
            } else if b(&battery["onAC"]) {
                t("prefs.power.onACFull")
            } else if minutes > 0 {
                tp(
                    "prefs.power.left",
                    json!({"time":self.caption(&duration(minutes))}),
                )
            } else {
                t("prefs.power.onBattery")
            };
            let health = if main["health"].as_f64().unwrap_or(-1.) >= 0. {
                tp(
                    if n(&main["health"]) >= 80. {
                        "prefs.power.healthNormal"
                    } else {
                        "prefs.power.healthService"
                    },
                    json!({"pct":main["health"]}),
                )
            } else {
                t("prefs.power.healthUnknown")
            };
            out.push(u.header(
                system::badge("bolt", "#30d158", 52.),
                if n(&battery["percent"]) >= 0. {
                    json!(format!("{} %", battery["percent"]))
                } else {
                    t("prefs.power.battery")
                },
                format!(
                    "{}\n{}{}",
                    self.caption(&status),
                    self.caption(&health),
                    if n(&main["cycles"]) > 0. {
                        format!(
                            " · {}",
                            self.caption(&tp("prefs.power.cycles", json!({"n":main["cycles"]})))
                        )
                    } else {
                        String::new()
                    }
                ),
                vec![],
            ));
        } else {
            out.push(u.header(
                system::badge("bolt", "#30d158", 44.),
                t("prefs.power.onPower"),
                t("prefs.power.noBattery"),
                vec![],
            ));
        }
        let profiles = self.d("power.profiles");
        let available = arr(&profiles["profiles"]);
        let options = opts(
            &["power-saver", "balanced", "performance"],
            "prefs.power.profile.",
        )
        .into_iter()
        .filter(|p| available.contains(&p[0]))
        .collect();
        out.push(u.section(
            "",
            "",
            vec![u.disabled(
                u.row(
                    t("prefs.power.mode"),
                    if available.is_empty() {
                        t("prefs.power.noProfiles")
                    } else if b(&profiles["degraded"]) {
                        t("prefs.power.degraded")
                    } else {
                        tp(
                            if b(&profiles["daemon"]) {
                                "prefs.power.modeHint"
                            } else {
                                "prefs.power.modeGovernor"
                            },
                            json!({"gov":profiles["governor"]}),
                        )
                    },
                    vec![u.segmented(profiles["active"].clone(), options, op("powerProfile"))],
                ),
                available.is_empty(),
            )],
        ));
        let mut rows = vec![];
        for (path, label, kind) in [
            ("offAC", if has { "offAC" } else { "off" }, "off"),
            ("offBattery", "offBattery", "off"),
            ("lockDelay", "lock", "lock"),
            ("sleepAC", if has { "sleepAC" } else { "sleep" }, "sleep"),
            ("sleepBattery", "sleepBattery", "sleep"),
        ] {
            if !has && path.ends_with("Battery") {
                continue;
            }
            let values: &[i64] = match kind {
                "off" => &[0, 60, 120, 180, 300, 600, 900, 1800, 3600],
                "sleep" => &[0, 900, 1800, 3600, 7200, 10800],
                _ => &[0, 5, 60, 300, 900, 3600, -1],
            };
            let options = values
                .iter()
                .map(|v| {
                    json!([
                        v,
                        if *v == 0 {
                            t(if kind == "lock" {
                                "prefs.power.immediately"
                            } else {
                                "prefs.power.never"
                            })
                        } else if *v < 0 {
                            t("prefs.power.never")
                        } else if *v < 60 {
                            tp("prefs.power.secs", json!({"s":v}))
                        } else {
                            duration(v / 60)
                        }
                    ])
                })
                .collect();
            rows.push(u.row(
                t(&format!("prefs.power.{label}")),
                "",
                vec![u.pop(
                    path,
                    self.pref("power")[path].clone(),
                    options,
                    patch(&format!("power.{path}"), Value::Null),
                    190.,
                    false,
                )],
            ));
        }
        out.push(u.section(
            t("prefs.power.screenSleep"),
            t("prefs.power.screenSleepHint"),
            rows,
        ));
        let buttons = self.d("power.buttons");
        let rows = [("power", "powerKey"), ("lid", "lid"), ("lidAC", "lidAC")]
            .iter()
            .filter(|(k, _)| *k == "power" || has)
            .map(|(k, label)| {
                u.disabled(
                    u.row(
                        t(&format!("prefs.power.{label}")),
                        "",
                        vec![u.pop(
                            k,
                            buttons[k].clone(),
                            opts(
                                &["suspend", "hibernate", "poweroff", "ignore"],
                                "prefs.power.act.",
                            ),
                            json!({"op":"powerButton","field":k}),
                            190.,
                            !self.admin(),
                        )],
                    ),
                    !self.admin(),
                )
            })
            .collect();
        out.push(u.section(
            t("prefs.power.buttons"),
            t(if self.admin() {
                "prefs.power.buttonsHint"
            } else {
                "prefs.power.needAdmin"
            }),
            rows,
        ));
        if has {
            out.push(u.section(
                t("prefs.power.battery"),
                "",
                vec![
                    self.hardware_bool(
                        "prefs.power.lowBattery",
                        "prefs.power.lowBatteryHint",
                        "power.lowBattery",
                    ),
                    self.hardware_bool("prefs.power.menuPercent", "", "power.menuPercent"),
                ],
            ));
        }
        stack(out)
    }
    fn hardware_bool(&self, label: &str, hint: &str, path: &str) -> Value {
        let u = self.ui();
        u.row(
            t(label),
            if hint.is_empty() { json!("") } else { t(hint) },
            vec![u.toggle(b(self.pref(path)), patch(path, json!(!b(self.pref(path)))))],
        )
    }
    pub(super) fn datetime(&self) -> Value {
        let u = self.ui();
        let td = self.d("timedate.state");
        let ntp = b(&td["ntp"]);
        let admin = self.admin();
        let tm = crate::clock::local(self.now);
        let date = crate::native_view::date_format::date(
            &mut self.i18n.borrow_mut(),
            &self.region,
            &tm,
            "long",
        );
        let time = crate::native_view::date_format::time(
            &mut self.i18n.borrow_mut(),
            &self.region,
            s(self.pref("clock.hour24")),
            &tm,
            true,
        );
        let mut first = vec![
            u.disabled(
                u.row(
                    t("prefs.time.auto"),
                    if td.is_null() {
                        json!("")
                    } else {
                        t(if !b(&td["canNTP"]) {
                            "prefs.time.noNtp"
                        } else if ntp {
                            if b(&td["synced"]) {
                                "prefs.time.synced"
                            } else {
                                "prefs.time.syncing"
                            }
                        } else {
                            "prefs.time.manual"
                        })
                    },
                    vec![u.toggle(ntp, op("ntp"))],
                ),
                !admin || (!td.is_null() && !b(&td["canNTP"])),
            ),
            u.row(
                t("prefs.time.dateTime"),
                format!("{date} · {time}"),
                vec![u.button(
                    t("prefs.time.change"),
                    op("manualTimeToggle"),
                    ntp || !admin,
                    false,
                )],
            ),
        ];
        if b(self.st("manualTime")) && !ntp {
            first.push(view(json!({"paddingVertical":8,"gap":8}),vec![view(json!({"flexDirection":"row","alignItems":"center","gap":8,"direction":"ltr"}),vec![json!({"kind":"input","key":"manual-date","value":self.st("manualDate"),"change":["field","manualDate"],"placeholder":"2026-10-18","style":{"width":130}}),json!({"kind":"input","key":"manual-clock","value":self.st("manualClock"),"change":["field","manualClock"],"placeholder":"14:30","style":{"width":80}}),u.button(t("prefs.time.set"),op("timeSet"),!manual_valid(s(self.st("manualDate")),s(self.st("manualClock"))),true)]),u.note(t("prefs.time.manualHint"),"")]));
        }
        let zone = s(self.d("time.zone"));
        let mut zones = vec![u.row(
            zone.rsplit('/').next().unwrap_or(zone).replace('_', " "),
            format!(
                "{}{}",
                zone.replace('_', " "),
                self.d("zoneOffset")["offset"]
                    .as_i64()
                    .map(|offset| format!(
                        " · {} ({})",
                        offset_text(offset),
                        s(&self.d("zoneOffset")["abbr"])
                    ))
                    .unwrap_or_default()
            ),
            vec![u.button(
                t("prefs.time.change"),
                state("zonePicker", json!(!b(self.st("zonePicker")))),
                !admin,
                false,
            )],
        )];
        if b(self.st("zonePicker")) {
            zones.push(self.zone_picker(zone));
        }
        let clock = self.pref("clock");
        let preview_time = crate::native_view::date_format::time(
            &mut self.i18n.borrow_mut(),
            &self.region,
            s(&clock["hour24"]),
            &tm,
            b(&clock["seconds"]),
        );
        let preview = match s(&clock["date"]) {
            "none" => preview_time,
            "weekday" => {
                let lang = self.i18n.borrow().lang.clone();
                let weekday = self
                    .i18n
                    .borrow_mut()
                    .entry(&lang, "format", "weekdaysShort")
                    .unwrap_or(Value::Null);
                format!("{}   {preview_time}", s(&weekday[tm.tm_wday as usize]))
            }
            _ => format!(
                "{}   {preview_time}",
                crate::native_view::date_format::date(
                    &mut self.i18n.borrow_mut(),
                    &self.region,
                    &tm,
                    "short"
                )
            ),
        };
        stack(vec![
            u.section(
                "",
                if admin {
                    json!("")
                } else {
                    t("prefs.time.needAdmin")
                },
                first,
            ),
            u.section(t("prefs.time.zone"), t("prefs.time.zoneFooter"), zones),
            u.section(
                t("prefs.time.format"),
                "",
                vec![u.row(
                    t("prefs.time.hour24"),
                    t("prefs.time.hour24Hint"),
                    vec![u.pop(
                        "hour24",
                        clock["hour24"].clone(),
                        vec![
                            json!(["auto", t("prefs.time.byRegion")]),
                            json!(["24", t("prefs.time.h24")]),
                            json!(["12", t("prefs.time.h12")]),
                        ],
                        patch("clock.hour24", Value::Null),
                        190.,
                        false,
                    )],
                )],
            ),
            u.section(
                t("prefs.time.menuClock"),
                "",
                vec![
                    u.row(
                        t("prefs.time.showDate"),
                        "",
                        vec![u.pop(
                            "clock-date",
                            clock["date"].clone(),
                            vec![
                                json!(["short", t("prefs.time.dateFull")]),
                                json!(["weekday", t("prefs.time.dateWeekday")]),
                                json!(["none", t("prefs.time.dateNone")]),
                            ],
                            patch("clock.date", Value::Null),
                            190.,
                            false,
                        )],
                    ),
                    self.hardware_bool("prefs.time.seconds", "", "clock.seconds"),
                    u.value(t("prefs.time.preview"), preview),
                ],
            ),
        ])
    }
    pub(super) fn hardware_command(&mut self, c: &Value) -> bool {
        match s(&c["op"]) {
            "btSearch" => self.start_bt_search(),
            "btUnblock" => self.admin_run(
                "bt-unblock",
                String::new(),
                false,
                json!({"op":"btUnblocked"}),
            ),
            "soundVolume" => self.call_latest(
                "sound.volume",
                json!([c["kind"], c["index"], n(&c["value"]).clamp(0., 1.)]),
                format!("sound:{}:{}", s(&c["kind"]), c["index"]),
            ),
            "soundPort" => self.call(
                "sound.port",
                json!([c["kind"], c["index"], c["value"]]),
                Value::Null,
            ),
            "powerProfile" => {
                self.state["pendingProfile"] = c["value"].clone();
                if b(&self.d("power.profiles")["daemon"]) {
                    self.call(
                        "power.profile",
                        json!([c["value"]]),
                        json!({"op":"powerProfileSet","profile":c["value"]}),
                    );
                } else {
                    self.admin_run("power-profile", s(&c["value"]).into(), false, json!({}));
                }
            }
            "powerButton" => {
                if !self.admin() {
                    return true;
                }
                let mut buttons = self.d("power.buttons").clone();
                buttons[s(&c["field"])] = c["value"].clone();
                self.admin_run(
                    "power-buttons",
                    format!(
                        "{} {} {}",
                        s(&buttons["power"]),
                        s(&buttons["lid"]),
                        s(&buttons["lidAC"])
                    ),
                    false,
                    json!({}),
                );
            }
            "ntp" => {
                if self.admin()
                    && (self.d("timedate.state").is_null()
                        || b(&self.d("timedate.state")["canNTP"]))
                {
                    self.admin_run(
                        "time-ntp",
                        if b(&self.d("timedate.state")["ntp"]) {
                            "off"
                        } else {
                            "on"
                        }
                        .into(),
                        false,
                        json!({"op":"timeChanged"}),
                    );
                }
            }
            "manualTimeToggle" => {
                if !self.admin() || b(&self.d("timedate.state")["ntp"]) {
                    return true;
                }
                let tm = crate::clock::local(self.now);
                self.state["manualDate"] = json!(format!(
                    "{:04}-{:02}-{:02}",
                    tm.tm_year + 1900,
                    tm.tm_mon + 1,
                    tm.tm_mday
                ));
                self.state["manualClock"] = json!(format!("{:02}:{:02}", tm.tm_hour, tm.tm_min));
                self.state["manualTime"] = json!(!b(self.st("manualTime")));
            }
            "timeSet" => {
                let date = s(self.st("manualDate"));
                let time = s(self.st("manualClock"));
                if self.admin() && !b(&self.d("timedate.state")["ntp"]) && manual_valid(date, time)
                {
                    self.admin_run(
                        "time-set",
                        format!("{date} {time}:00"),
                        false,
                        json!({"op":"manualTimeSet"}),
                    );
                }
            }
            "zonePick" => {
                let zone = s(&c["value"]);
                if self.admin() && arr(self.d("time.zones")).contains(&c["value"]) {
                    self.state["zonePicker"] = json!(false);
                    self.admin_run("timezone", zone.into(), false, json!({"op":"zoneChanged"}));
                }
            }
            _ => return self.display_command(c) || self.cooling_command(c),
        }
        true
    }
    pub(super) fn hardware_after(&mut self, after: &Value, value: &Value) {
        match s(&after["op"]) {
            "btUnblocked" => {
                self.state["btError"] = json!("");
                self.call("bt.power", json!([true]), Value::Null);
            }
            "powerProfileSet" => {
                if value == false {
                    self.admin_run(
                        "power-profile",
                        s(&after["profile"]).into(),
                        false,
                        json!({}),
                    );
                }
            }
            "timeChanged" | "manualTimeSet" | "zoneChanged" => {
                if after["op"] == "zoneChanged" {
                    self.call("time.reload", json!([]), Value::Null);
                }
                if after["op"] == "manualTimeSet" {
                    self.state["manualTime"] = json!(false);
                }
                self.call("timedate.refresh", json!([]), Value::Null);
            }
            _ => {}
        }
    }
    pub(super) fn hardware_failed(&mut self, after: &Value, _result: &Value) {
        if after["op"] == "powerProfileSet" {
            self.admin_run(
                "power-profile",
                s(&after["profile"]).into(),
                false,
                json!({}),
            );
        }
    }
    pub(super) fn hardware_event(&mut self, e: &Value) {
        match s(&e["kind"]) {
            "bluetooth" => {
                let r = &e["result"];
                if b(&r["ok"]) {
                    if r["op"] != "stop" {
                        self.state["btError"] = json!("");
                    }
                } else if r["op"] != "stop" {
                    let error = s(&r["error"]);
                    let lower = error.to_lowercase();
                    let code = if lower.contains("blocked") || lower.contains("rfkill") {
                        "blocked"
                    } else if ["profile-unavailable", "notavailable", "notsupported"]
                        .iter()
                        .any(|k| lower.contains(k))
                    {
                        "noProfile"
                    } else if ["authenticationfailed", "authenticationrejected", "rejected"]
                        .iter()
                        .any(|k| lower.contains(k))
                    {
                        "authFailed"
                    } else if lower.contains("canceled") {
                        "canceled"
                    } else if lower.contains("alreadyexists") {
                        ""
                    } else {
                        "failed"
                    };
                    if !code.is_empty() {
                        self.state["btError"] = json!(code);
                    }
                }
                self.read_at = 0;
            }
            "powerProfile" => {
                if b(&e["result"]["ok"]) {
                    self.state["pendingProfile"] = Value::Null;
                }
                if !b(&e["result"]["ok"]) && !s(self.st("pendingProfile")).is_empty() {
                    self.admin_run(
                        "power-profile",
                        s(self.st("pendingProfile")).into(),
                        false,
                        json!({}),
                    );
                }
            }
            _ => {}
        }
    }
}
pub(super) fn manual_valid(date: &str, time: &str) -> bool {
    date.len() == 10
        && date.bytes().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
        && time.len() == 5
        && time.bytes().enumerate().all(|(i, c)| {
            if i == 2 {
                c == b':'
            } else {
                c.is_ascii_digit()
            }
        })
}
fn duration(minutes: i64) -> Value {
    if minutes >= 60 {
        tp("prefs.power.hm", json!({"h":minutes/60,"m":minutes%60}))
    } else {
        tp("prefs.power.m", json!({"m":minutes}))
    }
}

fn offset_text(seconds: i64) -> String {
    let sign = if seconds < 0 { "−" } else { "+" };
    let absolute = seconds.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", absolute / 3600, absolute / 60 % 60)
}
