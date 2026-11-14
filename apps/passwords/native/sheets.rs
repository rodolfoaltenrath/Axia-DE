//! The sheets over the window (apps/passwords/Generator.jsx and Settings.jsx): the generator, the
//! vault settings, import, export, a category, a tag and an attachment's preview.
use super::glyphs::{glyph, icon_kind, GLYPH_KINDS};
use super::ui::*;
use super::{App, Sheet};
use serde_json::{json, Value};

fn opt(t: &Theme, caption: Value, control: Value) -> Value {
    view(json!({"flexDirection":"row","alignItems":"center","gap":12,"minHeight":32}), vec![
        text(caption, json!({"flex":1,"minWidth":0,"fontSize":13,"color":t.text()})),
        control,
    ])
}

pub fn gen_defaults() -> Value {
    json!({"mode":"password","length":20,"upper":true,"lower":true,"digits":true,"symbols":true,"noAmbiguous":false,
        "words":5,"sep":"-","capitalize":true,"addDigit":true})
}

fn generator(app: &App, t: &Theme, o: &Value, res: &Option<Value>, use_for: bool) -> Vec<Value> {
    let _ = app;
    let b = |key: &str| o[key] == true;
    let mut kids = vec![
        segmented(t, "genMode", o["mode"].as_str().unwrap_or("password"), &[("password", k("gen.modePassword")), ("words", k("gen.modeWords"))], true, true, false),
        view(json!({"gap":8}), vec![
            view(json!({"paddingHorizontal":14,"paddingVertical":10,"borderRadius":10,"minHeight":52,"justifyContent":"center","direction":"ltr",
                "border":format!("1px solid {}", t.s(t.sep())),"backgroundColor":if t.dark() {"rgba(255,255,255,0.05)"} else {"rgba(0,0,0,0.03)"}}), vec![
                text(json!(res.as_ref().and_then(|r| r["password"].as_str()).unwrap_or("")), json!({"fontSize":16,"fontFamily":"monospace","color":t.text(),"lineHeight":22,"textAlign":"center"})),
            ]),
            match res {
                Some(r) => strength_bar(t, r["score"].as_i64().unwrap_or(0), &r["bits"]),
                None => view(json!({"height":21}), vec![]),
            },
        ]),
    ];
    if o["mode"] != "words" {
        let len = o["length"].as_i64().unwrap_or(20);
        kids.push(group(t, vec![
            opt(t, kp("gen.length", json!({"n":len})), slider(t, "genLen", len as f64, 8., 64., 150.)),
            opt(t, k("gen.upper"), toggle(t, b("upper"), true, json!(["genOpt", "upper"]))),
            opt(t, k("gen.lower"), toggle(t, b("lower"), true, json!(["genOpt", "lower"]))),
            opt(t, k("gen.digits"), toggle(t, b("digits"), true, json!(["genOpt", "digits"]))),
            opt(t, k("gen.symbols"), toggle(t, b("symbols"), true, json!(["genOpt", "symbols"]))),
            opt(t, k("gen.noAmbiguous"), toggle(t, b("noAmbiguous"), true, json!(["genOpt", "noAmbiguous"]))),
        ], json!({})));
    } else {
        kids.push(view(json!({"gap":6}), vec![
            group(t, vec![
                opt(t, k("gen.words"), stepper(t, "genWords", o["words"].as_i64().unwrap_or(5), 3, 12)),
                opt(t, k("gen.sep"), input("genSep", o["sep"].as_str().unwrap_or("-"), json!(""), false, false, None, json!({"width":64,"height":24,"flex":0}))),
                opt(t, k("gen.capitalize"), toggle(t, b("capitalize"), true, json!(["genOpt", "capitalize"]))),
                opt(t, k("gen.addDigit"), toggle(t, b("addDigit"), true, json!(["genOpt", "addDigit"]))),
            ], json!({})),
            view(json!({"paddingHorizontal":8}), vec![hint(t, k("gen.wordsHint"), None)]),
        ]));
    }
    let mut actions = vec![
        button(t, k("gen.again"), json!(["genAgain"]), Btn { icon: Some("↻"), ..Btn::default() }),
        button(t, k("gen.copy"), json!(["genCopy"]), Btn { primary: !use_for, ..Btn::default() }),
    ];
    if use_for {
        actions.push(button(t, k("gen.use"), json!(["genUse"]), Btn { primary: true, ..Btn::default() }));
    }
    kids.push(view(json!({"flexDirection":"row","gap":8,"justifyContent":"flex-end"}), actions));
    vec![view(json!({"gap":12}), kids)]
}

