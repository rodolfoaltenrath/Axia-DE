//! The session's backups (docs/SYSTEM.md, "Backups"): the snapshot engine with hard links and
//! retention, the optional encryption of the copies, and the service thread that schedules them.
//!
//! `axia-server` reaches the service through the C ABI of `server/src/backup/backup.h`
//! (`ffi`); everything else is Rust. Paths are handled as bytes: a file name need not be UTF-8.
pub mod crypt;
pub mod engine;
/// The error code lines live in their own crate now (shared with the other crates the server calls).
pub use axia_errcode as errcode;
mod ffi;
pub mod monitor;
pub mod service;
