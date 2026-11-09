use super::displays::{placement_ok, resolve_drop, snap_display, touches};
use super::place::{clean_place, display_key, find_display, MAX_PLACES};
use super::*;
use serde_json::json;

fn c(op: &str, args: Value) -> Value {
    call(op, args.as_array().unwrap()).unwrap_or_else(|e| panic!("{op}: {e}"))
}
fn f(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or_else(|| panic!("{k} in {v}"))
}

// ---- remembered places (was server/tests/place_model_test.mjs) ----

fn a() -> Value {
    json!({ "id": 1, "name": "Dell U2415", "connector": "HDMI-A-1", "x": 0, "y": 0, "w": 1920, "h": 1080, "main": true })
}
fn b() -> Value {
    json!({ "id": 2, "name": "LG 27", "connector": "DP-1", "x": 1920, "y": 0, "w": 2560, "h": 1440 })
}
fn with(d: &Value, k: &str, v: Value) -> Value {
    let mut d = d.clone();
    d[k] = v;
    d
}
fn area(d: &Value) -> Value {
    json!({ "x": d["x"], "y": f(d, "y") + 28.0, "w": d["w"], "h": f(d, "h") - 28.0 })
}
fn restore(p: &Value, ds: &[Value], opts: Value) -> Value {
    let areas: Vec<Value> = ds.iter().map(area).collect();
    c("place.restore", json!([p, ds, areas, opts]))
}

#[test]
fn a_place_is_known_by_its_display_not_its_index() {
    let (a, b) = (a(), b());
    let p = c("place.of", json!([{ "x": 2020, "y": 128, "w": 800, "h": 600 }, b]));
    assert_eq!(p, json!({ "d": display_key(&b), "x": 100, "y": 128, "w": 800, "h": 600, "max": false }));
    assert_eq!(find_display(p["d"].as_str().unwrap(), &[b.clone(), a.clone()]), Some(0));
    // arranged on the other side, with a new id
    let moved = with(&with(&b, "x", json!(-2560)), "id", json!(9));
    assert_eq!(f(&restore(&p, &[a.clone(), moved], json!({ "main": 0 })), "x"), -2460.0);
    let key = display_key(&b);
    assert_eq!(find_display(&key, &[with(&b, "connector", json!("DP-2"))]), Some(0), "same monitor, another port");
    let twins = [with(&b, "connector", json!("DP-2")), with(&b, "connector", json!("DP-3"))];
    assert_eq!(find_display(&key, &twins), None, "two alike: can't tell");
    assert_eq!(find_display(&key, &[with(&a, "connector", json!("DP-1"))]), None, "another monitor on that port");
}

#[test]
fn a_place_off_screen_comes_back_visible() {
    let (a, b) = (a(), b());
    let both = [a.clone(), b.clone()];
    let p = c("place.of", json!([{ "x": 2020, "y": 128, "w": 800, "h": 600 }, b]));
    let gone = restore(&p, &[a.clone()], json!({ "main": 0 }));
    assert!(f(&gone, "x") >= 0.0 && f(&gone, "x") + f(&gone, "w") <= 1920.0);
    assert!(f(&gone, "y") >= 28.0 && f(&gone, "y") + f(&gone, "h") <= 1080.0);
    let off = restore(&with(&with(&p, "x", json!(9000)), "y", json!(9000)), &both, json!({ "main": 0 }));
    assert!(f(&off, "x") + f(&off, "w") <= 4480.0 && f(&off, "y") + f(&off, "h") <= 1440.0);
    let big = restore(&with(&with(&p, "w", json!(9000)), "h", json!(9000)), &[a.clone()], json!({ "main": 0 }));
    assert!(f(&big, "w") <= 1920.0 && f(&big, "h") <= 1052.0);
    // the Dock's assignment chooses the display; a fixed app keeps its size; a twin doesn't land on the first
    assert!(f(&restore(&p, &both, json!({ "main": 0, "assigned": 0 })), "x") < 1920.0);
    let fp = with(&with(&with(&p, "w", json!(1000)), "h", json!(900)), "max", json!(true));
    let fx = restore(&fp, &both, json!({ "main": 0, "fixed": true, "size": [400, 300] }));
    assert_eq!((f(&fx, "w"), f(&fx, "h"), fx["maximized"].clone()), (400.0, 300.0, json!(false)));
    assert_eq!(f(&restore(&p, &both, json!({ "main": 0, "shift": 30 })), "x"), 2050.0);
}

#[test]
fn what_a_window_leaves() {
    let a = a();
    let of = |w: Value| c("place.of", json!([w, a]));
    assert_eq!(of(json!({ "x": 0, "y": 0, "w": 900, "h": 700, "maximized": true }))["max"], json!(true));
    let snapped = json!({ "x": 0, "y": 0, "w": 960, "h": 1080, "snapped": "left", "restore": { "x": 50, "y": 60, "w": 500, "h": 400 } });
    assert_eq!(of(snapped)["w"], json!(500));
    let lifted = json!({ "x": 0, "y": 0, "w": 960, "h": 300, "oskFrame": { "x": 5, "y": 6, "w": 500, "h": 700, "maximized": true } });
    assert_eq!((of(lifted.clone())["h"].clone(), of(lifted)["max"].clone()), (json!(700), json!(true)));
    assert_eq!(of(json!({ "x": 0, "y": 0, "w": 960, "h": 300, "fullscreen": true })), Value::Null);
    assert_eq!(of(json!({ "x": 0, "y": 0, "w": 200, "h": 100, "pip": {} })), Value::Null);
    assert_eq!(of(json!({ "x": 0, "y": 0, "w": 0, "h": 100 })), Value::Null);
}

