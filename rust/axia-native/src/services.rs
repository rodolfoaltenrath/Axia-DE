//! UI-thread callbacks into the shell. Copy callbacks before invoking them so
//! nested icon/app requests never hold a registry borrow.
use crate::NativeApp;
use serde_json::Value;
use std::cell::Cell;
#[derive(Clone, Copy)]
pub struct Services {
    pub release_media: fn(i32),
    pub release_image: fn(i32),
    pub desktop_facts: fn() -> Vec<DesktopFacts>,
    pub desktop_generation: fn() -> u64,
    pub network: fn(Value) -> Result<Value, String>,
    pub winmodel: fn(&str, &[Value]) -> Result<Value, String>,
    pub text_rtl: fn(&str) -> bool,
    pub app: fn(&str) -> Option<Box<dyn NativeApp>>,
    pub manifests: fn() -> Value,
    pub measure_text: fn(&str, f64, f64) -> f64,
}
impl Default for Services {
    fn default() -> Self {
        Self {
            release_media: |_| {},
            release_image: |_| {},
            desktop_facts: Vec::new,
            desktop_generation: || 0,
            network: |_| Err("native.hostUnavailable".into()),
            winmodel: |_, _| Err("native.hostUnavailable".into()),
            text_rtl: fallback_text_rtl,
            app: |_| None,
            manifests: || serde_json::json!([]),
            measure_text: |text, size, _| text.chars().count() as f64 * size * 0.5,
        }
    }
}
thread_local! { static SERVICES: Cell<Option<Services>> = const { Cell::new(None) }; }
pub fn install(services: Services) {
    SERVICES.with(|s| s.set(Some(services)));
}
pub fn app(id: &str) -> Option<Box<dyn NativeApp>> {
    SERVICES.with(Cell::get).and_then(|s| (s.app)(id))
}
pub fn manifests() -> Value {
    SERVICES
        .with(Cell::get)
        .map(|s| (s.manifests)())
        .unwrap_or_else(|| serde_json::json!([]))
}
pub fn icon_tree(id: &str, size: f64) -> Option<Value> {
    let tree = app(id)?.request(&serde_json::json!({"cmd":"icon","size":size}));
    (!tree.is_null()).then_some(tree)
}
pub fn measure_text(text: &str, size: f64, weight: f64) -> f64 {
    SERVICES
        .with(Cell::get)
        .map(|s| (s.measure_text)(text, size, weight))
        .unwrap_or_else(|| text.chars().count() as f64 * size * 0.5)
}
pub fn release_media(id: i32) {
    if let Some(services) = SERVICES.with(Cell::get) {
        (services.release_media)(id);
    }
}
pub fn release_image(id: i32) {
    if let Some(services) = SERVICES.with(Cell::get) {
        (services.release_image)(id);
    }
}

/// An installed desktop app as other Rust modules see it (the Axia Store).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesktopFacts {
    /// The desktop id; the shell names the app `desktop:<id>`.
    pub id: String,
    /// `Categories=` as written (`;`-separated).
    pub categories: String,
    /// A PNG path, or empty.
    pub icon: String,
}

pub fn desktop_facts() -> Vec<DesktopFacts> {
    SERVICES
        .with(Cell::get)
        .map(|s| (s.desktop_facts)())
        .unwrap_or_default()
}
pub fn desktop_generation() -> u64 {
    SERVICES
        .with(Cell::get)
        .map(|s| (s.desktop_generation)())
        .unwrap_or_default()
}

pub fn network(value: Value) -> Result<Value, String> {
    SERVICES
        .with(Cell::get)
        .ok_or_else(|| "native.hostUnavailable".to_owned())
        .and_then(|s| (s.network)(value))
}
pub fn winmodel(op: &str, args: &[Value]) -> Result<Value, String> {
    SERVICES
        .with(Cell::get)
        .ok_or_else(|| "native.hostUnavailable".to_owned())
        .and_then(|s| (s.winmodel)(op, args))
}
pub fn text_rtl(text: &str) -> bool {
    SERVICES.with(Cell::get).map(|s| (s.text_rtl)(text)).unwrap_or_else(|| fallback_text_rtl(text))
}
fn fallback_text_rtl(text: &str) -> bool {
    text.chars().take(256).find(|c| c.is_alphabetic()).is_some_and(|c|
        matches!(c as u32,0x0590..=0x08ff|0xfb1d..=0xfdff|0xfe70..=0xfefc|0x10800..=0x10fff|0x1e800..=0x1eeff))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Icon;
    impl NativeApp for Icon {
        fn request(&mut self, _: &Value) -> Value {
            json!({"kind":"view"})
        }
    }

    #[test]
    fn registry_callbacks_can_reenter_and_stay_on_the_installing_thread() {
        install(Services {
            release_media: |_| {},
            release_image: |_| {},
            app: |id| match id {
                "nested" => {
                    assert_eq!(icon_tree("leaf", 32.).unwrap()["kind"], "view");
                    Some(Box::new(Icon))
                }
                "leaf" => Some(Box::new(Icon)),
                _ => None,
            },
            manifests: || json!([]),
            measure_text: |_, _, _| 42.,
            desktop_facts: Vec::new,
            desktop_generation: || 0,
            network: Ok,
            winmodel: |_, _| Ok(Value::Null),
            text_rtl: |_| false,
        });
        assert_eq!(icon_tree("nested", 32.).unwrap()["kind"], "view");
        assert!(icon_tree("missing", 32.).is_none());
        assert_eq!(measure_text("text", 12., 400.), 42.);
        std::thread::spawn(|| {
            assert!(app("leaf").is_none());
            assert!(network(Value::Null).is_err());
        })
        .join()
        .unwrap();
        SERVICES.with(|s| s.set(None));
    }
}
