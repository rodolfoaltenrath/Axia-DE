pub const ID: &str = "contacts";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.8.5","name":"Contacts","description":"Address books, groups and contact cards","size":[880,560],"minSize":[700,400],"dock":true,"category":"Office","permissions":[],"process":"isolated","nativeView":"rust","appearanceSource":"native","settings":true,"fullTheme":true,"keyboard":true,"windowState":true,
    "asyncServices":["pim.request","pim.watch","pim.events","pim.filePick","pim.fileRelease","pim.image","pim.imageRelease","windows.state","windows.open","clipboard.read"],"calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
