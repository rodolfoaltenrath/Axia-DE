//! Isolated PDF model test transport. Replays one owner's history per request
//! so synchronous Node tests need neither a compositor nor native image handles.
#[path = "../../../apps/pdf/model.rs"]
mod pdf_model;
use serde_json::Value;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.expect("PDF request")).expect("PDF JSON");
        let mut model = pdf_model::Model::default();
        let mut reply = Value::Null;
        for request in input.as_array().expect("owner request history") {
            reply = model.request(request);
        }
        println!("{reply}");
    }
}
