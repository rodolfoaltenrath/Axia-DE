pub const ID: &str = "notes";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Notes","description":"Notes and sticky notes on the desktop, with markdown, images and attachments","size":[860,560],"minSize":[560,360],"dock":true,"category":"Office","permissions":["fs:home"],"nativeView":"rust","facts":"system","settings":true,"sessionWidgets":"note","asyncServices":["windows.open","windows.menus","windows.events","print.open","clipboard.read","clipboard.image","inputMethod.context","inputMethod.state"],"calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
