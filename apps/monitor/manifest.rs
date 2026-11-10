//! Compiled Monitor metadata, including native privacy sections and host capabilities.
pub const ID: &str = "monitor";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Monitor","description":"CPU, memory, network, disks, processes and sensors","size":[1180,700],"minSize":[900,580],"dock":true,"permissions":["system:read","network:read"],"category":"System","nativeView":"rust","refreshMs":1000,"settings":true,
"facts":"apps","reads":{"fw":["rules"]},"services":{"fw":["status","state","poll","lists"]},"calls":{"fw":["setAppRule","setTrusted","setRestricted","setMode","removeRule","removeList","updateLists","setList","addRule","updateRule","addList"]},
"privacy":[{"id":"monitor","order":10,"section":"privacy"},{"id":"firewall","order":11,"section":"firewall"},{"id":"fwrules","order":12,"section":"rules"},{"id":"blocklists","order":13,"section":"blocklists"}],"dockActions":[{"id":"network","label":"monitor.network","args":{"tab":"network"}},{"id":"cooling","label":"monitor.tab.cooling","args":{"tab":"cooling"}},{"id":"disks","label":"monitor.disks","args":{"tab":"disks"}}]})
}
