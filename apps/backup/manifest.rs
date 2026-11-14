pub const ID: &str = "backup";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Backups",
        "description":"Scheduled backups of the home folder to an external disk or a folder, with restore by date",
        "size":[980,640],"minSize":[760,480],"dock":false,"category":"System",
        "permissions":["files:read","backup:configure"],"nativeView":"rust","settings":true,
        "asyncServices":["backup.watch","backup.events","backup.config","backup.configure","backup.run","backup.pause","backup.cancel","backup.list","backup.ls","backup.path","backup.restore","backup.remove","backup.unlock","backup.lock","fs.volumes","windows.open"],
        "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
