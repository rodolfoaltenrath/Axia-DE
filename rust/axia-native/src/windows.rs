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

/// Shared launcher/Dock identity, using the catalog's original match order.
pub fn launcher_identity(w: &WindowRef<'_>, registry: &[Value], catalog: &[Value]) -> String {
    if !w.id.starts_with("linux#") {
        return w.app.to_owned();
    }
    let id = w.args["appId"].as_str().unwrap_or("");
    if w.args["pkg"] == true
        && registry
            .iter()
            .any(|app| app["id"] == id && (app["package"] == true || app["isolated"] == true))
    {
        return id.to_owned();
    }
    catalog
        .iter()
        .find(|app| {
            app["kind"] == "linux"
                && matches_desktop(
                    id,
                    app["desktopId"].as_str().unwrap_or(""),
                    app["wmClass"].as_str().unwrap_or(""),
                )
        })
        .and_then(|app| app["id"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("linux:{}", if id.is_empty() { w.id } else { id }))
}

pub fn dock_identity(w: &WindowRef<'_>, registry: &[Value], catalog: &[Value]) -> String {
    let key = launcher_identity(w, registry, catalog);
    if key.starts_with("desktop:") || key.starts_with("linux:") {
        return key;
    }
    match registry
        .iter()
        .find(|app| app["id"] == key && app["builtin"] != true)
    {
        Some(app) => format!(
            "{}:{key}",
            if app["package"] == true { "lpk" } else { "app" }
        ),
        None => key,
    }
}

/// Match the same desktop identity for the launcher, Dock and app-control views.
pub fn matches_desktop(app_id: &str, desktop_id: &str, wm_class: &str) -> bool {
    if app_id.is_empty() {
        return false;
    }
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
