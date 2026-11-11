pub const ID: &str = "characters";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Emoji & Symbols",
        "description":"Emoji by category with search and skin tones, and Unicode symbols by block (quick picker with ❖.)",
        "size":[860,560],"minSize":[600,380],"dock":false,"category":"Utility",
        "permissions":[],"nativeView":"rust","settings":true})
}
