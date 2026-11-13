pub const ID: &str = "recorder";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Voice Recorder",
    "description":"Voice memos: record, waveform, rename, trim and export to Opus, MP3 or WAV",
    "size":[800,480],"minSize":[640,400],"dock":true,"category":"AudioVideo",
    "permissions":["microphone","fs:home:write"],"settings":true,"nativeView":"rust",
    "asyncServices":["media.dirs","media.sound","media.events","media.record","media.play","media.close","media.control","media.stop","media.waveform","media.transcode","fs.mkdir","fs.list","fs.stat","fs.rename"],
    "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
