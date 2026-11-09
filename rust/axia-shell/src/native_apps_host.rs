//! Shared ownership/FFI for compiled applications. Application behavior stays in apps/<id>.
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
pub enum NativeApp {
    ForceQuit(crate::forcequit::App),
    Calculator(crate::calculator::App),
    AppAbout(crate::appabout::App),
    Monitor(crate::monitor::App),
}
impl NativeApp {
    fn new(id: &[u8]) -> Option<Self> {
        if id == crate::native_apps::forcequit::ID.as_bytes() {
            Some(Self::ForceQuit(Default::default()))
        } else if id == crate::native_apps::calculator::ID.as_bytes() {
            Some(Self::Calculator(Default::default()))
        } else if id == crate::native_apps::appabout::ID.as_bytes() {
            Some(Self::AppAbout(Default::default()))
        } else if id == crate::native_apps::monitor::ID.as_bytes() {
            Some(Self::Monitor(Default::default()))
        } else {
            None
        }
    }
    fn request(&mut self, input: &Value) -> Value {
        if input["cmd"] == "catalog" {
            return crate::native_apps::manifests();
        }
        if input["cmd"] == "icon" {
            if let Some(id) = input["app"].as_str() {
                return Self::new(id.as_bytes()).map_or_else(
                    || json!({"error":"app.invalidRequest"}),
                    |mut app| app.dispatch(input),
                );
            }
        }
        self.dispatch(input)
    }
    fn dispatch(&mut self, input: &Value) -> Value {
        match self {
            Self::ForceQuit(app) => app.request(input),
            Self::Calculator(app) => app.request(input),
            Self::AppAbout(app) => app.request(input),
            Self::Monitor(app) => app.request(input),
        }
    }
}

// The main-thread host immediately copies these borrowed RGBA bytes into an ArrayBuffer.
// No pointer survives a request or a change to the application's image cache.
#[no_mangle]
pub unsafe extern "C" fn axia_native_app_pixels(
    app: *const NativeApp,
    id: *const c_char,
    width: *mut i32,
    height: *mut i32,
) -> *const u8 {
    if id.is_null() || width.is_null() || height.is_null() {
        return std::ptr::null();
    }
    let Some(NativeApp::Monitor(app)) = app.as_ref() else {
        return std::ptr::null();
    };
    let Ok(id) = CStr::from_ptr(id).to_str() else {
        return std::ptr::null();
    };
    let Some(image) = app.pixels(id) else {
        return std::ptr::null();
    };
    *width = image.width as i32;
    *height = image.height as i32;
    image.data.as_ptr()
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
