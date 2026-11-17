//! The offscreen engine has no terminal, Wayland surfaces or media players.
//! These Rust host endpoints belong only to the standalone helper/test executable.
#![allow(unused_variables)]
use libc::c_void;
#[repr(C)]
pub struct BoxRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}
macro_rules! noop{($name:ident($($arg:ident:$ty:ty),*))=>{#[no_mangle]pub extern "C" fn $name($($arg:$ty),*){}}}
noop!(term_paint(c:*mut c_void,id:i32,x:f32,y:f32,w:f32,h:f32,op:f32));
noop!(wl_paint(c:*mut c_void,id:i32,node:i32,x:f32,y:f32,w:f32,h:f32,op:f32));
noop!(wl_paint_holes(on:i32));
noop!(wl_paint_kept());
noop!(wl_paint_keep());
noop!(wl_paint_save());
noop!(wl_paint_restore());
noop!(wl_paint_begin());
noop!(wl_paint_popups(c:*mut c_void));
noop!(wl_tf_begin(node:i32,sx:f32,sy:f32,tx:f32,ty:f32,op:f32,skip:i32));
noop!(wl_tf_end());
noop!(wl_tf_nested(d:i32));
noop!(wl_tf_open_holes(c:*mut c_void));
noop!(media_paint(c:*mut c_void,id:i32,node:i32,x:f32,y:f32,w:f32,h:f32,r:f32,op:f32));
noop!(wl_preview_paint(c:*mut c_void,id:i32,x:f32,y:f32,w:f32,h:f32,op:f32));
#[no_mangle]
pub extern "C" fn wl_draws_meet(r: BoxRect) -> i32 {
    0
}
#[no_mangle]
pub extern "C" fn media_cover_node(x0: f32, y0: f32, x1: f32, y1: f32) -> i32 {
    0
}
#[no_mangle]
pub extern "C" fn gpu_active() -> i32 {
    0
}
#[no_mangle]
pub extern "C" fn term_preview_generation(id: i32) -> u64 {
    0
}
#[no_mangle]
pub extern "C" fn media_preview_generation(id: i32) -> u64 {
    0
}
#[no_mangle]
pub extern "C" fn wl_preview_generation(id: i32) -> u64 {
    0
}
