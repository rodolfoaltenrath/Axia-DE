//! Where a Linux app's new window opens, and which windows the session remembers.
//!
//! A dialog (a transient window: an app's "Open folder", "Open this website?") belongs to the moment,
//! not to its app: it never takes a frame saved by session restore or remembered for the app, and it is
//! never saved itself. Before this lived in Rust, a VS Code dialog shared the Dock id of the editor,
//! took the editor's restored frame (1595×570 for a two-button question) and also used up that slot, so
//! the editor's own window lost its place. sdk/wayland.js only gathers the facts and applies the frame.
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn from(v: &Value) -> Option<Self> {
        let n = |k: &str| v[k].as_f64().filter(|n| n.is_finite());
        let r = Rect { x: n("x")?, y: n("y")?, w: n("w")?, h: n("h")? };
        (r.w > 0. && r.h > 0.).then_some(r)
    }
    fn json(self) -> Value {
        json!({"x":self.x,"y":self.y,"w":self.w,"h":self.h})
    }
}

/// Whether session restore and the per-app window memory keep this window.
pub fn remembers(dialog: bool) -> bool {
    !dialog
}

/// The new window's frame. `saved` (session restore) wins over `kept` (the app's remembered place),
/// which wins over the computed one: its own size, centred in the work area, cascaded by `count`.
/// A dialog only ever gets the computed one.
pub fn place(
    area: Rect,
    chrome: (f64, f64),
    size: (f64, f64),
    count: u32,
    dialog: bool,
    saved: Option<Rect>,
    kept: Option<(Rect, bool)>,
) -> (Rect, &'static str, bool) {
    if remembers(dialog) {
        if let Some(f) = saved {
            return (f, "saved", false);
        }
        if let Some((f, maximized)) = kept {
            return (f, "kept", maximized);
        }
    }
    let w = area.w.min(size.0.max(1.) + chrome.0);
    let h = area.h.min(size.1.max(1.) + chrome.1);
    let step = f64::from(count % 5) * 24.;
    let x = (area.x + (area.w - w) / 2. + step).round();
    let y = (area.y + ((area.h - h) / 2. - 30.).max(8.) + step).round();
    let x = x.min(area.x + area.w - w).max(area.x);
    let y = y.min(area.y + area.h - h).max(area.y);
    (Rect { x, y, w, h }, "computed", false)
}

pub fn request(input: &Value) -> Value {
    match input["op"].as_str().unwrap_or("") {
        "remembers" => json!({"remembers": remembers(input["dialog"] == true)}),
        "placeLinux" => {
            let Some(area) = Rect::from(&input["area"]) else {
                return json!({"error":"app.invalidRequest"});
            };
            let n = |v: &Value| v.as_f64().filter(|n| n.is_finite() && *n >= 0.).unwrap_or(0.);
            let chrome = if input["csd"] == true { (0., 0.) } else { (n(&input["chrome"]["w"]), n(&input["chrome"]["h"])) };
            let kept = Rect::from(&input["kept"]).map(|r| (r, input["kept"]["maximized"] == true));
            let (frame, source, maximize) = place(
                area,
                chrome,
                (n(&input["width"]), n(&input["height"])),
                input["count"].as_u64().unwrap_or(0).min(u64::from(u32::MAX)) as u32,
                input["dialog"] == true,
                Rect::from(&input["saved"]),
                kept,
            );
            json!({"frame":frame.json(),"source":source,"maximize":maximize})
        }
        _ => json!({"error":"app.invalidRequest"}),
    }
}

/// lord.wm.policy(json) → json (server/src/ui/js_wm.c). The reply is freed with axia_window_policy_free.
#[no_mangle]
pub unsafe extern "C" fn axia_window_policy(input: *const c_char) -> *mut c_char {
    let reply = if input.is_null() {
        json!({"error":"app.invalidRequest"})
    } else {
        match serde_json::from_slice::<Value>(CStr::from_ptr(input).to_bytes()) {
            Ok(v) => request(&v),
            Err(_) => json!({"error":"app.invalidRequest"}),
        }
    };
    CString::new(reply.to_string()).unwrap_or_default().into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn axia_window_policy_free(reply: *mut c_char) {
    if !reply.is_null() {
        drop(CString::from_raw(reply));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const AREA: Rect = Rect { x: 0., y: 32., w: 1920., h: 980. };
    const EDITOR: Rect = Rect { x: 162., y: 182., w: 1595., h: 570. };

    #[test]
    fn a_dialog_never_takes_a_saved_or_remembered_frame() {
        // VS Code's "Open this website?" (an 400 × 160 GTK dialog) while the editor's frame is saved
        let (f, source, max) = place(AREA, (2., 43.), (400., 160.), 1, true, Some(EDITOR), Some((EDITOR, true)));
        assert_eq!(source, "computed");
        assert!(!max);
        assert_eq!((f.w, f.h), (402., 203.), "its own size, not the editor's");
        assert!(!remembers(true) && remembers(false));
        // the request tells the shell the slot wasn't used: the editor still finds it
        let r = request(&json!({"op":"placeLinux","area":AREA.json(),"chrome":{"w":2,"h":43},"width":400,"height":160,
            "count":1,"dialog":true,"saved":EDITOR.json(),"kept":{"x":1,"y":40,"w":900,"h":700,"maximized":true}}));
        assert_eq!(r["source"], "computed");
        assert_eq!(r["maximize"], false);
    }

    #[test]
    fn an_app_window_prefers_session_restore_then_its_remembered_place() {
        let kept = Rect { x: 10., y: 40., w: 900., h: 700. };
        assert_eq!(place(AREA, (2., 43.), (800., 600.), 0, false, Some(EDITOR), Some((kept, true))), (EDITOR, "saved", false));
        assert_eq!(place(AREA, (2., 43.), (800., 600.), 0, false, None, Some((kept, true))), (kept, "kept", true));
        assert_eq!(place(AREA, (2., 43.), (800., 600.), 0, false, None, None).1, "computed");
    }

    #[test]
    fn computed_frames_cascade_and_stay_in_the_work_area() {
        let (a, ..) = place(AREA, (2., 43.), (800., 600.), 0, false, None, None);
        let (b, ..) = place(AREA, (2., 43.), (800., 600.), 1, false, None, None);
        assert_eq!((b.x - a.x, b.y - a.y), (24., 24.));
        assert_eq!(place(AREA, (2., 43.), (800., 600.), 5, false, None, None).0, a, "the cascade wraps every 5");
        // bigger than the area: it fits, never under the menu bar or off screen
        let (big, ..) = place(AREA, (2., 43.), (4000., 3000.), 3, false, None, None);
        assert_eq!(big, AREA);
        // a CSD window (the app draws its frame) takes no chrome
        let r = request(&json!({"op":"placeLinux","area":AREA.json(),"chrome":{"w":2,"h":43},"csd":true,"width":800,"height":600}));
        assert_eq!((r["frame"]["w"].as_f64(), r["frame"]["h"].as_f64()), (Some(800.), Some(600.)));
    }

    #[test]
    fn malformed_requests_are_refused() {
        for input in [json!({}), json!({"op":"placeLinux"}), json!({"op":"placeLinux","area":{"x":0,"y":0,"w":0,"h":10}})] {
            assert_eq!(request(&input)["error"], "app.invalidRequest");
        }
        // a non-finite or negative size is never trusted
        let r = request(&json!({"op":"placeLinux","area":AREA.json(),"width":-5,"height":"x"}));
        assert!(r["frame"]["w"].as_f64().unwrap() >= 1.);
    }
}
