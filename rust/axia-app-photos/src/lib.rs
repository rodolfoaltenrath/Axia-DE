use axia_native as native_view;
#[path = "../../../apps/photos/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/photos/manifest.rs"]
pub mod manifest;
use axia_fs as files;
pub use manifest::ID;
