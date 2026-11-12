//! Test adapter for the existing Node suites. Uses the production Rust model
//! sources directly; stdin/stdout is one JSON request/reply per line.
#[path = "../../../apps/passwords/model.rs"]
mod passwords_model;
#[path = "../../../apps/settings/model.rs"]
mod settings_model;
#[path = "../../../apps/sysinfo/model.rs"]
mod sysinfo_model;
#[path = "../../../apps/terminal/model.rs"]
mod terminal_model;
use serde_json::{json, Value};
use std::io::{self, BufRead};
fn main() {
    let mut settings = settings_model::Model::default();
    for line in io::stdin().lock().lines() {
        let line = line.expect("read model request");
        let input: Value = serde_json::from_str(&line).expect("model request JSON");
        let reply = match input["app"].as_str().unwrap_or("") {
            "settings" => settings.request(&input["input"]),
            "terminal" => terminal_model::request(&input["input"]),
            "passwords" => passwords_model::request(&input["input"]),
            "sysinfo" => sysinfo_model::request(&input["input"]),
            _ => json!({"error":"app.invalidRequest"}),
        };
        println!("{reply}");
    }
}
