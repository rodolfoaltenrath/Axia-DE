//! PDF viewer policy. Raster images, promises and UI callbacks stay in the host;
//! Rust owns geometry, cache generations, render admission and eviction.
mod cache_model;
mod edit_model;
mod geometry;
mod navigation;
mod sidebar_model;
mod viewer_model;
use serde_json::{json, Value};

#[derive(Default)]
pub struct Model {
    cache: cache_model::Cache,
    layout: navigation::Layout,
    viewer: viewer_model::Viewer,
    editor: edit_model::Editor,
    sidebar: sidebar_model::Sidebar,
}

impl Model {
    pub fn request(&mut self, input: &Value) -> Value {
        let reply = if input["cmd"] == "sidebar" {
            self.sidebar.request(input)
        } else if input["cmd"] == "edit" {
            if input["op"] == "highlightReply" {
                self.highlight_reply(input)
            } else {
                let reply = self.editor.request(input);
                if input["op"] == "open" && reply.is_ok() {
                    let _ = self.viewer.request(&json!({"op":"reset"}));
                }
                reply
            }
        } else if input["cmd"] == "cache" {
            self.cache.request(input)
        } else if input["cmd"] == "view" {
            self.layout.request(input)
        } else if input["cmd"] == "viewer" {
            if input["op"] == "reset" {
                self.editor.reset();
            }
            self.viewer.request(input)
        } else {
            geometry::request(input, &mut self.layout)
        };
        reply.unwrap_or_else(|error| json!({"error":error}))
    }
    fn highlight_reply(&mut self, input: &Value) -> Result<Value, &'static str> {
        let selection=self.viewer.request(&json!({"op":"selectionReply","token":input["token"],"page":input["page"],"result":input["result"]}))?;
        if selection.is_null() {
            return Ok(Value::Null);
        }
        let mut reply = if selection["rects"].as_array().is_some_and(|a| !a.is_empty()) {
            match self.editor.request(&json!({"op":"annotate","annotation":{"type":"highlight","page":selection["page"],"rects":selection["rects"],"color":input["color"]}})) {
                Ok(reply)=>reply,Err(_)=>self.editor.annotation_rejected(),
            }
        } else {
            self.editor.request(&json!({"op":"state"}))?
        };
        self.viewer.request(&json!({"op":"selectionClear"}))?;
        if reply.is_object() {
            reply["clearSelection"] = json!(true);
        }
        Ok(reply)
    }
}

#[cfg(test)]
mod edit_tests;
#[cfg(test)]
mod sidebar_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod viewer_tests;
