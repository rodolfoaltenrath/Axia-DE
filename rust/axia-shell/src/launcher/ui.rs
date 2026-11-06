//! Widget descriptions for both menu materials. No React implementation of
//! these layouts or policies is retained; the adapter renders this vocabulary.
use super::{session::Menu, *};

fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn key(key: &str) -> Value {
    json!({"key":key})
}
fn events(mut node: Value, events: Value) -> Value {
    node["events"] = events;
    node
}
fn icon(id: &str, size: f64) -> Value {
    json!({"kind":"icon","app":id,"size":size})
}
fn token(t: &Value, k: &str, fallback: &str) -> Value {
    t.get(k)
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| json!(fallback))
}
fn app<'a>(m: &'a AxiaLauncher, id: &str) -> Option<&'a Value> {
    m.entries
        .iter()
        .find(|a| string(&a.metadata, "id") == id)
        .map(|a| &a.metadata)
}
fn popup_item(label: &str, action: Value) -> Value {
    json!({"label":key(label),"action":action})
}

fn scroll_node(m: &Menu, model: &AxiaLauncher, region: &str, style: Value) -> Value {
    json!({"kind":"scroll","region":region,"key":format!("{region}:{}:{}",m.scroll_generation,model.generation),"style":style})
}

fn glyph(name: &str, size: f64, color: Value, bg: Value, rtl: bool) -> Value {
    let abs = |style: Value| {
        let mut s = style;
        s["position"] = json!("absolute");
        view(s, vec![])
    };
    let children=match name {
        "power"=>{let k=size/14.;vec![
            abs(json!({"left":k,"top":2.*k,"width":12.*k,"height":12.*k,"borderRadius":6.*k,"border":format!("{}px solid {}",1.6*k,color.as_str().unwrap_or("#fff"))})),
            abs(json!({"left":4.*k,"top":0,"width":6.*k,"height":5.*k,"backgroundColor":bg})),
            abs(json!({"left":6.2*k,"top":0,"width":1.6*k,"height":7.*k,"borderRadius":1,"backgroundColor":color}))]},
        "chevron"=>return view(json!({"width":size*1.6,"height":size*1.6,"alignItems":"center","justifyContent":"center"}),vec![view(json!({"width":size,"height":size,"borderRightWidth":1.5,"borderBottomWidth":1.5,"borderColor":color,"transform":if rtl {"rotate(-45deg)"} else {"rotate(135deg)"}}),vec![])]),
        "folder"=>vec![abs(json!({"left":size*0.06,"top":size*0.16,"width":size*0.4,"height":size*0.2,"borderTopLeftRadius":2,"borderTopRightRadius":2,"backgroundColor":color,"opacity":0.8})),abs(json!({"left":size*0.06,"top":size*0.26,"width":size*0.88,"height":size*0.6,"borderRadius":2,"backgroundColor":color}))],
        "gear"=>{let mut v=Vec::new();for deg in [0,45,90,135] {v.push(abs(json!({"left":size*0.41,"top":0,"width":size*0.18,"height":size,"borderRadius":1,"backgroundColor":color,"transform":format!("rotate({deg}deg)")})));}
            v.push(abs(json!({"left":size*0.16,"top":size*0.16,"width":size*0.68,"height":size*0.68,"borderRadius":size,"backgroundColor":color})));
            v.push(abs(json!({"left":size*0.34,"top":size*0.34,"width":size*0.32,"height":size*0.32,"borderRadius":size,"backgroundColor":bg})));v},
        "page"=>return view(json!({"width":size,"height":size,"alignItems":"center"}),vec![view(json!({"width":size*0.66,"height":size*0.86,"marginTop":size*0.07,"borderRadius":2,"border":format!("1.4px solid {}",color.as_str().unwrap_or("#fff")),"paddingTop":size*0.18,"paddingHorizontal":size*0.12,"gap":size*0.1}),[1.,0.8,0.9].into_iter().map(|k|view(json!({"height":1.2,"width":format!("{}%",k*100.),"backgroundColor":color,"opacity":0.7}),vec![])).collect())]),
        _=>{let r=size*0.62;vec![abs(json!({"left":0,"top":0,"width":r,"height":r,"borderRadius":r/2.,"border":format!("{}px solid {}",1.4_f64.max(size/10.),color.as_str().unwrap_or("#fff"))})),abs(json!({"left":r*0.82,"top":r*0.98,"width":size*0.42,"height":1.6_f64.max(size/9.),"borderRadius":1,"backgroundColor":color,"transform":"rotate(45deg)","transformOrigin":"left top"}))]}
    };
    view(json!({"width":size,"height":size}), children)
}

