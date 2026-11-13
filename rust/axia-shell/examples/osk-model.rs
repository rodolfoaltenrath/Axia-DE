//! Node test transport using the exact production on-screen keyboard model.
#[path = "../src/launcher/osk_model.rs"]
mod osk_model;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let input: serde_json::Value = serde_json::from_str(&line.expect("read osk request")).expect("osk JSON");
        let result = osk_model::request(&input).unwrap_or_else(|error| serde_json::json!({"error":error}));
        println!("{result}");
    }
}
