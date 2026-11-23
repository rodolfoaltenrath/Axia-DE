//! Device/action policy is revalidated before the host invokes a closed action.
use super::{arr, text};
use serde_json::{json, Value};

pub fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 31
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}
pub fn can_touch(disk: &Value, admin: bool) -> bool {
    disk.is_object()
        && name(text(&disk["name"]))
        && disk["system"] != true
        && disk["ro"] != true
        && (disk["removable"] == true || admin)
}
pub fn smartable(disk: &Value) -> bool {
    disk["transport"] != "loop"
        && regex::Regex::new(r"^(sd[a-z]{1,3}|hd[a-z]{1,3}|nvme\d{1,3}n\d{1,3})$")
            .unwrap()
            .is_match(text(&disk["name"]))
}
pub fn own_smart(disk: &Value) -> bool {
    disk["transport"] != "loop"
        && ["sd", "hd", "nvme"]
            .iter()
            .any(|prefix| text(&disk["name"]).starts_with(prefix))
        && name(text(&disk["name"]))
}
pub fn target<'a>(disk: &'a Value, part: &'a Value) -> &'a Value {
    if part.is_object() {
        part
    } else {
        disk
    }
}
pub fn available(action: &str, disk: &Value, part: &Value, admin: bool) -> bool {
    if !can_touch(disk, admin) {
        return false;
    }
    let t = target(disk, part);
    if !name(text(&t["name"])) {
        return false;
    }
    match action {
        "mount" => {
            !text(&t["fstype"]).is_empty()
                && !["swap", "crypto_LUKS"].contains(&text(&t["fstype"]))
                && text(&t["mountpoint"]).is_empty()
        }
        "unmount" => !text(&t["mountpoint"]).is_empty(),
        "eject" => !part.is_object() && disk["removable"] == true,
        "format" | "erase" => true,
        "label" => {
            part.is_object()
                && ["vfat", "exfat", "ext2", "ext3", "ext4", "ntfs"]
                    .contains(&text(&part["fstype"]))
        }
        "image" => !part.is_object(),
        _ => false,
    }
}
pub fn fingerprint(disk: &Value, part: &Value) -> Value {
    let fields = |v: &Value, keys: &[&str]| keys.iter().map(|k| v[*k].clone()).collect::<Vec<_>>();
    json!([
        fields(disk, &["name", "model", "serial", "size"]),
        if part.is_object() {
            json!(fields(
                part,
                &[
                    "name",
                    "uuid",
                    "start",
                    "size",
                    "fstype",
                    "label",
                    "partlabel"
                ]
            ))
        } else {
            Value::Null
        }
    ])
}
pub fn current<'a>(list: &'a Value, disk: &Value, part: &Value) -> Option<(&'a Value, &'a Value)> {
    let live = arr(list).iter().find(|d| d["name"] == disk["name"])?;
    let live_part = if part.is_object() {
        arr(&live["partitions"])
            .iter()
            .find(|p| p["name"] == part["name"])?
    } else {
        &Value::Null
    };
    let layout = |d: &Value| {
        arr(&d["partitions"])
            .iter()
            .map(|p| fingerprint(&Value::Null, p))
            .collect::<Vec<_>>()
    };
    (fingerprint(live, live_part) == fingerprint(disk, part)
        && (part.is_object() || layout(live) == layout(disk)))
    .then_some((live, live_part))
}
pub fn clean_label(label: &str, kind: &str) -> String {
    if ["fat32", "vfat"].contains(&kind) {
        label.to_uppercase()
    } else {
        label.into()
    }
}
pub fn label_limit(kind: &str) -> usize {
    match kind {
        "fat32" | "vfat" => 11,
        "exfat" => 15,
        "ntfs" => 32,
        _ => 16,
    }
}
pub fn label_valid(label: &str, kind: &str) -> bool {
    let clean = clean_label(label, kind);
    clean.encode_utf16().count() <= label_limit(kind)
        && !clean.starts_with('-')
        && clean.chars().all(|c| {
            if ["fat32", "vfat"].contains(&kind) {
                c.is_ascii_uppercase() || c.is_ascii_digit() || " _-".contains(c)
            } else {
                !c.is_control() && !"/\\:*?\"<>|".contains(c)
            }
        })
}
pub fn formats(tools: &Value) -> Vec<&'static str> {
    ["fat32", "exfat", "ext4", "ntfs"]
        .into_iter()
        .filter(|key| tools[*key] == true || tools[*key].as_u64().is_some_and(|n| n > 0))
        .collect()
}
pub fn image_path(path: &str, home: &str) -> String {
    if path == "~" {
        home.into()
    } else if let Some(suffix) = path.strip_prefix("~/") {
        format!("{home}/{suffix}")
    } else {
        path.into()
    }
}
pub fn image_valid(stat: &Value, disk: &Value) -> bool {
    stat.is_object()
        && stat["dir"] != true
        && stat["size"]
            .as_u64()
            .is_some_and(|size| size > 0 && size <= disk["size"].as_u64().unwrap_or(0))
}
pub fn progress(line: &str) -> Option<Value> {
    let raw = line.strip_prefix("progress ")?;
    let mut result = json!({});
    for item in raw.split_whitespace().take(12) {
        let Some((name, value)) = item.split_once('=') else {
            continue;
        };
        match name {
            "phase" => result[name] = json!(value.chars().take(32).collect::<String>()),
            "done" | "total" | "mbps" => {
                if let Ok(n) = value.parse::<f64>() {
                    if n.is_finite() && n >= 0. {
                        result[name] = json!(n);
                    }
                }
            }
            _ => {}
        }
    }
    Some(result)
}
pub fn fs_name(kind: &str) -> &str {
    match kind {
        "hfsplus" => "HFS+",
        "apfs" => "APFS",
        "vfat" | "fat32" => "FAT32",
        "exfat" => "exFAT",
        "ntfs" => "NTFS",
        "iso9660" => "ISO 9660",
        "udf" => "UDF",
        "crypto_LUKS" => "LUKS",
        other => other,
    }
}
pub fn fs_color(kind: &str) -> &'static str {
    match kind {
        "vfat" => "#22c55e",
        "exfat" => "#14b8a6",
        "ntfs" => "#3b82f6",
        "ext4" => "#f97316",
        "ext3" => "#fb923c",
        "ext2" => "#fdba74",
        "btrfs" => "#eab308",
        "xfs" => "#ec4899",
        "swap" => "#8b5cf6",
        "iso9660" => "#a78bfa",
        "crypto_LUKS" => "#ef4444",
        "" => "#9ca3af",
        _ => "#64748b",
    }
}
