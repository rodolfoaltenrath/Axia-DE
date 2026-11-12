//! Owned compositor protocols. The host calls these on the Wayland/UI thread.
pub(crate) mod ffi;
mod gestures;
pub(crate) mod input;
mod text_input;
