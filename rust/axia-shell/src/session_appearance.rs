//! Resident integration with the toolkit portal, independent of React providers.
use serde_json::Value;
use std::cell::Cell;
use std::ffi::CStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    dark: bool,
    rgb: [u8; 3],
    name: &'static CStr,
    external: bool,
}
impl Plan {
    pub(super) fn from_profile(profile: &Value) -> Self {
        let accent = profile["accent"].as_str();
        let hex = crate::native_style::shell_theme::accent(accent);
        let color = u32::from_str_radix(&hex[1..], 16).expect("compiled shell palette");
        Self {
            dark: profile["mode"] != "light",
            rgb: [(color >> 16) as u8, (color >> 8) as u8, color as u8],
            name: match accent.unwrap_or("") {
                "violeta" => c"purple", "rosa" => c"pink", "naranja" => c"orange",
                "verde" => c"green", "grafito" => c"slate", "rojo" => c"red",
                "amarillo" => c"yellow", "turquesa" => c"teal", _ => c"blue",
            },
            external: profile["externalWindowStyle"] == true,
        }
    }
    fn buttons(&self) -> &'static CStr {
        if self.external { c"close,minimize,maximize:" } else { c":minimize,maximize,close" }
    }
}

// External application files are process-wide. Recreating the session's adapter
// must not repeat their synchronization; a color-only change does not affect them.
thread_local! { static EXTERNAL: Cell<Option<(bool, bool)>> = const { Cell::new(None) }; }

#[derive(Default)]
pub(super) struct Service { last: Option<Plan> }
impl Service {
    pub(super) fn sync(&mut self, plan: Plan) {
        if self.last == Some(plan) { return; }
        let style = (plan.external, plan.dark);
        let external_changed = EXTERNAL.with(|last| last.replace(Some(style)) != Some(style));
        apply(plan, external_changed);
        self.last = Some(plan);
    }
}

#[cfg(not(test))]
fn apply(plan: Plan, external_changed: bool) {
    extern "C" {
        fn portal_set(dark: i32, rgb: *const f64, name: *const std::ffi::c_char,
            buttons: *const std::ffi::c_char, external: i32);
    }
    let rgb = plan.rgb.map(|channel| f64::from(channel) / 255.);
    // C consumes the borrowed RGB array and static NUL-terminated strings now;
    // it retains no Rust references. No JS/JSON is involved in applying the plan.
    unsafe { portal_set(plan.dark.into(), rgb.as_ptr(), plan.name.as_ptr(),
        plan.buttons().as_ptr(), plan.external.into()); }
    if external_changed { crate::external_style::axia_external_style(plan.external, plan.dark); }
}

#[cfg(test)]
thread_local! { static APPLIED: std::cell::RefCell<Vec<(Plan, bool)>> = const { std::cell::RefCell::new(Vec::new()) }; }
#[cfg(test)]
fn apply(plan: Plan, external_changed: bool) { APPLIED.with(|calls| calls.borrow_mut().push((plan, external_changed))); }

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn portal_uses_the_shared_palette_and_native_preferences() {
        for (key, rgb, name) in [
            ("azul", [10,132,255], c"blue"), ("violeta", [191,90,242], c"purple"),
            ("rosa", [255,55,95], c"pink"), ("naranja", [255,159,10], c"orange"),
            ("verde", [48,209,88], c"green"), ("grafito", [142,142,147], c"slate"),
        ] {
            let plan = Plan::from_profile(&json!({"mode":"light","accent":key,"externalWindowStyle":false}));
            assert_eq!(plan.rgb, rgb); assert_eq!(plan.name, name); assert!(!plan.dark);
            assert_eq!(plan.buttons(), c":minimize,maximize,close");
        }
        let legacy = Plan::from_profile(&json!({"accent":"rojo","externalWindowStyle":true}));
        assert_eq!(legacy.rgb, [10,132,255]); assert_eq!(legacy.name, c"red");
        assert!(legacy.dark); assert_eq!(legacy.buttons(), c"close,minimize,maximize:");
        let invalid = Plan::from_profile(&json!({"accent":{"untrusted":true},"externalWindowStyle":"true"}));
        assert_eq!(invalid.rgb, [10,132,255]); assert_eq!(invalid.name, c"blue"); assert!(!invalid.external);
    }
    #[test]
    fn revisions_only_apply_changed_portal_fields_and_restyle_on_mode_or_layout() {
        EXTERNAL.with(|last| last.set(None)); APPLIED.with(|calls| calls.borrow_mut().clear());
        let mut service = Service::default();
        let mut profile = json!({"mode":"dark","accent":"azul","externalWindowStyle":false});
        service.sync(Plan::from_profile(&profile));
        for i in 0..100 { profile["unrelated"] = json!(i); service.sync(Plan::from_profile(&profile)); }
        profile["accent"] = json!("violeta"); service.sync(Plan::from_profile(&profile));
        profile["mode"] = json!("light"); service.sync(Plan::from_profile(&profile));
        profile["externalWindowStyle"] = json!(true); service.sync(Plan::from_profile(&profile));
        APPLIED.with(|calls| {
            let calls = calls.borrow(); assert_eq!(calls.len(), 4);
            assert_eq!(calls.iter().map(|(_, external)| *external).collect::<Vec<_>>(), [true,false,true,true]);
        });
        drop(service);
        Service::default().sync(Plan::from_profile(&profile));
        APPLIED.with(|calls| { let calls = calls.borrow(); assert_eq!(calls.len(), 5);
            assert!(!calls.last().unwrap().1, "reattachment must not rewrite external application files"); });
    }
}
