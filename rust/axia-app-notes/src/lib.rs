use axia_native as native_view;
use axia_native::clock;
#[path = "../../../apps/notes/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/notes/manifest.rs"]
pub mod manifest;
use axia_fs as files;
pub use manifest::ID;
