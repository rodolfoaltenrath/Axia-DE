//! App models and session app-control service. Native windows render through
//! native_view; transitional JSX adapters call the same compiled model code
//! through this bounded JSON bridge until their scenes migrate too. The Force
//! Quit controller also answers catalog and icon queries for application menus.
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
#[path = "../../../apps/pdf/model.rs"]
mod pdf_model;
pub enum NativeApp {
    Control(crate::forcequit::App),
    Settings(crate::settings_model::Model),
    Terminal,
    Passwords,
    Sysinfo,
    Pdf(pdf_model::Model),
}
impl NativeApp {
    fn new(id: &[u8]) -> Option<Self> {
        match id {
            b"forcequit" => Some(Self::Control(Default::default())),
            b"settings" => Some(Self::Settings(Default::default())),
            b"terminal" => Some(Self::Terminal),
            b"passwords" => Some(Self::Passwords),
            b"sysinfo" => Some(Self::Sysinfo),
            b"pdf" => Some(Self::Pdf(Default::default())),
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
            Self::Pdf(model) => model.request(input),
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
    fn pdf_models_are_available_through_the_ffi_and_have_independent_lifetimes() {
        unsafe {
            let name = CString::new("pdf").unwrap();
            let a = axia_native_app_new(name.as_ptr());
            let b = axia_native_app_new(name.as_ptr());
            assert!(!a.is_null() && !b.is_null());
            let call = |app, value: Value| {
                let text = CString::new(value.to_string()).unwrap();
                let reply = axia_native_app_request(app, text.as_ptr());
                assert!(!reply.is_null());
                let value: Value =
                    serde_json::from_slice(CStr::from_ptr(reply).to_bytes()).unwrap();
                axia_native_app_reply_free(reply);
                value
            };
            call(a, json!({"cmd":"cache","op":"invalidate","page":0}));
            let key = json!({"cmd":"cache","op":"key","page":0,"width":48,"rot":0,"inv":false});
            assert_eq!(call(a, key.clone())["value"], "0|64|0|0|1");
            assert_eq!(call(b, key.clone())["value"], "0|64|0|0|0");
            let ticket = call(a, json!({"cmd":"viewer","op":"findStart","query":"same"}));
            call(b, json!({"cmd":"viewer","op":"reset"}));
            assert_eq!(
                call(
                    a,
                    json!({"cmd":"viewer","op":"findAppend","token":ticket["token"],"page":0,"rects":[[1,2,3,4]]})
                )["go"]["page"],
                0
            );
            call(a, json!({"cmd":"edit","op":"open","pages":2}));
            call(b, json!({"cmd":"edit","op":"open","pages":2}));
            let mutation = call(
                a,
                json!({"cmd":"edit","op":"annotate","annotation":{"page":0,"type":"highlight","rects":[[1,2,3,4]],"color":"#ffd60a"}}),
            );
            let finish = json!({"cmd":"edit","op":"complete","ticket":mutation["effect"]["ticket"],"ok":true});
            assert!(call(b, finish.clone()).is_null());
            assert_eq!(call(a, finish)["state"]["history"], json!([0]));
            axia_native_app_free(a);
            assert_eq!(call(b, key)["value"], "0|64|0|0|0");
            assert_eq!(
                call(
                    b,
                    json!({"cmd":"toView","point":[10,20],"size":[100,200],"rot":1,"scale":2})
                ),
                json!([360., 20.])
            );
            axia_native_app_free(b);
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
