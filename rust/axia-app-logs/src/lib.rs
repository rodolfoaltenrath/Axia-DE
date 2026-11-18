use axia_native as native_view;
use axia_native::clock;
#[path = "../../../apps/logs/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/logs/manifest.rs"]
pub mod manifest;
pub mod monitor {
    pub use axia_native::read;
}
pub use manifest::ID;
