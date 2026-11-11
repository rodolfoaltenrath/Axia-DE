//! Axia Store metadata. A native view (Rust mounts it; no React): the shell runs only
//! its package, signing and launch effects.
pub const ID: &str = "store";
pub fn manifest() -> serde_json::Value {
    serde_json::json!({
        "id": ID, "version": "0.9.0", "name": "Axia Store",
        "description": "Apps for Axia DE: discover, install and manage them",
        "size": [1080, 700], "minSize": [760, 480], "dock": true,
        "permissions": ["packages:install", "apps:launch"], "category": "System",
        "nativeView": "rust", "facts": "apps",
        "reads": {"apps": ["packages", "inspectPackage"], "sign": ["status", "userKey"]},
        "calls": {"apps": ["installPackage", "removePackage"], "sign": ["restore", "share"]}
    })
}
