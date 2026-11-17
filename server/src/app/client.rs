//! Wayland client, multiple toplevels, double buffers, input and event loop.
use super::{wire::*, *};
use std::{cell::UnsafeCell, fs, os::unix::fs::DirBuilderExt};
const MAX_WINS: usize = 16;
macro_rules! callbacks {($name:ident;$($f:ident),+)=>{static $name:&[unsafe extern "C" fn()]=&[$(unsafe{std::mem::transmute($f as *const ())}),+];};}
extern "C" {
    fn axia_app_paint(node: i32, main: i32, px: *mut u32, w: i32, h: i32) -> i32;
    fn axia_app_hit(node: i32, main: i32, x: f32, y: f32) -> i32;
    fn axia_app_node_valid(node: i32) -> i32;
    fn axia_app_dirty() -> i32;
    fn paint_cache_tick(now: f64) -> f64;
    fn axia_app_quit() -> i32;
    fn text_init(fonts: *const c_char) -> i32;
    fn js_broker_set_pkgdir(dir: *const c_char, system: i32);
    fn js_broker_wants_network() -> i32;
    fn js_broker_code_refused() -> i32;
    fn js_broker_app_event(kind: *const c_char, json: *const c_char, len: usize);
    fn js_broker_pending() -> i32;
    fn js_broker_fd() -> i32;
    fn js_broker_poll();
    fn js_init_app(bundle: *const c_char, id: *const c_char) -> i32;
    fn js_app_recovery() -> i32;
    fn js_shutdown();
    fn js_tick(now: f64) -> f64;
    fn js_frame_ready(now: f64, waiting: i32, epoch: u32) -> i32;
    fn js_frame(now: f64);
    fn js_frame_wait(now: f64, waiting: i32, epoch: u32) -> f64;
    fn js_timers_changed() -> i32;
    fn js_pointer(kind: *const c_char, id: i32, x: f32, y: f32, button: i32);
    fn js_gesture(
        kind: *const c_char,
        phase: *const c_char,
        id: i32,
        x: f32,
        y: f32,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
    );
    fn js_key(code: i32, value: i32);
    fn tzset();
}
#[derive(Default)]
struct Buffer {
    proxy: Proxy,
    pixels: *mut u32,
    size: usize,
    busy: bool,
    w: i32,
    h: i32,
}
#[derive(Default)]
struct Window {
    used: bool,
    key: i32,
    surface: Proxy,
    xdg: Proxy,
    top: Proxy,
    deco: Proxy,
    w: i32,
    h: i32,
    configured: bool,
    dirty: bool,
    frame: Proxy,
    node: i32,
    title: Vec<u8>,
    buffers: [Buffer; 2],
}
struct State {
    display: Proxy,
    compositor: Proxy,
    shm: Proxy,
    wm: Proxy,
    seat: Proxy,
    pointer: Proxy,
    keyboard: Proxy,
    decorations: Proxy,
    gestures: Proxy,
    gestures_done: bool,
    windows: Vec<Box<Window>>,
    next_key: i32,
    closed: bool,
    epoch: u32,
    start: f64,
    first_frame: bool,
    id: CString,
    ptr: *mut Window,
    kbd: *mut Window,
    x: f64,
    y: f64,
    axis: i32,
    fine: bool,
    rotation: f64,
    fingers: u32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            display: ptr::null_mut(),
            compositor: ptr::null_mut(),
            shm: ptr::null_mut(),
            wm: ptr::null_mut(),
            seat: ptr::null_mut(),
            pointer: ptr::null_mut(),
            keyboard: ptr::null_mut(),
            decorations: ptr::null_mut(),
            gestures: ptr::null_mut(),
            gestures_done: false,
            windows: (0..MAX_WINS).map(|_| Box::default()).collect(),
            next_key: 1,
            closed: false,
            epoch: 0,
            start: now(),
            first_frame: false,
            id: CString::default(),
            ptr: ptr::null_mut(),
            kbd: ptr::null_mut(),
            x: 0.,
            y: 0.,
            axis: -1,
            fine: false,
            rotation: 0.,
            fingers: 0,
        }
    }
}
// Only the UI thread enters app mode. Raw pointers avoid keeping a RefCell or
// exclusive Rust borrow across QuickJS callbacks that can open/close a window.
thread_local! {static STATE:UnsafeCell<State>=UnsafeCell::new(State::default());}
fn state() -> *mut State {
    STATE.with(UnsafeCell::get)
}
unsafe fn by_key(key: i32) -> *mut Window {
    (*state())
        .windows
        .iter_mut()
        .find(|w| w.used && w.key == key)
        .map_or(ptr::null_mut(), |w| &mut **w)
}
unsafe fn by_surface(surface: Proxy) -> *mut Window {
    (*state())
        .windows
        .iter_mut()
        .find(|w| w.used && w.surface == surface)
        .map_or(ptr::null_mut(), |w| &mut **w)
}
unsafe fn event(w: *mut Window, kind: &str) {
    if w.is_null() {
        return;
    }
    let json = serde_json::to_vec(&serde_json::json!({"key":(*w).key,"type":kind})).unwrap();
    js_broker_app_event(c"appwindow".as_ptr(), json.as_ptr().cast(), json.len());
}
pub(super) fn pixels(w: i32, h: i32) -> Option<usize> {
    if !(1..=8192).contains(&w) || !(1..=8192).contains(&h) {
        return None;
    }
    (w as usize).checked_mul(h as usize)?.checked_mul(4)
}
pub(super) fn size(w: i32, h: i32) -> (i32, i32) {
    (
        if (160..=8192).contains(&w) { w } else { 640 },
        if (100..=8192).contains(&h) { h } else { 480 },
    )
}
pub(super) fn waiting(windows: impl Iterator<Item = (bool, bool)>) -> bool {
    let mut configured = false;
    for (configured_window, pending) in windows {
        if configured_window {
            configured = true;
            if !pending {
                return false;
            }
        }
    }
    configured
}
unsafe fn frames_waiting() -> i32 {
    waiting(
        (*state())
            .windows
            .iter()
            .filter(|w| w.used)
            .map(|w| (w.configured, !w.frame.is_null())),
    ) as i32
}
unsafe fn buffer_free(b: *mut Buffer) {
    destroy((*b).proxy, 0);
    if !(*b).pixels.is_null() {
        libc::munmap((*b).pixels.cast(), (*b).size);
    }
    *b = Buffer::default();
}
unsafe extern "C" fn buffer_release(data: Proxy, _buffer: Proxy) {
    (*(data.cast::<Buffer>())).busy = false;
}
callbacks!(BUFFER;buffer_release);
unsafe fn buffer_make(b: *mut Buffer, w: i32, h: i32) -> bool {
    let Some(size) = pixels(w, h) else {
        return false;
    };
    buffer_free(b);
    let fd = libc::memfd_create(c"lordos-app".as_ptr(), libc::MFD_CLOEXEC);
    if fd < 0 {
        return false;
    }
    if libc::ftruncate(fd, size as libc::off_t) < 0 {
        libc::close(fd);
        return false;
    }
    let mapped = libc::mmap(
        ptr::null_mut(),
        size,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_SHARED,
        fd,
        0,
    );
    if mapped == libc::MAP_FAILED {
        libc::close(fd);
        return false;
    }
    let proxy = shm_buffer((*state()).shm, fd, w, h, size as i32);
    libc::close(fd);
    if proxy.is_null() {
        libc::munmap(mapped, size);
        return false;
    }
    *b = Buffer {
        proxy,
        pixels: mapped.cast(),
        size,
        busy: false,
        w,
        h,
    };
    if !listener(proxy, BUFFER, b.cast()) {
        buffer_free(b);
        return false;
    }
    true
}
unsafe extern "C" fn frame_done(data: Proxy, cb: Proxy, _time: u32) {
    wl_proxy_destroy(cb);
    let w = data.cast::<Window>();
    if (*w).used && (*w).frame == cb {
        (*w).frame = ptr::null_mut();
        (*state()).epoch = (*state()).epoch.wrapping_add(1);
    }
}
callbacks!(FRAME;frame_done);
unsafe fn render(w: *mut Window) {
    let b = if !(*w).buffers[0].busy {
        &raw mut (*w).buffers[0]
    } else if !(*w).buffers[1].busy {
        &raw mut (*w).buffers[1]
    } else {
        return;
    };
    if (*b).w != (*w).w || (*b).h != (*w).h {
        if !buffer_make(b, (*w).w, (*w).h) {
            return;
        }
    }
    if axia_app_paint(
        (*w).node,
        ((*w).key == 0) as i32,
        (*b).pixels,
        (*w).w,
        (*w).h,
    ) == 0
    {
        return;
    }
    let cb = surface_attach((*w).surface, (*b).proxy, (*w).w, (*w).h);
    if cb.is_null() {
        return;
    }
    if !listener(cb, FRAME, w.cast()) {
        wl_proxy_destroy(cb);
        return;
    }
    (*b).busy = true;
    (*w).frame = cb;
    request((*w).surface, 6);
    (*w).dirty = false;
    if !(*state()).first_frame && (*w).key == 0 {
        (*state()).first_frame = true;
        let t = now();
        eprintln!(
            "app {}: first frame {:.1} ms after the process started (at {t:.1} ms, monotonic)",
            (*state()).id.to_string_lossy(),
            t - (*state()).start
        );
    }
}
unsafe extern "C" fn configured(data: Proxy, xdg: Proxy, serial: u32) {
    uint(xdg, 4, serial);
    let w = data.cast::<Window>();
    (*w).configured = true;
    (*w).dirty = true;
}
callbacks!(CONFIGURE;configured);
unsafe extern "C" fn top_size(data: Proxy, _top: Proxy, w: i32, h: i32, _states: Proxy) {
    if pixels(w, h).is_some() {
        let win = data.cast::<Window>();
        if ((*win).w, (*win).h) != (w, h) {
            (*win).w = w;
            (*win).h = h;
            (*win).dirty = true;
        }
    }
}
unsafe extern "C" fn top_close(data: Proxy, _top: Proxy) {
    event(data.cast(), "close");
}
unsafe extern "C" fn top_bounds(_data: Proxy, _top: Proxy, _w: i32, _h: i32) {}
unsafe extern "C" fn top_caps(_data: Proxy, _top: Proxy, _caps: Proxy) {}
callbacks!(TOP;top_size,top_close,top_bounds,top_caps);
unsafe extern "C" fn decoration_mode(_data: Proxy, _deco: Proxy, _mode: u32) {}
callbacks!(DECORATION;decoration_mode);
unsafe extern "C" fn ping(_data: Proxy, wm: Proxy, serial: u32) {
    uint(wm, 3, serial);
}
callbacks!(WM;ping);
pub(super) fn title(value: &[u8], id: &CStr) -> Vec<u8> {
    let value = if value.is_empty() {
        id.to_bytes()
    } else {
        value
    };
    let s = String::from_utf8_lossy(value);
    let mut n = s.len().min(255);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    let mut out = s.as_bytes()[..n].to_vec();
    out.push(0);
    out
}
unsafe fn create_window(key: i32, name: *const c_char, w: i32, h: i32) -> *mut Window {
    let s = state();
    let Some(slot) = (*s).windows.iter_mut().find(|w| !w.used) else {
        return ptr::null_mut();
    };
    let win = &mut **slot as *mut Window;
    let (w, h) = size(w, h);
    *win = Window {
        used: true,
        key,
        w,
        h,
        title: title(text(name), &(*s).id),
        ..Window::default()
    };
    (*win).surface = create((*s).compositor, 0, &wl_surface_interface);
    if (*win).surface.is_null() {
        (*win).used = false;
        return ptr::null_mut();
    }
    (*win).xdg = object((*s).wm, 2, &xdg_surface_interface, (*win).surface);
    if (*win).xdg.is_null() {
        destroy_window(win);
        return ptr::null_mut();
    }
    (*win).top = create((*win).xdg, 1, &xdg_toplevel_interface);
    if (*win).top.is_null() {
        destroy_window(win);
        return ptr::null_mut();
    }
    if !listener((*win).xdg, CONFIGURE, win.cast()) || !listener((*win).top, TOP, win.cast()) {
        destroy_window(win);
        return ptr::null_mut();
    }
    string((*win).top, 3, (*s).id.as_ptr());
    string((*win).top, 2, (*win).title.as_ptr().cast());
    if !(*s).decorations.is_null() {
        (*win).deco = object(
            (*s).decorations,
            1,
            &zxdg_toplevel_decoration_v1_interface,
            (*win).top,
        );
        if !(*win).deco.is_null() {
            listener((*win).deco, DECORATION, win.cast());
            uint((*win).deco, 1, 2);
        }
    }
    request((*win).surface, 6);
    win
}
unsafe fn destroy_window(w: *mut Window) {
    if w.is_null() || !(*w).used {
        return;
    }
    let s = state();
    if (*s).ptr == w {
        (*s).ptr = ptr::null_mut();
    }
    if (*s).kbd == w {
        (*s).kbd = ptr::null_mut();
    }
    if !(*w).frame.is_null() {
        wl_proxy_destroy((*w).frame);
    }
    destroy((*w).deco, 0);
    destroy((*w).top, 0);
    destroy((*w).xdg, 0);
    destroy((*w).surface, 0);
    buffer_free(&raw mut (*w).buffers[0]);
    buffer_free(&raw mut (*w).buffers[1]);
    *w = Window::default();
    if !(*s).windows.iter().any(|w| w.used) {
        (*s).closed = true;
    }
}
#[no_mangle]
pub unsafe extern "C" fn axia_app_window_open(name: *const c_char, w: i32, h: i32) -> i32 {
    let s = state();
    if (*s).next_key >= 1 << 20 {
        return -1;
    }
    let win = create_window((*s).next_key, name, w, h);
    if win.is_null() {
        return -1;
    }
    (*s).next_key += 1;
    (*win).key
}
#[no_mangle]
pub unsafe extern "C" fn axia_app_window_attach(key: i32, node: i32) -> i32 {
    let w = by_key(key);
    if w.is_null() || axia_app_node_valid(node) == 0 {
        return 0;
    }
    if (*w).node != node {
        (*w).node = node;
        (*w).dirty = true;
    }
    1
}
#[no_mangle]
pub unsafe extern "C" fn axia_app_window_close(key: i32) {
    if key == 0 && js_broker_code_refused() != 0 {
        return;
    }
    destroy_window(by_key(key));
}
#[no_mangle]
pub unsafe extern "C" fn axia_app_window_title(key: i32, name: *const c_char) {
    let w = by_key(key);
    if w.is_null() || name.is_null() {
        return;
    }
    let value = title(text(name), &(*state()).id);
    if (*w).title != value {
        (*w).title = value;
        string((*w).top, 2, (*w).title.as_ptr().cast());
    }
}
unsafe fn hit() -> i32 {
    let s = state();
    let w = (*s).ptr;
    if w.is_null() {
        0
    } else {
        axia_app_hit(
            (*w).node,
            ((*w).key == 0) as i32,
            (*s).x as f32,
            (*s).y as f32,
        )
    }
}
unsafe fn pointer_event(kind: &CStr, button: i32) {
    let s = state();
    let (id, x, y) = (hit(), (*s).x as f32, (*s).y as f32);
    js_pointer(kind.as_ptr(), id, x, y, button);
}
unsafe fn gesture(kind: &CStr, phase: &CStr, a: f64, b: f64, c: f64, d: f64) {
    let s = state();
    js_gesture(
        kind.as_ptr(),
        phase.as_ptr(),
        hit(),
        (*s).x as f32,
        (*s).y as f32,
        a,
        b,
        c,
        d,
    );
}
unsafe extern "C" fn p_enter(
    _data: Proxy,
    _p: Proxy,
    _serial: u32,
    surface: Proxy,
    x: i32,
    y: i32,
) {
    let s = state();
    (*s).ptr = by_surface(surface);
    (*s).x = x as f64 / 256.;
    (*s).y = y as f64 / 256.;
}
unsafe extern "C" fn p_leave(_data: Proxy, _p: Proxy, _serial: u32, surface: Proxy) {
    let s = state();
    if !(*s).ptr.is_null() && (*(*s).ptr).surface == surface {
        (*s).ptr = ptr::null_mut();
    }
}
unsafe extern "C" fn p_motion(_data: Proxy, _p: Proxy, _time: u32, x: i32, y: i32) {
    (*state()).x = x as f64 / 256.;
    (*state()).y = y as f64 / 256.;
    pointer_event(c"move", 0);
}
unsafe extern "C" fn p_button(
    _data: Proxy,
    _p: Proxy,
    _serial: u32,
    _time: u32,
    button: u32,
    pressed: u32,
) {
    pointer_event(c"move", 0);
    let button = match button {
        0x110 => 0,
        0x112 => 1,
        0x111 => 2,
        _ => return,
    };
    pointer_event(if pressed == 1 { c"down" } else { c"up" }, button);
}
unsafe extern "C" fn p_axis(_data: Proxy, _p: Proxy, _time: u32, axis: u32, value: i32) {
    let s = state();
    if (*s).axis == 1 {
        if !(*s).fine {
            (*s).fine = true;
            gesture(c"scroll", c"begin", 0., 0., 0., 0.);
        }
        let v = value as f64 / 256.;
        gesture(
            c"scroll",
            c"update",
            if axis == 1 { v } else { 0. },
            if axis == 1 { 0. } else { v },
            0.,
            0.,
        );
    } else if axis == 0 {
        pointer_event(c"wheel", if value > 0 { -1 } else { 1 });
    }
}
unsafe extern "C" fn p_frame(_data: Proxy, _p: Proxy) {
    (*state()).axis = -1;
}
unsafe extern "C" fn p_source(_data: Proxy, _p: Proxy, source: u32) {
    (*state()).axis = source as i32;
}
unsafe extern "C" fn p_stop(_data: Proxy, _p: Proxy, _time: u32, _axis: u32) {
    if (*state()).fine {
        (*state()).fine = false;
        gesture(c"scroll", c"end", 0., 0., 0., 0.);
    }
}
unsafe extern "C" fn p_discrete(_data: Proxy, _p: Proxy, _axis: u32, _value: i32) {}
callbacks!(POINTER;p_enter,p_leave,p_motion,p_button,p_axis,p_frame,p_source,p_stop,p_discrete);
unsafe extern "C" fn pinch_begin(
    _data: Proxy,
    _g: Proxy,
    _serial: u32,
    _time: u32,
    _surface: Proxy,
    _fingers: u32,
) {
    (*state()).rotation = 0.;
    gesture(c"pinch", c"begin", 1., 0., 0., 0.);
}
unsafe extern "C" fn pinch_update(
    _data: Proxy,
    _g: Proxy,
    _time: u32,
    dx: i32,
    dy: i32,
    scale: i32,
    rotation: i32,
) {
    (*state()).rotation += rotation as f64 / 256.;
    gesture(
        c"pinch",
        c"update",
        scale as f64 / 256.,
        (*state()).rotation,
        dx as f64 / 256.,
        dy as f64 / 256.,
    );
}
unsafe extern "C" fn pinch_end(_data: Proxy, _g: Proxy, _serial: u32, _time: u32, cancelled: i32) {
    gesture(
        c"pinch",
        if cancelled != 0 { c"cancel" } else { c"end" },
        1.,
        (*state()).rotation,
        0.,
        0.,
    );
}
callbacks!(PINCH;pinch_begin,pinch_update,pinch_end);
unsafe extern "C" fn swipe_begin(
    _data: Proxy,
    _g: Proxy,
    _serial: u32,
    _time: u32,
    _surface: Proxy,
    fingers: u32,
) {
    (*state()).fingers = fingers;
    gesture(c"swipe", c"begin", 0., 0., fingers as f64, 0.);
}
unsafe extern "C" fn swipe_update(_data: Proxy, _g: Proxy, _time: u32, dx: i32, dy: i32) {
    gesture(
        c"swipe",
        c"update",
        dx as f64 / 256.,
        dy as f64 / 256.,
        (*state()).fingers as f64,
        0.,
    );
}
unsafe extern "C" fn swipe_end(_data: Proxy, _g: Proxy, _serial: u32, _time: u32, cancelled: i32) {
    gesture(
        c"swipe",
        if cancelled != 0 { c"cancel" } else { c"end" },
        0.,
        0.,
        (*state()).fingers as f64,
        0.,
    );
}
callbacks!(SWIPE;swipe_begin,swipe_update,swipe_end);
unsafe fn setup_gestures() {
    let s = state();
    if (*s).gestures_done || (*s).gestures.is_null() || (*s).pointer.is_null() {
        return;
    }
    (*s).gestures_done = true;
    listener(
        object(
            (*s).gestures,
            1,
            &zwp_pointer_gesture_pinch_v1_interface,
            (*s).pointer,
        ),
        PINCH,
        ptr::null_mut(),
    );
    listener(
        object(
            (*s).gestures,
            0,
            &zwp_pointer_gesture_swipe_v1_interface,
            (*s).pointer,
        ),
        SWIPE,
        ptr::null_mut(),
    );
}
unsafe extern "C" fn k_keymap(_data: Proxy, _k: Proxy, format: u32, fd: i32, size: u32) {
    if fd < 0 {
        return;
    }
    let mut st: libc::stat = std::mem::zeroed();
    if format == 1
        && size > 1
        && size < 16 << 20
        && libc::fstat(fd, &mut st) == 0
        && st.st_size >= size as i64
    {
        let p = libc::mmap(
            ptr::null_mut(),
            size as usize,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd,
            0,
        );
        if p != libc::MAP_FAILED {
            let bytes = std::slice::from_raw_parts(p.cast::<u8>(), size as usize);
            if bytes.last() == Some(&0) || !bytes.contains(&0) {
                let end = bytes.iter().position(|c| *c == 0).unwrap_or(bytes.len());
                if let Ok(txt) = CString::new(&bytes[..end]) {
                    crate::keyboard::kb_set_keymap_string(txt.as_ptr());
                }
            }
            libc::munmap(p, size as usize);
        }
    }
    libc::close(fd);
}
unsafe extern "C" fn k_enter(_data: Proxy, _k: Proxy, _serial: u32, surface: Proxy, _keys: Proxy) {
    crate::keyboard::kb_reset_state();
    let w = by_surface(surface);
    if !w.is_null() && w != (*state()).kbd {
        (*state()).kbd = w;
        event(w, "focus");
    }
}
unsafe extern "C" fn k_leave(_data: Proxy, _k: Proxy, _serial: u32, _surface: Proxy) {
    crate::keyboard::kb_reset_state();
}
unsafe extern "C" fn k_key(
    _data: Proxy,
    _k: Proxy,
    _serial: u32,
    _time: u32,
    key: u32,
    pressed: u32,
) {
    js_key(key as i32, (pressed == 1) as i32);
}
unsafe extern "C" fn k_modifiers(
    _data: Proxy,
    _k: Proxy,
    _serial: u32,
    _depressed: u32,
    _latched: u32,
    _locked: u32,
    group: u32,
) {
    crate::keyboard::kb_set_group(group as i32);
}
unsafe extern "C" fn k_repeat(_data: Proxy, _k: Proxy, _rate: i32, _delay: i32) {}
callbacks!(KEYBOARD;k_keymap,k_enter,k_leave,k_key,k_modifiers,k_repeat);
unsafe extern "C" fn seat_caps(_data: Proxy, seat: Proxy, caps: u32) {
    let s = state();
    if caps & 1 != 0 && (*s).pointer.is_null() {
        (*s).pointer = create(seat, 0, &wl_pointer_interface);
        listener((*s).pointer, POINTER, ptr::null_mut());
        setup_gestures();
    }
    if caps & 2 != 0 && (*s).keyboard.is_null() {
        (*s).keyboard = create(seat, 1, &wl_keyboard_interface);
        listener((*s).keyboard, KEYBOARD, ptr::null_mut());
    }
}
unsafe extern "C" fn seat_name(_data: Proxy, _seat: Proxy, _name: *const c_char) {}
callbacks!(SEAT;seat_caps,seat_name);
unsafe extern "C" fn global(
    _data: Proxy,
    reg: Proxy,
    name: u32,
    iface: *const c_char,
    version: u32,
) {
    let s = state();
    match text(iface) {
        b"wl_compositor" => {
            (*s).compositor = bind(reg, name, &wl_compositor_interface, version.min(4))
        }
        b"wl_shm" => (*s).shm = bind(reg, name, &wl_shm_interface, 1),
        b"xdg_wm_base" => {
            (*s).wm = bind(reg, name, &xdg_wm_base_interface, version.min(6));
            listener((*s).wm, WM, ptr::null_mut());
        }
        b"wl_seat" => {
            (*s).seat = bind(reg, name, &wl_seat_interface, version.min(7));
            listener((*s).seat, SEAT, ptr::null_mut());
        }
        b"zxdg_decoration_manager_v1" => {
            (*s).decorations = bind(reg, name, &zxdg_decoration_manager_v1_interface, 1)
        }
        b"zwp_pointer_gestures_v1" => {
            (*s).gestures = bind(reg, name, &zwp_pointer_gestures_v1_interface, 1);
            setup_gestures();
        }
        _ => {}
    }
}
unsafe extern "C" fn global_remove(_data: Proxy, _reg: Proxy, _name: u32) {}
callbacks!(REGISTRY;global,global_remove);
pub(super) fn manifest_pair(bytes: &[u8], key: &str) -> Option<(i32, i32)> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let pair = v.get(key)?.as_array()?;
    if pair.len() != 2 {
        return None;
    }
    let w = i32::try_from(pair[0].as_i64()?).ok()?;
    let h = i32::try_from(pair[1].as_i64()?).ok()?;
    ((160..=8192).contains(&w) && (100..=8192).contains(&h)).then_some((w, h))
}
unsafe fn run(id: &CStr, bundle: &CStr, fonts: &CStr) -> i32 {
    let s = state();
    (*s).id = id.to_owned();
    (*s).start = now();
    libc::prctl(libc::PR_SET_NAME, id.as_ptr(), 0, 0, 0);
    let fonts_path = path(fonts.as_ptr());
    let assets = fonts_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(&fonts_path);
    let assets_c = cpath(assets);
    codesig_init(assets_c.as_ptr());
    let Some((pkg, system)) = super::spawn::package(id, assets, &path(lpk_root())) else {
        eprintln!(
            "axia-server --app: app '{}' is not installed",
            id.to_string_lossy()
        );
        return 2;
    };
    let pkg = cpath(&pkg);
    let home =
        std::env::var_os("HOME").map_or_else(|| PathBuf::from("/nonexistent"), PathBuf::from);
    let parent = home.join(".local/share/axia/appdata");
    let data = parent.join(OsStr::from_bytes(id.to_bytes()));
    // Preserve ownership/modes of existing folders; only newly created dirs use 0700.
    let _ = fs::DirBuilder::new().mode(0o700).create(&parent);
    let _ = fs::DirBuilder::new().mode(0o700).create(&data);
    let data = cpath(&data);
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(|v| CString::new(v.as_bytes()).unwrap());
    let host_path = path(bundle.as_ptr());
    let host_dir = cpath(
        host_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    );
    let langs = cpath(&home.join(".local/share/axia/locales"));
    let ro = [
        fonts.as_ptr(),
        host_dir.as_ptr(),
        c"/usr/share/fonts".as_ptr(),
        langs.as_ptr(),
        ptr::null(),
    ];
    let mut sandbox = SandboxConfig {
        id: id.as_ptr(),
        pkg_dir: pkg.as_ptr(),
        data_dir: data.as_ptr(),
        runtime_dir: runtime.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
        ro_extra: ro.as_ptr(),
        dev_files: ptr::null(),
        network: 0,
    };
    if system && id.to_bytes() == b"sysprobe" {
        let mut child = -1;
        let fd = super::probe::probe_helper_start(data.as_ptr(), &mut child);
        let mask = sandbox_apply(&sandbox);
        return super::probe::probe_main(cpath(&home).as_ptr(), data.as_ptr(), mask, fd, child);
    }
    if text_init(fonts.as_ptr()) != 0 {
        eprintln!("axia-server --app: no fonts");
        return 1;
    }
    js_sys_set_assets(cpath(&fonts_path.join("..")).as_ptr());
    crate::keyboard::kb_set(c"us".as_ptr(), c"".as_ptr());
    (*s).display = wl_display_connect(ptr::null());
    if (*s).display.is_null() {
        eprintln!("axia-server --app: could not connect to the compositor");
        return 1;
    }
    let reg = registry((*s).display);
    if !listener(reg, REGISTRY, ptr::null_mut())
        || wl_display_roundtrip((*s).display) < 0
        || (*s).compositor.is_null()
        || (*s).shm.is_null()
        || (*s).wm.is_null()
    {
        eprintln!("axia-server --app: the compositor lacks a required interface");
        return 1;
    }
    use std::io::Read;
    let mut manifest = Vec::new();
    if let Ok(f) = fs::File::open(path(pkg.as_ptr()).join("manifest.json")) {
        let _ = f.take(8192).read_to_end(&mut manifest);
    }
    let (w, h) = manifest_pair(&manifest, "size").unwrap_or((640, 480));
    let main = create_window(0, id.as_ptr(), w, h);
    if main.is_null() {
        return 1;
    }
    if let Some((w, h)) = manifest_pair(&manifest, "minSize") {
        pair((*main).top, 8, w, h);
    }
    if wl_display_roundtrip((*s).display) < 0 {
        return 1;
    }
    js_broker_set_pkgdir(pkg.as_ptr(), system as i32);
    sandbox.network = js_broker_wants_network();
    tzset();
    sandbox_apply(&sandbox);
    let loaded = js_init_app(bundle.as_ptr(), id.as_ptr());
    let mut recovered = loaded == -2;
    if recovered {
        eprintln!(
            "axia-server --app: the app host was modified and was not loaded (recovery screen)"
        );
    } else if loaded != 0 {
        eprintln!("axia-server --app: could not load the app");
        return 1;
    }
    let fd = wl_display_get_fd((*s).display);
    while !(*s).closed && axia_app_quit() == 0 {
        let mut delay = -1.;
        for _ in 0..64 {
            delay = js_tick(now());
            if delay != 0. {
                break;
            }
        }
        let cache_wait = paint_cache_tick(now());
        if cache_wait >= 0. && (delay < 0. || cache_wait < delay) {
            delay = cache_wait;
        }
        if js_frame_ready(now(), frames_waiting(), (*s).epoch) != 0 {
            js_frame(now());
        }
        if !recovered && js_broker_code_refused() != 0 {
            recovered = true;
            js_app_recovery();
            for w in &mut (*s).windows {
                if w.used {
                    if w.key == 0 {
                        w.node = 0;
                    }
                    w.dirty = true;
                }
            }
            continue;
        }
        if axia_app_dirty() != 0 {
            for w in &mut (*s).windows {
                if w.used {
                    w.dirty = true;
                }
            }
        }
        // Do not borrow the window vector over paint/QuickJS calls.
        for i in 0..MAX_WINS {
            let w = &mut *(&mut (*s).windows)[i] as *mut Window;
            if (*w).used && (*w).configured && (*w).dirty && (*w).frame.is_null() {
                render(w);
            }
        }
        let wait = js_frame_wait(now(), frames_waiting(), (*s).epoch);
        if wait >= 0. && (delay < 0. || wait < delay) {
            delay = wait;
        }
        if js_timers_changed() != 0 {
            delay = 0.;
        }
        wl_display_flush((*s).display);
        let bfd = js_broker_fd();
        let mut fds = [
            libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: bfd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let n = if bfd >= 0 { 2 } else { 1 };
        let wait = if js_broker_pending() != 0 {
            0
        } else if delay < 0. {
            100
        } else {
            delay.ceil().min(i32::MAX as f64) as i32
        };
        if wl_display_prepare_read((*s).display) != 0 {
            if wl_display_dispatch_pending((*s).display) < 0 {
                break;
            }
            continue;
        }
        if libc::poll(fds.as_mut_ptr(), n, wait) < 0 && errno() != libc::EINTR {
            wl_display_cancel_read((*s).display);
            break;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            if wl_display_read_events((*s).display) < 0 {
                break;
            }
        } else {
            wl_display_cancel_read((*s).display);
        }
        if wl_display_dispatch_pending((*s).display) < 0 {
            break;
        }
        if (n > 1 && fds[1].revents & libc::POLLIN != 0) || js_broker_pending() != 0 {
            js_broker_poll();
        }
        if fds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            break;
        }
    }
    js_shutdown();
    0
}
#[no_mangle]
pub unsafe extern "C" fn app_main(
    id: *const c_char,
    bundle: *const c_char,
    fonts: *const c_char,
) -> i32 {
    if id.is_null() || bundle.is_null() || fonts.is_null() || lpk_valid_id(id) == 0 {
        eprintln!("axia-server --app: invalid id");
        return 2;
    }
    let result = run(
        CStr::from_ptr(id),
        CStr::from_ptr(bundle),
        CStr::from_ptr(fonts),
    );
    let s = state();
    if !(*s).display.is_null() {
        for i in 0..MAX_WINS {
            destroy_window(&mut *(&mut (*s).windows)[i]);
        }
        wl_display_disconnect((*s).display);
        (*s).display = ptr::null_mut();
    }
    result
}
