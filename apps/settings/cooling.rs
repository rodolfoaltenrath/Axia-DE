use super::*;
const PRESET: [[i64; 2]; 3] = [[55, 30], [60, 55], [65, 100]];
impl App {
    fn custom_curve(&self) -> Value {
        if arr(&self.d("fan.status")["custom"]).is_empty() {
            json!(PRESET)
        } else {
            self.d("fan.status")["custom"].clone()
        }
    }
    pub(super) fn cooling(&self) -> Value {
        let u = self.ui();
        let st = self.d("fan.status");
        if !self.loaded {
            return u.note(t("cooling.loading"), "");
        }
        let fans = arr(&st["fans"]);
        let controllable = !st.is_null() && !fans.is_empty();
        let can_change = b(&st["canChange"]);
        let fan = fans.first().cloned().unwrap_or(Value::Null);
        let forced = st["force"] != "off" && !st.is_null();
        let variant = if fans.is_empty() {
            "none"
        } else if fans.iter().any(|f| f["kind"] == "board") {
            if self.d("fan.case") == "argon-one-v5" {
                "argon"
            } else {
                "cooler"
            }
        } else {
            "pc"
        };
        let status = if st.is_null() {
            t("cooling.serviceOff")
        } else if fans.is_empty() {
            t(if arr(&st["firmwareFans"]).is_empty() {
                "cooling.noFans"
            } else {
                "cooling.firmwareManaged"
            })
        } else if forced {
            t("cooling.forcedNow")
        } else if st["profile"] == "auto" {
            t("cooling.systemManaged")
        } else if n(&st["step"]) < 0. {
            tp("cooling.belowFirst", json!({"temp":st["curve"][0][0]}))
        } else {
            tp(
                "cooling.stepOf",
                json!({"n":n(&st["step"])+1.,"total":arr(&st["curve"]).len()}),
            )
        };
        let mut details = vec![
            u.text(t(&format!("cooling.variant.{variant}")), 11., true),
            u.text(
                if st["temp"].is_null() {
                    json!("—")
                } else {
                    tp("cooling.temp", json!({"v":n(&st["temp"]).round()}))
                },
                30.,
                false,
            ),
        ];
        if !fan.is_null() {
            details.push(u.text(
                tp("cooling.fanPct", json!({"v":n(&fan["pct"]).max(0.)})),
                17.,
                false,
            ));
            if n(&fan["rpm"]) >= 0. {
                details.push(u.text(tp("cooling.rpm", json!({"v":fan["rpm"]})), 12., true));
            }
        }
        details.push(u.text(status, 12., true));
        let mut out = vec![view(
            json!({"flexDirection":"row","alignItems":"center","gap":16,"padding":12,"borderRadius":12,"backgroundColor":if u.dark(){"rgba(255,255,255,.04)"}else{"rgba(0,0,0,.025)"},"border":format!("1px solid {}",s(&u.color("separator")))}),
            vec![
                self.cooling_art(variant, n(&fan["pct"]).max(0.), n(&st["temp"])),
                view(json!({"flex":1,"minWidth":0,"gap":8}), details),
            ],
        )];
        if st.is_null() {
            out.push(u.note(t("cooling.serviceOffHint"), "warn"));
        } else if !controllable {
            let firmware = arr(&st["firmwareFans"]);
            out.push(u.section(
                t("cooling.fans"),
                t(if firmware.is_empty() {
                    "cooling.noFansHint"
                } else {
                    "cooling.firmwareHint"
                }),
                if firmware.is_empty() {
                    vec![u.value(t("cooling.cooling"), t("cooling.passive"))]
                } else {
                    firmware
                        .iter()
                        .enumerate()
                        .map(|(i, f)| {
                            u.value(
                                tp("cooling.fanN", json!({"n":i+1})),
                                if n(&f["rpm"]) >= 0. {
                                    tp("cooling.rpm", json!({"v":f["rpm"]}))
                                } else {
                                    json!("—")
                                },
                            )
                        })
                        .collect()
                },
            ));
        }
        if controllable {
            let auto = st["profile"] == "auto";
            let mut rows = vec![u.disabled(
                u.row(
                    t("cooling.systemControl"),
                    t("cooling.systemControlHint"),
                    vec![u.toggle(
                        auto,
                        json!({"op":"fanProfile","value":if auto{"balanced"}else{"auto"}}),
                    )],
                ),
                !can_change,
            )];
            if !auto {
                rows.push(u.disabled(
                    u.row(
                        t("cooling.curveProfile"),
                        "",
                        vec![u.segmented(
                            st["profile"].clone(),
                            opts(
                                &["quiet", "balanced", "performance", "always", "custom"],
                                "cooling.profiles.",
                            ),
                            op("fanProfile"),
                        )],
                    ),
                    !can_change,
                ));
            }
            out.push(u.section(
                t("cooling.profile"),
                if can_change {
                    t(&format!("cooling.profileHint.{}", s(&st["profile"])))
                } else {
                    t("cooling.needAdmin")
                },
                rows,
            ));
            if !auto {
                let custom = st["profile"] == "custom";
                let curve = if custom {
                    self.custom_curve()
                } else {
                    st["curve"].clone()
                };
                let mut rows = vec![self.fan_curve(&curve, custom && can_change)];
                if custom && can_change {
                    rows.push(actions(vec![
                        u.button(
                            t("cooling.addPoint"),
                            op("fanAdd"),
                            arr(&curve).len() >= 8,
                            false,
                        ),
                        u.button(
                            t("cooling.removePoint"),
                            op("fanRemove"),
                            arr(&curve).len() <= 1,
                            false,
                        ),
                        u.button(t("cooling.resetCurve"), op("fanReset"), false, false),
                    ]));
                }
                out.push(u.section(
                    t("cooling.curve"),
                    tp(
                        if custom {
                            "cooling.curveHintCustom"
                        } else {
                            "cooling.curveHint"
                        },
                        json!({"hyst":st["hyst"]}),
                    ),
                    rows,
                ));
            }
            let value = if st["force"] == "off" {
                "off"
            } else if st["force"] == "on" {
                "on"
            } else {
                "timed"
            };
            let mut options = vec![
                json!(["off", t("cooling.forceOff")]),
                json!(["30", t("cooling.force30")]),
                json!(["60", t("cooling.force60")]),
                json!(["120", t("cooling.force120")]),
                json!(["on", t("cooling.forceOn")]),
            ];
            if value == "timed" {
                let minutes = (n(&st["forceLeft"]) / 60.).ceil() as i64;
                options.insert(
                    0,
                    json!([
                        "timed",
                        if minutes >= 60 {
                            tp("cooling.leftHM", json!({"h":minutes/60,"m":minutes%60}))
                        } else {
                            tp("cooling.leftM", json!({"m":minutes}))
                        }
                    ]),
                );
            }
            out.push(u.section(
                t("cooling.boost"),
                t("cooling.boostHint"),
                vec![u.disabled(
                    u.row(
                        t("cooling.force"),
                        "",
                        vec![u.pop(
                            "fan-force",
                            json!(value),
                            options,
                            op("fanForce"),
                            190.,
                            !can_change,
                        )],
                    ),
                    !can_change,
                )],
            ));
        }
        if st["board"] == "pi5" && !self.d("fan.case").is_null() {
            out.push(u.section(
                t("cooling.case"),
                t("cooling.caseHint"),
                vec![u.disabled(
                    u.row(
                        t("cooling.caseModel"),
                        "",
                        vec![u.pop(
                            "fan-case",
                            self.d("fan.case").clone(),
                            vec![
                                json!(["none", t("cooling.caseNone")]),
                                json!(["argon-one-v5", t("cooling.caseArgonV5")]),
                            ],
                            op("fanCase"),
                            210.,
                            !can_change,
                        )],
                    ),
                    !can_change,
                )],
            ));
        }
        if !st.is_null() {
            out.push(actions(vec![u.button(
                t("cooling.refresh"),
                op("fanRefresh"),
                false,
                false,
            )]));
        }
        stack(out)
    }
    fn fan_curve(&self, curve: &Value, editable: bool) -> Value {
        let u = self.ui();
        let width = (self.st("contentWidth").as_f64().unwrap_or(506.) - 24.).max(84.);
        let pw = width - 44.;
        let ph = 158.;
        let x = |temp: f64| 34. + (temp.clamp(30., 90.) - 30.) / 60. * pw;
        let y = |pct: f64| 10. + (1. - pct.clamp(0., 100.) / 100.) * ph;
        let curve = if self.drag["op"] == "curveDrag" {
            &self.drag["points"]
        } else {
            curve
        };
        let points = arr(curve);
        let mut kids = vec![];
        for v in [0., 25., 50., 75., 100.] {
            kids.push(view(json!({"position":"absolute","left":34,"width":pw,"top":y(v),"height":1,"backgroundColor":u.color("separator"),"opacity":if v==0.{1.}else{0.6}}),vec![]));
            let mut label = u.text(format!("{v}%"), 10., true);
            merge(
                &mut label["style"],
                &json!({"position":"absolute","left":0,"width":28,"top":y(v)-7.,"textAlign":"right"}),
            );
            kids.push(label);
        }
        for temp in (30..=90).step_by(10) {
            kids.push(view(json!({"position":"absolute","top":10,"height":ph,"left":x(temp as f64),"width":1,"backgroundColor":u.color("separator"),"opacity":0.45}),vec![]));
            let mut label = u.text(format!("{temp}°"), 10., true);
            merge(
                &mut label["style"],
                &json!({"position":"absolute","top":10.+ph+5.,"left":x(temp as f64)-16.,"width":32,"textAlign":"center"}),
            );
            kids.push(label);
        }
        let st = self.d("fan.status");
        let floor = if editable { 0. } else { n(&st["floor"]) };
        let mut segments = vec![];
        if let Some(first) = points.first() {
            segments.push((30., n(&first[0]), floor, -1));
        }
        for (i, p) in points.iter().enumerate() {
            segments.push((
                n(&p[0]),
                points.get(i + 1).map(|p| n(&p[0])).unwrap_or(90.),
                n(&p[1]),
                i as i64,
            ));
        }
        for (a, b, pct, index) in &segments {
            let on = st["force"] == "off" && st["step"] == *index;
            kids.push(view(json!({"position":"absolute","left":x(*a),"width":(x(*b)-x(*a)).max(0.),"top":y(*pct),"height":(y(0.)-y(*pct)).max(0.),"backgroundColor":u.color("accent"),"opacity":if on{0.22}else{0.1}}),vec![]));
            kids.push(view(json!({"position":"absolute","left":x(*a),"width":(x(*b)-x(*a)).max(2.),"top":y(*pct)-1.,"height":2.5,"borderRadius":1,"backgroundColor":u.color("accent")}),vec![]));
        }
        for pair in segments.windows(2) {
            kids.push(view(json!({"position":"absolute","left":x(pair[1].0)-1.,"width":2.5,"top":y(pair[0].2.max(pair[1].2)),"height":(y(pair[0].2)-y(pair[1].2)).abs()+1.,"backgroundColor":u.color("accent")}),vec![]));
        }
        for (i, p) in points.iter().enumerate() {
            kids.push(view(json!({"position":"absolute","left":x(n(&p[0])-n(&st["hyst"])),"width":x(n(&p[0]))-x(n(&p[0])-n(&st["hyst"])),"top":10,"height":ph,"backgroundColor":"#ff9f0a","opacity":if st["step"]==i{0.16}else{0.06}}),vec![]));
            let r = if editable { 7. } else { 5. };
            let mut handle = view(
                json!({"position":"absolute","left":x(n(&p[0]))-r,"top":y(n(&p[1]))-r,"width":r*2.,"height":r*2.,"borderRadius":r,"backgroundColor":if st["step"]==i{u.color("accent")}else{json!(if u.dark(){"#1c1d22"}else{"#fff"})},"border":format!("2px solid {}",s(&u.color("accent")))}),
                vec![],
            );
            if editable {
                handle["events"]["onMouseDown"] =
                    action(json!({"op":"curveStart","index":i,"points":curve,"pw":pw,"ph":ph}));
            }
            kids.push(handle);
            if self.drag["op"] == "curveDrag" && self.drag["index"] == i {
                let mut label = u.text(format!("{} °C · {} %", p[0], p[1]), 11., false);
                merge(
                    &mut label["style"],
                    &json!({"position":"absolute","left":x(n(&p[0]))-40.,"width":80,"top":y(n(&p[1]))-24.,"fontWeight":600,"textAlign":"center"}),
                );
                kids.push(label);
            }
        }
        if !st["temp"].is_null()
            && !st["fans"][0]["pct"].is_null()
            && n(&st["fans"][0]["pct"]) >= 0.
        {
            let temp = n(&st["temp"]);
            let pct = n(&st["fans"][0]["pct"]);
            for (radius, opacity) in [(11., 0.25), (5., 1.)] {
                kids.push(view(json!({"position":"absolute","left":x(temp)-radius,"top":y(pct)-radius,"width":radius*2.,"height":radius*2.,"borderRadius":radius,"backgroundColor":heat(temp,1.),"opacity":opacity,"border":if radius==5.{"2px solid #fff"}else{"0 solid transparent"}}),vec![]));
            }
            kids.push(view(json!({"position":"absolute","left":x(temp),"top":y(pct)+6.,"width":1,"height":(y(0.)-y(pct)-6.).max(0.),"backgroundColor":heat(temp,1.),"opacity":0.6}),vec![]));
        }
        view(json!({"height":190,"direction":"ltr"}), kids)
    }
    fn cooling_art(&self, variant: &str, pct: f64, temp: f64) -> Value {
        let u = self.ui();
        let dark = u.dark();
        let alu = if dark {
            "linear-gradient(170deg, #6b7079, #3b3f47)"
        } else {
            "linear-gradient(170deg, #e4e7ec, #aab0ba)"
        };
        let edge = if dark { "#2a2d33" } else { "#8d939d" };
        let pcb = if dark {
            "linear-gradient(160deg, #1f5a3d, #123826)"
        } else {
            "linear-gradient(160deg, #2f7a54, #1d5a3c)"
        };
        let slot = if dark { "#1b1d22" } else { "#3a3d44" };
        let blade = if dark { "#a9b0bb" } else { "#eef1f5" };
        let hub = if dark { "#d9dde3" } else { "#ffffff" };
        let fin = if dark { "#8b9099" } else { "#c3c8d0" };
        let angle = if b(self.pref("reduceMotion")) {
            0.
        } else {
            (self.now.rem_euclid(300000) as f64 / 1000. * pct.clamp(0., 100.) / 100. * 1.2 * 360.)
                % 360.
        };
        let fan = |size: f64, direction: f64| {
            let mut blades = vec![];
            for i in 0..5 {
                blades.push(view(json!({"position":"absolute","left":0,"top":0,"width":size,"height":size,"transform":format!("rotate({}deg)",i*72)}),vec![view(json!({"position":"absolute","left":size/2.-1.,"top":size/2.-(size*0.2).round(),"width":(size*0.42).round(),"height":(size*0.2).round(),"borderTopRightRadius":(size*0.2).round(),"borderBottomRightRadius":(size*0.2).round()/3.,"borderTopLeftRadius":2,"backgroundColor":blade}),vec![])]));
            }
            view(
                json!({"width":size,"height":size,"borderRadius":size/2.,"backgroundColor":slot,"alignItems":"center","justifyContent":"center"}),
                vec![
                    view(
                        json!({"position":"absolute","left":0,"top":0,"width":size,"height":size,"transform":format!("rotate({}deg)",angle*direction)}),
                        blades,
                    ),
                    view(
                        json!({"width":size*0.26,"height":size*0.26,"borderRadius":size*0.13,"backgroundColor":hub,"boxShadow":"0 1px 2px rgba(0,0,0,.35)"}),
                        vec![],
                    ),
                ],
            )
        };
        let fins = |count: usize, width: f64, length: f64, span: f64, vertical: bool| {
            view(json!({"flexDirection":if vertical{"row"}else{"column"},"gap":((span-count as f64*width)/(count as f64-1.).max(1.)).max(1.)}),(0..count).map(|_|view(json!({"width":if vertical{width}else{length},"height":if vertical{length}else{width},"borderRadius":1.5,"background":format!("linear-gradient({}deg, {fin}, {})",if vertical{180}else{90},heat(temp,if variant=="argon"{0.55}else if variant=="none"{0.45}else{0.6}))}),vec![])).collect())
        };
        let shadow = if dark {
            "rgba(0,0,0,0.5)"
        } else {
            "rgba(30,40,60,0.28)"
        };
        let port = if dark { "#a7acb5" } else { "#7d838d" };
        let frame = if dark { "#2c2f36" } else { "#5b616b" };
        let art = match variant {
            "argon" => {
                let (cw, ch) = (234., 98.);
                let mut ports = vec![];
                for (w, r) in [(18., 1.5), (18., 1.5), (7., 3.5), (7., 3.5)] {
                    ports.push(view(json!({"width":w,"height":7,"borderRadius":r,"backgroundColor":slot,"border":format!("1px solid {port}")}),vec![]));
                }
                ports.push(view(json!({"width":4,"height":4,"borderRadius":2,"marginLeft":6,"backgroundColor":if pct>0.{"#30d158"}else{port}}),vec![]));
                view(
                    json!({"position":"absolute","left":33,"top":26,"width":cw,"height":ch}),
                    vec![
                        view(
                            json!({"position":"absolute","left":6,"right":6,"top":ch-8.,"height":16,"borderRadius":10,"backgroundColor":shadow}),
                            vec![],
                        ),
                        view(
                            json!({"position":"absolute","left":0,"right":0,"top":0,"bottom":0,"borderRadius":14,"background":alu,"border":format!("1px solid {edge}"),"boxShadow":format!("0 10px 24px {shadow}"),"overflow":"hidden"}),
                            vec![
                                view(
                                    json!({"position":"absolute","left":18,"top":16,"width":cw*0.52,"height":ch*0.62,"justifyContent":"space-between"}),
                                    vec![fins(7, 3., cw * 0.52, ch * 0.62, false)],
                                ),
                                view(
                                    json!({"position":"absolute","right":22,"top":(ch-ch*0.62)/2.-4.,"width":ch*0.62,"height":ch*0.62,"borderRadius":ch*0.31,"backgroundColor":slot,"alignItems":"center","justifyContent":"center","border":format!("2px solid {edge}")}),
                                    vec![fan(ch * 0.54, 1.)],
                                ),
                                view(
                                    json!({"position":"absolute","left":18,"bottom":9,"flexDirection":"row","alignItems":"center","gap":7}),
                                    ports,
                                ),
                            ],
                        ),
                    ],
                )
            }
            "cooler" => {
                let (bw, bh, hs) = (198., 112., 74.);
                view(json!({"position":"absolute","left":51,"top":23,"width":bw,"height":bh}),vec![view(json!({"position":"absolute","left":0,"right":0,"top":0,"bottom":0,"borderRadius":10,"background":pcb,"boxShadow":format!("0 10px 22px {shadow}")}),vec![
                    view(json!({"position":"absolute","left":14,"top":7,"flexDirection":"row","gap":3}),(0..16).map(|_|view(json!({"width":4,"height":9,"borderRadius":1,"backgroundColor":"#d6c27a","opacity":0.85}),vec![])).collect()),
                    view(json!({"position":"absolute","right":-6,"top":18,"gap":6}),(0..3).map(|i|view(json!({"width":20,"height":if i==2{22}else{16},"borderRadius":2,"background":"linear-gradient(90deg, #c9cdd3, #8e939b)"}),vec![])).collect()),
                    view(json!({"position":"absolute","left":8,"top":bh-16.,"width":8,"height":8,"borderRadius":4,"backgroundColor":"rgba(0,0,0,0.35)"}),vec![]),
                    view(json!({"position":"absolute","left":bw-30.,"top":bh-16.,"width":8,"height":8,"borderRadius":4,"backgroundColor":"rgba(0,0,0,0.35)"}),vec![]),
                    view(json!({"position":"absolute","left":40,"top":23,"width":93,"height":hs,"borderRadius":8,"background":alu,"border":format!("1px solid {edge}"),"boxShadow":format!("0 6px 14px {shadow}"),"alignItems":"center","justifyContent":"center"}),vec![
                        view(json!({"position":"absolute","left":6,"right":6,"top":6,"bottom":6,"flexDirection":"row","justifyContent":"space-between"}),vec![fins(9,3.,hs-12.,hs*1.25-12.,true)]),
                        view(json!({"width":hs*0.8,"height":hs*0.8,"borderRadius":hs*0.4,"backgroundColor":frame,"alignItems":"center","justifyContent":"center","border":format!("2px solid {edge}")}),vec![fan(hs*0.72,1.)])
                    ])
                ])])
            }
            "pc" => {
                let (tw, th) = (177., 138.);
                let case_fan = |bottom: bool, k| {
                    let mut style = json!({"position":"absolute","right":12,"width":tw*0.26,"height":tw*0.26,"borderRadius":6,"backgroundColor":frame,"alignItems":"center","justifyContent":"center"});
                    style[if bottom { "bottom" } else { "top" }] = json!(12);
                    view(style, vec![fan(tw * 0.24, k)])
                };
                view(
                    json!({"position":"absolute","left":(300.-tw)/2.,"top":13,"width":tw,"height":th}),
                    vec![view(
                        json!({"position":"absolute","left":0,"right":0,"top":0,"bottom":0,"borderRadius":12,"background":if dark{"linear-gradient(170deg, #34373e, #1f2126)"}else{"linear-gradient(170deg, #50545c, #33363c)"},"border":format!("1px solid {edge}"),"boxShadow":format!("0 10px 24px {shadow}")}),
                        vec![view(
                            json!({"position":"absolute","left":10,"right":10,"top":10,"bottom":10,"borderRadius":8,"backgroundColor":if dark{"rgba(120,140,170,0.10)"}else{"rgba(150,180,220,0.18)"},"border":"1px solid rgba(255,255,255,0.12)"}),
                            vec![
                                view(
                                    json!({"position":"absolute","left":12,"top":12,"width":tw*0.5,"height":th*0.66,"borderRadius":4,"background":"linear-gradient(160deg, #2b3340, #1b2029)"}),
                                    vec![
                                        view(
                                            json!({"position":"absolute","left":tw*0.06,"top":th*0.08,"width":tw*0.26,"height":tw*0.26,"borderRadius":4,"background":alu,"alignItems":"center","justifyContent":"center"}),
                                            vec![
                                                view(
                                                    json!({"position":"absolute","left":4,"right":4,"top":4,"bottom":4,"flexDirection":"row","justifyContent":"space-between"}),
                                                    vec![fins(
                                                        7,
                                                        2.,
                                                        tw * 0.26 - 8.,
                                                        tw * 0.26 - 8.,
                                                        true,
                                                    )],
                                                ),
                                                fan(tw * 0.21, 1.),
                                            ],
                                        ),
                                        view(
                                            json!({"position":"absolute","left":tw*0.35,"top":th*0.08,"width":5,"height":th*0.3,"borderRadius":1,"backgroundColor":"#5e6878"}),
                                            vec![],
                                        ),
                                        view(
                                            json!({"position":"absolute","left":tw*0.38,"top":th*0.08,"width":5,"height":th*0.3,"borderRadius":1,"backgroundColor":"#5e6878"}),
                                            vec![],
                                        ),
                                        view(
                                            json!({"position":"absolute","left":tw*0.06,"top":th*0.5,"width":tw*0.36,"height":9,"borderRadius":2,"backgroundColor":"#3c4554"}),
                                            vec![],
                                        ),
                                    ],
                                ),
                                case_fan(false, -1.),
                                case_fan(true, 0.9),
                            ],
                        )],
                    )],
                )
            }
            _ => {
                let (cw, x, y) = (102., 99., 49.);
                let mut parts:Vec<_>=(0..3).map(|i|view(json!({"position":"absolute","left":x-30.+i as f64*18.,"top":y-34.-i as f64*6.,"width":cw*0.7,"height":3,"borderRadius":2,"backgroundColor":fin,"opacity":0.35-i as f64*0.08}),vec![])).collect();
                parts.push(view(json!({"position":"absolute","left":x,"top":y,"width":cw,"height":164.*0.42,"borderRadius":8,"background":alu,"border":format!("1px solid {edge}"),"boxShadow":format!("0 10px 22px {shadow}"),"paddingHorizontal":8,"paddingVertical":8,"flexDirection":"row","justifyContent":"space-between"}),vec![fins(8,3.,164.*0.42-16.,cw-16.,true)]));
                parts.push(view(json!({"position":"absolute","left":x-12.,"top":y+164.*0.42-2.,"width":cw+24.,"height":10,"borderRadius":3,"background":pcb}),vec![]));
                view(
                    json!({"position":"absolute","left":0,"top":0,"width":300,"height":164}),
                    parts,
                )
            }
        };
        view(
            json!({"width":300,"height":164,"borderRadius":14,"background":if dark{"linear-gradient(160deg, #23252c, #17181d)"}else{"linear-gradient(160deg, #eef1f6, #dde2ea)"},"overflow":"hidden","direction":"ltr"}),
            vec![
                view(
                    json!({"position":"absolute","left":45,"right":45,"top":16.4,"bottom":16.4,"borderRadius":164,"backgroundColor":heat(temp,if dark{0.08}else{0.1})}),
                    vec![],
                ),
                art,
            ],
        )
    }
    pub(super) fn cooling_command(&mut self, c: &Value) -> bool {
        let can_change = b(&self.d("fan.status")["canChange"]);
        match s(&c["op"]) {
            "fanRefresh" => self.read_at = 0,
            "fanProfile" => {
                if can_change {
                    self.call(
                        "fan.profile",
                        json!([c["value"]]),
                        json!({"op":"fanResult"}),
                    );
                }
            }
            "fanForce" => {
                if can_change && c["value"] != "timed" {
                    let value = if matches!(s(&c["value"]), "off" | "on") {
                        c["value"].clone()
                    } else {
                        json!(s(&c["value"]).parse::<u32>().unwrap_or(0))
                    };
                    self.call("fan.force", json!([value]), json!({"op":"fanResult"}));
                }
            }
            "fanAdd" | "fanRemove" | "fanReset" => {
                if !can_change || self.d("fan.status")["profile"] != "custom" {
                    return true;
                }
                let mut points = arr(&self.custom_curve()).to_vec();
                match s(&c["op"]) {
                    "fanAdd" => {
                        if points.len() < 8 {
                            if let Some(last) = points.last() {
                                if n(&last[0]) < 90. {
                                    points.push(json!([
                                        (n(&last[0]) + 5.).min(90.),
                                        (n(&last[1]) + 10.).min(100.)
                                    ]));
                                }
                            }
                        }
                    }
                    "fanRemove" => {
                        if points.len() > 1 {
                            points.pop();
                        }
                    }
                    _ => points = arr(&json!(PRESET)).to_vec(),
                }
                self.call("fan.curve", json!([points]), json!({"op":"fanResult"}));
            }
            "fanCase" => {
                if can_change {
                    self.admin_run(
                        "rpi-case",
                        s(&c["value"]).into(),
                        false,
                        json!({"success":t("cooling.caseRestart")}),
                    );
                }
            }
            "curveStart" => {
                if can_change && self.d("fan.status")["profile"] == "custom" {
                    self.drag = c.clone();
                    self.drag["op"] = json!("curveDrag");
                    self.drag["start"] = c["points"].clone();
                    self.drag["px"] = c["_event"]["x"].clone();
                    self.drag["py"] = c["_event"]["y"].clone();
                }
            }
            _ => return false,
        }
        true
    }
    pub(super) fn curve_drag(&mut self, v: &Value, finish: bool) {
        let mut points = arr(&self.drag["start"]).to_vec();
        let index = n(&self.drag["index"]) as usize;
        if index >= points.len() {
            return;
        }
        let lo = if index > 0 {
            n(&points[index - 1][0]) + 1.
        } else {
            30.
        };
        let hi = points.get(index + 1).map(|p| n(&p[0]) - 1.).unwrap_or(90.);
        let plo = if index > 0 {
            n(&points[index - 1][1])
        } else {
            0.
        };
        let phi = points.get(index + 1).map(|p| n(&p[1])).unwrap_or(100.);
        points[index][0] = json!((n(&points[index][0])
            + (n(&v["x"]) - n(&self.drag["px"])) * 60. / n(&self.drag["pw"]).max(1.))
        .round()
        .clamp(lo, hi));
        points[index][1] = json!(((n(&points[index][1])
            - (n(&v["y"]) - n(&self.drag["py"])) * 100. / n(&self.drag["ph"]).max(1.))
            / 5.)
            .round()
            .mul_add(5., 0.)
            .clamp(plo, phi));
        self.drag["points"] = json!(points);
        if finish
            && self.drag["points"] != self.drag["start"]
            && b(&self.d("fan.status")["canChange"])
            && self.d("fan.status")["profile"] == "custom"
        {
            self.call("fan.curve", json!([points]), json!({"op":"fanResult"}));
        }
    }
}
fn heat(temp: f64, alpha: f64) -> String {
    let (a, b, k) = if temp <= 40. {
        ([90., 160., 255.], [90., 160., 255.], 0.)
    } else if temp <= 65. {
        ([90., 160., 255.], [255., 180., 64.], (temp - 40.) / 25.)
    } else {
        (
            [255., 180., 64.],
            [255., 90., 54.],
            ((temp - 65.) / 15.).min(1.),
        )
    };
    format!(
        "rgba({},{},{},{alpha})",
        (a[0] + (b[0] - a[0]) * k).round(),
        (a[1] + (b[1] - a[1]) * k).round(),
        (a[2] + (b[2] - a[2]) * k).round()
    )
}
