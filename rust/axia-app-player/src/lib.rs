//! Player: the native window (library, queue, video, transport). Rules without a window are in `model.rs`.
use axia_native as native_view;
use axia_native::clock;
#[path = "../../../apps/player/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/player/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
