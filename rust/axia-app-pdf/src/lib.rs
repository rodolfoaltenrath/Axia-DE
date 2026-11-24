use axia_native as native_view;
#[path = "../../../apps/pdf/model.rs"]
mod model;
pub use model::*;
#[path = "../../../apps/pdf/native.rs"]
mod native;
pub use native::App;
