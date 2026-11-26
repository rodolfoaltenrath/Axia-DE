//! Text Editor: the native window (project, tabs, highlighted text with several cursors, find and
//! replace). The document and editing rules need no window (`apps/editor/buffer.rs`, `edit.rs`).
use axia_native as native_view;
use axia_native::clock;
#[path = "../../../apps/editor/mod.rs"]
mod app;
pub use app::*;
#[path = "../../../apps/editor/manifest.rs"]
pub mod manifest;
pub use manifest::ID;
