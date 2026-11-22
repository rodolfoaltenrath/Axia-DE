//! The shell's palette and glass materials. No scene, profile copies or timers.
use serde_json::{json, Value};

pub(crate) const ACCENTS: [(&str, &str); 6] = [
    ("azul", "#0a84ff"), ("violeta", "#bf5af2"), ("rosa", "#ff375f"),
    ("naranja", "#ff9f0a"), ("verde", "#30d158"), ("grafito", "#8e8e93"),
];

// Preserve the old theme's treatment of optional values, including effects
// objects accepted by callers. Glass defaults on unless explicitly false;
// panel opacity/blur follow truthiness, as before.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|v| v != 0.),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}

fn rgba(color: u32) -> String {
    let alpha = color >> 24;
    format!("rgba({},{},{},{}.{:04})", (color >> 16) & 255,
        (color >> 8) & 255, color & 255, alpha / 255,
        ((alpha % 255) * 10000 + 127) / 255)
}

pub(crate) fn accent(name: Option<&str>) -> &'static str {
    ACCENTS.iter().find(|(key, _)| Some(*key) == name).unwrap_or(&ACCENTS[0]).1
}

pub(crate) fn tokens(source: &Value) -> Value {
    let dark = source["mode"] == "dark";
    let effects = truthy(&source["effects"]);
    let accent = accent(source["accent"].as_str());
    let mut result = json!({
        "accent":accent,
        // Pairs retain the established order of controls across JSON's sorted maps.
        "accents":ACCENTS,
        "text":if dark {"#f5f5f7"} else {"#1d1d1f"},
        "textDim":if dark {"rgba(235,235,245,0.55)"} else {"rgba(60,60,67,0.6)"},
        "windowBg":if dark {"#1f1f23"} else {"#ffffff"},
        "sidebarBg":if dark {"#26262b"} else {"#f3f3f6"},
        "separator":if dark {"rgba(255,255,255,0.08)"} else {"rgba(0,0,0,0.08)"},
        "hover":if dark {"rgba(255,255,255,0.07)"} else {"rgba(0,0,0,0.05)"},
        "control":if dark {"#3a3a40"} else {"#e3e3e8"},
        "inputBg":if dark {"#2a2a2f"} else {"#ffffff"},
        "menuBg":if dark {"rgba(24,24,30,0.42)"} else {"rgba(250,250,252,0.55)"},
        "menuText":if dark {"#f5f5f7"} else {"#1d1d1f"},
        "menuHover":if dark {"rgba(255,255,255,0.16)"} else {"rgba(0,0,0,0.08)"},
        "panelBg":match (dark,effects) {
            (true,true)=>"rgba(36,36,42,0.72)", (true,false)=>"#2a2a30",
            (false,true)=>"rgba(246,246,248,0.78)", (false,false)=>"#f2f2f5",
        },
        "windowBorder":if dark {"rgba(255,255,255,0.12)"} else {"rgba(0,0,0,0.14)"},
        "titleBg":if dark {"#2b2b30"} else {"#ececef"},
        "dockBg":rgba(crate::glass::axia_glass_tint(dark, source["effects"] != false)),
        "dockBorder":rgba(crate::glass::axia_glass_outline(dark)),
        "lightOff":if dark {"#4a4a50"} else {"#c8c8cc"},
        "scrollThumb":if dark {"rgba(255,255,255,0.32)"} else {"rgba(0,0,0,0.3)"},
        "scrollThumbHover":if dark {"rgba(255,255,255,0.5)"} else {"rgba(0,0,0,0.45)"},
        "blur":if effects {24} else {0},
    });
    // Missing optional fields stay missing, rather than manufacturing a mode.
    for key in ["mode", "effects"] {
        if let Some(value) = source.get(key) { result[key] = value.clone(); }
    }
    result
}

