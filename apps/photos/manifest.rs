pub const ID: &str = "photos";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Photos","description":"Native image library and viewer with asynchronous zoom, animation and non-destructive editing","size":[980,640],"minSize":[520,360],"dock":true,"category":"Graphics","permissions":["fs:home:read","fs:home:write"],"nativeView":"rust","appearanceSource":"native","settings":true,"asyncServices":["windows.menus","windows.events","print.open"],"calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
