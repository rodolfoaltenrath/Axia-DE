pub const ID: &str = "logs";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Logs",
        "description":"System, app and Axia DE logs, with filters and live updates",
        "size":[1040,640],"minSize":[760,440],"dock":false,"permissions":["logs:read"],"category":"System",
        "nativeView":"rust","keyboard":true,"settings":true,
        "asyncServices":["logs.sources","logs.files","logs.query","logs.system","logs.tail","logs.follow","logs.write"],
        "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take","events"]}})
}
