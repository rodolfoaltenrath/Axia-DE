//! Arquivos: one Rust application and filesystem service for browser and dialogs.
mod address;
mod compat;
mod external;
mod ffi;
mod model;
pub mod operations;
mod pdf_thumbnails;
pub mod policy;
mod thumbnails;
pub mod trash;
mod udisks;
mod ui;
mod volumes;

pub type Result<T> = std::result::Result<T, String>;

pub fn io_error(e: std::io::Error) -> String {
    let name = match e.raw_os_error().unwrap_or(libc::EIO) {
        libc::ENOENT => "ENOENT",
        libc::EACCES => "EACCES",
        libc::EPERM => "EPERM",
        libc::EEXIST => "EEXIST",
        libc::ENOSPC => "ENOSPC",
        libc::EROFS => "EROFS",
        libc::ENOTDIR => "ENOTDIR",
        libc::EISDIR => "EISDIR",
        libc::ENOTEMPTY => "ENOTEMPTY",
        libc::ELOOP => "ELOOP",
        libc::ECANCELED => "ECANCELED",
        libc::EXDEV => "EXDEV",
        libc::ENAMETOOLONG => "ENAMETOOLONG",
        _ => "EIO",
    };
    format!("sys.errno errno={name}")
}

mod network;

#[cfg(test)]
mod tests;

/// App icon for the native shell; shares Files' authored geometry.
pub(crate) fn native_icon(size: f64) -> serde_json::Value {
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
