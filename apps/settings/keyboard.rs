use super::*;
impl App {
    fn keyboards(&self) -> Vec<Value> {
        let mine = arr(self.pref("keyboards"));
        if mine.is_empty() {
            vec![json!({"layout":self.pref("keyboard").as_str().unwrap_or("latam"),"variant":""})]
        } else {
            mine.to_vec()
        }
    }
    pub(super) fn keyboard(&self) -> Value {
        let u = self.ui();
        let all = arr(self.d("keyboard.list"));
        let mine = self.keyboards();
        let active = n(self.pref("keyboardIndex")) as usize;
        let mut rows = vec![];
        for (i, k) in mine.iter().enumerate() {
            let layout = all.iter().find(|x| x["id"] == k["layout"]);
            let variant =
                layout.and_then(|l| arr(&l["variants"]).iter().find(|v| v["id"] == k["variant"]));
            let name = if !s(&k["ime"]).is_empty() {
                t(&format!("settings.ime.{}", s(&k["ime"])))
            } else {
                variant
                    .or(layout)
                    .map(|v| v["name"].clone())
                    .unwrap_or_else(|| k["layout"].clone())
            };
            let mut controls = vec![];
            if i != active {
                controls.push(u.button(
                    t("settings.kb.use"),
                    patch("keyboardIndex", json!(i)),
                    false,
                    false,
                ));
            }
            if mine.len() > 1 {
                controls.push(u.button(
                    "−",
                    json!({"op":"keyboardRemove","value":i}),
                    false,
                    false,
                ));
            }
            let mut row = u.row(name, "", controls);
            let label = match s(&k["ime"]) {
                "hangul" => "한".into(),
                "kana" => "あ".into(),
                "pinyin" => "拼".into(),
                _ => k["label"]
                    .as_str()
                    .unwrap_or(s(&k["layout"]))
                    .chars()
                    .take(2)
                    .collect::<String>()
                    .to_uppercase(),
            };
            row["children"].as_array_mut().unwrap().insert(0,view(json!({"minWidth":30,"height":22,"paddingHorizontal":5,"borderRadius":5,"alignItems":"center","justifyContent":"center","backgroundColor":u.color(if i==active{"accent"}else{"control"})}),vec![u.text(label,11.,false)]));
            rows.push(row);
        }
        if b(self.st("addKeyboard")) && !all.is_empty() {
            let items = all
                .iter()
                .flat_map(|l| {
                    let id = s(&l["id"]);
                    let mut items = vec![json!([id, l["name"], id])];
                    for v in arr(&l["variants"]) {
                        let key = format!("{id}({})", s(&v["id"]));
                        items.push(json!([key, v["name"], key]));
                    }
                    items
                })
                .collect();
            rows.push(self.search_list(
                "keyboardSearch",
                items,
                op("keyboardAdd"),
                "",
                220.,
                "settings.kb.search",
                300,
            ));
        }
        rows.push(actions(vec![u.button(
            t("settings.kb.add"),
            state("addKeyboard", json!(!b(self.st("addKeyboard")))),
            false,
            false,
        )]));
        let mut out = vec![
            u.section(
                t("settings.kb.profile.title"),
                t("settings.kb.profile.hint"),
                vec![u.row(
                    t("settings.kb.profile.label"),
                    "",
                    vec![u.segments(
                        self.pref("shortcutProfile").clone(),
                        vec![
                            json!(["macos", "macOS"]),
                            json!(["linux", "Linux / Windows"]),
                        ],
                        patch("shortcutProfile", Value::Null),
                        false,
                        false,
                    )],
                )],
            ),
            u.section(t("settings.kb.layouts"), t("settings.kb.hint"), rows),
        ];
        if all.is_empty() {
            out.push(u.note(t("settings.kb.noxkb"), "warn"));
        }
        out.push(
            u.section(
                t("settings.ime.title"),
                t("settings.ime.hint"),
                ["hangul", "kana", "pinyin"]
                    .iter()
                    .map(|id| {
                        u.row(
                            t(&format!("settings.ime.{id}")),
                            "",
                            if mine.iter().any(|k| k["ime"] == *id) {
                                vec![]
                            } else {
                                vec![u.button(
                                    t("settings.ime.add"),
                                    json!({"op":"keyboardIme","value":id}),
                                    false,
                                    false,
                                )]
                            },
                        )
                    })
                    .collect(),
            ),
        );
        if !s(&mine[active.min(mine.len() - 1)]["ime"]).is_empty()
            && b(self.d("keyboard.missingDictionary"))
        {
            out.push(u.note(t("settings.ime.noDict"), "warn"));
        }
        out.push(u.section(
            t("settings.osk.title"),
            t("settings.osk.hint"),
            vec![actions(vec![u.button(
                t("settings.osk.show"),
                call("osk.show", json!([])),
                false,
                false,
            )])],
        ));
        out.push(u.section(t("settings.kb.test"),t("settings.kb.testHint"),vec![json!({"kind":"input","key":"keyboard-sample","value":self.st("keyboardSample").as_str().unwrap_or(""),"placeholder":t("settings.kb.placeholder"),"change":["field","keyboardSample"],"cancel":action(state("keyboardSample",json!("")))})]));
        if self.pref("shortcutProfile") != "linux" {
            out.push(u.section(
                t("settings.kb.desktopArrows.title"),
                t("settings.kb.desktopArrows.hint"),
                vec![u.row(
                    t("settings.kb.desktopArrows.label"),
                    "",
                    vec![u.toggle(
                        self.pref("desktopArrows") != false,
                        patch("desktopArrows", json!(self.pref("desktopArrows") == false)),
                    )],
                )],
            ));
        }
        for group in arr(self.d("shortcuts.list")) {
            let rows = arr(&group["items"])
                .iter()
                .map(|i| u.value(i["label"].clone(), i["keys"].clone()))
                .collect();
            out.push(u.section(
                t(&format!("settings.kb.shortcuts.{}", s(&group["group"]))),
                if group["group"] == "apps" {
                    t("settings.kb.shortcuts.note")
                } else {
                    json!("")
                },
                rows,
            ));
        }
        stack(out)
    }
    pub(super) fn keyboard_command(&mut self, c: &Value) -> bool {
        let mut mine = self.keyboards();
        match s(&c["op"]) {
            "keyboardRemove" => {
                let i = n(&c["value"]) as usize;
                if mine.len() <= 1 || i >= mine.len() {
                    return true;
                }
                mine.remove(i);
            }
            "keyboardAdd" => {
                let id = s(&c["value"]);
                let (layout, variant) = id
                    .split_once('(')
                    .map(|(l, v)| (l, v.trim_end_matches(')')))
                    .unwrap_or((id, ""));
                if !arr(self.d("keyboard.list")).iter().any(|l| {
                    l["id"] == layout
                        && (variant.is_empty()
                            || arr(&l["variants"]).iter().any(|v| v["id"] == variant))
                }) {
                    return true;
                }
                let item = json!({"layout":layout,"variant":variant});
                if !mine.iter().any(|k| {
                    k["layout"] == layout && k["variant"] == variant && s(&k["ime"]).is_empty()
                }) {
                    mine.push(item);
                }
                self.state["addKeyboard"] = json!(false);
            }
            "keyboardIme" => {
                let id = s(&c["value"]);
                if !["hangul", "kana", "pinyin"].contains(&id) {
                    return true;
                }
                if !mine.iter().any(|k| k["ime"] == id) {
                    mine.push(json!({"layout":"us","variant":"","ime":id}));
                }
            }
            _ => return false,
        }
        mine.truncate(6);
        self.patch_many(json!({"keyboards":mine,"keyboardIndex":0}));
        true
    }
}
