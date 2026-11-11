//! Build-time metadata. Shared by the Rust app registry and the bundler.
pub const ID: &str = "forcequit";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Force Quit",
        "description":"Ends an app that stopped answering", "size":[420,460],
        "minSize":[360,320],"dock":false,"permissions":[],"category":"System",
        "hidden":true,"nativeView":"rust","facts":"windows"})
}
