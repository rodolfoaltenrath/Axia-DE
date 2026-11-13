pub const ID: &str = "fonts";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Fonts",
    "description":"Font viewer and manager: samples, character map, install and remove user fonts",
    "size":[960,620],"minSize":[640,420],"dock":false,"category":"Utility","permissions":["fs:home:read","fs:home:write"],
    "nativeView":"rust","keyboard":true,
    "asyncServices":["fonts.list","fonts.info","fonts.sample","fonts.grid","fonts.release","fs.stat","fs.mkdir","fs.copy","fs.jobs","fs.forgetJob","fs.trash"],
    "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
