pub const ID: &str = "calculator";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Calculator",
        "description":"Calculator with keyboard input", "size":[300,430],
        "minSize":[260,380],"dock":true,"permissions":[],"category":"Utility",
        "nativeView":"widgets","keyboard":true})
}
