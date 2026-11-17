//! LordBench and Testbench share these native measurements and wire contracts.
//! C consumers retain their ABI while implementation and verification use Rust.
#[cfg(feature = "native-host")]
extern crate axia_shell;
mod boot;
mod conditions;
mod core;
mod frames;
mod input;
mod protocol;
mod video;
mod work;
pub use work::*;
mod ui;
pub use ui::*;
mod run;
pub use run::*;
mod helper;
pub use helper::helper_main;
mod glscene;
pub use glscene::*;
#[cfg(feature = "gpu")]
mod gpu;
pub use boot::*;
pub use conditions::*;
pub use core::*;
pub use frames::*;
#[cfg(feature = "gpu")]
pub use gpu::*;
pub use input::*;
pub use protocol::*;
pub use video::*;

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
