//! App models and session app-control service. Native windows render through
//! native_view; transitional JSX adapters call the same compiled model code
//! through this bounded JSON bridge until their scenes migrate too. The Force
//! Quit controller also answers catalog and icon queries for application menus.
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
pub enum NativeApp {
    Control(crate::app_control::Controller),
    Settings(crate::settings_model::Model),
    Terminal,
    Passwords,
    Sysinfo,
    Notes,
    Screenshots(axia_app_screenshots::Model),
}
impl NativeApp {
    fn new(id: &[u8]) -> Option<Self> {
        crate::native_view::install_services();
        match id {
            b"forcequit" => Some(Self::Control(Default::default())),
            b"settings" => Some(Self::Settings(Default::default())),
            b"terminal" => Some(Self::Terminal),
            b"passwords" => Some(Self::Passwords),
            b"sysinfo" => Some(Self::Sysinfo),
            b"notes" => Some(Self::Notes),
            b"screenshots" => Some(Self::Screenshots(Default::default())),
            _ => None,
        }
    }
    fn request(&mut self, input: &Value) -> Value {
        match self {
            Self::Control(_) if input["cmd"] == "catalog" => crate::native_apps::manifests(),
            Self::Control(_) if input["cmd"] == "icon" => match input["app"].as_str() {
                Some(id) => crate::native_view::app(id).map_or_else(
                    || json!({"error":"app.invalidRequest"}),
                    |mut app| app.request(input),
                ),
                None => json!({"error":"app.invalidRequest"}),
            },
            Self::Control(control) => control.request(input),
            Self::Settings(model) => model.request(input),
            Self::Terminal => crate::terminal_model::request(input),
            Self::Passwords => crate::passwords_model::request(input),
            Self::Sysinfo => crate::sysinfo_model::request(input),
            Self::Notes => crate::notes_app::store::background(input),
            Self::Screenshots(model) => model.request(input),
        }
    }
}

