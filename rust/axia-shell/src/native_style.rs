use serde_json::{json, Value};
pub use axia_native::style::*;
#[path = "shell_theme.rs"]
mod shell_theme;
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
        if bytes.len() > 128 * 1024 {
            Value::Null
        } else if let Ok(source) = serde_json::from_slice::<Value>(bytes) {
            if source["op"] == "shellTheme" {
                shell_theme::tokens(&source)
            } else if source["op"] == "dockLayout" {
                crate::dock_layout::request(&source)
            } else {
                let tokens = files_theme(&source);
                json!({"tokens":tokens,"chrome":frame(&tokens),"dockFeedback":dock_feedback(source["mode"] != "light")})
            }
        } else {
            Value::Null
        }
    };
    CString::new(reply.to_string()).unwrap().into_raw()
}
