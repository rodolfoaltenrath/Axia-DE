use super::*;
impl App {
    fn bool_row(&self, label: &str, hint: &str, path: &str) -> Value {
        let u = self.ui();
        u.row(
            t(label),
            if hint.is_empty() { json!("") } else { t(hint) },
            vec![u.toggle(b(self.pref(path)), patch(path, json!(!b(self.pref(path)))))],
        )
    }
    pub(super) fn appearance(&self) -> Value {
        let u = self.ui();
        let mut out = vec![];
        let mut themes = arr(self.d("themes.list")).to_vec();
        themes.sort_by_key(|x| (x["id"] != "default", s(&x["name"]).to_lowercase()));
        if themes.len() > 1 {
            out.push(u.section(t("settings.style.title"),t("settings.style.hint"),vec![view(json!({"flexDirection":"row","flexWrap":"wrap","justifyContent":"center","gap":16,"paddingVertical":12}),themes.iter().map(|th|self.theme_tile(th)).collect())]));
        }
        out.push(u.section(t("settings.mode"),"",vec![view(json!({"flexDirection":"row","justifyContent":"center","gap":24,"paddingVertical":12}),["dark","light"].iter().map(|mode|{
            let on=self.pref("mode")==*mode;let dark=*mode=="dark";click(view(json!({"alignItems":"center","gap":6}),vec![view(json!({"padding":2,"borderRadius":10,"border":format!("2px solid {}",if on{s(&u.color("accent")).to_owned()}else{"transparent".into()})}),vec![mode_thumb(dark,u.color("accent"))]),u.text(t(&format!("settings.{mode}")),12.,false)]),patch("mode",json!(mode)))
        }).collect())]));
        let mut rows = vec![];
        let accents = self.appearance_catalogs.map(|(_, accents)| accents).unwrap_or(&self.theme["accents"]).as_object();
        rows.push(u.row(t("shell.cc.accent"),"",vec![view(json!({"flexDirection":"row","gap":8,"direction":"ltr"}),accents.into_iter().flat_map(|map|map.iter()).map(|(name,color)|click(view(json!({"width":20,"height":20,"borderRadius":10,"alignItems":"center","justifyContent":"center","background":format!("linear-gradient(180deg, {}, {})",shade(s(color),1.15),s(color)),"boxShadow":"0 1px 2px rgba(0,0,0,.25)"}),if self.pref("accent")==name{vec![view(json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":"#fff"}),vec![])]}else{vec![]}),patch("accent",json!(name)))).collect())]));
        rows.push(self.bool_row("settings.effects", "settings.effectsHint", "effects"));
        rows.push(self.bool_row(
            "settings.reduceMotion",
            "settings.reduceMotionHint",
            "reduceMotion",
        ));
        out.push(u.section("", "", rows));
        out.push(u.section(
            "",
            t("settings.scrollBarsHint"),
            vec![u.row(
                t("settings.scrollBars"),
                "",
                vec![u.pop(
                    "scrollBars",
                    self.pref("scrollBars").clone(),
                    vec![
                        json!(["auto", t("settings.scrollBarsAuto")]),
                        json!(["scrolling", t("settings.scrollBarsScrolling")]),
                        json!(["always", t("settings.scrollBarsAlways")]),
                    ],
                    patch("scrollBars", Value::Null),
                    170.,
                    false,
                )],
            )],
        ));
        let on = b(self.pref("browserTabsInTitlebar"));
        let bs = self.d("browserFrame.state");
        let other = n(&bs[if on { "off" } else { "on" }]);
        let mut browser = vec![u.row(
            t("settings.browserTabs.label"),
            "",
            vec![u.toggle(on, call("browserFrame.set", json!([!on])))],
        )];
        if other > 0. && self.pref("browserFramePending") != true {
            browser.push(u.row(
                tp(
                    if on {
                        "settings.browserTabs.othersOff"
                    } else {
                        "settings.browserTabs.othersOn"
                    },
                    json!({"count":other}),
                ),
                "",
                vec![u.button(
                    t("settings.browserTabs.applyAll"),
                    call("browserFrame.set", json!([on])),
                    false,
                    false,
                )],
            ));
        }
        out.push(u.section("", t("settings.browserTabs.hint"), browser));
        let result = self.d("browserFrame.result");
        let failed = n(&result["chromium"]["failed"]) + n(&result["firefox"]["failed"]);
        if b(self.pref("browserFramePending")) {
            out.push(u.note(t("settings.browserTabs.chromiumOpen"), "warn"));
        }
        if failed > 0. {
            out.push(u.note(
                tp("settings.browserTabs.failed", json!({"count":failed})),
                "error",
            ));
        }
        if b(&result["firefox"]["kept"]) {
            out.push(u.note(t("settings.browserTabs.firefoxKept"), ""));
        }
        if failed == 0.
            && self.pref("browserFramePending") != true
            && (n(&result["chromium"]["changed"]) + n(&result["firefox"]["changed"]) > 0.)
        {
            out.push(u.note(t("settings.browserTabs.done"), "ok"));
        }
        out.push(u.section(
            "",
            t("settings.filesAppearance.hint"),
            vec![u.row(
                t("settings.filesAppearance.label"),
                "",
                vec![u.toggle(
                    self.pref("nativeAppearance") != "axia",
                    op("nativeAppearance"),
                )],
            )],
        ));
        out.push(u.section(
            "",
            t("settings.externalStyle.hint"),
            vec![self.bool_row("settings.externalStyle.label", "", "externalWindowStyle")],
        ));
        stack(out)
    }
    fn theme_tile(&self, th: &Value) -> Value {
        let u = self.ui();
        let on = b(&th["current"]);
        click(
            view(
                json!({"alignItems":"center","gap":6}),
                vec![
                    view(
                        json!({"padding":2,"borderRadius":10,"border":format!("2px solid {}",if on{s(&u.color("accent")).to_owned()}else{"transparent".into()})}),
                        vec![style_thumb(u.dark(), u.color("accent"))],
                    ),
                    {
                        let mut label = u.text(th["name"].clone(), 12., false);
                        label["style"]["fontWeight"] = json!(if on { 600 } else { 400 });
                        label
                    },
                ],
            ),
            json!({"op":"theme","value":th["id"],"current":on}),
        )
    }
    pub(super) fn dock(&self) -> Value {
        let u = self.ui();
        let mut sections=vec![u.section(t("settings.dock.position"),t("settings.dock.sidesHint"),vec![view(json!({"flexDirection":"row","flexWrap":"wrap","justifyContent":"center","gap":16,"paddingVertical":12}),["left","bottom","right"].iter().map(|side|{
            let mut bar=json!({"position":"absolute","borderRadius":3,"backgroundColor":"rgba(255,255,255,.8)"});merge(&mut bar,&match *side{"bottom"=>json!({"left":24,"right":24,"bottom":3,"height":6}),"left"=>json!({"left":3,"top":14,"bottom":10,"width":6}),_=>json!({"right":3,"top":14,"bottom":10,"width":6})});click(view(json!({"alignItems":"center","gap":6}),vec![view(json!({"padding":2,"borderRadius":9,"border":format!("2px solid {}",if self.pref("dockPosition")==*side{s(&u.color("accent")).to_owned()}else{"transparent".into()})}),vec![view(json!({"width":96,"height":60,"borderRadius":7,"overflow":"hidden","background":"linear-gradient(135deg, #4b3aa8, #c0457a)","direction":"ltr"}),vec![view(json!({"position":"absolute","left":0,"right":0,"top":0,"height":4,"backgroundColor":"rgba(0,0,0,.35)"}),vec![]),view(bar,vec![])])]),u.text(t(&format!("settings.dock.{side}")),12.,false)]),patch("dockPosition",json!(side)))
        }).collect())])];
        let mut size = vec![
            self.size_row("settings.dock.size", "dockSize", 36., 80.),
            self.bool_row(
                "settings.dock.magnify",
                "settings.dock.magnifyHint",
                "dockMagnify",
            ),
        ];
        if b(self.pref("dockMagnify")) {
            size.push(self.size_row(
                "settings.dock.magnifySize",
                "dockMagnifySize",
                n(self.pref("dockSize")),
                128.,
            ));
        }
        sections.push(u.section("", "", size));
        sections.push(u.section(
            "",
            "",
            vec![
                self.bool_row(
                    "settings.dock.autohide",
                    "settings.dock.autohideHint",
                    "dockAutohide",
                ),
                self.bool_row(
                    "settings.dock.recents",
                    "settings.dock.recentsHint",
                    "dockRecents",
                ),
            ],
        ));
        sections.push(u.section(
            "",
            t("settings.dock.resetHint"),
            if b(self.st("dockAsk")) {
                vec![
                    u.text(t("settings.dock.resetAsk"), 13., false),
                    u.text(t("settings.dock.resetDetail"), 12., true),
                    actions(vec![
                        u.regular_button(
                            t("settings.dock.resetCancel"),
                            state("dockAsk", json!(false)),
                            false,
                            false,
                        ),
                        u.regular_button(
                            t("settings.dock.resetConfirm"),
                            op("dockReset"),
                            false,
                            true,
                        ),
                    ]),
                ]
            } else {
                vec![u.row(
                    t("settings.dock.reset"),
                    if b(self.st("dockDone")) {
                        t("settings.dock.resetDone")
                    } else {
                        json!("")
                    },
                    vec![u.regular_button(
                        t("settings.dock.resetButton"),
                        state("dockAsk", json!(true)),
                        false,
                        false,
                    )],
                )]
            },
        ));
        stack(sections)
    }
    fn size_row(&self, label: &str, path: &str, min: f64, max: f64) -> Value {
        let u = self.ui();
        u.row(
            t(label),
            "",
            vec![
                u.text(t("settings.dock.small"), 11., true),
                u.slider(
                    path,
                    n(self.pref(path)),
                    min,
                    max,
                    1.,
                    150.,
                    patch(path, Value::Null),
                ),
                u.text(t("settings.dock.large"), 11., true),
            ],
        )
    }
    pub(super) fn clipboard(&self) -> Value {
        let u = self.ui();
        let mut limits = vec![];
        for (key, min, max, step, width) in [
            ("max", 50., 5000., 50., 60.),
            ("maxMB", 20., 2000., 20., 70.),
            ("expireDays", 0., 365., 1., 120.),
        ] {
            let path = format!("clipboard.{key}");
            let value = n(self.pref(&path));
            let label = match key {
                "maxMB" => json!(format!("{value} MB")),
                "expireDays" => {
                    if value > 0. {
                        tp("settings.clipboard.days", json!({"count":value}))
                    } else {
                        t("settings.clipboard.never")
                    }
                }
                _ => json!(value),
            };
            limits.push(u.row(
                t(&format!(
                    "settings.clipboard.{}",
                    if key == "expireDays" { "expire" } else { key }
                )),
                if key == "maxMB" || key == "expireDays" {
                    t(&format!(
                        "settings.clipboard.{}Hint",
                        if key == "expireDays" { "expire" } else { key }
                    ))
                } else {
                    json!("")
                },
                vec![u.stepper(&path, value, min, max, step, width, label)],
            ));
        }
        let ignored = arr(self.pref("clipboard.ignoreApps"));
        let mut ignore: Vec<_> = ignored
            .iter()
            .map(|a| {
                u.row(
                    a.clone(),
                    "",
                    vec![u.button(
                        "−",
                        json!({"op":"clipboardIgnore","value":a,"remove":true}),
                        false,
                        false,
                    )],
                )
            })
            .collect();
        let mut origins = vec![];
        for item in arr(self.d("clipboard.history")) {
            let origin = s(&item["origin"]);
            if !origin.is_empty()
                && !matches!(origin, "lordos" | "terminal" | "linux")
                && !ignored.contains(&json!(origin))
                && !origins.contains(&origin)
            {
                origins.push(origin);
            }
        }
        if !origins.is_empty() {
            ignore.push(view(
                json!({"flexDirection":"row","flexWrap":"wrap","gap":6,"paddingVertical":8}),
                origins
                    .into_iter()
                    .take(8)
                    .map(|o| {
                        u.button(
                            format!("+ {o}"),
                            json!({"op":"clipboardIgnore","value":o}),
                            false,
                            false,
                        )
                    })
                    .collect(),
            ));
        }
        ignore.push(view(json!({"flexDirection":"row","alignItems":"center","gap":8,"paddingVertical":8}),vec![json!({"kind":"input","key":"clipboard-app","value":self.st("clipboardApp"),"change":["field","clipboardApp"],"submit":action(op("clipboardAdd")),"placeholder":t("settings.clipboard.appId"),"style":{"flex":1}}),u.button(t("settings.clipboard.add"),op("clipboardAdd"),false,false)]));
        stack(vec![
            u.section(
                "",
                "",
                vec![
                    self.bool_row(
                        "settings.clipboard.save",
                        "settings.clipboard.saveHint",
                        "clipboard.save",
                    ),
                    self.bool_row(
                        "settings.clipboard.clearOnLogout",
                        "",
                        "clipboard.clearOnLogout",
                    ),
                ],
            ),
            u.section("", "", limits),
            u.section(
                t("settings.clipboard.ignore"),
                t("settings.clipboard.ignoreHint"),
                ignore,
            ),
            u.section(
                "",
                t("settings.clipboard.historyHint"),
                vec![u.row(
                    tp(
                        "settings.clipboard.history",
                        json!({"count":arr(self.d("clipboard.history")).len()}),
                    ),
                    "",
                    vec![
                        u.button(
                            t("settings.clipboard.open"),
                            call("clipboard.open", json!([])),
                            false,
                            false,
                        ),
                        u.button(
                            t("settings.clipboard.clearAll"),
                            call("clipboard.clear", json!([{"keepPinned":false}])),
                            false,
                            false,
                        ),
                    ],
                )],
            ),
            u.note(t("settings.clipboard.privacy"), ""),
        ])
    }
    pub(super) fn notifications(&self) -> Value {
        let u = self.ui();
        let p = self.pref("notifications");
        let sched = &p["schedule"];
        let mut focus=vec![u.row(t("prefs.notif.dnd"),t(if self.dnd_active(){"prefs.notif.dndOnHint"}else{"prefs.notif.dndOffHint"}),vec![u.toggle(p["dnd"]==true,patch("notifications.dnd",json!(p["dnd"]!=true)))]),u.row(t("prefs.notif.schedule"),if b(&sched["on"]){tp("prefs.notif.scheduleHint",json!({"from":hhmm(n(&sched["from"])as i64),"to":hhmm(n(&sched["to"])as i64)}))}else{json!("")},vec![u.toggle(b(&sched["on"]),patch("notifications.schedule.on",json!(!b(&sched["on"]))))])];
        if b(&sched["on"]) {
            focus.push(u.row(
                t("prefs.notif.from"),
                "",
                vec![
                    self.time_field("notifications.schedule.from"),
                    u.text(t("prefs.notif.to"), 12., true),
                    self.time_field("notifications.schedule.to"),
                ],
            ));
        }
        let mut ids: Vec<String> = arr(&p["seen"])
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        for id in [
            "settings",
            "backup",
            "clock",
            "terminal",
            "store",
            "mail",
            "passwords",
            "recorder",
            "screenshots",
        ] {
            if arr(self.d("registry")).iter().any(|a| a["id"] == id) && !ids.iter().any(|i| i == id)
            {
                ids.push(id.into());
            }
        }
        for a in arr(self.d("packages.list")) {
            if arr(&a["permissions"]).contains(&json!("notifications"))
                && !ids.iter().any(|i| i == s(&a["id"]))
            {
                ids.push(s(&a["id"]).into());
            }
        }
        let mut apps: Vec<_> = ids
            .into_iter()
            .filter_map(|id| {
                let app = arr(self.d("catalog"))
                    .iter()
                    .chain(arr(self.d("registry")))
                    .find(|a| {
                        a["id"] == id
                            || id
                                .strip_prefix("linux:")
                                .is_some_and(|did| s(&a["desktopId"]).eq_ignore_ascii_case(did))
                    });
                let name = app
                    .map(|a| s(&a["name"]).to_owned())
                    .or_else(|| id.strip_prefix("linux:").map(String::from))?;
                Some((name, id, app.cloned().unwrap_or(Value::Null)))
            })
            .collect();
        apps.sort_by_key(|(name, _, _)| name.to_lowercase());
        let rows = apps
            .into_iter()
            .map(|(name, id, app)| {
                let mut pref = json!({"allow":true,"style":"banner","sound":true,"lock":true});
                merge(&mut pref, &p["apps"][&id]);
                if b(&app["package"]) {
                    pref["allow"] =
                        json!(!arr(self.d("perms.list")).iter().any(|d| d["app"] == id
                            && d["perm"] == "notifications"
                            && d["state"] == "deny"));
                }
                let allow = b(&pref["allow"]);
                let open = b(&self.state["notifyOpen"][&id]);
                let mut row = u.row(
                    name,
                    if !allow {
                        t("prefs.notif.off")
                    } else {
                        json!(format!(
                            "{}{}",
                            self.tr(&format!("prefs.notif.style.{}", s(&pref["style"]))),
                            if b(&pref["sound"]) {
                                format!(", {}", self.tr("prefs.notif.sound"))
                            } else {
                                String::new()
                            }
                        ))
                    },
                    vec![u.text(if open { "⌃" } else { "⌄" }, 12., true)],
                );
                row=u.row_icon(row,if app.is_null(){view(json!({"width":28,"height":28,"borderRadius":7,"backgroundColor":u.color("control")}),vec![])}else{json!({"kind":"appIcon","app":app["id"],"size":28})});
                row = click(row, json!({"op":"notifyExpand","app":id,"value":!open}));
                let mut nodes = vec![row];
                if open {
                    let bind = |field: &str| json!({"op":"notifyPref","app":id,"field":field});
                    let cmd = |field: &str, value: Value| {
                        let mut c = bind(field);
                        c["value"] = value;
                        c
                    };
                    nodes.push(view(
                        json!({"paddingBottom":10,"paddingLeft":40,"gap":2}),
                        vec![
                            u.row(
                                t("prefs.notif.allow"),
                                if b(&app["package"]) {
                                    t("prefs.notif.allowPackage")
                                } else {
                                    json!("")
                                },
                                vec![u.toggle(allow, cmd("allow", json!(!allow)))],
                            ),
                            u.disabled(
                                u.row(
                                    t("prefs.notif.styleLabel"),
                                    t(&format!("prefs.notif.styleHint.{}", s(&pref["style"]))),
                                    vec![u.segmented(
                                        pref["style"].clone(),
                                        opts(&["none", "banner", "alert"], "prefs.notif.style."),
                                        bind("style"),
                                    )],
                                ),
                                !allow,
                            ),
                            u.disabled(
                                u.row(
                                    t("prefs.notif.playSound"),
                                    "",
                                    vec![u.toggle(
                                        b(&pref["sound"]),
                                        cmd("sound", json!(!b(&pref["sound"]))),
                                    )],
                                ),
                                !allow,
                            ),
                            u.disabled(
                                u.row(
                                    t("prefs.notif.lockScreen"),
                                    t("prefs.notif.lockScreenHint"),
                                    vec![u.toggle(b(&pref["lock"]), Value::Null)],
                                ),
                                true,
                            ),
                        ],
                    ));
                }
                view(json!({}), nodes)
            })
            .collect();
        stack(vec![
            u.section(t("prefs.notif.focus"), t("prefs.notif.dndFooter"), focus),
            u.section(
                "",
                "",
                vec![
                    u.row(
                        t("prefs.notif.grouping"),
                        t("prefs.notif.groupingHint"),
                        vec![u.pop(
                            "notification-grouping",
                            p["grouping"].clone(),
                            vec![
                                json!(["app", t("prefs.notif.byApp")]),
                                json!(["off", t("prefs.notif.noGrouping")]),
                            ],
                            patch("notifications.grouping", Value::Null),
                            190.,
                            false,
                        )],
                    ),
                    u.row(
                        t("prefs.notif.test"),
                        "",
                        vec![u.button(t("prefs.notif.testButton"), op("notifyTest"), false, false)],
                    ),
                ],
            ),
            u.section(t("prefs.notif.apps"), t("prefs.notif.appsFooter"), rows),
        ])
    }
    fn dnd_active(&self) -> bool {
        let p = self.pref("notifications");
        if p["dnd"] == true || p["dnd"].as_i64().is_some_and(|until| self.now < until) {
            return true;
        }
        let tm = crate::clock::local(self.now);
        let from = n(&p["schedule"]["from"]) as i64;
        let to = n(&p["schedule"]["to"]) as i64;
        let len = (to - from).rem_euclid(1440);
        b(&p["schedule"]["on"])
            && len > 0
            && (tm.tm_hour as i64 * 60 + tm.tm_min as i64 - from).rem_euclid(1440) < len
    }
    fn time_field(&self, path: &str) -> Value {
        let u = self.ui();
        let value = n(self.pref(path)) as i64;
        view(
            json!({"flexDirection":"row","alignItems":"center","gap":2,"direction":"ltr"}),
            vec![
                view(
                    json!({"height":24,"minWidth":66,"paddingHorizontal":8,"borderRadius":6,"backgroundColor":u.color("control"),"justifyContent":"center"}),
                    vec![u.text(hhmm(value), 12., false)],
                ),
                view(
                    json!({}),
                    vec![
                        click(
                            view(
                                json!({"width":16,"height":11,"alignItems":"center","justifyContent":"center"}),
                                vec![u.text("▲", 7., true)],
                            ),
                            patch(path, json!((value + 15).rem_euclid(1440))),
                        ),
                        click(
                            view(
                                json!({"width":16,"height":11,"alignItems":"center","justifyContent":"center"}),
                                vec![u.text("▼", 7., true)],
                            ),
                            patch(path, json!((value - 15).rem_euclid(1440))),
                        ),
                    ],
                ),
            ],
        )
    }
    pub(super) fn personal_command(&mut self, c: &Value) -> bool {
        if c["op"] == "notifyExpand" {
            self.state["notifyOpen"][s(&c["app"])] = c["value"].clone();
            return true;
        }
        match s(&c["op"]){
        "nativeAppearance"=>{let v=if self.pref("nativeAppearance")=="axia"{"adwaita"}else{"axia"};self.patch_many(json!({"nativeAppearance":v,"filesAppearance":v}));},
        "theme"=>if c["current"]!=true{self.effects.push_back(json!({"kind":"openApp","app":"settings","args":{"section":"appearance"}}));self.call("themes.use",json!([c["value"]]),Value::Null);},
        "dockReset"=>{if b(self.st("dockAsk")){self.call("dock.reset",json!([]),json!({"op":"dockResetDone"}));}},
        "clipboardAdd"=>{let app=self.st("clipboardApp").clone();self.personal_command(&json!({"op":"clipboardIgnore","value":app}));self.state["clipboardApp"]=json!("");},
        "clipboardIgnore"=>{let mut apps=arr(self.pref("clipboard.ignoreApps")).to_vec();if b(&c["remove"]){apps.retain(|x|x!=&c["value"]);}else if !s(&c["value"]).is_empty()&&!apps.contains(&c["value"]){apps.push(c["value"].clone());}self.patch("clipboard.ignoreApps",json!(apps));},
        "notifyTest"=>self.call("notify",json!([{"app":"settings","title":self.tr("prefs.notif.testTitle"),"body":self.tr("prefs.notif.testBody")}]),Value::Null),
        "notifyPref"=>{let id=s(&c["app"]);let field=s(&c["field"]);if !matches!(field,"allow"|"style"|"sound"){return true;}if field!="allow"&&self.pref("notifications")["apps"][id]["allow"]==false{return true;}if field=="allow"&&arr(self.d("registry")).iter().any(|a|a["id"]==id&&b(&a["package"])){self.call("perms.set",json!([id,"notifications",if b(&c["value"]){"allow"}else{"deny"}]),Value::Null);}let mut apps=self.pref("notifications.apps").clone();if !apps[id].is_object(){apps[id]=json!({});}apps[id][field]=c["value"].clone();self.patch("notifications.apps",apps);},
        _=>return false,
    }
        true
    }
    pub(super) fn personal_after(&mut self, after: &Value, _value: &Value) {
        if after["op"] == "dockResetDone" {
            self.state["dockAsk"] = json!(false);
            self.state["dockDone"] = json!(true);
        }
    }
}
pub(super) fn hhmm(value: i64) -> String {
    format!(
        "{:02}:{:02}",
        value.div_euclid(60).rem_euclid(24),
        value.rem_euclid(60)
    )
}
fn traffic_lights() -> Value {
    view(
        json!({"flexDirection":"row","gap":3,"padding":4}),
        ["#ff5f57", "#febc2e", "#28c840"]
            .iter()
            .map(|c| {
                view(
                    json!({"width":4,"height":4,"borderRadius":2,"backgroundColor":c}),
                    vec![],
                )
            })
            .collect(),
    )
}
fn mode_thumb(dark: bool, accent: Value) -> Value {
    view(
        json!({"width":84,"height":56,"borderRadius":7,"overflow":"hidden","direction":"ltr","background":if dark{"linear-gradient(160deg, #2b2d4a, #121218)"}else{"linear-gradient(160deg, #cfe0ff, #eef1f7)"}}),
        vec![view(
            json!({"position":"absolute","left":10,"top":10,"right":10,"bottom":-6,"borderRadius":5,"backgroundColor":if dark{"#2c2c30"}else{"#ffffff"},"boxShadow":"0 1px 4px rgba(0,0,0,.3)"}),
            vec![
                traffic_lights(),
                view(
                    json!({"marginLeft":6,"height":4,"borderRadius":2,"backgroundColor":accent,"width":26}),
                    vec![],
                ),
                view(
                    json!({"marginLeft":6,"marginTop":4,"height":3,"borderRadius":2,"backgroundColor":if dark{"#4a4a50"}else{"#d8d8de"},"width":40}),
                    vec![],
                ),
                view(
                    json!({"marginLeft":6,"marginTop":3,"height":3,"borderRadius":2,"backgroundColor":if dark{"#4a4a50"}else{"#d8d8de"},"width":32}),
                    vec![],
                ),
            ],
        )],
    )
}
fn style_thumb(dark: bool, accent: Value) -> Value {
    let glass = if dark {
        "rgba(30,30,36,.85)"
    } else {
        "rgba(255,255,255,.82)"
    };
    view(json!({"width":132,"height":82,"borderRadius":7,"overflow":"hidden","direction":"ltr","background":if dark{"linear-gradient(160deg, #2b2d4a, #121218)"}else{"linear-gradient(160deg, #cfe0ff, #eef1f7)"}}),vec![view(json!({"position":"absolute","left":0,"right":0,"top":0,"height":6,"backgroundColor":glass}),vec![]),view(json!({"position":"absolute","left":22,"top":14,"width":86,"height":50,"borderRadius":5,"backgroundColor":if dark{"#2c2c30"}else{"#ffffff"},"boxShadow":"0 1px 4px rgba(0,0,0,.3)"}),vec![traffic_lights(),view(json!({"marginLeft":6,"height":4,"borderRadius":2,"backgroundColor":accent,"width":30}),vec![])]),view(json!({"position":"absolute","left":36,"right":36,"bottom":3,"height":9,"borderRadius":3,"backgroundColor":glass,"flexDirection":"row","alignItems":"center","justifyContent":"center","gap":3}),(0..4).map(|i|view(json!({"width":5,"height":5,"borderRadius":1.5,"backgroundColor":if i==0{accent.clone()}else{json!("#8e8e93")}}),vec![])).collect())])
}
