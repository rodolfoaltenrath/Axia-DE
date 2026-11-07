//! Prepares the RPM source archive, with the frontend built from the lockfile.

use crate::{at, run, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PATHS: [&str; 33] = [
    ".cargo",
    "server",
    "rust",
    "runtime",
    "sdk",
    "apps",
    "themes",
    "assets",
    "third_party",
    "tests",
    "tools/desktop",
    "tools/build-theme.mjs",
    "tools/code-manifest.mjs",
    "packaging/desktop",
    "packaging/rpm",
    "packaging/systemd",
    "Makefile",
    "package.json",
    "package-lock.json",
    "code-manifest.json",
    "LICENSE",
    "THIRD_PARTY.md",
    "ORIGIN.md",
    "README.md",
    "docs/STANDALONE.md",
    "docs/COPR-UPDATES.md",
    "docs/REPOSITORY.md",
    "docs/RUST.md",
    "docs/SHORTCUTS.md",
    "docs/FILES.md",
    "docs/FEDORA-NETWORK-TRAY.md",
    "docs/DOCK-PREVIEWS.md",
    "docs/CLIPBOARD.md",
];

/// Matched by GNU tar against each whole path component, at any depth.
const EXCLUDED_NAMES: [&str; 6] = [
    ".git",
    "node_modules",
    "build",
    "dist",
    "__pycache__",
    "target",
];
const EXCLUDED_ENDINGS: [&str; 6] = ["*.rpm", "*.deb", "*.iso", "*.img", "*.lpk", "*.pyc"];

/// The version in package.json, which names the archive and must match the spec.
pub fn version(repo: &Path) -> Result<String> {
    let path = repo.join("package.json");
    let text = at(&path, fs::read_to_string(&path))?;
    let package: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    package["version"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "package.json has no version".to_string())
}

fn tar_command(repo: &Path, archive: &Path, top: &str) -> Command {
    let mut tar = Command::new("tar");
    tar.arg("--create").arg("--gzip").arg("--file").arg(archive);
    tar.arg("--directory").arg(repo);
    // Puts everything under the top folder. The S flag leaves link targets alone.
    tar.arg(format!("--transform=s,^,{top}/,S"));
    for pattern in EXCLUDED_NAMES.iter().chain(&EXCLUDED_ENDINGS) {
        tar.arg(format!("--exclude={pattern}"));
    }
    tar.arg("--");
    tar.args(PATHS);
    tar
}

pub fn dist(repo: &Path) -> Result<PathBuf> {
    run(Command::new("npm").arg("ci").current_dir(repo))?;
    run(Command::new("npm").args(["run", "build"]).current_dir(repo))?;
    let top = format!("axia-de-{}", version(repo)?);
    let out = repo.join("dist");
    at(&out, fs::create_dir_all(&out))?;
    let archive = out.join(format!("{top}.tar.gz"));
    run(&mut tar_command(repo, &archive, &top))?;
    Ok(archive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;
    use std::os::unix::fs::symlink;

    #[test]
    fn archive_skips_generated_files_and_keeps_links_and_similar_names() {
        let dir = TestDir::new("dist");
        let repo = dir.0.join("repo");
        for sub in [
            "server/build",
            "rust/target",
            "rust/axia-shell",
            "tools/desktop/__pycache__",
            "tools",
        ] {
            fs::create_dir_all(repo.join(sub)).unwrap();
        }
        for file in [
            "server/build/binary",
            "server/main.c",
            "rust/target/binary",
            "rust/axia-shell/lib.rs",
            "tools/desktop/__pycache__/a.pyc",
            "tools/desktop/kept.txt",
            "tools/desktop/old.rpm",
            "tools/build-theme.mjs",
        ] {
            fs::write(repo.join(file), "x").unwrap();
        }
        symlink("main.c", repo.join("server/link")).unwrap();
        // tar needs every listed path; these tests care only about the ones above.
        for path in PATHS {
            let path = repo.join(path);
            if !path.exists() {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, "x").unwrap();
            }
        }
        let archive = dir.0.join("out.tar.gz");
        run(&mut tar_command(&repo, &archive, "axia-de-1.0")).unwrap();

        let list = Command::new("tar")
            .arg("-tzvf")
            .arg(&archive)
            .output()
            .unwrap();
        let list = String::from_utf8(list.stdout).unwrap();
        for kept in [
            "axia-de-1.0/server/main.c",
            "axia-de-1.0/rust/axia-shell/lib.rs",
            "axia-de-1.0/tools/desktop/kept.txt",
            "axia-de-1.0/tools/build-theme.mjs",
            "axia-de-1.0/server/link -> main.c",
        ] {
            assert!(list.contains(kept), "{kept} missing from:\n{list}");
        }
        for gone in ["build/binary", "target/binary", "__pycache__", "old.rpm"] {
            assert!(!list.contains(gone), "{gone} found in:\n{list}");
        }
    }
}
