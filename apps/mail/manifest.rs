pub const ID: &str = "mail";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Mail",
        "description":"Mail with IMAP and SMTP accounts, conversations, offline search and safe HTML",
        "size":[1120,640],"minSize":[820,480],"dock":true,"category":"Office",
        "permissions":["network","fs:home"],"process":"isolated","nativeView":"rust","appearanceSource":"native",
        "settings":true,"fullTheme":true,"keyboard":true,
        "dockActions":[{"id":"compose","label":"mail.compose.newMessage","args":{"compose":true},"new":true}],
        "asyncServices":["mail.snapshot","mail.request","mail.pickFile","mail.attachmentFile","mail.releaseFile","mail.saveConfig","mail.account",
            "mail.allowImages","mail.openAttachment","mail.saveAttachment","mail.reveal",
            "pim.watch","pim.events","pim.image","pim.imageRelease","files.pick",
            "windows.open","windows.menus","windows.closeGuard","windows.close","clipboard.read",
            "inputMethod.context","inputMethod.state"],
        "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
