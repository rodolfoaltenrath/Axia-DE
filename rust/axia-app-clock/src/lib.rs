pub use axia_clock as clock;
use axia_native as native_view;
#[path = "../../../apps/clock/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/clock/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
