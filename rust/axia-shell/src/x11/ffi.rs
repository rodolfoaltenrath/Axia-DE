//! Transitional C ABI. The request buffer is opaque and Rust-owned; callers may
//! read its byte view only until the next mutation, and must not copy its handle.
use super::wire::*;
use libc::{c_char, c_int, c_void};
use std::{
    alloc::{alloc, Layout},
    ffi::CStr,
    ptr, slice,
};

#[repr(C)]
pub struct XOut {
    state: *mut Output,
    allocation_failed: c_int,
}

unsafe fn output<'a>(o: *mut XOut) -> Option<&'a mut Output> {
    let o = &mut *o;
    if o.allocation_failed != 0 {
        return None;
    }
    if o.state.is_null() {
        let state = alloc(Layout::new::<Output>()).cast::<Output>();
        if state.is_null() {
            o.allocation_failed = 1;
            return None;
        }
        state.write(Output::default());
        o.state = state;
    }
    Some(&mut *o.state)
}
unsafe fn input<'a, T>(p: *const T, n: usize) -> &'a [T] {
    if p.is_null() || n == 0 {
        &[]
    } else {
        slice::from_raw_parts(p, n)
    }
}
unsafe fn destination<'a>(p: *mut c_char, n: usize) -> &'a mut [u8] {
    if n == 0 {
        &mut []
    } else {
        slice::from_raw_parts_mut(p.cast(), n)
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_begin(o: *mut XOut, opcode: u8, data: u8) {
    if let Some(o) = output(o) {
        o.begin(opcode, data);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_u8(o: *mut XOut, v: u8) {
    if let Some(o) = output(o) {
        o.append(&[v]);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_u16(o: *mut XOut, v: u16) {
    if let Some(o) = output(o) {
        o.append(&v.to_le_bytes());
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_u32(o: *mut XOut, v: u32) {
    if let Some(o) = output(o) {
        o.append(&v.to_le_bytes());
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_pad(o: *mut XOut, n: usize) {
    if let Some(o) = output(o) {
        o.pad(n);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_bytes(o: *mut XOut, p: *const c_void, n: usize) {
    if let Some(o) = output(o) {
        let Some(more) = n.checked_add(3) else {
            o.failed = true;
            return;
        };
        // Do not construct a source slice if the bounded allocation already fails.
        if o.room(more) {
            o.bytes(input(p.cast(), n));
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_end(o: *mut XOut) {
    if let Some(o) = output(o) {
        o.end();
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_drop(o: *mut XOut, n: usize) {
    if !(*o).state.is_null() {
        (*(*o).state).drop_prefix(n);
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_free(o: *mut XOut) {
    if !(*o).state.is_null() {
        drop(Box::from_raw((*o).state));
    }
    o.write(XOut {
        state: ptr::null_mut(),
        allocation_failed: 0,
    });
}
#[no_mangle]
pub unsafe extern "C" fn xo_len(o: *const XOut) -> usize {
    if (*o).state.is_null() {
        0
    } else {
        (*(*o).state).bytes.len()
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_data(o: *const XOut) -> *const u8 {
    if (*o).state.is_null() {
        ptr::null()
    } else {
        (*(*o).state).bytes.as_ptr()
    }
}
#[no_mangle]
pub unsafe extern "C" fn xo_failed(o: *const XOut) -> c_int {
    i32::from((*o).allocation_failed != 0 || (!(*o).state.is_null() && (*(*o).state).failed))
}
#[no_mangle]
pub unsafe extern "C" fn xo_fail(o: *mut XOut) {
    (*o).allocation_failed = 1;
}

#[no_mangle]
pub unsafe extern "C" fn x11_setup_request(out: *mut u8) {
    ptr::copy_nonoverlapping(setup_request().as_ptr(), out, 12);
}
#[no_mangle]
pub unsafe extern "C" fn x11_parse_setup(
    p: *const u8,
    n: usize,
    out: *mut Setup,
    used: *mut usize,
) -> c_int {
    let parsed = parse_setup(input(p, n));
    if let Some(n) = parsed.used {
        used.write(n);
    }
    if let Some(s) = parsed.setup {
        // The historical ABI zeros padding too, including on malformed accepted headers.
        ptr::write_bytes(out.cast::<u8>(), 0, std::mem::size_of::<Setup>());
        (*out).id_base = s.id_base;
        (*out).id_mask = s.id_mask;
        (*out).root = s.root;
        (*out).root_visual = s.root_visual;
        (*out).max_req_bytes = s.max_req_bytes;
        (*out).width = s.width;
        (*out).height = s.height;
        (*out).root_depth = s.root_depth;
        (*out).min_keycode = s.min_keycode;
        (*out).max_keycode = s.max_keycode;
    }
    parsed.status
}
#[no_mangle]
pub unsafe extern "C" fn x11_packet_len(p: *const u8, n: usize) -> usize {
    packet_len(input(p, n))
}
#[no_mangle]
pub unsafe extern "C" fn x11_get16(p: *const u8) -> u16 {
    get16(input(p, 2))
}
#[no_mangle]
pub unsafe extern "C" fn x11_get32(p: *const u8) -> u32 {
    get32(input(p, 4))
}
#[no_mangle]
pub extern "C" fn x11_same_client(a: u32, b: u32, mask: u32) -> c_int {
    i32::from(a & !mask == b & !mask)
}
#[no_mangle]
pub extern "C" fn x11_seq_widen(last: u32, low: u16) -> u32 {
    seq_widen(last, low)
}
#[no_mangle]
pub unsafe extern "C" fn x11_wm_class(
    p: *const u8,
    n: usize,
    inst: *mut c_char,
    ni: usize,
    class: *mut c_char,
    nc: usize,
) {
    wm_class(input(p, n), destination(inst, ni), destination(class, nc));
}
#[no_mangle]
pub unsafe extern "C" fn x11_title(
    p: *const u8,
    n: usize,
    latin: c_int,
    out: *mut c_char,
    size: usize,
) {
    title(input(p, n), latin != 0, destination(out, size));
}
#[no_mangle]
pub unsafe extern "C" fn x11_motif_decorated(p: *const u32, n: usize) -> c_int {
    motif_decorated(input(p, n))
}
#[no_mangle]
pub unsafe extern "C" fn x11_size_hints(p: *const u32, n: usize, out: *mut SizeHints) {
    out.write(size_hints(input(p, n)));
}
#[no_mangle]
pub unsafe extern "C" fn x11_wm_hints_input(p: *const u32, n: usize) -> c_int {
    hints_input(input(p, n))
}
#[no_mangle]
pub extern "C" fn x11_moveresize_edges(direction: u32) -> c_int {
    moveresize_edges(direction)
}
#[no_mangle]
pub unsafe extern "C" fn x11_text_rank(name: *const c_char) -> c_int {
    if name.is_null() {
        0
    } else {
        text_rank(CStr::from_ptr(name).to_bytes())
    }
}
#[no_mangle]
pub unsafe extern "C" fn x11_text_target(name: *const c_char) -> c_int {
    i32::from(x11_text_rank(name) > 0)
}
#[no_mangle]
pub unsafe extern "C" fn x11_pick_display(
    taken: Option<unsafe extern "C" fn(c_int, *mut c_void) -> c_int>,
    ctx: *mut c_void,
) -> c_int {
    let Some(taken) = taken else {
        return -1;
    };
    (0..64).find(|&n| taken(n, ctx) == 0).unwrap_or(-1)
}
