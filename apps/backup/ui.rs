use super::{integer, key, string, yes, App};
use crate::native_view::date_format;
use serde_json::{json, Value};
fn v(id: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":id,"style":style,"children":children})
}
fn label(caption: Value, t: &Value, dim: bool, size: u64) -> Value {
    json!({"kind":"text","label":caption,"style":{"fontSize":size,"color":t[if dim{"textDim"}else{"text"}],"textAlign":"start","lineHeight":1.4}})
}
fn text(caption: Value, t: &Value) -> Value {
    label(caption, t, false, 13)
}
fn dim(caption: Value, t: &Value) -> Value {
    label(caption, t, true, 12)
}
fn row(id: &str, children: Vec<Value>) -> Value {
    v(
        id,
        json!({"flexDirection":"row","alignItems":"center","gap":10}),
        children,
    )
}
fn click(mut node: Value, cmd: &str, arg: Value) -> Value {
    node["events"]["onClick"] = json!([cmd, arg]);
    node
}
fn button(caption: Value, cmd: &str, arg: Value, primary: bool, disabled: bool) -> Value {
    json!({"kind":"button","key":format!("{cmd}:{arg}"),"label":caption,"action":[cmd,arg],"primary":primary,"disabled":disabled})
}
fn spacer() -> Value {
    v("spacer", json!({"flex":1}), vec![])
}
fn error(value: &Value, t: &Value) -> Value {
    let mut n = dim(value.clone(), t);
    n["style"]["color"] = json!("#e5534b");
    n
}
fn input(id: &str, value: &str, placeholder: &str, submit: &str, secure: bool) -> Value {
    let mut n = json!({"kind":"input","key":id,"value":value,"placeholder":key(placeholder),"secure":secure,"change":[id],"style":{"flex":1,"minWidth":40,"minHeight":28,"flexShrink":0}});
    if !submit.is_empty() {
        n["submit"] = json!([submit]);
    }
    n
}
fn toggle(caption: Value, cmd: &str, on: bool, disabled: bool, t: &Value) -> Value {
    row(
        cmd,
        vec![
            v("toggle-label", json!({"flex":1}), vec![text(caption, t)]),
            button(
                json!(if on { "✓" } else { "○" }),
                cmd,
                Value::Null,
                on,
                disabled,
            ),
        ],
    )
}
fn group(id: &str, title: &str, footer: Option<&str>, children: Vec<Value>, t: &Value) -> Value {
    let mut c = vec![
        label(key(title), t, true, 12),
        v(
            "group-body",
            json!({"backgroundColor":t["control"],"borderRadius":10,"borderWidth":1,"borderColor":t["separator"],"paddingHorizontal":12,"paddingVertical":8,"gap":8}),
            children,
        ),
    ];
    if let Some(footer) = footer {
        c.push(dim(key(footer), t));
    }
    v(id, json!({"gap":8}), c)
}
fn separator(t: &Value) -> Value {
    v(
        "rule",
        json!({"height":1,"backgroundColor":t["separator"]}),
        vec![],
    )
}
fn glyph(dir: bool, link: bool, t: &Value) -> Value {
    if link {
        return text(json!("↪"), t);
    }
    if dir {
        v(
            "folder",
            json!({"width":16,"height":13,"flexShrink":0,"justifyContent":"flex-end"}),
            vec![
                v(
                    "tab",
                    json!({"position":"absolute","top":0,"left":0,"width":7,"height":4,"borderRadius":1.5,"backgroundColor":"#5ea7ff"}),
                    vec![],
                ),
                v(
                    "body",
                    json!({"height":11,"borderRadius":2.5,"backgroundColor":"#5ea7ff"}),
                    vec![],
                ),
            ],
        )
    } else {
        v(
            "file",
            json!({"width":12,"height":15,"marginHorizontal":2,"borderRadius":2.5,"borderWidth":1.5,"borderColor":t["textDim"],"flexShrink":0}),
            vec![],
        )
    }
}
pub(super) fn icon(s: f64) -> Value {
    let s = if s.is_finite() {
        s.clamp(1., 512.)
    } else {
        32.
    };
    let ring = s * 0.62;
    let bw = (s * 0.06).max(1.5);
    v(
        "backup-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"alignItems":"center","justifyContent":"center","background":"linear-gradient(180deg, #2e8b57 0%, #1d5c3a 100%)","direction":"ltr"}),
        vec![
            v(
                "ring",
                json!({"width":ring,"height":ring,"borderRadius":ring/2.,"borderWidth":bw,"borderColor":"#ffffff","alignItems":"center","justifyContent":"center"}),
                vec![
                    v(
                        "minute",
                        json!({"position":"absolute","left":ring/2.-bw/2.-bw,"top":ring*0.16,"width":bw,"height":ring*0.36,"backgroundColor":"#ffffff","borderRadius":bw/2.}),
                        vec![],
                    ),
                    v(
                        "hour",
                        json!({"position":"absolute","left":ring/2.-bw,"top":ring/2.-bw*1.5,"width":ring*0.3,"height":bw,"backgroundColor":"#ffffff","borderRadius":bw/2.}),
                        vec![],
                    ),
                ],
            ),
            v(
                "gap",
                json!({"position":"absolute","left":s*0.13,"top":s*0.17,"width":s*0.2,"height":s*0.2,"backgroundColor":"#25744a","borderRadius":s*0.04}),
                vec![],
            ),
            v(
                "arrow-h",
                json!({"position":"absolute","left":s*0.16,"top":s*0.3,"width":s*0.17,"height":bw*1.4,"backgroundColor":"#ffffff","borderRadius":bw}),
                vec![],
            ),
            v(
                "arrow-v",
                json!({"position":"absolute","left":s*0.26,"top":s*0.2,"width":bw*1.4,"height":s*0.13,"backgroundColor":"#ffffff","borderRadius":bw}),
                vec![],
            ),
            v(
                "disk",
                json!({"position":"absolute","left":s*0.25,"bottom":s*0.08,"width":s*0.5,"height":s*0.07,"borderRadius":s*0.03,"backgroundColor":"rgba(255,255,255,0.55)"}),
                vec![],
            ),
        ],
    )
}
fn header(app: &mut App, now: i64) -> Value {
    let t = app.theme.clone();
    let running = app.running();
    let st = app.status.clone();
    let mut info = vec![
        label(key("backup.title"), &t, false, 22),
        dim(app.header_line(now), &t),
    ];
    if yes(&st["next"]) && !running && app.state() != "restoring" {
        info.push(dim(
            json!({"key":"backup.next","params":{"when":app.when(integer(&st["next"]),now)}}),
            &t,
        ));
    }
    if st["last"]["ok"] == 0 && !string(&st["last"]["error"]).is_empty() && !running {
        info.push(error(
            &json!({"key":"backup.lastError","params":{"error":{"error":st["last"]["error"]}}}),
            &t,
        ));
    }
    if !app.header_error.is_null() {
        info.push(error(&app.header_error, &t));
    }
    let actions = if running {
        row(
            "run-actions",
            vec![
                button(
                    key(if yes(&st["userPause"]) {
                        "backup.resume"
                    } else {
                        "backup.pause"
                    }),
                    "pause",
                    Value::Null,
                    false,
                    false,
                ),
                button(key("backup.stop"), "cancel", Value::Null, false, false),
            ],
        )
    } else {
        button(key("backup.now"), "run", Value::Null, true, false)
    };
    let mut content = vec![row(
        "header-row",
        vec![
            icon(56.),
            v("status", json!({"flex":1,"minWidth":0,"gap":2}), info),
            actions,
        ],
    )];
    if running {
        let p = &st["progress"];
        let bytes = p["bytes"].as_f64().unwrap_or(0.);
        let total = p["totalBytes"].as_f64().unwrap_or(0.);
        let progress = if total > 0. {
            (bytes / total).clamp(0., 1.)
        } else {
            0.
        };
        let mut filename = dim(p["file"].clone(), &t);
        filename["style"]["flex"] = json!(1);
        filename["style"]["whiteSpace"] = json!("nowrap");
        filename["style"]["overflow"] = json!("hidden");
        content.push(v("progress",json!({"height":6,"borderRadius":3,"backgroundColor":t["control"],"overflow":"hidden","direction":"ltr"}),vec![v("done",json!({"width":format!("{}%",(progress*1000.).round()/10.),"height":6,"borderRadius":3,"backgroundColor":t["accent"]}),vec![])]));
        content.push(row("progress-info",vec![filename,dim(json!({"key":"backup.bytes","params":{"done":{"bytes":bytes},"total":{"bytes":total}}}),&t)]));
    }
    v(
        "header",
        json!({"paddingHorizontal":24,"paddingTop":20,"paddingBottom":16,"gap":16,"flexShrink":0}),
        content,
    )
}
fn encryption(app: &App, t: &Value) -> Value {
    let encrypted = yes(&app.status["destEncrypted"]);
    let locked = yes(&app.status["locked"]);
    let setup = !encrypted && (app.want_encrypt || yes(&app.config["encrypt"]));
    let busy = app.state() == "unlocking" || app.unlock_pending || app.saving || !app.config_ready;
    let mut c = vec![];
    if !encrypted && !setup {
        c.push(dim(key("backup.encOffHelp"), t));
        c.push(toggle(
            key("backup.encryption"),
            "wantEncrypt",
            false,
            busy,
            t,
        ));
    }
    if setup {
        c.push(dim(key("backup.encWarn"), t));
        c.push(input("p1", &app.p1, "backup.encPass", "", true));
        c.push(input("p2", &app.p2, "backup.encPass2", "turnOn", true));
        c.push(row(
            "encryption-actions",
            vec![
                spacer(),
                button(key("backup.encNo"), "noEncrypt", Value::Null, false, busy),
                button(key("backup.encTurnOn"), "turnOn", Value::Null, true, busy),
            ],
        ));
    }
    if encrypted && locked {
        c.push(dim(key("backup.encLocked"), t));
        c.push(row(
            "unlock",
            vec![
                input("p1", &app.p1, "backup.encPass", "unlock", true),
                button(key("backup.encUnlock"), "unlock", Value::Null, true, busy),
            ],
        ));
    }
    if encrypted && !locked {
        c.push(row(
            "lock",
            vec![
                text(key("backup.encOn"), t),
                spacer(),
                button(key("backup.encLock"), "lock", Value::Null, false, busy),
            ],
        ));
    }
    if app.state() == "unlocking" {
        c.push(dim(key("backup.encWorking"), t));
    } else if yes(&app.status["unlock"]["done"]) && !yes(&app.status["unlock"]["ok"]) {
        c.push(error(&json!({"error":app.status["unlock"]["error"]}), t));
    } else if !app.encryption_error.is_null() {
        c.push(error(&app.encryption_error, t));
    }
    group("encryption", "backup.encryption", None, c, t)
}
fn settings(app: &App) -> Value {
    let t = &app.theme;
    let cfg = &app.config;
    let disabled = app.saving || !app.config_ready;
    let removable = app
        .volumes
        .iter()
        .filter(|v| string(&v["path"]) != "/" && string(&v["path"]) != app.home);
    let mut drives = vec![];
    for drive in removable {
        let selected = cfg["dest"] == drive["path"];
        let mut info = vec![
            text(
                if string(&drive["name"]).is_empty() {
                    drive["path"].clone()
                } else {
                    drive["name"].clone()
                },
                t,
            ),
            dim(
                json!({"key":"backup.volFree","params":{"free":{"bytes":integer(&drive["free"])},"total":{"bytes":integer(&drive["total"])},"fs":drive["fs"]}}),
                t,
            ),
        ];
        if ["vfat", "exfat", "msdos", "ntfs", "ntfs3", "fuseblk"].contains(&string(&drive["fs"])) {
            let mut warn = dim(key("backup.noLinks"), t);
            warn["style"]["color"] = json!("#d29922");
            info.push(warn);
        }
        let mut drive_node = row(
            &format!("drive:{}", drive["path"]),
            vec![
                v(
                    "drive-icon",
                    json!({"width":26,"height":18,"borderRadius":4,"backgroundColor":if t["mode"]=="dark"{"#8e8e93"}else{"#aeaeb2"},"direction":"ltr"}),
                    vec![v(
                        "led",
                        json!({"position":"absolute","right":5,"top":7,"width":4,"height":4,"borderRadius":2,"backgroundColor":"#30d158"}),
                        vec![],
                    )],
                ),
                v("drive-info", json!({"flex":1,"gap":1,"minWidth":0}), info),
                button(
                    json!(if selected { "✓" } else { "○" }),
                    "volume",
                    drive["path"].clone(),
                    selected,
                    disabled,
                ),
            ],
        );
        if !disabled {
            drive_node = click(drive_node, "volume", drive["path"].clone());
        }
        drives.push(drive_node);
    }
    if drives.is_empty() {
        drives.push(dim(key("backup.noDisks"), t));
    }
    drives.push(row(
        "folder",
        vec![
            input(
                "folder",
                &app.folder,
                "backup.folderPlaceholder",
                "useFolder",
                false,
            ),
            button(
                key("backup.useFolder"),
                "useFolder",
                Value::Null,
                false,
                disabled,
            ),
        ],
    ));
    if !string(&cfg["dest"]).is_empty() {
        let path = string(&cfg["dest"]);
        let short = path
            .strip_prefix(&app.home)
            .filter(|_| !app.home.is_empty())
            .map_or_else(|| path.into(), |s| format!("~{s}"));
        let mut parts = vec![json!({"key":"backup.current","params":{"path":short}})];
        if integer(&app.status["destTotal"]) > 0 {
            parts.extend([json!(" · "),json!({"key":"backup.free","params":{"free":{"bytes":app.status["destFree"]},"total":{"bytes":app.status["destTotal"]}}})]);
        }
        drives.push(row(
            "current",
            vec![
                v(
                    "current-path",
                    json!({"flex":1,"minWidth":0}),
                    vec![dim(json!({"parts":parts}), t)],
                ),
                button(
                    key("backup.stopUsing"),
                    "stopUsing",
                    Value::Null,
                    false,
                    disabled,
                ),
            ],
        ));
    }
    let mut intervals = vec![];
    for k in ["hourly", "daily", "weekly", "manual"] {
        intervals.push(button(
            key(&format!("backup.{k}")),
            "interval",
            json!(k),
            cfg["interval"] == k,
            disabled,
        ));
    }
    let mut schedule = row("intervals", intervals);
    schedule["style"]["flexWrap"] = json!("wrap");
    let mut excludes = vec![];
    for item in cfg["exclude"].as_array().into_iter().flatten() {
        excludes.push(row(
            &format!("exclude:{item}"),
            vec![
                glyph(true, false, t),
                v(
                    "exclude-path",
                    json!({"flex":1,"minWidth":0}),
                    vec![text(json!(format!("~/{}", string(item))), t)],
                ),
                button(
                    key("backup.remove"),
                    "removeExclude",
                    item.clone(),
                    false,
                    disabled,
                ),
            ],
        ));
    }
    excludes.push(row(
        "add-exclusion",
        vec![
            input(
                "exclude",
                &app.exclude,
                "backup.excludePlaceholder",
                "addExclude",
                false,
            ),
            button(
                key("backup.add"),
                "addExclude",
                Value::Null,
                false,
                disabled,
            ),
        ],
    ));
    let mut content = vec![
        group(
            "destination",
            "backup.dest",
            Some("backup.destHelp"),
            drives,
            t,
        ),
        group(
            "schedule",
            "backup.schedule",
            Some("backup.retention"),
            vec![
                schedule,
                toggle(
                    key("backup.battery"),
                    "battery",
                    cfg["pauseOnBattery"] != false,
                    disabled,
                    t,
                ),
                dim(key("backup.priority"), t),
            ],
            t,
        ),
        group(
            "exclusions",
            "backup.exclude",
            Some("backup.excludeHelp"),
            excludes,
            t,
        ),
        encryption(app, t),
    ];
    if !app.config_error.is_null() {
        content.push(error(&app.config_error, t));
    }
    json!({"kind":"scroll","key":"settings","style":{"flex":1},"children":[v("settings-pad",json!({"alignItems":"center","paddingHorizontal":24,"paddingTop":20,"paddingBottom":32}),vec![v("settings-body",json!({"width":"100%","maxWidth":640,"gap":24}),content)])]})
}
fn confirm(app: &mut App, now: i64) -> Value {
    let ask = app.ask.clone().unwrap();
    let t = app.theme.clone();
    let all = ask.kind == "all";
    let body = if all {
        if ask.replace {
            "backup.restoreAllReplace"
        } else {
            "backup.restoreAllKeep"
        }
    } else {
        "backup.deleteBody"
    };
    let mut ok = button(
        key(if all {
            "backup.restoreAll"
        } else {
            "backup.delete"
        }),
        "confirm",
        Value::Null,
        true,
        app.busy(),
    );
    if !all {
        ok["destructive"] = json!(true);
    }
    v(
        "confirm-overlay",
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"backgroundColor":"rgba(0,0,0,0.35)","alignItems":"center","justifyContent":"center"}),
        vec![v(
            "confirm",
            json!({"width":420,"padding":24,"gap":16,"borderRadius":12,"backgroundColor":t["windowBg"],"borderWidth":1,"borderColor":t["separator"],"boxShadow":"0 12px 40px rgba(0,0,0,0.45)"}),
            vec![
                label(
                    key(if all {
                        "backup.restoreAllTitle"
                    } else {
                        "backup.deleteTitle"
                    }),
                    &t,
                    false,
                    15,
                ),
                dim(
                    json!({"key":body,"params":{"when":app.when(ask.time,now)}}),
                    &t,
                ),
                row(
                    "confirm-actions",
                    vec![
                        spacer(),
                        button(key("backup.cancel"), "dismiss", Value::Null, false, false),
                        ok,
                    ],
                ),
            ],
        )],
    )
}
fn browser(app: &mut App, now: i64) -> Value {
    let t = app.theme.clone();
    if app.snapshots.is_empty() {
        let mut c = vec![
            icon(56.),
            label(key("backup.emptyTitle"), &t, false, 15),
            dim(
                key(if string(&app.status["dest"]).is_empty() {
                    "backup.emptyNoDest"
                } else {
                    "backup.emptyBody"
                }),
                &t,
            ),
        ];
        if !app.browser_error.is_null() {
            c.push(error(&app.browser_error, &t));
        }
        return v(
            "empty",
            json!({"flex":1,"alignItems":"center","justifyContent":"center","padding":20,"gap":12}),
            c,
        );
    }
    let narrow = app.width - 220. < 700.;
    let busy = app.busy();
    let mut timeline = vec![];
    let mut day = None;
    for snap in app.snapshots.clone() {
        let time = crate::clock::local(integer(&snap["time"]).saturating_mul(1000));
        let date = (time.tm_year, time.tm_yday);
        if day != Some(date) {
            day = Some(date);
            timeline.push(dim(
                json!(date_format::date(
                    &mut app.i18n,
                    &app.region,
                    &time,
                    "dayMonth"
                )),
                &t,
            ));
        }
        let mut node = button(
            json!(date_format::time(
                &mut app.i18n,
                &app.region,
                &app.hour,
                &time,
                false
            )),
            "snapshot",
            snap["id"].clone(),
            app.selected.as_deref() == Some(string(&snap["id"])),
            false,
        );
        node["style"] = json!({"alignSelf":"stretch"});
        timeline.push(node);
    }
    let sidebar = v(
        "timeline",
        json!({"width":220,"flexShrink":0,"backgroundColor":t["sidebarBg"],"borderEndWidth":1,"borderEndColor":t["separator"]}),
        vec![
            json!({"kind":"scroll","key":"timeline-scroll","style":{"flex":1},"children":[v("timeline-items",json!({"paddingHorizontal":10,"paddingBottom":16,"gap":2}),timeline)]}),
            v(
                "count",
                json!({"height":30,"justifyContent":"center","paddingHorizontal":20}),
                vec![dim(
                    json!({"key":"backup.count","params":{"count":app.snapshots.len()}}),
                    &t,
                )],
            ),
        ],
    );
    let mut crumbs = vec![button(key("backup.home"), "crumb", json!(0), false, false)];
    for (i, c) in app.rel.split('/').filter(|c| !c.is_empty()).enumerate() {
        crumbs.push(dim(json!(if app.rtl { "‹" } else { "›" }), &t));
        crumbs.push(button(json!(c), "crumb", json!(i + 1), false, false));
    }
    let mut crumbrow = row("crumbs", crumbs);
    crumbrow["style"]["flex"] = json!(1);
    crumbrow["style"]["minWidth"] = json!(0);
    crumbrow["style"]["overflow"] = json!("hidden");
    let toolbar = v(
        "toolbar",
        json!({"height":44,"paddingHorizontal":12,"flexDirection":"row","alignItems":"center","gap":4,"flexShrink":0}),
        vec![
            crumbrow,
            button(
                key("backup.openFiles"),
                "openFiles",
                Value::Null,
                false,
                !app.selected_exists(),
            ),
        ],
    );
    let cols = |name: Value, size: Value, modified: Option<Value>, icon: Value| {
        let mut names = vec![
            v(
                "file-icon",
                json!({"width":18,"alignItems":"center","flexShrink":0}),
                vec![icon],
            ),
            v(
                "name",
                json!({"flex":1,"minWidth":0,"overflow":"hidden"}),
                vec![name],
            ),
            v(
                "size",
                json!({"width":90,"alignItems":"flex-end","flexShrink":0}),
                vec![size],
            ),
        ];
        if let Some(modified) = modified {
            names.push(v(
                "modified",
                json!({"width":150,"alignItems":"flex-end","flexShrink":0}),
                vec![modified],
            ));
        }
        names
    };
    let headings = v(
        "headings",
        json!({"height":26,"flexDirection":"row","alignItems":"center","gap":10,"paddingHorizontal":20,"borderBottomWidth":1,"borderBottomColor":t["separator"],"flexShrink":0}),
        cols(
            dim(key("backup.col.name"), &t),
            dim(key("backup.col.size"), &t),
            (!narrow).then(|| dim(key("backup.col.modified"), &t)),
            Value::Null,
        ),
    );
    let mut items = vec![];
    for (i, item) in app.items.clone().into_iter().enumerate() {
        let selected = app.pick.as_deref() == Some(string(&item["name"]));
        let mut colors = t.clone();
        if selected {
            colors["text"] = json!("#fff");
            colors["textDim"] = json!("rgba(255,255,255,0.85)");
        }
        let mut name = text(item["name"].clone(), &colors);
        name["style"]["whiteSpace"] = json!("nowrap");
        name["style"]["overflow"] = json!("hidden");
        let modified = if narrow {
            None
        } else {
            Some(dim(app.when(integer(&item["mtime"]), now), &colors))
        };
        let mut node = v(
            &format!("entry:{}", item["name"]),
            json!({"flexDirection":"row","alignItems":"center","gap":10,"height":28,"paddingHorizontal":8,"borderRadius":6,"backgroundColor":if selected{t["accent"].clone()}else if i%2==1{json!(if t["mode"]=="dark"{"rgba(255,255,255,0.03)"}else{"rgba(0,0,0,0.03)"})}else{json!("transparent")}}),
            cols(
                name,
                dim(
                    if yes(&item["dir"]) {
                        json!("—")
                    } else {
                        json!({"bytes":integer(&item["size"])})
                    },
                    &colors,
                ),
                modified,
                glyph(yes(&item["dir"]), yes(&item["link"]), &colors),
            ),
        );
        node["events"]["onClick"] = json!(["pick", item["name"]]);
        node["events"]["onDoubleClick"] = json!(["enter", item["name"]]);
        items.push(node);
    }
    if items.is_empty() {
        items.push(dim(
            if app.browser_error.is_null() {
                key("backup.emptyFolder")
            } else {
                app.browser_error.clone()
            },
            &t,
        ));
    }
    let list = json!({"kind":"scroll","key":"browser-scroll","style":{"flex":1},"children":[v("items",json!({"paddingVertical":6,"paddingHorizontal":8}),items)]});
    let mut footer = vec![];
    let rr = &app.status["restore"];
    if yes(&rr["done"]) && app.selected.as_deref() == Some(string(&rr["id"])) {
        footer.push(dim(if yes(&rr["ok"]){json!({"key":"backup.restoredInfo","params":{"count":rr["files"],"both":rr["keptBoth"],"replaced":rr["replaced"]}})}else{json!({"key":"backup.restoreFailed","params":{"error":if !string(&rr["error"]).is_empty(){json!({"error":rr["error"]})}else{json!({"key":"backup.restoreErrors","params":{"count":rr["errors"]}})}}})},&t));
    }
    if !app.browser_error.is_null() && !app.items.is_empty() {
        footer.push(error(&app.browser_error, &t));
    }
    let mut actions = vec![
        button(
            json!(if app.replace { "✓" } else { "○" }),
            "replace",
            Value::Null,
            app.replace,
            false,
        ),
        dim(key("backup.replace"), &t),
        spacer(),
    ];
    if narrow {
        actions.push(button(
            key("backup.more"),
            "more",
            Value::Null,
            false,
            false,
        ));
    }
    if !narrow || app.more {
        actions.push(button(
            key("backup.delete"),
            "askDelete",
            Value::Null,
            false,
            busy,
        ));
        actions.push(button(
            key("backup.restoreAll"),
            "askAll",
            Value::Null,
            false,
            busy,
        ));
    }
    actions.push(button(
        if let Some(pick) = &app.pick {
            json!({"key":"backup.restoreItem","params":{"name":pick}})
        } else {
            key(if app.rel.is_empty() {
                "backup.restorePick"
            } else {
                "backup.restoreFolder"
            })
        },
        "restore",
        Value::Null,
        true,
        busy || app.pick.is_none() && app.rel.is_empty(),
    ));
    let mut actionrow = row("restore-actions", actions);
    actionrow["style"]["flexWrap"] = json!("wrap");
    footer.push(actionrow);
    let mut footerbox = v(
        "footer",
        json!({"paddingHorizontal":20,"paddingVertical":16,"gap":8,"flexShrink":0}),
        footer,
    );
    footerbox["children"]
        .as_array_mut()
        .unwrap()
        .insert(0, separator(&t));
    v(
        "browser",
        json!({"flex":1,"flexDirection":"row"}),
        vec![
            sidebar,
            v(
                "browser-main",
                json!({"flex":1,"minWidth":0}),
                vec![toolbar, headings, list, footerbox],
            ),
        ],
    )
}
pub(super) fn tree(app: &mut App, now: i64) -> Value {
    let t = app.theme.clone();
    let mut content = vec![
        header(app, now),
        v(
            "tabs",
            json!({"height":44,"flexDirection":"row","alignItems":"center","justifyContent":"center","gap":4,"borderBottomWidth":1,"borderBottomColor":t["separator"],"flexShrink":0}),
            vec![
                button(
                    key("backup.tabSnapshots"),
                    "tab",
                    json!("snapshots"),
                    app.tab == "snapshots",
                    false,
                ),
                button(
                    key("backup.tabSettings"),
                    "tab",
                    json!("settings"),
                    app.tab == "settings",
                    false,
                ),
            ],
        ),
        if app.tab == "snapshots" {
            browser(app, now)
        } else {
            settings(app)
        },
    ];
    if app.ask.is_some() {
        content.push(confirm(app, now));
    }
    v(
        "backup",
        json!({"flex":1,"direction":if app.rtl{"rtl"}else{"ltr"}}),
        content,
    )
}
