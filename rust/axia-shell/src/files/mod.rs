//! Arquivos: one Rust application and filesystem service for browser and dialogs.
mod address;
mod compat;
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