fn header(t: &Value, title: &str, action: Option<(&str, &str)>, back: bool, rtl: bool) -> Value {
    let mut kids = vec![text(
        key(title),
        json!({"flex":1,"fontSize":13,"fontWeight":600,"color":token(t,"text","#fff"),"textAlign":"start"}),
    )];
    if let Some((label, action)) = action {
        let mut c = vec![];
        if back {
            c.push(glyph(
                "chevron",
                5.,
                token(t, "text", "#fff"),
                Value::Null,
                rtl,
            ));
        }
        c.push(text(
            key(label),
            json!({"fontSize":12,"color":token(t,"text","#fff")}),
        ));
        if !back {
            c.push(glyph(
                "chevron",
                5.,
                token(t, "text", "#fff"),
                Value::Null,
                !rtl,
            ));
        }
        kids.push(events(view(json!({"flexDirection":"row","alignItems":"center","gap":4,"height":26,"paddingHorizontal":10,"borderRadius":4,"backgroundColor":token(t,"control","#333"),"border":format!("1px solid {}",string(t,"tileBorder"))}),c),json!({"onClick":[action]})));
    }
    view(
        json!({"flexDirection":"row","alignItems":"center","height":32,"paddingHorizontal":8}),
        kids,
    )
}

fn row(m: &Menu, t: &Value, a: &Value, result: Option<(&Value, usize, &str)>) -> Value {
    let id = string(a, "id");
    let selected = result.is_some_and(|(_, i, _)| i == m.selected);
    let hover = m.hover.as_deref() == Some(id);
    let mut children = Vec::new();
    if selected {
        children.push(view(json!({"position":"absolute","insetInlineStart":0,"top":14,"width":3,"height":16,"borderRadius":2,"backgroundColor":token(t,"accent","#3489bd")}),vec![]));
    }
    let (image, title, subtitle, ev) = if let Some((it, i, section)) = result {
        let image = if truthy(&it["hasIcon"]) {
            json!({"kind":"resultIcon","result":it["key"],"size":28})
        } else if truthy(&it["hasImage"]) {
            Value::Null
        } else if section == "files" {
            let folder = truthy(&it["subtitle"]) && string(it, "glyph") == "📁";
            glyph(
                if folder { "folder" } else { "page" },
                22.,
                token(t, if folder { "accent" } else { "textDim" }, "#aaa"),
                Value::Null,
                false,
            )
        } else if section == "settings" {
            glyph(
                "gear",
                18.,
                token(t, "textDim", "#aaa"),
                if string(t, "panelBg").starts_with("rgba") {
                    json!(if string(t, "mode") == "dark" {
                        "#26262b"
                    } else {
                        "#f6f6f8"
                    })
                } else {
                    t["panelBg"].clone()
                },
                false,
            )
        } else if section == "notes" {
            glyph(
                "page",
                22.,
                token(t, "accent", "#3489bd"),
                Value::Null,
                false,
            )
        } else if ["=", "⇄", "⏎"].contains(&string(it, "glyph")) {
            text(
                it["glyph"].clone(),
                json!({"fontSize":18,"color":token(t,"accent","#3489bd")}),
            )
        } else {
            glyph(
                "search",
                16.,
                token(t, "textDim", "#aaa"),
                Value::Null,
                false,
            )
        };
        let mut ev = json!({"onClick":["run",it["key"]],"onMouseEnter":["select",i]});
        if let Some(app) = it["app"].as_str() {
            ev["onContextMenu"] = json!(["context", app]);
        }
        (image, it["title"].clone(), it["subtitle"].clone(), ev)
    } else {
        (
            icon(id, 28.),
            a["name"].clone(),
            Value::Null,
            json!({"onClick":["open",id],"onContextMenu":["context",id],"onMouseDown":["drag",id],"onMouseEnter":["hover",id,true],"onMouseLeave":["hover",id,false]}),
        )
    };
    children.push(view(
        json!({"width":28,"height":28,"alignItems":"center","justifyContent":"center"}),
        vec![image],
    ));
    let mut captions = vec![text(
        title,
        json!({"fontSize":13,"color":token(t,"text","#fff"),"whiteSpace":"nowrap","textOverflow":"ellipsis","textAlign":"start"}),
    )];
    let subtitle = if result.is_none() && truthy(&a["recent"]) {
        key("shell.classic.recentHint")
    } else {
        subtitle
    };
    if truthy(&subtitle) {
        captions.push(text(subtitle,json!({"fontSize":11,"color":token(t,"textDim","#aaa"),"whiteSpace":"nowrap","textOverflow":"ellipsis","textAlign":"start"})));
    }
    children.push(view(json!({"flex":1,"minWidth":0}), captions));
    let mut node = events(
        view(
            json!({"height":44,"flexDirection":"row","alignItems":"center","gap":12,"paddingHorizontal":10,"borderRadius":6,"backgroundColor":if selected||hover {token(t,"hover","#444")} else {json!("transparent")}}),
            children,
        ),
        ev,
    );
    node["app"] = json!(id);
    node
}