#[test]
fn saved_places_are_data() {
    for v in [
        json!(null),
        json!("x"),
        json!({}),
        json!({ "d": "a", "x": "y", "y": 0, "w": 100, "h": 100 }),
        json!({ "d": "a", "x": 0, "y": 0, "w": 1, "h": 100 }),
        json!({ "d": "a", "x": 0, "y": 0, "w": 1e9, "h": 100 }),
        json!({ "d": "x".repeat(301), "x": 0, "y": 0, "w": 100, "h": 100 }),
    ] {
        assert!(clean_place(&v).is_none(), "{v}");
    }
    assert_eq!(restore(&json!({ "d": "a" }), &[a(), b()], json!({ "main": 0 })), Value::Null);
    assert_eq!(c("place.restore", json!([{ "d": "a", "x": 0, "y": 0, "w": 100, "h": 100 }, [], [], {}])), Value::Null);
}

#[test]
fn the_oldest_places_go_first() {
    let p = json!({ "d": "x|y", "x": 0, "y": 0, "w": 100, "h": 100, "max": false });
    let mut all = json!([]);
    for i in 0..MAX_PLACES + 5 {
        all = c("place.put", json!([all, format!("a{i}"), p]));
    }
    let keys: Vec<&str> = all.as_array().unwrap().iter().map(|e| e[0].as_str().unwrap()).collect();
    assert_eq!(keys.len(), MAX_PLACES);
    assert_eq!(keys[0], "a5");
    assert_eq!(*keys.last().unwrap(), format!("a{}", MAX_PLACES + 4));
    // saving an app again moves it to the end, once
    let again = c("place.put", json!([all, "a10", p]));
    let keys: Vec<&str> = again.as_array().unwrap().iter().map(|e| e[0].as_str().unwrap()).collect();
    assert_eq!((keys.len(), *keys.last().unwrap()), (MAX_PLACES, "a10"));
    assert_eq!(keys.iter().filter(|k| **k == "a10").count(), 1);
}

// ---- desktops (was part of server/tests/desktop_model_test.mjs) ----

#[test]
fn adding_and_removing_desktops() {
    let added = c("desk.add", json!([{ "list": [1], "active": 1 }]));
    assert_eq!(added, json!({ "state": { "list": [1, 2], "active": 1 }, "id": 2 }), "adding does not switch");
    let full = json!({ "list": [1, 2, 3, 4, 5, 6, 7, 8, 9], "active": 1 });
    assert_eq!(c("desk.add", json!([full]))["id"], Value::Null, "no more than the maximum");
    assert_eq!(c("desk.add", json!([{ "list": [1, 3], "active": 1 }]))["id"], json!(4), "ids are not reused");
    assert_eq!(
        c("desk.remove", json!([{ "list": [1, 2, 3], "active": 2 }, 2])),
        json!({ "state": { "list": [1, 3], "active": 3 }, "target": 3 }),
        "closing the active shows the next"
    );
    assert_eq!(
        c("desk.remove", json!([{ "list": [1, 2, 3], "active": 1 }, 3])),
        json!({ "state": { "list": [1, 2], "active": 1 }, "target": 2 }),
        "closing the last moves its windows to the previous"
    );
    assert_eq!(c("desk.remove", json!([{ "list": [1], "active": 1 }, 1]))["target"], Value::Null, "the last desktop stays");
    assert_eq!(c("desk.remove", json!([{ "list": [1], "active": 1 }, 9]))["target"], Value::Null, "unknown");
}

#[test]
fn moving_between_desktops() {
    assert_eq!(c("desk.neighbor", json!([[1, 2, 3], 2, 1])), json!(3));
    assert_eq!(c("desk.neighbor", json!([[1, 2, 3], 2, -1])), json!(1));
    assert_eq!(c("desk.neighbor", json!([[1, 2], 2, 1])), Value::Null, "no wrap at the end");
    assert_eq!(c("desk.neighbor", json!([[1, 2], 1, -1])), Value::Null, "no wrap at the start");
    let step = |op: &str, dir: &str, rtl: bool| c(op, json!([dir, rtl]));
    assert_eq!((step("desk.arrow", "left", false), step("desk.arrow", "right", false)), (json!(-1), json!(1)));
    assert_eq!((step("desk.arrow", "left", true), step("desk.arrow", "right", true)), (json!(1), json!(-1)));
    assert_eq!((step("desk.swipe", "left", false), step("desk.swipe", "right", false)), (json!(1), json!(-1)));
    assert_eq!((step("desk.swipe", "left", true), step("desk.swipe", "right", true)), (json!(-1), json!(1)));
}

#[test]
fn saved_desktop_values_come_from_a_file() {
    let count: Vec<Value> = [json!(4), json!(0), json!(99), json!("3"), json!(null), json!(2.5)]
        .into_iter()
        .map(|v| c("desk.count", json!([v])))
        .collect();
    assert_eq!(count, [json!(4), json!(1), json!(1), json!(1), json!(1), json!(1)]);
    let index: Vec<Value> = [json!(3), json!(-1), json!(99), json!("1"), json!(null)]
        .into_iter()
        .map(|v| c("desk.index", json!([v])))
        .collect();
    assert_eq!(index, [json!(3), json!(0), json!(0), json!(0), json!(0)]);
}

// ---- the Overview's and Snap Assist's layouts ----

