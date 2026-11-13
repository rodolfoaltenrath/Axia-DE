//! Node test transport using the exact production Dock policy source.
#[path = "../src/launcher/dock_model.rs"]
mod dock_model;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let input: serde_json::Value =
            serde_json::from_str(&line.expect("read Dock request")).expect("Dock JSON");
        let result =
            dock_model::request(&input).unwrap_or_else(|error| serde_json::json!({"error":error}));
        println!("{result}");
    }
}
