use axia_native as native_view;
use axia_native::clock;
#[path = "../../../apps/fonts/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/fonts/manifest.rs"]
pub mod manifest;
pub mod launcher {
    pub use axia_native::unicode::nfc as normalize_nfc;
}
pub use manifest::ID;
