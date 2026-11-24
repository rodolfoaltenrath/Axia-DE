pub const ID: &str = "screenshots";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.8.5","name":"Screenshots",
    "description":"Screenshots (screen, window or area) with annotations, and screen recording",
    "size":[440,420],"minSize":[380,360],"dock":false,"category":"Utility",
    "permissions":["screen","microphone","fs:home:write"],
    "nativeView":"rust","appearanceSource":"native","keyboard":true})
}
