//! Compiled wallpaper metadata, borrowed by native consumers and IPC serializers.
use serde_json::{json, Value};
use std::sync::OnceLock;

pub(crate) fn wallpapers() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let index: Value = serde_json::from_str(include_str!("../../../assets/wallpapers/index.json"))
            .expect("compiled wallpaper catalog");
        let mut out: Vec<Value> = index.as_array().unwrap().iter().map(|entry| {
            let id = entry["id"].as_str().unwrap();
            let name = id.split('-').map(|word| {
                let mut letters = word.chars();
                format!("{}{}", letters.next().unwrap().to_uppercase(), letters.as_str())
            }).collect::<Vec<_>>().join(" ");
            let variants: Vec<_> = entry["variants"].as_array().unwrap().iter().map(|v|
                json!({"w":v["w"],"h":v["h"],"src":format!("asset:wallpapers/{}",v["file"].as_str().unwrap())})).collect();
            json!({"id":id,"category":entry["category"],"nameKey":format!("shell.wallpaper.{id}"),"name":name,
                "src":format!("asset:wallpapers/{}",entry["file"].as_str().unwrap()),
                "thumb":format!("asset:wallpapers/{}",entry["thumb"].as_str().unwrap()),"variants":variants})
        }).collect();
        for (id, name) in [("aurora","Aurora"),("ocean","Ocean"),("sunset","Sunset"),("graphite","Graphite")] {
            out.push(json!({"id":id,"category":"gradients","nameKey":format!("shell.wallpaper.{id}"),"name":name,
                "src":format!("asset:wallpapers/gradients/{id}.png"),
                "thumb":format!("asset:wallpapers/thumbs/gradients/{id}.jpg")}));
        }
        Value::Array(out)
    })
}

/// The fixed accent metadata, shared without adding it to every color plan.
pub(crate) fn accents() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| Value::Object(super::shell_theme::ACCENTS.iter()
        .map(|(name, color)| ((*name).into(), json!(color))).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_keeps_names_paths_order_and_one_shared_allocation() {
        let catalog = wallpapers(); assert!(std::ptr::eq(catalog, wallpapers()));
        let palette = accents(); assert!(std::ptr::eq(palette, accents()));
        assert_eq!(palette.as_object().unwrap().len(), super::super::shell_theme::ACCENTS.len());
        for (name, color) in super::super::shell_theme::ACCENTS { assert_eq!(palette[name], color); }
        let list = catalog.as_array().unwrap();
        let index: Value = serde_json::from_str(include_str!("../../../assets/wallpapers/index.json")).unwrap();
        assert_eq!(list.len(), index.as_array().unwrap().len()+4);
        assert_eq!(list[0]["name"], "Eternal Dawn");
        for (entry, original) in list.iter().zip(index.as_array().unwrap()) {
            assert_eq!(entry["id"], original["id"]);
            assert!(entry["src"].as_str().unwrap().starts_with("asset:wallpapers/"));
            assert!(entry["thumb"].as_str().unwrap().starts_with("asset:wallpapers/"));
            for (v, raw) in entry["variants"].as_array().unwrap().iter().zip(original["variants"].as_array().unwrap()) {
                assert_eq!(v["w"], raw["w"]); assert_eq!(v["h"], raw["h"]);
                assert_eq!(v["src"], format!("asset:wallpapers/{}",raw["file"].as_str().unwrap()));
            }
        }
        assert_eq!(list[list.len()-4]["id"], "aurora");
        let last = list.last().unwrap();
        assert_eq!(last["src"], "asset:wallpapers/gradients/graphite.png");
        assert_eq!(last["thumb"], "asset:wallpapers/thumbs/gradients/graphite.jpg");
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for entry in &list[list.len()-4..] {
            for key in ["src", "thumb"] {
                let file = assets.join(entry[key].as_str().unwrap().strip_prefix("asset:").unwrap());
                assert!(file.is_file(), "{}", file.display());
            }
        }
    }
}
