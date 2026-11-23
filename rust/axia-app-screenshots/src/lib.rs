#[path = "../../../apps/screenshots/auxiliary.rs"]
pub mod auxiliary;
#[path = "../../../apps/screenshots/editor_view.rs"]
pub mod editor_view;
#[path = "../../../apps/screenshots/geometry.rs"]
pub mod geometry;
#[path = "../../../apps/screenshots/markup.rs"]
pub mod markup;
#[path = "../../../apps/screenshots/panel.rs"]
pub mod panel;
#[path = "../../../apps/screenshots/selector.rs"]
pub mod selector;
#[path = "../../../apps/screenshots/session.rs"]
pub mod session;
#[derive(Default)]
pub struct Model {
    pub editor: markup::Editor,
    pub session: session::State,
}
impl Model {
    pub fn request(&mut self, input: &serde_json::Value) -> serde_json::Value {
        if input["cmd"] == "session" {
            self.session.request(input, axia_native::clock::now())
        } else if input["cmd"] == "markup" {
            self.editor.request(input)
        } else {
            geometry::request(input)
        }
    }
}
