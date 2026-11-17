use super::*;
fn printer_glyph(size: f64, color: &str) -> Value {
    view(
        json!({"width":size,"height":size,"alignItems":"center","justifyContent":"center"}),
        vec![
            view(
                json!({"position":"absolute","top":size*0.08,"width":size*0.5,"height":size*0.3,"backgroundColor":"#ffffff","border":"1px solid rgba(0,0,0,.25)"}),
                vec![],
            ),
            view(
                json!({"position":"absolute","top":size*0.32,"width":size*0.86,"height":size*0.38,"borderRadius":size*0.08,"backgroundColor":color}),
                vec![],
            ),
            view(
                json!({"position":"absolute","top":size*0.6,"width":size*0.56,"height":size*0.3,"backgroundColor":"#ffffff","border":"1px solid rgba(0,0,0,.25)"}),
                vec![],
            ),
        ],
    )
}
pub(super) fn queue_name(name: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || "_-".contains(c) {
            if separator && !result.is_empty() {
                result.push('_');
            }
            separator = false;
            result.push(c);
        } else {
            separator = true;
        }
        if result.len() >= 60 {
            break;
        }
    }
    let result = result.trim_matches('_');
    if result.is_empty() {
        "Impresora".into()
    } else {
        result.into()
    }
}
impl App {
    pub(super) fn printers(&self) -> Value {
        let u = self.ui();
        if self.loaded && !b(self.d("print.hasCups")) {
            return stack(vec![u.header(
                printer_glyph(44., "#8e8e93"),
                t("settings.section.printers"),
                t("print.noCupsLong"),
                vec![],
            )]);
        }
        let data = self.d("printers");
        let printers = arr(&data["printers"]);
        let sel = s(self.st("printer"));
        let cur = printers.iter().find(|p| p["name"] == sel);
        let mut rows = vec![];
        for p in printers {
            let state = if p["state"] == 5 {
                "stopped"
            } else if p["state"] == 4 {
                "printing"
            } else if !b(&p["accepting"]) {
                "rejecting"
            } else {
                "idle"
            };
            let hint = [
                self.tr(&format!("print.state.{state}")),
                s(&p["model"]).into(),
                s(&p["location"]).into(),
                s(&p["message"]).into(),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<String>>()
            .join(" · ");
            let mut controls = vec![];
            if data["default"] == p["name"] {
                let mut label = u.text(t("print.defaultBadge"), 10., false);
                label["style"]["fontWeight"] = json!(600);
                label["style"]["color"] = json!("#fff");
                controls.push(view(json!({"paddingHorizontal":7,"height":18,"borderRadius":9,"justifyContent":"center","backgroundColor":u.color("accent")}),vec![label]));
            }
            if n(&p["queued"]) > 0. {
                controls.push(u.text(tp("print.queued", json!({"count":p["queued"]})), 11., true));
            }
            let mut check = u.text(if p["name"] == sel { "✓" } else { "" }, 14., false);
            merge(
                &mut check["style"],
                &json!({"width":16,"fontWeight":600,"color":u.color("accent"),"textAlign":"center"}),
            );
            controls.push(check);
            let mut row = click(
                u.row_icon(
                    u.row(
                        p["info"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .unwrap_or(s(&p["name"])),
                        hint,
                        controls,
                    ),
                    printer_glyph(
                        30.,
                        if p["state"] == 5 {
                            "#ff453a"
                        } else if p["state"] == 4 {
                            "#30d158"
                        } else {
                            "#8e8e93"
                        },
                    ),
                ),
                ui::state("printer", p["name"].clone()),
            );
            row["style"]["minHeight"] = json!(48);
            row["children"][1]["children"][0]["style"]["fontWeight"] =
                json!(if p["name"] == sel { 600 } else { 400 });
            rows.push(row);
        }
        if rows.is_empty() {
            rows.push(u.row(
                t(if data.is_null() {
                    "print.loading"
                } else {
                    "print.noPrinters"
                }),
                "",
                vec![],
            ));
        }
        let mut out = vec![u.section("", "", rows)];
        if let Some(p) = cur {
            let name = p["info"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(s(&p["name"]));
            out.push(actions(vec![
                u.button(
                    t("print.setDefault"),
                    json!({"op":"printerDefault","name":p["name"],"label":name}),
                    false,
                    false,
                ),
                u.button(
                    t("print.testPage"),
                    json!({"op":"printerTest","printer":p}),
                    false,
                    false,
                ),
                u.button(
                    t("print.remove"),
                    json!({"op":"printerRemove","name":p["name"],"label":name}),
                    false,
                    false,
                ),
            ]));
            let jobs: Vec<_> = arr(&self.d("printJobs")["jobs"])
                .iter()
                .filter(|j| sel.is_empty() || j["printer"] == sel)
                .collect();
            out.push(u.section(
                tp("print.queueOf", json!({"name":name})),
                "",
                if jobs.is_empty() {
                    vec![u.row(t("print.queueEmpty"), "", vec![])]
                } else {
                    jobs.into_iter()
                        .map(|j| {
                            let state = match j["state"].as_i64().unwrap_or(3) {
                                4 => "held",
                                5 => "processing",
                                6 => "stopped",
                                7 => "canceled",
                                8 => "aborted",
                                9 => "completed",
                                _ => "pending",
                            };
                            u.row(
                                j["name"]
                                    .as_str()
                                    .filter(|s| !s.is_empty())
                                    .map(|s| json!(s))
                                    .unwrap_or_else(|| t("print.untitled")),
                                format!(
                                    "{} · {}{}",
                                    s(&j["user"]),
                                    self.tr(&format!("print.job.{state}")),
                                    if n(&j["kb"]) > 0. {
                                        format!(" · {} KB", j["kb"])
                                    } else {
                                        String::new()
                                    }
                                ),
                                vec![u.button(
                                    t("print.cancelJob"),
                                    json!({"op":"printerCancel","id":j["id"]}),
                                    false,
                                    false,
                                )],
                            )
                        })
                        .collect()
                },
            ));
        }
        let found = self.d("found");
        let discover_actions = actions(vec![
            u.button(
                t(if b(self.st("searchingPrinters")) {
                    "print.searching"
                } else {
                    "print.searchAgain"
                }),
                op("printerDiscover"),
                false,
                false,
            ),
            u.button(t("print.manual"), op("printerManual"), false, false),
        ]);
        let mut discovered = vec![];
        for f in arr(&found["printers"]) {
            let known = printers.iter().any(|p| p["uri"] == f["uri"]);
            discovered.push(
                u.row_icon(
                    u.row(
                        f["name"].clone(),
                        [
                            s(&f["model"]).to_owned(),
                            s(&f["uri"]).to_owned(),
                            if f["color"] == "T" {
                                self.tr("print.color")
                            } else {
                                String::new()
                            },
                            if f["duplex"] == "T" {
                                self.tr("print.duplex")
                            } else {
                                String::new()
                            },
                        ]
                        .into_iter()
                        .filter(|x| !x.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                        if known {
                            vec![u.text(t("print.alreadyAdded"), 11., true)]
                        } else {
                            vec![u.button(
                                t("print.add"),
                                json!({"op":"printerAddFound","printer":f}),
                                false,
                                false,
                            )]
                        },
                    ),
                    printer_glyph(26., "#8e8e93"),
                ),
            );
        }
        if arr(&found["printers"]).is_empty() {
            discovered.push(u.row(
                t(if found.is_null() {
                    "print.searching"
                } else {
                    "print.noneFound"
                }),
                "",
                vec![],
            ));
        }
        out.push(u.accessory(
            u.section(t("print.addTitle"), t("print.addHint"), discovered),
            discover_actions,
        ));
        if self.st("manualPrinter").is_object() {
            out.push(
                u.section(
                    "",
                    "",
                    [
                        ("uri", "URI"),
                        ("info", "print.fieldName"),
                        ("location", "print.fieldLocation"),
                    ]
                    .iter()
                    .map(|(key, label)| {
                        u.field(
                            if *label == "URI" {
                                json!("URI")
                            } else {
                                t(label)
                            },
                            "",
                            &format!("manualPrinter.{key}"),
                            self.st("manualPrinter")[key].clone(),
                            false,
                        )
                    })
                    .collect(),
                ),
            );
            out.push(actions(vec![
                u.regular_button(
                    t("print.cancel"),
                    state("manualPrinter", Value::Null),
                    false,
                    false,
                ),
                u.button(t("print.add"), op("printerAddManual"), false, true),
            ]));
        }
        let scanners = arr(&found["scanners"]);
        out.push(u.section(
            t("print.scanners"),
            "",
            if scanners.is_empty() {
                vec![u.row(
                    t(if found.is_null() {
                        "print.searching"
                    } else {
                        "print.noScanners"
                    }),
                    "",
                    vec![],
                )]
            } else {
                scanners
                    .iter()
                    .map(|s| {
                        u.row(
                            s["name"].clone(),
                            format!(
                                "{} · {}:{}{}",
                                s["model"].as_str().unwrap_or(""),
                                s["addr"].as_str().unwrap_or(""),
                                s["port"],
                                if b(&s["tls"]) {
                                    format!(" · {}", self.tr("print.tlsUnsupported"))
                                } else {
                                    String::new()
                                }
                            ),
                            if !b(&s["tls"]) && b(&s["lan"]) {
                                vec![u.button(
                                    t("print.scan"),
                                    json!({"op":"open","app":"scanner","args":{"scanner":s}}),
                                    false,
                                    false,
                                )]
                            } else {
                                vec![]
                            },
                        )
                    })
                    .collect()
            },
        ));
        stack(out)
    }
    pub(super) fn discover_printers(&mut self) {
        self.state["searchingPrinters"] = json!(true);
        self.call(
            "print.discover",
            json!([]),
            json!({"op":"data","key":"found"}),
        );
    }
    pub(super) fn printer_command(&mut self, c: &Value) -> bool {
        let mut after = json!({"op":"printerChanged"});
        match s(&c["op"]){"printerDiscover"=>self.discover_printers(),"printerManual"=>self.state["manualPrinter"]=if self.st("manualPrinter").is_object(){Value::Null}else{json!({"uri":"ipp://","info":"","location":""})},"printerDefault"=>{after["success"]=tp("print.defaultSet",json!({"name":c["label"]}));self.call("print.default",json!([c["name"]]),after);},"printerRemove"=>{after["success"]=tp("print.removed",json!({"name":c["label"]}));self.call("print.remove",json!([c["name"]]),after);},"printerCancel"=>{after["success"]=tp("print.jobCanceled",json!({"id":c["id"]}));self.call("print.cancel",json!([c["id"]]),after);},"printerTest"=>self.call("print.open",json!([{"title":self.tr("print.testTitle"),"text":self.caption(&tp("print.testText",json!({"name":c["printer"]["info"],"model":c["printer"]["model"]})))}]),Value::Null),"printerAddFound"=>{let f=&c["printer"];after["success"]=tp("print.added",json!({"name":f["name"]}));self.call("print.add",json!([queue_name(s(&f["name"])),f["uri"],f["name"],f["note"].as_str().unwrap_or("")]),after);},"printerAddManual"=>{let f=self.st("manualPrinter").clone();if !f.is_object(){return true;}let info=s(&f["info"]).trim();let info=if info.is_empty(){s(&f["uri"]).split_once("://").map(|(_,s)|s).unwrap_or(s(&f["uri"]))}else{info};after["success"]=tp("print.added",json!({"name":info}));self.call("print.add",json!([queue_name(info),s(&f["uri"]).trim(),info,s(&f["location"]).trim()]),after);self.state["manualPrinter"]=Value::Null;},_=>return false}
        true
    }
    pub(super) fn printer_after(&mut self, after: &Value, _value: &Value) {
        if after["op"] == "printerChanged" {
            self.state["printAt"] = json!(0);
        }
    }
}
