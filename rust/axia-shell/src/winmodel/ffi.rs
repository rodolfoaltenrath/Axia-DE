//! C ABI (server/src/ui/winmodel_ffi.h): one call, JSON in and JSON out. Stateless; any thread.
use super::call;
use serde_json::Value;
use std::ffi::{c_char, CStr, CString};

/// Arguments larger than this are refused.
pub const MAX_ARGS: usize = 1 << 20;

fn into_c(s: String) -> *mut c_char {
    // serde_json escapes NUL; our own messages have none
    CString::new(s).map_or(std::ptr::null_mut(), CString::into_raw)
}

/// Runs `op` with `json` (a JSON array of `len` bytes). 0 and the result's JSON in `*out`, or -1 and
/// why in `*out`. Free `*out` with axia_winmodel_free.
#[no_mangle]
pub unsafe extern "C" fn axia_winmodel_call(op: *const c_char, json: *const c_char, len: usize, out: *mut *mut c_char) -> i32 {
    let Some(out) = out.as_mut() else { return -1 };
    let result = (|| {
        if op.is_null() || json.is_null() {
            return Err("winmodel: missing op or arguments".to_string());
        }
        if len > MAX_ARGS {
            return Err("winmodel: arguments too large".to_string());
        }
        let op = CStr::from_ptr(op).to_str().map_err(|_| "winmodel: op is not UTF-8".to_string())?;
        let bytes = std::slice::from_raw_parts(json.cast::<u8>(), len);
        let args: Value = serde_json::from_slice(bytes).map_err(|e| format!("winmodel {op}: {e}"))?;
        let args = args.as_array().ok_or_else(|| format!("winmodel {op}: arguments are not a list"))?;
        call(op, args).map(|v| v.to_string())
    })();
    match result {
        Ok(s) => {
            *out = into_c(s);
            if out.is_null() { -1 } else { 0 }
        }
        Err(e) => {
            *out = into_c(e);
            -1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn axia_winmodel_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}