/// The Dock material used by shell overlays, with the same glass/effects policy.
pub(crate) fn dock_surface(theme: &Value) -> Value {
    let dark = theme["mode"] == "dark";
    let effects = theme["effects"] != false;
    let material = rgba(crate::glass::axia_glass_tint(dark, effects));
    let background = if effects { theme["dockBg"].as_str().filter(|s| !s.is_empty()).unwrap_or(&material) } else { &material };
    let outline = rgba(crate::glass::axia_glass_outline(dark));
    let border = theme["dockBorder"].as_str().filter(|s| !s.is_empty()).unwrap_or(&outline);
    let blur = if effects { theme["blur"].as_f64().unwrap_or(24.) } else { 0. };
    json!({"backgroundColor":background,"backdropFilter":format!("blur({blur}px)"),"border":format!("1px solid {border}")})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_keeps_accent_order_and_unknown_fallback() {
        for (name, color) in ACCENTS {
            let plan = tokens(&json!({"mode":"dark","accent":name,"effects":true}));
            assert_eq!(plan["accent"], color);
            assert_eq!(plan["accents"], json!(ACCENTS));
        }
        for value in [Value::Null, json!("#abcdef"), json!({}), json!("unknown")] {
            assert_eq!(tokens(&json!({"accent":value}))["accent"], "#0a84ff");
        }
    }
    #[test]
    fn materials_use_the_compositors_alpha_without_locale_dependent_numbers() {
        for (mode, bg, border, opaque) in [
            ("dark", "rgba(46,46,56,0.3765)", "rgba(255,255,255,0.2196)", "rgba(42,42,48,1.0000)"),
            ("light", "rgba(255,255,255,0.3412)", "rgba(255,255,255,0.6510)", "rgba(242,242,245,1.0000)"),
        ] {
            let on = tokens(&json!({"mode":mode,"effects":true}));
            let off = tokens(&json!({"mode":mode,"effects":false}));
            assert_eq!(on["dockBg"], bg); assert_eq!(on["dockBorder"], border);
            assert_eq!(off["dockBg"], opaque); assert_eq!(off["dockBorder"], border);
            assert_eq!(on["blur"], 24); assert_eq!(off["blur"], 0);
        }
    }
    #[test]
    fn optional_effects_keep_existing_panel_and_glass_semantics() {
        for value in [json!(false), Value::Null, json!(0), json!("")] {
            let plan = tokens(&json!({"mode":"dark","effects":value}));
            assert_eq!(plan["blur"], 0); assert_eq!(plan["panelBg"], "#2a2a30");
            assert_eq!(plan["dockBg"], if value == false {"rgba(42,42,48,1.0000)"} else {"rgba(46,46,56,0.3765)"});
        }
        for value in [json!(true), json!({}), json!([]), json!(1), json!("on")] {
            assert_eq!(tokens(&json!({"effects":value}))["blur"], 24);
        }
        let absent = tokens(&json!({}));
        assert!(absent.get("mode").is_none()); assert!(absent.get("effects").is_none());
        assert_eq!(absent["windowBg"], "#ffffff");
    }
    #[test]
    fn overlay_material_preserves_glass_and_opaque_policy() {
        for (mode, opaque) in [("dark","rgba(42,42,48,1.0000)"),("light","rgba(242,242,245,1.0000)")] {
            let mut theme = tokens(&json!({"mode":mode,"effects":true}));
            let on = dock_surface(&theme);
            assert_eq!(on["backgroundColor"], theme["dockBg"]);
            assert_eq!(on["backdropFilter"], "blur(24px)");
            theme["dockBg"] = json!("#abcdef"); theme["dockBorder"] = json!("#112233");
            assert_eq!(dock_surface(&theme)["backgroundColor"], "#abcdef");
            theme["effects"] = json!(false);
            let off = dock_surface(&theme);
            assert_eq!(off["backgroundColor"], opaque);
            assert_eq!(off["backdropFilter"], "blur(0px)");
            assert_eq!(off["border"], "1px solid #112233");
        }
    }
    #[test]
    fn only_palette_inputs_enter_the_result() {
        let source = json!({"mode":"light","effects":false,"wallpapers":["large"],"windows":[1],"settings":{"secret":true}});
        let plan = tokens(&source);
        assert!(plan.get("wallpapers").is_none()); assert!(plan.get("windows").is_none());
        assert!(plan.get("settings").is_none());
        assert_eq!(plan["text"], "#1d1d1f"); assert_eq!(plan["panelBg"], "#f2f2f5");
    }
}