const SLOT_ICON: [(&str, &str); 6] = [("password", "lock"), ("keyfile", "drive"), ("passkey", "drive"), ("fido", "key"), ("recovery", "note"), ("session", "person")];

fn row(t: &Theme, caption: Value, hint_caption: Value, control: Value) -> Value {
    let mut lines = vec![text(caption, json!({"fontSize":13,"color":t.text()}))];
    if !hint_caption.is_null() {
        lines.push(text(hint_caption, json!({"fontSize":11,"color":t.dim(),"lineHeight":15})));
    }
    view(json!({"flexDirection":"row","alignItems":"center","gap":12,"paddingVertical":8,"minHeight":36}), vec![
        view(json!({"flex":1,"minWidth":0,"gap":2}), lines),
        control,
    ])
}

pub const AUTOLOCK: [i64; 5] = [60, 300, 900, 3600, 0];
pub fn autolock_caption(n: i64) -> Value {
    match n {
        0 => k("set.never"),
        3600 => k("set.hour"),
        n => kp("set.min", json!({"n":n / 60})),
    }
}
pub const CLIP: [i64; 4] = [15, 30, 60, 120];

fn settings(app: &App, t: &Theme, s: &super::SetState) -> Vec<Value> {
    let mut kids = vec![segmented(t, "setTab", &s.tab, &[("unlock", k("set.tabUnlock")), ("prefs", k("set.tabPrefs"))], true, true, false)];
    if s.tab == "unlock" {
        let slots = app.status["main"]["slots"].as_array().cloned().unwrap_or_default();
        let rows = slots
            .iter()
            .map(|sl| {
                let ty = sl["type"].as_str().unwrap_or("");
                let icon = SLOT_ICON.iter().find(|(k, _)| *k == ty).map_or("key", |(_, g)| g);
                let mut name = vec![k(&format!("method.{ty}"))];
                if let Some(l) = sl["label"].as_str().filter(|l| !l.is_empty()) {
                    name.push(json!(format!(" · {l}")));
                }
                let mut sub = vec![];
                if let Some(m) = sl["m"].as_f64().filter(|m| *m > 0.) {
                    sub.push(kp("set.kdf", json!({"m":(m / 1024.).round(),"t":sl["t"]})));
                }
                if let Some(c) = sl["created"].as_f64().filter(|c| *c > 0.) {
                    sub.push(json!(format!(" {}", app.date(c))));
                }
                let action = if ty == "recovery" {
                    button(t, k("set.newRecovery"), json!(["recNew"]), Btn { small: true, ..Btn::default() })
                } else {
                    button(t, k("set.remove"), json!(["slotDel", sl["id"]]), Btn { small: true, destructive: true, ..Btn::default() })
                };
                view(json!({"flexDirection":"row","alignItems":"center","gap":10,"minHeight":48}), vec![
                    glyph(icon, 16., &t.s(t.accent())),
                    view(json!({"flex":1,"minWidth":0}), vec![
                        text(json!({"parts":name}), json!({"fontSize":13,"color":t.text(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
                        text(json!({"parts":sub}), json!({"fontSize":11,"color":t.dim(),"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
                    ]),
                    action,
                ])
            })
            .collect();
        let mut u = vec![hint(t, k("set.unlockHint"), None), group(t, rows, json!({}))];
        let has_pw = slots.iter().any(|sl| sl["type"] == "password");
        match s.add.as_deref() {
            None => {
                let mut b = vec![];
                let small = Btn { small: true, ..Btn::default() };
                if has_pw {
                    b.push(button(t, k("set.change"), json!(["setAdd", "change"]), small));
                } else {
                    b.push(button(t, k("set.addPassword"), json!(["setAdd", "password"]), small));
                }
                b.push(button(t, k("set.addKeyfile"), json!(["setAdd", "keyfile"]), small));
                b.push(button(t, k("set.addPasskey"), json!(["setAdd", "passkey"]), small));
                b.push(button(t, k("set.addFido"), json!(["setAdd", "fido"]), small));
                u.push(view(json!({"flexDirection":"row","flexWrap":"wrap","gap":8}), b));
                if !s.err.is_null() {
                    u.push(text(s.err.clone(), json!({"fontSize":12,"color":RED})));
                }
            }
            Some(add) => {
                let mut f = vec![
                    text(k(&format!("set.adding.{add}")), json!({"fontSize":13,"fontWeight":600,"color":t.text()})),
                    hint(t, k(&format!("create.about.{}", if add == "change" { "password" } else { add })), None),
                ];
                if add == "change" {
                    f.push(input("setOld", &s.old, k("set.oldPassword"), true, false, None, json!({})));
                }
                if ["password", "passkey", "change"].contains(&add) {
                    f.push(input("setPw", &s.pw, k("create.password"), true, false, None, json!({})));
                    f.push(input("setPw2", &s.pw2, k("create.repeat"), true, false, None, json!({})));
                }
                if add == "keyfile" || add == "passkey" {
                    let has = s.path.is_some();
                    f.push(view(json!({"flexDirection":"row","gap":8,"alignItems":"center"}), vec![
                        glyph("drive", 14., &t.s(if has { t.accent() } else { t.dim() })),
                        text(s.path.as_ref().map_or(k("create.keyWhere"), |p| json!(p.name)),
                            json!({"flex":1,"minWidth":0,"fontSize":12,"color":if has { t.text() } else { t.dim() },"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
                        button(t, k("create.chooseKeyPath"), json!(["setPick"]), Btn { small: true, ..Btn::default() }),
                    ]));
                }
                if add == "passkey" {
                    f.push(warning(t, k("set.passkeyWarn"), false));
                }
                if add == "fido" {
                    f.push(hint(t, k(if app.touch { "fido.touch" } else { "fido.insertCreate" }), None));
                    f.push(input("setPin", &s.pin, k("fido.pinOptional"), true, false, None, json!({})));
                }
                if !s.err.is_null() {
                    f.push(text(s.err.clone(), json!({"fontSize":12,"color":RED})));
                }
                let label = if s.busy { k(if app.touch { "fido.touchShort" } else { "saving" }) } else { k("save") };
                f.push(view(json!({"flexDirection":"row","gap":8,"justifyContent":"flex-end"}), vec![
                    button(t, k("cancel"), json!(["setReset"]), Btn::default()),
                    button(t, label, json!(["setSubmit"]), Btn { primary: true, ..Btn::default() }),
                ]));
                u.push(view(json!({"gap":10,"padding":14,"borderRadius":10,"border":format!("1px solid {}", t.s(t.sep()))}), f));
            }
        }
        kids.push(view(json!({"gap":10}), u));
    } else {
        let autolock = app.meta["autolock"].as_i64().unwrap_or(300);
        let clip = app.meta["clip"].as_i64().unwrap_or(30);
        kids.push(view(json!({"gap":12}), vec![
            group(t, vec![
                row(t, k("set.autolock"), k("set.autolockHint"), choice(t, "autolock", autolock_caption(autolock), app.choice.as_deref() == Some("autolock"), 190.)),
                row(t, k("set.clip"), k("set.clipHint"), choice(t, "clip", kp("set.sec", json!({"n":clip})), app.choice.as_deref() == Some("clip"), 190.)),
                row(t, k("set.calibrate"), k("set.calibrateHint"),
                    button(t, k(if s.busy { "saving" } else { "set.measure" }), json!(["calibrate"]), Btn { small: true, ..Btn::default() })),
            ], json!({})),
            hint(t, kp("set.memory", json!({"kb":(app.status["mem"]["inUse"].as_f64().unwrap_or(0.) / 1024.).round(),
                "locked":(app.status["mem"]["locked"].as_f64().unwrap_or(0.) / 1024.).round()})), None),
        ]));
    }
    kids
}

fn import(app: &App, t: &Theme, s: &super::ImpState) -> Vec<Value> {
    let _ = app;
    let name = s.file.as_ref().map(|f| f.name.to_lowercase()).unwrap_or_default();
    let kdbx = name.ends_with(".kdbx");
    let vault = name.ends_with(".lvault");
    let has = s.file.is_some();
    let mut kids = vec![
        hint(t, k("imp.formats"), None),
        view(json!({"height":40,"flexDirection":"row","gap":8,"alignItems":"center","paddingInlineStart":12,"paddingInlineEnd":6,"borderRadius":8,
            "border":format!("1px solid {}", t.s(t.sep()))}), vec![
            glyph("note", 15., &t.s(if has { t.accent() } else { t.dim() })),
            text(s.file.as_ref().map_or(k("imp.noFile"), |f| json!(f.name)), json!({"flex":1,"minWidth":0,"fontSize":12,"color":if has { t.text() } else { t.dim() },
                "whiteSpace":"nowrap","textOverflow":"ellipsis"})),
            button(t, k("imp.choose"), json!(["impPick", "file"]), Btn { small: true, ..Btn::default() }),
        ]),
    ];
    if kdbx || vault {
        kids.push(view(json!({"gap":8}), vec![
            input("impPw", &s.pw, k(if vault { "imp.vaultPassword" } else { "imp.kdbxPassword" }), true, false, Some("impGo"), json!({})),
            view(json!({"flexDirection":"row","gap":8,"alignItems":"center"}), vec![
                text(s.key.as_ref().map_or(k("imp.noKey"), |f| json!(f.name)), json!({"flex":1,"minWidth":0,"fontSize":12,
                    "color":if s.key.is_some() { t.text() } else { t.dim() },"whiteSpace":"nowrap","textOverflow":"ellipsis"})),
                button(t, k("imp.chooseKey"), json!(["impPick", "key"]), Btn { small: true, ..Btn::default() }),
            ]),
        ]));
    }
    if kdbx {
        kids.push(hint(t, k("imp.kdbxHint"), None));
    }
    if !s.err.is_null() {
        kids.push(text(s.err.clone(), json!({"fontSize":12,"color":RED})));
    }
    if let Some(r) = &s.res {
        let mut lines = vec![text(kp("imp.done", json!({"count":r["count"]})), json!({"fontSize":13,"fontWeight":600,"color":t.text(),"lineHeight":18}))];
        if let Some(fmt) = r["format"].as_str().filter(|f| !f.is_empty()) {
            lines.push(text(kp("imp.detected", json!({"format":k(&format!("fmt.{fmt}"))})), json!({"fontSize":12,"color":t.dim()})));
        }
        if r["cats"].as_i64().unwrap_or(0) > 0 {
            lines.push(text(kp("imp.cats", json!({"count":r["cats"]})), json!({"fontSize":12,"color":t.dim()})));
        }
        kids.push(view(json!({"flexDirection":"row","gap":10,"paddingHorizontal":12,"paddingVertical":10,"borderRadius":10,"backgroundColor":"rgba(48,209,88,0.12)",
            "border":"1px solid rgba(48,209,88,0.35)"}), vec![
            text(json!("✓"), json!({"fontSize":13,"fontWeight":700,"color":GREEN,"lineHeight":18})),
            view(json!({"flex":1,"gap":2}), lines),
        ]));
    }
    kids
}

fn export(t: &Theme, s: &super::ExpState) -> Vec<Value> {
    let mut kids = vec![segmented(t, "expKind", &s.kind, &[("vault", k("exp.vault")), ("csv", k("exp.csv"))], true, true, false)];
    if s.kind == "vault" {
        kids.push(hint(t, k("exp.vaultHint"), None));
    } else {
        kids.push(view(json!({"gap":8}), vec![
            warning(t, k("exp.csvWarning"), false),
            check(t, s.ok, k("exp.csvConfirm"), json!(["expOk"]), Some(json!(RED))),
        ]));
    }
    if !s.err.is_null() {
        kids.push(text(s.err.clone(), json!({"fontSize":12,"color":RED})));
    }
    kids
}

fn color_dots(t: &Theme, colors: &[Value], current: &str, cmd: &str) -> Value {
    view(json!({"flexDirection":"row","flexWrap":"wrap","gap":8}), colors.iter().filter_map(Value::as_str).map(|c| {
        click(
            view(json!({"width":22,"height":22,"borderRadius":11,"alignItems":"center","justifyContent":"center",
                "border":if c == current { format!("2px solid {}", t.s(t.accent())) } else { "2px solid transparent".into() }}), vec![
                view(json!({"width":16,"height":16,"borderRadius":8,"backgroundColor":c}), vec![]),
            ]),
            json!([cmd, c]),
        )
    }).collect())
}

fn category(app: &App, t: &Theme, s: &super::CatState) -> Vec<Value> {
    let catalog = app.catalog();
    let colors = catalog["colors"].as_array().cloned().unwrap_or_default();
    let icons = GLYPH_KINDS.iter().map(|i| {
        let on = *i == s.icon;
        click(
            view(json!({"width":32,"height":32,"borderRadius":7,"alignItems":"center","justifyContent":"center","backgroundColor":if on { t.accent() } else { t.control() }}),
                vec![glyph(i, 16., &if on { "#fff".to_string() } else { t.s(t.text()) })]),
            json!(["catIcon", i]),
        )
    }).collect();
    let parent_label = if s.parent.is_empty() { k("cats.noParent") } else {
        let cats = app.all_cats();
        cats.as_array().into_iter().flatten().find(|c| c["id"] == s.parent.as_str()).map(|c| app.cat_name(c)).unwrap_or(json!(s.parent))
    };
    let mut kids = vec![
        view(json!({"flexDirection":"row","gap":12,"alignItems":"center"}), vec![
            tile(&icon_kind(&s.icon, "folder"), &s.color, 36.),
            input("catName", &s.name, k("cats.name"), false, true, None, json!({"flex":1,"minWidth":0})),
        ]),
        view(json!({"gap":6}), vec![label(t, k("cats.icon")), view(json!({"flexDirection":"row","flexWrap":"wrap","gap":4}), icons)]),
        view(json!({"gap":6}), vec![label(t, k("cats.color")), color_dots(t, &colors, &s.color, "catColor")]),
        view(json!({"flexDirection":"row","gap":12}), vec![
            view(json!({"flex":1,"minWidth":0,"gap":6}), vec![label(t, k("cats.parent")), choice(t, "parent", parent_label, app.choice.as_deref() == Some("parent"), 220.)]),
            view(json!({"flex":1,"minWidth":0,"gap":6}), vec![label(t, k("cats.template")),
                choice(t, "template", k(&format!("cat.{}", s.template)), app.choice.as_deref() == Some("template"), 220.)]),
        ]),
        label(t, k("cats.fields")),
        hint(t, k("cats.fieldsHint"), None),
    ];
    for (i, f) in s.fields.iter().enumerate() {
        let ty = if f["multiline"] == true { "multiline".to_string() } else { f["t"].as_str().unwrap_or("text").to_string() };
        let id = format!("cft:{i}");
        kids.push(view(json!({"flexDirection":"row","gap":6,"alignItems":"center"}), vec![
            input(&format!("cfl:{i}"), f["l"].as_str().unwrap_or(""), k("edit.fieldName"), false, false, None, json!({"flex":1,"minWidth":0})),
            choice(t, &id, k(&format!("ftype.{ty}")), app.choice.as_deref() == Some(id.as_str()), 150.),
            button(t, json!("✕"), json!(["cfDel", i]), Btn { small: true, plain: true, ..Btn::default() }),
        ]));
    }
    let mut add = button(t, k("edit.addFieldDots"), json!(["cfAdd"]), Btn { small: true, icon: Some("+"), ..Btn::default() });
    add["style"]["alignSelf"] = json!("flex-start");
    kids.push(add);
    if !s.err.is_null() {
        kids.push(text(s.err.clone(), json!({"fontSize":12,"color":RED})));
    }
    kids
}

fn tile(kind: &str, color: &str, size: f64) -> Value {
    super::glyphs::tile(kind, color, size, None)
}

pub fn sheet(app: &App, t: &Theme) -> Option<Value> {
    let close = Some(json!(["sheetClose"]));
    let sheet = app.sheet.as_ref()?;
    Some(match sheet {
        Sheet::Gen { o, res, for_field, .. } => super::ui::sheet(t, k("gen.title"), close, 420., generator(app, t, o, res, for_field.is_some()), vec![]),
        Sheet::Settings(s) => match &s.rec {
            Some(code) => super::ui::sheet(t, k("recovery.title"), Some(json!(["recClose"])), 600., vec![super::lock::recovery_body(t, code, s.rec_ok, "recConfirm", "recClose")], vec![]),
            None => super::ui::sheet(t, k("set.title"), close, 560., settings(app, t, s), vec![]),
        },
        Sheet::Import(s) => super::ui::sheet(t, k("imp.title"), close, 540., import(app, t, s), vec![
            button(t, k("close"), json!(["sheetClose"]), Btn::default()),
            button(t, k(if s.busy { "imp.working" } else { "imp.button" }), json!(["impGo"]), Btn { primary: true, ..Btn::default() }),
        ]),
        Sheet::Export(s) => super::ui::sheet(t, k("exp.title"), close, 520., export(t, s), vec![
            button(t, k("cancel"), json!(["sheetClose"]), Btn::default()),
            button(t, k("exp.button"), json!(["expGo"]), Btn { primary: true, disabled: s.kind == "csv" && !s.ok, ..Btn::default() }),
        ]),
        Sheet::Cat(s) => {
            let mut foot = vec![];
            if s.orig.is_some() {
                foot.push(button(t, k("cats.delete"), json!(["catDelete"]), Btn { destructive: true, ..Btn::default() }));
                foot.push(view(json!({"flex":1}), vec![]));
            }
            foot.push(button(t, k("cancel"), json!(["sheetClose"]), Btn::default()));
            foot.push(button(t, k("save"), json!(["catSave"]), Btn { primary: true, ..Btn::default() }));
            super::ui::sheet(t, k(if s.orig.is_none() { "cats.new" } else { "cats.edit" }), close, 540., category(app, t, s), foot)
        }
        Sheet::Tag(s) => {
            let colors = app.catalog()["colors"].as_array().cloned().unwrap_or_default();
            super::ui::sheet(t, k("tags.edit"), close, 420., vec![
                input("tagName", &s.name, json!(""), false, true, None, json!({})),
                color_dots(t, &colors, &s.color, "tagColor"),
            ], vec![
                button(t, k("tags.delete"), json!(["tagDelete"]), Btn { destructive: true, ..Btn::default() }),
                view(json!({"flex":1}), vec![]),
                button(t, k("cancel"), json!(["sheetClose"]), Btn::default()),
                button(t, k("save"), json!(["tagSave"]), Btn { primary: true, ..Btn::default() }),
            ])
        }
        Sheet::Att { name, image, text: body } => {
            let (width, content) = match image {
                Some((w, h, gen)) => ((*w as f64 + 40.).max(360.),
                    json!({"kind":"raster","key":"att","image":"att","generation":gen,"style":{"width":w,"height":h,"borderRadius":6}})),
                None => (520., super::ui::text(json!(body), json!({"fontSize":12,"fontFamily":"monospace","color":t.text(),"lineHeight":17}))),
            };
            super::ui::sheet(t, json!(name), close, width, vec![content], vec![])
        }
    })
}
