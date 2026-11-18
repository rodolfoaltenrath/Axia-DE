use axia_native as native_view;
#[path = "../../../apps/store/mod.rs"]
mod app;
pub use app::*;
use axia_native::services as applications;
#[path = "../../../apps/store/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
