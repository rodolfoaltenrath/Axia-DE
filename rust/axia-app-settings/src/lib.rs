use axia_native as native_view;
use axia_native::clock;
pub mod search {
    pub use axia_native::matching;
    pub use axia_native::matching::fold;
}
#[path = "../../../apps/settings/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/settings/manifest.rs"]
pub mod manifest;
pub mod native_apps {
    pub use axia_native::services::manifests;
}
pub mod network {
    pub use axia_native::services::network as request;
}
pub mod winmodel {
    pub use axia_native::services::winmodel as call;
}
#[path = "../../../apps/settings/model.rs"]
pub mod settings_model;
pub use manifest::ID;
