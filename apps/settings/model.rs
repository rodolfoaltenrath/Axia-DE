//! Settings navigation policy. The section order, capabilities, search and
//! remembered selection belong to Rust; the transitional JSX only draws them.
use serde_json::{json, Value};

const GROUPS: &[&[(&str, &str, &str)]] = &[
    &[("general", "#8e8e93", "i"), ("appearance", "#3a3a3c", "◐"), ("wallpaper", "#30b0c7", "▣"), ("dock", "#3a3a3c", "▁"), ("displays", "#0a84ff", "▭")],
    &[("network", "#0a84ff", "⇅"), ("bluetooth", "#0a84ff", "ᛒ"), ("sound", "#ff375f", "♪"), ("notifications", "#ff3b30", "◉"), ("printers", "#636366", "⎙"), ("clipboard", "#5e5ce6", "⎘")],
    &[("language", "#30b0c7", "A"), ("datetime", "#0a84ff", "◷"), ("keyboard", "#8e8e93", "⌨"), ("trackpad", "#8e8e93", "▭"), ("defaultapps", "#8e8e93", "◫")],
    &[("power", "#30d158", "bolt"), ("cooling", "#32ade6", "fan"), ("firmware", "#5e5ce6", "chip"), ("privacy", "#0a84ff", "lock"), ("users", "#0a84ff", "☺"), ("updates", "#8e8e93", "↻"), ("session", "#ff453a", "↪")],
];

pub struct Model { section: String }
impl Default for Model {
    fn default() -> Self { Self { section: "general".into() } }
}
fn shown(id: &str, input: &Value) -> bool {
    (id != "trackpad" || input["hasPad"] == true)
        && (id != "firmware" || input["pi5"] == true)
        && !(input["standalone"] == true && matches!(id, "users" | "cooling" | "firmware"))
}
impl Model {
    pub fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "catalog" => json!(GROUPS.iter().map(|group| group.iter().map(|(id,color,glyph)| json!({"id":id,"color":color,"glyph":glyph})).collect::<Vec<_>>()).collect::<Vec<_>>()),
            "select" => {
                let mut id = input["section"].as_str().unwrap_or("");
                if id == "themes" { id = "appearance"; }
                if input["standalone"] == true && matches!(id,"users"|"cooling"|"firmware") { id = "general"; }
                if GROUPS.iter().flat_map(|g| g.iter()).any(|(key,_,_)| *key == id) { self.section = id.into(); }
                json!(self.section)
            }
            "current" => json!(self.section),
            "groups" => {
                let q = input["query"].as_str().unwrap_or("").trim().to_lowercase();
                json!(GROUPS.iter().filter_map(|g| {
                    let rows: Vec<_> = g.iter().filter(|(id,_,_)| shown(id,input)
                        && input["captions"][*id].as_str().unwrap_or("").to_lowercase().contains(&q))
                        .map(|(id,_,_)| *id).collect();
                    (!rows.is_empty()).then_some(rows)
                }).collect::<Vec<_>>())
            }
            _ => json!({"error":"app.invalidRequest"}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_keeps_alias_remount_and_host_policy() {
        let mut m = Model::default();
        assert_eq!(m.request(&json!({"cmd":"current"})),"general");
        assert_eq!(m.request(&json!({"cmd":"select","section":"themes"})),"appearance");
        assert_eq!(m.request(&json!({"cmd":"select","section":"bogus"})),"appearance");
        assert_eq!(m.request(&json!({"cmd":"current"})),"appearance");
        assert_eq!(m.request(&json!({"cmd":"select","section":"users","standalone":true})),"general");
    }
    #[test]
    fn search_retains_groups_and_hardware_visibility() {
        let mut m = Model::default();
        let all=m.request(&json!({"cmd":"groups","hasPad":true,"pi5":true}));
        assert_eq!(all[3],json!(["power","cooling","firmware","privacy","users","updates","session"]));
        let hidden=m.request(&json!({"cmd":"groups","standalone":true}));
        let ids=hidden.to_string();
        for id in ["trackpad","firmware","cooling","users"] { assert!(!ids.contains(id)); }
        assert_eq!(m.request(&json!({"cmd":"groups","query":"  SOM  ","captions":{"sound":"Som áudio","network":"Rede"}})),json!([["sound"]]));
    }
}
