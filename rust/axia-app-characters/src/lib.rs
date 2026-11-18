use axia_native as native_view;
pub mod search {
    pub use axia_native::matching;
    pub use axia_native::matching::fold;
}
#[path = "../../../apps/characters/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/characters/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
