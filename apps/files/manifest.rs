//! The browser and all file dialogs share the existing Rust Files service.
pub const ID: &str = "files";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Files",
        "description":"Axia file explorer with shared open and save dialogs, powered by Rust",
        "size":[960,600],"minSize":[640,400],"dock":true,
        "permissions":["fs:home:read","fs:home:write","fs:media","apps:open-with"],
        "category":"System","nativeView":"files","mode":"browser"})
}
