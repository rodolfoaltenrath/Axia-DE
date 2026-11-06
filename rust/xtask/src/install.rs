//! Stages the desktop only. No host configuration or services are changed.

use crate::{at, sha256, Result};
use std::collections::BTreeSet;
use std::fs::{self, File, FileTimes, Metadata, Permissions};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

pub const DESKTOP_BINARIES: [&str; 12] = [
    "axia-server",
    "axia-vault",
    "axia-maild",
    "axia-doc",
    "axia-media",
    "axia-lan",
    "axia-rdesk",
    "axia-bench",
    "axia-dl",
    "axia-sysreport",
    "axia-fetch",
    "axia-testbench",
];

const ASSET_DIRS: [&str; 13] = [
    "fonts",
    "locales",
    "shell",
    "icons",
    "brand",
    "monitor",
    "help",
    "wallpapers",
    "gtk",
    "store",
    "hwreport",
    "fw",
    "browser",
];

const REQUIRED: [&str; 6] = [
    "server/build/axia-server",
    "server/build/axia-vault",
    "themes/default/theme.bundle.js",
    "themes/classic/theme.bundle.js",
    "assets/app-host.bundle.js",
    "code-manifest.json",
];

const DOCS: [&str; 8] = [
    "STANDALONE.md",
    "COPR-UPDATES.md",
    "REPOSITORY.md",
    "RUST.md",
    "FEDORA-NETWORK-TRAY.md",
    "DOCK-PREVIEWS.md",
    "FILES.md",
    "SHORTCUTS.md",
];

fn apply_times(path: &Path, from: &Metadata) -> Result<()> {
    let times = FileTimes::new()
        .set_accessed(at(path, from.accessed())?)
        .set_modified(at(path, from.modified())?);
    at(
        path,
        File::open(path).and_then(|file| file.set_times(times)),
    )
}

fn make_parent(path: &Path) -> Result<()> {
    match path.parent() {
        Some(parent) => at(parent, fs::create_dir_all(parent)),
        None => Ok(()),
    }
}

/// Copies a file with its mode and times, following a symbolic link at `src`.
fn copy(src: &Path, dst: &Path) -> Result<()> {
    make_parent(dst)?;
    at(dst, fs::copy(src, dst))?;
    apply_times(dst, &at(src, fs::metadata(src))?)
}

fn copy_with_mode(src: &Path, dst: &Path, mode: u32) -> Result<()> {
    copy(src, dst)?;
    at(dst, fs::set_permissions(dst, Permissions::from_mode(mode)))
}

/// Copies a directory into `dst`, merging with what is there. Symbolic links
/// are followed, so the stage holds real files. A directory's mode and times
/// are applied last, because copying its entries would change them.
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    at(dst, fs::create_dir_all(dst))?;
    for entry in at(src, fs::read_dir(src))? {
        let entry = at(src, entry)?;
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        if at(&from, fs::metadata(&from))?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            copy(&from, &to)?;
        }
    }
    let info = at(src, fs::metadata(src))?;
    apply_times(dst, &info)?;
    at(dst, fs::set_permissions(dst, info.permissions()))
}

/// All the files under `dir`. Links to files count; links to directories are
/// not entered, which also keeps a link cycle from recursing forever.
fn files_under(dir: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for entry in at(dir, fs::read_dir(dir))? {
        let entry = at(dir, entry)?;
        let path = entry.path();
        if at(&path, entry.file_type())?.is_dir() {
            files_under(&path, found)?;
        } else if path.is_file() {
            found.push(path);
        }
    }
    Ok(())
}

