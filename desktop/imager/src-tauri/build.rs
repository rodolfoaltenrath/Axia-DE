fn main() {
    // `windows`: writing a whole disk needs an administrator, so the app asks for it when it starts
    // (src-tauri/windows-app.manifest). `macos` and `linux` ask only when the drive is opened.
    // The target, not the host, decides (the Windows build is cross-compiled).
    let mut attrs = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        attrs = attrs.windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows-app.manifest")));
    }
    tauri_build::try_build(attrs).expect("tauri-build");
}
