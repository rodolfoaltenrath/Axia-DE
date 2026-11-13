//! Pointer drag ownership and wl_data_device v1–3. UI thread only.
//! Motion borrows the existing hit test: no allocation, file I/O or JS dispatch.
use super::{dnd::*, ffi::*};
use libc::{c_char, c_int, c_void};
use std::{
    cell::{RefCell, UnsafeCell},
    ffi::{CStr, CString},
    os::fd::{FromRawFd, OwnedFd},
    ptr,
    time::{Duration, Instant},
};

const COPY: u32 = 1;
const MOVE: u32 = 2;
const ASK: u32 = 4;
const ACTIONS: u32 = COPY | MOVE | ASK;
const URI: &CStr = c"text/uri-list";
const SESSION_LIMIT: usize = 8;
const OFFER_LIMIT: usize = 64;
type EventFn = unsafe extern "C" fn(c_int, *const c_char, *const c_char);

/// Opaque sidecar for the existing clipboard source. Rust owns drag actions and
/// the one-use rule; the C clipboard implementation only carries this handle.
pub struct SourceState {
    actions: u32,
    actions_set: bool,
    used: bool,
}
impl Default for SourceState {
    fn default() -> Self {
        Self {
            actions: COPY,
            actions_set: false,
            used: false,
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn dnd_source_free(state: *mut SourceState) {
    if !state.is_null() {
        drop(Box::from_raw(state));
    }
}
#[no_mangle]
pub unsafe extern "C" fn dnd_source_used(state: *const SourceState) -> i32 {
    i32::from(state.as_ref().is_some_and(|s| s.used))
}
#[no_mangle]
pub unsafe extern "C" fn dnd_source_is_drag(state: *const SourceState) -> i32 {
    i32::from(state.as_ref().is_some_and(|s| s.used || s.actions_set))
}
#[no_mangle]
pub unsafe extern "C" fn dnd_source_actions(
    resource: *mut Resource,
    handle: *mut *mut SourceState,
    actions: u32,
) {
    let Some(handle) = handle.as_mut() else {
        return;
    };
    if handle.as_ref().is_some_and(|s| s.used || s.actions_set) {
        wl_resource_post_error(resource, 1, c"drag source already used".as_ptr());
        return;
    }
    if actions & !ACTIONS != 0 {
        wl_resource_post_error(resource, 0, c"invalid drag actions".as_ptr());
        return;
    }
    if handle.is_null() {
        *handle = Box::into_raw(Box::new(SourceState::default()));
    }
    let state = &mut **handle;
    state.actions = actions;
    state.actions_set = true;
}
#[no_mangle]
pub unsafe extern "C" fn dnd_start_source(
    source: *mut Resource,
    origin: *mut Resource,
    mimes: *const *const c_char,
    count: i32,
    handle: *mut *mut SourceState,
    button: u32,
) -> i32 {
    if source.is_null() {
        return dnd_start(source, origin, mimes, count, COPY, button);
    }
    let Some(handle) = handle.as_mut() else {
        return 0;
    };
    if handle.as_ref().is_some_and(|s| s.used) {
        wl_resource_post_error(source, 1, c"drag source already used".as_ptr());
        return 0;
    }
    let actions = handle.as_ref().map_or(COPY, |s| s.actions);
    if dnd_start(source, origin, mimes, count, actions, button) == 0 {
        wl_resource_post_event(source, 2);
        return 0;
    }
    if handle.is_null() {
        *handle = Box::into_raw(Box::new(SourceState::default()));
    }
    (**handle).used = true;
    1
}

// Addresses stay stable inside their Boxes until listeners are detached.
#[repr(C)]
struct Watch {
    listener: Listener,
    session: u64,
    kind: u8,
    attached: bool,
}
impl Watch {
    fn new(session: u64, kind: u8) -> Self {
        Self {
            listener: Listener {
                link: UnsafeCell::new(List {
                    prev: ptr::null_mut(),
                    next: ptr::null_mut(),
                }),
                notify: Some(resource_gone),
            },
            session,
            kind,
            attached: false,
        }
    }
    unsafe fn attach(&mut self, resource: *mut Resource) {
        if !resource.is_null() {
            wl_resource_add_destroy_listener(resource, &mut self.listener);
            self.attached = true;
        }
    }
    unsafe fn detach(&mut self) {
        if self.attached {
            wl_list_remove(self.listener.link.get());
            self.attached = false;
        }
    }
}
struct Input {
    reader: Reader,
    event: *mut EventSource,
}
impl Drop for Input {
    fn drop(&mut self) {
        unsafe {
            wl_event_source_remove(self.event);
        }
    }
}
struct Session {
    id: u64,
    source: *mut Resource,
    origin: *mut Resource,
    surface: *mut Resource,
    device: *mut Resource,
    watches: [Watch; 4],
    payload: Option<Payload>,
    reader: Option<Input>,
    offer: *mut Resource,
    mimes: Vec<CString>,
    mask: u32,
    action: u32,
    hover_token: i32,
    token: i32,
    origin_node: i32,
    internal_move: bool,
    no_source: bool,
    deadline: Option<Instant>,
    x: f32,
    y: f32,
}
impl Session {
    fn new(id: u64) -> Box<Self> {
        Box::new(Self {
            id,
            source: ptr::null_mut(),
            origin: ptr::null_mut(),
            surface: ptr::null_mut(),
            device: ptr::null_mut(),
            watches: std::array::from_fn(|i| Watch::new(id, i as u8)),
            payload: None,
            reader: None,
            offer: ptr::null_mut(),
            mimes: Vec::new(),
            mask: COPY,
            action: 0,
            hover_token: 0,
            token: 0,
            origin_node: 0,
            internal_move: false,
            no_source: false,
            deadline: None,
            x: 0.,
            y: 0.,
        })
    }
    fn has_mime(&self, mime: &CStr) -> bool {
        self.mimes.iter().any(|m| m.as_c_str() == mime)
    }
    unsafe fn leave(&mut self) {
        if !self.device.is_null() && self.deadline.is_none() {
            wl_resource_post_event(self.device, 2);
        }
        self.watches[2].detach();
        self.watches[3].detach();
        self.surface = ptr::null_mut();
        self.device = ptr::null_mut();
        self.offer = ptr::null_mut();
        self.token = 0;
        self.hover_token = 0;
        if !self.source.is_null() && self.deadline.is_none() {
            wl_resource_post_event(self.source, 0, ptr::null::<c_char>());
            if self.action != 0 && wl_resource_get_version(self.source) >= 3 {
                wl_resource_post_event(self.source, 5, 0u32);
            }
        }
        self.action = 0;
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        for watch in &mut self.watches {
            unsafe {
                watch.detach();
            }
        }
    }
}
struct Offer {
    resource: *mut Resource,
    session: Option<u64>,
    mask: u32,
    preferred: u32,
    accepted: bool,
}
struct Output {
    id: u64,
    writer: Writer,
    event: *mut EventSource,
}
impl Drop for Output {
    fn drop(&mut self) {
        unsafe {
            wl_event_source_remove(self.event);
        }
    }
}
struct Event {
    drop: bool,
    json: CString,
    token:u32,
    internal:bool,
}
#[derive(Clone, Copy, Default)]
struct Press {
    serial: u32,
    origin: *mut Resource,
    client: *mut Client,
    button: u32,
}
struct State {
    display: *mut Display,
    event_loop: *mut EventLoop,
    devices: *mut List,
    timer: *mut EventSource,
    active: Option<u64>,
    sequence: u64,
    sessions: Vec<Box<Session>>,
    offers: Vec<Offer>,
    outputs: Vec<Output>,
    events: [Option<Event>; 16],
    held: u32,
    presses: [Press; 16],
    press_slot: usize,
    button: u32,
    escape_held: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            display: ptr::null_mut(),
            event_loop: ptr::null_mut(),
            devices: ptr::null_mut(),
            timer: ptr::null_mut(),
            active: None,
            sequence: 0,
            sessions: Vec::with_capacity(SESSION_LIMIT),
            offers: Vec::with_capacity(OFFER_LIMIT),
            outputs: Vec::with_capacity(8),
            events: std::array::from_fn(|_| None),
            held: 0,
            presses: [Press::default(); 16],
            press_slot: 0,
            button: 0,
            escape_held: false,
        }
    }
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
impl State {
    fn session(&self, id: u64) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id).map(Box::as_ref)
    }
    fn session_mut(&mut self, id: u64) -> Option<&mut Session> {
        self.sessions
            .iter_mut()
            .find(|s| s.id == id)
            .map(Box::as_mut)
    }
    fn next_id(&mut self) -> u64 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }
    unsafe fn arm(&self) {
        if !self.timer.is_null() {
            wl_event_source_timer_update(
                self.timer,
                if self.sessions.len() > usize::from(self.active.is_some())
                    || !self.outputs.is_empty()
                {
                    1000
                } else {
                    0
                },
            );
        }
    }
    fn queue(&mut self, token: i32, paths: Option<CString>, internal: bool) {
        let Some(slot) = self.events.iter_mut().find(|e| e.is_none()) else {
            return;
        };
        let drop = paths.is_some();
        if token as u32 & crate::native_view::DROP_TOKEN_FLAG!=0 {
            if let Some(paths)=paths {
                *slot=Some(Event {drop:true,json:paths,token:token as u32,internal});
            }
            return;
        }
        let paths = paths
            .as_ref()
            .map_or("null", |p| p.to_str().unwrap_or("null"));
        *slot = CString::new(format!(
            "{{\"token\":{token},\"native\":{internal},\"paths\":{paths}}}"
        ))
        .ok()
            .map(|json| Event { drop, json, token:token as u32, internal });
    }
    fn begin(&mut self) -> Option<u64> {
        if self.display.is_null()
            || self.timer.is_null()
            || self.active.is_some()
            || self.sessions.len() >= SESSION_LIMIT
        {
            return None;
        }
        let id = self.next_id();
        self.sessions.push(Session::new(id));
        self.active = Some(id);
        Some(id)
    }
    unsafe fn finish(&mut self, id: u64, success: bool) {
        let Some(index) = self.sessions.iter().position(|s| s.id == id) else {
            return;
        };
        let mut s = self.sessions.swap_remove(index);
        if self.active == Some(id) {
            self.active = None;
            if s.payload.is_some() {
                self.queue(0, None, false);
            }
        }
        s.leave();
        if !s.source.is_null() {
            wl_resource_post_event(
                s.source,
                if success && wl_resource_get_version(s.source) >= 3 {
                    4
                } else {
                    2
                },
            );
        }
        for offer in &mut self.offers {
            if offer.session == Some(id) {
                offer.session = None;
            }
        }
        drop(s);
        self.arm();
    }
    unsafe fn negotiate(&mut self, id: u64) {
        let Some(s) = self.session(id) else {
            return;
        };
        let Some(o) = self.offers.iter().find(|o| o.resource == s.offer) else {
            return;
        };
        let available = s.mask & o.mask;
        let action = if available & o.preferred != 0 {
            o.preferred
        } else if available & COPY != 0 {
            COPY
        } else if available & MOVE != 0 {
            MOVE
        } else if available & ASK != 0 {
            ASK
        } else {
            0
        };
        if action == s.action {
            return;
        }
        let s = self.session_mut(id).unwrap();
        s.action = action;
        if wl_resource_get_version(s.offer) >= 3 {
            wl_resource_post_event(s.offer, 2, action);
        }
        if !s.source.is_null() && wl_resource_get_version(s.source) >= 3 {
            wl_resource_post_event(s.source, 5, action);
        }
    }
    fn live(&self, resource: *mut Resource) -> Option<u64> {
        let id = self
            .offers
            .iter()
            .find(|o| o.resource == resource)?
            .session?;
        self.session(id).filter(|s| s.offer == resource).map(|_| id)
    }
    unsafe fn enter(&mut self, id: u64, surface: *mut Resource, token: i32, x: f32, y: f32) {
        let s = self.session_mut(id).unwrap();
        s.leave();
        s.x = x;
        s.y = y;
        s.hover_token = token;
        if surface.is_null() {
            // High bit marks a destructive native-only target (Files' Trash).
            if token < 0 && (s.payload.is_none() || !s.internal_move) {
                return;
            }
            let token = token & i32::MAX;
            if token != 0 && s.has_mime(URI) && s.mask & COPY != 0 {
                s.token = token;
                s.action = COPY;
                if !s.source.is_null() {
                    wl_resource_post_event(s.source, 0, URI.as_ptr());
                    if wl_resource_get_version(s.source) >= 3 {
                        wl_resource_post_event(s.source, 5, COPY);
                    }
                }
            }
            return;
        }
        // Cache even an unsupported surface so motion never repeats enter work.
        s.surface = surface;
        s.watches[2].attach(surface);
        let client = wl_resource_get_client(surface);
        if s.no_source && client != wl_resource_get_client(s.origin) {
            return;
        }
        let mut device = ptr::null_mut();
        let mut link = (*self.devices).next;
        while link != self.devices {
            let r = wl_resource_from_link(link);
            if wl_resource_get_client(r) == client {
                device = r;
                break;
            }
            link = (*link).next;
        }
        if device.is_null() || self.offers.len() >= OFFER_LIMIT {
            return;
        }
        let no_source = self.session(id).unwrap().no_source;
        if !no_source {
            let resource = wl_resource_create(
                client,
                &wl_data_offer_interface,
                wl_resource_get_version(device),
                0,
            );
            if resource.is_null() {
                wl_client_post_no_memory(client);
                return;
            }
            let offer = Offer {
                resource,
                session: Some(id),
                mask: if wl_resource_get_version(resource) < 3 {
                    COPY
                } else {
                    0
                },
                preferred: 0,
                accepted: false,
            };
            wl_resource_set_implementation(
                resource,
                (&OFFER_IMPL as *const OfferImpl).cast(),
                ptr::null_mut(),
                Some(offer_gone),
            );
            self.offers.push(offer);
            let s = self.session_mut(id).unwrap();
            s.offer = resource;
            wl_resource_post_event(device, 0, resource);
            for mime in &s.mimes {
                wl_resource_post_event(resource, 0, mime.as_ptr());
            }
            if wl_resource_get_version(resource) >= 3 {
                wl_resource_post_event(resource, 1, s.mask);
            }
        }
        let serial = wl_display_next_serial(self.display);
        let s = self.session_mut(id).unwrap();
        s.device = device;
        s.watches[3].attach(device);
        wl_resource_post_event(device, 1, serial, surface, fixed(x), fixed(y), s.offer);
        self.negotiate(id);
    }
    unsafe fn send_payload(&mut self, id: u64, fd: i32) {
        if self.outputs.len() >= 8 {
            libc::close(fd);
            return;
        }
        let payload = self.session(id).and_then(|s| s.payload.as_ref()).unwrap();
        let Some(writer) = Writer::new(OwnedFd::from_raw_fd(fd), payload) else {
            return;
        };
        let id = self.next_id();
        let event = wl_event_loop_add_fd(
            self.event_loop,
            fd,
            2,
            Some(output_ready),
            id as usize as *mut c_void,
        );
        if event.is_null() {
            return;
        }
        self.outputs.push(Output { id, writer, event });
        self.arm();
    }
    unsafe fn pointer(
        &mut self,
        surface: *mut Resource,
        token: i32,
        x: f32,
        y: f32,
        moved: bool,
        released: bool,
        time: u32,
    ) {
        let Some(id) = self.active else {
            return;
        };
        let s = self.session(id).unwrap();
        if surface != s.surface || (surface.is_null() && token != s.hover_token) {
            self.enter(id, surface, token, x, y);
        }
        let s = self.session_mut(id).unwrap();
        if moved && !s.device.is_null() && (x != s.x || y != s.y) {
            wl_resource_post_event(s.device, 3, time, fixed(x), fixed(y));
            s.x = x;
            s.y = y;
        }
        if !released {
            return;
        }
        let s = self.session(id).unwrap();
        let accepted = s.no_source
            || self
                .offers
                .iter()
                .any(|o| o.resource == s.offer && o.accepted);
        let s = self.session_mut(id).unwrap();
        if !s.device.is_null() && accepted && (s.no_source || s.action != 0) {
            wl_resource_post_event(s.device, 4);
            if !s.source.is_null() && wl_resource_get_version(s.source) >= 3 {
                wl_resource_post_event(s.source, 3);
            }
            s.deadline = Some(Instant::now() + Duration::from_secs(1800));
            let native = s.payload.is_some();
            let no_source = s.no_source;
            self.active = None;
            if native {
                self.queue(0, None, false);
            }
            if no_source {
                self.finish(id, true);
            } else {
                self.arm();
            }
            return;
        }
        if s.token == 0 || s.action == 0 {
            self.finish(id, false);
            return;
        }
        let token = s.token;
        if let Some(payload) = s.payload.as_ref() {
            let paths = payload.json();
            let internal = s.internal_move;
            self.finish(id, paths.is_some());
            if paths.is_some() {
                self.queue(token, paths, internal);
            }
            return;
        }
        let mut pipes = [-1; 2];
        if libc::pipe2(pipes.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            self.finish(id, false);
            return;
        }
        let Some(reader) = Reader::new(OwnedFd::from_raw_fd(pipes[0])) else {
            libc::close(pipes[1]);
            self.finish(id, false);
            return;
        };
        let event = wl_event_loop_add_fd(
            self.event_loop,
            pipes[0],
            1,
            Some(input_ready),
            id as usize as *mut c_void,
        );
        if event.is_null() {
            libc::close(pipes[1]);
            self.finish(id, false);
            return;
        }
        let s = self.session_mut(id).unwrap();
        s.reader = Some(Input { reader, event });
        wl_resource_post_event(s.source, 1, URI.as_ptr(), pipes[1]);
        libc::close(pipes[1]);
        if wl_resource_get_version(s.source) >= 3 {
            wl_resource_post_event(s.source, 3);
        }
        s.deadline = Some(Instant::now() + Duration::from_secs(1800));
        self.active = None;
        self.arm();
    }
}
fn fixed(value: f32) -> i32 {
    (f64::from(value) * 256.).round() as i32
}

