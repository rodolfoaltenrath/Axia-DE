use axia_native as native_view;
use axia_native::clock;
#[path = "../../../apps/recorder/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/recorder/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
