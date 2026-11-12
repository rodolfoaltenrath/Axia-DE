//! v3 pointer-gestures wire metadata, also exported to the remaining native client code.
#![allow(non_upper_case_globals)]
use super::ffi::*;
use std::ptr;

static EMPTY: Types<5> = Types([ptr::null(); 5]);
static BEGIN: Types<4> = Types([
    ptr::null(),
    ptr::null(),
    &raw const wl_surface_interface,
    ptr::null(),
]);
static SWIPE_GET: Types<2> = Types([
    &zwp_pointer_gesture_swipe_v1_interface,
    &raw const wl_pointer_interface,
]);
static PINCH_GET: Types<2> = Types([
    &zwp_pointer_gesture_pinch_v1_interface,
    &raw const wl_pointer_interface,
]);
static HOLD_GET: Types<2> = Types([
    &zwp_pointer_gesture_hold_v1_interface,
    &raw const wl_pointer_interface,
]);
static REQUESTS: [Message; 4] = [
    Message::new(b"get_swipe_gesture\0", b"no\0", SWIPE_GET.0.as_ptr()),
    Message::new(b"get_pinch_gesture\0", b"no\0", PINCH_GET.0.as_ptr()),
    Message::new(b"release\0", b"2\0", EMPTY.0.as_ptr()),
    Message::new(b"get_hold_gesture\0", b"3no\0", HOLD_GET.0.as_ptr()),
];
static DESTROY: [Message; 1] = [Message::new(b"destroy\0", b"\0", EMPTY.0.as_ptr())];
static HOLD_DESTROY: [Message; 1] = [Message::new(b"destroy\0", b"3\0", EMPTY.0.as_ptr())];
static SWIPE_EVENTS: [Message; 3] = [
    Message::new(b"begin\0", b"uuou\0", BEGIN.0.as_ptr()),
    Message::new(b"update\0", b"uff\0", EMPTY.0.as_ptr()),
    Message::new(b"end\0", b"uui\0", EMPTY.0.as_ptr()),
];
static PINCH_EVENTS: [Message; 3] = [
    Message::new(b"begin\0", b"uuou\0", BEGIN.0.as_ptr()),
    Message::new(b"update\0", b"uffff\0", EMPTY.0.as_ptr()),
    Message::new(b"end\0", b"uui\0", EMPTY.0.as_ptr()),
];
static HOLD_EVENTS: [Message; 2] = [
    Message::new(b"begin\0", b"3uuou\0", BEGIN.0.as_ptr()),
    Message::new(b"end\0", b"3uui\0", EMPTY.0.as_ptr()),
];

#[no_mangle]
pub static zwp_pointer_gestures_v1_interface: Interface = Interface {
    name: b"zwp_pointer_gestures_v1\0".as_ptr().cast(),
    version: 3,
    method_count: 4,
    methods: REQUESTS.as_ptr(),
    event_count: 0,
    events: ptr::null(),
};
#[no_mangle]
pub static zwp_pointer_gesture_swipe_v1_interface: Interface = Interface {
    name: b"zwp_pointer_gesture_swipe_v1\0".as_ptr().cast(),
    version: 3,
    method_count: 1,
    methods: DESTROY.as_ptr(),
    event_count: 3,
    events: SWIPE_EVENTS.as_ptr(),
};
#[no_mangle]
pub static zwp_pointer_gesture_pinch_v1_interface: Interface = Interface {
    name: b"zwp_pointer_gesture_pinch_v1\0".as_ptr().cast(),
    version: 3,
    method_count: 1,
    methods: DESTROY.as_ptr(),
    event_count: 3,
    events: PINCH_EVENTS.as_ptr(),
};
#[no_mangle]
pub static zwp_pointer_gesture_hold_v1_interface: Interface = Interface {
    name: b"zwp_pointer_gesture_hold_v1\0".as_ptr().cast(),
    version: 3,
    method_count: 1,
    methods: HOLD_DESTROY.as_ptr(),
    event_count: 2,
    events: HOLD_EVENTS.as_ptr(),
};
