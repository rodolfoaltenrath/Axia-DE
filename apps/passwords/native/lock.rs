//! The lock screens (apps/passwords/Lock.jsx): creating the vault, unlocking it and the
//! recovery key. A centered column (360 px; 460 px for the forms with more choices), the
//! padlock, a 22 px title, a neutral method switch and one primary button.
use super::glyphs::glyph;
use super::ui::*;
use super::{App, Lock};
use serde_json::{json, Value};

/// The padlock of the lock screens, in the app icon's colors; the shackle is a neutral gray
/// that reads on light and dark backgrounds.
fn padlock(t: &Theme, size: f64, open: bool) -> Value {
    let shackle = if t.dark() { "rgba(235,235,245,0.62)" } else { "#a8a8b0" };
    view(json!({"width":size,"height":size*1.04,"alignItems":"center","direction":"ltr"}), vec![
        view(json!({"width":size*0.5,"height":size*0.5,"borderRadius":size*0.25,
            "border":format!("{}px solid {shackle}", (size*0.085).round().max(3.)),
            "marginBottom":-size*0.25,"marginLeft":if open {size*0.34} else {0.}}), vec![]),
        view(json!({"width":size*0.8,"height":size*0.56,"borderRadius":size*0.13,"background":"linear-gradient(180deg, #38c6d4, #1f6fd1)",
            "alignItems":"center","justifyContent":"center","boxShadow":"0 4px 12px rgba(0,0,0,0.22)"}), vec![
            view(json!({"width":size*0.12,"height":size*0.12,"borderRadius":size*0.06,"backgroundColor":"#fff"}), vec![]),
            view(json!({"width":size*0.05,"height":size*0.12,"marginTop":-1,"borderRadius":size*0.02,"backgroundColor":"#fff"}), vec![]),
        ]),
    ])
}

/// The header of a lock screen: picture, title and one line under it.
fn head(t: &Theme, pic: Value, title: Value, sub: Value) -> Value {
    let mut kids = vec![pic, text(title, json!({"marginTop":6,"fontSize":22,"fontWeight":600,"color":t.text(),"textAlign":"center"}))];
    if !sub.is_null() {
        kids.push(text(sub, json!({"fontSize":13,"color":t.dim(),"textAlign":"center","lineHeight":18,"maxWidth":400})));
    }
    view(json!({"alignItems":"center","gap":8,"marginBottom":4}), kids)
}

/// A picked file: its name (or a dim placeholder) and the button that picks it.
fn file_row(t: &Theme, name: Option<&str>, empty: Value, button_label: Value, action: Value) -> Value {
    let has = name.is_some();
    view(
        json!({"height":36,"flexDirection":"row","gap":8,"alignItems":"center","paddingInlineStart":10,"paddingInlineEnd":4,"borderRadius":8,
            "border":format!("1px solid {}", t.s(t.sep()))}),
        vec![
            glyph("drive", 14., &t.s(if has { t.accent() } else { t.dim() })),
            text(name.map_or(empty, |n| json!(n)), json!({"flex":1,"minWidth":0,"fontSize":12,"color":if has { t.text() } else { t.dim() },
                "whiteSpace":"nowrap","textOverflow":"ellipsis"})),
            button(t, button_label, action, Btn { small: true, ..Btn::default() }),
        ],
    )
}

fn shell(t: &Theme, wide: bool, kids: Vec<Value>) -> Value {
    view(json!({"flex":1,"alignItems":"center","justifyContent":"center","backgroundColor":t.bg(),"padding":20}), vec![
        view(json!({"width":if wide {460} else {360},"maxWidth":"100%","gap":14,"alignItems":"stretch"}), kids.into_iter().filter(|k| !k.is_null()).collect()),
    ])
}

fn error_line(err: &Value, centered: bool) -> Value {
    if err.is_null() {
        return Value::Null;
    }
    let mut style = json!({"fontSize":12,"color":RED});
    if centered {
        style["textAlign"] = json!("center");
        style["lineHeight"] = json!(17);
    }
    text(err.clone(), style)
}

/// The methods the vault can be unlocked with, in the order of the switch.
pub fn unlock_methods(status: &Value) -> Vec<&'static str> {
    let slots = status["main"]["slots"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    ["password", "passkey", "keyfile", "fido", "recovery"].into_iter().filter(|m| slots.iter().any(|s| s["type"] == *m)).collect()
}
/// The method picked first: the first one there is, else the recovery key.
pub fn first_method(status: &Value) -> &'static str {
    unlock_methods(status).into_iter().find(|m| *m != "recovery").unwrap_or("recovery")
}

