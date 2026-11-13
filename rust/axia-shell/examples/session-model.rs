//! Node test transport using the exact production session-restore model (and the Dock model whose
//! id rules it shares).
#[path = "../src/launcher/dock_model.rs"]
mod dock_model;
#[path = "../src/launcher/session_model.rs"]
mod session_model;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let input: serde_json::Value = serde_json::from_str(&line.expect("read request")).expect("request JSON");
        let result = match input["op"].as_str() {
            Some("session") => session_model::request(&input),
            _ => dock_model::request(&input),
        }
        .unwrap_or_else(|error| serde_json::json!({"error":error}));
        println!("{result}");
    }
}