fn cell(m: &Menu, t: &Value, a: &Value, flow: bool) -> Value {
    let id = string(a, "id");
    let style = if flow {
        json!({"width":130,"alignItems":"center","gap":10})
    } else {
        json!({"width":92,"height":82,"alignItems":"center","justifyContent":"center","gap":6,"borderRadius":6,"paddingHorizontal":4,"backgroundColor":if m.hover.as_deref()==Some(id){token(t,"hover","#444")}else{json!("transparent")}})
    };
    let label_style = if flow {
        json!({"fontSize":13,"fontWeight":500,"color":"#fff","whiteSpace":"nowrap","maxWidth":120,"overflow":"hidden"})
    } else {
        json!({"fontSize":12,"color":token(t,"text","#fff"),"textAlign":"center","numberOfLines":2,"maxWidth":84})
    };
    let mut handlers =
        json!({"onClick":["open",id],"onMouseDown":["drag",id],"onContextMenu":["context",id]});
    if !flow {
        handlers["onMouseEnter"] = json!(["hover", id, true]);
        handlers["onMouseLeave"] = json!(["hover", id, false]);
    }
    let label = if flow {
        a.get("displayName").unwrap_or(&a["name"])
    } else {
        &a["name"]
    };
    let mut node = events(
        view(
            style,
            vec![
                icon(id, if flow { 72. } else { 32. }),
                text(label.clone(), label_style),
            ],
        ),
        handlers,
    );
    node["key"] = json!(id);
    node["app"] = json!(id);
    node
}

