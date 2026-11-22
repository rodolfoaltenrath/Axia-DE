//! The original inset Settings controls, drawn directly in the native scene.
use serde_json::{json, Value};
pub fn t(key: &str) -> Value {
    json!({"key":key})
}
pub fn tp(key: &str, params: Value) -> Value {
    json!({"key":key,"params":params})
}
pub fn view(style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","style":style,"children":children})
}
pub fn click(mut node: Value, command: Value) -> Value {
    if !command.is_object() {
        return node;
    }
    node["events"]["onClick"] = action(command);
    node
}
pub fn action(command: Value) -> Value {
    json!(["do", command.to_string()])
}
pub fn state(path: &str, value: Value) -> Value {
    json!({"op":"state","path":path,"value":value})
}
pub fn patch(path: &str, value: Value) -> Value {
    json!({"op":"patch","path":path,"value":value})
}
pub fn call(name: &str, args: Value) -> Value {
    json!({"op":"call","name":name,"args":args})
}
pub fn op(name: &str) -> Value {
    json!({"op":name})
}
pub fn arr(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub fn s(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
pub fn n(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.)
}
pub fn b(value: &Value) -> bool {
    value.as_bool().unwrap_or(false)
}
pub fn get<'a>(value: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(value, |v, k| &v[k])
}
pub fn put(value: &mut Value, path: &str, item: Value) {
    if let Some((head, tail)) = path.split_once('.') {
        if !value[head].is_object() {
            value[head] = json!({});
        }
        put(&mut value[head], tail, item);
    } else {
        if !value.is_object() {
            *value = json!({});
        }
        value[path] = item;
    }
}
pub fn merge(base: &mut Value, other: &Value) {
    if let Some(map) = other.as_object() {
        for (k, v) in map {
            base[k] = v.clone();
        }
    }
}
pub fn stack(children: Vec<Value>) -> Value {
    view(json!({"gap":20,"paddingBottom":24}), children)
}
pub fn actions(children: Vec<Value>) -> Value {
    view(
        json!({"flexDirection":"row","alignItems":"center","flexWrap":"wrap","gap":8,"justifyContent":"flex-end"}),
        children,
    )
}
pub struct Ui {
    pub theme: std::rc::Rc<Value>,
}
impl Ui {
    pub fn color(&self, key: &str) -> Value {
        self.theme.get(key).cloned().unwrap_or_else(|| {
            json!(match key {
                "text" => "#f5f5f7",
                "textDim" => "#99999f",
                "accent" => "#0a84ff",
                "separator" => "rgba(255,255,255,0.08)",
                "control" => "#3a3a40",
                _ => "transparent",
            })
        })
    }
    pub fn dark(&self) -> bool {
        self.theme["mode"] != "light"
    }
    pub fn text(&self, label: impl Into<Value>, size: f64, dim: bool) -> Value {
        json!({"kind":"text","label":label.into(),"style":{"fontSize":size,"color":self.color(if dim{"textDim"}else{"text"}),"textAlign":"start"}})
    }
    pub fn note(&self, label: impl Into<Value>, tone: &str) -> Value {
        let mut v = self.text(label, 11., true);
        merge(
            &mut v["style"],
            &json!({"lineHeight":1.4,"marginHorizontal":4}),
        );
        if !tone.is_empty() {
            v["style"]["color"] = json!(match tone {
                "error" => "#ff453a",
                "warn" => "#ff9f0a",
                "ok" => "#30d158",
                _ => "#8e8e93",
            });
        }
        v
    }
    pub fn section(
        &self,
        title: impl Into<Value>,
        footer: impl Into<Value>,
        rows: Vec<Value>,
    ) -> Value {
        let title = title.into();
        let footer = footer.into();
        let mut children = vec![];
        if title != json!("") && !title.is_null() {
            let mut title = self.text(title, 13., false);
            merge(
                &mut title["style"],
                &json!({"fontWeight":600,"flex":1,"minWidth":0}),
            );
            children.push(view(json!({"flexDirection":"row","alignItems":"center","gap":8,"minHeight":22,"paddingHorizontal":4}),vec![title]));
        }
        let mut content = vec![];
        for row in rows.into_iter().filter(|v| !v.is_null()) {
            if !content.is_empty() {
                content.push(view(
                    json!({"height":1,"backgroundColor":self.color("separator")}),
                    vec![],
                ));
            }
            content.push(row);
        }
        if !content.is_empty() {
            children.push(view(json!({"paddingHorizontal":12,"borderRadius":self.theme["groupRadius"].as_f64().unwrap_or(10.),"border":format!("1px solid {}",s(&self.color("separator"))),"backgroundColor":self.theme.get("groupBg").cloned().unwrap_or_else(||json!(if self.dark(){"rgba(255,255,255,0.04)"}else{"rgba(0,0,0,0.025)"}))}),content));
        }
        if footer != json!("") && !footer.is_null() {
            children.push(self.note(footer, ""));
        }
        view(json!({"gap":6}), children)
    }
    pub fn row(
        &self,
        label: impl Into<Value>,
        hint: impl Into<Value>,
        controls: Vec<Value>,
    ) -> Value {
        let hint = hint.into();
        let has = hint != json!("") && !hint.is_null();
        let mut labels = vec![self.text(label, 13., false)];
        if has {
            let mut h = self.text(hint, 11., true);
            h["style"]["lineHeight"] = json!(1.35);
            labels.push(h);
        }
        view(
            json!({"minHeight":36,"flexDirection":"row","alignItems":"center","gap":12,"paddingVertical":if has{7}else{4}}),
            vec![
                view(json!({"flex":1,"minWidth":0,"gap":2}), labels),
                view(
                    json!({"flexDirection":"row","alignItems":"center","gap":8}),
                    controls,
                ),
            ],
        )
    }
    pub fn row_icon(&self, mut row: Value, icon: Value) -> Value {
        row["children"].as_array_mut().unwrap().insert(0, icon);
        row
    }
    pub fn section_icon(&self, mut section: Value, icon: Value) -> Value {
        if let Some(children) = section["children"][0]["children"].as_array_mut() {
            children.insert(0, icon);
        }
        section
    }
    pub fn accessory(&self, mut section: Value, accessory: Value) -> Value {
        if let Some(children) = section["children"][0]["children"].as_array_mut() {
            children.push(accessory);
        }
        section
    }
    pub fn disabled(&self, mut row: Value, disabled: bool) -> Value {
        if disabled {
            row["style"]["opacity"] = json!(0.45);
            row["style"]["pointerEvents"] = json!("none");
        }
        row
    }
    pub fn value(&self, label: impl Into<Value>, value: impl Into<Value>) -> Value {
        let mut v = self.text(value, 13., true);
        merge(
            &mut v["style"],
            &json!({"flex":1,"minWidth":0,"textAlign":"end","whiteSpace":"nowrap","overflow":"hidden"}),
        );
        view(
            json!({"minHeight":36,"flexDirection":"row","alignItems":"center","gap":12,"paddingVertical":4}),
            vec![
                self.text(label, 13., false),
                view(
                    json!({"flex":1,"minWidth":0,"flexDirection":"row","alignItems":"center","justifyContent":"flex-end","gap":8}),
                    vec![v],
                ),
            ],
        )
    }
    pub fn field(
        &self,
        label: impl Into<Value>,
        hint: impl Into<Value>,
        path: &str,
        value: impl Into<Value>,
        secure: bool,
    ) -> Value {
        let mut field = json!({"kind":"input","key":path,"value":value.into(),"secure":secure,"change":["field",path],"style":{"width":240,"flexShrink":1,"minWidth":120}});
        if path == "newUser.full" {
            field["autoFocus"] = json!(true);
        }
        if path == "newUser.pass2" {
            field["submit"] = action(op("userCreate"));
        }
        self.row(label, hint, vec![field])
    }
    pub fn button(
        &self,
        label: impl Into<Value>,
        command: Value,
        disabled: bool,
        primary: bool,
    ) -> Value {
        let destructive = matches!(
            s(&command["op"]),
            "userDelete" | "printerRemove" | "vpnDelete"
        ) || matches!(s(&command["name"]), "bt.forget" | "clipboard.clear");
        let mut v = click(
            view(
                json!({"height":22,"paddingHorizontal":10,"borderRadius":self.theme["controlRadiusSmall"].as_f64().unwrap_or(6.),"alignItems":"center","justifyContent":"center","backgroundColor":self.color(if primary{"accent"}else{"control"}),"boxShadow":"0 1px 1px rgba(0,0,0,0.12)","opacity":if disabled{0.45}else{1.}}),
                vec![self.text(label, 12., false)],
            ),
            command,
        );
        if primary {
            v["children"][0]["style"]["color"] = json!("#fff");
        } else if destructive {
            v["children"][0]["style"]["color"] = json!("#ff453a");
        }
        v["children"][0]["style"]["fontWeight"] = json!(500);
        v["children"][0]["style"]["whiteSpace"] = json!("nowrap");
        if disabled {
            v["events"] = json!({});
        }
        v["hoverStyle"] = json!({"opacity":0.88});
        v
    }
    pub fn regular_button(
        &self,
        label: impl Into<Value>,
        command: Value,
        disabled: bool,
        primary: bool,
    ) -> Value {
        let mut button = self.button(label, command, disabled, primary);
        merge(
            &mut button["style"],
            &json!({"height":28,"paddingHorizontal":14,"borderRadius":self.theme["controlRadius"].as_f64().unwrap_or(7.)}),
        );
        button["children"][0]["style"]["fontSize"] = json!(13);
        button
    }
    pub fn stepper(
        &self,
        path: &str,
        value: f64,
        min: f64,
        max: f64,
        step: f64,
        width: f64,
        label: Value,
    ) -> Value {
        let arrow = |glyph: &str, d: f64| {
            let mut text = self.text(glyph, 15., false);
            text["style"]["fontWeight"] = json!(600);
            click(
                view(
                    json!({"width":26,"height":26,"alignItems":"center","justifyContent":"center"}),
                    vec![text],
                ),
                patch(
                    path,
                    json!(((value + d) / step)
                        .round()
                        .mul_add(step, 0.)
                        .clamp(min, max)),
                ),
            )
        };
        let mut text = self.text(label, 12., false);
        merge(
            &mut text["style"],
            &json!({"width":width,"textAlign":"center","fontWeight":600}),
        );
        view(
            json!({"flexDirection":"row","alignItems":"center","backgroundColor":self.color("control"),"borderRadius":self.theme["controlRadius"].as_f64().unwrap_or(7.),"height":26}),
            vec![arrow("−", -step), text, arrow("+", step)],
        )
    }
    pub fn toggle(&self, on: bool, command: Value) -> Value {
        click(
            view(
                json!({"width":36,"height":20,"borderRadius":10,"backgroundColor":self.color(if on{"accent"}else{"control"}),"direction":"ltr"}),
                vec![view(
                    json!({"position":"absolute","top":2,"left":if on{18}else{2},"width":16,"height":16,"borderRadius":8,"backgroundColor":"#fff","boxShadow":"0 1px 3px rgba(0,0,0,.35)"}),
                    vec![],
                )],
            ),
            command,
        )
    }
    pub fn slider(
        &self,
        key: &str,
        value: f64,
        min: f64,
        max: f64,
        step: f64,
        width: f64,
        binding: Value,
    ) -> Value {
        let fraction = ((value - min) / (max - min)).clamp(0., 1.);
        let command = json!({"binding":binding,"min":min,"max":max,"step":step});
        json!({"kind":"view","key":key,"style":{"width":width,"height":20,"justifyContent":"center","direction":"ltr"},"events":{"onMouseDown":["slider",command.to_string()]},"children":[view(json!({"height":4,"borderRadius":2,"backgroundColor":self.color("control")}),vec![view(json!({"width":format!("{}%",fraction*100.),"height":4,"borderRadius":2,"backgroundColor":self.color("accent")}),vec![])]),view(json!({"position":"absolute","left":(width-16.)*fraction,"top":2,"width":16,"height":16,"borderRadius":8,"backgroundColor":"#fff","boxShadow":"0 1px 3px rgba(0,0,0,.3)"}),vec![])]})
    }
    pub fn pop(
        &self,
        key: &str,
        value: Value,
        options: Vec<Value>,
        binding: Value,
        width: f64,
        disabled: bool,
    ) -> Value {
        let label = options
            .iter()
            .find(|o| o[0] == value)
            .map(|o| o[1].clone())
            .unwrap_or(json!(""));
        let mut v=self.button(label,json!({"op":"popup","anchor":key,"value":value,"options":options,"binding":binding,"width":width}),disabled,false);
        v["key"] = json!(key);
        merge(
            &mut v["style"],
            &json!({"height":24,"width":width,"flexDirection":"row","gap":6,"paddingHorizontal":9}),
        );
        v["children"][0]["style"]["flex"] = json!(1);
        if let Some(icon) = options
            .iter()
            .find(|o| o[0] == value)
            .and_then(|o| o.get(2))
            .filter(|v| v.is_object())
        {
            v["children"]
                .as_array_mut()
                .unwrap()
                .insert(0, icon.clone());
        }
        let arrow = if self.theme["menuHighlight"] == "subtle" {
            view(
                json!({"width":16,"height":11,"alignItems":"center","justifyContent":"center"}),
                vec![view(
                    json!({"width":5,"height":5,"borderRightWidth":1,"borderBottomWidth":1,"borderColor":self.color("textDim"),"transform":"rotate(45deg)"}),
                    vec![],
                )],
            )
        } else {
            let mut up = self.text("▲", 8., false);
            up["style"]["color"] = json!("#fff");
            up["style"]["lineHeight"] = json!(8);
            let mut down = up.clone();
            down["label"] = json!("▼");
            view(
                json!({"width":16,"height":18,"borderRadius":3,"backgroundColor":self.color("accent"),"alignItems":"center","justifyContent":"center"}),
                vec![up, down],
            )
        };
        v["children"].as_array_mut().unwrap().push(arrow);
        v
    }
    pub fn segmented(&self, value: Value, options: Vec<Value>, binding: Value) -> Value {
        self.segments(value, options, binding, true, false)
    }
    pub fn tabs(&self, value: Value, options: Vec<Value>, binding: Value) -> Value {
        self.segments(value, options, binding, false, true)
    }
    pub fn segments(
        &self,
        value: Value,
        options: Vec<Value>,
        binding: Value,
        small: bool,
        neutral: bool,
    ) -> Value {
        view(json!({"flexDirection":"row","padding":2,"borderRadius":if small{self.theme["controlRadiusSmall"].as_f64().unwrap_or(6.)}else{self.theme["controlRadius"].as_f64().unwrap_or(7.)+1.},"backgroundColor":self.color("control")}),options.into_iter().map(|o|{let mut cmd=binding.clone();cmd["value"]=o[0].clone();let on=value==o[0];let mut label=self.text(o[1].clone(),if small{11.}else{12.},false);label["style"]["fontWeight"]=json!(if neutral{if on{600}else{500}}else{600});if on&&!neutral{label["style"]["color"]=json!("#fff");}click(view(json!({"paddingHorizontal":if small{10}else{14},"height":if small{18}else{24},"borderRadius":if small{5}else{6},"alignItems":"center","justifyContent":"center","backgroundColor":if !on{json!("transparent")}else if neutral{json!(if self.dark(){"rgba(255,255,255,.18)"}else{"#fff"})}else{self.color("accent")},"boxShadow":if on&&neutral{"0 1px 2px rgba(0,0,0,.18)"}else{"none"}}),vec![label]),cmd)}).collect())
    }
    pub fn header(
        &self,
        icon: Value,
        title: impl Into<Value>,
        subtitle: impl Into<Value>,
        controls: Vec<Value>,
    ) -> Value {
        let mut title = self.text(title, 15., false);
        title["style"]["fontWeight"] = json!(600);
        let mut children = vec![
            icon,
            view(
                json!({"flex":1,"minWidth":0,"gap":3}),
                vec![title, self.text(subtitle, 12., true)],
            ),
        ];
        children.extend(controls);
        view(
            json!({"flexDirection":"row","alignItems":"center","gap":16,"padding":16,"borderRadius":self.theme["groupRadius"].as_f64().unwrap_or(10.),"backgroundColor":if self.dark(){"rgba(255,255,255,.04)"}else{"rgba(0,0,0,.025)"},"border":format!("1px solid {}",s(&self.color("separator")))}),
            children,
        )
    }
}
pub fn opts(values: &[&str], prefix: &str) -> Vec<Value> {
    values
        .iter()
        .map(|v| json!([v, t(&format!("{prefix}{v}"))]))
        .collect()
}
pub fn shade(hex: &str, f: f64) -> String {
    let Ok(n) = u32::from_str_radix(hex.trim_start_matches('#'), 16) else {
        return hex.into();
    };
    if hex.len() != 7 {
        return hex.into();
    }
    let c = |v: u32| {
        let v = v as f64;
        (if f > 1. {
            v + (255. - v) * (f - 1.)
        } else {
            v * f
        })
        .round()
        .clamp(0., 255.) as u8
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        c(n >> 16),
        c((n >> 8) & 255),
        c(n & 255)
    )
}
