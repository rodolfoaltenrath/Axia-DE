use super::*;
impl App {
    pub(super) fn wallpapers(&self) -> Vec<&Value> {
        let catalog = self.appearance_catalogs.map(|(wallpapers, _)| wallpapers).unwrap_or(&self.theme["wallpapers"]);
        arr(self.pref("wallpaperUser")).iter().chain(arr(catalog)).collect()
    }
    fn wallpaper_name(&self, w: &Value) -> String {
        let key = s(&w["nameKey"]);
        if !key.is_empty() {
            let label = self.tr(key);
            if label != key {
                return label;
            }
        }
        s(&w["name"]).into()
    }
    pub(super) fn wallpaper(&self) -> Value {
        let u = self.ui();
        let list = self.wallpapers();
        let current = list
            .iter()
            .find(|w| w["id"] == *self.pref("wallpaper"))
            .or_else(|| list.iter().find(|w| w["id"] == "infinite-galaxy"))
            .or_else(|| list.first());
        let mut out = vec![u.section(
            t("settings.wallpaper.personal"),
            "",
            vec![
                u.regular_button(
                    t("settings.wallpaper.chooseImage"),
                    op("wallpaperChoose"),
                    false,
                    false,
                ),
                u.note(t("settings.wallpaper.imageHint"), ""),
            ],
        )];
        let category = |c: &str| {
            let key = format!("settings.wallpaperCategory.{c}");
            let label = self.tr(&key);
            if label == key {
                t("settings.wallpaperCategory.other")
            } else {
                json!(label)
            }
        };
        out.push(u.section("","",vec![view(json!({"flexDirection":"row","alignItems":"center","gap":16,"paddingVertical":12}),vec![view(json!({"width":128,"height":72,"borderRadius":8,"overflow":"hidden","border":format!("1px solid {}",s(&u.color("separator")))}),vec![current.map(|w|image(w,128.,72.)).unwrap_or(Value::Null)]),view(json!({"flex":1,"minWidth":0,"gap":2}),vec![u.text(current.map(|w|json!(self.wallpaper_name(w))).unwrap_or_else(||t("settings.noWallpapers")),17.,false),u.text(current.map(|w|category(w["category"].as_str().unwrap_or("other"))).unwrap_or(json!("")),11.,true)])]) ]));
        let mut cats = vec!["horizons".to_owned(), "cosmos".into(), "gradients".into()];
        for w in &list {
            let c = w["category"].as_str().unwrap_or("other").to_owned();
            if !cats.contains(&c) {
                cats.push(c);
            }
        }
        for c in cats {
            let ws: Vec<_> = list
                .iter()
                .filter(|w| w["category"].as_str().unwrap_or("other") == c)
                .collect();
            if ws.is_empty() {
                continue;
            }
            out.push(u.section(category(&c),"",vec![view(json!({"flexDirection":"row","flexWrap":"wrap","gap":10,"paddingVertical":12}),ws.into_iter().map(|w|{let selected=current.is_some_and(|cur|cur["id"]==w["id"]);click(view(json!({"width":108,"alignItems":"center","gap":6}),vec![view(json!({"padding":2,"borderRadius":10,"border":format!("2px solid {}",if selected{s(&u.color("accent")).to_owned()}else{"transparent".into()})}),vec![view(json!({"width":100,"height":56,"borderRadius":7,"overflow":"hidden","border":format!("1px solid {}",s(&u.color("separator")))}),vec![image(w,100.,56.)])]),u.text(self.wallpaper_name(w),11.,!selected)]),patch("wallpaper",w["id"].clone()))}).collect())]));
        }
        stack(out)
    }
    pub(super) fn wallpaper_command(&mut self, c: &Value) -> bool {
        if c["op"] != "wallpaperChoose" {
            return false;
        }
        self.jobs.enqueue(Tag::Raw(json!({"op":"wallpaperPicked"})),"files.pick",json!([{"title":self.tr("settings.wallpaper.chooseImage"),"extensions":["png","jpg","jpeg"]}]));
        true
    }
    pub(super) fn wallpaper_after(&mut self, after: &Value, value: &Value) {
        match s(&after["op"]) {
            "wallpaperPicked" => {
                if !s(value).is_empty() {
                    self.call(
                        "wallpaper.import",
                        json!([value]),
                        json!({"op":"wallpaperImported"}),
                    );
                }
            }
            "wallpaperImported" => {
                if value.is_object() && !s(&value["id"]).is_empty() {
                    let mut list = vec![value.clone()];
                    list.extend(
                        arr(self.pref("wallpaperUser"))
                            .iter()
                            .filter(|w| w["id"] != value["id"])
                            .cloned(),
                    );
                    self.patch_many(json!({"wallpaperUser":list,"wallpaper":value["id"]}));
                } else {
                    self.message(false, t("settings.wallpaper.importFailed"));
                }
            }
            _ => {}
        }
    }
}
fn image(w: &Value, width: f64, height: f64) -> Value {
    json!({"kind":"image","src":w["thumb"].as_str().unwrap_or(s(&w["src"])),"style":{"width":width,"height":height,"objectFit":"cover"}})
}