fn is_license_name(name: &str) -> bool {
    let name = name.to_uppercase();
    ["LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn check_paths(prefix: &Path, libexecdir: &Path) -> Result<()> {
    if !prefix.is_absolute() || !libexecdir.is_absolute() {
        return Err("prefix and libexecdir must be absolute paths".into());
    }
    let text = format!("{}{}", prefix.display(), libexecdir.display());
    if text
        .chars()
        .any(|c| c.is_whitespace() || "'\"\\".contains(c))
    {
        return Err("installation paths must not contain whitespace or shell quotes".into());
    }
    Ok(())
}

/// Checks the bundles against code-manifest.json and returns the theme names.
fn check_manifest(repo: &Path) -> Result<BTreeSet<String>> {
    let path = repo.join("code-manifest.json");
    let text = at(&path, fs::read_to_string(&path))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    let files = manifest["files"]
        .as_array()
        .ok_or("code-manifest.json has no list of files")?;
    let mut themes = BTreeSet::new();
    let mut checked = Vec::new();
    for file in files {
        let (name, digest) = match (file["path"].as_str(), file["sha256"].as_str()) {
            (Some(name), Some(digest)) => (name, digest),
            _ => return Err("code-manifest.json has an incomplete entry".into()),
        };
        if name.starts_with("themes/greeter/") {
            return Err("stale OS bundles found; rebuild the desktop themes".into());
        }
        if let Some(theme) = name
            .strip_prefix("themes/")
            .and_then(|p| p.split('/').next())
        {
            themes.insert(theme.to_string());
        }
        checked.push((name, digest));
    }
    for (name, digest) in checked {
        match fs::read(repo.join(name)) {
            Ok(data) if sha256::hex(&data) == digest => {}
            _ => {
                return Err(format!(
                    "stale or missing bundle: {name}; rebuild the themes"
                ))
            }
        }
    }
    Ok(themes)
}

pub fn install(repo: &Path, prefix: &str, libexecdir: &str, destdir: &str) -> Result<()> {
    let (prefix, libexecdir) = (Path::new(prefix), Path::new(libexecdir));
    check_paths(prefix, libexecdir)?;
    // Validate the complete install plan before copying any file.
    for rel in REQUIRED {
        if !repo.join(rel).is_file() {
            return Err(format!("missing {rel}; run npm ci && make first"));
        }
    }
    let themes = check_manifest(repo)?;

    let root = libexecdir.join("axia");
    let stage = Path::new(if destdir.is_empty() { "/" } else { destdir });
    let staged = |path: &Path| stage.join(path.strip_prefix("/").expect("checked absolute"));
    let target = staged(&root);
    let share = staged(&prefix.join("share"));
    let bindir = prefix.join("bin");

    for name in DESKTOP_BINARIES {
        let src = repo.join("server/build").join(name);
        if src.is_file() {
            copy_with_mode(&src, &target.join("bin").join(name), 0o755)?;
        }
    }
    for name in ASSET_DIRS {
        copy_tree(
            &repo.join("assets").join(name),
            &target.join("assets").join(name),
        )?;
    }
    // GTK searches the shared data directories, not the compositor's libexec assets.
    for name in ["Axia", "Axia-dark"] {
        copy_tree(
            &repo.join("assets/gtk").join(name),
            &share.join("themes").join(name),
        )?;
    }
    copy(
        &repo.join("assets/app-host.bundle.js"),
        &target.join("assets/app-host.bundle.js"),
    )?;
    copy_tree(&repo.join("assets/sysapps"), &target.join("assets/sysapps"))?;
    for name in &themes {
        let (from, to) = (
            repo.join("themes").join(name),
            target.join("themes").join(name),
        );
        for file in ["theme.json", "theme.bundle.js"] {
            copy(&from.join(file), &to.join(file))?;
        }
        if from.join("assets").exists() {
            copy_tree(&from.join("assets"), &to.join("assets"))?;
        }
    }
    copy(
        &repo.join("code-manifest.json"),
        &target.join("code-manifest.json"),
    )?;
    if repo.join("code-manifest.sig").exists() {
        copy(
            &repo.join("code-manifest.sig"),
            &target.join("code-manifest.sig"),
        )?;
    }

    let session = target.join("fedora-session");
    copy_with_mode(
        &repo.join("tools/desktop/fedora-session.in"),
        &session,
        0o755,
    )?;
    let text = at(&session, fs::read_to_string(&session))?;
    at(
        &session,
        fs::write(
            &session,
            text.replace("@BINDIR@", &bindir.to_string_lossy()),
        ),
    )?;

    let root_text = root.to_string_lossy();
    let launcher = staged(&bindir).join("axia-session");
    let source = repo.join("tools/desktop/axia-session.in");
    make_parent(&launcher)?;
    let text = at(&source, fs::read_to_string(&source))?;
    at(
        &launcher,
        fs::write(&launcher, text.replace("@ROOT@", &root_text)),
    )?;
    at(
        &launcher,
        fs::set_permissions(&launcher, Permissions::from_mode(0o755)),
    )?;

    // The terminal commands find their helpers through their real path in libexec.
    for name in ["axia-fetch", "axia-sysreport"] {
        if target.join("bin").join(name).is_file() {
            let link = staged(&bindir).join(name);
            if fs::symlink_metadata(&link).is_ok() {
                at(&link, fs::remove_file(&link))?;
            }
            at(&link, symlink(root.join("bin").join(name), &link))?;
        }
    }

    let entry = share.join("wayland-sessions/axia.desktop");
    let source = repo.join("packaging/desktop/axia.desktop.in");
    make_parent(&entry)?;
    let text = at(&source, fs::read_to_string(&source))?;
    at(
        &entry,
        fs::write(&entry, text.replace("@ROOT@", &root_text)),
    )?;

    let doc = share.join("doc/axia-de");
    for file in ["LICENSE", "THIRD_PARTY.md", "ORIGIN.md", "README.md"] {
        copy(&repo.join(file), &doc.join(file))?;
    }
    for file in DOCS {
        copy(&repo.join("docs").join(file), &doc.join("docs").join(file))?;
    }

    let licenses = share.join("licenses/axia-de");
    copy(
        &repo.join("server/src/bench/fonts/OFL.txt"),
        &licenses.join("OFL-Noto.txt"),
    )?;
    let third_party = repo.join("third_party");
    let mut found = Vec::new();
    files_under(&third_party, &mut found)?;
    for src in &found {
        let name = src.file_name().unwrap_or_default();
        if ["LICENSE", "LICENCE.md", "LICENSE-SecLists"]
            .iter()
            .any(|n| name == *n)
        {
            let rel = src
                .strip_prefix(&third_party)
                .expect("found under third_party");
            copy(src, &licenses.join("third_party").join(rel))?;
        }
    }
    let vendor = repo.join("rust/vendor");
    found.clear();
    files_under(&vendor, &mut found)?;
    for src in &found {
        if is_license_name(&src.file_name().unwrap_or_default().to_string_lossy()) {
            let rel = src.strip_prefix(&vendor).expect("found under rust/vendor");
            copy(src, &licenses.join("rust").join(rel))?;
        }
    }
    println!("Installed desktop in {}", target.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;
    use std::time::{Duration, SystemTime};

    #[test]
    fn paths_must_be_absolute_and_free_of_shell_characters() {
        let check =
            |prefix: &str, libexec: &str| check_paths(Path::new(prefix), Path::new(libexec));
        assert!(check("/usr", "/usr/libexec").is_ok());
        assert!(check("usr", "/usr/libexec").is_err());
        assert!(check("/usr", "libexec").is_err());
        for bad in ["/us r", "/us'r", "/us\"r", "/us\\r", "/us\tr"] {
            assert!(check(bad, "/usr/libexec").is_err(), "{bad}");
        }
    }

    #[test]
    fn license_names_ignore_case_and_need_the_word_at_the_start() {
        for name in [
            "LICENSE",
            "license-MIT",
            "Licence.md",
            "COPYING",
            "NOTICE.txt",
            "copyright",
        ] {
            assert!(is_license_name(name), "{name}");
        }
        for name in ["UNLICENSE", "lib.rs", "my-LICENSE"] {
            assert!(!is_license_name(name), "{name}");
        }
    }

    #[test]
    fn copy_tree_merges_and_keeps_modes_and_times_and_follows_links() {
        let dir = TestDir::new("copy-tree");
        let (src, dst) = (dir.0.join("src"), dir.0.join("dst"));
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/tool"), "new").unwrap();
        fs::set_permissions(src.join("sub/tool"), Permissions::from_mode(0o750)).unwrap();
        symlink("sub/tool", src.join("link")).unwrap();
        let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        File::open(src.join("sub/tool"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        fs::set_permissions(src.join("sub"), Permissions::from_mode(0o700)).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join("kept"), "kept").unwrap();

        copy_tree(&src, &dst).unwrap();

        let tool = fs::metadata(dst.join("sub/tool")).unwrap();
        assert_eq!(tool.permissions().mode() & 0o7777, 0o750);
        assert_eq!(tool.modified().unwrap(), old);
        assert_eq!(
            fs::metadata(dst.join("sub")).unwrap().permissions().mode() & 0o7777,
            0o700
        );
        assert!(fs::symlink_metadata(dst.join("link")).unwrap().is_file());
        assert_eq!(fs::read_to_string(dst.join("link")).unwrap(), "new");
        assert_eq!(fs::read_to_string(dst.join("kept")).unwrap(), "kept");
    }

    #[test]
    fn manifest_with_a_wrong_digest_or_old_bundle_is_refused() {
        let dir = TestDir::new("manifest");
        let write = |digest: &str, path: &str| {
            let text = format!(r#"{{"files":[{{"path":"{path}","sha256":"{digest}"}}]}}"#);
            fs::write(dir.0.join("code-manifest.json"), text).unwrap();
        };
        fs::create_dir_all(dir.0.join("themes/default")).unwrap();
        fs::write(dir.0.join("themes/default/theme.bundle.js"), "abc").unwrap();
        let good = sha256::hex(b"abc");

        write(&good, "themes/default/theme.bundle.js");
        assert_eq!(
            check_manifest(&dir.0)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            ["default"]
        );
        write(&"0".repeat(64), "themes/default/theme.bundle.js");
        assert!(check_manifest(&dir.0)
            .unwrap_err()
            .starts_with("stale or missing"));
        write(&good, "themes/greeter/theme.bundle.js");
        assert!(check_manifest(&dir.0)
            .unwrap_err()
            .starts_with("stale OS bundles"));
    }
}
