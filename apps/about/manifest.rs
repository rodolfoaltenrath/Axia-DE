pub const ID: &str = "about";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"About Axia",
        "description":"System and graphics server information","size":[490,730],
        "minSize":[160,100],"dock":true,"permissions":["system:read"],"category":"System",
        "nativeView":"rust","facts":"system",
        "services":{"system":["info"],"$root":["stats"],"code":["status"],"fan":["caseConfig"]},
        "serviceIntervals":{"system":5000,"$root":2000,"code":-1,"fan":-1}})
}
