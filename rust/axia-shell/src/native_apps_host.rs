//! Session app-control service. Native windows render through native_view;
//! this JSON bridge supplies grouping/force-quit policy to shell menus as well.
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
pub enum NativeApp {
    Control(crate::forcequit::App),
    Settings(crate::settings_model::Model),
}
impl NativeApp {
    fn new(id: &[u8]) -> Option<Self> {
        match id {
            b"forcequit" => Some(Self::Control(Default::default())),
            b"settings" => Some(Self::Settings(Default::default())),
            _ => None,
        }
    }
    fn request(&mut self, input: &Value) -> Value {
        match self {
            Self::Control(control) if input["cmd"] != "catalog" => control.request(input),
            Self::Control(_) => crate::native_apps::manifests(),
            Self::Settings(model) => model.request(input),
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
