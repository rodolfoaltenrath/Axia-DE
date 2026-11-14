pub const ID: &str = "camera";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Camera",
        "description":"Webcam preview, photos and videos, with mirroring and camera choice",
        "size":[820,560],"minSize":[480,360],"dock":true,"category":"AudioVideo",
        "permissions":["camera","microphone","fs:home:write"],"nativeView":"rust","settings":true,
        "asyncServices":["media.cameras","media.camera","media.view","media.close","media.photo","media.cameraRecord","media.events","media.thumbs","media.thumb","media.dirs","fs.mkdir","fs.stat","windows.open","settings.patch"],
        "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
