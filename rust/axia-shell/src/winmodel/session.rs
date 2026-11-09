//! The session's windows (docs/WINDOWS.md, docs/DESKTOPS.md): the list in stacking order (bottom to
//! top), the desktops and the one showing, and every rule that changes them. sdk/windows.js keeps a
//! mirror for React and sends the facts each rule needs (the displays and their work areas, the apps'
//! sizes); the views it draws from (what shows, what is mounted, who is in front) are computed here
//! once per change.
//!
//! A window is a JSON object with the fields themes read (id, app, title, args, desktop, x, y, w, h,
//! minimized, maximized, snapped, restore, fullscreen, pip, oskFrame, windowMenu, protected). Each one
//! has a revision: the mirror replaces only the objects that changed, so React keeps the others.
use super::{get, js_round, num, number, same, truthy, Rect};
use crate::snap::{self, Dir, Next, Zone};
use serde_json::{json, Map, Value};

pub const FIRST_DESKTOP: i64 = 1;
const MAX_DESKTOPS: usize = super::desktops::MAX_DESKTOPS;
/// More windows than this are refused (an app opening windows in a loop).
pub const MAX_WINDOWS: usize = 512;

#[derive(Clone, Debug)]
struct Win {
    rev: u64,
    data: Map<String, Value>,
}

impl Win {
    fn id(&self) -> &str {
        self.data.get("id").and_then(Value::as_str).unwrap_or("")
    }
    fn f(&self, k: &str) -> f64 {
        self.data.get(k).map_or(f64::NAN, number)
    }
    fn on(&self, k: &str) -> bool {
        self.data.get(k).is_some_and(truthy)
    }
    fn set(&mut self, k: &str, v: Value) {
        self.data.insert(k.to_owned(), v);
    }
    fn rect(&self) -> Rect {
        Rect::new(self.f("x"), self.f("y"), self.f("w"), self.f("h"))
    }
    fn set_rect(&mut self, r: Rect) {
        self.set("x", num(r.x));
        self.set("y", num(r.y));
        self.set("w", num(r.w));
        self.set("h", num(r.h));
    }
    fn desktop(&self) -> i64 {
        self.data.get("desktop").and_then(Value::as_f64).map_or(FIRST_DESKTOP, |d| d as i64)
    }
    fn surface(&self) -> bool {
        self.data.get("args").is_some_and(|a| truthy(get(a, "surface")))
    }
    fn app(&self) -> &str {
        self.data.get("app").and_then(Value::as_str).unwrap_or("")
    }
    fn zone(&self) -> Option<Zone> {
        self.data.get("snapped").and_then(Value::as_str).and_then(Zone::from_name)
    }
    /// Merges `p` into the window (JavaScript's { ...w, ...p }).
    fn patch(&mut self, p: Value) {
        if let Value::Object(p) = p {
            for (k, v) in p {
                self.data.insert(k, v);
            }
        }
    }
    fn unsnapped() -> Value {
        json!({ "maximized": false, "snapped": null, "restore": null })
    }
}

/// One display: its id, bounds, work area and whether it is the main one.
#[derive(Clone, Debug)]
struct Display {
    id: Value,
    b: Rect,
    work: Rect,
    main: bool,
}

/// What a rule needs from the session that only JS knows: the displays (in lord.screen's order, with
/// the theme's work areas), the apps' sizes, and whether the theme snaps windows.
#[derive(Debug, Default)]
pub struct Facts {
    displays: Vec<Display>,
    apps: Map<String, Value>,
    snap: bool,
}

impl Facts {
    pub fn from_value(v: &Value) -> Facts {
        let displays = get(v, "displays")
            .as_array()
            .map(|l| {
                l.iter()
                    .filter_map(|d| {
                        let b = Rect::from_value(d)?;
                        Some(Display { id: get(d, "id").clone(), b, work: Rect::from_value(get(d, "work")).unwrap_or(b), main: truthy(get(d, "main")) })
                    })
                    .take(16)
                    .collect()
            })
            .unwrap_or_default();
        let apps = get(v, "apps").as_object().cloned().unwrap_or_default();
        Facts { displays, apps, snap: truthy(get(v, "snap")) }
    }

    fn main(&self) -> Option<usize> {
        if self.displays.is_empty() {
            return None;
        }
        Some(self.displays.iter().position(|d| d.main).unwrap_or(0))
    }

