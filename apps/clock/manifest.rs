pub const ID: &str = "clock";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Clock",
 "description":"World clock, alarms, timer and stopwatch","size":[560,520],"minSize":[440,420],
 "dock":false,"permissions":[],"category":"Utility","nativeView":"rust","settings":true,
 "facts":"clock","reads":{"time":["zone","zones","zoneOffset"]}})
}
