use axia_native as native_view;
use axia_native::clock;
#[path = "../../../apps/scanner/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/scanner/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
