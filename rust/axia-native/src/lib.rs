//! Shared app contracts. Scene ownership and platform adapters stay in axia-shell.
mod app;
pub use app::NativeApp;
pub mod async_jobs;
pub mod civil_date;
pub mod clock;
pub mod date_format;
pub mod filename;
pub mod i18n;
pub mod icons;
pub mod markdown;
pub mod matching;
pub mod paint;
pub mod retained;
pub mod services;
pub mod style;
pub mod unicode;
pub mod windows;
pub use services::{app, icon_tree, measure_text};

/// Bounded UTF-8 reads shared by the toolkit and applications.
pub fn read(path: &std::path::Path, limit: usize) -> String {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    if file.take(limit as u64).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