fn inside(p: &Value, a: &Rect) -> bool {
    f(p, "x") >= a.x && f(p, "ly") >= a.y && f(p, "x") + f(p, "w") <= a.x + a.w && f(p, "y") + f(p, "h") <= a.y + a.h
}

#[test]
fn the_overview_grid_fits_the_area() {
    let a = Rect::new(0.0, 28.0, 1280.0, 700.0);
    let g = c("layout.grid", json!([[{ "w": 800, "h": 600 }, { "w": 300, "h": 900 }, { "w": 2000, "h": 1000 }], a.to_value()]));
    let g = g.as_array().unwrap();
    assert_eq!(g.len(), 3);
    assert!(g.iter().all(|p| inside(p, &a)), "every window fits: {g:?}");
    assert!(g.iter().all(|p| f(p, "scale") <= 0.85), "never larger than 85 %");
    assert!(g.iter().all(|p| f(p, "y") - f(p, "ly") == 30.0 && f(p, "lh") == 26.0), "the title row above each");
    assert_eq!(c("layout.grid", json!([[], a.to_value()])), json!([]));
    // a maximized window is laid out at the work area's size, not its own
    let big = c("layout.grid", json!([[{ "w": 960, "h": 640 }, { "w": a.w, "h": a.h }], a.to_value()]));
    assert!(inside(&big[1], &a));
}

#[test]
fn assist_miniatures_fit_their_zone() {
    let cell = Rect::new(640.0, 0.0, 640.0, 752.0);
    for n in [1usize, 2, 3, 5, 12] {
        let frames: Vec<(f64, f64)> = (0..n).map(|i| (600.0 + 40.0 * i as f64, 400.0)).collect();
        let ps = grid(&frames, cell, &ASSIST_GRID);
        assert_eq!(ps.len(), n);
        for (i, p) in ps.iter().enumerate() {
            let v = p.to_value();
            assert!(inside(&v, &cell), "{n} candidates: {i} with its title inside {cell:?}: {v}");
            assert!(p.scale > 0.0 && p.scale <= 0.85);
            for q in &ps[i + 1..] {
                let apart = p.x + p.w <= q.x || q.x + q.w <= p.x || p.y + p.h <= q.ly || q.y + q.h <= p.ly;
                assert!(apart, "{n}: miniatures and titles don't overlap");
            }
        }
    }
}

