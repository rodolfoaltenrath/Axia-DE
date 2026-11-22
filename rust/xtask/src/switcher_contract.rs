//! Shared observations for CI and the offline RPM check; assertions stay in Rust.
pub const DRIVER: &str = include_str!("../tests/fixtures/switcher_runtime.jsx");

pub fn verify(report: &serde_json::Value) -> Result<(), String> {
    for flag in [
        "noFullscreenFacts",
        "realClientClosed",
        "realFullscreenClient",
        "nativeFullscreenProbe",
        "nativePreparation",
        "beforeMountCancel",
        "missingFullscreenRejected",
        "noDisplayTransport",
        "nativeDisplayGeometry",
        "nativeFocusCommands",
        "staleCompletionIgnored",
        "noCatalogTransport",
        "nativeCatalogRevision",
        "staleCatalogFacts",
        "nativeBuiltinPolicy",
        "noAppearanceFacts",
        "withoutAppearanceFacts",
        "staleAppearanceMirror",
        "appearanceRetained",
        "bound",
        "initial",
        "freshCommit",
        "cycling",
        "cancelled",
        "fastRelease",
        "rawTarget",
        "desktop",
        "fullscreenCancel",
        "fullscreenCommit",
        "profileCancel",
        "dispatcher",
        "viewport",
        "pointerCommit",
        "empty",
        "unmounted",
        "noWindowTransport",
        "saved",
    ] {
        if report[flag] != true {
            return Err(format!("switcher {flag}: {report}"));
        }
    }
    Ok(())
}
