//! Node test transport using the exact production SMART model.
use axia_hw::smart as smart_model;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let input: serde_json::Value = serde_json::from_str(&line.expect("read SMART request")).expect("SMART JSON");
        let result = smart_model::request(&input).unwrap_or_else(|error| serde_json::json!({"error":error}));
        println!("{result}");
    }
}
