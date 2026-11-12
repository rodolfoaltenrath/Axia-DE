//! UI-thread ownership of moving/resizing nodes and their original theme styles.
//! Geometry and snapping use native Rust owners rather than opaque FFI handles.
use super::{
    node_get, node_invalidate, node_moved, state,
    types::{Dim, Node},
};
use crate::{
    cursor,
    snap::{
        DragZones, Grab, GrabArea, GrabStart, LinkedResize, SnapDisplay, SnapMember, SnapRect,
        DETACHED, MOVED, SIZED,
    },
};
use std::{cell::RefCell, ptr};
const LINKS: usize = 8;
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Link {
    node: i32,
    inset: f32,
    min_w: f32,
    min_h: f32,
    max_w: f32,
    max_h: f32,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Limits {
    inset: f32,
    min_w: f32,
    min_h: f32,
    max_w: f32,
    max_h: f32,
    min_top: f32,
    nareas: i32,
    ax: [f32; 4],
    aw: [f32; 4],
    ay: [f32; 4],
    nsnap: i32,
    snap: [SnapDisplay; 8],
    nlinks: i32,
    links: [Link; LINKS],
    area: SnapRect,
    detach_w: f32,
    detach_h: f32,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct EndLink {
    node: i32,
    dx: f32,
    dy: f32,
    dw: f32,
    dh: f32,
}
#[repr(C)]
#[derive(Default)]
pub struct End {
    node: i32,
    edges: i32,
    dx: f32,
    dy: f32,
    dw: f32,
    dh: f32,
    zone: i32,
    detached: i32,
    nlinks: i32,
    links: [EndLink; LINKS],
}
struct Saved {
    node: i32,
    left: Dim,
    top: Dim,
    width: Dim,
    height: Dim,
}
#[derive(Default)]
struct Member {
    node: i32,
    inset: f32,
    origin_x: f32,
    origin_y: f32,
}
struct Active {
    node: i32,
    edges: i32,
    pointer: [f32; 2],
    geometry: Grab,
    zones: Option<DragZones>,
    linked: Option<LinkedResize>,
    members: Vec<Member>,
    start: Vec<SnapRect>,
    out: Vec<SnapRect>,
}
#[derive(Default)]
struct Script {
    on: bool,
    start: f64,
    rx: f32,
    ry: f32,
    period: f32,
}
struct Controller {
    active: Option<Active>,
    saved: Vec<Saved>,
    pointer: [f32; 2],
    buttons: i32,
    shift: bool,
    last_edges: i32,
    zone: i32,
    zone_pending: bool,
    zone_rect: SnapRect,
    script: Script,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            active: None,
            saved: Vec::new(),
            pointer: [0.0; 2],
            buttons: 0,
            shift: false,
            last_edges: 0,
            zone: -1,
            zone_pending: false,
            zone_rect: SnapRect::default(),
            script: Script::default(),
        }
    }
}
#[cfg(not(test))]
mod backend {
    extern "C" {
        pub fn paint_layer_begin(node: i32, resize: i32);
        pub fn paint_layer_end();
        pub fn lbi_start(rx: f32, ry: f32, period: f32) -> i32;
        pub fn lbi_stop();
        pub fn lbi_fd() -> i32;
        pub fn lbi_take(dx: *mut f32, dy: *mut f32) -> i32;
        pub fn lbi_path(rx: f32, ry: f32, period: f32, time: f64, dx: *mut f32, dy: *mut f32);
    }
}
#[cfg(test)]
mod backend {
    pub unsafe fn paint_layer_begin(_: i32, _: i32) {}
    pub unsafe fn paint_layer_end() {}
    pub unsafe fn lbi_start(_: f32, _: f32, _: f32) -> i32 {
        -1
    }
    pub unsafe fn lbi_stop() {}
    pub unsafe fn lbi_fd() -> i32 {
        -1
    }
    pub unsafe fn lbi_take(_: *mut f32, _: *mut f32) -> i32 {
        0
    }
    pub unsafe fn lbi_path(rx: f32, ry: f32, period: f32, time: f64, dx: *mut f32, dy: *mut f32) {
        let angle = 2.0 * std::f64::consts::PI * time / period as f64;
        *dx = (rx as f64 * angle.sin()) as f32;
        *dy = (ry as f64 * (2.0 * angle).sin() / 2.0) as f32;
    }
}
fn dim_or(value: Dim, fallback: f32) -> f32 {
    if value.unit == 1 {
        value.v
    } else {
        fallback
    }
}
fn same(a: Dim, b: Dim) -> bool {
    a.unit == b.unit && a.v == b.v
}
fn px(value: f32) -> Dim {
    Dim { v: value, unit: 1 }
}
unsafe fn dirty() {
    ptr::addr_of_mut!(state::ui.dirty).write(1);
}
impl Controller {
    unsafe fn script_off(&mut self) {
        self.script.on = false;
        backend::lbi_stop();
    }
    unsafe fn save(&mut self, node: *mut Node, id: i32) {
        if node.is_null()
            || self.saved.len() > LINKS
            || self.saved.iter().any(|saved| saved.node == id)
        {
            return;
        }
        self.saved.push(Saved {
            node: id,
            left: (*node).s.pos[0],
            top: (*node).s.pos[1],
            width: (*node).s.width,
            height: (*node).s.height,
        });
    }
    unsafe fn restore(&mut self) {
        for saved in self.saved.drain(..) {
            let node = node_get(saved.node);
            if node.is_null() {
                continue;
            }
            let sized =
                !same((*node).s.width, saved.width) || !same((*node).s.height, saved.height);
            if !sized && same((*node).s.pos[0], saved.left) && same((*node).s.pos[1], saved.top) {
                continue;
            }
            (*node).s.pos[0] = saved.left;
            (*node).s.pos[1] = saved.top;
            node_moved(node);
            if sized {
                (*node).s.width = saved.width;
                (*node).s.height = saved.height;
                node_invalidate(node);
            }
            dirty();
        }
    }
    fn zone_check(&mut self, x: f32, y: f32) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let Some(zones) = active.zones.as_mut() else {
            return;
        };
        if !zones.update(x as f64, y as f64, self.shift) {
            return;
        }
        self.zone = match zones.current() {
            Some((zone, rect)) => {
                self.zone_rect = rect;
                zone.index()
            }
            None => -1,
        };
        self.zone_pending = true;
    }
    unsafe fn member(
        &mut self,
        node: *mut Node,
        id: i32,
        inset: f32,
        min_w: f32,
        min_h: f32,
        max_w: f32,
        max_h: f32,
    ) -> (Member, SnapMember) {
        let left = dim_or((*node).s.pos[0], (*node).x);
        let top = dim_or((*node).s.pos[1], (*node).y);
        let width = dim_or((*node).s.width, (*node).w);
        let height = dim_or((*node).s.height, (*node).h);
        self.save(node, id);
        let origin_x = (*node).ax - (*node).x;
        let origin_y = (*node).ay - (*node).y;
        (
            Member {
                node: id,
                inset,
                origin_x,
                origin_y,
            },
            SnapMember {
                frame: SnapRect::new(
                    (origin_x + left + inset) as f64,
                    (origin_y + top + inset) as f64,
                    (width - 2.0 * inset) as f64,
                    (height - 2.0 * inset) as f64,
                ),
                min_w: min_w as f64,
                min_h: min_h as f64,
                max_w: max_w as f64,
                max_h: max_h as f64,
            },
        )
    }
    unsafe fn setup(&mut self, id: i32, edges: i32, limits: Limits) -> i32 {
        let node = node_get(id);
        if node.is_null() || (*node).s.absolute == 0 {
            return -1;
        }
        if self.active.is_some() || !self.saved.is_empty() {
            self.cancel();
        }
        let mut start = GrabStart {
            node: SnapRect::new(
                dim_or((*node).s.pos[0], (*node).x) as f64,
                dim_or((*node).s.pos[1], (*node).y) as f64,
                dim_or((*node).s.width, (*node).w) as f64,
                dim_or((*node).s.height, (*node).h) as f64,
            ),
            origin_x: ((*node).ax - (*node).x) as f64,
            origin_y: ((*node).ay - (*node).y) as f64,
            pointer_x: self.pointer[0] as f64,
            pointer_y: self.pointer[1] as f64,
            edges: edges & 15,
            inset: limits.inset as f64,
            min_w: limits.min_w as f64,
            min_h: limits.min_h as f64,
            max_w: limits.max_w as f64,
            max_h: limits.max_h as f64,
            min_top: limits.min_top as f64,
            detach_w: limits.detach_w as f64,
            detach_h: limits.detach_h as f64,
            ..GrabStart::default()
        };
        for i in 0..limits.nareas.clamp(0, 4) as usize {
            start.areas[i] = GrabArea {
                x: limits.ax[i] as f64,
                w: limits.aw[i] as f64,
                top: limits.ay[i] as f64,
            };
            start.nareas += 1;
        }
        let geometry = Grab::new(start);
        self.last_edges = edges & 15;
        self.save(node, id);
        let zones = if edges & 15 == 0 && limits.nsnap > 0 {
            Some(DragZones::new(&limits.snap[..limits.nsnap.min(8) as usize]))
        } else {
            None
        };
        let mut active = Active {
            node: id,
            edges: edges & 15,
            pointer: self.pointer,
            geometry,
            zones,
            linked: None,
            members: Vec::new(),
            start: Vec::new(),
            out: Vec::new(),
        };
        if edges & 15 != 0 && limits.nlinks > 0 {
            let mut members = Vec::new();
            let (member, frame) = self.member(
                node,
                id,
                limits.inset,
                limits.min_w,
                limits.min_h,
                limits.max_w,
                limits.max_h,
            );
            active.members.push(member);
            members.push(frame);
            for link in &limits.links[..limits.nlinks.min(LINKS as i32) as usize] {
                let other = if link.node == id {
                    ptr::null_mut()
                } else {
                    node_get(link.node)
                };
                if other.is_null() || (*other).s.absolute == 0 {
                    continue;
                }
                let (member, frame) = self.member(
                    other, link.node, link.inset, link.min_w, link.min_h, link.max_w, link.max_h,
                );
                active.members.push(member);
                members.push(frame);
            }
            active.linked = LinkedResize::new(active.edges, &members, limits.area);
            if active.linked.is_some() {
                active.start = members.iter().map(|member| member.frame).collect();
                active.out = active.start.clone();
            }
        }
        let layered = active.linked.is_none();
        let waiting = active.geometry.waiting();
        self.active = Some(active);
        if edges & 15 == 0 && limits.nsnap > 0 && !waiting {
            self.zone_check(self.pointer[0], self.pointer[1]);
        }
        if layered {
            backend::paint_layer_begin(id, (edges & 15 != 0) as i32);
        }
        0
    }
    unsafe fn set_frame(member: &Member, frame: SnapRect) {
        let node = node_get(member.node);
        if node.is_null() {
            return;
        }
        let inset = member.inset;
        let x = (frame.x as f32 - inset - member.origin_x).round();
        let y = (frame.y as f32 - inset - member.origin_y).round();
        let width = (frame.w as f32 + 2.0 * inset).round();
        let height = (frame.h as f32 + 2.0 * inset).round();
        let cx = dim_or((*node).s.pos[0], (*node).x);
        let cy = dim_or((*node).s.pos[1], (*node).y);
        let cw = dim_or((*node).s.width, (*node).w);
        let ch = dim_or((*node).s.height, (*node).h);
        if x == cx && y == cy && width == cw && height == ch {
            return;
        }
        (*node).s.pos[0] = px(x);
        (*node).s.pos[1] = px(y);
        node_moved(node);
        if width != cw || height != ch {
            (*node).s.width = px(width);
            (*node).s.height = px(height);
            node_invalidate(node);
        }
        dirty();
    }
    unsafe fn motion(&mut self, x: f32, y: f32) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let node = node_get(active.node);
        if node.is_null() {
            self.cancel();
            return;
        }
        if let Some(linked) = &active.linked {
            active.out = linked.apply(
                (x - active.pointer[0]).round() as f64,
                (y - active.pointer[1]).round() as f64,
            );
            for (member, frame) in active.members.iter().zip(&active.out) {
                Self::set_frame(member, *frame);
            }
            return;
        }
        let moved = active.geometry.motion(
            x as f64,
            y as f64,
            ptr::addr_of!(state::ui.width).read() as f64,
            ptr::addr_of!(state::ui.height).read() as f64,
        );
        if active.geometry.waiting() {
            return;
        }
        let frame = active.geometry.now();
        let node_id = active.node;
        let moving = active.edges == 0;
        if moving {
            self.zone_check(x, y);
        }
        if moved & DETACHED != 0 {
            backend::paint_layer_end();
            backend::paint_layer_begin(node_id, 0);
        }
        if moved & MOVED == 0 {
            return;
        }
        (*node).s.pos[0] = px(frame.x as f32);
        (*node).s.pos[1] = px(frame.y as f32);
        node_moved(node);
        if moved & SIZED != 0 {
            (*node).s.width = px(frame.w as f32);
            (*node).s.height = px(frame.h as f32);
            node_invalidate(node);
        }
        dirty();
    }
    unsafe fn end(&mut self) {
        self.active = None;
        if self.zone >= 0 {
            self.zone = -1;
            self.zone_pending = true;
        }
        backend::paint_layer_end();
    }
    unsafe fn cancel(&mut self) {
        self.script_off();
        if self.active.is_some() {
            self.end();
        }
        self.restore();
    }
    unsafe fn finish(&mut self) -> Option<End> {
        if self.active.is_none() || self.buttons != 0 {
            return None;
        }
        if self.script.on {
            self.cancel();
            return None;
        }
        let active = self.active.as_ref().unwrap();
        let mut end = End {
            node: active.node,
            edges: active.edges,
            zone: -1,
            ..End::default()
        };
        let delta = if active.linked.is_some() {
            let a = active.start[0];
            let b = active.out[0];
            SnapRect::new(b.x - a.x, b.y - a.y, b.w - a.w, b.h - a.h)
        } else {
            end.detached = active.geometry.detached() as i32;
            active.geometry.delta()
        };
        end.dx = delta.x as f32;
        end.dy = delta.y as f32;
        end.dw = delta.w as f32;
        end.dh = delta.h as f32;
        let zones = active.zones.is_some() && !active.geometry.waiting();
        if zones {
            self.zone_check(self.pointer[0], self.pointer[1]);
            end.zone = self
                .active
                .as_ref()
                .unwrap()
                .zones
                .as_ref()
                .unwrap()
                .current()
                .map_or(-1, |(zone, _)| zone.index());
        }
        let active = self.active.as_ref().unwrap();
        if let Some(linked) = &active.linked {
            for i in 1..active.members.len() {
                if !linked.moves(i) {
                    continue;
                }
                let a = active.start[i];
                let b = active.out[i];
                end.links[end.nlinks as usize] = EndLink {
                    node: active.members[i].node,
                    dx: (b.x - a.x) as f32,
                    dy: (b.y - a.y) as f32,
                    dw: (b.w - a.w) as f32,
                    dh: (b.h - a.h) as f32,
                };
                end.nlinks += 1;
            }
        }
        self.end();
        Some(end)
    }
    unsafe fn script_tick(&mut self, now: f64) {
        if !self.script.on {
            return;
        }
        let Some(active) = self.active.as_ref() else {
            self.script_off();
            return;
        };
        let pointer = active.pointer;
        let (mut dx, mut dy) = (0.0, 0.0);
        if backend::lbi_fd() >= 0 {
            if backend::lbi_take(&mut dx, &mut dy) != 0 {
                self.motion(pointer[0] + dx, pointer[1] + dy);
            }
            return;
        }
        if self.script.start < 0.0 {
            self.script.start = now;
        }
        backend::lbi_path(
            self.script.rx,
            self.script.ry,
            self.script.period,
            now - self.script.start,
            &mut dx,
            &mut dy,
        );
        self.motion(pointer[0] + dx, pointer[1] + dy);
    }
}
thread_local! {static CONTROLLER:RefCell<Controller>=RefCell::new(Controller::default());}
#[no_mangle]
pub extern "C" fn grab_note_pointer(x: f32, y: f32, buttons: i32) {
    CONTROLLER.with(|state| {
        let mut state = state.borrow_mut();
        state.pointer = [x, y];
        state.buttons = buttons;
    });
}
#[no_mangle]
pub extern "C" fn grab_active() -> i32 {
    CONTROLLER.with(|state| state.borrow().active.is_some() as i32)
}
#[no_mangle]
pub extern "C" fn grab_cursor() -> i32 {
    CONTROLLER.with(|state| {
        let edges = state.borrow().last_edges;
        if edges != 0 {
            cursor::cursor_for_edges(edges)
        } else {
            cursor::MOVE
        }
    })
}
/// # Safety
/// UI thread only, with three writable, nonoverlapping output pointers.
#[no_mangle]
pub unsafe extern "C" fn grab_state(x: *mut f32, y: *mut f32, edges: *mut i32) -> i32 {
    CONTROLLER.with(|state| {
        let state = state.borrow();
        *x = state.pointer[0];
        *y = state.pointer[1];
        *edges = state.active.as_ref().map_or(0, |a| a.edges);
        state.active.is_some() as i32
    })
}
/// # Safety
/// UI-thread, nonreentrant call; limits points to a readable GrabLimits.
#[no_mangle]
pub unsafe extern "C" fn grab_begin(node: i32, edges: i32, limits: *const Limits) -> i32 {
    CONTROLLER.with(|state| {
        let mut state = state.borrow_mut();
        if state.buttons == 0 {
            return -1;
        }
        state.script_off();
        state.setup(node, edges, limits.read())
    })
}
#[no_mangle]
pub extern "C" fn grab_note_shift(held: i32) {
    CONTROLLER.with(|state| {
        let mut state = state.borrow_mut();
        let held = held != 0;
        if held == state.shift {
            return;
        }
        state.shift = held;
        if state.active.is_some() {
            let [x, y] = state.pointer;
            state.zone_check(x, y);
        }
    });
}
/// # Safety
/// On success, outputs point to writable zone and SnapRect storage; UI thread only.
#[no_mangle]
pub unsafe extern "C" fn grab_zone_take(zone: *mut i32, rect: *mut SnapRect) -> i32 {
    CONTROLLER.with(|state| {
        let mut state = state.borrow_mut();
        if !state.zone_pending {
            return 0;
        }
        state.zone_pending = false;
        *zone = state.zone;
        *rect = state.zone_rect;
        1
    })
}
/// # Safety
/// UI-thread, nonreentrant call with no active scene borrow.
#[no_mangle]
pub unsafe extern "C" fn grab_motion(x: f32, y: f32) {
    CONTROLLER.with(|state| state.borrow_mut().motion(x, y));
}
/// # Safety
/// UI thread, nonreentrant. On success, out points to writable GrabEnd storage.
#[no_mangle]
pub unsafe extern "C" fn grab_finish(out: *mut End) -> i32 {
    CONTROLLER.with(|state| {
        if let Some(end) = state.borrow_mut().finish() {
            out.write(end);
            1
        } else {
            0
        }
    })
}
/// # Safety
/// UI-thread, nonreentrant call with no active scene borrow.
#[no_mangle]
pub unsafe extern "C" fn grab_restore() {
    CONTROLLER.with(|state| state.borrow_mut().restore());
}
/// # Safety
/// UI-thread, nonreentrant call with no active scene borrow.
#[no_mangle]
pub unsafe extern "C" fn grab_cancel() {
    CONTROLLER.with(|state| state.borrow_mut().cancel());
}
/// # Safety
/// Same UI-thread and readable-limits requirements as grab_begin.
#[no_mangle]
pub unsafe extern "C" fn grab_script_begin(
    node: i32,
    limits: *const Limits,
    rx: f32,
    ry: f32,
    period: f32,
) -> i32 {
    CONTROLLER.with(|state| {
        let mut state = state.borrow_mut();
        if rx < 0.0 || ry < 0.0 || period < 500.0 {
            return -1;
        }
        if state.setup(node, 0, limits.read()) != 0 {
            return -1;
        }
        state.script = Script {
            on: true,
            start: -1.0,
            rx,
            ry,
            period,
        };
        backend::lbi_start(rx, ry, period);
        dirty();
        0
    })
}
#[no_mangle]
pub extern "C" fn grab_scripted() -> i32 {
    CONTROLLER.with(|state| {
        let state = state.borrow();
        (state.script.on && state.active.is_some()) as i32
    })
}
/// # Safety
/// UI-thread, nonreentrant call with no active scene borrow.
#[no_mangle]
pub unsafe extern "C" fn grab_script_tick(now: f64) {
    CONTROLLER.with(|state| state.borrow_mut().script_tick(now));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zone_release_keeps_last_rectangle_for_the_pending_hide_notification() {
        let start = GrabStart {
            node: SnapRect::new(100.0, 100.0, 320.0, 240.0),
            ..GrabStart::default()
        };
        let displays = [SnapDisplay {
            bounds: SnapRect::new(0.0, 0.0, 1280.0, 800.0),
            work: SnapRect::new(0.0, 28.0, 1280.0, 752.0),
        }];
        let mut controller = Controller::default();
        controller.active = Some(Active {
            node: 1,
            edges: 0,
            pointer: [150.0, 120.0],
            geometry: Grab::new(start),
            zones: Some(DragZones::new(&displays)),
            linked: None,
            members: Vec::new(),
            start: Vec::new(),
            out: Vec::new(),
        });
        controller.zone_check(0.0, 400.0);
        assert_eq!(controller.zone, 0);
        assert_eq!(controller.zone_rect.w, 640.0);
        controller.shift = true;
        controller.zone_check(0.0, 400.0);
        assert_eq!(controller.zone, 9);
        assert_eq!(controller.zone_rect.w, 427.0);
        let previous = controller.zone_rect;
        controller.zone_pending = false;
        unsafe {
            controller.end();
        }
        assert!(controller.active.is_none());
        assert_eq!(controller.zone, -1);
        assert!(controller.zone_pending);
        assert_eq!(controller.zone_rect, previous);
        // Style restoration uses C numeric equality, including signed zero.
        assert!(same(Dim { v: -0.0, unit: 1 }, Dim { v: 0.0, unit: 1 }));
        assert!(!same(
            Dim {
                v: f32::NAN,
                unit: 1
            },
            Dim {
                v: f32::NAN,
                unit: 1
            }
        ));
    }
    // Native ABI/scene integration uses a child because the exported UI state is
    // confined to one UI thread. The pure test above also runs under Miri.
    #[cfg(not(miri))]
    #[test]
    fn process_contract() {
        if std::env::var_os("AXIA_WINDOW_GRAB_TEST_CHILD").is_none() {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "runtime::window_grab::tests::process_contract",
                    "--nocapture",
                ])
                .env("AXIA_WINDOW_GRAB_TEST_CHILD", "1")
                .output()
                .unwrap();
            assert!(
                child.status.success(),
                "{} {}",
                String::from_utf8_lossy(&child.stdout),
                String::from_utf8_lossy(&child.stderr)
            );
            return;
        }
        unsafe {
            ptr::addr_of_mut!(state::ui.width).write(1280.0);
            ptr::addr_of_mut!(state::ui.height).write(800.0);
            let node = crate::runtime::node_new(0);
            assert!(!node.is_null());
            let id = (*node).id;
            (*node).s.absolute = 1;
            (*node).x = 100.0;
            (*node).ax = 100.0;
            (*node).y = 100.0;
            (*node).ay = 100.0;
            (*node).w = 320.0;
            (*node).h = 240.0;
            (*node).s.pos[0] = Dim { v: 50.0, unit: 2 };
            (*node).s.pos[1] = px(100.0);
            (*node).s.width = Dim { v: 30.0, unit: 2 };
            (*node).s.height = Dim::default();
            let limits = Limits {
                inset: 6.0,
                min_w: 160.0,
                min_h: 100.0,
                min_top: 28.0,
                ..Limits::default()
            };
            grab_note_pointer(120.0, 130.0, 1);
            assert_eq!(grab_begin(id, 0, &limits), 0);
            grab_note_pointer(160.0, 180.0, 1);
            grab_motion(160.0, 180.0);
            assert_eq!((*node).s.pos[0].v, 140.0);
            assert_eq!((*node).s.pos[1].v, 150.0);
            grab_note_pointer(160.0, 180.0, 0);
            let mut end = End::default();
            assert_eq!(grab_finish(&mut end), 1);
            assert_eq!((end.dx, end.dy, end.dw, end.dh), (40.0, 50.0, 0.0, 0.0));
            grab_restore();
            assert_eq!((*node).s.pos[0].unit, 2);
            assert_eq!((*node).s.width.unit, 2);
            assert_eq!((*node).s.height.unit, 0);
            grab_note_pointer(120.0, 130.0, 1);
            assert_eq!(grab_begin(id, 0, &limits), 0);
            crate::runtime::node_destroy(node);
            grab_motion(130.0, 140.0);
            assert_eq!(grab_active(), 0);
        }
    }
}
