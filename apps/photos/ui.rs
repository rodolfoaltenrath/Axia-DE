use super::App;
use serde_json::{json, Value};
pub fn month(seconds: i64) -> (i32, u32) {
    unsafe {
        let seconds = seconds as libc::time_t;
        let mut t: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&seconds, &mut t).is_null() {
            (1970, 0)
        } else {
            (t.tm_year + 1900, t.tm_mon.clamp(0, 11) as u32)
        }
    }
}
fn v(key: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":key,"style":style,"children":children})
}
fn text(label: Value, color: &Value, size: f64) -> Value {
    json!({"kind":"text","label":label,"style":{"color":color,"fontSize":size,"minWidth":0,"overflow":"hidden","lineHeight":1.3}})
}
fn label(k: &str) -> Value {
    json!({"key":format!("photos.{k}")})
}
fn button(c: Value, cmd: &str, arg: Value) -> Value {
    json!({"kind":"button","key":format!("{cmd}:{arg}"),"label":c,"action":[cmd,arg],"small":true})
}
fn action(mut node: Value, event: &str, cmd: &str, arg: Value) -> Value {
    node["events"][event] = json!([cmd, arg]);
    if event == "onMouseDown" {
        node["events"]["onClick"] = json!(["noop"]);
    }
    node
}
fn row(key: &str, children: Vec<Value>, height: f64) -> Value {
    v(
        key,
        json!({"height":height,"flexShrink":0,"flexDirection":"row","gap":6,"paddingHorizontal":10,"alignItems":"center"}),
        children,
    )
}
pub fn icon(s: f64) -> Value {
    let s = if s.is_finite() {
        s.clamp(1., 512.)
    } else {
        32.
    };
    let mut children = vec![];
    for (i, (c, x, y)) in [
        ("#ffb340", 0.35, 0.16),
        ("#ff5f57", 0.55, 0.27),
        ("#c86dd7", 0.55, 0.47),
        ("#4c8dff", 0.35, 0.58),
        ("#33c7a8", 0.15, 0.47),
        ("#9bd44a", 0.15, 0.27),
    ]
    .into_iter()
    .enumerate()
    {
        children.push(v(&format!("petal{i}"),json!({"position":"absolute","left":s*x,"top":s*y,"width":s*0.3,"height":s*0.3,"borderRadius":s*0.15,"backgroundColor":c,"opacity":0.88}),vec![]))
    }
    children.push(v("center",json!({"position":"absolute","left":s*0.42,"top":s*0.4,"width":s*0.16,"height":s*0.16,"borderRadius":s*0.08,"backgroundColor":"#fff"}),vec![]));
    v(
        "photos-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"backgroundColor":"#fbfbfd","overflow":"hidden"}),
        children,
    )
}
pub fn tree(app: &App) -> Value {
    let dark = app.current.is_some();
    let color = if dark {
        json!("#f6f6f8")
    } else {
        app.theme["text"].clone()
    };
    let dim = if dark {
        json!("#a9a9b2")
    } else {
        app.theme["textDim"].clone()
    };
    let mut children = vec![];
    if let Some(e) = &app.current {
        let mut tools = vec![
            button(json!("‹"), "library", Value::Null),
            text(json!(e.name), &color, 13.),
            v("space", json!({"flex":1}), vec![]),
            button(json!("−"), "zoomOut", Value::Null),
            button(
                json!(format!(
                    "{}%",
                    (app.view.scale(app.image_size.0, app.image_size.1) * 100.).round()
                )),
                "actual",
                Value::Null,
            ),
            button(json!("+"), "zoomIn", Value::Null),
            button(label("fit"), "fit", Value::Null),
            button(json!("↶"), "rotateLeft", Value::Null),
            button(json!("↷"), "rotateRight", Value::Null),
            button(label("edit"), "edit", Value::Null),
        ];
        if app.width < 720. {
            tools.retain(|v| {
                ![
                    json!(["rotateLeft", null]),
                    json!(["rotateRight", null]),
                    json!(["actual", null]),
                ]
                .contains(&v["action"])
            });
        }
        children.push(row("toolbar", tools, 48.));
        let (w, h) = app.viewport();
        let mut content = vec![];
        if app.image_size.0 > 0 {
            content.push(json!({"kind":"raster","key":"photo-raster","image":"viewport","generation":app.raster_generation,"style":{"position":"absolute","left":0,"top":0,"width":w,"height":h}}));
        }
        let selection = if let Some([x, y, cw, ch]) = app.crop {
            let (ox, oy) = app.view.origin(app.image_size.0, app.image_size.1);
            let scale = app.view.scale(app.image_size.0, app.image_size.1);
            Some((
                (ox + x as f64 * scale, oy + y as f64 * scale),
                (
                    ox + (x as f64 + cw as f64) * scale,
                    oy + (y as f64 + ch as f64) * scale,
                ),
            ))
        } else {
            app.selection
        };
        if let Some((start, end)) = selection {
            let x = start.0.min(end.0).clamp(0., w as f64);
            let y = start.1.min(end.1).clamp(0., h as f64);
            let right = start.0.max(end.0).clamp(0., w as f64);
            let bottom = start.1.max(end.1).clamp(0., h as f64);
            content.push(v("crop",json!({"position":"absolute","left":x,"top":y,"width":right-x,"height":bottom-y,"borderWidth":2,"borderColor":"#fff","backgroundColor":"rgba(255,255,255,0.08)","pointerEvents":"none"}),vec![]));
        }
        if app.busy && app.image_size.0 == 0 {
            content.push(text(json!({"key":"common.loading"}), &dim, 14.));
        }
        let mut viewport = v(
            "viewport",
            json!({"width":w,"height":h,"flexShrink":0,"backgroundColor":"#141416","overflow":"hidden","alignItems":"center","justifyContent":"center"}),
            content,
        );
        viewport["events"] = json!({"onClick":["noop"],"onMouseDown":["pan"],"onWheel":["wheel"],"onGesture":["gesture"],"onDoubleClick":[if app.view.zoom==0.{"actual"}else{"fit"}]});
        children.push(viewport);
        if app.edit {
            let mut controls = vec![];
            for (key, value) in [
                ("brightness", app.view.brightness),
                ("contrast", app.view.contrast),
            ] {
                let track = action(
                    v(
                        key,
                        json!({"height":22,"width":140,"backgroundColor":"#303038","borderRadius":8}),
                        vec![v(
                            "knob",
                            json!({"position":"absolute","left":(value+100) as f64/200.*128.,"top":3,"width":12,"height":16,"borderRadius":6,"backgroundColor":"#fff"}),
                            vec![],
                        )],
                    ),
                    "onMouseDown",
                    "range",
                    json!(key),
                );
                controls.push(text(label(key), &dim, 11.));
                controls.push(track);
                controls.push(text(json!(value.to_string()), &dim, 11.));
            }
            children.push(row("adjustments", controls, 30.));
            children.push(row(
                "crop-tools",
                vec![
                    text(
                        label(if app.crop.is_some() {
                            "cropReady"
                        } else {
                            "cropHint"
                        }),
                        &dim,
                        11.,
                    ),
                    v("space", json!({"flex":1}), vec![]),
                    button(label("reset"), "reset", Value::Null),
                    button(label("saveCopy"), "save", Value::Null),
                ],
                30.,
            ));
        }
        let status = if app.saving {
            "…".to_owned()
        } else if !app.error.is_empty() {
            app.error.clone()
        } else if !app.message.is_empty() {
            app.message.clone()
        } else {
            format!("{} × {}", app.image_size.0, app.image_size.1)
        };
        children.push(row(
            "status",
            vec![
                button(json!("‹"), "previous", Value::Null),
                button(json!("›"), "nextImage", Value::Null),
                text(
                    if app.error.is_empty() {
                        json!(status)
                    } else {
                        json!({"error":app.error})
                    },
                    &dim,
                    11.,
                ),
                v("space", json!({"flex":1}), vec![]),
                button(
                    json!(if app.slide { "Ⅱ" } else { "▶" }),
                    "slideshow",
                    Value::Null,
                ),
                button(json!("⛶"), "fullscreen", Value::Null),
                button(label("menu.print"), "print", Value::Null),
            ],
            38.,
        ));
    } else {
        children.push(row(
            "library-tools",
            vec![
                text(label("library"), &color, 18.),
                text(
                    json!({"key":"photos.count","params":{"count":app.entries.len()}}),
                    &dim,
                    12.,
                ),
                v("space", json!({"flex":1}), vec![]),
                button(json!("↻"), "refresh", Value::Null),
                text(label("thumbSize"), &dim, 11.),
                button(json!("−"), "thumbSmaller", Value::Null),
                button(json!("+"), "thumbLarger", Value::Null),
            ],
            54.,
        ));
        let mut sidebar = vec![
            button(label("allPhotos"), "all", Value::Null),
            text(label("folders"), &dim, 11.),
        ];
        if app.folder != app.root {
            sidebar.push(button(
                json!(".."),
                "folder",
                json!(app.folder.parent().unwrap_or(&app.root)),
            ));
        }
        let mut folders = vec![];
        let count = ((app.height - 170.).max(1.) / 32.).ceil() as usize + 2;
        let start = (app.folder_scroll / 32.).floor() as usize;
        for (i, p) in app.folders.iter().enumerate().skip(start).take(count) {
            let mut b = button(
                json!(p.file_name().unwrap_or_default().to_string_lossy()),
                "folder",
                json!(p),
            );
            b["style"] = json!({"position":"absolute","left":0,"top":i as f64*32.-app.folder_scroll,"width":156,"height":28,"overflow":"hidden"});
            folders.push(b);
        }
        let mut list = v(
            "folder-list",
            json!({"position":"relative","height":(app.height-134.).max(1.),"overflow":"hidden"}),
            folders,
        );
        list["events"] = json!({"onWheel":["folderWheel"],"onGesture":["folderGesture"]});
        sidebar.push(list);
        let side = v(
            "sidebar",
            json!({"width":180,"height":(app.height-54.).max(0.),"padding":12,"gap":8,"flexShrink":0,"backgroundColor":app.theme["sidebarBg"],"overflow":"hidden"}),
            sidebar,
        );
        let mut tiles = vec![];
        let mut col = 0;
        let cols = app.columns();
        let rows = app.rows();
        for &(i, y, header) in rows {
            if let Some((year, month)) = header {
                col = 0;
                if y >= app.scroll && y - 32. < app.scroll + app.height {
                    tiles.push(v(&format!("month{i}"),json!({"position":"absolute","left":16,"top":y-30.-app.scroll,"height":25,"flexDirection":"row","gap":5}),vec![text(label(&format!("month.{month}")),&color,14.),text(json!(year.to_string()),&dim,14.)]));
                }
            }
            let c = col;
            col = (col + 1) % cols;
            if y + app.thumb_size + 34. < app.scroll || y > app.scroll + app.height {
                continue;
            }
            let entry = &app.entries[i];
            let id = entry.path.to_string_lossy();
            let mut contents = vec![];
            if let Some(t) = app.thumbs.get(id.as_ref()) {
                if !t.bytes.is_empty() {
                    contents.push(json!({"kind":"raster","key":"thumbnail","image":id,"generation":app.generation,"style":{"width":app.thumb_size,"height":app.thumb_size,"objectFit":"contain"}}));
                }
            } else {
                contents.push(text(
                    json!(entry
                        .path
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_ascii_uppercase()),
                    &dim,
                    12.,
                ));
            }
            let tile = v(
                &format!("tile{i}"),
                json!({"height":app.thumb_size,"width":app.thumb_size,"borderRadius":6,"overflow":"hidden","backgroundColor":app.theme["control"],"borderWidth":if app.selected==Some(i){3}else{0},"borderColor":app.theme["accent"],"alignItems":"center","justifyContent":"center"}),
                contents,
            );
            let mut tile = action(tile, "onClick", "select", json!(i));
            tile["events"]["onDoubleClick"] = json!(["open", i]);
            let cell = v(
                &format!("cell{i}"),
                json!({"position":"absolute","left":16.+if app.rtl{(cols-1-c) as f64}else{c as f64}*(app.thumb_size+12.),"top":y-app.scroll,"width":app.thumb_size,"gap":4}),
                vec![tile, text(json!(entry.name), &dim, 11.)],
            );
            tiles.push(cell);
        }
        if app.entries.is_empty() {
            tiles.push(v(
                "empty",
                json!({"padding":36,"gap":12}),
                vec![
                    text(
                        label(if app.scanning {
                            "library"
                        } else {
                            "emptyTitle"
                        }),
                        &color,
                        18.,
                    ),
                    text(
                        if app.error.is_empty() {
                            label("empty")
                        } else {
                            json!({"error":app.error})
                        },
                        &dim,
                        13.,
                    ),
                ],
            ));
        }
        if app.max_scroll() > 0. {
            let height = (app.height - 64.).max(1.);
            let handle = (height * height / (height + app.max_scroll())).max(24.);
            let bar = action(
                v(
                    "scrollbar",
                    json!({"position":"absolute","right":3,"top":(height-handle)*app.scroll/app.max_scroll(),"width":7,"height":handle,"backgroundColor":app.theme["textDim"],"borderRadius":4}),
                    vec![],
                ),
                "onMouseDown",
                "scrollbar",
                Value::Null,
            );
            tiles.push(bar);
        }
        let mut grid = v(
            "grid",
            json!({"position":"relative","flex":1,"height":(app.height-54.).max(1.),"overflow":"hidden"}),
            tiles,
        );
        grid["events"] = json!({"onWheel":["wheel"],"onGesture":["gesture"]});
        children.push(v(
            "body",
            json!({"flexDirection":"row","flex":1,"minHeight":0}),
            vec![side, grid],
        ));
    }
    v(
        "photos",
        json!({"width":app.width,"height":app.height,"backgroundColor":if dark{json!("#141416")}else{app.theme["windowBg"].clone()},"overflow":"hidden"}),
        children,
    )
}
