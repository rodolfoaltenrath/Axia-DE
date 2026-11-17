//! Contacts' three columns and local editors, mounted directly in the scene.
use super::*;
fn v(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
fn label(k: &str) -> Value {
    json!({"key":format!("contacts.{k}")})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn token(a: &App, k: &str, fallback: &str) -> String {
    a.theme[k].as_str().unwrap_or(fallback).into()
}
fn fg(a: &App) -> String {
    token(a, "text", "#eeeeee")
}
fn dim(a: &App) -> String {
    token(a, "textDim", "#999999")
}
fn accent(a: &App) -> String {
    token(a, "accent", "#007aff")
}
fn click(mut node: Value, action: Value) -> Value {
    node["events"]["onClick"] = action;
    node
}
fn spacer() -> Value {
    v("spacer", json!({"flex":1}), vec![])
}
fn button(k: &str, action: Value, disabled: bool) -> Value {
    json!({"kind":"button","key":format!("button-{k}"),"label":label(k),"small":true,"disabled":disabled,"action":action})
}
fn input(
    key: &str,
    value: &str,
    placeholder: Value,
    action: Value,
    auto: bool,
    disabled: bool,
) -> Value {
    json!({"kind":"input","key":key,"value":value,"placeholder":placeholder,"change":if disabled{json!(["noop"])}else{action},"autoFocus":auto&&!disabled,"size":13,"style":{"height":26,"opacity":if disabled{0.6}else{1.}}})
}
fn rule(a: &App) -> Value {
    v(
        "rule",
        json!({"height":1,"backgroundColor":token(a,"separator","#555555")}),
        vec![],
    )
}
fn footer(a: &App, children: Vec<Value>) -> Value {
    v(
        "footer",
        json!({"height":40,"paddingHorizontal":10,"gap":8,"flexDirection":"row","alignItems":"center","borderTopWidth":1,"borderColor":token(a,"separator","#555555")}),
        children,
    )
}
fn fallback_name(c: &Value) -> Value {
    let name = display_name(c);
    if name.is_empty() {
        label("noName")
    } else {
        json!(name)
    }
}
fn type_label(kind: &str, raw: &str) -> Value {
    let ty = field_type(kind, raw);
    if field_types(kind).contains(&ty.as_str()) {
        label(&format!("type.{ty}"))
    } else {
        json!(ty)
    }
}
fn person(size: f64, color: &str) -> Value {
    v(
        "person",
        json!({"width":size,"height":size,"alignItems":"center"}),
        vec![
            v(
                "head",
                json!({"width":size*0.44,"height":size*0.44,"borderRadius":size*0.22,"backgroundColor":color}),
                vec![],
            ),
            v(
                "body",
                json!({"marginTop":size*0.06,"width":size*0.86,"height":size*0.42,"borderTopLeftRadius":size*0.43,"borderTopRightRadius":size*0.43,"borderBottomLeftRadius":size*0.08,"borderBottomRightRadius":size*0.08,"backgroundColor":color}),
                vec![],
            ),
        ],
    )
}
pub fn icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    let mut children = vec![
        v(
            "binding",
            json!({"position":"absolute","left":0,"top":0,"bottom":0,"width":s*0.16,"backgroundColor":"#6f5843"}),
            vec![],
        ),
        v(
            "card",
            json!({"position":"absolute","left":s*0.24,"top":s*0.14,"width":s*0.62,"height":s*0.72,"borderRadius":s*0.06,"backgroundColor":"#f4f1ec","overflow":"hidden","alignItems":"center","paddingTop":s*0.12}),
            vec![
                v(
                    "head",
                    json!({"width":s*0.22,"height":s*0.22,"borderRadius":s*0.11,"backgroundColor":"#9b9b9f"}),
                    vec![],
                ),
                v(
                    "body",
                    json!({"marginTop":s*0.04,"width":s*0.42,"height":s*0.42,"borderRadius":s*0.21,"backgroundColor":"#9b9b9f"}),
                    vec![],
                ),
            ],
        ),
    ];
    for (i, y) in [0.3, 0.5, 0.7].iter().enumerate() {
        children.push(v(&format!("tab-{i}"),json!({"position":"absolute","left":s*0.86,"top":s*y,"width":s*0.06,"height":s*0.1,"borderRadius":s*0.02,"backgroundColor":"#e0a33a"}),vec![]));
    }
    v(
        "contacts-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(180deg, #b49a7e, #8c7259)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden"}),
        children,
    )
}
fn source_row(
    a: &App,
    id: &str,
    title: Value,
    count: usize,
    indent: bool,
    glyph: Option<&str>,
) -> Value {
    let selected = a.state.source == id;
    let color = if selected { "#ffffff".into() } else { fg(a) };
    let mut row = click(
        v(
            &format!("source-{id}"),
            json!({"height":28,"paddingHorizontal":if indent{24}else{8},"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":8,"backgroundColor":if selected{accent(a)}else{"transparent".into()}}),
            vec![
                if let Some(g) = glyph {
                    text(
                        json!(g),
                        json!({"width":16,"fontSize":13,"color":if selected{"#ffffff".into()}else{accent(a)}}),
                    )
                } else {
                    person(
                        14.,
                        &if selected {
                            "#ffffff".into()
                        } else {
                            accent(a)
                        },
                    )
                },
                text(
                    title,
                    json!({"flex":1,"fontSize":13,"color":color,"whiteSpace":"nowrap","overflow":"hidden"}),
                ),
                text(
                    json!(count.to_string()),
                    json!({"fontSize":11,"color":if selected{"rgba(255,255,255,0.8)".into()}else{dim(a)}}),
                ),
            ],
        ),
        json!(["source", id]),
    );
    row["hoverStyle"] = json!({"backgroundColor":if selected{accent(a)}else{token(a,"hover","rgba(127,127,127,0.12)")}});
    if let Some(k) = id.strip_prefix("group:") {
        row["events"]["onContextMenu"] = json!(["menu", format!("group:{k}")]);
    }
    row
}
fn sources(a: &App) -> Value {
    let people: Vec<_> = a
        .state
        .contacts
        .iter()
        .filter(|c| c["kind"] != "group")
        .collect();
    let groups = groups(&a.state.contacts);
    let mut children = vec![source_row(
        a,
        "all",
        label("allContacts"),
        people.len(),
        false,
        None,
    )];
    for b in &a.state.books {
        let mut rows = vec![
            text(
                if string(&b["acct"]).is_empty() {
                    label("onThisComputer")
                } else {
                    json!(b["acctName"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(string(&b["acct"])))
                },
                json!({"fontSize":11,"fontWeight":600,"color":dim(a),"marginHorizontal":8,"marginBottom":2,"whiteSpace":"nowrap","overflow":"hidden"}),
            ),
            source_row(
                a,
                &format!("book:{}", string(&b["id"])),
                b["name"].clone(),
                people.iter().filter(|c| c["book"] == b["id"]).count(),
                false,
                Some("▤"),
            ),
        ];
        for g in groups.iter().filter(|g| g["book"] == b["id"]) {
            rows.push(source_row(
                a,
                &format!("group:{}", key(g)),
                if display_name(g).is_empty() {
                    label("untitledGroup")
                } else {
                    json!(display_name(g))
                },
                g["members"].as_array().map_or(0, Vec::len),
                true,
                Some("▢"),
            ));
        }
        children.push(v(
            &format!("book-{}", string(&b["id"])),
            json!({"gap":2,"marginTop":16}),
            rows,
        ));
    }
    v(
        "sources",
        json!({"width":212,"backgroundColor":token(a,"sidebarBg","#202024"),"borderEndWidth":1,"borderColor":token(a,"separator","#555555")}),
        vec![
            json!({"kind":"scroll","key":"sources-scroll","style":{"flex":1},"children":[v("sources-content",json!({"paddingHorizontal":10,"paddingTop":12,"paddingBottom":12,"gap":2}),children)]}),
            footer(
                a,
                vec![
                    button("newGroup", json!(["groupNew"]), a.state.unavailable),
                    spacer(),
                    button("import", json!(["import"]), a.state.unavailable),
                ],
            ),
        ],
    )
}
fn contact_list(a: &App) -> Value {
    let list = visible(&a.state.contacts, &a.state.source, &a.state.query);
    let layout = ListLayout::new(&list);
    let h = (a.height - 92.).max(1.);
    let offset = a.offset;
    let mut rows = vec![];
    for i in layout.viewport(offset, h) {
        let (top, row) = &layout.rows[i];
        let node = match row {
            Row::Heading(s) => v(
                &format!("heading-{i}"),
                json!({"position":"absolute","left":14,"right":14,"top":top-offset,"height":24,"justifyContent":"flex-end","paddingBottom":3,"borderBottomWidth":1,"borderColor":token(a,"separator","#555555")}),
                vec![text(
                    json!(s),
                    json!({"fontSize":11,"fontWeight":600,"color":dim(a)}),
                )],
            ),
            Row::Contact(index) => {
                let c = list[*index];
                let selected = key(c) == a.state.selected;
                let color = if selected { "#ffffff".into() } else { fg(a) };
                let given = ["prefix", "given", "middle"]
                    .iter()
                    .map(|k| string(&c["n"][k]))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let family = ["family", "suffix"]
                    .iter()
                    .map(|k| string(&c["n"][k]))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut pieces = vec![];
                if !given.is_empty() {
                    pieces.push(text(
                        json!(given),
                        json!({"fontSize":13,"color":color,"whiteSpace":"nowrap"}),
                    ));
                }
                if !family.is_empty() {
                    pieces.push(text(json!(family),json!({"fontSize":13,"fontWeight":600,"color":color,"whiteSpace":"nowrap","overflow":"hidden","flexShrink":1})));
                }
                if pieces.is_empty() {
                    pieces.push(text(fallback_name(c),json!({"fontSize":13,"fontWeight":600,"color":color,"whiteSpace":"nowrap","overflow":"hidden","flexShrink":1})));
                }
                let mut n = click(
                    v(
                        &format!("contact-{}", key(c)),
                        json!({"position":"absolute","left":8,"right":8,"top":top-offset+1.,"height":28,"paddingHorizontal":8,"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":4,"backgroundColor":if selected{accent(a)}else{"transparent".into()}}),
                        pieces,
                    ),
                    json!(["select", key(c)]),
                );
                n["hoverStyle"] = json!({"backgroundColor":if selected{accent(a)}else{token(a,"hover","rgba(127,127,127,0.12)")}});
                n
            }
        };
        rows.push(node);
    }
    if layout.height > h {
        let bar = (h * h / layout.height).max(28.).min(h);
        let mut n = v(
            "list-thumb",
            json!({"position":"absolute","width":6,"borderRadius":3,"height":bar,"top":(h-bar)*offset/(layout.height-h).max(1.),"backgroundColor":if a.theme["mode"]=="dark"{"rgba(255,255,255,0.3)"}else{"rgba(0,0,0,0.28)"}}),
            vec![],
        );
        n["style"][if a.rtl { "left" } else { "right" }] = json!(2);
        n["events"] = json!({"onClick":["noop"],"onMouseDown":["scrollDrag"]});
        rows.push(n);
    }
    if list.is_empty() && !a.state.unavailable {
        rows.push(empty(
            a,
            if a.state.query.trim().is_empty() {
                "noContacts"
            } else {
                "noResults"
            },
            None,
        ));
    }
    let mut viewport = v("contact-list", json!({"flex":1,"overflow":"hidden"}), rows);
    viewport["events"] = json!({"onWheel":["wheel"],"onGesture":["gesture"]});
    let count = json!({"key":"contacts.count","params":{"count":list.len(),"n":a.i18n.number(list.len() as f64,0)}});
    v(
        "list-column",
        json!({"width":270,"borderEndWidth":1,"borderColor":token(a,"separator","#555555")}),
        vec![
            v(
                "search",
                json!({"height":52,"paddingHorizontal":12,"flexDirection":"row","alignItems":"center","gap":4}),
                vec![
                    v(
                        "search-field",
                        json!({"flex":1}),
                        vec![input(
                            "search",
                            &a.state.query,
                            label("search"),
                            json!(["query"]),
                            false,
                            false,
                        )],
                    ),
                    if a.state.query.is_empty() {
                        Value::Null
                    } else {
                        click(
                            v(
                                "clear-search",
                                json!({"width":18,"height":22,"alignItems":"center","justifyContent":"center"}),
                                vec![text(json!("×"), json!({"fontSize":16,"color":dim(a)}))],
                            ),
                            json!(["clearQuery"]),
                        )
                    },
                ],
            ),
            viewport,
            footer(
                a,
                vec![
                    button("newContact", json!(["new"]), a.state.unavailable),
                    text(
                        count,
                        json!({"flex":1,"fontSize":11,"color":dim(a),"textAlign":"center","whiteSpace":"nowrap"}),
                    ),
                ],
            ),
        ],
    )
}
fn empty(a: &App, title: &str, hint: Option<&str>) -> Value {
    let mut children = vec![text(
        label(title),
        json!({"fontSize":16,"fontWeight":600,"color":dim(a),"textAlign":"center"}),
    )];
    if let Some(k) = hint {
        children.push(text(
            label(k),
            json!({"fontSize":12,"color":dim(a),"textAlign":"center"}),
        ));
    }
    v(
        "empty",
        json!({"flex":1,"padding":20,"gap":10,"alignItems":"center","justifyContent":"center"}),
        children,
    )
}
fn photo(a: &App, c: &Value, size: f64) -> Value {
    let child = if a.state.photo_handle > 0 {
        json!({"kind":"mailImage","key":"contact-photo","image":a.state.photo_handle,"style":{"width":size,"height":size,"objectFit":"cover"}})
    } else {
        text(
            json!(initials(c)),
            json!({"fontSize":size*0.4,"fontWeight":600,"color":"#ffffff"}),
        )
    };
    v(
        "photo",
        json!({"width":size,"height":size,"borderRadius":size/2.,"overflow":"hidden","backgroundColor":photo_color(c),"alignItems":"center","justifyContent":"center"}),
        vec![child],
    )
}
fn card_row(a: &App, title: Value, value: String, action: Option<Value>) -> Value {
    let w = a.width - 482.;
    let lw = if w < 300. {
        0
    } else if w < 400. {
        70
    } else {
        96
    };
    let mut val = text(
        json!(value),
        json!({"flex":1,"fontSize":13,"color":if action.is_some(){accent(a)}else{fg(a)},"lineHeight":18}),
    );
    if let Some(action) = action {
        val["style"].as_object_mut().unwrap().remove("flex");
        val = click(v("link", json!({"flex":1}), vec![val]), action);
    }
    v(
        "card-row",
        json!({"flexDirection":if lw==0{"column"}else{"row"},"gap":if lw==0{1}else{12},"paddingVertical":5}),
        vec![
            text(
                title,
                if lw == 0 {
                    json!({"fontSize":11,"color":dim(a)})
                } else {
                    json!({"width":lw,"fontSize":12,"color":dim(a),"textAlign":"end","paddingTop":1,"whiteSpace":"nowrap","overflow":"hidden"})
                },
            ),
            val,
        ],
    )
}
fn card(a: &mut App, c: &Value) -> Value {
    let narrow = a.width - 482. < 400.;
    let mut names = vec![text(
        fallback_name(c),
        json!({"fontSize":if narrow{20}else{24},"fontWeight":600,"color":fg(a),"textAlign":if narrow{"center"}else{"start"}}),
    )];
    let sub = ["title", "dept", "org"]
        .iter()
        .map(|k| string(&c[k]))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let show_org = !string(&c["org"]).is_empty()
        && (!string(&c["n"]["given"]).is_empty() || !string(&c["n"]["family"]).is_empty())
        && display_name(c) != c["org"];
    if !sub.is_empty() && (show_org || !string(&c["title"]).is_empty()) {
        names.push(text(
            json!(sub),
            json!({"fontSize":13,"color":dim(a),"textAlign":if narrow{"center"}else{"start"}}),
        ));
    }
    if !string(&c["nick"]).is_empty() {
        names.push(text(
            json!(format!("“{}”", string(&c["nick"]))),
            json!({"fontSize":12,"color":dim(a)}),
        ));
    }
    let mine = groups(&a.state.contacts)
        .iter()
        .filter(|g| is_member(g, c))
        .map(|g| display_name(g))
        .collect::<Vec<_>>()
        .join(", ");
    if !mine.is_empty() {
        names.push(text(json!(mine), json!({"fontSize":11,"color":dim(a)})));
    }
    let heading = v(
        "heading",
        json!({"flexDirection":if narrow{"column"}else{"row"},"alignItems":"center","gap":if narrow{12}else{20}}),
        vec![
            photo(a, c, if narrow { 72. } else { 88. }),
            v(
                "name",
                json!({"flex":if narrow{0}else{1},"alignSelf":"stretch","alignItems":if narrow{"center"}else{"flex-start"},"gap":3}),
                names,
            ),
        ],
    );
    let mut rows = vec![];
    for kind in KINDS {
        for (i, r) in c[kind].as_array().into_iter().flatten().enumerate() {
            let value = if kind == "addrs" {
                let middle = [string(&r["code"]), string(&r["city"])]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                [
                    string(&r["street"]),
                    &middle,
                    string(&r["region"]),
                    string(&r["country"]),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
            } else {
                string(&r["value"]).into()
            };
            let mut row = card_row(
                a,
                type_label(kind, string(&r["type"])),
                value,
                (kind == "emails").then(|| json!(["email", i])),
            );
            row["key"] = json!(format!("{kind}-{i}"));
            rows.push(row);
        }
    }
    if !string(&c["bday"]).is_empty() {
        let birthday = a.birthday(string(&c["bday"]), false);
        rows.push(card_row(a, label("birthday"), birthday, None));
    }
    if !string(&c["notes"]).is_empty() {
        rows.push(card_row(
            a,
            label("notes"),
            string(&c["notes"]).into(),
            None,
        ));
    }
    let has_groups = groups(&a.state.contacts)
        .iter()
        .any(|g| g["book"] == c["book"]);
    v(
        "card",
        json!({"flex":1}),
        vec![
            json!({"kind":"scroll","key":format!("card-scroll-{}",key(c)),"style":{"flex":1},"children":[v("card-content",json!({"paddingHorizontal":if narrow{16}else{28},"paddingTop":24,"paddingBottom":24,"gap":20}),vec![heading,v("details",json!({}),rows)])]}),
            footer(
                a,
                vec![
                    button("groups", json!(["menu", "membership"]), !has_groups),
                    spacer(),
                    button("edit", json!(["edit"]), false),
                ],
            ),
        ],
    )
}
fn editor(a: &mut App, now: i64) -> Value {
    let d = a.state.draft.as_ref().unwrap();
    let c = d.value.clone();
    let busy = d.busy;
    let mut names = vec![];
    for (field, k) in [
        ("given", "given"),
        ("family", "family"),
        ("title", "jobTitle"),
        ("org", "company"),
    ] {
        names.push(input(
            field,
            if field == "given" || field == "family" {
                string(&c["n"][field])
            } else {
                string(&c[field])
            },
            label(k),
            json!([
                if field == "given" || field == "family" {
                    "name"
                } else {
                    "field"
                },
                field
            ]),
            field == "given" && c["_new"] == true,
            busy,
        ));
    }
    let photo_widget = click(
        v(
            "edit-photo",
            json!({"alignItems":"center","gap":4}),
            vec![
                photo(a, &c, 96.),
                text(
                    label(if d.photo.is_u64() {
                        "photoChosen"
                    } else {
                        "editPhoto"
                    }),
                    json!({"fontSize":11,"color":accent(a)}),
                ),
            ],
        ),
        if busy {
            json!(["noop"])
        } else {
            json!(["menu", "photo"])
        },
    );
    let mut children = vec![v(
        "heading",
        json!({"flexDirection":"row","alignItems":"flex-start","gap":18}),
        vec![photo_widget, v("names", json!({"flex":1,"gap":6}), names)],
    )];
    for kind in KINDS {
        let mut fields = vec![];
        for (i, r) in c[kind].as_array().into_iter().flatten().enumerate() {
            let mut row = vec![click(
                v(
                    "remove",
                    json!({"width":18,"height":18,"borderRadius":9,"alignItems":"center","justifyContent":"center","backgroundColor":"#ff3b30","marginTop":if kind=="addrs"{4}else{0}}),
                    vec![text(
                        json!("−"),
                        json!({"fontSize":13,"fontWeight":700,"color":"#ffffff"}),
                    )],
                ),
                if busy {
                    json!(["noop"])
                } else {
                    json!(["removeRow",{"kind":kind,"index":i}])
                },
            )];
            row.push(click(v(&format!("type-{kind}-{i}"),json!({"width":90,"height":26,"paddingHorizontal":7,"borderRadius":6,"flexDirection":"row","alignItems":"center","gap":4,"backgroundColor":token(a,"control","#3a3a3f")}),vec![text(type_label(kind,string(&r["type"])),json!({"flex":1,"fontSize":12,"color":fg(a),"whiteSpace":"nowrap","overflow":"hidden"})),text(json!("▾"),json!({"fontSize":10,"color":dim(a)}))]),if busy{json!(["noop"])}else{json!(["typeMenu",{"kind":kind,"index":i}])}));
            let columns: Vec<_> = if kind == "addrs" {
                vec![
                    vec![("street", "street")],
                    vec![("city", "city"), ("code", "postalCode")],
                    vec![("region", "region"), ("country", "country")],
                ]
            } else {
                vec![vec![("value", "")]]
            };
            let controls = columns
                .iter()
                .enumerate()
                .map(|(n, line)| {
                    v(
                        &format!("line-{n}"),
                        json!({"flexDirection":"row","gap":4}),
                        line.iter()
                            .map(|(field, k)| {
                                v(
                                    field,
                                    json!({"flex":1}),
                                    vec![input(
                                        &format!("{kind}-{i}-{field}"),
                                        string(&r[field]),
                                        if k.is_empty() {
                                            label(&format!("placeholder.{kind}"))
                                        } else {
                                            label(k)
                                        },
                                        json!(["row",{"kind":kind,"index":i,"field":field}]),
                                        false,
                                        busy,
                                    )],
                                )
                            })
                            .collect(),
                    )
                })
                .collect();
            row.push(v("values", json!({"flex":1,"gap":4}), controls));
            fields.push(v(&format!("row-{kind}-{i}"),json!({"flexDirection":"row","alignItems":if kind=="addrs"{"flex-start"}else{"center"},"gap":8}),row));
        }
        fields.push(button(
            &format!("add.{kind}"),
            json!(["addRow", kind]),
            busy,
        ));
        children.push(v(kind, json!({"gap":6}), fields));
    }
    children.push(if string(&c["bday"]).is_empty(){button("addBirthday",json!(["birthdayAdd"]),busy)}else{
        let birthday=a.birthday(string(&c["bday"]),true);
        v("birthday-row",json!({"flexDirection":"row","alignItems":"center","gap":8}),vec![button("delete",json!(["birthdayRemove"]),busy),text(label("birthday"),json!({"fontSize":12,"color":dim(a)})),click(v("birthday-value",json!({"height":26,"paddingHorizontal":9,"justifyContent":"center","borderRadius":6,"backgroundColor":token(a,"control","#3a3a3f")}),vec![text(json!(birthday),json!({"fontSize":13,"color":fg(a)}))]),if busy{json!(["noop"])}else{json!(["birthdayPicker"])})])
    });
    let notes = string(&c["notes"]);
    let caret = if a.state.notes_focus && (now - a.notes_since).max(0) / 530 % 2 == 0 {
        "▏"
    } else {
        ""
    };
    let note = click(
        v(
            "notes-input",
            json!({"minHeight":70,"padding":7,"borderRadius":7,"backgroundColor":token(a,"inputBg","#202024"),"borderWidth":1,"borderColor":if a.state.notes_focus{accent(a)}else{token(a,"separator","#555555")}}),
            vec![text(
                json!(format!("{notes}{caret}")),
                json!({"fontSize":13,"color":fg(a),"lineHeight":18}),
            )],
        ),
        json!(["noop"]),
    );
    let mut note = note;
    note["events"]["onMouseDown"] = if busy {
        json!(["noop"])
    } else {
        json!(["notesFocus"])
    };
    children.push(v(
        "notes",
        json!({"gap":4}),
        vec![
            text(
                label("notes"),
                json!({"fontSize":11,"fontWeight":600,"color":dim(a)}),
            ),
            note,
        ],
    ));
    let mut buttons = vec![];
    if !string(&c["uid"]).is_empty() {
        let mut b = button("deleteContact", json!(["delete", key(&c)]), busy);
        b["destructive"] = json!(true);
        buttons.push(b);
    }
    let err = &a.state.draft.as_ref().unwrap().error;
    buttons.push(text(
        if err.is_empty() {
            json!("")
        } else {
            json!({"key":err})
        },
        json!({"flex":1,"fontSize":12,"color":"#ff3b30","textAlign":"center"}),
    ));
    buttons.extend([
        button("cancel", json!(["cancel"]), false),
        button("done", json!(["save"]), busy),
    ]);
    v(
        "editor",
        json!({"flex":1}),
        vec![
            json!({"kind":"scroll","key":"editor-scroll","style":{"flex":1},"children":[v("editor-content",json!({"padding":24,"gap":16}),children)]}),
            footer(a, buttons),
        ],
    )
}
fn popup(
    a: &App,
    id: &str,
    anchor: &str,
    width: f64,
    children: Vec<Value>,
    dismiss: Value,
) -> Value {
    let mut box_node = click(
        v(
            id,
            json!({"position":"absolute","left":8,"top":8,"width":width,"padding":6,"borderRadius":10,"backgroundColor":if a.theme["mode"]=="dark"{"#2c2c31"}else{"#ffffff"},"borderWidth":1,"borderColor":token(a,"separator","#555555"),"boxShadow":"0 12px 32px rgba(0,0,0,0.32)"}),
            children,
        ),
        json!(["noop"]),
    );
    box_node["anchorKey"] = json!(anchor);
    box_node["fitPopover"] = json!(true);
    let mut bg = v(
        "dismiss",
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0}),
        vec![],
    );
    bg["events"]["onMouseDown"] = dismiss;
    v(
        &format!("{id}-layer"),
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":200}),
        vec![bg, box_node],
    )
}
fn menu_row(
    a: &App,
    k: &str,
    title: Value,
    action: Value,
    checked: bool,
    destructive: bool,
) -> Value {
    let mut row = click(
        v(
            k,
            json!({"height":26,"paddingHorizontal":8,"borderRadius":5,"flexDirection":"row","gap":6,"alignItems":"center"}),
            vec![
                text(
                    json!(if checked { "✓" } else { "" }),
                    json!({"width":12,"fontSize":11,"color":fg(a)}),
                ),
                text(
                    title,
                    json!({"flex":1,"fontSize":13,"whiteSpace":"nowrap","overflow":"hidden","color":if destructive{"#ff3b30".into()}else{fg(a)}}),
                ),
            ],
        ),
        action,
    );
    row["hoverStyle"] = json!({"backgroundColor":token(a,"hover","rgba(127,127,127,0.12)")});
    row
}
fn menu(a: &App) -> Value {
    let id = &a.state.menu;
    let mut rows = vec![];
    let anchor;
    if id == "photo" {
        anchor = "edit-photo".into();
        rows.push(menu_row(
            a,
            "choosePhoto",
            label("choosePhoto"),
            json!(["photoPick"]),
            false,
            false,
        ));
        rows.push(menu_row(
            a,
            "removePhoto",
            label("removePhoto"),
            json!(["photoRemove"]),
            false,
            true,
        ));
    } else if id == "membership" {
        anchor = "button-groups".into();
        if let Some(c) = a.state.selected_contact() {
            for g in groups(&a.state.contacts)
                .iter()
                .filter(|g| g["book"] == c["book"])
            {
                rows.push(menu_row(
                    a,
                    &key(g),
                    fallback_name(g),
                    json!(["groupToggle", key(g)]),
                    is_member(g, c),
                    false,
                ));
            }
        }
    } else if let Some(k) = id.strip_prefix("group:") {
        anchor = format!("source-group:{k}");
        rows.push(menu_row(
            a,
            "renameGroup",
            label("renameGroup"),
            json!(["groupEdit", k]),
            false,
            false,
        ));
        rows.push(rule(a));
        rows.push(menu_row(
            a,
            "deleteGroup",
            label("deleteGroup"),
            json!(["groupDelete", k]),
            false,
            true,
        ));
    } else {
        return Value::Null;
    }
    let menu_height = (rows
        .iter()
        .map(|r| if r["style"]["height"] == 1 { 1. } else { 26. })
        .sum::<f64>())
    .min((a.height - 40.).max(40.));
    popup(
        a,
        "menu",
        &anchor,
        220.,
        vec![v(
            "menu-height",
            json!({"height":menu_height}),
            vec![json!({"kind":"scroll","key":"menu-options","style":{"flex":1},"children":rows})],
        )],
        json!(["menu", ""]),
    )
}
fn picker(a: &mut App, now: i64) -> Value {
    if a.picker == "birthday" {
        return calendar(a, now);
    }
    let kind = string(&a.picker["kind"]);
    let index = a.picker["index"].as_u64().unwrap_or(u64::MAX) as usize;
    let Some(row) = a
        .state
        .draft
        .as_ref()
        .and_then(|d| d.value[kind].get(index))
    else {
        return Value::Null;
    };
    let current = field_type(kind, string(&row["type"]));
    let mut types: Vec<_> = field_types(kind).iter().map(|s| s.to_string()).collect();
    if !types.contains(&current) {
        types.push(current.clone());
    }
    let options = types
        .iter()
        .map(|ty| {
            menu_row(
                a,
                ty,
                if field_types(kind).contains(&ty.as_str()) {
                    label(&format!("type.{ty}"))
                } else {
                    json!(ty)
                },
                json!(["row",{"kind":kind,"index":index,"field":"type","value":ty}]),
                ty == &current,
                false,
            )
        })
        .collect();
    popup(
        a,
        "type-options",
        &format!("type-{kind}-{index}"),
        180.,
        options,
        json!(["closePicker"]),
    )
}
fn calendar(a: &mut App, now: i64) -> Value {
    let selected = a
        .state
        .draft
        .as_ref()
        .and_then(|d| date::parse(string(&d.value["bday"])))
        .unwrap_or(now);
    let tm = crate::clock::local(a.picker_month);
    let first = date::add_days(a.picker_month, -((tm.tm_wday + 6) % 7));
    let month = a
        .i18n
        .caption(&json!({"key":format!("calendar.month.{}",tm.tm_mon)}));
    let title = a.i18n.caption(
        &json!({"key":"calendar.monthYear","params":{"month":month,"year":tm.tm_year+1900}}),
    );
    let mut header = vec![text(
        json!(title),
        json!({"flex":1,"fontSize":12,"fontWeight":600,"color":fg(a),"whiteSpace":"nowrap","overflow":"hidden"}),
    )];
    for (delta, glyph) in [
        ("-1", if a.rtl { "›" } else { "‹" }),
        ("today", "●"),
        ("1", if a.rtl { "‹" } else { "›" }),
    ] {
        header.push(click(
            v(
                delta,
                json!({"width":20,"height":20,"alignItems":"center","justifyContent":"center"}),
                vec![text(
                    json!(glyph),
                    json!({"fontSize":if delta=="today"{7}else{15},"color":dim(a)}),
                )],
            ),
            json!(["month", delta]),
        ));
    }
    let lang = a.i18n.lang.clone();
    let weekdays = a
        .i18n
        .entry(&lang, "format", "weekdays")
        .or_else(|| a.i18n.entry("en", "format", "weekdays"))
        .unwrap_or(Value::Null);
    let mut children=vec![v("header",json!({"height":24,"flexDirection":"row","alignItems":"center"}),header),v("weekdays",json!({"flexDirection":"row"}),[1,2,3,4,5,6,0].iter().map(|i|text(json!(string(&weekdays[*i]).chars().next().map(|c|c.to_string()).unwrap_or_default()),json!({"width":28,"fontSize":10,"fontWeight":600,"textAlign":"center","color":dim(a)}))).collect())];
    for week in 0..6 {
        let mut cells = vec![];
        for col in 0..7 {
            let day = date::add_days(first, week * 7 + col);
            let d = crate::clock::local(day);
            let is_selected = date::ymd(day) == date::ymd(selected);
            let today = date::ymd(day) == date::ymd(now);
            cells.push(click(v(&format!("day-{}",date::ymd(day)),json!({"width":28,"height":28,"alignItems":"center","justifyContent":"center"}),vec![v("circle",json!({"width":24,"height":24,"borderRadius":12,"alignItems":"center","justifyContent":"center","backgroundColor":if is_selected{if today{"#ff3b30".into()}else{accent(a)}}else{"transparent".into()}}),vec![text(json!(d.tm_mday.to_string()),json!({"fontSize":11,"fontWeight":if today||is_selected{600}else{400},"color":if is_selected{"#ffffff".into()}else if today{"#ff3b30".into()}else if d.tm_mon==tm.tm_mon{fg(a)}else{dim(a)}}))])]),json!(["pickDay",day])));
        }
        children.push(v(
            &format!("week-{week}"),
            json!({"flexDirection":"row"}),
            cells,
        ));
    }
    popup(
        a,
        "birthday-calendar",
        "birthday-value",
        212.,
        children,
        json!(["closePicker"]),
    )
}
fn sheet(a: &App) -> Value {
    let s = &a.state.sheet;
    let busy = s["busy"] == true;
    let mut children = vec![];
    let mut buttons = vec![];
    if s.get("group").is_some() {
        children.push(text(
            label(if s["group"].is_object() {
                "renameGroup"
            } else {
                "newGroup"
            }),
            json!({"fontSize":15,"fontWeight":600,"color":fg(a)}),
        ));
        let mut name = input(
            "group-name",
            string(&s["name"]),
            label("groupName"),
            json!(["groupName"]),
            true,
            busy,
        );
        name["submit"] = json!(["groupSave"]);
        name["cancel"] = json!(["closeSheet"]);
        children.push(name);
        if !string(&s["error"]).is_empty() {
            children.push(text(
                json!({"key":s["error"]}),
                json!({"fontSize":12,"color":"#ff3b30"}),
            ));
        }
        buttons.extend([
            spacer(),
            button("cancel", json!(["closeSheet"]), false),
            button(
                "save",
                json!(["groupSave"]),
                busy || string(&s["name"]).trim().is_empty(),
            ),
        ]);
    } else if s["confirm"].is_object() {
        children.push(text(
            json!({"key":"contacts.deleteTitle","params":{"name":fallback_name(&s["confirm"])}}),
            json!({"fontSize":15,"fontWeight":600,"color":fg(a)}),
        ));
        children.push(text(
            label("deleteHint"),
            json!({"fontSize":13,"color":dim(a)}),
        ));
        let mut delete = button("delete", json!(["confirmDelete"]), busy);
        delete["destructive"] = json!(true);
        buttons.extend([
            spacer(),
            button("cancel", json!(["closeSheet"]), false),
            delete,
        ]);
    } else {
        let count = s["count"].as_u64().unwrap_or(0);
        children.push(text(
            json!({"key":s["notice"],"params":{"count":count,"n":a.i18n.number(count as f64,0)}}),
            json!({"fontSize":13,"color":fg(a)}),
        ));
        buttons.extend([spacer(), button("ok", json!(["closeSheet"]), false)]);
    }
    v(
        "sheet-layer",
        json!({"position":"absolute","left":0,"top":0,"right":0,"bottom":0,"zIndex":100,"alignItems":"center","justifyContent":"center","backgroundColor":"rgba(0,0,0,0.3)"}),
        vec![v(
            "sheet",
            json!({"width":360,"maxWidth":"94%","maxHeight":"94%","padding":20,"gap":16,"borderRadius":12,"backgroundColor":token(a,"windowBg","#252529"),"borderWidth":1,"borderColor":token(a,"separator","#555555"),"boxShadow":"0 18px 50px rgba(0,0,0,0.45)"}),
            vec![
                v("body", json!({"gap":12,"flexShrink":1}), children),
                v(
                    "actions",
                    json!({"flexDirection":"row","alignItems":"center","gap":8}),
                    buttons,
                ),
            ],
        )],
    )
}
pub fn tree(a: &mut App, now: i64) -> Value {
    let detail = if a.state.unavailable {
        empty(a, "unavailable", Some("unavailableHint"))
    } else if a.state.draft.is_some() {
        editor(a, now)
    } else if let Some(c) = a.state.selected_contact().cloned() {
        card(a, &c)
    } else {
        empty(a, "noSelection", None)
    };
    let mut children = vec![
        sources(a),
        contact_list(a),
        v("detail", json!({"flex":1,"minWidth":0}), vec![detail]),
    ];
    if !a.state.sheet.is_null() {
        children.push(sheet(a));
    }
    if !a.state.menu.is_empty() {
        children.push(menu(a));
    }
    if !a.picker.is_null() {
        children.push(picker(a, now));
    }
    v(
        "contacts",
        json!({"flex":1,"flexDirection":"row","direction":if a.rtl{"rtl"}else{"ltr"},"backgroundColor":token(a,"windowBg","#252529")}),
        children,
    )
}
