pub const ID: &str = "reminders";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.8.5","name":"Reminders","description":"To-do lists with due dates, priorities and flags","size":[820,560],"minSize":[600,400],"dock":true,"category":"Office","permissions":[],"process":"isolated","nativeView":"rust","appearanceSource":"native","settings":true,"fullTheme":true,"keyboard":true,"windowState":true,"asyncServices":["pim.request","pim.watch","pim.events","windows.state","clipboard.read"],"calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