pub fn unlock(app: &App, t: &Theme) -> Value {
    let f: &Lock = &app.lock;
    let opts: Vec<(&str, Value)> = unlock_methods(&app.status).into_iter().map(|m| (m, k(&format!("method.{m}")))).collect();
    let m = f.method.as_str();
    let mut kids = vec![head(t, padlock(t, 64., false), k("unlock.title"), k("unlock.subtitle"))];
    if opts.len() > 1 {
        kids.push(view(json!({"alignItems":"center"}), vec![segmented(t, "method", m, &opts, true, false, false)]));
    }
    if m == "password" || m == "passkey" {
        kids.push(input("pw", &f.pw, k("unlock.password"), true, true, Some("go"), json!({})));
    }
    if m == "keyfile" || m == "passkey" {
        kids.push(file_row(t, f.keyfile.as_ref().map(|f| f.name.as_str()), k("unlock.noKeyfile"), k("unlock.chooseKeyfile"), json!(["pickKey"])));
    }
    if m == "fido" {
        kids.push(view(json!({"gap":10}), vec![
            view(json!({"flexDirection":"row","gap":10,"alignItems":"center","paddingHorizontal":12,"paddingVertical":10,"borderRadius":8,"backgroundColor":t.control()}), vec![
                glyph("key", 16., &t.s(t.accent())),
                text(k(if app.touch { "fido.touch" } else { "fido.insert" }), json!({"flex":1,"fontSize":12,"color":t.text(),"lineHeight":17})),
            ]),
            input("pin", &f.pin, k("fido.pinOptional"), true, false, Some("go"), json!({})),
        ]));
    }
    if m == "recovery" {
        kids.push(input("rec", &f.rec, json!("XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX"), false, false, Some("go"), json!({"direction":"ltr"})));
    }
    kids.push(error_line(&f.err, true));
    let label = if f.busy { k(if app.touch { "fido.touchShort" } else { "unlock.working" }) } else { k("unlock.button") };
    kids.push(button(t, label, json!(["go"]), Btn { primary: true, ..Btn::default() }));
    if let Some(ms) = &f.ms {
        kids.push(text(kp("unlock.took", json!({"ms":ms})), json!({"fontSize":11,"color":t.dim(),"textAlign":"center"})));
    }
    if app.status["main"]["unreadable"] == true {
        kids.push(warning(t, k("unlock.unreadable"), false));
    }
    shell(t, false, kids)
}

pub const CREATE_METHODS: [&str; 4] = ["password", "fido", "keyfile", "passkey"];

pub fn create(app: &App, t: &Theme) -> Value {
    let f = &app.lock;
    if let Some(code) = &f.recovery {
        return recovery(app, t, code);
    }
    let m = f.method.as_str();
    let need_pw = m == "password" || m == "passkey";
    let need_key = m == "keyfile" || m == "passkey";
    let opts: Vec<(&str, Value)> = CREATE_METHODS.iter().map(|m| (*m, k(&format!("method.{m}")))).collect();
    let mut kids = vec![
        head(t, padlock(t, 60., true), k("create.title"), k("create.subtitle")),
        view(json!({"gap":8,"marginTop":4}), vec![
            label(t, k("create.how")),
            segmented(t, "method", m, &opts, true, true, false),
            hint(t, k(&format!("create.about.{m}")), None),
        ]),
    ];
    if need_pw {
        let mut pws = vec![
            input("pw", &f.pw, k("create.password"), true, true, None, json!({})),
            input("pw2", &f.pw2, k("create.repeat"), true, false, Some("go"), json!({})),
        ];
        if let Some(score) = &f.score {
            pws.push(strength_bar(t, score["score"].as_i64().unwrap_or(0), &score["bits"]));
        }
        kids.push(view(json!({"gap":8}), pws));
    }
    if need_key {
        kids.push(file_row(t, f.keyfile.as_ref().map(|f| f.name.as_str()), k("create.keyWhere"), k("create.chooseKeyPath"), json!(["pickKey"])));
    }
    if m == "fido" {
        kids.push(view(json!({"gap":8}), vec![
            hint(t, k(if app.touch { "fido.touch" } else { "fido.insertCreate" }), None),
            input("pin", &f.pin, k("fido.pinOptional"), true, false, None, json!({})),
        ]));
    }
    kids.push(error_line(&f.err, false));
    let label = if f.busy { k(if app.touch { "fido.touchShort" } else { "create.working" }) } else { k("create.button") };
    kids.push(button(t, label, json!(["go"]), Btn { primary: true, ..Btn::default() }));
    kids.push(text(k("create.local"), json!({"fontSize":11,"color":t.dim(),"textAlign":"center","lineHeight":15})));
    shell(t, true, kids)
}

fn recovery(app: &App, t: &Theme, code: &str) -> Value {
    recovery_body(t, code, app.lock.confirmed, "confirm", "done")
}

/// The recovery key to keep: the code, Save, the confirmation and Done (also in Settings).
pub fn recovery_body(t: &Theme, code: &str, confirmed: bool, confirm: &str, done: &str) -> Value {
    let pic = view(json!({"width":56,"height":56,"borderRadius":14,"alignItems":"center","justifyContent":"center","background":"linear-gradient(180deg, #38c6d4, #1f6fd1)"}),
        vec![glyph("key", 32., "#fff")]);
    shell(t, true, vec![
        head(t, pic, k("recovery.title"), k("recovery.body")),
        view(json!({"paddingHorizontal":16,"paddingVertical":14,"borderRadius":10,"border":format!("1px solid {}", t.s(t.sep())),
            "backgroundColor":if t.dark() {"rgba(255,255,255,0.05)"} else {"rgba(0,0,0,0.03)"},"alignItems":"center","direction":"ltr"}), vec![
            text(json!(code), json!({"fontSize":15,"fontWeight":600,"color":t.text(),"fontFamily":"monospace","textAlign":"center","lineHeight":22})),
        ]),
        view(json!({"flexDirection":"row","gap":8,"justifyContent":"center"}), vec![
            button(t, k("recovery.save"), json!(["saveRecovery", code]), Btn { small: true, ..Btn::default() }),
        ]),
        check(t, confirmed, k("recovery.confirm"), json!([confirm]), None),
        button(t, k("recovery.done"), json!([done]), Btn { primary: true, disabled: !confirmed, ..Btn::default() }),
    ])
}
