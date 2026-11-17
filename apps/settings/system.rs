use super::*;
pub(super) const DEFAULT_IDS: [&str; 9] = [
    "browser",
    "mail",
    "music",
    "video",
    "images",
    "pdf",
    "text",
    "terminal",
    "screenshots",
];
pub(super) fn badge(glyph: &str, color: &str, size: f64) -> Value {
    let children = match glyph {
        "fan" => {
            let mut blades = vec![];
            for rotation in [0, 120, 240] {
                blades.push(view(json!({"position":"absolute","left":0,"top":0,"width":14,"height":14,"transform":format!("rotate({rotation}deg)")}),vec![view(json!({"position":"absolute","left":6,"top":0,"width":5,"height":7,"borderTopRightRadius":5,"borderTopLeftRadius":2,"borderBottomRightRadius":2,"backgroundColor":"#fff"}),vec![])]));
            }
            blades.push(view(json!({"position":"absolute","left":5,"top":5,"width":4,"height":4,"borderRadius":2,"backgroundColor":"#fff"}),vec![]));
            vec![view(
                json!({"width":14,"height":14,"direction":"ltr"}),
                blades,
            )]
        }
        "chip" => {
            let mut pins = vec![view(
                json!({"position":"absolute","left":3,"top":3,"width":8,"height":8,"borderRadius":1.5,"backgroundColor":"#fff"}),
                vec![],
            )];
            for y in [4., 6.5, 9.] {
                for x in [0, 11] {
                    pins.push(view(json!({"position":"absolute","left":x,"top":y-0.5,"width":3,"height":1.5,"backgroundColor":"#fff"}),vec![]));
                }
            }
            vec![view(
                json!({"width":14,"height":14,"direction":"ltr"}),
                pins,
            )]
        }
        "camera" => vec![view(
            json!({"width":13,"height":10,"alignItems":"center","justifyContent":"center"}),
            vec![
                view(
                    json!({"position":"absolute","top":0,"width":5,"height":3,"borderRadius":1,"backgroundColor":"#fff"}),
                    vec![],
                ),
                view(
                    json!({"position":"absolute","top":2,"width":13,"height":8,"borderRadius":2,"backgroundColor":"#fff"}),
                    vec![],
                ),
                view(
                    json!({"position":"absolute","top":4,"width":5,"height":5,"borderRadius":3,"backgroundColor":"#30d158"}),
                    vec![],
                ),
            ],
        )],
        "microphone" => vec![view(
            json!({"width":10,"height":13,"alignItems":"center"}),
            vec![
                view(
                    json!({"width":5,"height":8,"borderRadius":3,"backgroundColor":"#fff"}),
                    vec![],
                ),
                view(
                    json!({"width":2,"height":3,"backgroundColor":"#fff"}),
                    vec![],
                ),
                view(
                    json!({"width":7,"height":2,"borderRadius":1,"backgroundColor":"#fff"}),
                    vec![],
                ),
            ],
        )],
        "folder" => vec![view(
            json!({"width":13,"height":10}),
            vec![
                view(
                    json!({"position":"absolute","left":0,"top":0,"width":6,"height":3,"borderRadius":1,"backgroundColor":"#fff"}),
                    vec![],
                ),
                view(
                    json!({"position":"absolute","left":0,"top":2,"width":13,"height":8,"borderRadius":2,"backgroundColor":"#fff"}),
                    vec![],
                ),
            ],
        )],
        "lock" => vec![view(
            json!({"width":12,"height":13,"alignItems":"center"}),
            vec![
                view(
                    json!({"width":8,"height":9,"borderRadius":4,"border":"2px solid #fff"}),
                    vec![],
                ),
                view(
                    json!({"position":"absolute","top":5,"width":12,"height":8,"borderRadius":2,"backgroundColor":"#fff"}),
                    vec![],
                ),
            ],
        )],
        "bolt" => vec![view(
            json!({"width":12,"height":14,"direction":"ltr"}),
            vec![
                view(
                    json!({"position":"absolute","left":5,"top":0,"width":4,"height":7,"borderRadius":1,"backgroundColor":"#fff"}),
                    vec![],
                ),
                view(
                    json!({"position":"absolute","left":2,"top":5,"width":8,"height":3,"borderRadius":1,"backgroundColor":"#fff"}),
                    vec![],
                ),
                view(
                    json!({"position":"absolute","left":3,"top":7,"width":4,"height":7,"borderRadius":1,"backgroundColor":"#fff"}),
                    vec![],
                ),
            ],
        )],
        _ => vec![
            json!({"kind":"text","label":glyph,"style":{"fontSize":(size*0.55).round(),"fontWeight":600,"color":"#fff","lineHeight":size,"textAlign":"center"}}),
        ],
    };
    view(
        json!({"width":size,"height":size,"borderRadius":(size*0.24).round(),"alignItems":"center","justifyContent":"center","background":format!("linear-gradient(180deg, {}, {color})",shade(color,1.18))}),
        children,
    )
}
pub(super) fn icon(size: f64) -> Value {
    view(
        json!({"width":size,"height":size,"borderRadius":size*0.225,"background":"linear-gradient(180deg, #bcbcc2, #74747e)","boxShadow":"0 2px 4px rgba(0,0,0,.3)"}),
        vec![
            view(
                json!({"position":"absolute","left":size*0.44,"top":size*0.14,"width":size*0.12,"height":size*0.72,"borderRadius":size*0.04,"backgroundColor":"#3f3f46"}),
                vec![],
            ),
            view(
                json!({"position":"absolute","left":size*0.14,"top":size*0.44,"width":size*0.72,"height":size*0.12,"borderRadius":size*0.04,"backgroundColor":"#3f3f46"}),
                vec![],
            ),
            view(
                json!({"position":"absolute","left":size*0.21,"top":size*0.21,"width":size*0.58,"height":size*0.58,"borderRadius":size*0.29,"backgroundColor":"#3f3f46"}),
                vec![],
            ),
            view(
                json!({"position":"absolute","left":size*0.36,"top":size*0.36,"width":size*0.28,"height":size*0.28,"borderRadius":size*0.14,"background":"linear-gradient(180deg, #bcbcc2, #74747e)"}),
                vec![],
            ),
        ],
    )
}
pub(super) fn eeprom_status(line: &str) -> Option<Value> {
    let parts: Vec<_> = line.split(' ').collect();
    if parts.len() != 4
        || !matches!(parts[0], "uptodate" | "available" | "pending")
        || parts[1..3]
            .iter()
            .any(|p| p.is_empty() || p.len() > 10 || !p.bytes().all(|b| b.is_ascii_digit()))
        || parts[3].is_empty()
        || parts[3].len() > 15
        || !parts[3].bytes().all(|b| b.is_ascii_lowercase())
    {
        return None;
    }
    Some(
        json!({"state":parts[0],"current":parts[1].parse::<u64>().ok()?,"latest":parts[2].parse::<u64>().ok()?,"release":parts[3]}),
    )
}
impl App {
    pub(super) fn bytes(&self, mut value: f64) -> String {
        let units = ["B", "KB", "MB", "GB", "TB"];
        let mut i = 0;
        while value >= 1024. && i < 4 {
            value /= 1024.;
            i += 1;
        }
        format!(
            "{} {}",
            if value < 10. && i > 0 {
                self.i18n.borrow().number(value, 1)
            } else {
                format!("{:.0}", value.round())
            },
            units[i]
        )
    }
    pub(super) fn general(&self) -> Value {
        let u = self.ui();
        let info = self.d("system.info");
        let stats = self.d("system.stats");
        let cpu = s(&info["cpu"])
            .replace("(R)", "")
            .replace("(TM)", "")
            .replace("CPU", "")
            .replace("Processor", "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let version = info["version"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(s(&self.d("platform")["version"]));
        let id = s(&info["channel"]);
        let channel = match id {
            "beta" => self.tr("about.channelBeta"),
            "alpha" => self.tr("about.channelAlpha"),
            "rc" => self.tr("about.channelRc"),
            "dev" => self.tr("about.channelDev"),
            _ => id
                .chars()
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + &id[c.len_utf8()..])
                .unwrap_or_default(),
        };
        let mut machine=vec![u.value(t("settings.general.name"),info["hostname"].clone()),u.value(t("about.cpu"),tp("about.cpuValue",json!({"cpu":if cpu.is_empty(){self.tr("about.cpuUnknown")}else{cpu.chars().take(44).collect()},"count":info["cores"]}))),u.value(t("about.memory"),tp("shell.cc.memDetail",json!({"used":self.bytes((n(&info["memTotalKB"])-n(&info["memAvailableKB"]))*1024.),"total":self.bytes(n(&info["memTotalKB"])*1024.)})))];
        if n(&info["diskTotal"]) > 0. {
            machine.push(u.value(t("settings.general.storage"),tp("settings.general.storageValue",json!({"free":self.bytes(n(&info["diskFree"])),"total":self.bytes(n(&info["diskTotal"]))}))));
        }
        machine.push(u.value(
            t("about.display"),
            format!(
                "{} × {}",
                self.d("platform")["screen"]["width"],
                self.d("platform")["screen"]["height"]
            ),
        ));
        let seconds = n(&info["uptime"]) as i64;
        let days = seconds / 86400;
        let hours = seconds % 86400 / 3600;
        let minutes = seconds % 3600 / 60;
        machine.push(u.value(
            t("about.uptime"),
            if days > 0 {
                format!("{days} d {hours} h")
            } else if hours > 0 {
                format!("{hours} h {minutes} min")
            } else {
                format!("{minutes} min")
            },
        ));
        let mut software = vec![u.value(
            "Axia DE",
            if channel.is_empty() {
                version.into()
            } else {
                format!("{version} {channel}")
            },
        )];
        if !s(&info["build"]).is_empty() {
            software.push(u.value(t("about.buildLabel"), info["build"].clone()));
        }
        software.extend([
            u.value(t("about.os"), info["os"].clone()),
            u.value(
                t("about.kernel"),
                format!("{} ({})", s(&info["kernel"]), s(&info["arch"])),
            ),
            u.value(
                t("about.server"),
                "axia-server (C · DRM/KMS · evdev · Wayland)",
            ),
            u.value(
                t("about.engine"),
                format!("QuickJS + React {}", s(&self.d("platform")["reactVersion"])),
            ),
            u.value(
                t("about.ram"),
                format!(
                    "{} MB (JS {} MB)",
                    self.i18n.borrow().number(n(&stats["rssKB"]) / 1024., 1),
                    self.i18n.borrow().number(n(&stats["jsKB"]) / 1024., 1)
                ),
            ),
        ]);
        let branding = view(
            json!({"alignItems":"center","gap":4,"paddingTop":4}),
            vec![
                view(
                    json!({"width":72,"height":72,"borderRadius":15.84,"boxShadow":"0 2.88px 8.64px rgba(0,0,0,.3)"}),
                    vec![
                        json!({"kind":"image","src":"asset:icons/axia-icon-256.png","style":{"width":72,"height":72}}),
                    ],
                ),
                view(
                    json!({"marginTop":8,"flexDirection":"row","alignItems":"center","direction":"ltr"}),
                    vec![
                        json!({"kind":"text","label":"Axia","style":{"fontFamily":"brand","fontWeight":800,"fontSize":22,"color":if u.dark(){"#ffffff"}else{"#12122A"},"whiteSpace":"nowrap"}}),
                    ],
                ),
                u.text(
                    if channel.is_empty() {
                        tp("about.version", json!({"v":version}))
                    } else {
                        tp(
                            "about.versionChannel",
                            json!({"v":version,"channel":channel}),
                        )
                    },
                    12.,
                    true,
                ),
                u.text(
                    format!("{} · {}", s(&info["hostname"]), s(&info["os"])),
                    11.,
                    true,
                ),
            ],
        );
        stack(vec![
            branding,
            u.section(t("settings.general.machine"), "", machine),
            u.section(t("settings.general.software"), "", software),
        ])
    }
    pub(super) fn session(&self) -> Value {
        let u = self.ui();
        let mode = self.pref("sessionRestore");
        stack(vec![
            u.section(
                "",
                t("settings.session.restoreHint"),
                vec![
                    u.row(
                        t("settings.session.restore"),
                        "",
                        vec![u.pop(
                            "session-restore",
                            mode.clone(),
                            opts(&["always", "ask", "off"], "settings.session.restore."),
                            json!({"op":"sessionRestore"}),
                            190.,
                            false,
                        )],
                    ),
                    u.disabled(
                        u.row(
                            t("settings.session.browsers"),
                            t("settings.session.browsersHint"),
                            vec![u.toggle(
                                self.pref("sessionBrowsers") != false,
                                op("sessionBrowsers"),
                            )],
                        ),
                        mode == "off",
                    ),
                ],
            ),
            u.section(
                "",
                t(if b(self.d("session.managed")) {
                    "settings.session.hintManaged"
                } else {
                    "settings.session.hint"
                }),
                vec![u.row(
                    t("shell.menu.logout"),
                    "",
                    vec![u.button(
                        t("shell.menu.logout"),
                        call("session.quit", json!([])),
                        false,
                        false,
                    )],
                )],
            ),
        ])
    }
    pub(super) fn updates(&self) -> Value {
        let u = self.ui();
        let open = u.regular_button(
            t("settings.updates.open"),
            json!({"op":"open","app":"store","args":{"page":"updates"}}),
            false,
            true,
        );
        if self.standalone() {
            return stack(vec![
                u.note(t("store.host.description"), ""),
                actions(vec![open]),
            ]);
        }
        let count = arr(self.d("pkg.upgrades")).len();
        let at = n(self.d("pkg.lastUpdate")) as i64;
        let date = if at > 0 {
            crate::native_view::date_format::date(
                &mut self.i18n.borrow_mut(),
                &self.region,
                &crate::clock::local(at * 1000),
                "long",
            )
        } else {
            "—".into()
        };
        let mut out = vec![
            u.section(
                "",
                t("settings.updates.hint"),
                vec![
                    u.value(
                        t("settings.updates.debian"),
                        if count > 0 {
                            tp("settings.updates.available", json!({"count":count}))
                        } else {
                            t("settings.updates.none")
                        },
                    ),
                    u.value(t("settings.updates.last"), date),
                ],
            ),
            actions(vec![
                u.regular_button(
                    t(if self.st("busy") == "pkg-update" {
                        "settings.updates.checking"
                    } else {
                        "settings.updates.check"
                    }),
                    op("updatesCheck"),
                    !s(self.st("busy")).is_empty(),
                    false,
                ),
                open,
            ]),
        ];
        if !self.admin() {
            out.push(u.note(t("settings.admin.unavailable"), "warn"));
        }
        stack(out)
    }
    pub(super) fn firmware(&self) -> Value {
        let u = self.ui();
        let st = self.d("firmware");
        let state = s(&st["state"]);
        let busy = !s(self.st("busy")).is_empty();
        let missing = self.st("errorCode") == "hw.noEeprom";
        let can_restart = b(self.d("session.canPower"));
        let date = |key: &str| {
            if n(&st[key]) > 0. {
                json!(crate::native_view::date_format::date(
                    &mut self.i18n.borrow_mut(),
                    &self.region,
                    &crate::clock::local(n(&st[key]) as i64 * 1000),
                    "long"
                ))
            } else {
                t("settings.firmware.unknown")
            }
        };
        let mut buttons = vec![u.button(
            t("settings.firmware.check"),
            op("firmwareCheck"),
            busy,
            false,
        )];
        if state == "available" {
            buttons.push(u.button(
                t(if self.st("busy") == "rpi-eeprom-update" {
                    "settings.firmware.updating"
                } else {
                    "settings.firmware.update"
                }),
                op("firmwareUpdate"),
                busy || !self.machine_admin(),
                true,
            ));
        }
        if state == "pending" && can_restart {
            buttons.push(u.button(
                t("settings.firmware.restart"),
                call("session.restart", json!([])),
                false,
                true,
            ));
        }
        let mut out = vec![u.section(
            t("settings.firmware.bootloader"),
            t("settings.firmware.hint"),
            vec![
                u.text(
                    if !state.is_empty() {
                        t(&format!("settings.firmware.state.{state}"))
                    } else if self.st("busy") == "rpi-eeprom-status" {
                        t("settings.firmware.checking")
                    } else {
                        json!("—")
                    },
                    17.,
                    false,
                ),
                u.value(
                    t("settings.firmware.current"),
                    if st.is_null() {
                        json!("—")
                    } else {
                        date("current")
                    },
                ),
                u.value(
                    t("settings.firmware.latest"),
                    if st.is_null() {
                        json!("—")
                    } else {
                        date("latest")
                    },
                ),
                u.value(
                    t("settings.firmware.releaseLabel"),
                    if matches!(s(&st["release"]), "default" | "latest") {
                        t(&format!("settings.firmware.release.{}", s(&st["release"])))
                    } else {
                        st["release"].clone()
                    },
                ),
                actions(buttons),
            ],
        )];
        if state == "pending" {
            out.push(u.note(
                t(if can_restart {
                    "settings.firmware.pendingHint"
                } else {
                    "settings.firmware.pendingHow"
                }),
                "warn",
            ));
        }
        if state == "available" && !self.machine_admin() {
            out.push(u.note(t("settings.firmware.needAdmin"), ""));
        }
        if self.st("busy") == "rpi-eeprom-update" {
            out.push(u.note(t("settings.firmware.updatingHint"), ""));
        }
        if missing {
            out.push(u.section(
                "",
                t("settings.firmware.missingHint"),
                vec![u.row(
                    t("settings.firmware.missing"),
                    "",
                    vec![u.button(
                        t(if self.st("busy") == "pkg-install" {
                            "settings.firmware.installing"
                        } else {
                            "settings.firmware.install"
                        }),
                        op("firmwareInstall"),
                        busy || !self.machine_admin(),
                        false,
                    )],
                )],
            ));
        }
        stack(out)
    }
    pub(super) fn firmware_check(&mut self) {
        self.admin_run(
            "rpi-eeprom-status",
            String::new(),
            true,
            json!({"op":"firmwareStatus"}),
        );
    }
    pub(super) fn search_list(
        &self,
        path: &str,
        items: Vec<Value>,
        binding: Value,
        current: &str,
        height: f64,
        placeholder: &str,
        limit: usize,
    ) -> Value {
        let u = self.ui();
        let query = s(self.st(path)).trim().to_lowercase();
        view(
            json!({"gap":8,"paddingVertical":8}),
            vec![
                json!({"kind":"input","key":path,"value":self.st(path).as_str().unwrap_or(""),"change":["field",path],"autoFocus":true,"placeholder":t(placeholder)}),
                view(
                    json!({"height":height,"borderRadius":8,"border":format!("1px solid {}",s(&u.color("separator"))),"backgroundColor":if u.dark(){"rgba(0,0,0,.12)"}else{"#fff"}}),
                    vec![
                        json!({"kind":"scroll","style":{"padding":4,"flex":1},"children":items.into_iter().filter(|item|query.is_empty()||format!("{} {}",s(&item[0]),self.caption(&item[1])).to_lowercase().contains(&query)).take(limit).map(|item|{let on=item[0]==current;let mut command=binding.clone();command["value"]=item[0].clone();let mut label=u.text(item[1].clone(),12.,false);label["style"]["flex"]=json!(1);if on{label["style"]["color"]=json!("#fff");}click(view(json!({"height":26,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":8,"borderRadius":5,"backgroundColor":if on{u.color("accent")}else{json!("transparent")}}),vec![label,u.text(item[2].as_str().unwrap_or(""),11.,true)]),command)}).collect::<Vec<_>>()}),
                    ],
                ),
            ],
        )
    }
    pub(super) fn system_command(&mut self, c: &Value) -> bool {
        match s(&c["op"]) {
            "sessionRestore" => {
                if matches!(s(&c["value"]), "always" | "ask" | "off") {
                    self.call("session.restoreMode", json!([c["value"]]), Value::Null);
                    self.patch("sessionRestore", c["value"].clone());
                }
            }
            "sessionBrowsers" => {
                if self.pref("sessionRestore") != "off" {
                    self.call(
                        "session.restoreBrowsers",
                        json!([self.pref("sessionBrowsers") == false]),
                        Value::Null,
                    );
                }
            }
            "updatesCheck" => self.admin_run(
                "pkg-update",
                String::new(),
                false,
                json!({"success":t("settings.updates.checked")}),
            ),
            "firmwareCheck" => self.firmware_check(),
            "firmwareUpdate" => {
                if self.machine_admin() && self.d("firmware")["state"] == "available" {
                    self.admin_run(
                        "rpi-eeprom-update",
                        String::new(),
                        true,
                        json!({"op":"firmwareStatus"}),
                    );
                }
            }
            "firmwareInstall" => {
                if self.machine_admin() && self.st("errorCode") == "hw.noEeprom" {
                    self.admin_run(
                        "pkg-install",
                        "rpi-eeprom".into(),
                        false,
                        json!({"op":"firmwareInstalled"}),
                    );
                }
            }
            _ => {
                return self.locale_command(c)
                    || self.keyboard_command(c)
                    || self.wallpaper_command(c)
                    || self.printer_command(c)
                    || self.default_command(c)
            }
        }
        true
    }
    pub(super) fn system_after(&mut self, after: &Value, value: &Value) {
        match s(&after["op"]) {
            "firmwareStatus" => {
                if let Some(status) = eeprom_status(s(value)) {
                    self.data["firmware"] = status;
                    self.state["errorCode"] = json!("");
                } else if after["adminAction"] == "rpi-eeprom-update" {
                    self.firmware_check();
                } else {
                    self.data["firmware"] = Value::Null;
                    self.state["errorCode"] = json!("hw.eepromFailed");
                    self.message(false, t("errors.hw.eepromFailed"));
                }
            }
            "firmwareInstalled" => self.firmware_check(),
            "firmwareIndex" => self.admin_run(
                "pkg-install",
                "rpi-eeprom".into(),
                false,
                json!({"op":"firmwareInstalled","retried":true}),
            ),
            _ => {
                self.locale_after(after, value);
                self.wallpaper_after(after, value);
                self.printer_after(after, value);
            }
        }
    }
    pub(super) fn system_failed(&mut self, after: &Value, result: &Value) {
        if after["op"] == "firmwareInstalled"
            && after["retried"] != true
            && result["code"] == "pkg.noIndex"
        {
            self.admin_run(
                "pkg-update",
                String::new(),
                false,
                json!({"op":"firmwareIndex"}),
            );
        }
    }
}
