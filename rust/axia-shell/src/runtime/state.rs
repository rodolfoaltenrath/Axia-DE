//! Storage for the UI-thread state borrowed by the existing renderer.
use super::types::Ui;

#[no_mangle]
pub static mut ui: Ui = Ui::empty();

/// UI-thread callback, installed by the native animation bridge. No reentry.
#[no_mangle]
pub static mut node_destroy_hook: Option<unsafe extern "C" fn(i32)> = None;
