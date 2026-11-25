use axia_native as native_view;
#[path = "../../../apps/terminal/model.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/terminal/workspace.rs"]
pub mod workspace;
#[path = "../../../apps/terminal/native.rs"]
mod native;
pub use native::App;
#[path = "../../../apps/terminal/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
#[path = "../../../apps/terminal/iterm.rs"]
pub mod iterm;