pub(super) fn scroll(m: &mut Menu, model: &mut AxiaLauncher, region: &str) -> Value {
    let t = m.context["theme"].clone();
    let fallback = if m.classic {
        number(&m.context["area"], "height", 700., 1., 16384.) - 168.
    } else {
        number(&m.context["area"], "h", 700., 1., 16384.) - 190.
    };
    let (offset, height) = m
        .offsets
        .get(region)
        .copied()
        .unwrap_or((0., fallback.max(1.)));
    if region == "grid" || region == "all" {
        let plan=model.view(&json!({"layout":if region=="grid" {"grid"} else {"list"},"query":if region=="grid" {m.query.as_str()}else{""},"category":if region=="grid" {m.category.as_str()}else{""},"offset":offset,"height":height})).unwrap();
        let mut children = Vec::new();
        for r in plan["items"].as_array().unwrap() {
            if region == "grid" {
                let kids = r["indices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| {
                        cell(
                            m,
                            &t,
                            &model.entries[i.as_u64().unwrap() as usize].metadata,
                            true,
                        )
                    })
                    .collect();
                let mut node = view(
                    json!({"position":"absolute","top":r["top"],"left":0,"width":"100%","height":98,"flexDirection":"row"}),
                    kids,
                );
                node["key"] = r["row"].clone();
                children.push(node);
            } else {
                let a = &model.entries[r["index"].as_u64().unwrap() as usize].metadata;
                let mut kids = vec![];
                if r["heading"].is_string() {
                    kids.push(text(r["heading"].clone(),json!({"fontSize":12,"fontWeight":600,"color":token(&t,"accent","#3489bd"),"marginHorizontal":12,"marginTop":8,"marginBottom":2,"textAlign":"start"})));
                }
                kids.push(row(m, &t, a, None));
                let mut node = view(
                    json!({"position":"absolute","top":r["top"],"left":0,"width":"100%","height":r["height"]}),
                    kids,
                );
                node["key"] = a["id"].clone();
                children.push(node);
            }
        }
        if plan["total"] == 0 && region == "grid" {
            children.push(text(key("shell.launchpad.empty"),json!({"marginTop":16,"fontSize":14,"color":"rgba(255,255,255,0.7)","textAlign":"center"})));
        }
        let tree = view(json!({"height":plan["height"]}), children);
        m.plan = plan;
        return tree;
    }
    if region == "pins" {
        let pins = m.pins(model);
        let rows = pins.len().div_ceil(6);
        let content = rows as f64 * 82.;
        let offset = offset.min((content - height).max(0.));
        let start = ((offset / 82.).floor() as usize).saturating_sub(1);
        let end = ((offset + height) / 82.).ceil() as usize + 1;
        let mut kids = vec![];
        for r in start..end.min(rows) {
            let children = pins[r * 6..((r + 1) * 6).min(pins.len())]
                .iter()
                .filter_map(|id| app(model, id))
                .map(|a| cell(m, &t, a, false))
                .collect();
            let mut node = view(
                json!({"position":"absolute","top":r as f64*82.,"left":2,"width":"100%","height":82,"flexDirection":"row"}),
                children,
            );
            node["key"] = json!(r);
            kids.push(node);
        }
        return view(json!({"height":content}), kids);
    }
    let mut kids = Vec::new();
    let mut top = 0.;
    let mut index = 0;
    for section in &m.results {
        let items = section["items"].as_array().unwrap();
        let section_height = 22. + items.len() as f64 * 44. + 6.;
        if top + section_height >= offset - 44. && top <= offset + height + 44. {
            kids.push(view(json!({"position":"absolute","top":top,"left":0,"width":"100%"}),vec![text(section["title"].clone(),json!({"fontSize":11,"fontWeight":600,"color":token(&t,"textDim","#aaa"),"marginHorizontal":10,"marginTop":6,"marginBottom":2,"textAlign":"start"}))]));
            for (i, it) in items.iter().enumerate() {
                let row_top = top + 22. + i as f64 * 44.;
                if row_top + 44. >= offset - 44. && row_top <= offset + height + 44. {
                    let a = it["app"]
                        .as_str()
                        .and_then(|id| app(model, id))
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let mut node = view(
                        json!({"position":"absolute","top":row_top,"left":0,"width":"100%","height":44}),
                        vec![row(m, &t, &a, Some((it, index + i, string(section, "id"))))],
                    );
                    node["key"] = it["key"].clone();
                    kids.push(node);
                }
            }
        }
        index += items.len();
        top += section_height;
    }
    if index == 0 {
        kids.push(view(json!({"height":(height-40.).max(1.),"alignItems":"center","justifyContent":"center","gap":8}),vec![glyph("search",32.,token(&t,"textDim","#aaa"),Value::Null,false),text(json!({"key":"shell.classic.noResults","params":{"query":unicode::query(&m.query)}}),json!({"fontSize":13,"color":token(&t,"textDim","#aaa")}))]));
        top = height;
    }
    view(json!({"height":top}), kids)
}

