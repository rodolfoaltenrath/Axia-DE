//! LordBench and Testbench share these native measurements and wire contracts.
//! C consumers retain their ABI while implementation and verification use Rust.
mod boot;
mod core;
mod protocol;
pub use boot::*;
pub use core::*;
pub use protocol::*;

use libc::{c_char, c_int};
use std::ffi::CStr;
pub(crate) unsafe fn bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        b""
    } else {
        CStr::from_ptr(p).to_bytes()
    }
}
pub(crate) fn status(result: std::io::Result<()>) -> c_int {
    if result.is_ok() {
        0
    } else {
        -1
    }
}
