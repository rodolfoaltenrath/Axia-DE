use crate::native_apps_host::*;
use serde_json::Value;
use std::ffi::CStr;
#[test]
fn native_handle_rejects_unknown_ids_and_invalid_json() {
    unsafe {
        assert!(axia_native_app_new(c"unknown".as_ptr()).is_null());
        let app = axia_native_app_new(c"forcequit".as_ptr());
        assert!(!app.is_null());
        let reply = axia_native_app_request(app, c"{invalid".as_ptr());
        let value: Value = serde_json::from_slice(CStr::from_ptr(reply).to_bytes()).unwrap();
        assert_eq!(value["error"], "app.invalidRequest");
        axia_native_app_reply_free(reply);
        axia_native_app_free(app);
        axia_native_app_free(std::ptr::null_mut());
    }
}
