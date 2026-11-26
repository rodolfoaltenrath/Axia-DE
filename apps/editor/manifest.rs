pub const ID: &str = "editor";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Text Editor",
        "description":"TextMate-style code editor: project, tabs, multiple cursors, find and replace, syntax highlighting",
        "size":[900,600],"minSize":[520,340],"dock":true,"category":"Development",
        "permissions":["fs:read","fs:write"],"nativeView":"rust","appearanceSource":"native",
        "asyncServices":["files.pick","windows.menus","windows.events","windows.closeGuard","windows.close","print.open","inputMethod.context","inputMethod.state"],
        "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