unsafe extern "C" fn resource_gone(listener: *mut Listener, _: *mut c_void) {
    let watch = listener.cast::<Watch>();
    let id = (*watch).session;
    let kind = (*watch).kind;
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let active = state.active == Some(id);
        let Some(s) = state.session_mut(id) else {
            return;
        };
        s.watches[kind as usize].detach();
        match kind {
            0 => {
                s.source = ptr::null_mut();
                state.finish(id, false);
            }
            1 => {
                s.origin = ptr::null_mut();
                if active {
                    state.finish(id, false);
                }
            }
            _ => {
                if kind == 3 {
                    s.device = ptr::null_mut();
                }
                let dropped = s.deadline.is_some();
                s.leave();
                if dropped {
                    state.finish(id, false);
                }
            }
        }
    });
}
unsafe extern "C" fn offer_gone(resource: *mut Resource) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(index) = state.offers.iter().position(|o| o.resource == resource) {
            let offer = state.offers.swap_remove(index);
            if let Some(id) = offer.session {
                if let Some(s) = state.session_mut(id).filter(|s| s.offer == resource) {
                    s.offer = ptr::null_mut();
                    if s.deadline.is_some() {
                        state.finish(id, wl_resource_get_version(resource) < 3);
                    }
                }
            }
        }
    });
}
unsafe extern "C" fn offer_accept(_: *mut Client, r: *mut Resource, _: u32, mime: *const c_char) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(id) = state.live(r) else {
            return;
        };
        let s = state.session(id).unwrap();
        let accepted = !mime.is_null() && s.has_mime(CStr::from_ptr(mime));
        let source = s.source;
        state
            .offers
            .iter_mut()
            .find(|o| o.resource == r)
            .unwrap()
            .accepted = accepted;
        if !source.is_null() {
            wl_resource_post_event(source, 0, if accepted { mime } else { ptr::null() });
        }
    });
}
unsafe extern "C" fn offer_receive(_: *mut Client, r: *mut Resource, mime: *const c_char, fd: i32) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(id) = state.live(r) else {
            libc::close(fd);
            return;
        };
        let s = state.session(id).unwrap();
        if mime.is_null() || !s.has_mime(CStr::from_ptr(mime)) {
            libc::close(fd);
            return;
        }
        if !s.source.is_null() {
            wl_resource_post_event(s.source, 1, mime, fd);
            libc::close(fd);
        } else {
            state.send_payload(id, fd);
        }
    });
}
unsafe extern "C" fn offer_destroy(_: *mut Client, r: *mut Resource) {
    wl_resource_destroy(r);
}
unsafe extern "C" fn offer_finish(_: *mut Client, r: *mut Resource) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let valid = state.live(r).filter(|id| {
            let s = state.session(*id).unwrap();
            s.deadline.is_some()
                && s.action != 0
                && s.action != ASK
                && state.offers.iter().any(|o| o.resource == r && o.accepted)
        });
        if let Some(id) = valid {
            state.finish(id, true);
        } else {
            wl_resource_post_error(r, 0, c"invalid drag finish".as_ptr());
        }
    });
}
unsafe extern "C" fn offer_actions(_: *mut Client, r: *mut Resource, mask: u32, preferred: u32) {
    if mask & !ACTIONS != 0 {
        wl_resource_post_error(r, 1, c"invalid drag actions".as_ptr());
        return;
    }
    if preferred.count_ones() > 1 || preferred & !mask != 0 {
        wl_resource_post_error(r, 2, c"invalid preferred drag action".as_ptr());
        return;
    }
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(id) = state.live(r) else {
            return;
        };
        let s = state.session(id).unwrap();
        if s.deadline.is_some() && s.action != ASK {
            return;
        }
        let o = state.offers.iter_mut().find(|o| o.resource == r).unwrap();
        o.mask = mask;
        o.preferred = preferred;
        state.negotiate(id);
    });
}
#[repr(C)]
struct OfferImpl {
    accept: unsafe extern "C" fn(*mut Client, *mut Resource, u32, *const c_char),
    receive: unsafe extern "C" fn(*mut Client, *mut Resource, *const c_char, i32),
    destroy: unsafe extern "C" fn(*mut Client, *mut Resource),
    finish: unsafe extern "C" fn(*mut Client, *mut Resource),
    set_actions: unsafe extern "C" fn(*mut Client, *mut Resource, u32, u32),
}
static OFFER_IMPL: OfferImpl = OfferImpl {
    accept: offer_accept,
    receive: offer_receive,
    destroy: offer_destroy,
    finish: offer_finish,
    set_actions: offer_actions,
};
unsafe extern "C" fn input_ready(_: i32, _: u32, data: *mut c_void) -> i32 {
    let id = data as usize as u64;
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(s) = state.session_mut(id) else {
            return;
        };
        let reader = &mut s.reader.as_mut().unwrap().reader;
        let result = reader.poll();
        if result == Status::Pending {
            return;
        }
        let token = s.token;
        let paths = if result == Status::Complete {
            reader.json().map(CStr::to_owned)
        } else {
            None
        };
        state.finish(id, paths.is_some());
        if paths.is_some() {
            state.queue(token, paths, false);
        }
    });
    0
}
unsafe extern "C" fn output_ready(_: i32, _: u32, data: *mut c_void) -> i32 {
    let id = data as usize as u64;
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(index) = state.outputs.iter().position(|o| o.id == id) {
            if state.outputs[index].writer.poll() != Status::Pending {
                state.outputs.swap_remove(index);
                state.arm();
            }
        }
    });
    0
}
unsafe extern "C" fn expire(_: *mut c_void) -> i32 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let now = Instant::now();
        // Fixed-size scratch: expiration never allocates a collection.
        let mut expired = [0u64; SESSION_LIMIT];
        let mut count = 0;
        for s in &state.sessions {
            if s.deadline.is_some_and(|deadline| now >= deadline)
                || s.reader.as_ref().is_some_and(|r| r.reader.expired())
            {
                expired[count] = s.id;
                count += 1;
            }
        }
        for id in &expired[..count] {
            state.finish(*id, false);
        }
        state.outputs.retain(|o| !o.writer.expired());
        state.arm();
    });
    0
}

