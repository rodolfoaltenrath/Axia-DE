use axia_native as native_view;
use axia_native::style as native_style;
#[path = "../../../apps/files/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/files/manifest.rs"]
pub mod manifest;
use app as files;
use axia_native::filename as text_edit;
pub use manifest::ID;
