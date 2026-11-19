//! Borrowed session windows. Native apps never retain a JS object or an FFI pointer.
use serde_json::Value;

pub struct WindowRef<'a> {
    pub id: &'a str,
    pub app: &'a str,
    pub title: &'a str,
    pub args: &'a Value,
}

/// UI-thread reader. A visit borrows the model only for the callback's duration.
pub trait WindowSource {
    fn revision(&self) -> u64;
    fn visit(&self, visitor: &mut dyn FnMut(WindowRef<'_>));
}

/// Match the same desktop identity for the launcher, Dock and app-control views.
pub fn matches_desktop(app_id: &str, desktop_id: &str, wm_class: &str) -> bool {
    if app_id.is_empty() { return false; }
    let want = app_id.to_lowercase();
    let desktop = desktop_id.to_lowercase();
    desktop == want || wm_class.to_lowercase() == want || desktop.ends_with(&format!(".{want}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_matching_keeps_class_suffix_and_empty_identity_rules() {
        assert!(matches_desktop("EDITOR", "org.editor", ""));
        assert!(matches_desktop("SomeApp", "other.desktop", "someapp"));
        assert!(matches_desktop("org.Editor", "org.editor", ""));
        assert!(!matches_desktop("", "", ""));
        assert!(!matches_desktop("editor", "org.noteditor", ""));
    }
}
