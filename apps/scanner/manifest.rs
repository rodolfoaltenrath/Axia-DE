pub const ID: &str = "scanner";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Scanner",
    "description":"Driverless network scanners (eSCL): preview, area, resolution and save as PDF, PNG or JPEG",
    "size":[860,620],"minSize":[640,460],"dock":false,"category":"Graphics","permissions":["network:lan","fs:home:write"],
    "nativeView":"rust","settings":true,
    "asyncServices":["scanner.discover","scanner.caps","scanner.scan","images.decode","images.pdfThumb","images.release","images.convert","fs.stat","fs.copy","fs.jobs","fs.forgetJob","print.open"],
    "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