#[test]
fn the_strip_of_desktops() {
    let a = Rect::new(0.0, 28.0, 1280.0, 700.0).to_value();
    let s = c("layout.strip", json!([[1, 2, 3], a, { "canAdd": true }]));
    let xs: Vec<f64> = s["cards"].as_array().unwrap().iter().map(|c| f(c, "x")).collect();
    assert_eq!(s["cards"].as_array().unwrap().iter().map(|c| c["id"].clone()).collect::<Vec<_>>(), [json!(1), json!(2), json!(3)]);
    assert!(xs[0] < xs[1] && xs[1] < f(&s["add"], "x"), "in order");
    let r = c("layout.strip", json!([[1, 2, 3], a, { "canAdd": true, "rtl": true }]));
    let rx: Vec<f64> = r["cards"].as_array().unwrap().iter().map(|c| f(c, "x")).collect();
    assert!(rx[0] > rx[1] && rx[2] > f(&r["add"], "x"), "mirrored");
    let min = |v: &[f64], add: f64| v.iter().copied().fold(add, f64::min);
    assert_eq!(min(&rx, f(&r["add"], "x")), min(&xs, f(&s["add"], "x")), "mirror keeps the box");
    let nine = json!([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(c("layout.strip", json!([nine, a, { "canAdd": false }]))["add"], Value::Null, "full: no add button");
    let small = c("layout.strip", json!([nine, { "x": 0, "y": 0, "w": 1024, "h": 700 }, { "canAdd": false }]));
    let cards = small["cards"].as_array().unwrap();
    assert!(f(&cards[8], "x") + f(&cards[8], "w") <= 1024.0 && f(&cards[0], "x") >= 0.0, "nine fit on 1024");
    assert_eq!(f(&s, "h"), 14.0 + 105.0 + 30.0, "default aspect 1.6");
}

// ---- Settings → Displays (was server/tests/display_layout_test.mjs) ----

const A: Rect = Rect::new(0.0, 0.0, 1280.0, 800.0);
const T: f64 = 120.0;

#[test]
fn display_numbers() {
    let outs = json!([
        { "id": 40, "active": true, "w": 1280 }, { "id": 41, "active": true, "w": 1920, "main": true },
        { "id": 42, "active": false }, { "id": 43, "active": true, "w": 800 },
    ]);
    assert_eq!(c("display.numbers", json!([outs])), json!({ "41": 1, "40": 2, "43": 3 }));
    assert_eq!(c("display.numbers", json!([[{ "id": 7, "active": true, "w": 1280, "main": true }]])), json!({ "7": 1 }));
    assert_eq!(c("display.numbers", json!([null])), json!({}));
}

#[test]
fn displays_touch_along_an_edge() {
    assert!(touches(&A, &Rect::new(1280.0, 0.0, 100.0, 100.0)), "side by side");
    assert!(touches(&A, &Rect::new(100.0, 800.0, 100.0, 100.0)), "stacked");
    assert!(!touches(&A, &Rect::new(1280.0, 800.0, 100.0, 100.0)), "a corner alone");
    assert!(!touches(&A, &Rect::new(1281.0, 0.0, 100.0, 100.0)), "a gap");
}

#[test]
fn snapping_while_dragging() {
    let two = [A, Rect::new(1280.0, 0.0, 1920.0, 1080.0)];
    let s = |x, y| snap_display(&two, 1, x, y, T);
    assert_eq!(s(1330.0, 60.0), (1280.0, 0.0), "edge to edge, tops aligned");
    assert_eq!(s(1300.0, -250.0), (1280.0, -280.0), "bottoms aligned when closer than tops");
    assert_eq!(s(1290.0, -130.0), (1280.0, -140.0), "centers aligned");
    assert_eq!(s(-1900.0, 10.0), (-1920.0, 0.0), "left of it");
    assert_eq!(s(50.0, 850.0), (0.0, 800.0), "below it, lefts aligned");
    assert_eq!(s(-300.0, 820.0), (-320.0, 800.0), "below it, centered");
    assert_eq!(s(3000.0, 3000.0), (3000.0, 3000.0), "far away: no snap");
    assert_eq!(s(1300.0, 2000.0), (1300.0, 2000.0), "far below: x does not stick to its side");
    // the JSON op gives the same
    let r = c("display.snap", json!([[A.to_value(), two[1].to_value()], 1, 1330, 60, T]));
    assert_eq!(r, json!({ "x": 1280, "y": 0 }));
}

#[test]
fn a_drop_always_touches_and_never_overlaps() {
    let two = [A, Rect::new(1280.0, 0.0, 1920.0, 1080.0)];
    let valid = |rs: &[Rect], i: usize, p: (f64, f64)| placement_ok(rs, i, p.0, p.1);
    assert_eq!(resolve_drop(&two, 1, 1330.0, 60.0, T), (1280.0, 0.0), "valid snapped drop is kept");
    assert!(valid(&two, 1, resolve_drop(&two, 1, 400.0, 200.0, T)), "dropped on top");
    let far = resolve_drop(&two, 1, 4000.0, 3000.0, T);
    assert_eq!(far, (1280.0, 799.0), "dropped far away: the nearest corner region");
    assert!(valid(&two, 1, resolve_drop(&two, 1, 1280.0, 900.0, 0.0)), "corner only: slides to share an edge");
    assert_eq!(resolve_drop(&two, 1, 1280.0, 500.0, 0.0), (1280.0, 500.0), "diagonal offset allowed while edges touch");
    let three = [A, Rect::new(1280.0, 0.0, 1920.0, 1080.0), Rect::new(3200.0, 0.0, 1024.0, 768.0)];
    let p = resolve_drop(&three, 2, 1500.0, 100.0, T);
    assert!(valid(&three, 2, p), "three: slid out of the middle one");
    assert_eq!(resolve_drop(&two, 1, -310.0, 805.0, T), (-320.0, 800.0), "stacked, centered");
    assert!(placement_ok(&[A], 0, 5000.0, 5000.0), "alone is always fine");
    assert_eq!(c("display.ok", json!([[A.to_value(), two[1].to_value()], 1, 1281, 0])), json!(false));
    assert!(call("display.drop", &[json!([]), json!(0), json!(0), json!(0), json!(0)]).is_err());
    assert!(call("display.drop", &[json!([A.to_value()]), json!(3), json!(0), json!(0), json!(0)]).is_err());
}

#[test]
fn every_drop_over_a_grid_is_valid() {
    let two = [A, Rect::new(1280.0, 0.0, 1920.0, 1080.0)];
    let three = [A, Rect::new(1280.0, 0.0, 1920.0, 1080.0), Rect::new(3200.0, 0.0, 1024.0, 768.0)];
    let mut bad = 0;
    for (rects, i) in [(&two[..], 1), (&two[..], 0), (&three[..], 2), (&three[..], 1), (&three[..], 0)] {
        for x in (-5000..=6000).step_by(173) {
            for y in (-3000..=3000).step_by(157) {
                for th in [0.0, T] {
                    let p = resolve_drop(rects, i, x as f64, y as f64, th);
                    if !placement_ok(rects, i, p.0, p.1) || p.0.fract() != 0.0 || p.1.fract() != 0.0 {
                        bad += 1;
                    }
                }
            }
        }
    }
    assert_eq!(bad, 0);
}

// ---- the bridge ----

#[test]
fn bad_calls_say_why() {
    assert!(call("nothing", &[]).unwrap_err().contains("unknown"));
    assert!(call("layout.grid", &[json!([]), json!("not a rect")]).is_err());
    unsafe {
        use super::ffi::{axia_winmodel_call, axia_winmodel_free};
        let mut out = std::ptr::null_mut();
        let args = b"[[1, 2, 3], 2, 1]";
        let rc = axia_winmodel_call(b"desk.neighbor\0".as_ptr().cast(), args.as_ptr().cast(), args.len(), &mut out);
        assert_eq!((rc, std::ffi::CStr::from_ptr(out).to_str().unwrap()), (0, "3"));
        axia_winmodel_free(out);
        let bad = b"{not json";
        let rc = axia_winmodel_call(b"desk.neighbor\0".as_ptr().cast(), bad.as_ptr().cast(), bad.len(), &mut out);
        assert_eq!(rc, -1);
        assert!(!out.is_null());
        axia_winmodel_free(out);
        axia_winmodel_free(std::ptr::null_mut());
    }
}

// ---- the session's windows (session.rs; the predicates were server/tests/desktop_model_test.mjs) ----

use super::session::Session;

fn facts() -> Value {
    json!({
        "displays": [{ "id": 1, "x": 0, "y": 0, "w": 1280, "h": 800, "main": true, "work": { "x": 0, "y": 28, "w": 1280, "h": 772 } }],
        "apps": { "files": { "size": [800, 500], "minSize": [400, 300] } },
        "snap": true,
    })
}
fn op(s: &mut Session, name: &str, args: Value) -> Value {
    let mut all = vec![facts()];
    all.extend(args.as_array().unwrap().iter().cloned());
    s.call(name, &all).unwrap_or_else(|e| panic!("{name}: {e}"))
}
fn state(s: &mut Session) -> Value {
    op(s, "state", json!([]))["state"].clone()
}
fn ids(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_owned()).collect()
}
fn window(s: &mut Session, id: &str) -> Value {
    s.debug_window(id)
}
/// A session from plain windows (bottom to top) on `desktops`, `active` showing.
fn session(list: Value, desktops: Value, active: i64) -> Session {
    let mut s = Session::default();
    s.call("setDesktops", &[facts(), desktops, json!(active)]).unwrap();
    s.debug_load(list.as_array().unwrap());
    s
}
fn lin(sid: u64, desktop: i64, extra: Value) -> Value {
    let mut w = json!({ "id": format!("linux#{sid}"), "desktop": desktop, "args": { "surface": sid }, "x": 0, "y": 0, "w": 100, "h": 100 });
    for (k, v) in extra.as_object().unwrap() {
        w[k] = v.clone();
    }
    w
}
fn win(id: &str, desktop: i64, extra: Value) -> Value {
    let mut w = json!({ "id": id, "desktop": desktop, "x": 0, "y": 0, "w": 100, "h": 100 });
    for (k, v) in extra.as_object().unwrap() {
        w[k] = v.clone();
    }
    w
}

#[test]
fn what_shows_and_what_is_mounted() {
    let list = json!([win("a", 1, json!({})), lin(5, 2, json!({})), win("b", 2, json!({})), win("m", 1, json!({ "minimized": true })),
        win("fs", 1, json!({ "fullscreen": true })), win("p", 2, json!({ "pip": { "x": 0 } }))]);
    let mut s = session(list.clone(), json!([1, 2, 3]), 1);
    let v = state(&mut s)["views"].clone();
    assert_eq!(ids(&v["visible"]), ["a"]);
    assert_eq!(ids(&v["mounted"]), ["a", "b", "m"], "a Linux window on a hidden desktop has no node");
    assert_eq!(v["front"], json!("fs"), "fullscreen is in front");
    assert_eq!(v["top"], json!("a"), "the theme's top framed window");
    assert_eq!(ids(&v["hidden"]), ["linux#5", "b", "m"], "pip is on every desktop");
    assert_eq!(ids(&v["desk"]["3"]), ["p"], "pip is sticky");
    op(&mut s, "setActiveDesktop", json!([2]));
    let v = state(&mut s)["views"].clone();
    assert_eq!(ids(&v["visible"]), ["linux#5", "b"]);
    assert_eq!(ids(&v["mounted"]), ["a", "linux#5", "b", "m"]);
    assert_eq!(v["front"], json!("b"), "front skips pip");
    op(&mut s, "setActiveDesktop", json!([3]));
    assert_eq!(state(&mut s)["views"]["front"], Value::Null, "an empty desktop has none");
    // a window without a desktop is on the first
    let mut old = session(json!([{ "id": "old", "x": 0, "y": 0, "w": 10, "h": 10 }]), json!([1]), 1);
    assert_eq!(ids(&state(&mut old)["views"]["visible"]), ["old"]);
}

#[test]
fn minimized_linux_windows_in_the_overview() {
    let iconic = json!([lin(5, 1, json!({ "minimized": true })), lin(6, 2, json!({ "minimized": true })), lin(7, 1, json!({}))]);
    let mut s = session(iconic, json!([1, 2]), 1);
    assert_eq!(ids(&state(&mut s)["views"]["mounted"]), ["linux#7"]);
    op(&mut s, "setOverview", json!([true]));
    let v = state(&mut s)["views"].clone();
    assert_eq!(ids(&v["mounted"]), ["linux#5", "linux#7"], "the overview brings this desktop's minimized");
    assert_eq!(ids(&v["placeholder"]), ["linux#5", "linux#6"]);
    op(&mut s, "setActiveDesktop", json!([2]));
    assert_eq!(ids(&state(&mut s)["views"]["mounted"]), ["linux#6"], "not another desktop's");
}

// REGRESSION: the keyboard never lands on a hidden desktop, whatever the stack looks like
#[test]
fn the_front_window_is_never_on_a_hidden_desktop() {
    let pool = vec![lin(1, 1, json!({})), lin(2, 2, json!({})), lin(3, 3, json!({})), win("x", 2, json!({})),
        lin(4, 1, json!({ "minimized": true })), lin(6, 2, json!({ "minimized": true }))];
    fn perms(v: Vec<Value>) -> Vec<Vec<Value>> {
        if v.len() < 2 {
            return vec![v];
        }
        let mut out = Vec::new();
        for i in 0..v.len() {
            let mut rest = v.clone();
            let x = rest.remove(i);
            for mut p in perms(rest) {
                p.insert(0, x.clone());
                out.push(p);
            }
        }
        out
    }
    let mut wrong = 0;
    let all = perms(pool);
    assert_eq!(all.len(), 720);
    for order in all {
        let mut s = session(Value::Array(order.clone()), json!([1, 2, 3, 4]), 1);
        for active in 1..=4 {
            op(&mut s, "setActiveDesktop", json!([active]));
            let front = state(&mut s)["views"]["front"].clone();
            if let Some(id) = front.as_str() {
                let w = order.iter().find(|w| w["id"] == json!(id)).unwrap();
                if w["desktop"] != json!(active) || truthy(&w["minimized"]) {
                    wrong += 1;
                }
            }
        }
    }
    assert_eq!(wrong, 0);
}

#[test]
fn which_window_an_app_comes_back_with() {
    let app = json!([win("t1", 1, json!({})), win("t2", 2, json!({})), win("t3", 2, json!({ "minimized": true }))]);
    let mut s = session(app, json!([1, 2, 3]), 2);
    let pick = |s: &mut Session, list: Value| op(s, "pick", json!([list]))["result"].clone();
    assert_eq!(pick(&mut s, json!(["t1", "t2", "t3"])), json!("t2"), "here");
    op(&mut s, "setActiveDesktop", json!([1]));
    assert_eq!(pick(&mut s, json!(["t1", "t2", "t3"])), json!("t1"));
    op(&mut s, "setActiveDesktop", json!([3]));
    assert_eq!(pick(&mut s, json!(["t1", "t2", "t3"])), json!("t2"), "none here: the latest");
    assert_eq!(pick(&mut s, json!(["t3"])), json!("t3"), "only a minimized one");
    assert_eq!(pick(&mut s, json!([])), Value::Null);
}

#[test]
fn opening_places_and_raises() {
    let mut s = Session::default();
    let id = op(&mut s, "open", json!([{ "app": "files", "size": [800, 500], "area": { "x": 0, "y": 28, "w": 1280, "h": 772 } }]))["result"].clone();
    assert_eq!(id, json!("files"));
    let w = window(&mut s, "files");
    assert_eq!((w["x"].clone(), w["y"].clone(), w["w"].clone(), w["h"].clone()), (json!(180), json!(124), json!(800), json!(500)));
    assert_eq!((w["desktop"].clone(), w["title"].clone(), w["args"].clone()), (json!(1), Value::Null, Value::Null));
    let second = op(&mut s, "open", json!([{ "app": "files", "new": true, "args": { "path": "/tmp" }, "size": [800, 500], "area": { "x": 0, "y": 28, "w": 1280, "h": 772 } }]));
    assert_eq!(second["result"], json!("files#2"));
    let w2 = window(&mut s, "files#2");
    assert_eq!((w2["x"].clone(), w2["args"]["path"].clone(), w2["args"]["nonce"].clone()), (json!(210), json!("/tmp"), json!(3)), "cascaded, with a nonce");
    // opening it again raises it and passes the new args
    op(&mut s, "open", json!([{ "app": "files", "args": { "path": "/home" } }]));
    let st = state(&mut s);
    assert_eq!(st["views"]["top"], json!("files"));
    assert_eq!(window(&mut s, "files")["args"]["path"], json!("/home"));
    // a frame asked for, and the place the app was left in
    op(&mut s, "open", json!([{ "app": "a", "frame": { "x": 5, "y": 6, "w": 300, "h": 200 } }]));
    assert_eq!(window(&mut s, "a")["x"], json!(5));
    op(&mut s, "open", json!([{ "app": "b", "memory": { "x": 7, "y": 8, "w": 300, "h": 200, "maximized": true } }]));
    assert_eq!((window(&mut s, "b")["x"].clone(), window(&mut s, "b")["maximized"].clone()), (json!(7), json!(true)));
    assert!(s.call("open", &[facts(), json!({ "app": "" })]).is_err());
    assert_eq!(op(&mut s, "open", json!([{ "app": "t", "instance": 5 }]))["result"], json!("t#5"), "a numeric instance");
    assert_eq!(op(&mut s, "open", json!([{ "app": "t", "instance": 0 }]))["result"], json!("t"), "0 is no instance");
}

#[test]
fn focus_brings_its_desktop_and_close_removes() {
    let mut s = session(json!([win("a", 1, json!({})), win("b", 2, json!({ "minimized": true })), win("c", 1, json!({}))]), json!([1, 2]), 1);
    op(&mut s, "focus", json!(["b"]));
    let st = state(&mut s);
    assert_eq!((st["active"].clone(), st["views"]["top"].clone()), (json!(2), json!("b")), "its desktop shows and it comes up");
    op(&mut s, "raise", json!(["a"]));
    assert_eq!(state(&mut s)["active"], json!(2), "raise keeps the desktop");
    assert_eq!(op(&mut s, "close", json!(["a"]))["result"], json!(true));
    assert_eq!(op(&mut s, "close", json!(["a"]))["result"], Value::Null, "already closed");
    op(&mut s, "moveToDesktop", json!(["c", 2]));
    assert_eq!(window(&mut s, "c")["desktop"], json!(2));
    op(&mut s, "moveToDesktop", json!(["c", 9]));
    assert_eq!(window(&mut s, "c")["desktop"], json!(2), "no such desktop");
}

#[test]
fn maximize_snap_and_restore() {
    let mut s = session(json!([{ "id": "files", "app": "files", "x": 100, "y": 100, "w": 500, "h": 400 }]), json!([1]), 1);
    op(&mut s, "snap", json!(["files", "left"]));
    let w = window(&mut s, "files");
    assert_eq!((w["x"].clone(), w["y"].clone(), w["w"].clone(), w["h"].clone(), w["snapped"].clone()), (json!(0), json!(28), json!(640), json!(772), json!("left")));
    assert_eq!(w["restore"], json!({ "x": 100, "y": 100, "w": 500, "h": 400 }));
    op(&mut s, "snap", json!(["files", "right"]));
    assert_eq!(window(&mut s, "files")["restore"]["x"], json!(100), "the size from before the first snap");
    op(&mut s, "toggleMaximize", json!(["files"]));
    let w = window(&mut s, "files");
    assert_eq!((w["maximized"].clone(), w["snapped"].clone(), w["x"].clone()), (json!(true), Value::Null, json!(100)));
    op(&mut s, "snap", json!(["files", "restore"]));
    assert_eq!(window(&mut s, "files")["maximized"], json!(false));
    // Super+arrows: from a free window, Snap Assist follows
    assert_eq!(op(&mut s, "snapKey", json!(["files", "left"]))["result"], json!("left"));
    assert_eq!(op(&mut s, "snapKey", json!(["files", "up"]))["result"], Value::Null);
    assert_eq!(window(&mut s, "files")["snapped"], json!("top-left"));
    op(&mut s, "snapKey", json!(["files", "right"]));
    op(&mut s, "snapKey", json!(["files", "up"]));
    assert_eq!(window(&mut s, "files")["maximized"], json!(true));
    op(&mut s, "snapKey", json!(["files", "down"]));
    op(&mut s, "snapKey", json!(["files", "down"]));
    assert_eq!(window(&mut s, "files")["minimized"], json!(true));
    // the app's minimum size wins over a small zone
    let mut small = Session::default();
    small.call("setDesktops", &[facts(), json!([1]), json!(1)]).unwrap();
    small.debug_load(&[json!({ "id": "files", "app": "files", "x": 0, "y": 0, "w": 500, "h": 400 })]);
    let tiny = json!({ "displays": [{ "id": 1, "x": 0, "y": 0, "w": 600, "h": 400, "main": true }], "apps": { "files": { "minSize": [400, 300] } } });
    small.call("snap", &[tiny, json!("files"), json!("top-left")]).unwrap();
    assert_eq!(small.debug_window("files")["w"], json!(400));
}

#[test]
fn a_dropped_window_snaps_or_comes_out() {
    let mut s = session(json!([{ "id": "files", "app": "files", "x": 300, "y": 200, "w": 500, "h": 400 }]), json!([1]), 1);
    let start = json!({ "x": 300, "y": 200, "w": 500, "h": 400 });
    state(&mut s); // the mirror has it all
    // a click on the bar changes nothing
    assert_eq!(op(&mut s, "grabEnd", json!([{ "id": "files", "f": start, "edges": 0, "dx": 0, "dy": 0, "dw": 0, "dh": 0 }]))["state"], Value::Null);
    let r = op(&mut s, "grabEnd", json!([{ "id": "files", "f": start, "edges": 0, "dx": -300, "dy": 10, "pointer": { "x": 2, "y": 400 }, "zone": "left" }]));
    assert_eq!(r["result"], json!({ "assist": "left", "display": 0 }));
    let w = window(&mut s, "files");
    assert_eq!((w["snapped"].clone(), w["restore"].clone()), (json!("left"), json!({ "x": 0, "y": 210, "w": 500, "h": 400 })));
    // dragged out: back to its size under the pointer, where it was grabbed
    let snapped = json!({ "x": 0, "y": 28, "w": 640, "h": 772 });
    op(&mut s, "grabEnd", json!([{ "id": "files", "f": snapped, "edges": 0, "dx": 400, "dy": 100, "pointer": { "x": 720, "y": 140 } }]));
    let w = window(&mut s, "files");
    assert_eq!((w["w"].clone(), w["snapped"].clone(), w["y"].clone()), (json!(500), Value::Null, json!(128)));
    assert_eq!(w["x"], json!(720 - 250), "the pointer at the same share of its width");
    // a plain resize
    let now = window(&mut s, "files");
    let f = json!({ "x": now["x"], "y": now["y"], "w": now["w"], "h": now["h"] });
    op(&mut s, "grabEnd", json!([{ "id": "files", "f": f, "edges": 8, "dw": 40 }]));
    assert_eq!(window(&mut s, "files")["w"], json!(540));
}

#[test]
fn a_linked_resize_keeps_both_snapped() {
    let l = json!({ "id": "l", "app": "files", "x": 0, "y": 28, "w": 640, "h": 772, "snapped": "left", "restore": { "x": 1, "y": 1, "w": 1, "h": 1 } });
    let r = json!({ "id": "r", "app": "files", "x": 640, "y": 28, "w": 640, "h": 772, "snapped": "right", "restore": { "x": 1, "y": 1, "w": 1, "h": 1 } });
    let mut s = session(json!([l, r]), json!([1]), 1);
    op(&mut s, "grabEnd", json!([{ "id": "l", "f": { "x": 0, "y": 28, "w": 640, "h": 772 }, "edges": 8, "dw": 60, "links": [{ "id": "r", "dx": 60, "dw": -60 }] }]));
    let (wl, wr) = (window(&mut s, "l"), window(&mut s, "r"));
    assert_eq!((wl["w"].clone(), wr["x"].clone(), wr["w"].clone()), (json!(700), json!(700), json!(580)));
    assert_eq!((wl["snapped"].clone(), wr["snapped"].clone()), (json!("left"), json!("right")));
}

#[test]
fn geometry_rules() {
    let mut s = session(json!([{ "id": "a", "app": "files", "x": 900, "y": 500, "w": 600, "h": 400 }]), json!([1]), 1);
    op(&mut s, "move", json!(["a", 10.4, 3]));
    let w = window(&mut s, "a");
    assert_eq!((w["x"].clone(), w["y"].clone()), (json!(10), json!(28)), "never above the work area");
    op(&mut s, "center", json!(["a"]));
    let w = window(&mut s, "a");
    assert_eq!((w["x"].clone(), w["y"].clone()), (json!(340), json!(214)));
    op(&mut s, "liftForKeyboard", json!(["a", { "y": 40 }]));
    assert_eq!(window(&mut s, "a")["oskFrame"]["y"], json!(214));
    op(&mut s, "dropFromKeyboard", json!(["a"]));
    let w = window(&mut s, "a");
    assert_eq!((w["y"].clone(), w["oskFrame"].clone()), (json!(214), Value::Null));
    op(&mut s, "patch", json!(["a", { "title": "Doc", "x": 1, "protected": true }]));
    let w = window(&mut s, "a");
    assert_eq!((w["title"].clone(), w["protected"].clone(), w["x"].clone()), (json!("Doc"), json!(true), json!(340)), "only its own fields");
    op(&mut s, "setArgs", json!(["a", { "csd": true }]));
    op(&mut s, "setArgs", json!(["a", { "k": 1 }]));
    assert_eq!(window(&mut s, "a")["args"], json!({ "csd": true, "k": 1 }));
    // a display that is gone: the window comes back on the main one
    s.debug_load(&[json!({ "id": "far", "app": "files", "x": 5000, "y": 100, "w": 600, "h": 400 })]);
    op(&mut s, "screen", json!([]));
    let w = window(&mut s, "far");
    assert_eq!((w["x"].clone(), w["y"].clone()), (json!(340), json!(214)));
}

#[test]
fn pip_fullscreen_and_cycling() {
    let mut s = session(json!([win("a", 1, json!({})), win("b", 1, json!({})), win("c", 1, json!({}))]), json!([1, 2]), 1);
    op(&mut s, "setFullscreen", json!(["b", true]));
    assert_eq!(op(&mut s, "exitFullscreen", json!([]))["result"], json!(true));
    assert_eq!(op(&mut s, "exitFullscreen", json!([]))["result"], Value::Null);
    op(&mut s, "cycle", json!(["left"]));
    assert_eq!(ids(&state(&mut s)["views"]["visible"]), ["b", "c", "a"], "the rearmost comes up");
    op(&mut s, "cycle", json!(["right"]));
    assert_eq!(ids(&state(&mut s)["views"]["visible"]), ["a", "b", "c"], "the front one goes under");
    op(&mut s, "toBack", json!(["c"]));
    assert_eq!(ids(&state(&mut s)["views"]["visible"])[0], "c");
    op(&mut s, "setPip", json!(["a", { "x": 1, "y": 1, "w": 200, "h": 120 }]));
    op(&mut s, "setActiveDesktop", json!([2]));
    assert_eq!(ids(&state(&mut s)["views"]["desk"]["2"]), ["a"], "on every desktop");
    op(&mut s, "setPip", json!(["a", null]));
    assert_eq!(window(&mut s, "a")["desktop"], json!(2), "back on the one in view");
}

#[test]
fn desktops_come_and_go() {
    let mut s = session(json!([win("a", 2, json!({})), win("b", 3, json!({}))]), json!([1, 2, 3]), 2);
    assert_eq!(op(&mut s, "createDesktop", json!([]))["result"], json!(4));
    assert_eq!(op(&mut s, "closeDesktop", json!([2]))["result"], json!(true));
    let st = state(&mut s);
    assert_eq!((st["desktops"].clone(), st["active"].clone()), (json!([1, 3, 4]), json!(3)), "the next one takes its windows and shows");
    assert_eq!(window(&mut s, "a")["desktop"], json!(3));
    op(&mut s, "closeDesktop", json!([4]));
    op(&mut s, "closeDesktop", json!([3]));
    assert_eq!(op(&mut s, "closeDesktop", json!([1]))["result"], json!(false), "the last stays");
    for _ in 0..20 {
        op(&mut s, "createDesktop", json!([]));
    }
    assert_eq!(state(&mut s)["desktops"].as_array().unwrap().len(), 9);
    assert_eq!(op(&mut s, "createDesktop", json!([]))["result"], Value::Null);
}

#[test]
fn the_mirror_gets_only_what_changed() {
    let mut s = session(json!([win("a", 1, json!({})), win("b", 1, json!({}))]), json!([1]), 1);
    let first = state(&mut s);
    assert!(first["windows"].as_array().unwrap().iter().all(|w| w.get("w").is_some()), "everything the first time");
    let r = op(&mut s, "patch", json!(["b", { "title": "B" }]));
    let ws = r["state"]["windows"].as_array().unwrap();
    assert!(ws[0].get("w").is_none() && ws[1]["w"]["title"] == json!("B"), "only b again: {ws:?}");
    assert!(ws[1]["rev"].as_u64() > ws[0]["rev"].as_u64());
    assert_eq!(op(&mut s, "patch", json!(["b", { "title": "B" }]))["state"], Value::Null, "nothing changed, nothing sent");
    assert!(op(&mut s, "raise", json!(["a"]))["state"].is_object(), "the order changed");
    assert!(s.call("nothing", &[facts()]).is_err());
    let again = op(&mut s, "resync", json!([]));
    assert!(again["state"]["windows"].as_array().unwrap().iter().all(|w| w.get("w").is_some()), "resync sends everything");
}
