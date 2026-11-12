pub const ID: &str = "help";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Help",
        "description":"Axia help and tips","size":[860,600],"minSize":[640,440],
        "dock":false,"permissions":[],"category":"System","nativeView":"rust",
        "facts":"apps","settings":true})
}
