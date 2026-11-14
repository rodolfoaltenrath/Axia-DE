use super::{duration, str_value, App};
use serde_json::{json, Value};
fn v(id: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"kind":"view","key":id,"style":style,"children":children})
}
fn key(k: &str) -> Value {
    json!({"key":k})
}
fn text(label: Value, size: f64, color: &str) -> Value {
    json!({"kind":"text","label":label,"style":{"fontSize":size,"color":color}})
}
fn click(mut node: Value, cmd: &str, arg: Value) -> Value {
    node["events"]["onClick"] = json!([cmd, arg]);
    node
}
pub(super) fn icon(s: f64) -> Value {
    let s = if s.is_finite() {
        s.clamp(1., 512.)
    } else {
        32.
    };
    v(
        "camera-icon",
        json!({"width":s,"height":s,"borderRadius":s*0.225,"background":"linear-gradient(160deg, #3a3f47, #15171b)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",s*0.04,s*0.12),"overflow":"hidden","alignItems":"center","justifyContent":"center"}),
        vec![
            v(
                "body",
                json!({"position":"absolute","left":s*0.16,"top":s*0.3,"width":s*0.68,"height":s*0.46,"borderRadius":s*0.08,"backgroundColor":"#d9dde3"}),
                vec![],
            ),
            v(
                "bump",
                json!({"position":"absolute","left":s*0.24,"top":s*0.23,"width":s*0.18,"height":s*0.09,"borderRadius":s*0.03,"backgroundColor":"#d9dde3"}),
                vec![],
            ),
            v(
                "lens",
                json!({"position":"absolute","left":s*0.32,"top":s*0.33,"width":s*0.36,"height":s*0.36,"borderRadius":s*0.18,"backgroundColor":"#23262c"}),
                vec![],
            ),
            v(
                "glass",
                json!({"position":"absolute","left":s*0.38,"top":s*0.39,"width":s*0.24,"height":s*0.24,"borderRadius":s*0.12,"background":"linear-gradient(135deg, #5b8cff, #1d2a66)"}),
                vec![],
            ),
            v(
                "shine",
                json!({"position":"absolute","left":s*0.44,"top":s*0.43,"width":s*0.06,"height":s*0.06,"borderRadius":s*0.03,"backgroundColor":"rgba(255,255,255,0.8)"}),
                vec![],
            ),
            v(
                "led",
                json!({"position":"absolute","left":s*0.72,"top":s*0.35,"width":s*0.06,"height":s*0.06,"borderRadius":s*0.03,"backgroundColor":"#30d158"}),
                vec![],
            ),
        ],
    )
}
fn no_camera(title: Value, hint: Value) -> Value {
    v(
        "no-camera",
        json!({"position":"absolute","left":0,"right":0,"top":0,"bottom":0,"alignItems":"center","justifyContent":"center","gap":8,"paddingHorizontal":32}),
        vec![
            v(
                "empty-camera",
                json!({"width":52,"height":38,"marginBottom":8}),
                vec![
                    v(
                        "bump",
                        json!({"position":"absolute","left":12,"top":0,"width":16,"height":8,"borderRadius":2,"backgroundColor":"rgba(255,255,255,0.35)"}),
                        vec![],
                    ),
                    v(
                        "body",
                        json!({"position":"absolute","left":0,"top":5,"width":52,"height":33,"borderRadius":7,"backgroundColor":"rgba(255,255,255,0.35)"}),
                        vec![],
                    ),
                    v(
                        "lens",
                        json!({"position":"absolute","left":15,"top":10,"width":22,"height":22,"borderRadius":11,"backgroundColor":"#0b0b0d"}),
                        vec![],
                    ),
                    v(
                        "glass",
                        json!({"position":"absolute","left":20,"top":15,"width":12,"height":12,"borderRadius":6,"backgroundColor":"rgba(255,255,255,0.35)"}),
                        vec![],
                    ),
                ],
            ),
            text(title, 15., "#f2f2f7"),
            v(
                "hint",
                json!({"maxWidth":360}),
                vec![text(hint, 12., "rgba(235,235,245,0.6)")],
            ),
        ],
    )
}
pub(super) fn tree(app: &App, now: i64) -> Value {
    let mut view = vec![
        json!({"kind":"media","key":"viewfinder","media":app.id,"style":{"position":"absolute","left":0,"top":0,"right":0,"bottom":0}}),
    ];
    if app.cameras.is_empty() {
        view.push(no_camera(key("camera.none"), key("camera.noneHint")));
    } else if app.denied {
        view.push(no_camera(key("camera.deniedTitle"), key("camera.denied")));
    } else if !app.error.is_null() {
        view.push(no_camera(
            key("camera.errorTitle"),
            json!({"key":"camera.error","params":{"error":app.error}}),
        ));
    }
    if app.flash_until > now {
        view.push(v("flash",json!({"position":"absolute","left":0,"right":0,"top":0,"bottom":0,"backgroundColor":"rgba(255,255,255,0.85)"}),vec![]));
    }
    if let Some(start) = app.rec_start {
        view.push(v("timer",json!({"position":"absolute","top":12,"left":0,"right":0,"alignItems":"center"}),vec![v("timer-body",json!({"flexDirection":"row","gap":8,"alignItems":"center","paddingHorizontal":12,"height":26,"borderRadius":13,"backgroundColor":"rgba(0,0,0,0.6)"}),vec![v("record-led",json!({"width":8,"height":8,"borderRadius":4,"backgroundColor":"#ff3b30"}),vec![]),text(json!(duration(now-start)),12.,"#ffffff")])]));
    }
    let thumbs=app.shots.iter().take(3).map(|shot|{
        let image=shot.thumb.as_ref().map(|src|json!({"kind":"image","src":src,"style":{"width":52,"height":40,"objectFit":"cover"}})).into_iter().collect();
        click(v(&shot.path,json!({"width":52,"height":40,"borderRadius":6,"overflow":"hidden","backgroundColor":"#2a2a2e","borderWidth":1,"borderColor":"rgba(255,255,255,0.12)","flexShrink":0}),image),"openShot",json!(shot.path))
    }).collect();
    let recording = app.rec_start.is_some();
    let mut shutter = v(
        "shutter",
        json!({"width":60,"height":60,"borderRadius":30,"borderWidth":3,"borderColor":"#ffffff","alignItems":"center","justifyContent":"center","flexShrink":0,"opacity":if app.id>0&&app.folder_ready{1.}else{0.5}}),
        vec![v(
            "shutter-center",
            json!({"width":if recording{22}else{46},"height":if recording{22}else{46},"borderRadius":if recording{5}else{23},"backgroundColor":if app.mode=="video"{"#ff3b30"}else{"#ffffff"}}),
            vec![],
        )],
    );
    shutter["events"]["onClick"] = json!(["shoot"]);
    let modes=[("photo","camera.photo"),("video","camera.video")].iter().map(|(mode,k)|click(v(mode,json!({"height":24,"paddingHorizontal":10,"borderRadius":12,"justifyContent":"center","backgroundColor":if app.mode==*mode{"rgba(255,214,10,0.14)"}else{"transparent"}}),vec![text(key(k),12.,if app.mode==*mode{"#ffd60a"}else{"rgba(255,255,255,0.7)"})]),"mode",json!(mode))).collect();
    let mirror = click(
        v(
            "mirror-toggle",
            json!({"width":28,"height":18,"borderRadius":9,"backgroundColor":if app.mirror{"#ffd60a"}else{"#56565a"},"justifyContent":"center","paddingHorizontal":2,"alignItems":if app.mirror{"flex-end"}else{"flex-start"},"direction":"ltr"}),
            vec![v(
                "knob",
                json!({"width":14,"height":14,"borderRadius":7,"backgroundColor":"#ffffff"}),
                vec![],
            )],
        ),
        "mirror",
        Value::Null,
    );
    let controls = v(
        "controls",
        json!({"height":88,"flexDirection":"row","alignItems":"center","gap":16,"paddingHorizontal":20,"backgroundColor":"#161618","flexShrink":0}),
        vec![
            v(
                "separator",
                json!({"position":"absolute","left":0,"right":0,"top":0,"height":1,"backgroundColor":"rgba(255,255,255,0.08)"}),
                vec![],
            ),
            v(
                "thumbnails",
                json!({"flex":1,"minWidth":0,"overflow":"hidden","flexDirection":"row","gap":6}),
                thumbs,
            ),
            shutter,
            v(
                "choices",
                json!({"flex":1,"alignItems":"flex-end","gap":8}),
                vec![
                    v("modes", json!({"flexDirection":"row","gap":4}), modes),
                    v(
                        "mirror",
                        json!({"flexDirection":"row","alignItems":"center","gap":8}),
                        vec![
                            text(key("camera.mirror"), 12., "rgba(235,235,245,0.7)"),
                            mirror,
                        ],
                    ),
                ],
            ),
        ],
    );
    let mut content = vec![v("preview", json!({"flex":1}), view), controls];
    if app.cameras.len() > 1 {
        content.push(v("devices",json!({"position":"absolute","right":12,"top":12,"flexDirection":"row","gap":4,"padding":3,"borderRadius":9,"backgroundColor":"rgba(0,0,0,0.55)"}),app.cameras.iter().map(|c|click(v(str_value(&c["device"]),json!({"paddingHorizontal":10,"height":24,"borderRadius":6,"justifyContent":"center","backgroundColor":if app.device.as_deref()==Some(str_value(&c["device"])){app.theme["accent"].clone()}else{json!("transparent")}}),vec![text(c["name"].clone(),12.,"#ffffff")]),"device",c["device"].clone())).collect()));
    } else if let Some(camera) = app
        .cameras
        .iter()
        .find(|c| Some(str_value(&c["device"])) == app.device.as_deref())
    {
        let info = if app.info.is_object() {
            format!(" · {}×{}", app.info["width"], app.info["height"])
        } else {
            String::new()
        };
        content.push(v("camera-name",json!({"position":"absolute","left":12,"top":12,"paddingHorizontal":10,"height":24,"justifyContent":"center","borderRadius":12,"backgroundColor":"rgba(0,0,0,0.5)"}),vec![text(json!(format!("{}{info}",str_value(&camera["name"]))),11.,"#e5e5ea")]));
    }
    v(
        "camera",
        json!({"flex":1,"backgroundColor":"#0b0b0d","direction":if app.rtl{"rtl"}else{"ltr"}}),
        content,
    )
}
