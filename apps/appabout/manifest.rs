pub const ID: &str = "appabout";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"About App",
        "description":"About one app: its name, version and where it comes from",
        "size":[340,300],"minSize":[300,260],"dock":false,"permissions":[],
        "category":"System","hidden":true,"nativeView":"widgets","facts":"apps"})
}