// The host owns this opaque app through a QuickJS finalizer. No JS values or process IDs
// are retained. Effects use existing session APIs, whose Wayland credential checks remain in force.
#[no_mangle]
pub unsafe extern "C" fn axia_native_app_new(id: *const c_char) -> *mut NativeApp {
    if id.is_null() {
        return std::ptr::null_mut();
    }
    let Some(app) = NativeApp::new(CStr::from_ptr(id).to_bytes()) else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(app))
}
/// Session controller bound to the authoritative windows of this context.
/// The C/QuickJS owner is borrowed only during this call; Rust retains a reader.
#[no_mangle]
pub unsafe extern "C" fn axia_native_app_new_windows(
    id: *const c_char,
    windows: *const crate::winmodel::Session,
) -> *mut NativeApp {
    if id.is_null() || windows.is_null() || CStr::from_ptr(id).to_bytes() != b"forcequit" {
        return std::ptr::null_mut();
    }
    crate::native_view::install_services();
    let (assets, home) = crate::native_view::resource_roots();
    crate::clock_settings::bind(home.as_deref().unwrap_or(""));
    (*windows).notification_service().borrow_mut().bind_settings(home.as_deref().unwrap_or(""));
    (*windows).restore_service().borrow_mut().bind_settings(home.as_deref().unwrap_or(""));
    let mut control = crate::app_control::Controller::new((*windows).clone());
    control.bind_settings(home.as_deref().unwrap_or(""));
    (*windows).bind_preference_publications(home.as_deref().unwrap_or(""), &assets);
    (*windows).activate_services();
    (*windows).activate_clock_and_notifications();
    Box::into_raw(Box::new(NativeApp::Control(control)))
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_app_free(app: *mut NativeApp) {
    if !app.is_null() {
        drop(Box::from_raw(app));
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_app_request(
    app: *mut NativeApp,
    input: *const c_char,
) -> *mut c_char {
    let reply = match (app.as_mut(), input.is_null()) {
        (Some(app), false) => {
            let bytes = CStr::from_ptr(input).to_bytes();
            if bytes.len() > 1024 * 1024 {
                json!({"error":"app.invalidRequest"})
            } else {
                match serde_json::from_slice(bytes) {
                    Ok(value) => app.request(&value),
                    Err(_) => json!({"error":"app.invalidRequest"}),
                }
            }
        }
        _ => json!({"error":"app.invalidRequest"}),
    };
    CString::new(reply.to_string()).unwrap().into_raw()
}
/// Only the root controller exposes this drain. Quiet reads allocate no JSON.
#[no_mangle]
pub unsafe extern "C" fn axia_native_app_take_services(app: *const NativeApp) -> *mut c_char {
    let effects = match app.as_ref() {
        Some(NativeApp::Control(control)) => control.take_services(),
        _ => None,
    };
    effects.and_then(|effects| CString::new(effects.to_string()).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw)
}
#[no_mangle]
pub unsafe extern "C" fn axia_native_app_reply_free(reply: *mut c_char) {
    if !reply.is_null() {
        drop(CString::from_raw(reply));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bound_controllers_borrow_their_context_and_ignore_legacy_window_payloads() {
        unsafe {
            let mut a = crate::winmodel::Session::default();
            let mut b = crate::winmodel::Session::default();
            for (session, instance) in [(&mut a, "a"), (&mut b, "b")] {
                session.call("open", &[json!({}), json!({"app":"notes","instance":instance})]).unwrap();
            }
            let first = axia_native_app_new_windows(c"forcequit".as_ptr(), &a);
            let second = axia_native_app_new_windows(c"forcequit".as_ptr(), &b);
            assert!(!first.is_null() && !second.is_null());
            assert!(axia_native_app_new_windows(c"settings".as_ptr(), &a).is_null());
            assert!(axia_native_app_new_windows(c"forcequit".as_ptr(), std::ptr::null()).is_null());
            let call = |app, input: Value| {
                let input = CString::new(input.to_string()).unwrap();
                let reply = axia_native_app_request(app, input.as_ptr());
                let value: Value = serde_json::from_slice(CStr::from_ptr(reply).to_bytes()).unwrap();
                axia_native_app_reply_free(reply);value
            };
            for app in [first, second] {
                assert_eq!(call(app, json!({"cmd":"metadata","registry":[{"id":"notes","name":"Notes"}],"catalog":[]}))["ok"], true);
            }
            let groups = json!({"cmd":"groups","windows":[{"id":"stale","app":"notes","key":"notes"}]});
            assert_eq!(call(first, groups.clone())["groups"][0]["windows"], json!(["notes#a"]));
            assert_eq!(call(second, groups.clone())["groups"][0]["windows"], json!(["notes#b"]));
            a.call("close", &[json!({}), json!("notes#a")]).unwrap();
            a.call("open", &[json!({}), json!({"app":"notes","instance":"fresh"})]).unwrap();
            assert_eq!(call(first, json!({"cmd":"quit","key":"notes"}))["effect"], json!({"kind":"closeWindow","id":"notes#fresh"}));
            drop(a);
            assert_eq!(call(first, groups.clone())["groups"][0]["windows"], json!(["notes#fresh"]));
            assert_eq!(call(second, groups)["groups"][0]["windows"], json!(["notes#b"]));
            axia_native_app_free(first);axia_native_app_free(second);
        }
    }
    fn numeric_values(value: &Value) -> Value {
        match value {
            Value::Number(n) => json!(n.as_f64().unwrap()),
            Value::Array(a) => json!(a.iter().map(numeric_values).collect::<Vec<_>>()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| (k.clone(), numeric_values(v)))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
    #[test]
    fn models_match_pre_migration_javascript_reference_cases() {
        let cases: Value =
            serde_json::from_str(include_str!("../tests/fixtures/app_models.json")).unwrap();
        for (index, case) in cases.as_array().unwrap().iter().enumerate() {
            let mut model = NativeApp::new(case["app"].as_str().unwrap().as_bytes()).unwrap();
            let got = model.request(&case["input"]);
            assert_eq!(
                numeric_values(&got),
                numeric_values(&case["expected"]),
                "reference case {index}: {}",
                case["input"]
            );
        }
    }
}
