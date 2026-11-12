//! zwp_text_input_v3 v1: focus and committed caret geometry for the on-screen keyboard.
//! Text continues through wl_keyboard. Surrounding text is deliberately never retained.
use super::ffi::*;
use libc::{c_char, c_void};
use std::{cell::RefCell, ptr};

type Report = Option<unsafe extern "C" fn(i32, i32, i32, i32, i32)>;

// Wire signatures and opcodes of the advertised v1 protocol. Defining descriptors here
// removes the generated C protocol dependency as well as the C implementation.
static EMPTY_TYPES: Types<4> = Types([ptr::null(); 4]);
static SURFACE_TYPES: Types<1> = Types([&raw const wl_surface_interface]);
static GET_TYPES: Types<2> = Types([&TEXT_INPUT, &raw const wl_seat_interface]);
static REQUESTS: [Message; 8] = [
    Message::new(b"destroy\0", b"\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(b"enable\0", b"\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(b"disable\0", b"\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(b"set_surrounding_text\0", b"sii\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(b"set_text_change_cause\0", b"u\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(b"set_content_type\0", b"uu\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(b"set_cursor_rectangle\0", b"iiii\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(b"commit\0", b"\0", EMPTY_TYPES.0.as_ptr()),
];
static EVENTS: [Message; 6] = [
    Message::new(b"enter\0", b"o\0", SURFACE_TYPES.0.as_ptr()),
    Message::new(b"leave\0", b"o\0", SURFACE_TYPES.0.as_ptr()),
    Message::new(b"preedit_string\0", b"?sii\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(b"commit_string\0", b"?s\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(
        b"delete_surrounding_text\0",
        b"uu\0",
        EMPTY_TYPES.0.as_ptr(),
    ),
    Message::new(b"done\0", b"u\0", EMPTY_TYPES.0.as_ptr()),
];
static TEXT_INPUT: Interface = Interface {
    name: b"zwp_text_input_v3\0".as_ptr().cast(),
    version: 1,
    method_count: 8,
    methods: REQUESTS.as_ptr(),
    event_count: 6,
    events: EVENTS.as_ptr(),
};
static MANAGER_REQUESTS: [Message; 2] = [
    Message::new(b"destroy\0", b"\0", EMPTY_TYPES.0.as_ptr()),
    Message::new(b"get_text_input\0", b"no\0", GET_TYPES.0.as_ptr()),
];
static MANAGER: Interface = Interface {
    name: b"zwp_text_input_manager_v3\0".as_ptr().cast(),
    version: 1,
    method_count: 2,
    methods: MANAGER_REQUESTS.as_ptr(),
    event_count: 0,
    events: ptr::null(),
};

#[derive(Default)]
struct Input {
    resource: *mut Resource,
    client: *mut Client,
    entered: bool,
    want: bool,
    active: bool,
    rect: [i32; 4],
    pending: [i32; 4],
}

impl Input {
    fn enable(&mut self) {
        self.want = true;
        self.pending = [0; 4];
    }

    fn commit(&mut self) -> Option<(bool, [i32; 4])> {
        let was = self.active;
        // Preserve the existing policy: a width-only change does not trigger a report.
        let moved = [0, 1, 3].iter().any(|&i| self.rect[i] != self.pending[i]);
        self.active = self.want;
        self.rect = self.pending;
        (self.entered && (self.active != was || (self.active && moved)))
            .then_some((self.active, self.rect))
    }
}

#[derive(Default)]
struct State {
    // Most recently created first, matching the previous wl_list insertion order.
    inputs: Vec<Input>,
    client: *mut Client,
    surface: *mut Resource,
    report: Report,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

fn modify(resource: *mut Resource, f: impl FnOnce(&mut Input)) {
    STATE.with(|s| {
        if let Some(input) = s
            .borrow_mut()
            .inputs
            .iter_mut()
            .find(|i| i.resource == resource)
        {
            f(input);
        }
    });
}

unsafe fn notify(report: Report, on: bool, rect: [i32; 4]) {
    if let Some(report) = report {
        report(on.into(), rect[0], rect[1], rect[2], rect[3]);
    }
}

unsafe extern "C" fn destroy(_: *mut Client, resource: *mut Resource) {
    wl_resource_destroy(resource);
}
unsafe extern "C" fn enable(_: *mut Client, resource: *mut Resource) {
    modify(resource, Input::enable);
}
unsafe extern "C" fn disable(_: *mut Client, resource: *mut Resource) {
    modify(resource, |i| i.want = false);
}
unsafe extern "C" fn surrounding(
    _: *mut Client,
    _: *mut Resource,
    _: *const c_char,
    _: i32,
    _: i32,
) {
}
unsafe extern "C" fn cause(_: *mut Client, _: *mut Resource, _: u32) {}
unsafe extern "C" fn content(_: *mut Client, _: *mut Resource, _: u32, _: u32) {}
unsafe extern "C" fn cursor(
    _: *mut Client,
    resource: *mut Resource,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    modify(resource, |i| i.pending = [x, y, w, h]);
}
unsafe extern "C" fn commit(_: *mut Client, resource: *mut Resource) {
    let (report, change) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let change = s
            .inputs
            .iter_mut()
            .find(|i| i.resource == resource)
            .and_then(Input::commit);
        (s.report, change)
    });
    if let Some((on, rect)) = change {
        notify(report, on, rect);
    }
}

#[repr(C)]
struct InputImpl {
    destroy: unsafe extern "C" fn(*mut Client, *mut Resource),
    enable: unsafe extern "C" fn(*mut Client, *mut Resource),
    disable: unsafe extern "C" fn(*mut Client, *mut Resource),
    surrounding: unsafe extern "C" fn(*mut Client, *mut Resource, *const c_char, i32, i32),
    cause: unsafe extern "C" fn(*mut Client, *mut Resource, u32),
    content: unsafe extern "C" fn(*mut Client, *mut Resource, u32, u32),
    cursor: unsafe extern "C" fn(*mut Client, *mut Resource, i32, i32, i32, i32),
    commit: unsafe extern "C" fn(*mut Client, *mut Resource),
}
static INPUT_IMPL: InputImpl = InputImpl {
    destroy,
    enable,
    disable,
    surrounding,
    cause,
    content,
    cursor,
    commit,
};

unsafe extern "C" fn gone(resource: *mut Resource) {
    let (report, removed) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let removed = s
            .inputs
            .iter()
            .position(|i| i.resource == resource)
            .map(|p| s.inputs.remove(p));
        (s.report, removed)
    });
    if let Some(i) = removed {
        if i.entered && i.active {
            notify(report, false, i.rect);
        }
    }
}

unsafe extern "C" fn get(client: *mut Client, manager: *mut Resource, id: u32, _: *mut Resource) {
    let resource = wl_resource_create(client, &TEXT_INPUT, wl_resource_get_version(manager), id);
    if resource.is_null() {
        wl_client_post_no_memory(client);
        return;
    }
    let allocation = STATE.with(|s| s.borrow_mut().inputs.try_reserve(1));
    if allocation.is_err() {
        wl_resource_destroy(resource);
        wl_client_post_no_memory(client);
        return;
    }
    wl_resource_set_implementation(
        resource,
        (&INPUT_IMPL as *const InputImpl).cast(),
        ptr::null_mut(),
        Some(gone),
    );
    let surface = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let entered = s.client == client;
        s.inputs.insert(
            0,
            Input {
                resource,
                client,
                entered,
                ..Input::default()
            },
        );
        entered.then_some(s.surface)
    });
    if let Some(surface) = surface {
        wl_resource_post_event(resource, 0, surface);
    }
}

