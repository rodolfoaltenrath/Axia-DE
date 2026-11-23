#[path = "../../../apps/screenshots/geometry.rs"]
pub mod geometry;
#[path = "../../../apps/screenshots/markup.rs"]
pub mod markup;
#[derive(Default)]
pub struct Model {
    pub editor: markup::Editor,
}
impl Model {
    pub fn request(&mut self, input: &serde_json::Value) -> serde_json::Value {
        if input["cmd"] == "markup" {
            self.editor.request(input)
        } else {
            geometry::request(input)
        }
    }
}
