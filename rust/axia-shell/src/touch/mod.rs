//! Multitouch protocol-B driver with owned devices and a transitional C ABI.
//! Calls which dispatch callbacks are confined to the compositor input thread.
mod device;
mod gestures;
mod route;
use device::Device;
use std::{
    cell::UnsafeCell,
    ffi::{c_char, c_int, c_void},
    ptr,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};
const MAX_SLOTS: usize = 10;
const MAX_DEVICES: usize = 16;
const TOUCHPAD: c_int = 1;
const TOUCHSCREEN: c_int = 2;
const BEGIN: c_int = 1;
const UPDATE: c_int = 2;
const END: c_int = 3;
const CANCEL: c_int = 4;
const MOMENTUM: c_int = 5;
const MOMENTUM_END: c_int = 6;
const TAP_TIME: f64 = 200.0;
const TAP_DRAG_TIME: f64 = 180.0;
const EDGE_TIME: f64 = 300.0;
const TS_PRESS_TIME: f64 = 90.0;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Config {
    speed: f64,
    tap: c_int,
    tap_drag: c_int,
    natural: c_int,
    secondary: c_int,
    three_drag: c_int,
    dwt: c_int,
    scroll: c_int,
    kinetic: c_int,
    pinch: c_int,
    swipe3: c_int,
    swipe4: c_int,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            speed: 0.5,
            tap: 1,
            tap_drag: 1,
            natural: 1,
            secondary: 1,
            three_drag: 0,
            dwt: 1,
            scroll: 1,
            kinetic: 1,
            pinch: 1,
            swipe3: 1,
            swipe4: 0,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Sink {
    motion: Option<unsafe extern "C" fn(*mut c_void, f64, f64, f64, f64)>,
    button: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int)>,
    scroll: Option<unsafe extern "C" fn(*mut c_void, c_int, f64, f64)>,
    pinch: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, f64, f64, f64, f64)>,
    swipe: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, f64, f64)>,
    hold: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int)>,
    touch: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, f64, f64) -> c_int>,
    touch_frame: Option<unsafe extern "C" fn(*mut c_void)>,
    warp: Option<unsafe extern "C" fn(*mut c_void, f64, f64)>,
    frame: Option<unsafe extern "C" fn(*mut c_void)>,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Caps {
    kind: c_int,
    clickpad: c_int,
    has_right: c_int,
    nslots: c_int,
    max_fingers: c_int,
    x: libc::input_absinfo,
    y: libc::input_absinfo,
    pressure: libc::input_absinfo,
    major: libc::input_absinfo,
    has_pressure: c_int,
    has_major: c_int,
    name: [u8; 80],
}
impl Default for Caps {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
// Callbacks may query device metadata while a device's state machine has an
// exclusive borrow. Keep immutable metadata outside that state, and access the
// input-thread-only state through UnsafeCell without aliasing references.
struct Registered {
    device: Box<UnsafeCell<Device>>,
    caps: Arc<Caps>,
    width: f64,
    height: f64,
}
struct Manager {
    config: Config,
    sink: Sink,
    context: usize,
    screen: (c_int, c_int),
    last_key: f64,
    devices: Vec<Registered>,
}
impl Default for Manager {
    fn default() -> Self {
        Self {
            config: Config::default(),
            sink: Sink::default(),
            context: 0,
            screen: (1920, 1080),
            last_key: -1e12,
            devices: Vec::new(),
        }
    }
}
static MANAGER: OnceLock<Mutex<Manager>> = OnceLock::new();
fn manager() -> MutexGuard<'static, Manager> {
    MANAGER
        .get_or_init(|| Mutex::new(Manager::default()))
        .lock()
        .unwrap()
}
fn config() -> Config {
    manager().config
}
fn screen() -> (c_int, c_int) {
    manager().screen
}
fn last_key() -> f64 {
    manager().last_key
}
fn sink() -> (Sink, *mut c_void) {
    let m = manager();
    (m.sink, m.context as *mut c_void)
}
fn out_motion(dx: f64, dy: f64, rx: f64, ry: f64) {
    let (s, u) = sink();
    if let Some(f) = s.motion {
        unsafe { f(u, dx, dy, rx, ry) }
    }
}
fn out_button(button: c_int, down: c_int) {
    let (s, u) = sink();
    if let Some(f) = s.button {
        unsafe { f(u, button, down) }
    }
}
fn out_scroll(phase: c_int, dx: f64, dy: f64) {
    let (s, u) = sink();
    if let Some(f) = s.scroll {
        unsafe { f(u, phase, dx, dy) }
    }
}
fn out_pinch(phase: c_int, n: c_int, dx: f64, dy: f64, scale: f64, rotation: f64) {
    let (s, u) = sink();
    if let Some(f) = s.pinch {
        unsafe { f(u, phase, n, dx, dy, scale, rotation) }
    }
}
fn out_swipe(phase: c_int, n: c_int, dx: f64, dy: f64) {
    let (s, u) = sink();
    if let Some(f) = s.swipe {
        unsafe { f(u, phase, n, dx, dy) }
    }
}
fn out_hold(phase: c_int, n: c_int) {
    let (s, u) = sink();
    if let Some(f) = s.hold {
        unsafe { f(u, phase, n) }
    }
}
fn out_touch(phase: c_int, id: c_int, x: f64, y: f64) -> c_int {
    let (s, u) = sink();
    s.touch.map_or(0, |f| unsafe { f(u, phase, id, x, y) })
}
fn out_touch_frame() {
    let (s, u) = sink();
    if let Some(f) = s.touch_frame {
        unsafe { f(u) }
    }
}
fn out_warp(x: f64, y: f64) {
    let (s, u) = sink();
    if let Some(f) = s.warp {
        unsafe { f(u, x, y) }
    }
}
fn out_frame() {
    let (s, u) = sink();
    if let Some(f) = s.frame {
        unsafe { f(u) }
    }
}
fn mono_ms() -> f64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as f64 * 1e3 + t.tv_nsec as f64 / 1e6
}
#[no_mangle]
pub extern "C" fn tp_config() -> *const Config {
    &manager().config
}
/// # Safety
/// `value` must point to a readable configuration; input-thread mutation only.
#[no_mangle]
pub unsafe extern "C" fn tp_set_config(value: *const Config) {
    let mut value = value.read();
    if value.speed < 0.0 {
        value.speed = 0.0;
    }
    if value.speed > 1.0 {
        value.speed = 1.0;
    }
    if !(0..=2).contains(&value.secondary) {
        value.secondary = 0;
    }
    manager().config = value;
}
/// # Safety
/// The callbacks and context must stay valid until replacement. Callbacks must
/// not free a device being dispatched or recursively dispatch that same device.
#[no_mangle]
pub unsafe extern "C" fn tp_set_sink(value: *const Sink, context: *mut c_void) {
    let value = if value.is_null() {
        Sink::default()
    } else {
        value.read()
    };
    let mut m = manager();
    m.sink = value;
    m.context = context as usize;
}
#[no_mangle]
pub extern "C" fn tp_set_screen(width: c_int, height: c_int) {
    if width > 0 && height > 0 {
        manager().screen = (width, height);
    }
}
#[no_mangle]
pub extern "C" fn tp_accel(velocity: f64, setting: f64) -> f64 {
    let setting = if setting < 0.0 {
        0.0
    } else if setting > 1.0 {
        1.0
    } else {
        setting
    };
    let factor = 0.5 * 4.0_f64.powf(setting);
    let mut t = (velocity - 12.0) / (170.0 - 12.0);
    if t < 0.0 {
        t = 0.0;
    }
    if t > 1.0 {
        t = 1.0;
    }
    t = t * t * (3.0 - 2.0 * t);
    factor * (2.4 + (15.0 - 2.4) * t)
}
/// # Safety
/// `caps` is readable; callers serialize all device lifecycle/dispatch calls.
#[no_mangle]
pub unsafe extern "C" fn tp_new(caps: *const Caps) -> *mut Device {
    let caps = caps.read();
    let mut m = manager();
    if m.devices.len() >= MAX_DEVICES {
        return ptr::null_mut();
    }
    let value = Device::new(caps);
    let record = Registered {
        caps: Arc::new(value.c),
        width: value.wmm,
        height: value.hmm,
        device: Box::new(UnsafeCell::new(value)),
    };
    let pointer = record.device.get();
    m.devices.push(record);
    pointer
}
/// # Safety
/// `device` is null or a live handle from tp_new/tp_open, not currently inside a
/// callback or dispatch. The handle and borrowed caps are invalid after return.
#[no_mangle]
pub unsafe extern "C" fn tp_free(device: *mut Device) {
    if device.is_null() {
        return;
    }
    let mut m = manager();
    if let Some(index) = m.devices.iter().position(|d| d.device.get() == device) {
        m.devices.swap_remove(index);
    }
}
/// # Safety
/// `device` is null or a live handle, without concurrent free.
#[no_mangle]
pub unsafe extern "C" fn tp_kind(device: *const Device) -> c_int {
    if device.is_null() {
        0
    } else {
        manager()
            .devices
            .iter()
            .find(|d| ptr::eq(d.device.get(), device))
            .map_or(0, |d| d.caps.kind)
    }
}
/// # Safety
/// `device` is a live handle. Returned caps are borrowed until device destruction.
#[no_mangle]
pub unsafe extern "C" fn tp_caps(device: *const Device) -> *const Caps {
    manager()
        .devices
        .iter()
        .find(|d| ptr::eq(d.device.get(), device))
        .map_or(ptr::null(), |d| Arc::as_ptr(&d.caps))
}
#[no_mangle]
pub extern "C" fn tp_count(kind: c_int) -> c_int {
    manager()
        .devices
        .iter()
        .filter(|device| kind == 0 || device.caps.kind == kind)
        .count() as c_int
}
/// # Safety
/// For positive size, `out` holds that many writable bytes; input-thread use only.
#[no_mangle]
pub unsafe extern "C" fn tp_list_json(out: *mut c_char, size: c_int) -> c_int {
    if size < 0 {
        return 0;
    }
    let devices: Vec<_> = manager()
        .devices
        .iter()
        .map(|d| (*d.caps, d.width, d.height))
        .collect();
    let mut bytes = vec![b'['];
    for (index, (caps, width, height)) in devices.iter().enumerate() {
        if bytes.len() as i32 >= size - 200 {
            break;
        }
        if index != 0 {
            bytes.push(b',');
        }
        bytes.extend_from_slice(b"{\"name\":\"");
        bytes.extend(
            caps.name
                .iter()
                .copied()
                .take_while(|&b| b != 0)
                .filter(|&b| b != b'"' && b != b'\\' && b >= 32)
                .take(78),
        );
        bytes.extend_from_slice(format!("\",\"kind\":\"{}\",\"clickpad\":{},\"width\":{:.0},\"height\":{:.0},\"fingers\":{}}}",if caps.kind==TOUCHSCREEN{"touchscreen"}else{"touchpad"},caps.clickpad,width,height,caps.max_fingers).as_bytes());
    }
    if (bytes.len() as i32) < size - 1 {
        bytes.push(b']');
    }
    if size > 0 {
        let len = bytes.len().min(size as usize - 1);
        ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast(), len);
        out.add(len).write(0);
    }
    bytes.len() as c_int
}
#[no_mangle]
pub extern "C" fn tp_key_activity(code: c_int, now: f64) {
    if ![29, 97, 42, 54, 56, 100, 125, 126, 58, 464].contains(&code) {
        manager().last_key = now;
    }
}
/// # Safety
/// The handle is live and exclusively dispatched on the input thread. Callback
/// contracts from tp_set_sink apply. `event` points to a readable input_event.
#[no_mangle]
pub unsafe extern "C" fn tp_event_at(
    device: *mut Device,
    event: *const libc::input_event,
    now: f64,
) {
    (*device).event(&*event, now);
}
/// # Safety
/// Same handle, callback and event requirements as tp_event_at.
#[no_mangle]
pub unsafe extern "C" fn tp_event(device: *mut Device, event: *const libc::input_event) {
    let event = &*event;
    let mut time = event.time.tv_sec as f64 * 1e3 + event.time.tv_usec as f64 / 1e3;
    if time <= 0.0 {
        time = mono_ms();
    }
    (*device).event(event, time);
}
fn devices() -> Vec<*mut Device> {
    manager().devices.iter().map(|d| d.device.get()).collect()
}
/// # Safety
/// Input-thread only, outside device callbacks; no active device mutation/free.
#[no_mangle]
pub unsafe extern "C" fn tp_timeout(now: f64) -> f64 {
    let mut best = -1.0;
    for d in devices() {
        (*d).timeout(now, &mut best);
    }
    best
}
/// # Safety
/// Input-thread only; callbacks must not destroy or recursively dispatch devices.
#[no_mangle]
pub unsafe extern "C" fn tp_tick(now: f64) {
    for d in devices() {
        (*d).tick(now);
    }
}
/// # Safety
/// Live exclusively accessed device and readable SYN input_event; input-thread only.
#[no_mangle]
pub unsafe extern "C" fn tp_latency(device: *mut Device, syn: *const libc::input_event) {
    (*device).latency(&*syn);
}

#[cfg(test)]
mod tests;
