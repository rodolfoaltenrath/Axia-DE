pub const ID: &str = "calendar";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.8.5","name":"Calendar","description":"Day, week and month views of your local and synced calendars","size":[960,600],"minSize":[640,440],"dock":true,"category":"Office","permissions":[],"process":"isolated","nativeView":"rust","settings":true,"fullTheme":true,"keyboard":true,"windowState":true,"asyncServices":["pim.request","pim.watch","pim.events","windows.state","clipboard.read"],"calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
