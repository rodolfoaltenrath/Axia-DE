//! `code-manifest.json`: the size and SHA-256 of every file Axia runs as its own code (see
//! docs/SIGNING.md, "Verified system code"). axia-server checks each file against it before loading
//! it (server/src/sec/codesig.c); cargo xtask sign-code signs it with the release key.
//!
//!   cargo xtask code-manifest [--root DIR] [--check]
//!
//! DIR (default: the repository) holds themes/ and assets/ as installed. The output is
//! deterministic: the same files give the same bytes, so a signature made on the maintainer's machine
//! stays valid for every package. If the manifest changes, a code-manifest.sig next to it no longer
//! matches and is removed. `--check` only compares with the existing manifest (exit 1 if it would
//! change).
//!
//! Listed: themes/*/theme.bundle.js, assets/app-host.bundle.js and assets/sysapps/*/app.js. Not
//! listed (data, not code): fonts, locales, icons, images, help articles, theme.json and the system
//! apps' manifest.json.

use super::entries;
use crate::{sha256, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn is_file(path: &Path) -> bool {
    fs::symlink_metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

fn list(dir: &Path) -> Vec<String> {
    entries(dir).iter().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
}

/// The files that count as code, relative to `dir`, in byte order (axia-server compares with strcmp).
pub fn code_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut add = |rel: String| {
        if is_file(&dir.join(&rel)) {
            out.push(rel);
        }
    };
    for theme in list(&dir.join("themes")) {
        add(format!("themes/{theme}/theme.bundle.js"));
    }
    add("assets/app-host.bundle.js".into());
    for app in list(&dir.join("assets/sysapps")) {
        add(format!("assets/sysapps/{app}/app.js"));
    }
    out.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    out
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("a string always serializes")
}

pub fn manifest_text(dir: &Path, version: &str) -> Result<String> {
    let mut files = Vec::new();
    for rel in code_files(dir) {
        let path = dir.join(&rel);
        let data = crate::at(&path, fs::read(&path))?;
        files.push(format!(
            "    {{ \"path\": {}, \"size\": {}, \"sha256\": \"{}\" }}",
            quote(&rel),
            data.len(),
            sha256::hex(&data)
        ));
    }
    Ok(format!(
        "{{\n  \"format\": 1,\n  \"kind\": \"lordos-system-code\",\n  \"version\": {},\n  \"files\": [\n{}\n  ]\n}}\n",
        quote(version),
        files.join(",\n")
    ))
}

/// What a write (or check) of the manifest did: `Some(files)` when it changed.
pub struct Outcome {
    pub changed: bool,
    pub files: usize,
}

pub fn write_manifest(repo: &Path, dir: &Path, check: bool) -> Result<Outcome> {
    let package = repo.join("package.json");
    let package: serde_json::Value =
        serde_json::from_str(&crate::at(&package, fs::read_to_string(&package))?).map_err(|e| e.to_string())?;
    let version = package["version"].as_str().ok_or("package.json has no version")?;
    let text = manifest_text(dir, version)?;
    let files = text.matches("\"path\"").count();
    let (file, sig) = (dir.join("code-manifest.json"), dir.join("code-manifest.sig"));
    if fs::read_to_string(&file).ok().as_deref() == Some(text.as_str()) {
        return Ok(Outcome { changed: false, files });
    }
    if check {
        return Ok(Outcome { changed: true, files });
    }
    crate::at(&file, fs::write(&file, &text))?;
    if sig.exists() {
        crate::at(&sig, fs::remove_file(&sig))?; // it signed a different manifest
    }
    Ok(Outcome { changed: true, files })
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let mut root: PathBuf = repo.to_path_buf();
    let mut check = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--root" => root = std::path::absolute(rest.next().ok_or("--root needs a value")?).map_err(|e| e.to_string())?,
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    let outcome = write_manifest(repo, &root, check)?;
    if check {
        println!("{}", if outcome.changed { "code-manifest.json is out of date" } else { "code-manifest.json is up to date" });
        return Ok(ExitCode::from(u8::from(outcome.changed)));
    }
    println!("code-manifest.json: {} files{}", outcome.files, if outcome.changed { "" } else { " (unchanged)" });
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestDir;

    fn tree(name: &str) -> TestDir {
        let dir = TestDir::new(name);
        let write = |rel: &str, text: &str| {
            let path = dir.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write("themes/b/theme.bundle.js", "b");
        write("themes/a/theme.bundle.js", "a");
        write("themes/a/theme.json", "{}");
        write("themes/c/index.jsx", "not built");
        write("assets/app-host.bundle.js", "host");
        write("assets/sysapps/z/app.js", "z");
        write("assets/sysapps/z/manifest.json", "{}");
        write("assets/other.js", "data");
        dir
    }

    #[test]
    fn only_built_code_is_listed_in_byte_order() {
        let dir = tree("manifest-list");
        assert_eq!(
            code_files(&dir.0),
            ["assets/app-host.bundle.js", "assets/sysapps/z/app.js", "themes/a/theme.bundle.js", "themes/b/theme.bundle.js"]
        );
    }

    #[test]
    fn the_text_is_deterministic_and_has_the_old_layout() {
        let dir = tree("manifest-text");
        let text = manifest_text(&dir.0, "1.2.3").unwrap();
        assert_eq!(text, manifest_text(&dir.0, "1.2.3").unwrap());
        assert!(text.starts_with("{\n  \"format\": 1,\n  \"kind\": \"lordos-system-code\",\n  \"version\": \"1.2.3\",\n  \"files\": [\n    { \"path\": \"assets/app-host.bundle.js\", \"size\": 4, \"sha256\": \""));
        assert!(text.ends_with("\" }\n  ]\n}\n"));
        assert!(text.contains(&sha256::hex(b"host")));
    }

    #[test]
    fn a_changed_manifest_drops_its_signature_and_check_never_writes() {
        let repo = TestDir::new("manifest-repo");
        fs::write(repo.0.join("package.json"), "{\"version\":\"9.9.9\"}").unwrap();
        let dir = tree("manifest-write");
        fs::write(dir.0.join("code-manifest.sig"), "sig").unwrap();
        let checked = write_manifest(&repo.0, &dir.0, true).unwrap();
        assert!(checked.changed && !dir.0.join("code-manifest.json").exists() && dir.0.join("code-manifest.sig").exists());
        let wrote = write_manifest(&repo.0, &dir.0, false).unwrap();
        assert!(wrote.changed && wrote.files == 4);
        assert!(!dir.0.join("code-manifest.sig").exists());
        fs::write(dir.0.join("code-manifest.sig"), "sig").unwrap();
        let again = write_manifest(&repo.0, &dir.0, false).unwrap();
        assert!(!again.changed && dir.0.join("code-manifest.sig").exists());
        fs::write(dir.0.join("assets/app-host.bundle.js"), "host2").unwrap();
        assert!(write_manifest(&repo.0, &dir.0, true).unwrap().changed);
    }
}
