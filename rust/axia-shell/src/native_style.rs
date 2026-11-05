//! Toolkit-independent native styling. Adwaita/GTK 3 is the visual reference;
//! Axia keeps its renderer, filesystem workers and bounded widget delivery.
use serde_json::{json, Value};

pub fn files_theme(source: &Value) -> Value {
    if source["nativeAppearance"] == "axia"
        || (source.get("nativeAppearance").is_none() && source["filesAppearance"] == "axia")
    {
        return source.clone();
    }
    let mut out = if source.is_object() {
        source.clone()
    } else {
        json!({})
    };
    let dark = source["mode"] != "light";
    let colors = if dark {
        json!({
            "text":"#eeeeec","textDim":"#aaaaaa","windowBg":"#2d2d2d",
            "sidebarBg":"#303030","titleBg":"#353535","windowBorder":"#1b1b1b",
            "separator":"#202020","hover":"rgba(255,255,255,0.09)","control":"#404040",
            "inputBg":"#252525","popupBg":"#353535"
        })
    } else {
        json!({
            "text":"#2e3436","textDim":"#686c6d","windowBg":"#ffffff",
            "sidebarBg":"#f6f5f4","titleBg":"#e6e5e3","windowBorder":"#b6b5b3",
            "separator":"#d5d4d2","hover":"rgba(0,0,0,0.07)","control":"#e8e7e5",
            "inputBg":"#ffffff","popupBg":"#f6f5f4"
        })
    };
    out.as_object_mut()
        .unwrap()
        .extend(colors.as_object().unwrap().clone());
    out.as_object_mut().unwrap().extend(
        json!({
            "nativeStyle":"adwaita","controlRadius":4,"controlRadiusSmall":3,
        "groupRadius":5,"dialogRadius":6,"menuRadius":4,"menuItemRadius":2,"buttonBorder":true,
            "menuHighlight":"accent","selection":"fill","inputFocus":"ring",
            "radius":5,"contextMenuRadius":4,"contextMenuItemRadius":2,
            "mode":if dark{"dark"}else{"light"}
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    if !out["accent"].is_string() {
        out["accent"] = json!("#3584e4");
    }
    // Visual geometry changes must never turn off the user's glass. The
    // compositor's Rust/C backdrop path and its caches remain untouched.
    if source["effects"] != false {
        out["popupBg"] = json!(if dark {
            "rgba(53,53,53,0.82)"
        } else {
            "rgba(246,245,244,0.84)"
        });
    }
    out["panelBg"] = source
        .get("panelBg")
        .cloned()
        .unwrap_or_else(|| out["popupBg"].clone());
    out
}

// A declarative frame plan, reusable by other apps when the pilot is approved.
// The host binds its action names to the existing window manager only. Heights
// stay identical to Axia's current chrome, preserving content/window geometry.
pub fn frame(theme: &Value) -> Value {
    if theme["nativeStyle"] != "adwaita" {
        return Value::Null;
    }
    let dark = theme["mode"] != "light";
    json!({
        "schema":1,"tokens":theme,"radius":6,
        "header":{"height":40,"flexDirection":"row","alignItems":"center",
            "background":if dark{"linear-gradient(180deg, #3b3b3b, #303030)"}else{"linear-gradient(180deg, #eae9e7, #deddda)"}},
        "title":{"paddingHorizontal":112,"textOverflow":"ellipsis","numberOfLines":1,"overflow":"hidden"},
        "controls":{"position":"absolute","right":8,"top":6,"flexDirection":"row","gap":4,"alignItems":"center"},
        "shadowFocused":"0 6px 16px rgba(0,0,0,0.35)",
        "shadowInactive":"0 3px 8px rgba(0,0,0,0.22)",
        "button":{"width":28,"height":28,"borderRadius":4,"alignItems":"center","justifyContent":"center","backgroundColor":"transparent"},
        "buttonHover":{"backgroundColor":theme["hover"]},
        "glyph":{"fontSize":17,"lineHeight":24,"fontWeight":400},
        "buttons":[{"key":"min","glyph":"−"},{"key":"max","glyph":"□"},{"key":"close","glyph":"×"}]
    })
}

// Pure style transport: no filesystem/session initialization, workers or polling.
// SAFETY: the C bridge supplies a valid NUL-terminated string for this call.
#[no_mangle]
pub unsafe extern "C" fn axia_native_style_request(
    input: *const std::ffi::c_char,
) -> *mut std::ffi::c_char {
    use std::ffi::{CStr, CString};
    let reply = if input.is_null() {
        Value::Null
    } else {
        let bytes = CStr::from_ptr(input).to_bytes();
        if bytes.len() > 16 * 1024 {
            Value::Null
        } else if let Ok(source) = serde_json::from_slice::<Value>(bytes) {
            let tokens = files_theme(&source);
            json!({"tokens":tokens,"chrome":frame(&tokens)})
        } else {
            Value::Null
        }
    };
    CString::new(reply.to_string()).unwrap().into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_style_preserves_shell_accent_and_legacy_tokens() {
        let source = json!({"mode":"light","accent":"#9141ac","windowBg":"#abcdef","filesAppearance":"adwaita"});
        let before = source.clone();
        let local = files_theme(&source);
        assert_eq!(source, before);
        assert_eq!(local["accent"], source["accent"]);
        assert_eq!(local["windowBg"], "#ffffff");
        let old = json!({"mode":"dark","filesAppearance":"axia","windowBg":"#123456"});
        assert_eq!(files_theme(&old), old);
        assert!(frame(&old).is_null());
    }
    #[test]
    fn shared_style_preserves_glass_and_obeys_global_rollback() {
        let source = json!({"mode":"dark","effects":true,"blur":24,"panelBg":"rgba(36,36,42,0.72)","nativeAppearance":"adwaita"});
        let styled = files_theme(&source);
        assert_eq!(styled["effects"], true);
        assert_eq!(styled["blur"], 24);
        assert_eq!(styled["panelBg"], source["panelBg"]);
        assert!(styled["popupBg"].as_str().unwrap().starts_with("rgba("));
        let off = files_theme(&json!({"effects":false}));
        assert_eq!(off["effects"], false);
        assert_eq!(off["popupBg"], "#353535");
        let old =
            json!({"nativeAppearance":"axia","filesAppearance":"adwaita","effects":true,"blur":18});
        assert_eq!(files_theme(&old), old);
        assert!(frame(&old).is_null());
    }
    #[test]
    fn traditional_frame_keeps_geometry_and_action_semantics() {
        let plan = frame(&files_theme(&json!({"mode":"dark"})));
        assert_eq!(plan["header"]["height"], 40);
        assert_eq!(
            plan["buttons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["key"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["min", "max", "close"]
        );
        assert!(plan["controls"].get("right").is_some());
        assert!(plan["header"].get("backdropBlur").is_none());
    }
}
