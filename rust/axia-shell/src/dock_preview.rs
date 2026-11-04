//! Dock hover policy and aggregate capture budget. No window-system objects live here.
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{c_char, CStr, CString},
};
const OPEN_MS: f64 = 220.;
const LEAVE_MS: f64 = 300.;
#[derive(Default)]
struct Preview {
    key: String,
    since: f64,
    left: Option<f64>,
    open: bool,
    page: usize,
    anchor: Option<(f64, f64)>,
    signatures: HashMap<i32, u64>,
    next_capture: f64,
    interval: f64,
    cost: f64,
}
impl Preview {
    fn update(&mut self, v: &Value, now: f64) -> Value {
        if v["op"] == "dismiss" {
            *self = Self::default();
            return json!({"open":false,"pending":false});
        }
        let key = v["key"].as_str().unwrap_or("");
        let inside = v["inside"].as_bool().unwrap_or(false);
        if !key.is_empty() {
            if self.key != key {
                *self = Self {
                    key: key.chars().take(1024).collect(),
                    since: now,
                    interval: 67.,
                    ..Self::default()
                };
            }
            self.left = None;
        } else if !inside {
            let left = *self.left.get_or_insert(now);
            if !self.open || now - left >= LEAVE_MS {
                *self = Self::default();
            }
        } else {
            self.left = None;
        }
        let empty = vec![];
        let all = v["windows"].as_array().unwrap_or(&empty);
        if self.key.is_empty() || all.is_empty() {
            self.open = false;
            return json!({"open":false,"pending":false});
        }
        if !self.open && now - self.since >= OPEN_MS {
            self.open = true;
        }
        if !self.open {
            return json!({"open":false,"pending":true});
        }
        let number = |key: &str, fallback: f64| {
            v[key]
                .as_f64()
                .filter(|n| n.is_finite())
                .unwrap_or(fallback)
        };
        let aw = number("aw", 1366.).clamp(160., 32768.);
        let ah = number("ah", 768.).clamp(120., 32768.);
        let ax = number("ax", 0.);
        let ay = number("ay", 0.);
        let side = v["side"].as_str().unwrap_or("bottom");
        // Keep side docks' panels inside the space between the Dock and the far edge.
        let x = number("x", ax + aw / 2.);
        let y = number("y", ay + ah - 80.);
        // Hold the opening position as Dock magnification settles under the pointer.
        let (x, y) = *self.anchor.get_or_insert((x, y));
        let room_w = if side == "left" {
            ax + aw - x - 12.
        } else if side == "right" {
            x - ax - 12.
        } else {
            aw - 24.
        };
        let room_h = if side == "bottom" {
            y - ay - 32.
        } else {
            ah - 56.
        };
        let card_w = ((room_w - 24.).max(72.)).min(232.);
        let image_h = (card_w * 0.61).min((room_h - 72.).max(36.));
        let capacity = (((room_w - 16.) / (card_w + 8.)).floor() as usize).clamp(1, 4);
        let pages = all.len().div_ceil(capacity).max(1);
        if v["op"] == "page" {
            self.page = if v["back"].as_bool().unwrap_or(false) {
                (self.page + pages - 1) % pages
            } else {
                (self.page + 1) % pages
            };
        }
        self.page = self.page.min(pages - 1);
        let cards: Vec<Value> = all
            .iter()
            .skip(self.page * capacity)
            .take(capacity)
            .cloned()
            .collect();
        let width = 16. + cards.len() as f64 * (card_w + 8.) - 8.;
        let height = 16. + image_h + 30. + if pages > 1 { 28. } else { 0. };
        let left = if side == "left" {
            x
        } else if side == "right" {
            x - width
        } else {
            x - width / 2.
        };
        let top = if side == "bottom" {
            y - height
        } else {
            y - height / 2.
        };
        self.signatures.retain(|node, _| {
            cards.iter().any(|c| {
                c["node"].as_i64() == Some(*node as i64)
                    || c["surface"]
                        .as_i64()
                        .filter(|s| *s > 0)
                        .map(|s| s | 0x40000000)
                        == Some(*node as i64)
            })
        });
        json!({"open":true,"pending":false,"key":self.key,"cards":cards,"page":self.page,"pages":pages,
            "left":left.clamp(ax + 4., (ax + aw - width - 4.).max(ax + 4.)),
            "top":top.clamp(ay + 30., (ay + ah - height - 4.).max(ay + 30.)),
            "width":width,"height":height,"cardWidth":card_w,"imageHeight":image_h})
    }
    fn due(&mut self, source: i32, signature: u64, now: f64, force: bool) -> bool {
        if !self.open
            || source <= 0
            || now < self.next_capture
            || (!force && self.signatures.get(&source) == Some(&signature))
        {
            return false;
        }
        self.signatures.insert(source, signature);
        self.next_capture = now + self.interval.max(67.);
        true
    }
    fn done(&mut self, cost: f64, now: f64) {
        self.cost = if self.cost == 0. {
            cost
        } else {
            self.cost * 0.75 + cost * 0.25
        };
        // Target <= 3% of one UI thread, with an immediate backoff after a slow readback.
        self.interval = (self.cost.max(cost) * 34.).clamp(67., 1000.);
        self.next_capture = self.next_capture.max(now + self.interval);
    }
}
thread_local! { static STATES: RefCell<(u32, HashMap<u32, Preview>)> = RefCell::new((0, HashMap::new())); }
fn request(v: Value, now: f64) -> Value {
    STATES.with(|states| {
        let mut states = states.borrow_mut();
        if v["op"] == "create" {
            if states.1.len() >= 8 {
                return json!({"error":"preview limit"});
            }
            states.0 = states.0.wrapping_add(1).max(1);
            let id = states.0;
            states.1.insert(id, Preview::default());
            return json!({"id":id});
        }
        let id = v["id"].as_u64().unwrap_or(0) as u32;
        if v["op"] == "close" {
            states.1.remove(&id);
            return json!({});
        }
        states
            .1
            .get_mut(&id)
            .map(|s| s.update(&v, now))
            .unwrap_or(json!({"open":false,"pending":false}))
    })
}
#[no_mangle]
pub extern "C" fn axia_dock_preview_reset() {
    STATES.with(|states| *states.borrow_mut() = (0, HashMap::new()));
}

