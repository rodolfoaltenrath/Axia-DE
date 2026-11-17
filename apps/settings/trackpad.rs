use super::*;
impl App {
    pub(super) fn trackpad(&self) -> Value {
        let u = self.ui();
        let tab = self.st("trackpadTab");
        let config = self.pref("trackpad");
        let kinds: &[&str] = match s(tab) {
            "scroll" => &["natural", "scroll", "kinetic", "pinch"],
            "more" => &["swipe3", "swipe4"],
            _ => &["speed", "tap", "tapDrag", "secondary", "threeDrag", "dwt"],
        };
        let rows=kinds.iter().map(|kind|{let path=format!("trackpad.{kind}");let command=patch(&path,Value::Null);let controls=match *kind{"speed"=>vec![u.text(t("trackpad.slow"),11.,true),u.slider(&path,n(&config["speed"]),0.,1.,0.01,120.,command),u.text(t("trackpad.fast"),11.,true)],"secondary"=>vec![u.segmented(config["secondary"].clone(),vec![json!(["two-fingers",t("trackpad.secondary.two")]),json!(["bottom-right",t("trackpad.secondary.right")]),json!(["bottom-left",t("trackpad.secondary.left")])],command)],_=>vec![u.toggle(b(&config[kind]),patch(&path,json!(!b(&config[kind]))))]};let mut row=u.row(if *kind=="swipe3"&&b(&config["threeDrag"]){t("trackpad.swipe3Moved")}else{t(&format!("trackpad.{kind}"))},t(&format!("trackpad.{kind}Hint")),controls);row["style"]["minHeight"]=json!(56);row["children"].as_array_mut().unwrap().insert(0,self.gesture(kind));row["events"]=json!({"onMouseEnter":action(json!({"op":"gesture","value":kind})),"onMouseLeave":action(json!({"op":"gesture","value":""}))});row}).collect();
        let mut out = vec![
            view(
                json!({"alignItems":"center"}),
                vec![u.tabs(
                    tab.clone(),
                    opts(&["point", "scroll", "more"], "trackpad.tab."),
                    state("trackpadTab", Value::Null),
                )],
            ),
            u.section("", "", rows),
        ];
        for device in arr(self.d("input.devices"))
            .iter()
            .filter(|d| d["kind"] == "touchpad")
        {
            out.push(u.note(
                format!(
                    "{}{}",
                    self.caption(&tp(
                        "trackpad.device",
                        json!({"name":device["name"],"w":device["width"],"h":device["height"]})
                    )),
                    if b(&device["clickpad"]) {
                        format!(" · {}", self.tr("trackpad.clickpad"))
                    } else {
                        String::new()
                    }
                ),
                "",
            ));
        }
        stack(out)
    }
    fn gesture(&self, kind: &str) -> Value {
        let u = self.ui();
        let active = self.st("gesture") == kind && !b(self.pref("reduceMotion"));
        let p = if active {
            (self.now - self.st("gestureAt").as_i64().unwrap_or(self.now)).rem_euclid(1600) as f64
                / 1600.
        } else {
            0.35
        };
        let e = if p < 0.5 { p * 2. } else { 2. - p * 2. };
        let row = |count: usize, y: f64, x: f64, dx: f64| {
            (0..count)
                .map(|i| (x + i as f64 * 0.16 + dx, y, false))
                .collect::<Vec<_>>()
        };
        let fingers = match kind {
            "speed" => vec![(0.25 + 0.5 * e, 0.5, false)],
            "tap" => vec![(0.5, 0.5, p < 0.25)],
            "tapDrag" => {
                if p < 0.2 {
                    vec![(0.3, 0.55, true)]
                } else {
                    vec![(0.3 + 0.4 * ((p - 0.2) * 1.6).min(1.), 0.55, false)]
                }
            }
            "secondary" => vec![(0.42, 0.5, p < 0.25), (0.58, 0.5, p < 0.25)],
            "threeDrag" => row(3, 0.55, 0.2, 0.3 * e),
            "dwt" => vec![(0.5, 0.5, false)],
            "natural" | "scroll" => {
                vec![(0.42, 0.75 - 0.5 * e, false), (0.58, 0.75 - 0.5 * e, false)]
            }
            "kinetic" => {
                if p < 0.35 {
                    vec![(0.42, 0.8 - p * 1.5, false), (0.58, 0.8 - p * 1.5, false)]
                } else {
                    vec![]
                }
            }
            "pinch" => vec![
                (0.38 - 0.18 * e, 0.62 + 0.14 * e, false),
                (0.62 + 0.18 * e, 0.38 - 0.14 * e, false),
            ],
            "swipe3" => row(3, 0.5, 0.5 - 0.3 * e, 0.),
            "swipe4" => row(4, 0.75 - 0.45 * e, 0.26, 0.),
            _ => vec![],
        };
        let mut children = vec![];
        if kind == "natural" || kind == "kinetic" {
            let y = if kind == "natural" {
                0.75 - 0.5 * e
            } else {
                0.8 - (1. - (1. - p).powi(3)).min(1.) * 0.65
            };
            children.push(view(json!({"position":"absolute","left":64.*0.78,"top":44.*y-6.,"width":6,"height":12,"borderRadius":2,"backgroundColor":u.color("textDim"),"opacity":0.6}),vec![]));
        }
        if kind == "dwt" {
            let mut glyph = u.text("⌨", 11., true);
            merge(
                &mut glyph["style"],
                &json!({"position":"absolute","left":6,"top":4}),
            );
            children.push(glyph);
        }
        for (x, y, press) in fingers {
            children.push(view(json!({"position":"absolute","left":x*64.-5.,"top":y*44.-5.,"width":10,"height":10,"borderRadius":5,"backgroundColor":u.color("accent"),"opacity":if kind=="dwt"{0.35}else if press{1.}else{0.8},"border":if press{"2px solid #fff"}else{"0 solid transparent"}}),vec![]));
        }
        view(
            json!({"width":64,"height":44,"borderRadius":7,"backgroundColor":u.color("control"),"border":format!("1px solid {}",s(&u.color("separator"))),"overflow":"hidden","direction":"ltr"}),
            children,
        )
    }
}
