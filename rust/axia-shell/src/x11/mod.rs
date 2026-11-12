//! X11 protocol and lazy XWayland process services, without libX11/XCB.
mod ffi;
#[cfg(test)]
mod tests;
pub(crate) mod wire;
mod xwayland;
