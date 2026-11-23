pub const ID: &str = "disks";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.8.5","name":"Disks",
    "description":"Disks and USB sticks: usage, health (SMART), mount, format, erase and create a bootable USB",
    "size":[960,620],"minSize":[760,480],"dock":false,"permissions":["disks:manage"],"category":"System",
    "nativeView":"rust","appearanceSource":"native","keyboard":true,
    "asyncServices":["disks.snapshot","disks.run","disks.events","disks.cancel","disks.images","disks.stat",
        "disks.smartInfo","disks.smartHealth","disks.smartOwn","disks.smartAbort","disks.smartTest","disks.installSmart","disks.summary"],
    "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
