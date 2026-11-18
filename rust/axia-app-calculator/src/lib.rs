use axia_native as native_view;
#[path = "../../../apps/calculator/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/calculator/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
