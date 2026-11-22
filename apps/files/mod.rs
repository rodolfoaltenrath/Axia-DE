//! Arquivos: one Rust application and filesystem service for browser and dialogs.
mod address;
mod compat;
mod external;
mod ffi;
mod model;
mod native;
mod native_app;
mod native_resources;
pub use axia_fs::operations;
mod pdf_thumbnails;
pub use axia_fs::policy;
mod thumbnails;
pub use axia_fs::trash;
mod udisks;
mod ui;
mod video_thumbnails;
mod volumes;

pub use native_app::FilesView;
pub use ffi::{bind_native_appearance, Appearance};
pub use model::natural_cmp;
pub use native_resources::discard_result as discard_native_result;

pub use axia_fs::{io_error, Result};

mod network;

#[cfg(test)]
mod tests;

/// App icon for the native shell; shares Files' authored geometry.
pub fn native_icon(size: f64) -> serde_json::Value {
    fn convert(value: &mut serde_json::Value) {
        if let Some(object) = value.as_object_mut() {
            if let Some(kind) = object.remove("type") {
                object.insert("kind".into(), kind);
            }
            if let Some(text) = object.remove("text") {
                object.insert("label".into(), text);
            }
            if let Some(children) = object.get_mut("children").and_then(|v| v.as_array_mut()) {
                for child in children {
                    convert(child);
                }
            }
        }
    }
    let mut tree = ui::icon("files", size);
    convert(&mut tree);
    tree
}