pub(super) fn render(m: &mut Menu, model: &mut AxiaLauncher) -> Value {
    let t = m.context["theme"].clone();
    let area = m.context["area"].clone();
    let rtl = truthy(&m.context["rtl"]);
    let q = unicode::query(&m.query);
    m.plan=model.view(&json!({"query":if m.classic {""} else {q.as_str()},"category":if m.classic {""}else{m.category.as_str()},"height":1})).unwrap();
    let mut children = Vec::new();
    let mut input = json!({"kind":"input","search":m.classic,"value":m.query,"placeholder":key(if m.classic {"shell.classic.searchHint"}else{"shell.launchpad.search"}),"autoFocus":true});
    input["style"] = if m.classic {
        json!({"height":34})
    } else {
        json!({"backgroundColor":"rgba(255,255,255,0.14)","border":"1px solid rgba(255,255,255,0.22)"})
    };
    children.push(events(
        view(
            if m.classic {
                json!({"height":72,"paddingHorizontal":28,"justifyContent":"center"})
            } else {
                json!({"marginTop":48,"width":260})
            },
            vec![input],
        ),
        json!({"onClick":["noop"]}),
    ));
    if !m.classic {
        if q.is_empty() {
            let mut cats = vec![String::new()];
            cats.extend(model.categories.iter().map(|s| s.to_string()));
            children.push(events(view(json!({"marginTop":22,"flexDirection":"row","gap":6}),cats.iter().map(|c|events(view(json!({"paddingHorizontal":12,"paddingVertical":5,"borderRadius":14,"backgroundColor":if c==&m.category {"rgba(255,255,255,0.32)"}else{"rgba(255,255,255,0.1)"}}),vec![text(key(&if c.is_empty(){"shell.category.all".into()}else{format!("shell.category.{c}")}),json!({"fontSize":12,"fontWeight":600,"color":"#fff"}))]),json!({"onClick":["category",c]}))).collect()),json!({"onClick":["noop"]})));
        }
        children.push(scroll_node(m,model,"grid",json!({"marginTop":if q.is_empty(){14}else{40},"height":number(&area,"h",700.,1.,16384.)-190.,"width":780_f64.min(number(&area,"w",1000.,1.,16384.)-20.)})));
    } else {
        let body_height = (number(&area, "height", 700., 1., 16384.) - 136.).max(1.);
        let body = if !q.is_empty() {
            scroll_node(m, model, "search", json!({"height":body_height}))
        } else if m.all {
            view(
                json!({"height":body_height}),
                vec![
                    header(
                        &t,
                        "shell.classic.allApps",
                        Some(("shell.classic.back", "home")),
                        true,
                        rtl,
                    ),
                    scroll_node(m, model, "all", json!({"flex":1})),
                ],
            )
        } else {
            let pins = m.pins(model);
            let recent = m.recent(model);
            let grid_height = pins
                .len()
                .div_ceil(6)
                .min(if recent.is_empty() { 5 } else { 3 }) as f64
                * 82.;
            let mut kids = vec![header(
                &t,
                "shell.classic.pinned",
                Some(("shell.classic.allApps", "all")),
                false,
                rtl,
            )];
            if pins.is_empty() {
                kids.push(text(key("shell.classic.noPins"),json!({"fontSize":12,"color":token(&t,"textDim","#aaa"),"margin":12,"textAlign":"start"})));
            } else {
                kids.push(scroll_node(
                    m,
                    model,
                    "pins",
                    json!({"height":grid_height,"flex":0}),
                ));
            }
            if !recent.is_empty() {
                let rows = recent
                    .iter()
                    .filter_map(|id| app(model, id))
                    .map(|a| {
                        let mut a = a.clone();
                        a["recent"] = json!(true);
                        view(json!({"width":"50%"}), vec![row(m, &t, &a, None)])
                    })
                    .collect();
                kids.push(view(
                    json!({"marginTop":10}),
                    vec![
                        header(&t, "shell.classic.recent", None, false, rtl),
                        view(json!({"flexDirection":"row","flexWrap":"wrap"}), rows),
                    ],
                ));
            }
            view(json!({"height":body_height,"gap":4}), kids)
        };
        children.push(view(json!({"flex":1,"paddingHorizontal":22}), vec![body]));
        let name = if !string(&m.context["info"], "fullName").is_empty() {
            string(&m.context["info"], "fullName")
        } else {
            string(&m.context["info"], "user")
        };
        let initials = name
            .split_whitespace()
            .take(2)
            .filter_map(|s| s.chars().next())
            .flat_map(char::to_uppercase)
            .collect::<String>();
        let user = events(
            view(
                json!({"flexDirection":"row","alignItems":"center","gap":10,"paddingHorizontal":8,"height":40,"borderRadius":6}),
                vec![
                    view(
                        json!({"width":32,"height":32,"borderRadius":16,"alignItems":"center","justifyContent":"center","backgroundColor":token(&t,"accent","#3489bd")}),
                        vec![text(
                            json!(if initials.is_empty() { "?" } else { &initials }),
                            json!({"fontSize":13,"fontWeight":600,"color":"#fff"}),
                        )],
                    ),
                    text(
                        json!(name),
                        json!({"fontSize":13,"color":token(&t,"text","#fff"),"whiteSpace":"nowrap"}),
                    ),
                ],
            ),
            json!({"onClick":["user"]}),
        );
        let dark = string(&t, "mode") == "dark";
        let bg = if m.power.is_some() {
            if dark {
                "#2e2e33"
            } else {
                "#e6e6ea"
            }
        } else if dark {
            "#232327"
        } else {
            "#efeff2"
        };
        let power = events(
            view(
                json!({"width":40,"height":40,"borderRadius":6,"alignItems":"center","justifyContent":"center","backgroundColor":if m.power.is_some(){token(&t,"hover","#444")}else{json!("transparent")}}),
                vec![glyph(
                    "power",
                    16.,
                    token(&t, "text", "#fff"),
                    json!(bg),
                    false,
                )],
            ),
            json!({"onClick":["powerMenu"]}),
        );
        children.push(view(json!({"height":64,"flexDirection":"row","alignItems":"center","paddingHorizontal":28,"gap":10,"backgroundColor":token(&t,"panelFoot","#202024"),"borderTopWidth":1,"borderColor":token(&t,"separator","#444")}),vec![user,view(json!({"flex":1}),vec![]),power]));
    }
    if let Some(p) = &m.popup {
        let id = string(p, "id");
        let d = &p["dock"];
        let mut items = vec![popup_item("shell.dock.open", json!(["open", id]))];
        if m.classic {
            items.push(json!("-"));
            items.push(popup_item(
                if m.pins(model).iter().any(|x| x == id) {
                    "shell.classic.unpinStart"
                } else {
                    "shell.classic.pinStart"
                },
                json!(["pinStart", id]),
            ));
        }
        if truthy(&d["id"]) {
            let mut it = popup_item(
                if m.classic {
                    if truthy(&d["pinned"]) {
                        "shell.dock.unpin"
                    } else {
                        "shell.dock.pin"
                    }
                } else {
                    "shell.dock.keep"
                },
                json!(["dock", id]),
            );
            it["disabled"] = json!(truthy(&d["pinned"]) && !truthy(&d["removable"]));
            if !m.classic {
                it["checked"] = d["pinned"].clone();
            }
            items.push(it);
        }
        children.push(json!({"kind":"menu","key":"context","x":p["x"],"y":p["y"],"items":items,"close":["closeContext"]}));
    }
    if let Some(p) = &m.power {
        let mut items = vec![];
        if truthy(&m.context["session"]["managed"]) {
            items.extend([
                popup_item("shell.menu.lock", json!(["session", "lock"])),
                popup_item("shell.menu.switchUser", json!(["session", "switchUser"])),
            ]);
        }
        items.push(popup_item("shell.menu.logout", json!(["session", "quit"])));
        if truthy(&m.context["session"]["canPower"]) {
            items.extend([
                json!("-"),
                popup_item("shell.menu.restart", json!(["session", "restart"])),
                popup_item("shell.menu.shutdown", json!(["session", "shutdown"])),
            ]);
        }
        children.push(json!({"kind":"menu","key":"power","x":p["x"],"y":p["y"],"width":220,"above":true,"items":items,"close":["closePower"]}));
    }
    let style = if m.classic {
        let mut s = area;
        s["position"] = json!("absolute");
        s["borderRadius"] = json!(10);
        s["overflow"] = json!("hidden");
        s["backgroundColor"] = token(&t, "panelBg", "#252529");
        if t["blur"].as_f64().is_some_and(|v| v > 0.) {
            s["backdropFilter"] = json!(format!("blur({}px)", t["blur"]));
        }
        s["border"] = json!(format!("1px solid {}", string(&t, "windowBorder")));
        s["boxShadow"] = t["shadow"].clone();
        s
    } else {
        json!({"position":"absolute","left":area["x"],"top":area["y"],"width":area["w"],"height":area["h"],"alignItems":"center","backgroundColor":if truthy(&t["effects"]){"rgba(10,10,16,0.35)"}else{"rgba(18,18,24,0.94)"},"backdropFilter":if truthy(&t["effects"]){"blur(32px)"}else{"blur(0px)"}})
    };
    let mut root = view(style, children);
    root["root"] = json!(true);
    if !m.classic {
        root["events"] = json!({"onClick":["close"]});
    }
    root
}
