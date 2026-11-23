#[path = "../../../apps/sysinfo/model.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/sysinfo/native.rs"]
mod view;
pub use view::App;
#[path = "../../../apps/sysinfo/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
