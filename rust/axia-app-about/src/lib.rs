use axia_native as native_view;
#[path = "../../../apps/about/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/about/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