    /// The display containing a point, else the nearest one (runtime/index.js displayAt).
    fn display_at(&self, x: f64, y: f64) -> Option<usize> {
        let mut best = None;
        let mut dist = f64::INFINITY;
        for (i, d) in self.displays.iter().enumerate() {
            let dx = (d.b.x - x).max(0.0).max(x - (d.b.x + d.b.w - 1.0));
            let dy = (d.b.y - y).max(0.0).max(y - (d.b.y + d.b.h - 1.0));
            let k = dx * dx + dy * dy;
            if k < dist {
                dist = k;
                best = Some(i);
            }
        }
        best.or_else(|| self.main())
    }

    /// The display with most of a rectangle, else the one at its center (runtime/index.js displayFor).
    fn display_for(&self, r: Rect) -> Option<usize> {
        let mut best = None;
        let mut area = 0.0;
        for (i, d) in self.displays.iter().enumerate() {
            let w = (r.x + r.w).min(d.b.x + d.b.w) - r.x.max(d.b.x);
            let h = (r.y + r.h).min(d.b.y + d.b.h) - r.y.max(d.b.y);
            if w > 0.0 && h > 0.0 && w * h > area {
                area = w * h;
                best = Some(i);
            }
        }
        best.or_else(|| self.display_at(r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    fn work(&self, i: Option<usize>) -> Option<Rect> {
        i.and_then(|i| self.displays.get(i)).map(|d| d.work)
    }

    fn app_pair(&self, app: &str, key: &str, default: (f64, f64)) -> (f64, f64) {
        self.apps
            .get(app)
            .and_then(|a| a.get(key))
            .and_then(Value::as_array)
            .and_then(|s| Some((s.first()?.as_f64()?, s.get(1)?.as_f64()?)))
            .unwrap_or(default)
    }
    fn min_size(&self, app: &str) -> (f64, f64) {
        self.app_pair(app, "minSize", (160.0, 100.0))
    }
}

#[derive(Debug)]
pub struct Session {
    wins: Vec<Win>,
    desktops: Vec<i64>,
    active: i64,
    overview: bool,
    seq: u64,
    clock: u64,
    sent: u64,
}

impl Default for Session {
    fn default() -> Self {
        Session { wins: Vec::new(), desktops: vec![FIRST_DESKTOP], active: FIRST_DESKTOP, overview: false, seq: 2, clock: 0, sent: 0 }
    }
}

fn rect_value(r: Rect) -> Value {
    r.to_value()
}

impl Session {
    fn find(&self, id: &str) -> Option<usize> {
        self.wins.iter().position(|w| w.id() == id)
    }
    fn here(&self, w: &Win) -> bool {
        w.on("pip") || w.desktop() == self.active
    }

    // ---- stacking ----

    /// To the top, and shown.
    fn raise(&mut self, i: usize) {
        let mut w = self.wins.remove(i);
        w.set("minimized", Value::Bool(false));
        self.wins.push(w);
    }
    fn raise_unless_top(&mut self, id: &str) {
        let Some(i) = self.find(id) else { return };
        if i + 1 == self.wins.len() && !self.wins[i].on("minimized") {
            return;
        }
        self.raise(i);
    }
    /// Bringing a window forward shows its desktop, except picture-in-picture (on all of them).
    fn show_desktop_of(&mut self, id: &str) {
        if let Some(i) = self.find(id) {
            let d = self.wins[i].desktop();
            if !self.here(&self.wins[i]) && self.desktops.contains(&d) {
                self.active = d;
            }
        }
    }
    fn focus(&mut self, id: &str) {
        self.show_desktop_of(id);
        self.raise_unless_top(id);
    }

    // ---- opening ----

    /// Where a new window goes: the frame asked for, the place it was left in, or centered and
    /// cascaded inside the work area where it opens (`area`).
    fn place(count: usize, o: &Value) -> Value {
        if let Some(f) = get(o, "frame").as_object() {
            let mut f = f.clone();
            f.insert("minimized".into(), Value::Bool(false));
            f.insert("maximized".into(), Value::Bool(false));
            return Value::Object(f);
        }
        if let Some(k) = get(o, "memory").as_object() {
            let mut k = k.clone();
            k.insert("minimized".into(), Value::Bool(false));
            return Value::Object(k);
        }
        let size = get(o, "size").as_array().and_then(|s| Some((s.first()?.as_f64()?, s.get(1)?.as_f64()?)));
        let (sw, sh) = size.unwrap_or((640.0, 420.0));
        let a = Rect::from_value(get(o, "area")).unwrap_or(Rect::new(0.0, 0.0, sw, sh));
        let offset = (count % 6) as f64 * 30.0;
        // the whole window stays inside the work area: a cascaded window must not slide under the Dock
        let (w, h) = (sw.min(a.w), sh.min(a.h));
        let x = js_round(a.x + (a.w - w) / 2.0 - 60.0 + offset);
        let y = js_round((a.y + 16.0).max(a.y + (a.h - h) / 2.0 - 40.0 + offset));
        json!({
            "w": num(w), "h": num(h), "minimized": false, "maximized": false,
            "x": num(a.x.max(x.min(a.x + a.w - w))),
            "y": num(a.y.max(y.min(a.y + a.h - h))),
        })
    }

    /// o: { app, instance, new, args, desktop, frame, memory, size, area } → the window's id.
    fn open(&mut self, o: &Value) -> Result<Value, String> {
        let app = get(o, "app").as_str().filter(|a| !a.is_empty()).ok_or("open: no app")?.to_owned();
        let id = if let Some(inst) = get(o, "instance").as_str().filter(|s| !s.is_empty()) {
            format!("{app}#{inst}")
        } else if truthy(get(o, "new")) {
            let id = format!("{app}#{}", self.seq);
            self.seq += 1;
            id
        } else {
            app.clone()
        };
        let args = match get(o, "args") {
            Value::Object(a) => {
                let mut a = a.clone();
                a.insert("nonce".into(), Value::from(self.seq));
                self.seq += 1;
                Value::Object(a)
            }
            _ => Value::Null,
        };
        let asked = get(o, "desktop").as_f64().map(|d| d as i64);
        let desktop = asked.filter(|d| self.desktops.contains(d)).unwrap_or(self.active);
        self.show_desktop_of(&id);
        if let Some(i) = self.find(&id) {
            self.raise(i);
            if !args.is_null() {
                self.wins.last_mut().unwrap().set("args", args);
            }
        } else {
            if self.wins.len() >= MAX_WINDOWS {
                return Err(format!("open: more than {MAX_WINDOWS} windows"));
            }
            let mut w = Win { rev: 0, data: Map::new() };
            w.set("id", Value::String(id.clone()));
            w.set("app", Value::String(app));
            w.set("title", Value::Null);
            w.set("args", args);
            w.set("desktop", Value::from(desktop));
            w.patch(Self::place(self.wins.len(), o));
            self.wins.push(w);
        }
        Ok(Value::String(id))
    }

    // ---- snapping (docs/WINDOWS.md "Snapping") ----

    fn snap_to(&mut self, f: &Facts, i: usize, zone: Zone, display: Option<usize>) {
        let w = &self.wins[i];
        let restore = if w.zone().is_some() { get_value(w, "restore") } else { rect_value(w.rect()) };
        if zone == Zone::Max {
            let mut p = json!({ "maximized": true, "minimized": false, "snapped": null, "restore": null });
            if w.zone().is_some() {
                if let Value::Object(r) = &restore {
                    for (k, v) in r {
                        p[k.as_str()] = v.clone();
                    }
                }
            }
            self.wins[i].patch(p);
            return;
        }
        let Some(a) = f.work(display.or_else(|| f.display_for(w.rect()))) else { return };
        let (min_w, min_h) = f.min_size(w.app());
        let r = snap::zone_rect(zone, crate::snap::SnapRect::new(a.x, a.y, a.w, a.h));
        let w = &mut self.wins[i];
        w.set_rect(Rect::new(r.x, r.y, r.w.max(min_w.min(a.w)), r.h.max(min_h.min(a.h))));
        w.patch(json!({ "maximized": false, "minimized": false, "snapped": zone.name(), "restore": restore }));
    }

    fn unsnap(&mut self, i: usize) {
        let w = &mut self.wins[i];
        if w.on("maximized") {
            w.set("maximized", Value::Bool(false));
            return;
        }
        if w.zone().is_none() {
            return;
        }
        let r = get_value(w, "restore");
        w.patch(r);
        w.patch(json!({ "snapped": null, "restore": null }));
    }

    fn toggle_maximize(&mut self, i: usize) {
        let w = &mut self.wins[i];
        if w.on("snapped") && w.on("restore") {
            let r = get_value(w, "restore");
            w.patch(r);
            w.patch(json!({ "snapped": null, "restore": null, "maximized": true }));
        } else {
            let m = !w.on("maximized");
            w.set("maximized", Value::Bool(m));
        }
    }

    /// Super+arrows. The zone to offer Snap Assist for, when the window was free.
    fn snap_key(&mut self, f: &Facts, i: usize, dir: Dir) -> Value {
        let w = &self.wins[i];
        if w.on("fullscreen") || w.on("pip") {
            return Value::Null;
        }
        let at = if w.on("maximized") { Some(Zone::Max) } else { w.zone() };
        let was_free = w.zone().is_none() && !w.on("maximized");
        match snap::snap_next(at, dir) {
            Some(Next::Minimize) => self.wins[i].set("minimized", Value::Bool(true)),
            Some(Next::Restore) => self.unsnap(i),
            Some(Next::Zone(z)) => {
                self.snap_to(f, i, z, None);
                if was_free && z != Zone::Max {
                    return Value::String(z.name().into());
                }
            }
            None => {}
        }
        Value::Null
    }

    fn arrange(&mut self, f: &Facts, ids: &[Value], layout: &str) {
        use Zone::*;
        let zones: &[Zone] = match layout {
            "left-right" => &[Left, Right],
            "right-left" => &[Right, Left],
            "top-bottom" => &[Top, Bottom],
            "bottom-top" => &[Bottom, Top],
            "quarters" => &[TopLeft, TopRight, BottomLeft, BottomRight],
            _ => return,
        };
        let ids: Vec<String> = ids.iter().filter_map(|v| v.as_str().map(str::to_owned)).take(zones.len()).collect();
        let Some(first) = ids.first().and_then(|id| self.find(id)) else { return };
        let d = f.display_for(self.wins[first].rect());
        for (id, z) in ids.iter().zip(zones) {
            if let Some(i) = self.find(id) {
                self.snap_to(f, i, *z, d);
            }
        }
        for id in ids.iter().rev() {
            self.focus(id);
        }
    }

    // ---- geometry ----

    fn center(&mut self, f: &Facts, i: usize) {
        let w = &self.wins[i];
        let Some(a) = f.work(f.display_for(w.rect())) else { return };
        let r = if w.on("snapped") && w.on("restore") { w.data.get("restore").and_then(Rect::from_value).unwrap_or(w.rect()) } else { w.rect() };
        let (ww, hh) = (r.w.min(a.w), r.h.min(a.h));
        let w = &mut self.wins[i];
        w.set_rect(Rect::new(js_round(a.x + (a.w - ww) / 2.0), js_round(a.y + (a.h - hh) / 2.0), ww, hh));
        w.patch(Win::unsnapped());
    }

    fn move_to_display(&mut self, f: &Facts, i: usize, display: &Value) {
        let Some(d) = f.displays.iter().position(|x| same(&x.id, display)) else { return };
        if let Some(z) = self.wins[i].zone() {
            self.snap_to(f, i, z, Some(d));
            return;
        }
        let w = self.wins[i].rect();
        let (Some(from), Some(to)) = (f.work(f.display_for(w)), f.work(Some(d))) else { return };
        let (ww, hh) = (w.w.min(to.w), w.h.min(to.h));
        let fx = if from.w > w.w { (w.x - from.x) / (from.w - w.w) } else { 0.0 };
        let fy = if from.h > w.h { (w.y - from.y) / (from.h - w.h) } else { 0.0 };
        let clamp = |v: f64| v.clamp(0.0, 1.0);
        self.wins[i].set_rect(Rect::new(js_round(to.x + clamp(fx) * (to.w - ww)), js_round(to.y + clamp(fy) * (to.h - hh)), ww, hh));
    }

    fn move_to(&mut self, f: &Facts, i: usize, x: f64, y: f64) {
        let top = f.work(f.display_at(x, y)).map_or(f64::NEG_INFINITY, |a| a.y);
        let w = &mut self.wins[i];
        w.set("x", num(js_round(x)));
        w.set("y", num(js_round(top.max(y))));
        w.patch(json!({ "maximized": false, "snapped": null, "restore": null, "oskFrame": null }));
    }

    fn set_frame(&mut self, i: usize, frame: &Value) {
        let w = &mut self.wins[i];
        w.patch(frame.clone());
        w.patch(json!({ "maximized": false, "snapped": null, "restore": null, "oskFrame": null }));
    }

    /// The on-screen keyboard moves the window with the field out of its way (sdk/osk.js), and puts
    /// it back as it was. A window the person moves meanwhile stays where they left it.
    fn lift_for_keyboard(&mut self, i: usize, frame: &Value) {
        let w = &mut self.wins[i];
        let osk = match w.data.get("oskFrame") {
            Some(v) if truthy(v) => v.clone(),
            _ => {
                let mut o = Map::new();
                for k in ["x", "y", "w", "h", "maximized", "snapped", "restore"] {
                    if let Some(v) = w.data.get(k) {
                        o.insert(k.into(), v.clone());
                    }
                }
                Value::Object(o)
            }
        };
        w.patch(frame.clone());
        w.patch(json!({ "maximized": false, "snapped": null, "restore": null, "oskFrame": osk }));
    }

    fn drop_from_keyboard(&mut self, i: usize) {
        let w = &mut self.wins[i];
        let osk = get_value(w, "oskFrame");
        if truthy(&osk) {
            w.patch(osk);
            w.set("oskFrame", Value::Null);
        }
    }

    /// The display arrangement changed: a window left on a display that is gone (or outside every
    /// display) moves to the main one; the others stay on theirs, inside its work area.
    fn screen_changed(&mut self, f: &Facts) {
        let main = f.work(f.main());
        for i in 0..self.wins.len() {
            let w = self.wins[i].rect();
            let on_some = f.displays.iter().any(|d| {
                w.x < d.b.x + d.b.w - 48.0 && w.x + w.w > d.b.x + 48.0 && w.y < d.b.y + d.b.h - 24.0 && w.y + 24.0 > d.b.y
            });
            let Some(a) = (if on_some { f.work(f.display_for(w)) } else { main }) else { continue };
            let (ww, hh) = (w.w.min(a.w), w.h.min(a.h));
            let keep = on_some && w.x + ww > a.x && w.x < a.x + a.w;
            let x = if keep { w.x.max(a.x - ww + 96.0).min(a.x + a.w - 96.0) } else { a.x + (a.w - ww) / 2.0 };
            let y = if keep { w.y.max(a.y).min(a.y + a.h - hh.min(48.0)) } else { a.y + ((a.h - hh) / 2.0).max(0.0) };
            self.wins[i].set_rect(Rect::new(js_round(x), js_round(y), ww, hh));
        }
    }

    /// A drag in C ended (__lordWm('end')). g: { id, f: the frame at the start, edges, dx, dy, dw, dh,
    /// pointer: { x, y }, zone, links: [{ id, dx, dy, dw, dh }] }. Returns { assist, display } when the
    /// window snapped by being dropped (Snap Assist follows), else null.
    fn grab_end(&mut self, f: &Facts, g: &Value) -> Value {
        let id = get(g, "id").as_str().unwrap_or_default().to_owned();
        let d = |k: &str| get(g, k).as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
        let (dx, dy, dw, dh) = (d("dx"), d("dy"), d("dw"), d("dh"));
        if dx == 0.0 && dy == 0.0 && dw == 0.0 && dh == 0.0 {
            return Value::Null; // a click on the bar
        }
        let Some(start) = Rect::from_value(get(g, "f")) else { return Value::Null };
        let edges = get(g, "edges").as_i64().unwrap_or(0);
        let (nx, ny) = (js_round(start.x + dx), js_round(start.y + dy));
        let Some(i) = self.find(&id) else { return Value::Null };
        let links = get(g, "links").as_array().cloned().unwrap_or_default();
        if edges != 0 && !links.is_empty() {
            // a linked resize: the windows along the line keep their snapped places, at their new sizes
            self.wins[i].set_rect(Rect::new(nx, ny, js_round(start.w + dw), js_round(start.h + dh)));
            for l in &links {
                let Some(j) = get(l, "id").as_str().and_then(|lid| self.find(lid)) else { continue };
                let r = self.wins[j].rect();
                let ld = |k: &str| number(get(l, k));
                self.wins[j].set_rect(Rect::new(js_round(r.x + ld("dx")), js_round(r.y + ld("dy")), js_round(r.w + ld("dw")), js_round(r.h + ld("dh"))));
            }
            return Value::Null;
        }
        let pointer = (number(get(get(g, "pointer"), "x")), number(get(get(g, "pointer"), "y")));
        if f.snap && edges == 0 && pointer.0.is_finite() && pointer.1.is_finite() {
            if let Some(zone) = get(g, "zone").as_str().and_then(Zone::from_name) {
                let display = f.display_at(pointer.0, pointer.1);
                if self.wins[i].zone().is_none() {
                    // the size to go back to: from before the first snap
                    self.wins[i].set_rect(Rect::new(nx, ny, start.w, start.h));
                }
                self.snap_to(f, i, zone, display);
                return json!({ "assist": zone.name(), "display": display });
            }
            let w = &self.wins[i];
            if w.zone().is_some() && w.on("restore") {
                // dragged out of a snapped place: back to its size, under the pointer where it was grabbed
                let r = w.data.get("restore").and_then(Rect::from_value).unwrap_or(start);
                let rel = if start.w != 0.0 { ((pointer.0 - nx) / start.w).clamp(0.0, 1.0) } else { 0.5 };
                let w = &mut self.wins[i];
                w.set_rect(Rect::new(js_round(pointer.0 - rel * r.w), ny, r.w, r.h));
                w.patch(Win::unsnapped());
                return Value::Null;
            }
        }
        let w = &mut self.wins[i];
        w.set_rect(Rect::new(nx, ny, js_round(start.w + dw), js_round(start.h + dh)));
        w.set("maximized", Value::Bool(false));
        if edges != 0 {
            w.patch(json!({ "snapped": null, "restore": null }));
        }
        Value::Null
    }

    // ---- desktops (docs/DESKTOPS.md) ----

    fn create_desktop(&mut self) -> Value {
        if self.desktops.len() >= MAX_DESKTOPS {
            return Value::Null;
        }
        let id = self.desktops.iter().copied().max().unwrap_or(0).max(0) + 1;
        self.desktops.push(id);
        Value::from(id)
    }

    /// Closes a desktop: its windows move to the next one (the previous for the last). false for the
    /// last desktop.
    fn close_desktop(&mut self, id: i64) -> Value {
        let Some(i) = self.desktops.iter().position(|d| *d == id) else { return Value::Bool(false) };
        let target = if i + 1 < self.desktops.len() {
            self.desktops[i + 1]
        } else if i > 0 {
            self.desktops[i - 1]
        } else {
            return Value::Bool(false);
        };
        for w in &mut self.wins {
            if w.desktop() == id {
                w.set("desktop", Value::from(target));
            }
        }
        self.desktops.remove(i);
        if self.active == id {
            self.active = target;
        }
        Value::Bool(true)
    }

    // ---- what the theme draws ----

    fn views(&self) -> Value {
        let here = |w: &Win| self.here(w);
        let ids = |p: &dyn Fn(&Win) -> bool| -> Vec<Value> { self.wins.iter().filter(|w| p(w)).map(|w| Value::String(w.id().into())).collect() };
        let visible = ids(&|w| !w.on("minimized") && !w.on("fullscreen") && !w.on("pip") && here(w));
        // A LordOS app minimized or on a hidden desktop stays mounted (display: none) and keeps its
        // state; a Linux window has its state in its own process and no node then. In the Overview,
        // the minimized windows of this desktop are shown too.
        let mounted = ids(&|w| !w.on("fullscreen") && !w.on("pip") && (!w.surface() || (here(w) && (self.overview || !w.on("minimized")))));
        // the front window of this desktop: never another desktop's, never picture-in-picture
        let front = self.wins.iter().rev().find(|w| !w.on("minimized") && !w.on("pip") && here(w)).map(|w| w.id().to_owned());
        let hidden = ids(&|w| w.on("minimized") || !here(w));
        let parked = ids(&|w| !here(w));
        // a minimized Linux window in the Overview may have nothing to draw: a placeholder goes behind it
        let placeholder = if self.overview { ids(&|w| w.on("minimized") && w.surface()) } else { Vec::new() };
        let mut desk = Map::new();
        for d in &self.desktops {
            desk.insert(d.to_string(), Value::Array(ids(&|w| w.on("pip") || w.desktop() == *d)));
        }
        json!({
            "visible": visible, "mounted": mounted, "front": front, "top": visible.last(),
            "hidden": hidden, "parked": parked, "placeholder": placeholder, "desk": desk,
        })
    }

    /// The state for the mirror: every window as { id, rev } plus, for those changed since the last
    /// snapshot, `w` (the window).
    fn snapshot(&mut self) -> Value {
        let wins: Vec<Value> = self
            .wins
            .iter()
            .map(|w| {
                if w.rev > self.sent {
                    json!({ "id": w.id(), "rev": w.rev, "w": Value::Object(w.data.clone()) })
                } else {
                    json!({ "id": w.id(), "rev": w.rev })
                }
            })
            .collect();
        self.sent = self.clock;
        json!({ "windows": wins, "desktops": self.desktops, "active": self.active, "overview": self.overview, "views": self.views() })
    }

    /// The window that changed gets a new revision.
    fn stamp(&mut self, before: &[Win]) {
        for w in &mut self.wins {
            let old = before.iter().find(|b| b.id() == w.id());
            if old.map_or(true, |b| b.data != w.data) {
                self.clock += 1;
                w.rev = self.clock;
            }
        }
    }

    /// Runs one rule: `args` [facts, op arguments…]. Returns { result, state } (state: the snapshot,
    /// only when something changed or was asked for).
    pub fn call(&mut self, op: &str, args: &[Value]) -> Result<Value, String> {
        let facts = Facts::from_value(args.first().unwrap_or(&Value::Null));
        let a = |i: usize| args.get(i + 1).unwrap_or(&Value::Null);
        let before = self.wins.clone();
        let (desktops, active, overview) = (self.desktops.clone(), self.active, self.overview);
        let win = |s: &Self| a(0).as_str().and_then(|id| s.find(id));
        let mut result = Value::Null;
        match op {
            "state" => {}
            "open" => result = self.open(a(0))?,
            "close" => {
                if let Some(i) = win(self) {
                    self.wins.remove(i);
                    result = Value::Bool(true);
                }
            }
            "focus" => self.focus(a(0).as_str().unwrap_or_default()),
            "raise" => self.raise_unless_top(a(0).as_str().unwrap_or_default()),
            "toBack" => {
                if let Some(i) = win(self) {
                    let w = self.wins.remove(i);
                    self.wins.insert(0, w);
                }
            }
            "cycle" => {
                let vis: Vec<usize> =
                    (0..self.wins.len()).filter(|&i| { let w = &self.wins[i]; !w.on("minimized") && !w.on("fullscreen") && !w.on("pip") && self.here(w) }).collect();
                if vis.len() >= 2 {
                    if a(0).as_str() == Some("left") {
                        let w = self.wins.remove(vis[0]); // the rearmost comes up
                        self.wins.push(w);
                    } else {
                        let w = self.wins.remove(*vis.last().unwrap()); // the front one goes under everything
                        self.wins.insert(0, w);
                    }
                }
            }
            "pick" => {
                // the window an app comes back with: on this desktop if it has one, a visible one before
                // a minimized one, else the top one of any desktop
                let ids: Vec<&str> = a(0).as_array().map(|l| l.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                let group: Vec<&Win> = ids.iter().filter_map(|id| self.find(id).map(|i| &self.wins[i])).collect();
                let here: Vec<&Win> = group.iter().copied().filter(|w| self.here(w)).collect();
                let from = if here.is_empty() { &group } else { &here };
                let pick = from.iter().rev().find(|w| !w.on("minimized")).or(from.last());
                result = pick.map_or(Value::Null, |w| Value::String(w.id().into()));
            }
            "moveToDesktop" => {
                let d = a(1).as_f64().map(|d| d as i64);
                if let (Some(i), Some(d)) = (win(self), d.filter(|d| self.desktops.contains(d))) {
                    self.wins[i].set("desktop", Value::from(d));
                }
            }
            "minimize" => {
                if let Some(i) = win(self) {
                    self.wins[i].set("minimized", Value::Bool(true));
                }
            }
            "toggleMaximize" => {
                if let Some(i) = win(self) {
                    self.toggle_maximize(i);
                }
            }
            "snap" => {
                if let Some(i) = win(self) {
                    let display = a(2).as_u64().map(|d| d as usize).filter(|&d| d < facts.displays.len());
                    match a(1).as_str() {
                        Some("restore") => self.unsnap(i),
                        Some(z) => {
                            if let Some(z) = Zone::from_name(z) {
                                self.snap_to(&facts, i, z, display);
                            }
                        }
                        None => {}
                    }
                }
            }
            "snapKey" => {
                let dir = match a(1).as_str() {
                    Some("left") => Some(Dir::Left),
                    Some("right") => Some(Dir::Right),
                    Some("up") => Some(Dir::Up),
                    Some("down") => Some(Dir::Down),
                    _ => None,
                };
                if let (Some(i), Some(dir)) = (win(self), dir) {
                    result = self.snap_key(&facts, i, dir);
                }
            }
            "arrange" => self.arrange(&facts, a(0).as_array().map_or(&[], Vec::as_slice), a(1).as_str().unwrap_or_default()),
            "center" => {
                if let Some(i) = win(self) {
                    self.center(&facts, i);
                }
            }
            "moveToDisplay" => {
                if let Some(i) = win(self) {
                    self.move_to_display(&facts, i, a(1));
                }
            }
            "move" => {
                if let (Some(i), Some(x), Some(y)) = (win(self), a(1).as_f64(), a(2).as_f64()) {
                    self.move_to(&facts, i, x, y);
                }
            }
            "resize" => {
                if let (Some(i), Some(w), Some(h)) = (win(self), a(1).as_f64(), a(2).as_f64()) {
                    self.wins[i].set("w", num(js_round(w)));
                    self.wins[i].set("h", num(js_round(h)));
                }
            }
            "setFrame" => {
                if let Some(i) = win(self) {
                    self.set_frame(i, a(1));
                }
            }
            "liftForKeyboard" => {
                if let Some(i) = win(self) {
                    self.lift_for_keyboard(i, a(1));
                }
            }
            "dropFromKeyboard" => {
                if let Some(i) = win(self) {
                    self.drop_from_keyboard(i);
                }
            }
            "patch" => {
                // the fields the window manager sets as they are: title, protected, windowMenu
                if let Some(i) = win(self) {
                    let mut p = Map::new();
                    for (k, v) in a(1).as_object().into_iter().flatten() {
                        if matches!(k.as_str(), "title" | "protected" | "windowMenu") {
                            p.insert(k.clone(), v.clone());
                        }
                    }
                    self.wins[i].patch(Value::Object(p));
                }
            }
            "setArgs" => {
                if let Some(i) = win(self) {
                    let mut args = self.wins[i].data.get("args").and_then(Value::as_object).cloned().unwrap_or_default();
                    for (k, v) in a(1).as_object().into_iter().flatten() {
                        args.insert(k.clone(), v.clone());
                    }
                    self.wins[i].set("args", Value::Object(args));
                }
            }
            "setFullscreen" => {
                if let Some(i) = win(self) {
                    self.wins[i].set("fullscreen", Value::Bool(truthy(a(1))));
                    self.wins[i].set("minimized", Value::Bool(false));
                }
            }
            "exitFullscreen" => {
                // Escape: the top fullscreen window of this desktop goes back
                let top = (0..self.wins.len()).rev().find(|&i| self.wins[i].on("fullscreen") && self.here(&self.wins[i]));
                if let Some(i) = top {
                    self.wins[i].set("fullscreen", Value::Bool(false));
                    result = Value::Bool(true);
                }
            }
            "setPip" => {
                // a small frameless window on top and on every desktop; back from it, on the one in view
                if let Some(i) = win(self) {
                    let frame = a(1).as_object().map(|f| Value::Object(f.clone())).unwrap_or(Value::Null);
                    let back = frame.is_null();
                    self.wins[i].set("pip", frame);
                    self.wins[i].set("fullscreen", Value::Bool(false));
                    if back {
                        self.wins[i].set("desktop", Value::from(self.active));
                    }
                }
            }
            "grabEnd" => result = self.grab_end(&facts, a(0)),
            "screen" => self.screen_changed(&facts),
            "createDesktop" => result = self.create_desktop(),
            "closeDesktop" => result = a(0).as_f64().map_or(Value::Bool(false), |d| self.close_desktop(d as i64)),
            "setDesktops" => {
                let list: Vec<i64> = a(0).as_array().map(|l| l.iter().filter_map(Value::as_f64).map(|d| d as i64).take(MAX_DESKTOPS).collect()).unwrap_or_default();
                if !list.is_empty() {
                    self.desktops = list;
                    self.active = a(1).as_f64().map_or(self.desktops[0], |d| d as i64);
                }
            }
            "setActiveDesktop" => {
                if let Some(d) = a(0).as_f64().map(|d| d as i64).filter(|d| self.desktops.contains(d)) {
                    self.active = d;
                }
            }
            "setOverview" => self.overview = truthy(a(0)),
            _ => return Err(format!("unknown window op '{op}'")),
        }
        self.stamp(&before);
        let changed = self.clock > self.sent || self.wins.len() != before.len()
            || self.wins.iter().zip(&before).any(|(w, b)| w.id() != b.id())
            || self.desktops != desktops || self.active != active || self.overview != overview;
        let state = if changed || op == "state" { self.snapshot() } else { Value::Null };
        Ok(json!({ "result": result, "state": state }))
    }
}

fn get_value(w: &Win, k: &str) -> Value {
    w.data.get(k).cloned().unwrap_or(Value::Null)
}

#[cfg(test)]
impl Session {
    /// Tests: windows as they are (bottom to top), appended.
    pub fn debug_load(&mut self, list: &[Value]) {
        for w in list {
            if let Value::Object(data) = w {
                self.clock += 1;
                self.wins.push(Win { rev: self.clock, data: data.clone() });
            }
        }
    }
    /// Tests: a window as it is now.
    pub fn debug_window(&self, id: &str) -> Value {
        self.find(id).map_or(Value::Null, |i| Value::Object(self.wins[i].data.clone()))
    }
}