#[repr(C)]
struct ManagerImpl {
    destroy: unsafe extern "C" fn(*mut Client, *mut Resource),
    get: unsafe extern "C" fn(*mut Client, *mut Resource, u32, *mut Resource),
}
static MANAGER_IMPL: ManagerImpl = ManagerImpl { destroy, get };

unsafe extern "C" fn bind(client: *mut Client, _: *mut c_void, version: u32, id: u32) {
    let resource = wl_resource_create(client, &MANAGER, version as i32, id);
    if !resource.is_null() {
        wl_resource_set_implementation(
            resource,
            (&MANAGER_IMPL as *const ManagerImpl).cast(),
            ptr::null_mut(),
            None,
        );
    }
}

/// Switch keyboard focus. Call only on the Wayland thread, without recursively
/// switching/destroying protocol resources from the report callback.
#[no_mangle]
pub unsafe extern "C" fn wl_textinput_focus(client: *mut Client, surface: *mut Resource) {
    // Release the state borrow before callbacks or libwayland event emission.
    let (report, previous_surface, leaves, enters) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let report = s.report;
        let previous_surface = s.surface;
        let mut leaves = Vec::new();
        let mut enters = Vec::new();
        for i in &mut s.inputs {
            if i.entered {
                leaves.push((i.resource, i.active, i.rect));
                i.entered = false;
                i.active = false;
                i.want = false;
            }
        }
        s.client = client;
        s.surface = surface;
        if !client.is_null() {
            for i in &mut s.inputs {
                if i.client == client {
                    i.entered = true;
                    enters.push(i.resource);
                }
            }
        }
        (report, previous_surface, leaves, enters)
    });
    for (resource, active, rect) in leaves {
        if active {
            notify(report, false, rect);
        }
        if !previous_surface.is_null() {
            wl_resource_post_event(resource, 1, previous_surface);
        }
    }
    for resource in enters {
        wl_resource_post_event(resource, 0, surface);
    }
}

/// The focused surface is being destroyed; preserve the existing silent reset.
#[no_mangle]
pub extern "C" fn wl_textinput_gone() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        for i in &mut s.inputs {
            i.entered = false;
            i.active = false;
            i.want = false;
        }
        s.client = ptr::null_mut();
        s.surface = ptr::null_mut();
    });
}

/// Initialize once per display lifetime, on its dispatch thread. Resources from
/// an earlier display must have been destroyed before initializing another.
#[no_mangle]
pub unsafe extern "C" fn wl_textinput_globals(display: *mut Display, report: Report) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        debug_assert!(s.inputs.is_empty());
        *s = State {
            report,
            ..State::default()
        };
    });
    wl_global_create(display, &MANAGER, 1, ptr::null_mut(), Some(bind));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_commit_and_caret_policy() {
        let mut i = Input {
            entered: true,
            ..Input::default()
        };
        i.pending = [3, 4, 5, 6];
        i.enable();
        assert_eq!(i.pending, [0; 4]);
        assert!(!i.active);
        assert_eq!(i.commit(), Some((true, [0; 4])));
        assert_eq!(i.commit(), None);
        i.pending = [10, -20, 30, 40];
        assert_eq!(i.commit(), Some((true, i.pending)));
        i.pending[2] = 70;
        assert_eq!(i.commit(), None);
        assert_eq!(i.rect[2], 70);
        i.want = false;
        assert_eq!(i.commit(), Some((false, i.pending)));
        i.pending[0] = -900;
        assert_eq!(i.commit(), None);
        i.entered = false;
        i.enable();
        assert_eq!(i.commit(), None);
        assert!(i.active);
    }
}