#[no_mangle]
pub unsafe extern "C" fn dnd_init(display: *mut Display, devices: *mut List) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.display = display;
        state.devices = devices;
        state.event_loop = wl_display_get_event_loop(display);
        state.timer = wl_event_loop_add_timer(state.event_loop, Some(expire), ptr::null_mut());
        state.held = 0;
        state.presses = [Press::default(); 16];
        state.escape_held = false;
    });
}
#[no_mangle]
pub unsafe extern "C" fn dnd_shutdown() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        while let Some(s) = state.sessions.first() {
            let id = s.id;
            state.finish(id, false);
        }
        state.outputs.clear();
        state.events = std::array::from_fn(|_| None);
        if !state.timer.is_null() {
            wl_event_source_remove(state.timer);
        }
        state.timer = ptr::null_mut();
        state.display = ptr::null_mut();
        state.active = None;
    });
}
#[no_mangle]
pub unsafe extern "C" fn dnd_start(
    source: *mut Resource,
    origin: *mut Resource,
    mimes: *const *const c_char,
    count: i32,
    actions: u32,
    button: u32,
) -> i32 {
    if origin.is_null() || count < 0 || (count > 0 && mimes.is_null()) || actions & !ACTIONS != 0 {
        return 0;
    }
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(id) = state.begin() else {
            return 0;
        };
        state.button = button;
        let s = state.session_mut(id).unwrap();
        s.source = source;
        s.origin = origin;
        s.no_source = source.is_null();
        s.mask = if s.no_source { COPY } else { actions };
        for i in 0..count.min(32) as usize {
            let mime = *mimes.add(i);
            if mime.is_null() {
                continue;
            }
            let mime = CStr::from_ptr(mime);
            if mime.to_bytes().len() < 128 {
                s.mimes.push(mime.to_owned());
            }
        }
        s.watches[0].attach(source);
        s.watches[1].attach(origin);
        1
    })
}
#[no_mangle]
pub unsafe extern "C" fn dnd_start_files(
    paths: *const c_char,
    origin_node: i32,
    internal_move: i32,
) -> i32 {
    if paths.is_null() {
        return 0;
    }
    if !STATE.with(|state| {
        let s = state.borrow();
        s.held & 1 != 0
            && s.active.is_none()
            && !s.display.is_null()
            && s.sessions.len() < SESSION_LIMIT
    }) {
        return 0;
    }
    let Some(payload) = Payload::new(CStr::from_ptr(paths)) else {
        return 0;
    };
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.held & 1 == 0 {
            return 0;
        }
        let Some(id) = state.begin() else {
            return 0;
        };
        state.button = 0;
        let s = state.session_mut(id).unwrap();
        s.payload = Some(payload);
        s.origin_node = origin_node;
        s.internal_move = internal_move != 0;
        s.mimes.push(URI.to_owned());
        1
    })
}
/// Native view teardown may run after this thread's protocol state was destroyed.
/// Cancel only the owning source, without invoking an extern-C entrypoint at exit.
pub(crate) fn cancel_native_origin(node:i32) {
    let _=STATE.try_with(|state| {
        let Ok(mut state)=state.try_borrow_mut() else {return;};
        if let Some(id)=state.active.filter(|id|state.session(*id).is_some_and(|s|s.origin_node==node)) {
            unsafe {state.finish(id,false);}
        }
    });
}
#[no_mangle]
pub extern "C" fn dnd_active() -> i32 {
    STATE.with(|s| i32::from(s.borrow().active.is_some()))
}
#[no_mangle]
pub extern "C" fn dnd_origin_node() -> i32 {
    STATE.with(|s| {
        let s = s.borrow();
        s.active
            .and_then(|id| s.session(id))
            .map_or(0, |s| s.origin_node)
    })
}
#[no_mangle]
pub unsafe extern "C" fn dnd_cancel() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(id) = state.active {
            state.finish(id, false);
        }
    });
}
#[no_mangle]
pub unsafe extern "C" fn dnd_key(code: u32, value: i32, alt: i32) -> i32 {
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        if let Some(id) = s.active {
            let session = s.session_mut(id).unwrap();
            if session.payload.is_some() && session.internal_move != (alt == 0) {
                session.internal_move = alt == 0;
                if session.hover_token < 0 {
                    session.hover_token = 0;
                }
            }
        }
        if code != 1 {
            return 0;
        } // KEY_ESC; consume its matching release too.
        if value == 0 && s.escape_held {
            s.escape_held = false;
            return 1;
        }
        let Some(id) = s.active else {
            return 0;
        };
        if value == 1 {
            s.escape_held = true;
            s.finish(id, false);
        }
        1
    })
}
#[no_mangle]
pub unsafe extern "C" fn dnd_pointer(
    surface: *mut Resource,
    token: i32,
    x: f32,
    y: f32,
    moved: i32,
    released: i32,
    time: u32,
) {
    STATE.with(|s| {
        s.borrow_mut()
            .pointer(surface, token, x, y, moved != 0, released != 0, time)
    });
}
#[no_mangle]
pub unsafe extern "C" fn dnd_events(callback: EventFn) {
    let events = STATE.with(|state| {
        let mut state = state.borrow_mut();
        std::array::from_fn::<_, 16, _>(|i| state.events[i].take())
    });
    for e in events.into_iter().flatten() {
        if e.drop && e.token & crate::native_view::DROP_TOKEN_FLAG!=0 {
            if let Ok(paths)=serde_json::from_slice::<serde_json::Value>(e.json.to_bytes()) {
                crate::native_view::deliver_file_drop(e.token,&paths,e.internal);
            }
            continue;
        }
        callback(
            0,
            if e.drop { c"filedrop" } else { c"dragend" }.as_ptr(),
            e.json.as_ptr(),
        );
    }
}
#[no_mangle]
pub extern "C" fn dnd_button(button: u32, down: i32) -> i32 {
    if button >= 32 {
        return 0;
    }
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        let bit = 1 << button;
        if down != 0 {
            s.held |= bit;
        } else {
            s.held &= !bit;
        }
        for press in &mut s.presses {
            if press.button == button {
                press.origin = ptr::null_mut();
            }
        }
        i32::from(down == 0 && s.active.is_some() && s.button == button)
    })
}
#[no_mangle]
pub unsafe extern "C" fn dnd_record_press(origin: *mut Resource, serial: u32, button: u32) {
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        let slot = s.press_slot % 16;
        s.press_slot = s.press_slot.wrapping_add(1);
        s.presses[slot] = Press {
            origin,
            serial,
            button,
            client: wl_resource_get_client(origin),
        };
    });
}
#[no_mangle]
pub extern "C" fn dnd_press_button(
    client: *mut Client,
    origin: *mut Resource,
    serial: u32,
    focus: *mut Resource,
) -> i32 {
    if origin.is_null() || origin != focus {
        return -1;
    }
    STATE.with(|state| {
        let s = state.borrow();
        s.presses
            .iter()
            .find(|p| {
                p.origin == origin
                    && p.client == client
                    && p.serial == serial
                    && s.held & (1 << p.button) != 0
            })
            .map_or(-1, |p| p.button as i32)
    })
}
#[no_mangle]
pub extern "C" fn dnd_surface_gone(surface: *mut Resource) {
    STATE.with(|state| {
        for p in &mut state.borrow_mut().presses {
            if p.origin == surface {
                p.origin = ptr::null_mut();
            }
        }
    });
}

#[cfg(test)]
mod tests;
