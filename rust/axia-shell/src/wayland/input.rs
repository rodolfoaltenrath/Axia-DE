//! Wayland gestures, scrolling and direct touches. All entry points run on the
//! display's dispatch thread. Native resource handles are borrowed until their
//! destruction callbacks remove them; Rust owns all protocol/session state.
use super::{ffi::*, gestures::*};
use libc::c_void;
use std::{
    cell::{RefCell, UnsafeCell},
    pin::Pin,
    ptr,
};

const BEGIN: i32 = 1;
const UPDATE: i32 = 2;
const END: i32 = 3;
const CANCEL: i32 = 4;
const MAX_TOUCH: usize = 10;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Swipe,
    Pinch,
    Hold,
    Touch,
}
struct Entry {
    resource: *mut Resource,
    client: *mut Client,
    kind: Kind,
}
#[derive(Default, Clone, Copy)]
struct Touch {
    on: bool,
    client: *mut Client,
    ox: f64,
    oy: f64,
}
struct Seat {
    listener: Listener,
    resource: *mut Resource,
}
#[derive(Default)]
struct State {
    ready: bool,
    entries: Vec<Entry>,
    seats: Vec<Pin<Box<Seat>>>,
    gesture_clients: [*mut Client; 3],
    rest: [i32; 2],
    touches: [Touch; MAX_TOUCH],
    frames: [*mut Client; MAX_TOUCH],
    nframe: usize,
}
impl State {
    fn mark_frame(&mut self, client: *mut Client) {
        if self.nframe < MAX_TOUCH && !self.frames[..self.nframe].contains(&client) {
            self.frames[self.nframe] = client;
            self.nframe += 1;
        }
    }
    fn wheel(&mut self, vertical: i32, horizontal: i32) -> ([i32; 2], [i32; 2]) {
        let values = [vertical.wrapping_neg(), horizontal];
        let mut discrete = [0; 2];
        for a in 0..2 {
            self.rest[a] = self.rest[a].wrapping_add(values[a]);
            discrete[a] = self.rest[a] / 120;
            self.rest[a] -= discrete[a] * 120;
        }
        (values, discrete)
    }
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

extern "C" {
    fn wl__display() -> *mut Display;
    fn wl__pointer_focus() -> *mut Resource;
    fn wl__pointers() -> *mut List;
    fn wl__surface_at(x: f32, y: f32, lx: *mut f32, ly: *mut f32) -> *mut Resource;
}

fn now_ms() -> u32 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time);
    }
    (time.tv_sec as u64)
        .wrapping_mul(1000)
        .wrapping_add(time.tv_nsec as u64 / 1_000_000) as u32
}
// Public Wayland headers use round-to-nearest, ties away from zero for 24.8 values.
fn fixed(v: f64) -> i32 {
    (v * 256.).round() as i32
}
unsafe fn serial() -> u32 {
    wl_display_next_serial(wl__display())
}
unsafe fn focus() -> *mut Resource {
    if STATE.with(|s| s.borrow().ready) {
        wl__pointer_focus()
    } else {
        ptr::null_mut()
    }
}
fn resources(kind: Kind, client: *mut Client) -> Vec<*mut Resource> {
    STATE.with(|s| {
        s.borrow()
            .entries
            .iter()
            .filter(|e| e.kind == kind && e.client == client)
            .map(|e| e.resource)
            .collect()
    })
}
unsafe fn pointers(client: *mut Client) -> Vec<*mut Resource> {
    let head = wl__pointers();
    let mut link = (*head).next;
    let mut result = Vec::new();
    while link != head {
        let resource = wl_resource_from_link(link);
        if wl_resource_get_client(resource) == client {
            result.push(resource);
        }
        link = (*link).next;
    }
    result
}
unsafe fn pointer_frames(client: *mut Client) {
    for p in pointers(client) {
        if wl_resource_get_version(p) >= 5 {
            wl_resource_post_event(p, 5);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_has_focus() -> i32 {
    (!focus().is_null()).into()
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_axis(source: i32, dx: f64, dy: f64, stop: i32, natural: i32) {
    let f = focus();
    if f.is_null() {
        return;
    }
    let time = now_ms();
    let source = match source {
        0 => 1u32,
        1 => 0,
        _ => 2,
    };
    for p in pointers(wl_resource_get_client(f)) {
        let version = wl_resource_get_version(p);
        if version >= 5 {
            wl_resource_post_event(p, 6, source);
        }
        for (axis, value) in [dy, dx].into_iter().enumerate() {
            let axis = axis as u32;
            if stop != 0 {
                if version >= 5 {
                    wl_resource_post_event(p, 7, time, axis);
                }
                continue;
            }
            if value == 0. {
                continue;
            }
            if version >= 9 {
                wl_resource_post_event(p, 10, axis, u32::from(natural != 0));
            }
            wl_resource_post_event(p, 4, time, axis, fixed(value));
        }
        if version >= 5 {
            wl_resource_post_event(p, 5);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_wheel(p: *mut Resource, time: u32, steps: i32) {
    if steps == 0 {
        return;
    }
    let version = wl_resource_get_version(p);
    if version >= 5 {
        wl_resource_post_event(p, 6, 0u32);
    }
    if version >= 8 {
        wl_resource_post_event(p, 9, 0u32, steps.wrapping_mul(-120));
    } else if version >= 5 {
        wl_resource_post_event(p, 8, 0u32, steps.wrapping_neg());
    }
    wl_resource_post_event(p, 4, time, 0u32, steps.wrapping_mul(-2560));
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_wheel120(vertical: i32, horizontal: i32) {
    let f = focus();
    if f.is_null() {
        return;
    }
    let time = now_ms();
    let (values, discrete) = STATE.with(|s| s.borrow_mut().wheel(vertical, horizontal));
    for p in pointers(wl_resource_get_client(f)) {
        let version = wl_resource_get_version(p);
        if version >= 5 {
            wl_resource_post_event(p, 6, 0u32);
        }
        for a in 0..2 {
            if values[a] == 0 {
                continue;
            }
            if version >= 8 {
                wl_resource_post_event(p, 9, a as u32, values[a]);
            } else if version >= 5 && discrete[a] != 0 {
                wl_resource_post_event(p, 8, a as u32, discrete[a]);
            }
            wl_resource_post_event(p, 4, time, a as u32, fixed(values[a] as f64 / 12.));
        }
        if version >= 5 {
            wl_resource_post_event(p, 5);
        }
    }
}

unsafe fn gesture(
    kind: Kind,
    phase: i32,
    fingers: i32,
    dx: f64,
    dy: f64,
    scale: f64,
    rotation: f64,
) {
    let index = match kind {
        Kind::Swipe => 0,
        Kind::Pinch => 1,
        Kind::Hold => 2,
        Kind::Touch => unreachable!(),
    };
    if phase == BEGIN {
        let f = focus();
        let client = if f.is_null() {
            ptr::null_mut()
        } else {
            wl_resource_get_client(f)
        };
        STATE.with(|s| s.borrow_mut().gesture_clients[index] = client);
    }
    let client = STATE.with(|s| s.borrow().gesture_clients[index]);
    if client.is_null() {
        return;
    }
    let f = wl__pointer_focus();
    let time = now_ms();
    for r in resources(kind, client) {
        if phase == BEGIN {
            if !f.is_null() {
                wl_resource_post_event(r, 0, serial(), time, f, fingers as u32);
            }
        } else if phase == UPDATE && kind != Kind::Hold {
            if kind == Kind::Swipe {
                wl_resource_post_event(r, 1, time, fixed(dx), fixed(dy));
            } else {
                wl_resource_post_event(
                    r,
                    1,
                    time,
                    fixed(dx),
                    fixed(dy),
                    fixed(scale),
                    fixed(rotation),
                );
            }
        } else {
            wl_resource_post_event(
                r,
                if kind == Kind::Hold { 1 } else { 2 },
                serial(),
                time,
                i32::from(phase == CANCEL),
            );
        }
    }
    pointer_frames(client);
    if phase == END || phase == CANCEL || (kind == Kind::Hold && phase != BEGIN) {
        STATE.with(|s| s.borrow_mut().gesture_clients[index] = ptr::null_mut());
    }
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_swipe(phase: i32, fingers: i32, dx: f64, dy: f64) {
    gesture(Kind::Swipe, phase, fingers, dx, dy, 1., 0.);
}
#[no_mangle]
pub unsafe extern "C" fn wl_input_pinch(
    phase: i32,
    fingers: i32,
    dx: f64,
    dy: f64,
    scale: f64,
    rotation: f64,
) {
    gesture(Kind::Pinch, phase, fingers, dx, dy, scale, rotation);
}
#[no_mangle]
pub unsafe extern "C" fn wl_input_hold(phase: i32, fingers: i32) {
    gesture(Kind::Hold, phase, fingers, 0., 0., 1., 0.);
}

unsafe extern "C" fn destroy(_: *mut Client, resource: *mut Resource) {
    wl_resource_destroy(resource);
}
unsafe extern "C" fn gone(resource: *mut Resource) {
    STATE.with(|s| s.borrow_mut().entries.retain(|e| e.resource != resource));
}
#[repr(C)]
struct DestroyImpl {
    destroy: unsafe extern "C" fn(*mut Client, *mut Resource),
}
static DESTROY_IMPL: DestroyImpl = DestroyImpl { destroy };

unsafe fn make(
    client: *mut Client,
    manager: *mut Resource,
    id: u32,
    kind: Kind,
    interface: *const Interface,
) {
    let r = wl_resource_create(client, interface, wl_resource_get_version(manager), id);
    if r.is_null() {
        wl_client_post_no_memory(client);
        return;
    }
    if STATE
        .with(|s| s.borrow_mut().entries.try_reserve(1))
        .is_err()
    {
        wl_resource_destroy(r);
        wl_client_post_no_memory(client);
        return;
    }
    wl_resource_set_implementation(
        r,
        (&DESTROY_IMPL as *const DestroyImpl).cast(),
        ptr::null_mut(),
        Some(gone),
    );
    STATE.with(|s| {
        s.borrow_mut().entries.insert(
            0,
            Entry {
                resource: r,
                client,
                kind,
            },
        )
    });
}
unsafe extern "C" fn swipe(client: *mut Client, manager: *mut Resource, id: u32, _: *mut Resource) {
    make(
        client,
        manager,
        id,
        Kind::Swipe,
        &zwp_pointer_gesture_swipe_v1_interface,
    );
}
unsafe extern "C" fn pinch(client: *mut Client, manager: *mut Resource, id: u32, _: *mut Resource) {
    make(
        client,
        manager,
        id,
        Kind::Pinch,
        &zwp_pointer_gesture_pinch_v1_interface,
    );
}
unsafe extern "C" fn hold(client: *mut Client, manager: *mut Resource, id: u32, _: *mut Resource) {
    make(
        client,
        manager,
        id,
        Kind::Hold,
        &zwp_pointer_gesture_hold_v1_interface,
    );
}
#[repr(C)]
struct ManagerImpl {
    swipe: unsafe extern "C" fn(*mut Client, *mut Resource, u32, *mut Resource),
    pinch: unsafe extern "C" fn(*mut Client, *mut Resource, u32, *mut Resource),
    release: unsafe extern "C" fn(*mut Client, *mut Resource),
    hold: unsafe extern "C" fn(*mut Client, *mut Resource, u32, *mut Resource),
}
static MANAGER_IMPL: ManagerImpl = ManagerImpl {
    swipe,
    pinch,
    release: destroy,
    hold,
};
unsafe extern "C" fn bind(client: *mut Client, _: *mut c_void, version: u32, id: u32) {
    let r = wl_resource_create(
        client,
        &zwp_pointer_gestures_v1_interface,
        version as i32,
        id,
    );
    if r.is_null() {
        wl_client_post_no_memory(client);
        return;
    }
    wl_resource_set_implementation(
        r,
        (&MANAGER_IMPL as *const ManagerImpl).cast(),
        ptr::null_mut(),
        None,
    );
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_get_touch(client: *mut Client, seat: *mut Resource, id: u32) {
    make(client, seat, id, Kind::Touch, &raw const wl_touch_interface);
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_touch(phase: i32, id: i32, x: f64, y: f64) -> i32 {
    if !STATE.with(|s| s.borrow().ready) || !(0..MAX_TOUCH as i32).contains(&id) {
        return 0;
    }
    let index = id as usize;
    let time = now_ms();
    if phase == BEGIN {
        let (mut lx, mut ly) = (0f32, 0f32);
        let surface = wl__surface_at(x as f32, y as f32, &mut lx, &mut ly);
        if surface.is_null() {
            return 0;
        }
        let client = wl_resource_get_client(surface);
        let recipients = resources(Kind::Touch, client);
        if recipients.is_empty() {
            return 0;
        }
        STATE.with(|s| {
            s.borrow_mut().touches[index] = Touch {
                on: true,
                client,
                ox: x - lx as f64,
                oy: y - ly as f64,
            }
        });
        for r in recipients {
            wl_resource_post_event(
                r,
                0,
                serial(),
                time,
                surface,
                id,
                fixed(lx as f64),
                fixed(ly as f64),
            );
        }
        STATE.with(|s| s.borrow_mut().mark_frame(client));
        return 1;
    }
    let touch = STATE.with(|s| s.borrow().touches[index]);
    if !touch.on {
        return 0;
    }
    for r in resources(Kind::Touch, touch.client) {
        if phase == UPDATE {
            wl_resource_post_event(r, 2, time, id, fixed(x - touch.ox), fixed(y - touch.oy));
        } else if phase == CANCEL {
            wl_resource_post_event(r, 4);
        } else {
            wl_resource_post_event(r, 1, serial(), time, id);
        }
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if phase != UPDATE {
            s.touches[index].on = false;
        }
        s.mark_frame(touch.client);
    });
    1
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_touch_frame() {
    let clients = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let clients = s.frames[..s.nframe].to_vec();
        s.nframe = 0;
        clients
    });
    for client in clients {
        for r in resources(Kind::Touch, client) {
            wl_resource_post_event(r, 3);
        }
    }
}

unsafe extern "C" fn seat_gone(listener: *mut Listener, _: *mut c_void) {
    let seat = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.seats
            .iter()
            .position(|seat| ptr::eq(&seat.listener, listener))
            .map(|p| s.seats.remove(p))
    });
    if let Some(seat) = seat {
        wl_list_remove(seat.listener.link.get());
    }
}
fn caps() -> u32 {
    3 | if crate::touch::tp_count(2) != 0 { 4 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_seat_caps(resource: *mut Resource) {
    wl_resource_post_event(resource, 0, caps());
    if STATE.with(|s| s.borrow_mut().seats.try_reserve(1)).is_err() {
        return;
    }
    let seat = Box::pin(Seat {
        listener: Listener {
            link: UnsafeCell::new(List {
                prev: ptr::null_mut(),
                next: ptr::null_mut(),
            }),
            notify: Some(seat_gone),
        },
        resource,
    });
    let listener = (&seat.listener as *const Listener).cast_mut();
    STATE.with(|s| s.borrow_mut().seats.insert(0, seat));
    wl_resource_add_destroy_listener(resource, listener);
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_devices_changed() {
    let seats = STATE.with(|s| {
        let s = s.borrow();
        if s.ready {
            s.seats.iter().map(|s| s.resource).collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    });
    for seat in seats {
        wl_resource_post_event(seat, 0, caps());
    }
}

#[no_mangle]
pub unsafe extern "C" fn wl_input_globals(display: *mut Display) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        debug_assert!(s.entries.is_empty() && s.seats.is_empty());
        *s = State {
            ready: true,
            ..State::default()
        };
    });
    wl_global_create(
        display,
        &zwp_pointer_gestures_v1_interface,
        3,
        ptr::null_mut(),
        Some(bind),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wheel_remainders_and_fixed_rounding() {
        let mut s = State::default();
        assert_eq!(s.wheel(60, 70), ([-60, 70], [0, 0]));
        assert_eq!(s.wheel(60, 50), ([-60, 50], [-1, 1]));
        assert_eq!(s.rest, [0; 2]);
        assert_eq!(s.wheel(-239, -239), ([239, -239], [1, -1]));
        assert_eq!(s.rest, [119, -119]);
        assert_eq!(s.wheel(119, 119), ([-119, 119], [0, 0]));
        for n in -1024..=1024 {
            assert_eq!(fixed(n as f64 / 256.), n);
        }
        assert_eq!(fixed(0.5 / 256.), 1);
        assert_eq!(fixed(-0.5 / 256.), -1);
    }
    #[test]
    fn touch_frames_deduplicate_and_bound_clients() {
        let mut s = State::default();
        // Addresses are identity tokens only, never dereferenced.
        for n in 1..20 {
            let c = ptr::without_provenance_mut(n);
            s.mark_frame(c);
            s.mark_frame(c);
        }
        assert_eq!(s.nframe, MAX_TOUCH);
        assert_eq!(s.frames[9], ptr::without_provenance_mut(10));
    }
}
