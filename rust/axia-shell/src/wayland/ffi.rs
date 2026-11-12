//! The public libwayland-server ABI. Opaque handles are borrowed, never dereferenced.
use libc::{c_char, c_int, c_void};
use std::cell::UnsafeCell;

pub type Client = c_void;
pub type Resource = c_void;
pub type Display = c_void;
pub type EventLoop = c_void;
pub type EventSource = c_void;

#[repr(C)]
pub struct List {
    pub prev: *mut List,
    pub next: *mut List,
}

#[repr(C)]
pub struct Listener {
    // libwayland owns the intrusive links even while Rust owns the listener.
    pub link: UnsafeCell<List>,
    pub notify: Option<unsafe extern "C" fn(*mut Listener, *mut c_void)>,
}

#[repr(C)]
pub struct Message {
    pub name: *const c_char,
    pub signature: *const c_char,
    pub types: *const *const Interface,
}

#[repr(C)]
pub struct Interface {
    pub name: *const c_char,
    pub version: c_int,
    pub method_count: c_int,
    pub methods: *const Message,
    pub event_count: c_int,
    pub events: *const Message,
}

// Protocol descriptors only point to immutable static strings, arrays and descriptors.
unsafe impl Sync for Message {}
unsafe impl Sync for Interface {}
pub struct Types<const N: usize>(pub [*const Interface; N]);
unsafe impl<const N: usize> Sync for Types<N> {}

impl Message {
    pub const fn new(
        name: &'static [u8],
        signature: &'static [u8],
        types: *const *const Interface,
    ) -> Self {
        Self {
            name: name.as_ptr().cast(),
            signature: signature.as_ptr().cast(),
            types,
        }
    }
}

pub type Destroy = unsafe extern "C" fn(*mut Resource);
pub type Bind = unsafe extern "C" fn(*mut Client, *mut c_void, u32, u32);

#[link(name = "wayland-server")]
extern "C" {
    pub fn wl_display_get_event_loop(display: *mut Display) -> *mut EventLoop;
    pub fn wl_event_loop_add_fd(
        event_loop: *mut EventLoop,
        fd: c_int,
        mask: u32,
        callback: Option<unsafe extern "C" fn(c_int, u32, *mut c_void) -> c_int>,
        data: *mut c_void,
    ) -> *mut EventSource;
    pub fn wl_event_source_remove(source: *mut EventSource) -> c_int;
    pub fn wl_client_create(display: *mut Display, fd: c_int) -> *mut Client;
    pub fn wl_client_destroy(client: *mut Client);
    pub fn wl_client_add_destroy_listener(client: *mut Client, listener: *mut Listener);
    pub static wl_surface_interface: Interface;
    pub static wl_seat_interface: Interface;
    pub static wl_pointer_interface: Interface;
    pub static wl_touch_interface: Interface;
    pub fn wl_display_next_serial(display: *mut Display) -> u32;
    pub fn wl_resource_get_client(resource: *mut Resource) -> *mut Client;
    pub fn wl_resource_from_link(link: *mut List) -> *mut Resource;
    pub fn wl_resource_add_destroy_listener(resource: *mut Resource, listener: *mut Listener);
    pub fn wl_list_remove(link: *mut List);
    pub fn wl_global_create(
        display: *mut Display,
        interface: *const Interface,
        version: c_int,
        data: *mut c_void,
        bind: Option<Bind>,
    ) -> *mut c_void;
    pub fn wl_resource_create(
        client: *mut Client,
        interface: *const Interface,
        version: c_int,
        id: u32,
    ) -> *mut Resource;
    pub fn wl_resource_set_implementation(
        resource: *mut Resource,
        implementation: *const c_void,
        data: *mut c_void,
        destroy: Option<Destroy>,
    );
    pub fn wl_resource_get_version(resource: *mut Resource) -> c_int;
    pub fn wl_resource_destroy(resource: *mut Resource);
    pub fn wl_resource_post_event(resource: *mut Resource, opcode: u32, ...);
    pub fn wl_client_post_no_memory(client: *mut Client);
}
