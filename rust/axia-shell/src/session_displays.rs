//! Physical displays from compositor events, shared by resident consumers by reference.
use axia_state::{Reader, Store};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr};

thread_local! { static DISPLAYS: Store<Vec<Value>> = Store::new(Vec::new()); }

pub(crate) fn reader() -> Reader<Vec<Value>> { DISPLAYS.with(Store::reader) }
fn normalize(input: &Value) -> Result<Vec<Value>, ()> {
    let displays = input["displays"].as_array().ok_or(())?;
    Ok(displays.iter().take(16).filter_map(|display| {
        let frame = crate::winmodel::Rect::from_value(display)?;
        if ![frame.x, frame.y, frame.w, frame.h].into_iter().all(f64::is_finite)
            || frame.w <= 0. || frame.h <= 0. { return None; }
        Some(json!({"id":display["id"],"name":display["name"],"connector":display["connector"],
            "x":frame.x,"y":frame.y,"w":frame.w,"h":frame.h}))
    }).collect())
}
pub(crate) fn update(input: &Value) -> Result<bool, ()> {
    let next = normalize(input)?;
    Ok(DISPLAYS.with(|store| store.update(|current| {
        let changed = *current != next;
        if changed { *current = next; }
        (changed, changed)
    })))
}
/// The compositor lends a UTF-8 JSON event for this call only. Invalid events preserve the source.
#[no_mangle]
pub unsafe extern "C" fn axia_session_displays_update(input: *const c_char) -> i32 {
    if input.is_null() { return -1; }
    let bytes = CStr::from_ptr(input).to_bytes();
    if bytes.len() > 65_536 { return -1; }
    serde_json::from_slice::<Value>(bytes).ok().and_then(|value| update(&value).ok())
        .map_or(-1, i32::from)
}

#[cfg(test)]
#[path = "session_displays_tests.rs"]
mod tests;
