//! About Axia: the original layout, branding, status and report action, in the Rust scene.
#[cfg(test)]
mod tests;
mod ui;
use serde_json::{json, Value};
#[derive(Default)]
pub struct App;
impl App {
    pub fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "icon" => ui::icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => json!({"tree":ui::tree(input)}),
            "report" => json!({"effect":{"kind":"openApp","app":"sysinfo"}}),
            _ => Value::Null,
        }
    }
}
impl crate::native_view::NativeApp for App {
    fn request(&mut self, input: &Value) -> Value {
        App::request(self, input)
    }
    fn pixels(&self, _: &str) -> Option<(usize, usize, &[u8])> {
        None
    }
}
