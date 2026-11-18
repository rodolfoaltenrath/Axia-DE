use axia_native as native_view;
#[path = "../../../apps/sysprobe/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/sysprobe/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
