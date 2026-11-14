//! libwayland-client ABI and generated protocol requests. Opaque proxies are
//! borrowed by the UI thread; every request matches the scanner-generated header.
use super::*;
pub(super) use crate::wayland::ffi::Interface;
pub(super) type Proxy = *mut c_void;
#[link(name = "wayland-client")]
extern "C" {
    pub fn wl_display_connect(name: *const c_char) -> Proxy;
    pub fn wl_display_disconnect(dpy: Proxy);
    pub fn wl_display_get_fd(dpy: Proxy) -> i32;
    pub fn wl_display_roundtrip(dpy: Proxy) -> i32;
    pub fn wl_display_flush(dpy: Proxy) -> i32;
    pub fn wl_display_prepare_read(dpy: Proxy) -> i32;
    pub fn wl_display_cancel_read(dpy: Proxy);
    pub fn wl_display_read_events(dpy: Proxy) -> i32;
    pub fn wl_display_dispatch_pending(dpy: Proxy) -> i32;
    pub fn wl_proxy_marshal_flags(
        p: Proxy,
        op: u32,
        iface: *const Interface,
        version: u32,
        flags: u32,
        ...
    ) -> Proxy;
    pub fn wl_proxy_get_version(p: Proxy) -> u32;
    pub fn wl_proxy_add_listener(
        p: Proxy,
        callbacks: *const unsafe extern "C" fn(),
        data: *mut c_void,
    ) -> i32;
    pub fn wl_proxy_destroy(p: Proxy);
    pub static wl_registry_interface: Interface;
    pub static wl_compositor_interface: Interface;
    pub static wl_surface_interface: Interface;
    pub static wl_shm_interface: Interface;
    pub static wl_shm_pool_interface: Interface;
    pub static wl_buffer_interface: Interface;
    pub static wl_callback_interface: Interface;
    pub static wl_seat_interface: Interface;
    pub static wl_pointer_interface: Interface;
    pub static wl_keyboard_interface: Interface;
}
extern "C" {
    pub static xdg_wm_base_interface: Interface;
    pub static xdg_surface_interface: Interface;
    pub static xdg_toplevel_interface: Interface;
    pub static zxdg_decoration_manager_v1_interface: Interface;
    pub static zxdg_toplevel_decoration_v1_interface: Interface;
    pub static zwp_pointer_gestures_v1_interface: Interface;
    pub static zwp_pointer_gesture_pinch_v1_interface: Interface;
    pub static zwp_pointer_gesture_swipe_v1_interface: Interface;
}
pub(super) unsafe fn request(p: Proxy, op: u32) {
    wl_proxy_marshal_flags(p, op, ptr::null(), wl_proxy_get_version(p), 0);
}
pub(super) unsafe fn destroy(p: Proxy, op: u32) {
    if !p.is_null() {
        wl_proxy_marshal_flags(p, op, ptr::null(), wl_proxy_get_version(p), 1);
    }
}
pub(super) unsafe fn create(p: Proxy, op: u32, iface: &Interface) -> Proxy {
    wl_proxy_marshal_flags(
        p,
        op,
        iface,
        wl_proxy_get_version(p),
        0,
        ptr::null::<c_void>(),
    )
}
pub(super) unsafe fn object(p: Proxy, op: u32, iface: &Interface, arg: Proxy) -> Proxy {
    wl_proxy_marshal_flags(
        p,
        op,
        iface,
        wl_proxy_get_version(p),
        0,
        ptr::null::<c_void>(),
        arg,
    )
}
pub(super) unsafe fn string(p: Proxy, op: u32, s: *const c_char) {
    wl_proxy_marshal_flags(p, op, ptr::null(), wl_proxy_get_version(p), 0, s);
}
pub(super) unsafe fn uint(p: Proxy, op: u32, value: u32) {
    wl_proxy_marshal_flags(p, op, ptr::null(), wl_proxy_get_version(p), 0, value);
}
pub(super) unsafe fn pair(p: Proxy, op: u32, a: i32, b: i32) {
    wl_proxy_marshal_flags(p, op, ptr::null(), wl_proxy_get_version(p), 0, a, b);
}
pub(super) unsafe fn registry(dpy: Proxy) -> Proxy {
    create(dpy, 1, &wl_registry_interface)
}
pub(super) unsafe fn bind(reg: Proxy, name: u32, iface: &Interface, version: u32) -> Proxy {
    wl_proxy_marshal_flags(
        reg,
        0,
        iface,
        version,
        0,
        name,
        iface.name,
        version,
        ptr::null::<c_void>(),
    )
}
pub(super) unsafe fn listener(
    p: Proxy,
    callbacks: &[unsafe extern "C" fn()],
    data: *mut c_void,
) -> bool {
    !p.is_null() && wl_proxy_add_listener(p, callbacks.as_ptr(), data) == 0
}
pub(super) unsafe fn surface_attach(surface: Proxy, buffer: Proxy, w: i32, h: i32) -> Proxy {
    wl_proxy_marshal_flags(
        surface,
        1,
        ptr::null(),
        wl_proxy_get_version(surface),
        0,
        buffer,
        0i32,
        0i32,
    );
    // wl_surface.damage_buffer is v4. Older compositors use surface-local damage.
    let op = if wl_proxy_get_version(surface) >= 4 {
        9
    } else {
        2
    };
    wl_proxy_marshal_flags(
        surface,
        op,
        ptr::null(),
        wl_proxy_get_version(surface),
        0,
        0i32,
        0i32,
        w,
        h,
    );
    create(surface, 3, &wl_callback_interface)
}
pub(super) unsafe fn shm_buffer(shm: Proxy, fd: i32, w: i32, h: i32, size: i32) -> Proxy {
    let pool = wl_proxy_marshal_flags(
        shm,
        0,
        &wl_shm_pool_interface,
        wl_proxy_get_version(shm),
        0,
        ptr::null::<c_void>(),
        fd,
        size,
    );
    if pool.is_null() {
        return ptr::null_mut();
    }
    let buffer = wl_proxy_marshal_flags(
        pool,
        0,
        &wl_buffer_interface,
        wl_proxy_get_version(pool),
        0,
        ptr::null::<c_void>(),
        0i32,
        w,
        h,
        w * 4,
        0u32,
    );
    destroy(pool, 1);
    buffer
}
