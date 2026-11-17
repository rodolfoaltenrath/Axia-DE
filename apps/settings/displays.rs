use super::*;
impl App {
    fn selected_display(&self) -> Value {
        let outs = arr(&self.d("display.list")["outputs"]);
        outs.iter()
            .find(|o| o["id"] == *self.st("display") && b(&o["active"]))
            .or_else(|| outs.iter().find(|o| b(&o["main"])))
            .or_else(|| outs.iter().find(|o| b(&o["active"])))
            .or_else(|| outs.first())
            .cloned()
            .unwrap_or(Value::Null)
    }
    pub(super) fn displays(&self) -> Value {
        let u = self.ui();
        let d = self.d("display.list");
        if d.is_null() {
            return u.note(t("prefs.display.unavailable"), "");
        }
        let cur = self.selected_display();
        if !b(&d["onMonitor"]) || cur.is_null() {
            return stack(vec![
                u.note(t("prefs.display.notOnMonitor"), ""),
                self.night(),
                self.browser_video(),
            ]);
        }
        let outs = arr(&d["outputs"]);
        let active = outs.iter().filter(|o| b(&o["active"])).count();
        let several = active > 1 || outs.len() > 1;
        let mode = if cur["mode"].is_object() {
            cur["mode"].clone()
        } else {
            cur["preferred"].clone()
        };
        let scale = cur["scale"].as_f64().unwrap_or(100.);
        let rotation = cur["rotation"].as_i64().unwrap_or(0);
        let rec = cur["recommended"].as_f64().unwrap_or(100.);
        let resolutions = arr(&cur["resolutions"]);
        let current_resolution = resolutions
            .iter()
            .find(|r| r["w"] == mode["w"] && r["h"] == mode["h"])
            .or_else(|| resolutions.first());
        let mut out = vec![self.arrangement()];
        let looks = if scale != 100. && n(&cur["w"]) > 0. {
            self.caption(&tp(
                "prefs.display.looksLike",
                json!({"w":cur["w"],"h":cur["h"]}),
            ))
        } else {
            String::new()
        };
        let size = [
            s(&cur["connector"]).to_owned(),
            if n(&cur["diagonal"]) > 0. {
                self.caption(&tp(
                    "prefs.display.inches",
                    json!({"n":format!("{:.1}",n(&cur["diagonal"])/10.)}),
                ))
            } else {
                String::new()
            },
            format!("{} × {}", mode["w"], mode["h"]),
            looks.clone(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        let numbers = crate::winmodel::call("display.numbers", &[d["outputs"].clone()])
            .unwrap_or(Value::Null);
        let mut name = u.text(cur["name"].clone(), 13., false);
        name["style"]["fontWeight"] = json!(600);
        let mut titles = vec![name];
        let num = &numbers[cur["id"].to_string().trim_matches('"')];
        let pill = |label| {
            view(
                json!({"height":16,"paddingHorizontal":6,"borderRadius":8,"justifyContent":"center","backgroundColor":if u.dark(){"rgba(255,255,255,0.1)"}else{"rgba(0,0,0,0.07)"}}),
                vec![u.text(label, 10., true)],
            )
        };
        if num.as_i64().is_some_and(|n| n > 0) {
            titles.push(pill(tp("prefs.display.number", json!({"n":num}))));
        }
        if b(&cur["main"]) && several {
            titles.push(pill(t("prefs.display.mainLabel")));
        }
        let display_glyph = view(
            json!({"width":36,"height":36,"alignItems":"center"}),
            vec![
                view(
                    json!({"width":36,"height":36.*0.68,"borderRadius":36.*0.08,"border":format!("2px solid {}",s(&u.color("accent"))),"backgroundColor":"rgba(10,132,255,0.12)"}),
                    vec![],
                ),
                view(
                    json!({"width":36.*0.12,"height":36.*0.14,"backgroundColor":u.color("accent")}),
                    vec![],
                ),
                view(
                    json!({"width":36.*0.42,"height":2,"borderRadius":2,"backgroundColor":u.color("accent")}),
                    vec![],
                ),
            ],
        );
        let mut details = vec![view(
            json!({"minHeight":56,"flexDirection":"row","alignItems":"center","gap":12,"paddingVertical":8}),
            vec![
                display_glyph,
                view(
                    json!({"flex":1,"minWidth":0,"gap":2}),
                    vec![
                        view(
                            json!({"flexDirection":"row","alignItems":"center","gap":8}),
                            titles,
                        ),
                        u.text(size, 11., true),
                    ],
                ),
            ],
        )];
        if several {
            details.push(u.disabled(
                u.row(
                    t("prefs.display.useAsMain"),
                    if b(&cur["main"]) {
                        t("prefs.display.mainHint")
                    } else {
                        json!("")
                    },
                    vec![u.toggle(b(&cur["main"]), json!({"op":"displayMain","id":cur["id"]}))],
                ),
                !b(&cur["active"]) || b(&d["mirror"]),
            ));
        }
        let brightness = self.d("display.brightness");
        let devices = arr(&brightness["devices"]);
        let mut sliders = vec![];
        for device in devices {
            if devices.len() > 1 {
                sliders.push(u.text(device["name"].clone(), 11., true));
            }
            sliders.push(view(
                json!({"flexDirection":"row","alignItems":"center","gap":8,"direction":"ltr"}),
                vec![
                    u.text("☀", 18., true),
                    u.slider(
                        &format!("brightness:{}", s(&device["name"])),
                        n(&device["percent"]),
                        1.,
                        100.,
                        1.,
                        196.,
                        json!({"op":"brightness","name":device["name"]}),
                    ),
                    u.text(format!("{} %", device["percent"]), 11., true),
                ],
            ));
        }
        if devices.is_empty() {
            sliders.push(u.note(t("shell.cc.brightnessUnavailable"), ""));
        }
        if !s(&brightness["error"]).is_empty() {
            sliders.push(u.note(t("shell.cc.brightnessFailed"), "warn"));
        }
        details.push(u.row(
            t("prefs.display.brightness"),
            "",
            vec![view(json!({"gap":8}), sliders)],
        ));
        out.push(u.section("", "", details));
        let scale_accessory = view(
            json!({"flexDirection":"row","alignItems":"center","gap":8}),
            vec![
                u.text(t("prefs.display.showList"), 11., true),
                u.toggle(
                    b(self.st("scaleList")),
                    state("scaleList", json!(!b(self.st("scaleList")))),
                ),
            ],
        );
        let mut rows = vec![];
        let scale_options = (0..5)
            .map(|i| {
                let n = 100 + i * 25;
                json!([
                    n,
                    tp(
                        if n as f64 == rec {
                            "prefs.display.scaleRecommended"
                        } else {
                            "prefs.display.scaleN"
                        },
                        json!({"n":n})
                    )
                ])
            })
            .collect();
        if b(self.st("scaleList")) {
            rows.push(u.row(
                t("prefs.display.scale"),
                looks,
                vec![u.pop(
                    "display-scale",
                    json!(scale),
                    scale_options,
                    json!({"op":"displayMode","field":"scale"}),
                    210.,
                    false,
                )],
            ));
        } else {
            rows.push(view(json!({"paddingTop":12,"paddingBottom":8,"gap":4}),vec![view(json!({"flexDirection":"row","alignItems":"flex-start","gap":4}),[200,175,150,125,100].iter().map(|n|{let on=*n as f64==scale;let ww=(20.+(*n as f64-100.)*0.26).round();let wh=(ww*0.64).round();click(view(json!({"flex":1,"minWidth":0,"alignItems":"center","gap":5}),vec![view(json!({"width":70,"height":48,"padding":2,"borderRadius":8,"border":format!("2px solid {}",if on{s(&u.color("accent")).to_owned()}else{"transparent".into()})}),vec![view(json!({"flex":1,"borderRadius":4,"overflow":"hidden","alignItems":"center","justifyContent":"center","background":"linear-gradient(160deg, #3d7be0, #7b46cf)","direction":"ltr"}),vec![view(json!({"position":"absolute","left":0,"right":0,"top":0,"height":(*n as f64/50.).round().max(2.),"backgroundColor":"rgba(255,255,255,.8)"}),vec![]),view(json!({"width":ww,"height":wh,"marginTop":3,"borderRadius":2,"backgroundColor":"rgba(246,246,249,.95)","boxShadow":"0 1px 3px rgba(0,0,0,.3)"}),vec![view(json!({"height":(wh/6.).round().max(2.),"backgroundColor":"rgba(0,0,0,.12)","borderRadius":2}),vec![])])])]),u.text(tp("prefs.display.scaleN",json!({"n":n})),11.,!on),u.text(if *n as f64==rec{t("prefs.display.recommended")}else{json!("")},10.,true)]),json!({"op":"displayMode","field":"scale","value":n}))}).collect()),view(json!({"flexDirection":"row","justifyContent":"space-between","paddingHorizontal":4}),vec![u.text(t("prefs.display.largerText"),11.,true),u.text(t("prefs.display.moreSpace"),11.,true)])]));
        }
        rows.push(u.row(
            t("prefs.display.resolution"),
            "",
            vec![u.pop(
                    "display-resolution",
                    json!(format!("{}x{}", mode["w"], mode["h"])),
                    resolutions
                        .iter()
                        .map(|r| {
                            json!([
                                format!("{}x{}", r["w"], r["h"]),
                                if r["w"] == cur["preferred"]["w"]
                                    && r["h"] == cur["preferred"]["h"]
                                {
                                    tp("prefs.display.native", json!({"w":r["w"],"h":r["h"]}))
                                } else {
                                    json!(format!("{} × {}", r["w"], r["h"]))
                                }
                            ])
                        })
                        .collect(),
                    json!({"op":"displayResolution"}),
                    210.,
                    false,
                )],
        ));
        let rates = current_resolution.map(|r| arr(&r["rates"])).unwrap_or(&[]);
        rows.push(u.row(t("prefs.display.refresh"),"",vec![u.pop("display-refresh",mode["refresh"].clone(),rates.iter().map(|r|{let rate=n(r)/1000.;json!([r,tp("prefs.display.hz",json!({"hz":if(rate-rate.round()).abs()<0.005{format!("{rate:.0}")}else{format!("{rate:.2}")}}))])}).collect(),json!({"op":"displayMode","field":"refresh"}),210.,rates.len()<2)]));
        rows.push(u.row(
            t("prefs.display.rotation"),
            if rotation != 0 {
                t("prefs.display.rotationHint")
            } else {
                json!("")
            },
            vec![u.pop(
                "display-rotation",
                json!(rotation),
                vec![
                    json!([0, t("prefs.display.rotStandard")]),
                    json!([1, "90°"]),
                    json!([2, "180°"]),
                    json!([3, "270°"]),
                ],
                json!({"op":"displayMode","field":"rotation"}),
                210.,
                false,
            )],
        ));
        out.push(u.accessory(
            u.section(t("prefs.display.scale"), t("prefs.display.scaleHint"), rows),
            scale_accessory,
        ));
        if several {
            let mut rows = vec![
                u.disabled(
                    u.row(
                        t("prefs.display.mirrorDisplays"),
                        "",
                        vec![u.toggle(
                            b(&d["mirror"]),
                            call("display.mirror", json!([!b(&d["mirror"])])),
                        )],
                    ),
                    active < 2 && !b(&d["mirror"]),
                ),
                u.disabled(
                    u.row(
                        t("prefs.display.menuBarAll"),
                        "",
                        vec![u.toggle(
                            b(self.pref("menuBarAllDisplays")),
                            patch(
                                "menuBarAllDisplays",
                                json!(!b(self.pref("menuBarAllDisplays"))),
                            ),
                        )],
                    ),
                    b(&d["mirror"]),
                ),
                u.row(
                    t("prefs.display.capture"),
                    t("prefs.display.captureHint"),
                    vec![u.pop(
                        "display-capture",
                        json!(if b(&d["captureAll"]) { "all" } else { "main" }),
                        vec![
                            json!(["main", t("prefs.display.captureMain")]),
                            json!(["all", t("prefs.display.captureAll")]),
                        ],
                        op("displayCapture"),
                        170.,
                        false,
                    )],
                ),
            ];
            for o in outs.iter().filter(|o| !b(&o["active"])) {
                rows.push(u.disabled(
                    u.row(
                        o["name"].clone(),
                        tp("prefs.display.noCrtc", json!({"connector":o["connector"]})),
                        vec![],
                    ),
                    true,
                ));
            }
            out.push(u.section(
                t("prefs.display.multiple"),
                t("prefs.display.multipleHint"),
                rows,
            ));
        }
        out.push(self.night());
        if b(&d["softTint"]) {
            out.push(u.note(t("prefs.night.software"), ""));
        }
        out.push(self.browser_video());
        stack(out)
    }
    fn arrangement(&self) -> Value {
        let u = self.ui();
        let d = self.d("display.list");
        let outs: Vec<_> = arr(&d["outputs"])
            .iter()
            .filter(|o| b(&o["active"]) && n(&o["w"]) > 0.)
            .collect();
        let width = self.st("contentWidth").as_f64().unwrap_or(520.);
        let height = 180.;
        let mirror = b(&d["mirror"]);
        let nums = crate::winmodel::call("display.numbers", &[d["outputs"].clone()])
            .unwrap_or(Value::Null);
        let selected = self.selected_display();
        let positions: Vec<_> = outs
            .iter()
            .enumerate()
            .map(|(i, o)| {
                if self.st("displayPreview")["id"] == o["id"] {
                    (
                        n(&self.st("displayPreview")["x"]),
                        n(&self.st("displayPreview")["y"]),
                    )
                } else if mirror {
                    (i as f64 * 18., i as f64 * 14.)
                } else {
                    (n(&o["x"]), n(&o["y"]))
                }
            })
            .collect();
        let x0 = positions.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let y0 = positions.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let x1 = outs
            .iter()
            .zip(&positions)
            .map(|(o, p)| p.0 + n(&o["w"]))
            .fold(0., f64::max);
        let y1 = outs
            .iter()
            .zip(&positions)
            .map(|(o, p)| p.1 + n(&o["h"]))
            .fold(0., f64::max);
        let k = ((width - 56.) / (x1 - x0).max(1.)).min((height - 40.) / (y1 - y0).max(1.));
        let transform = if self.drag["op"] == "displayDrag" {
            self.drag["view"].clone()
        } else {
            json!({"k":k,"x0":x0,"y0":y0,"offX":(width-(x1-x0)*k)/2.,"offY":(height-(y1-y0)*k)/2.})
        };
        let k = n(&transform["k"]);
        let mut tiles = vec![];
        for (i, o) in outs.iter().enumerate() {
            let on = o["id"] == selected["id"];
            let lit = self.st("displayFlash") == &o["id"]
                && self.now < self.st("displayFlashUntil").as_i64().unwrap_or(0);
            let w = (n(&o["w"]) * k - 3.).max(28.);
            let h = (n(&o["h"]) * k - 3.).max(18.);
            let mut labels = vec![];
            if b(&o["main"]) {
                labels.push(view(json!({"position":"absolute","left":0,"right":0,"top":0,"height":5,"backgroundColor":"rgba(255,255,255,.85)"}),vec![]));
            }
            let number = nums[o["id"].to_string().trim_matches('"')].clone();
            let mut title = u.text(
                if outs.len() > 1 && !mirror {
                    json!(format!("{}  {}", number, s(&o["name"])))
                } else {
                    o["name"].clone()
                },
                11.,
                false,
            );
            title["style"]["color"] = json!("#fff");
            labels.push(title);
            if h > 44. {
                let mut text = u.text(
                    format!("{} × {}", o["mode"]["w"], o["mode"]["h"]),
                    10.,
                    false,
                );
                text["style"]["color"] = json!("rgba(255,255,255,.82)");
                labels.push(text);
            }
            let mut tile = view(
                json!({"position":"absolute","left":n(&transform["offX"])+(positions[i].0-n(&transform["x0"]))*k-3.,"top":n(&transform["offY"])+(positions[i].1-n(&transform["y0"]))*k-3.,"width":w+6.,"height":h+6.,"padding":1,"borderRadius":8,"border":format!("2px solid {}",if on||lit{s(&u.color("accent")).to_owned()}else{"transparent".into()})}),
                vec![view(
                    json!({"flex":1,"borderRadius":5,"overflow":"hidden","alignItems":"center","justifyContent":"center","gap":1,"background":if lit{"linear-gradient(160deg, #6aa4ff, #a57cf0)"}else if on{"linear-gradient(160deg, #3d7be0, #7b46cf)"}else{"linear-gradient(160deg, #4f6fae, #5d4790)"},"boxShadow":"0 3px 10px rgba(0,0,0,.3)"}),
                    labels,
                )],
            );
            tile["events"]["onMouseDown"] = action(
                json!({"op":"displayStart","id":o["id"],"view":transform,"x":o["x"],"y":o["y"],"movable":outs.len()>1&&!mirror}),
            );
            tiles.push(tile);
        }
        let mut out = vec![view(
            json!({"height":height,"borderRadius":10,"direction":"ltr","overflow":"hidden","backgroundColor":if u.dark(){"rgba(0,0,0,.22)"}else{"rgba(0,0,0,.045)"},"border":format!("1px solid {}",s(&u.color("separator")))}),
            tiles,
        )];
        if outs.len() > 1 && !mirror {
            out.push(view(
                json!({"flexDirection":"row","alignItems":"flex-start","gap":8}),
                vec![
                    u.note(t("prefs.display.arrangeHint"), ""),
                    u.button(
                        t("prefs.display.identify"),
                        call("display.identify", json!([])),
                        false,
                        false,
                    ),
                ],
            ));
        }
        view(json!({"gap":6}), out)
    }
    fn night(&self) -> Value {
        let u = self.ui();
        let p = self.pref("night");
        let mut rows = vec![u.row(
            t("prefs.night.schedule"),
            if p["schedule"] == "sun" {
                self.sun_window()
                    .map(|(from, to)| {
                        tp(
                            "prefs.night.sunHint",
                            json!({"set":self.clock_minutes(from),"rise":self.clock_minutes(to)}),
                        )
                    })
                    .unwrap_or_else(|| t("prefs.night.noSun"))
            } else {
                t(if p["schedule"] == "custom" {
                    "prefs.night.customHint"
                } else {
                    "prefs.night.offHint"
                })
            },
            vec![u.pop(
                "night-schedule",
                p["schedule"].clone(),
                opts(&["off", "sun", "custom"], "prefs.night."),
                op("nightSchedule"),
                190.,
                false,
            )],
        )];
        if p["schedule"] == "custom" {
            rows.push(u.row(
                t("prefs.night.from"),
                "",
                vec![
                    self.night_time("from"),
                    u.text(t("prefs.night.to"), 12., true),
                    self.night_time("to"),
                ],
            ));
        }
        let tm = crate::clock::local(self.now);
        let m = tm.tm_hour as f64 * 60. + tm.tm_min as f64 + tm.tm_sec as f64 / 60.;
        let window = match s(&p["schedule"]) {
            "custom" => Some((n(&p["from"]) as i64, n(&p["to"]) as i64)),
            "sun" => Some(self.sun_window().unwrap_or((1320, 420))),
            _ => None,
        };
        let active =
            if p["manual"].is_object() && self.now < p["manual"]["until"].as_i64().unwrap_or(0) {
                b(&p["manual"]["on"])
            } else {
                window.is_some_and(|(from, to)| night_active(m, from, to))
            };
        rows.push(u.row(
            t("prefs.night.now"),
            if p["manual"].is_object()&&self.now<p["manual"]["until"].as_i64().unwrap_or(0){let until=crate::clock::local(p["manual"]["until"].as_i64().unwrap_or(0));tp(if b(&p["manual"]["on"]){"prefs.night.manualOn"}else{"prefs.night.manualOff"},json!({"time":crate::native_view::date_format::time(&mut self.i18n.borrow_mut(),&self.region,s(self.pref("clock.hour24")),&until,false)}))}else{json!("")},
            vec![u.toggle(active, call("night.manual", json!([!active])))],
        ));
        let temp = self.st("nightTemp").as_f64().unwrap_or(n(&p["temp"]));
        rows.push(u.row(
            t("prefs.night.temp"),
            "",
            vec![
                u.text(t("prefs.night.lessWarm"), 11., true),
                u.slider(
                    "night-temp",
                    6500. - temp,
                    500.,
                    4600.,
                    1.,
                    150.,
                    op("nightTemperature"),
                ),
                u.text(t("prefs.night.moreWarm"), 11., true),
            ],
        ));
        u.section(t("prefs.night.title"), t("prefs.night.footer"), rows)
    }
    fn sun_window(&self) -> Option<(i64, i64)> {
        let sun = self.d("sun");
        let set = sun["set"].as_f64()?;
        let rise = sun["rise"].as_f64()?;
        let mut tm = crate::clock::local(self.now);
        tm.tm_hour = 0;
        tm.tm_min = 0;
        tm.tm_sec = 0;
        let midnight = unsafe { libc::timegm(&mut tm) } * 1000;
        let local = |minute: f64| {
            let tm = crate::clock::local(midnight + (minute * 60000.) as i64);
            tm.tm_hour as i64 * 60 + tm.tm_min as i64
        };
        Some((local(set), local(rise)))
    }
    fn clock_minutes(&self, minutes: i64) -> String {
        let mut tm = crate::clock::local(self.now);
        tm.tm_hour = (minutes / 60) as i32;
        tm.tm_min = (minutes % 60) as i32;
        tm.tm_sec = 0;
        crate::native_view::date_format::time(
            &mut self.i18n.borrow_mut(),
            &self.region,
            s(self.pref("clock.hour24")),
            &tm,
            false,
        )
    }
    fn night_time(&self, key: &str) -> Value {
        let u = self.ui();
        let value = n(&self.pref("night")[key]) as i64;
        view(
            json!({"flexDirection":"row","alignItems":"center","gap":2}),
            vec![
                u.text(self.clock_minutes(value), 12., false),
                view(
                    json!({}),
                    vec![
                        u.button(
                            "▴",
                            patch(
                                &format!("night.{key}"),
                                json!((value + 15).rem_euclid(1440)),
                            ),
                            false,
                            false,
                        ),
                        u.button(
                            "▾",
                            patch(
                                &format!("night.{key}"),
                                json!((value - 15).rem_euclid(1440)),
                            ),
                            false,
                            false,
                        ),
                    ],
                ),
            ],
        )
    }
    fn browser_video(&self) -> Value {
        let u = self.ui();
        let prefs = self.pref("video");
        let hw = self.d("video.hardware");
        let hint = match s(&prefs["mode"]) {
            "always" => t("prefs.video.statusAlways"),
            "never" => t("prefs.video.statusNever"),
            _ if hw.is_null() => t("prefs.video.checking"),
            _ if b(&hw["vp9"]) && b(&hw["av1"]) => t("prefs.video.statusHardware"),
            _ => tp(
                "prefs.video.statusAuto",
                json!({"codecs":if !b(&hw["vp9"])&&!b(&hw["av1"]){self.tr("prefs.video.codecsBoth")}else if !b(&hw["vp9"]){"VP9".into()}else{"AV1".into()},"reason":self.tr(&format!("prefs.video.reason.{}",match s(&hw["reason"]){"virtualGpu"=>"virtualGpu","noDriver"=>"noDriver",_=>"noProfiles"}))}),
            ),
        };
        let mut rows = vec![
            u.row(
                t("prefs.video.optimize"),
                hint,
                vec![u.pop(
                    "video-mode",
                    prefs["mode"].clone(),
                    opts(&["auto", "always", "never"], "prefs.video.mode."),
                    patch("video.mode", Value::Null),
                    170.,
                    false,
                )],
            ),
            u.row(
                t("prefs.video.autoLower"),
                t("prefs.video.autoLowerHint"),
                vec![u.toggle(
                    b(&prefs["autoLower"]),
                    patch("video.autoLower", json!(!b(&prefs["autoLower"]))),
                )],
            ),
        ];
        if !s(self.d("video.error")).is_empty() {
            rows.push(u.note(self.d("video.error").clone(), "warn"));
        }
        u.section(t("prefs.video.title"), t("prefs.video.footer"), rows)
    }
    pub(super) fn display_command(&mut self, c: &Value) -> bool {
        match s(&c["op"]) {
            "displayMode" | "displayResolution" => {
                let cur = self.selected_display();
                let mut mode = json!({"w":cur["mode"]["w"],"h":cur["mode"]["h"],"refresh":cur["mode"]["refresh"],"rotation":cur["rotation"].as_i64().unwrap_or(0),"scale":cur["scale"].as_i64().unwrap_or(100)});
                if c["op"] == "displayResolution" {
                    if let Some((w, h)) = s(&c["value"]).split_once('x') {
                        mode["w"] = json!(w.parse::<u32>().unwrap_or(0));
                        mode["h"] = json!(h.parse::<u32>().unwrap_or(0));
                        mode["refresh"] = json!(0);
                    }
                } else if matches!(s(&c["field"]), "scale" | "rotation" | "refresh") {
                    mode[s(&c["field"])] = c["value"].clone();
                }
                self.call(
                    "display.apply",
                    json!([cur, mode]),
                    json!({"op":"reportCall"}),
                );
            }
            "displayMain" => {
                let cur = self.selected_display();
                if b(&cur["active"]) && !b(&cur["main"]) && !b(&self.d("display.list")["mirror"]) {
                    self.call("display.main", json!([c["id"]]), Value::Null);
                }
            }
            "displayCapture" => {
                self.call("display.capture", json!([c["value"] == "all"]), Value::Null)
            }
            "nightSchedule" => {
                let mut night = self.pref("night").clone();
                night["schedule"] = c["value"].clone();
                night["manual"] = Value::Null;
                self.patch_many(json!({"night":night}));
            }
            "nightTemperature" => {
                let temp = 6500. - n(&c["value"]);
                self.state["nightTemp"] = json!(temp);
                self.patch("night.temp", json!(temp));
                self.call_latest("night.preview", json!([temp]), "night-preview".into());
            }
            "brightness" => {
                self.call_latest("display.brightness",json!([{"op":"set","name":c["name"],"percent":n(&c["value"]).round().clamp(1.,100.)}]),format!("brightness:{}",s(&c["name"])));
            }
            "displayStart" => {
                self.state["display"] = c["id"].clone();
                self.drag = c.clone();
                self.drag["op"] = json!("displayDrag");
                self.drag["px"] = c["_event"]["x"].clone();
                self.drag["py"] = c["_event"]["y"].clone();
                self.drag["moved"] = json!(false);
            }
            _ => return false,
        }
        true
    }
    pub(super) fn display_drag(&mut self, v: &Value, finish: bool) {
        let dx = n(&v["x"]) - n(&self.drag["px"]);
        let dy = n(&v["y"]) - n(&self.drag["py"]);
        if !b(&self.drag["movable"]) || (!b(&self.drag["moved"]) && dx.hypot(dy) < 4.) {
            if finish && b(&self.drag["movable"]) {
                let id = self.drag["id"].clone();
                self.state["displayFlash"] = id.clone();
                self.state["displayFlashUntil"] = json!(self.now + 2000);
                self.call("display.identify", json!([[id]]), Value::Null);
            }
            return;
        }
        self.drag["moved"] = json!(true);
        let k = n(&self.drag["view"]["k"]).max(0.001);
        let x = n(&self.drag["x"]) + dx / k;
        let y = n(&self.drag["y"]) + dy / k;
        let outs: Vec<_> = arr(&self.d("display.list")["outputs"])
            .iter()
            .filter(|o| b(&o["active"]) && n(&o["w"]) > 0.)
            .cloned()
            .collect();
        let Some(index) = outs.iter().position(|o| o["id"] == self.drag["id"]) else {
            return;
        };
        if let Ok(point) = crate::winmodel::call(
            if finish {
                "display.drop"
            } else {
                "display.snap"
            },
            &[
                json!(outs),
                json!(index),
                json!(x),
                json!(y),
                json!(12. / k),
            ],
        ) {
            self.state["displayPreview"] =
                json!({"id":self.drag["id"],"x":point["x"],"y":point["y"]});
            if finish {
                self.call(
                    "display.arrange",
                    json!([self.drag["id"], point["x"], point["y"], true]),
                    json!({"op":"reportCall"}),
                );
                self.state["displayPreview"] = Value::Null;
            }
        }
    }
}

pub(super) fn night_active(minutes: f64, from: i64, to: i64) -> bool {
    let length = (to - from).rem_euclid(1440) as f64;
    let elapsed = (minutes - from as f64).rem_euclid(1440.);
    length > 0. && elapsed > 0. && elapsed < length
}
