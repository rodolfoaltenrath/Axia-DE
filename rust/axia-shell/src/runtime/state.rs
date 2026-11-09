//! Storage for the UI-thread state borrowed by the existing renderer.
use super::types::Ui;

#[no_mangle]
pub static mut ui: Ui = Ui::empty();
