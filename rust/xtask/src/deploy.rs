//! Activate a checked local desktop build over an existing installation.
//!
//! Each file is written beside its destination and renamed over it. This also works when
//! the old compositor is running: its executable inode stays alive until that session exits.
//! Existing helpers are preserved, and failed verification restores every changed file.
use crate::{at, sha256, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct Contents {
    bytes: Vec<u8>,
    mode: u32,
}

fn regular(path: &Path) -> Result<Contents> {
    let info = at(path, fs::symlink_metadata(path))?;
    if !info.is_file() {
        return Err(format!("{} must be a regular file", path.display()));
    }
    Ok(Contents {
        bytes: at(path, fs::read(path))?,
        mode: info.permissions().mode() & 0o7777,
    })
}

fn relative(text: &str) -> Result<PathBuf> {
    let path = PathBuf::from(text);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("invalid deployment path: {text}"));
    }
    Ok(path)
}

/// Reject links in the destination path. Directory entries themselves are never replaced.
fn check_destination(root: &Path, relative: &Path) -> Result<()> {
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(info) if info.file_type().is_symlink() => {
                return Err(format!("symlink in deployment path: {}", path.display()))
            }
            Ok(info) if path == root.join(relative) && !info.is_file() => {
                return Err(format!("not a regular destination: {}", path.display()))
            }
            Ok(info) if path != root.join(relative) && !info.is_dir() => {
                return Err(format!("not a directory: {}", path.display()))
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    Ok(())
}

fn plan(source: &Path, server_digest: &str) -> Result<Vec<(PathBuf, Contents)>> {
    let manifest = regular(&source.join("code-manifest.json"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&manifest.bytes).map_err(|e| e.to_string())?;
    if value["format"] != 1 || value["kind"] != "lordos-system-code" {
        return Err("unsupported code manifest".into());
    }
    let entries = value["files"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or("empty code manifest")?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for entry in entries {
        let name = entry["path"].as_str().ok_or("missing code path")?;
        let path = relative(name)?;
        if !name.ends_with(".js")
            || !(name.starts_with("assets/") || name.starts_with("themes/"))
            || !seen.insert(path.clone())
        {
            return Err(format!("unexpected or repeated code path: {name}"));
        }
        check_destination(source, &path)?;
        let mut file = regular(&source.join(&path))?;
        if entry["size"].as_u64() != Some(file.bytes.len() as u64)
            || entry["sha256"].as_str() != Some(sha256::hex(&file.bytes).as_str())
        {
            return Err(format!("code hash/size mismatch: {name}"));
        }
        file.mode = 0o644;
        out.push((path, file));
    }
    check_destination(source, Path::new("bin/axia-server"))?;
    let mut server = regular(&source.join("bin/axia-server"))?;
    if server_digest.len() != 64 || sha256::hex(&server.bytes) != server_digest {
        return Err("compositor hash mismatch".into());
    }
    if !server.bytes.starts_with(b"\x7fELF") {
        return Err("compositor must be an ELF executable".into());
    }
    server.mode = 0o755;
    out.push((PathBuf::from("bin/axia-server"), server));
    out.push((
        PathBuf::from("code-manifest.json"),
        Contents {
            mode: 0o644,
            ..manifest
        },
    ));
    Ok(out)
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn replace(path: &Path, file: &Contents) -> Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or("destination has no parent")?;
    at(parent, fs::create_dir_all(parent))?;
    let name = parent.join(format!(
        ".axia-deploy-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut output = at(
        &name,
        OpenOptions::new().write(true).create_new(true).open(&name),
    )?;
    let temp = Temporary(name);
    at(
        &temp.0,
        output.set_permissions(Permissions::from_mode(file.mode)),
    )?;
    at(&temp.0, output.write_all(&file.bytes))?;
    at(&temp.0, output.sync_all())?;
    drop(output); // Never expose an executable while its replacement is still open for writing.
    at(path, fs::rename(&temp.0, path))?;
    at(parent, File::open(parent).and_then(|p| p.sync_all()))
}

fn apply(
    source: &Path,
    target: &Path,
    backups: &Path,
    digest: &str,
    verify: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    if !backups.is_absolute() {
        return Err("backup directory must be absolute".into());
    }
    let source = at(source, fs::canonicalize(source))?;
    let target = at(target, fs::canonicalize(target))?;
    if source.starts_with(&target) || target.starts_with(&source) || backups.starts_with(&target) {
        return Err("source, target and backup directories must be separate".into());
    }
    let files = plan(&source, digest)?; // Snapshot and verify all input bytes before changing anything.
    let signature = PathBuf::from("code-manifest.sig");
    let mut originals = BTreeMap::new();
    for path in files
        .iter()
        .map(|(p, _)| p)
        .chain(std::iter::once(&signature))
    {
        check_destination(&target, path)?;
        let original = match fs::symlink_metadata(target.join(path)) {
            Ok(_) => Some(regular(&target.join(path))?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        originals.insert(path.clone(), original);
    }
    at(backups, fs::create_dir_all(backups))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let backup = backups.join(format!("rust-deploy-{stamp}-{}", std::process::id()));
    at(&backup, fs::DirBuilder::new().mode(0o700).create(&backup))?;
    for (path, original) in &originals {
        if let Some(file) = original {
            replace(&backup.join(path), file)?;
        }
    }
    let inventory: Vec<_> = originals
        .iter()
        .map(|(path, old)| serde_json::json!({"path":path,"existed":old.is_some()}))
        .collect();
    replace(
        &backup.join("deployment.json"),
        &Contents {
            bytes: serde_json::to_vec_pretty(
                &serde_json::json!({"target":target,"files":inventory}),
            )
            .map_err(|e| e.to_string())?,
            mode: 0o600,
        },
    )?;
    let mut changed = Vec::new();
    let result = (|| {
        for (path, file) in &files {
            // Record first: even an error syncing the directory after a successful rename rolls back.
            changed.push(path.clone());
            replace(&target.join(path), file)?;
        }
        if originals[&signature].is_some() {
            changed.push(signature.clone());
            at(&target, fs::remove_file(target.join(&signature)))?;
        }
        verify(&target)?;
        for (path, file) in &files {
            if regular(&target.join(path))?.bytes != file.bytes {
                return Err(format!("installed bytes differ: {}", path.display()));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failed = Vec::new();
        for path in changed.iter().rev() {
            let restored = match &originals[path] {
                Some(file) => replace(&target.join(path), file),
                None => match fs::remove_file(target.join(path)) {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e.to_string()),
                },
            };
            if let Err(e) = restored {
                failed.push(e);
            }
        }
        return Err(format!(
            "{error}; backup: {}; rollback: {}",
            backup.display(),
            if failed.is_empty() {
                "restored".into()
            } else {
                failed.join("; ")
            }
        ));
    }
    println!(
        "Installed and verified {} files. Backup: {}",
        files.len(),
        backup.display()
    );
    Ok(backup)
}

pub fn deploy(source: &Path, target: &Path, backups: &Path, digest: &str) -> Result<()> {
    apply(source, target, backups, digest, |target| {
        let result = Command::new(target.join("bin/axia-server"))
            .args(["--check-code"])
            .arg(target)
            .output()
            .map_err(|e| e.to_string())?;
        if !result.status.success() {
            return Err(format!(
                "installed compositor verification failed: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        Ok(())
    })
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;
    use std::os::unix::fs::symlink;
    fn fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf, String) {
        let (source, target, backups) = (
            root.join("source"),
            root.join("target"),
            root.join("backups"),
        );
        for dir in [&source, &target] {
            fs::create_dir_all(dir.join("bin")).unwrap();
            fs::create_dir_all(dir.join("themes/default")).unwrap();
        }
        fs::copy("/bin/true", source.join("bin/axia-server")).unwrap();
        let digest = sha256::hex(&fs::read(source.join("bin/axia-server")).unwrap());
        fs::write(source.join("themes/default/theme.bundle.js"), b"new bundle").unwrap();
        let manifest = serde_json::json!({"format":1,"kind":"lordos-system-code","files":[{"path":"themes/default/theme.bundle.js","size":10,"sha256":sha256::hex(b"new bundle")}]});
        fs::write(source.join("code-manifest.json"), manifest.to_string()).unwrap();
        fs::write(target.join("bin/axia-server"), b"old executable").unwrap();
        fs::write(target.join("bin/axia-media"), b"native host helper").unwrap();
        fs::write(target.join("code-manifest.json"), b"old manifest").unwrap();
        fs::write(target.join("code-manifest.sig"), b"old signature").unwrap();
        (source, target, backups, digest)
    }
    #[test]
    fn installs_over_nonempty_directories_without_replacing_helpers() {
        let d = TestDir::new("deploy-existing");
        let (source, target, backups, digest) = fixture(&d.0);
        let backup = apply(&source, &target, &backups, &digest, |_| Ok(())).unwrap();
        assert_eq!(
            fs::read(target.join("themes/default/theme.bundle.js")).unwrap(),
            b"new bundle"
        );
        assert_eq!(
            fs::read(target.join("bin/axia-media")).unwrap(),
            b"native host helper"
        );
        assert!(!target.join("code-manifest.sig").exists());
        assert_eq!(
            fs::read(backup.join("code-manifest.sig")).unwrap(),
            b"old signature"
        );
        assert_eq!(
            fs::metadata(target.join("bin/axia-server"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
    #[test]
    fn failed_verification_restores_files_signature_and_absence() {
        let d = TestDir::new("deploy-rollback");
        let (source, target, backups, digest) = fixture(&d.0);
        let error = apply(&source, &target, &backups, &digest, |_| {
            Err("simulated verification failure".into())
        })
        .unwrap_err();
        assert!(error.contains("rollback: restored"));
        assert_eq!(
            fs::read(target.join("bin/axia-server")).unwrap(),
            b"old executable"
        );
        assert_eq!(
            fs::read(target.join("code-manifest.json")).unwrap(),
            b"old manifest"
        );
        assert_eq!(
            fs::read(target.join("code-manifest.sig")).unwrap(),
            b"old signature"
        );
        assert!(!target.join("themes/default/theme.bundle.js").exists());
    }
    #[test]
    fn corrupt_input_and_symlink_destinations_fail_before_modification() {
        let d = TestDir::new("deploy-invalid");
        let (source, target, backups, digest) = fixture(&d.0);
        assert!(apply(&source, &target, &backups, &"0".repeat(64), |_| Ok(())).is_err());
        assert!(!backups.exists());
        let external = d.0.join("external");
        fs::write(&external, b"untouched").unwrap();
        symlink(&external, target.join("themes/default/theme.bundle.js")).unwrap();
        assert!(apply(&source, &target, &backups, &digest, |_| Ok(())).is_err());
        assert_eq!(fs::read(external).unwrap(), b"untouched");
        assert!(!backups.exists());
    }
    #[test]
    fn running_executable_is_replaced_by_rename() {
        let d = TestDir::new("deploy-running");
        let (source, target, backups, digest) = fixture(&d.0);
        fs::copy("/bin/sleep", target.join("bin/axia-server")).unwrap();
        struct Running(std::process::Child);
        impl Drop for Running {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut running = Running(
            Command::new(target.join("bin/axia-server"))
                .arg("30")
                .spawn()
                .unwrap(),
        );
        assert!(fs::write(
            target.join("bin/axia-server"),
            b"cannot overwrite an executing inode"
        )
        .is_err());
        apply(&source, &target, &backups, &digest, |_| Ok(())).unwrap();
        assert!(running.0.try_wait().unwrap().is_none());
        assert_eq!(
            sha256::hex(&fs::read(target.join("bin/axia-server")).unwrap()),
            digest
        );
    }
}
