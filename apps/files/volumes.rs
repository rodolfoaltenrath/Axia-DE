//! Read-only disk facts supplied by Axia's udev/mountinfo service.
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub id: String,
    pub disk: String,
    pub label: String,
    pub path: Option<PathBuf>,
    pub removable: bool,
    pub mountable: bool,
}
fn device(value: &Value) -> Option<String> {
    let name = value.as_str()?;
    (!name.is_empty()
        && name.len() < 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
    .then(|| name.into())
}
pub fn safe_mount(path: &Path) -> bool {
    path.is_absolute()
        && path != Path::new("/")
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        && !["/proc", "/sys", "/dev", "/etc", "/usr", "/var", "/boot"]
            .iter()
            .any(|p| path.starts_with(p))
        && path != Path::new("/home")
        && path != Path::new("/run")
        && path != Path::new("/media")
}
pub fn parse(facts: &Value) -> Vec<Volume> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for d in facts.as_array().into_iter().flatten().take(128) {
        if d["system"] == true {
            continue;
        }
        let Some(disk) = device(&d["name"]) else {
            continue;
        };
        let removable = d["removable"] == true || d["transport"] == "usb";
        let parts = d["partitions"].as_array().filter(|p| !p.is_empty());
        for part in parts
            .map(|p| p.iter().collect::<Vec<_>>())
            .unwrap_or_else(|| vec![d])
            .into_iter()
            .take(128)
        {
            let Some(id) = device(&part["name"]) else {
                continue;
            };
            if !seen.insert(id.clone()) {
                continue;
            }
            let fs = part["fstype"].as_str().unwrap_or("");
            if ["swap", "LVM2_member", "linux_raid_member"].contains(&fs) {
                continue;
            }
            let mountable = [
                "vfat", "exfat", "ntfs", "ext4", "ext3", "ext2", "btrfs", "xfs", "f2fs", "iso9660",
                "udf", "hfsplus", "apfs",
            ]
            .contains(&fs);
            let raw = part["mountpoint"]
                .as_str()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("");
            let path = if raw.is_empty() {
                None
            } else if safe_mount(Path::new(raw)) {
                Some(PathBuf::from(raw))
            } else {
                continue;
            };
            let label = part["label"]
                .as_str()
                .filter(|v| !v.is_empty())
                .or_else(|| part["partlabel"].as_str().filter(|v| !v.is_empty()))
                .unwrap_or(&id)
                .chars()
                .take(160)
                .collect();
            result.push(Volume {
                id,
                disk: disk.clone(),
                label,
                path,
                removable,
                mountable,
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn system_paths_are_hidden_and_encrypted_devices_are_manageable() {
        let facts = json!([
            {"name":"sda","system":true,"fstype":"ext4","mountpoint":"/"},
            {"name":"sdb","partitions":[{"name":"sdb1","fstype":"ext4","mountpoint":"/mnt/Data"},{"name":"sdb2","fstype":"swap"},{"name":"sdb3","fstype":"crypto_LUKS"}]},
            {"name":"sdc","fstype":"ext4","mountpoint":"/etc"},
            {"name":"../../sdd","fstype":"exfat"}
        ]);
        assert!(
            safe_mount(Path::new("/mnt")),
            "fstab may mount a data disk directly at /mnt"
        );
        let list = parse(&facts);
        assert_eq!(list.len(), 2);
        assert!(!list[1].mountable);
        assert_eq!(list[0].id, "sdb1");
        assert_eq!(list[0].path, Some(PathBuf::from("/mnt/Data")));
    }
}