// UI-thread only. C bounds input to 128 KiB and supplies a finite monotonic timestamp.
#[no_mangle]
pub unsafe extern "C" fn axia_dock_preview_request(input: *const c_char, now: f64) -> *mut c_char {
    if input.is_null() || !now.is_finite() {
        return std::ptr::null_mut();
    }
    let result = CStr::from_ptr(input)
        .to_str()
        .ok()
        .and_then(|s| serde_json::from_str(s).ok())
        .map(|v| request(v, now))
        .unwrap_or(json!({"error":"invalid preview request"}));
    CString::new(result.to_string()).unwrap().into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn axia_dock_preview_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}
#[no_mangle]
pub extern "C" fn axia_dock_preview_due(
    id: u32,
    source: i32,
    signature: u64,
    now: f64,
    force: i32,
) -> i32 {
    STATES.with(|states| {
        states
            .borrow_mut()
            .1
            .get_mut(&id)
            .map(|s| s.due(source, signature, now, force != 0) as i32)
            .unwrap_or(0)
    })
}
#[no_mangle]
pub extern "C" fn axia_dock_preview_done(id: u32, cost: f64, now: f64) {
    STATES.with(|states| {
        if let Some(s) = states.borrow_mut().1.get_mut(&id) {
            s.done(cost, now);
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input(key: &str, inside: bool) -> Value {
        json!({"key":key,"inside":inside,"windows":[{"id":"a","node":5}],"aw":800,"ah":600,"x":400,"y":500})
    }
    #[test]
    fn delay_grace_and_switching_apps() {
        let mut s = Preview::default();
        assert_eq!(s.update(&input("a", false), 0.)["pending"], true);
        assert_eq!(s.update(&input("a", false), 219.)["open"], false);
        assert_eq!(s.update(&input("a", false), 220.)["open"], true);
        assert_eq!(s.update(&input("", false), 430.)["open"], true);
        assert_eq!(s.update(&input("", true), 600.)["open"], true);
        assert_eq!(s.update(&input("b", false), 610.)["open"], false);
        assert_eq!(s.update(&input("b", false), 1030.)["open"], true);
        s.update(&input("", false), 1040.);
        assert_eq!(s.update(&input("", false), 1340.)["open"], false);
    }
    #[test]
    fn popup_stays_put_when_dock_shrinks_and_pointer_enters_preview() {
        let mut s = Preview::default();
        let mut v = input("a", false);
        s.update(&v, 0.);
        let first = s.update(&v, 230.);
        v["x"] = json!(540.);
        v["y"] = json!(560.);
        let shrink = s.update(&v, 240.);
        v["key"] = json!("");
        v["inside"] = json!(true);
        let enter = s.update(&v, 260.);
        assert_eq!(first["left"], shrink["left"]);
        assert_eq!(first["top"], shrink["top"]);
        assert_eq!(first["left"], enter["left"]);
        assert_eq!(first["top"], enter["top"]);
    }
    #[test]
    fn aggregate_budget_deduplicates_and_backs_off() {
        let mut s = Preview {
            open: true,
            interval: 67.,
            ..Preview::default()
        };
        assert!(s.due(1, 10, 0., false));
        assert!(!s.due(2, 20, 0., false));
        s.done(2., 2.);
        assert!(!s.due(1, 10, 100., false));
        assert!(s.due(2, 20, 100., false));
        s.done(20., 120.);
        assert!(!s.due(1, 11, 200., false));
        assert!(s.due(1, 11, 801., false));
        s.open = false;
        assert!(!s.due(1, 12, 2000., true));
    }
    #[test]
    fn pagination_and_screen_edges_are_bounded() {
        for side in ["bottom", "left", "right"] {
            let mut s = Preview::default();
            let mut v = input("a", false);
            v["windows"] = json!((1..=20)
                .map(|i| json!({"id":i.to_string(),"node":i}))
                .collect::<Vec<_>>());
            v["side"] = json!(side);
            v["x"] = json!(if side == "left" { 80. } else { 720. });
            s.update(&v, 0.);
            let r = s.update(&v, 500.);
            assert!(r["cards"].as_array().unwrap().len() <= 4);
            assert!(r["left"].as_f64().unwrap() >= 0.);
            assert!(r["left"].as_f64().unwrap() + r["width"].as_f64().unwrap() <= 800.);
            v["op"] = json!("page");
            let next = s.update(&v, 600.);
            assert_eq!(next["page"], 1);
        }
    }
}
