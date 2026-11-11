//! App models and session app-control service. Native windows render through
//! native_view; transitional JSX adapters call the same compiled model code
//! through this bounded JSON bridge until their scenes migrate too.
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
pub enum NativeApp {
    Control(crate::forcequit::App),
    Settings(crate::settings_model::Model),
    Terminal,
    Passwords,
    Sysinfo,
}
impl NativeApp {
    fn new(id: &[u8]) -> Option<Self> {
        match id {
            b"forcequit" => Some(Self::Control(Default::default())),
            b"settings" => Some(Self::Settings(Default::default())),
            b"terminal" => Some(Self::Terminal),
            b"passwords" => Some(Self::Passwords),
            b"sysinfo" => Some(Self::Sysinfo),
            _ => None,
        }
    }
    fn request(&mut self, input: &Value) -> Value {
        match self {
            Self::Control(control) if input["cmd"] != "catalog" => control.request(input),
            Self::Control(_) => crate::native_apps::manifests(),
            Self::Settings(model) => model.request(input),
            Self::Terminal => crate::terminal_model::request(input),
            Self::Passwords => crate::passwords_model::request(input),
            Self::Sysinfo => crate::sysinfo_model::request(input),
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
