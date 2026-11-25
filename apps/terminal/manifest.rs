pub const ID: &str = "terminal";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.8.5","name":"Terminal",
    "description":"Native terminal: tabs, panes, profiles, search, images",
    "size":[860,540],"minSize":[360,220],"dock":true,"transparent":true,"category":"System",
    "permissions":["shell"],
    "nativeView":"rust","appearanceSource":"native","keyboard":true,"settings":true,
    "dockActions":[{"id":"newWindow","label":"terminal.action.newWindow","args":{},"new":true},
        {"id":"newTab","label":"terminal.action.newTab","args":{}}],
    "asyncServices":["term.create","term.configure","term.close","term.infos","term.events","term.copy","term.selectOutput",
        "term.search","term.searchMove","term.logicalText","term.setMatches","term.cursorWord","term.words","term.replay",
        "term.copyKey","term.urlAt","term.log","term.export","term.screenText","term.highlight","term.tmuxPane",
        "clipboard.read","clipboard.history","clipboard.remember","fonts.list","windows.open","windows.menus","windows.guard"],
    "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
