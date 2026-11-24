pub const ID: &str = "passwords";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.8.5","name":"Passwords",
    "description":"Passwords, 2FA codes, Wi-Fi, cards and secure notes in an encrypted vault; also the session keyring",
    "size":[1040,660],"minSize":[820,500],"dock":true,"category":"Utility","permissions":["usb"],
    "process":"isolated","protected":true,"singleWindow":true,
    "nativeView":"rust","fullTheme":true,
    "asyncServices":["windows.open","windows.events","windows.menus","vault.watch","vault.events","vault.request",
        "file.pick","file.save","clipboard.secret","clipboard.text"],
    "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
