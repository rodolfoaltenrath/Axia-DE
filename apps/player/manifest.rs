pub const ID: &str = "player";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({"id":ID,"version":"0.9.0","name":"Player",
        "description":"Videos and music: library by artist and album, queue, subtitles, full screen and picture in picture",
        "size":[960,600],"minSize":[480,320],"dock":true,"category":"AudioVideo",
        "permissions":["fs:home:read"],"nativeView":"rust","appearanceSource":"native",
        "asyncServices":["media.dirs","media.probe","media.play","media.control","media.close","media.events","media.thumb","media.thumbs","media.nowPlaying","windows.screen","windows.pip"],
        "calls":{"nativeAsync":["start"]},"reads":{"nativeAsync":["take"]}})
}
