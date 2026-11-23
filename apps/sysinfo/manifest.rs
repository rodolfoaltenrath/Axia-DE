pub const ID: &str = "sysinfo";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.8.6","name":"System Information",
        "description":"Every piece of hardware detected and what Axia DE makes of it",
        "size":[960,640],"minSize":[760,480],"dock":false,"permissions":["system:read"],
        "category":"System","nativeView":"rust","appearanceSource":"native","keyboard":true,
        "asyncServices":["sysinfo.snapshot","sysinfo.local","sysinfo.serials","sysinfo.hideSerials",
            "sysinfo.modules","sysinfo.benchStart","sysinfo.benchPoll","sysinfo.benchCancel",
            "sysinfo.probeStart","sysinfo.probePoll","sysinfo.testbench","sysinfo.bootStart",
            "sysinfo.bootCancel","sysinfo.restart","sysinfo.scene","sysinfo.save","sysinfo.openFile",
            "windows.open","clipboard.write"],
        "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
