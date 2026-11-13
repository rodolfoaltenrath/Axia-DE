use super::{App, Family};
use serde_json::{json, Value};
fn v(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
fn text(label: Value, style: Value) -> Value {
    json!({"kind":"text","label":label,"style":style})
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn click(mut node: Value, action: Value) -> Value {
    node["events"]["onClick"] = action;
    node
}
fn button(label: Value, action: Value, primary: bool, disabled: bool) -> Value {
    json!({"kind":"button","key":action.to_string(),"label":label,"action":action,"primary":primary,"disabled":disabled})
}
fn dim(label: Value, t: &Value) -> Value {
    text(label, json!({"fontSize":12,"color":t["textDim"]}))
}
fn choice(cmd: &str, id: Value, label: Value, selected: bool, t: &Value) -> Value {
    click(
        v(
            &format!("{cmd}:{id}"),
            json!({"height":26,"paddingHorizontal":10,"borderRadius":6,"alignItems":"center","justifyContent":"center","backgroundColor":if selected{t["accent"].clone()}else{t["control"].clone()}}),
            vec![text(
                label,
                json!({"fontSize":12,"color":if selected{json!("#fff")}else{t["text"].clone()},"whiteSpace":"nowrap","overflow":"hidden"}),
            )],
        ),
        json!([cmd, id]),
    )
}
pub(super) fn icon(size: f64) -> Value {
    let s = if size.is_finite() {
        size.clamp(1., 512.)
    } else {
        32.
    };
    v(
        "fonts-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #f5f5f7, #c7c7cc)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden","alignItems":"center","justifyContent":"center"}),
        vec![
            text(
                json!("A"),
                json!({"position":"absolute","left":s*0.12,"top":s*0.06,"fontSize":s*0.62,"fontWeight":700,"color":"#1c1c1e"}),
            ),
            text(
                json!("a"),
                json!({"position":"absolute","left":s*0.52,"top":s*0.36,"fontSize":s*0.44,"color":"#d70015"}),
            ),
            v(
                "rule",
                json!({"position":"absolute","left":s*0.12,"right":s*0.12,"bottom":s*0.16,"height":1_f64.max(s*0.03),"backgroundColor":"#8e8e93"}),
                vec![],
            ),
        ],
    )
}
fn section(label: Value, t: &Value) -> Value {
    text(
        label,
        json!({"fontSize":11,"lineHeight":14,"whiteSpace":"nowrap","overflow":"hidden","fontWeight":600,"color":t["textDim"],"paddingHorizontal":10,"marginTop":8,"marginBottom":6}),
    )
}
fn family_row(f: &Family, on: bool, y: f64, t: &Value) -> Value {
    let mut children = vec![text(
        json!(f.name),
        json!({"flex":1,"fontSize":13,"color":if on{json!("#fff")}else{t["text"].clone()},"whiteSpace":"nowrap","overflow":"hidden"}),
    )];
    if f.file {
        children.push(text(
            key("fonts.new"),
            json!({"fontSize":10,"color":if on{json!("#fff")}else{t["accent"].clone()}}),
        ));
    } else if f.user {
        children.push(text(
            json!("●"),
            json!({"fontSize":10,"color":if on{json!("#fff")}else{t["textDim"].clone()}}),
        ));
    }
    click(
        v(
            &format!("{}:{}", f.file, f.name),
            json!({"position":"absolute","left":0,"right":0,"top":y,"height":26,"paddingHorizontal":10,"flexDirection":"row","alignItems":"center","gap":6,"borderRadius":6,"backgroundColor":if on{t["accent"].clone()}else{json!("transparent")}}),
            children,
        ),
        json!(["family", if f.file { "file" } else { &f.name }]),
    )
}
fn sidebar(app: &App) -> Value {
    let t = &app.theme;
    let visible = app.visible();
    let current = app.family();
    let mut children = vec![
        json!({"kind":"input","key":"search","value":app.query,"change":["search"],"placeholder":key("fonts.search"),"style":{"height":28,"marginBottom":6}}),
        section(key("fonts.collections"), t),
    ];
    for id in ["all", "user", "mono", "color"] {
        let mut node = choice(
            "collection",
            json!(id),
            key(&format!("fonts.coll.{id}")),
            app.collection == id,
            t,
        );
        node["style"]["justifyContent"] = json!("flex-start");
        node["style"]["alignItems"] = json!("flex-start");
        if app.collection != id {
            node["style"]["backgroundColor"] = json!("transparent");
        }
        children.push(node);
    }
    children.push(section(
        json!({"key":"fonts.families","params":{"count":visible.len()}}),
        t,
    ));
    let all: Vec<_> = app
        .file_font
        .iter()
        .chain(visible.iter().map(|i| &app.families[*i]))
        .collect();
    let from = (app.family_offset / 26.).floor() as usize;
    let count = (app.family_height() / 26.).ceil() as usize + 2;
    let rows = all
        .iter()
        .enumerate()
        .skip(from)
        .take(count)
        .map(|(i, f)| {
            family_row(
                f,
                current.is_some_and(|c| c.name == f.name && c.file == f.file),
                i as f64 * 26. - app.family_offset,
                t,
            )
        })
        .collect();
    let mut list = v("family-list", json!({"flex":1,"overflow":"hidden"}), rows);
    list["events"]["onWheel"] = json!(["familyWheel"]);
    children.push(list);
    children.push(v("separator",json!({"position":"absolute","right":0,"top":0,"bottom":0,"width":1,"backgroundColor":t["separator"]}),vec![]));
    v(
        "sidebar",
        json!({"width":230,"flexShrink":0,"backgroundColor":t["sidebarBg"],"paddingTop":12,"paddingHorizontal":10,"gap":2}),
        children,
    )
}
fn sample(app: &App) -> Value {
    let t = &app.theme;
    let mut slider = v(
        "size",
        json!({"width":120,"height":28,"justifyContent":"center"}),
        vec![
            v(
                "track",
                json!({"height":4,"borderRadius":2,"backgroundColor":t["separator"]}),
                vec![],
            ),
            v(
                "thumb",
                json!({"position":"absolute","left":(app.size-8.)/112.*120.-6.,"top":8,"width":12,"height":12,"borderRadius":6,"backgroundColor":t["accent"]}),
                vec![],
            ),
        ],
    );
    slider["events"]["onMouseDown"] = json!(["sizeDown"]);
    let controls = v(
        "sample-controls",
        json!({"height":28,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":12}),
        vec![
            json!({"kind":"input","key":"sample-text","value":app.text,"change":["sampleText"],"placeholder":key("fonts.typeHere"),"style":{"flex":1}}),
            v(
                "size-group",
                json!({"width":164,"flexDirection":"row","alignItems":"center","gap":8,"direction":"ltr"}),
                vec![
                    text(
                        json!("A"),
                        json!({"width":10,"fontSize":11,"color":t["textDim"]}),
                    ),
                    slider,
                    text(
                        json!("A"),
                        json!({"width":18,"fontSize":17,"color":t["textDim"]}),
                    ),
                ],
            ),
            text(
                json!(format!("{} px", app.size as u32)),
                json!({"width":48,"fontSize":12,"color":t["textDim"],"textAlign":"end"}),
            ),
        ],
    );
    let mut images = vec![];
    for slot in 0..2 {
        if slot == 1 {
            images.push(v(
                "rule",
                json!({"height":1,"backgroundColor":t["separator"],"marginVertical":14}),
                vec![],
            ));
        }
        if app.sample_error[slot] {
            images.push(text(
                key("fonts.cantRender"),
                json!({"fontSize":12,"color":"#ff453a"}),
            ));
        } else if let Some(image) = &app.samples[slot] {
            images.push(json!({"kind":"documentImage","key":format!("sample:{slot}"),"image":image.id,"style":{"width":image.w,"height":image.h,"flexShrink":0}}));
        } else {
            images.push(v(
                &format!("sample:{slot}"),
                json!({"height":app.size*1.4,"flexShrink":0}),
                vec![],
            ));
        }
    }
    v(
        "samples",
        json!({"flex":1,"gap":12}),
        vec![
            controls,
            v(
                "paper",
                json!({"flex":1,"borderRadius":10,"paddingVertical":14,"paddingHorizontal":16,"backgroundColor":if t["mode"]=="dark"{"#1c1c20"}else{"#ffffff"},"borderWidth":1,"borderColor":t["separator"]}),
                vec![
                    json!({"kind":"scroll","key":"sample-scroll","style":{"flex":1},"children":images}),
                ],
            ),
        ],
    )
}
fn chars(app: &App) -> Value {
    let t = &app.theme;
    if app.info.is_none() {
        return dim(json!("…"), t);
    }
    if app.info.as_ref().is_some_and(|info| info["error"] == true) {
        return text(
            key("fonts.invalid"),
            json!({"fontSize":12,"color":"#ff453a"}),
        );
    }
    let blocks = app.blocks();
    if blocks.is_empty() {
        return dim(key("fonts.noChars"), t);
    }
    let Some((_, a, b)) = app.current_block() else {
        return dim(key("fonts.noChars"), t);
    };
    let block_rows = blocks
        .iter()
        .enumerate()
        .map(|(i, idx)| {
            let name = key(&format!("fonts.block.{}", super::blocks::BLOCKS[*idx].0));
            let mut node = choice("block", json!(i), name.clone(), i == app.block, t);
            node["tooltip"] = name;
            node
        })
        .collect::<Vec<_>>();
    let block_list = json!({"kind":"scroll","key":"blocks","style":{"width":app.block_width(),"flexShrink":0},"children":block_rows});
    let pages = ((b - a) as usize) / 256 + 1;
    let mut head = vec![text(
        json!(format!("U+{a:04X}–U+{b:04X}")),
        json!({"flex":1,"fontSize":11,"color":t["textDim"],"whiteSpace":"nowrap","overflow":"hidden"}),
    )];
    if pages > 1 {
        head.extend([
            dim(json!(format!("{}/{pages}", app.chunk + 1)), t),
            button(json!("‹"), json!(["chunk", "prev"]), false, app.chunk == 0),
            button(
                json!("›"),
                json!(["chunk", "next"]),
                false,
                app.chunk + 1 >= pages,
            ),
        ]);
    }
    let mut grid_children = vec![];
    let cps = app.codepoints();
    let cell = app.cell();
    let height = if let Some(image) = &app.grid {
        grid_children.push(json!({"kind":"documentImage","key":"glyph-raster","image":image.id,"style":{"width":image.w,"height":image.h}}));
        for (i, cp) in cps.iter().enumerate() {
            if image.present.get(i) == Some(&true) {
                grid_children.push(click(v(&format!("glyph:{cp}"),json!({"position":"absolute","left":(i%16) as f64*cell,"top":(i/16) as f64*cell,"width":cell,"height":cell}),vec![]),json!(["glyph",i])));
            }
        }
        image.h
    } else {
        200.
    };
    let grid = v(
        "grid",
        json!({"width":cell*16.,"height":height,"borderRadius":8,"backgroundColor":if t["mode"]=="dark"{"#1c1c20"}else{"#ffffff"},"borderWidth":1,"borderColor":t["separator"],"direction":"ltr","flexShrink":0}),
        grid_children,
    );
    let mut content = vec![
        v(
            "char-header",
            json!({"flexDirection":"row","alignItems":"center","gap":8}),
            head,
        ),
        json!({"kind":"scroll","key":"grid-scroll","style":{"flex":1},"children":[grid]}),
    ];
    if let Some(cp) = app.pick {
        content.push(v("picked",json!({"flexDirection":"row","alignItems":"center","gap":12,"paddingVertical":8,"paddingHorizontal":12,"borderRadius":10,"backgroundColor":t["control"]}),vec![
        text(json!(char::from_u32(cp).unwrap_or('�').to_string()),json!({"fontSize":28,"color":t["text"],"minWidth":40,"textAlign":"center"})),
        text(json!(format!("U+{cp:04X}")),json!({"flex":1,"fontSize":12,"color":t["textDim"]})),button(key("fonts.copy"),json!(["copy"]),false,false)
    ]));
    }
    v(
        "characters",
        json!({"flex":1,"flexDirection":"row","gap":16}),
        vec![
            block_list,
            v(
                "character-pane",
                json!({"flex":1,"gap":8,"minWidth":0}),
                content,
            ),
        ],
    )
}
fn info(app: &App) -> Value {
    let t = &app.theme;
    let Some(info) = &app.info else {
        return v("loading-info", json!({"flex":1}), vec![]);
    };
    if info["error"] == true {
        return text(
            key("fonts.invalid"),
            json!({"fontSize":12,"color":"#ff453a"}),
        );
    }
    let mut kinds = vec![];
    if info["mono"] == true {
        kinds.push(key("fonts.coll.mono"));
    }
    if info["color"] == true {
        kinds.push(key("fonts.coll.color"));
    }
    kinds.push(key(if info["scalable"] == true {
        "fonts.scalable"
    } else {
        "fonts.bitmap"
    }));
    let parts: Vec<Value> = kinds
        .into_iter()
        .enumerate()
        .flat_map(|(i, k)| {
            if i == 0 {
                vec![k]
            } else {
                vec![json!(" · "), k]
            }
        })
        .collect();
    let fields = [
        ("family", info["family"].clone()),
        ("style", info["style"].clone()),
        ("ps", info["ps"].clone()),
        ("glyphs", info["glyphs"].clone()),
        ("chars", info["chars"].clone()),
        ("kind", json!({"parts":parts})),
        (
            "file",
            app.font.as_ref().unwrap_or(&Value::Null)["path"].clone(),
        ),
        (
            "size",
            app.bytes
                .map(|size| json!({"size":size}))
                .unwrap_or_else(|| json!("—")),
        ),
    ];
    let rows=fields.into_iter().map(|(name,value)|v(name,json!({"minHeight":32,"paddingHorizontal":12,"paddingVertical":6,"gap":12,"flexDirection":"row","alignItems":"center"}),vec![
        text(key(&format!("fonts.info.{name}")),json!({"width":150,"fontSize":13,"color":t["textDim"]})),text(value,json!({"flex":1,"fontSize":13,"color":t["text"]}))
    ])).collect();
    json!({"kind":"scroll","key":"info-scroll","style":{"flex":1},"children":[v("metadata",json!({"borderRadius":10,"borderWidth":1,"borderColor":t["separator"]}),rows)]})
}
pub(super) fn tree(app: &App) -> Value {
    let t = &app.theme;
    let mut body = vec![];
    let busy = app.install.is_some() || app.removal.is_some();
    if let Some(family) = app.family() {
        let mut header = vec![v(
            "title",
            json!({"flex":1,"gap":2,"overflow":"hidden"}),
            vec![
                text(
                    json!(family.name),
                    json!({"fontSize":22,"fontWeight":600,"color":t["text"],"whiteSpace":"nowrap"}),
                ),
                dim(
                    if family.file {
                        key("fonts.notInstalled")
                    } else {
                        json!({"key":"fonts.styles","params":{"count":family.styles.len()}})
                    },
                    t,
                ),
            ],
        )];
        if family.file {
            if app.already_installed() {
                header.push(dim(key("fonts.already"), t));
            } else {
                header.push(button(key("fonts.install"), json!(["install"]), true, busy));
            }
        } else if family.user {
            header.push(button(key("fonts.remove"), json!(["remove"]), false, busy));
        }
        body.push(v(
            "header",
            json!({"flexDirection":"row","alignItems":"center","gap":12}),
            header,
        ));
        if family.styles.len() > 1 {
            body.push(v(
                "styles",
                json!({"flexDirection":"row","flexWrap":"wrap","gap":6}),
                family
                    .styles
                    .iter()
                    .enumerate()
                    .map(|(i, f)| choice("style", json!(i), f["style"].clone(), i == app.style, t))
                    .collect(),
            ));
        }
        body.push(v("tabs",json!({"flexDirection":"row","alignSelf":"center","gap":2,"padding":2,"borderRadius":7,"backgroundColor":t["control"]}),["sample","chars","info"].iter().map(|tab|choice("tab",json!(tab),key(&format!("fonts.tab.{tab}")),app.tab==*tab,t)).collect()));
        body.push(match app.tab.as_str() {
            "chars" => chars(app),
            "info" => info(app),
            _ => sample(app),
        });
    } else {
        body.push(v(
            "empty",
            json!({"flex":1,"alignItems":"center","justifyContent":"center","gap":12}),
            vec![
                text(json!("A"), json!({"fontSize":48,"color":t["textDim"]})),
                text(
                    key("fonts.none"),
                    json!({"fontSize":15,"color":t["textDim"]}),
                ),
            ],
        ));
    }
    if !app.notice.is_null() {
        body.push(text(
            app.notice.clone(),
            json!({"fontSize":12,"color":t["accent"]}),
        ));
    }
    v(
        "fonts",
        json!({"flex":1,"flexDirection":"row"}),
        vec![
            sidebar(app),
            v(
                "pane",
                json!({"flex":1,"paddingVertical":20,"paddingHorizontal":24,"gap":14,"minWidth":0}),
                body,
            ),
        ],
    )
}
