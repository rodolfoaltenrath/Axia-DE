//! Real libwayland resources and descriptors. No GPU, desktop or user files.
use super::*;
use std::os::{
    fd::{AsRawFd, IntoRawFd},
    unix::net::UnixStream,
};

#[repr(C)]
union Argument {
    integer: i32,
    pointer: *mut c_void,
}
#[repr(C)]
struct ProtocolMessage {
    resource: *mut Resource,
    opcode: i32,
    message: *const Message,
    count: i32,
    arguments: *const Argument,
}
#[link(name = "wayland-server")]
extern "C" {
    fn wl_display_create() -> *mut Display;
    fn wl_display_destroy(display: *mut Display);
    fn wl_display_destroy_clients(display: *mut Display);
    fn wl_display_flush_clients(display: *mut Display);
    fn wl_event_loop_dispatch(event_loop: *mut EventLoop, timeout: i32) -> i32;
    fn wl_resource_get_class(resource: *mut Resource) -> *const c_char;
    fn wl_resource_get_link(resource: *mut Resource) -> *mut List;
    fn wl_list_init(list: *mut List);
    fn wl_list_insert(list: *mut List, element: *mut List);
    fn wl_display_add_protocol_logger(
        display: *mut Display,
        callback: Option<unsafe extern "C" fn(*mut c_void, i32, *const ProtocolMessage)>,
        data: *mut c_void,
    ) -> *mut c_void;
    fn wl_protocol_logger_destroy(logger: *mut c_void);
    static wl_data_device_interface: Interface;
    static wl_data_source_interface: Interface;
}
#[derive(Default)]
struct Log {
    fd: Option<std::os::fd::OwnedFd>,
    motions: usize,
    source_events: Vec<u32>,
    native: Vec<serde_json::Value>,
}
thread_local! { static LOG:RefCell<Log>=RefCell::new(Log::default()); }
unsafe extern "C" fn protocol(_: *mut c_void, direction: i32, m: *const ProtocolMessage) {
    if direction != 1 {
        return;
    }
    let m = &*m;
    let interface = CStr::from_ptr(wl_resource_get_class(m.resource));
    LOG.with(|log| {
        let mut log = log.borrow_mut();
        if interface == c"wl_data_device" && m.opcode == 3 {
            log.motions += 1;
        }
        if interface == c"wl_data_source" {
            log.source_events.push(m.opcode as u32);
            if m.opcode == 1 {
                use std::os::fd::FromRawFd;
                let fd = libc::dup((*m.arguments.add(1)).integer);
                assert!(fd >= 0);
                assert!(log.fd.is_none());
                log.fd = Some(std::os::fd::OwnedFd::from_raw_fd(fd));
            }
        }
    });
}
unsafe extern "C" fn delivered(_: i32, _: *const c_char, json: *const c_char) {
    // Event callbacks run outside the protocol borrow and may reenter the host.
    let _ = dnd_active();
    LOG.with(|log| {
        log.borrow_mut()
            .native
            .push(serde_json::from_slice(CStr::from_ptr(json).to_bytes()).unwrap())
    });
}
unsafe extern "C" fn device_gone(resource: *mut Resource) {
    wl_list_remove(wl_resource_get_link(resource));
}
unsafe fn resource(
    client: *mut Client,
    interface: *const Interface,
    version: i32,
) -> *mut Resource {
    let resource = wl_resource_create(client, interface, version, 0);
    assert!(!resource.is_null());
    resource
}
struct Fixture {
    display: *mut Display,
    source: *mut Client,
    target: *mut Client,
    origin: *mut Resource,
    surface: *mut Resource,
    src: *mut Resource,
    devices: Box<List>,
    logger: *mut c_void,
    _connections: [UnixStream; 2],
}
impl Fixture {
    unsafe fn new() -> Self {
        LOG.with(|log| *log.borrow_mut() = Log::default());
        let display = wl_display_create();
        assert!(!display.is_null());
        let (a, b) = UnixStream::pair().unwrap();
        let (c, d) = UnixStream::pair().unwrap();
        let source = wl_client_create(display, a.into_raw_fd());
        let target = wl_client_create(display, c.into_raw_fd());
        assert!(!source.is_null() && !target.is_null());
        let mut devices = Box::new(List {
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
        });
        wl_list_init(&mut *devices);
        for client in [source, target] {
            let device = resource(client, &wl_data_device_interface, 3);
            wl_resource_set_implementation(device, ptr::null(), ptr::null_mut(), Some(device_gone));
            wl_list_insert(&mut *devices, wl_resource_get_link(device));
        }
        let origin = resource(source, &wl_surface_interface, 1);
        let surface = resource(target, &wl_surface_interface, 1);
        let src = resource(source, &wl_data_source_interface, 3);
        let logger = wl_display_add_protocol_logger(display, Some(protocol), ptr::null_mut());
        assert!(!logger.is_null());
        dnd_init(display, &mut *devices);
        dnd_button(0, 1);
        Self {
            display,
            source,
            target,
            origin,
            surface,
            src,
            devices,
            logger,
            _connections: [b, d],
        }
    }
    unsafe fn start(&self, actions: u32) {
        let mimes = [URI.as_ptr(), c"text/plain".as_ptr()];
        assert_eq!(
            dnd_start(self.src, self.origin, mimes.as_ptr(), 2, actions, 0),
            1
        );
    }
    unsafe fn enter(&self) -> *mut Resource {
        dnd_pointer(self.surface, 0, 4., 5., 1, 0, 1);
        let offer = STATE.with(|s| {
            let s = s.borrow();
            s.session(s.active.unwrap()).unwrap().offer
        });
        assert!(!offer.is_null());
        offer_actions(self.target, offer, COPY | MOVE, COPY);
        offer_accept(self.target, offer, 0, URI.as_ptr());
        offer
    }
    unsafe fn pump(&self) {
        wl_display_flush_clients(self.display);
        // Consume mock client traffic, including SCM_RIGHTS copies of send fds.
        let mut bytes = [0u8; 8192];
        for connection in &self._connections {
            while libc::recv(
                connection.as_raw_fd(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                libc::MSG_DONTWAIT,
            ) > 0
            {}
        }
        assert_eq!(
            wl_event_loop_dispatch(wl_display_get_event_loop(self.display), 0),
            0
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            dnd_shutdown();
            wl_protocol_logger_destroy(self.logger);
            wl_display_destroy_clients(self.display);
            wl_display_destroy(self.display);
            assert!(STATE.with(|s| s.borrow().offers.is_empty()));
            // Keep the intrusive device list allocated through client destruction.
            assert_eq!(
                self.devices.next,
                &*self.devices as *const List as *mut List
            );
        }
    }
}
fn sessions() -> usize {
    STATE.with(|s| s.borrow().sessions.len())
}
fn take_fd() -> std::os::fd::OwnedFd {
    LOG.with(|log| log.borrow_mut().fd.take().unwrap())
}

#[test]
fn native_motion_100000_events_keeps_storage_and_never_dispatches_js() {
    unsafe {
        let _fixture = Fixture::new();
        assert_eq!(
            dnd_start_files(c"[\"/tmp/um vídeo.mp4\"]".as_ptr(), 42, 1),
            1
        );
        dnd_pointer(ptr::null_mut(), 12, 0., 0., 1, 0, 1);
        let storage = STATE.with(|s| {
            let s = s.borrow();
            (
                &*s.sessions[0] as *const Session,
                s.sessions.as_ptr(),
                s.offers.as_ptr(),
                s.outputs.as_ptr(),
                s.events.iter().filter(|e| e.is_some()).count(),
            )
        });
        for i in 0..100000 {
            dnd_pointer(ptr::null_mut(), 12, (i % 100) as f32, 0., 1, 0, i);
        }
        STATE.with(|s| {
            let s = s.borrow();
            assert_eq!(
                storage,
                (
                    &*s.sessions[0] as *const Session,
                    s.sessions.as_ptr(),
                    s.offers.as_ptr(),
                    s.outputs.as_ptr(),
                    s.events.iter().filter(|e| e.is_some()).count()
                )
            );
            assert!(s.offers.is_empty() && s.outputs.is_empty() && s.sessions.len() == 1);
        });
        LOG.with(|log| assert!(log.borrow().motions == 0 && log.borrow().native.is_empty()));
        dnd_pointer(ptr::null_mut(), 12, 0., 0., 0, 1, 2);
        assert_eq!(sessions(), 0);
        dnd_events(delivered);
        LOG.with(|log| {
            let log = log.borrow();
            assert_eq!(log.native.len(), 2);
            assert_eq!(
                log.native[1],
                serde_json::json!({"token":12,"native":true,"paths":["/tmp/um vídeo.mp4"]})
            );
        });
    }
}
#[test]
fn native_to_wayland_stream_outlives_drag_and_shares_encoded_payload() {
    unsafe {
        let f = Fixture::new();
        assert_eq!(
            dnd_start_files(c"[\"/tmp/um vídeo.mp4\"]".as_ptr(), 42, 0),
            1
        );
        let offer = f.enter();
        let (mut read, write) = UnixStream::pair().unwrap();
        offer_receive(f.target, offer, URI.as_ptr(), write.into_raw_fd());
        dnd_pointer(f.surface, 0, 4., 5., 0, 1, 3);
        assert_eq!(sessions(), 1);
        assert_eq!(dnd_active(), 0);
        offer_finish(f.target, offer);
        assert_eq!(sessions(), 0);
        f.pump();
        use std::io::Read;
        let mut data = String::new();
        read.read_to_string(&mut data).unwrap();
        assert_eq!(data, "file:///tmp/um%20v%C3%ADdeo.mp4\r\n");
        wl_resource_destroy(offer);
    }
}
#[test]
fn external_drop_decodes_at_eof_even_if_origin_closes_after_drop() {
    unsafe {
        let f = Fixture::new();
        f.start(COPY | MOVE);
        dnd_pointer(ptr::null_mut(), 17, 0., 0., 1, 1, 1);
        assert_eq!(dnd_active(), 0);
        assert_eq!(sessions(), 1);
        wl_resource_destroy(f.origin);
        use std::io::Write;
        let mut file = std::fs::File::from(take_fd());
        file.write_all(b"file:///tmp/movie.mp4\r\n").unwrap();
        drop(file);
        f.pump();
        assert_eq!(sessions(), 0);
        dnd_events(delivered);
        LOG.with(|log| {
            let log = log.borrow();
            assert_eq!(log.native.len(), 1);
            assert_eq!(
                log.native[0],
                serde_json::json!({"token":17,"native":false,"paths":["/tmp/movie.mp4"]})
            );
            assert!(log.source_events.contains(&4));
        });
    }
}
#[test]
fn stale_offer_destruction_does_not_cancel_new_drop_and_forwarding_is_direct() {
    unsafe {
        let f = Fixture::new();
        f.start(COPY | MOVE);
        let stale = f.enter();
        dnd_pointer(ptr::null_mut(), 0, 0., 0., 1, 0, 2);
        let offer = f.enter();
        let (mut read, write) = UnixStream::pair().unwrap();
        offer_receive(f.target, offer, c"text/plain".as_ptr(), write.into_raw_fd());
        use std::io::{Read, Write};
        let mut file = std::fs::File::from(take_fd());
        file.write_all(b"direct descriptor").unwrap();
        drop(file);
        f.pump();
        let mut data = String::new();
        read.read_to_string(&mut data).unwrap();
        assert_eq!(data, "direct descriptor");
        dnd_pointer(f.surface, 0, 4., 5., 0, 1, 3);
        wl_resource_destroy(stale);
        assert_eq!(sessions(), 1);
        offer_finish(f.target, offer);
        assert_eq!(sessions(), 0);
        wl_resource_destroy(offer);
    }
}
#[test]
fn closing_target_or_source_and_unoffered_mime_are_safe() {
    unsafe {
        let f = Fixture::new();
        f.start(COPY);
        let offer = f.enter();
        let (read, write) = UnixStream::pair().unwrap();
        let fd = write.into_raw_fd();
        offer_receive(f.target, offer, c"unoffered/type".as_ptr(), fd);
        let mut byte = 0u8;
        assert_eq!(
            libc::recv(
                read.as_raw_fd(),
                (&mut byte as *mut u8).cast(),
                1,
                libc::MSG_DONTWAIT
            ),
            0
        );
        drop(read);
        dnd_pointer(f.surface, 0, 4., 5., 0, 1, 2);
        wl_resource_destroy(f.surface);
        assert_eq!(sessions(), 0);
        wl_resource_destroy(offer);
        f.start(COPY);
        wl_resource_destroy(f.src);
        assert_eq!(dnd_active(), 0);
        assert_eq!(sessions(), 0);
    }
}
#[test]
fn pointer_serial_must_belong_to_live_held_press_and_origin_client() {
    unsafe {
        let f = Fixture::new();
        assert_eq!(dnd_press_button(f.source, f.origin, 12, f.origin), -1);
        dnd_record_press(f.origin, 12, 0);
        assert_eq!(dnd_press_button(f.source, f.origin, 12, f.origin), 0);
        assert_eq!(dnd_press_button(f.target, f.origin, 12, f.origin), -1);
        assert_eq!(dnd_press_button(f.source, f.origin, 12, f.surface), -1);
        dnd_button(0, 0);
        assert_eq!(dnd_press_button(f.source, f.origin, 12, f.origin), -1);
        dnd_button(0, 1);
        assert_eq!(dnd_press_button(f.source, f.origin, 12, f.origin), -1);
        dnd_record_press(f.origin, 13, 0);
        dnd_surface_gone(f.origin);
        assert_eq!(dnd_press_button(f.source, f.origin, 13, f.origin), -1);
        assert_eq!(dnd_start_files(c"[\"relative\"]".as_ptr(), 42, 0), 0);
    }
}
#[test]
fn stalled_payloads_and_pending_protocol_sessions_expire_without_idle_polling() {
    unsafe {
        let f = Fixture::new();
        f.start(COPY);
        let offer = f.enter();
        dnd_pointer(f.surface, 0, 4., 5., 0, 1, 2);
        STATE.with(|s| {
            s.borrow_mut().sessions[0].deadline = Some(Instant::now() - Duration::from_secs(1))
        });
        expire(ptr::null_mut());
        assert_eq!(sessions(), 0);
        wl_resource_destroy(offer);
        // An idle timer is disarmed: dispatching does not manufacture app events.
        f.pump();
        dnd_events(delivered);
        LOG.with(|log| assert!(log.borrow().native.is_empty()));
    }
}
#[test]
fn source_actions_and_one_use_rule_are_owned_by_rust() {
    unsafe {
        let f = Fixture::new();
        let mut handle = ptr::null_mut();
        assert_eq!(dnd_source_used(handle), 0);
        assert_eq!(dnd_source_is_drag(handle), 0);
        dnd_source_actions(f.src, &mut handle, MOVE);
        assert!(!handle.is_null());
        assert_eq!(dnd_source_is_drag(handle), 1);
        assert_eq!(dnd_source_used(handle), 0);
        let mimes = [URI.as_ptr()];
        assert_eq!(
            dnd_start_source(f.src, f.origin, mimes.as_ptr(), 1, &mut handle, 0),
            1
        );
        assert_eq!(dnd_source_used(handle), 1);
        dnd_pointer(ptr::null_mut(), 17, 0., 0., 1, 1, 2); // Native targets accept COPY only.
        assert_eq!(sessions(), 0);
        assert!(LOG.with(|log| log.borrow().fd.is_none()));
        dnd_source_free(handle);
    }
}
#[test]
fn native_destructive_targets_and_escape_do_not_leak_events_or_delete_external_files() {
    unsafe {
        let f = Fixture::new();
        f.start(COPY | MOVE);
        dnd_pointer(
            ptr::null_mut(),
            (0x80000000u32 | 17) as i32,
            0.,
            0.,
            1,
            1,
            2,
        );
        assert_eq!(sessions(), 0);
        assert!(LOG.with(|log| log.borrow().fd.is_none()));
        assert_eq!(dnd_start_files(c"[\"/tmp/file.mp4\"]".as_ptr(), 42, 1), 1);
        assert_eq!(dnd_key(1, 1, 0), 1);
        assert_eq!(dnd_active(), 0);
        assert_eq!(dnd_key(1, 0, 0), 1);
        assert_eq!(dnd_key(1, 0, 0), 0);
    }
}
#[test]
fn alt_can_change_native_move_to_copy_after_drag_has_started() {
    unsafe {
        let _f = Fixture::new();
        assert_eq!(dnd_start_files(c"[\"/tmp/file.mp4\"]".as_ptr(), 42, 1), 1);
        let trash = (0x80000000u32 | 17) as i32;
        dnd_pointer(ptr::null_mut(), trash, 0., 0., 1, 0, 1);
        assert_eq!(dnd_key(56, 1, 1), 0);
        dnd_pointer(ptr::null_mut(), trash, 0., 0., 0, 1, 2);
        dnd_events(delivered);
        LOG.with(|log| assert!(log.borrow().native.iter().all(|e| e["paths"].is_null())));
        assert_eq!(dnd_start_files(c"[\"/tmp/file.mp4\"]".as_ptr(), 42, 1), 1);
        dnd_key(56, 1, 1);
        dnd_pointer(ptr::null_mut(), 19, 0., 0., 0, 1, 3);
        dnd_events(delivered);
        LOG.with(|log| assert_eq!(log.borrow().native.last().unwrap()["native"], false));
    }
}
