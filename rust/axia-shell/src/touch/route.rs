//! Gesture target latching and routing between Wayland and the shell.
//! Locks protect only state updates; no foreign callback executes under a lock.
use super::*;
use crate::wayland::input::{
    wl_input_axis, wl_input_has_focus, wl_input_hold, wl_input_pinch, wl_input_swipe,
    wl_input_touch, wl_input_touch_frame, wl_input_wheel120,
};
use std::ffi::CStr;
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Host {
    move_: Option<unsafe extern "C" fn(f64, f64, f64, f64)>,
    warp: Option<unsafe extern "C" fn(f64, f64)>,
    button: Option<unsafe extern "C" fn(c_int, c_int)>,
    position: Option<unsafe extern "C" fn(*mut f64, *mut f64, *mut c_int, *mut c_int)>,
    flush: Option<unsafe extern "C" fn()>,
}
#[derive(Default, Clone, Copy)]
struct Latch {
    active: bool,
    wayland: bool,
    node: i32,
}
#[derive(Default)]
struct Routing {
    host: Host,
    scroll: Latch,
    pinch: Latch,
    swipe: Latch,
    last_rotation: f64,
    system_swipe: bool,
    wheel_vertical: i32,
    wheel_horizontal: i32,
}
static ROUTING: OnceLock<Mutex<Routing>> = OnceLock::new();
fn routing() -> MutexGuard<'static, Routing> {
    ROUTING
        .get_or_init(|| Mutex::new(Routing::default()))
        .lock()
        .unwrap()
}
extern "C" {
    fn js_gesture(
        kind: *const c_char,
        phase: *const c_char,
        node: c_int,
        x: f32,
        y: f32,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
    );
}
fn phase_name(phase: i32) -> &'static CStr {
    match phase {
        BEGIN => c"begin",
        END => c"end",
        CANCEL => c"cancel",
        MOMENTUM => c"momentum",
        MOMENTUM_END => c"momentum-end",
        _ => c"update",
    }
}
fn position() -> (f64, f64, i32, i32) {
    let (mut x, mut y) = (0.0, 0.0);
    let (mut width, mut height) = unsafe { crate::runtime::input_dimensions() };
    let host = routing().host;
    if let Some(f) = host.position {
        unsafe { f(&mut x, &mut y, &mut width, &mut height) }
    }
    (x, y, width, height)
}
fn node_at(x: f64, y: f64) -> i32 {
    unsafe { crate::runtime::input_hit_id(x as f32, y as f32) }
}
fn latch(x: f64, y: f64) -> Latch {
    let wayland = unsafe { wl_input_has_focus() != 0 };
    Latch {
        active: true,
        wayland,
        node: if wayland { 0 } else { node_at(x, y) },
    }
}
fn gesture(kind: &CStr, phase: &CStr, node: i32, x: f64, y: f64, a: f64, b: f64, c: f64, d: f64) {
    unsafe {
        js_gesture(
            kind.as_ptr(),
            phase.as_ptr(),
            node,
            x as f32,
            y as f32,
            a,
            b,
            c,
            d,
        )
    }
}
/// # Safety
/// UI/input-thread only. Foreign callbacks must obey the compositor lifetime rules.
#[no_mangle]
pub unsafe extern "C" fn tp_route_scroll(phase: i32, dx: f64, dy: f64, source: i32) {
    let (x, y, _, _) = position();
    if phase == MOMENTUM || phase == MOMENTUM_END {
        let scroll = routing().scroll;
        if scroll.wayland {
            return;
        }
        gesture(
            c"scroll",
            phase_name(phase),
            scroll.node,
            x,
            y,
            dx,
            dy,
            source as f64,
            0.0,
        );
        return;
    }
    if phase == BEGIN || !routing().scroll.active {
        let target = latch(x, y);
        routing().scroll = target;
    }
    let scroll = routing().scroll;
    if scroll.wayland {
        wl_input_axis(
            if source == 0 {
                0
            } else if source == 1 {
                1
            } else {
                2
            },
            dx,
            dy,
            (phase == END || phase == CANCEL) as i32,
            config().natural,
        );
    } else {
        gesture(
            c"scroll",
            phase_name(phase),
            scroll.node,
            x,
            y,
            dx,
            dy,
            source as f64,
            0.0,
        );
    }
    if phase == END || phase == CANCEL {
        routing().scroll.active = false;
    }
}
/// # Safety
/// UI/input-thread only, with valid host callbacks.
#[no_mangle]
pub unsafe extern "C" fn tp_route_pinch(
    phase: i32,
    n: i32,
    dx: f64,
    dy: f64,
    scale: f64,
    rotation: f64,
) {
    let (x, y, _, _) = position();
    if phase == BEGIN || !routing().pinch.active {
        let target = latch(x, y);
        let mut state = routing();
        state.pinch = target;
        state.last_rotation = 0.0;
    }
    let (pinch, last) = {
        let state = routing();
        (state.pinch, state.last_rotation)
    };
    if pinch.wayland {
        wl_input_pinch(phase, n, dx, dy, scale, rotation - last);
        routing().last_rotation = rotation;
    } else {
        gesture(
            c"pinch",
            phase_name(phase),
            pinch.node,
            x,
            y,
            scale,
            rotation,
            dx,
            dy,
        );
    }
    if phase == END || phase == CANCEL {
        routing().pinch.active = false;
    }
}
fn system_swipe(n: i32) -> bool {
    let config = config();
    match n {
        3 => config.swipe3 != 0 && config.three_drag == 0,
        4 => config.swipe4 != 0 || (config.three_drag != 0 && config.swipe3 != 0),
        _ => false,
    }
}
unsafe fn swipe(phase: i32, n: i32, dx: f64, dy: f64) {
    let (x, y, _, _) = position();
    if phase == BEGIN || !routing().swipe.active {
        let target = latch(x, y);
        let system = system_swipe(n);
        let mut state = routing();
        state.swipe = target;
        state.system_swipe = system;
    }
    let (target, system) = {
        let state = routing();
        (state.swipe, state.system_swipe)
    };
    if system || !target.wayland {
        gesture(
            c"swipe",
            phase_name(phase),
            if system { node_at(x, y) } else { target.node },
            x,
            y,
            dx,
            dy,
            n as f64,
            system as i32 as f64,
        );
    } else {
        wl_input_swipe(phase, n, dx, dy);
    }
    if phase == END || phase == CANCEL {
        routing().swipe.active = false;
    }
}
#[no_mangle]
pub extern "C" fn tp_route_wheel120(vertical: i32, horizontal: i32) {
    let mut state = routing();
    state.wheel_vertical = state.wheel_vertical.wrapping_add(vertical);
    state.wheel_horizontal = state.wheel_horizontal.wrapping_add(horizontal);
}
/// # Safety
/// UI/input-thread only, with valid host callbacks.
#[no_mangle]
pub unsafe extern "C" fn tp_route_wheel_flush() {
    let (v, h) = {
        let mut state = routing();
        let values = (state.wheel_vertical, state.wheel_horizontal);
        if values == (0, 0) {
            return;
        }
        state.wheel_vertical = 0;
        state.wheel_horizontal = 0;
        values
    };
    let (x, y, _, _) = position();
    if wl_input_has_focus() != 0 {
        wl_input_wheel120(v, h);
        return;
    }
    routing().scroll.active = false;
    gesture(
        c"scroll",
        c"update",
        node_at(x, y),
        x,
        y,
        h as f64 * 48.0 / 120.0,
        -(v as f64) * 48.0 / 120.0,
        1.0,
        0.0,
    );
}
unsafe extern "C" fn motion(_: *mut c_void, dx: f64, dy: f64, rx: f64, ry: f64) {
    let callback = routing().host.move_;
    if let Some(f) = callback {
        f(dx, dy, rx, ry);
    }
}
unsafe extern "C" fn button(_: *mut c_void, b: i32, down: i32) {
    let callback = routing().host.button;
    if let Some(f) = callback {
        f(b, down);
    }
}
unsafe extern "C" fn scroll(_: *mut c_void, phase: i32, dx: f64, dy: f64) {
    tp_route_scroll(phase, dx, dy, 0);
}
unsafe extern "C" fn pinch(
    _: *mut c_void,
    phase: i32,
    n: i32,
    dx: f64,
    dy: f64,
    scale: f64,
    rotation: f64,
) {
    tp_route_pinch(phase, n, dx, dy, scale, rotation);
}
unsafe extern "C" fn swipe_sink(_: *mut c_void, phase: i32, n: i32, dx: f64, dy: f64) {
    swipe(phase, n, dx, dy);
}
unsafe extern "C" fn hold(_: *mut c_void, phase: i32, n: i32) {
    if phase != BEGIN || wl_input_has_focus() != 0 {
        wl_input_hold(phase, n);
    }
}
unsafe extern "C" fn touch(_: *mut c_void, phase: i32, id: i32, x: f64, y: f64) -> i32 {
    let (_, _, width, height) = position();
    wl_input_touch(phase, id, x * width as f64, y * height as f64)
}
unsafe extern "C" fn touch_frame(_: *mut c_void) {
    wl_input_touch_frame();
}
unsafe extern "C" fn warp(_: *mut c_void, x: f64, y: f64) {
    let (_, _, width, height) = position();
    let callback = routing().host.warp;
    if let Some(f) = callback {
        f(x * (width - 1) as f64, y * (height - 1) as f64);
    }
}
unsafe extern "C" fn frame(_: *mut c_void) {
    let callback = routing().host.flush;
    if let Some(f) = callback {
        f();
    }
}
static SINK: Sink = Sink {
    motion: Some(motion),
    button: Some(button),
    scroll: Some(scroll),
    pinch: Some(pinch),
    swipe: Some(swipe_sink),
    hold: Some(hold),
    touch: Some(touch),
    touch_frame: Some(touch_frame),
    warp: Some(warp),
    frame: Some(frame),
};
/// # Safety
/// `host` points to a valid callback table; callbacks stay valid until replacement.
/// UI/input-thread only, with no device destruction during callback dispatch.
#[no_mangle]
pub unsafe extern "C" fn tp_route_init(host: *const Host) {
    let host = host.read();
    routing().host = host;
    tp_set_sink(&SINK, ptr::null_mut());
}
