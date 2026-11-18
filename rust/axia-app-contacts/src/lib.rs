use axia_native as native_view;
use axia_native::clock;
#[path = "../../../apps/contacts/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/contacts/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
