//! Axia's build and packaging commands, run from the repository root:
//!
//!   cargo xtask install --prefix /usr --libexecdir /usr/libexec --destdir DIR
//!   cargo xtask dist
//!   cargo xtask srpm --outdir DIR
//!   cargo xtask desktop-binaries
//!
//! This crate runs on the development or build machine only.

mod deploy;
mod dist;
mod install;
mod sha256;
mod srpm;

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub type Result<T> = std::result::Result<T, String>;

/// Adds the path to an I/O error, which alone does not say which file failed.
pub fn at<T>(path: &Path, result: io::Result<T>) -> Result<T> {
    result.map_err(|error| format!("{}: {error}", path.display()))
}

/// Runs a command and treats a non-zero exit as an error.
pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .map_err(|error| format!("{command:?}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed: {status}"))
    }
}

/// The repository root: this crate lives in rust/xtask.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("rust/xtask has two parent directories")
        .to_path_buf()
}

/// Reads `--name value` and `--name=value`. An empty value is valid: the
/// Makefile passes `--destdir ''` for an installation without a stage.
fn options(args: &[String], known: &[&str]) -> Result<HashMap<String, String>> {
    let mut found = HashMap::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let flag = arg
            .strip_prefix("--")
            .ok_or_else(|| format!("unexpected argument: {arg}"))?;
        let (name, value) = match flag.split_once('=') {
            Some((name, value)) => (name, value.to_string()),
            None => {
                let value = args.next().ok_or_else(|| format!("{arg} needs a value"))?;
                (flag, value.clone())
            }
        };
        if !known.contains(&name) {
            return Err(format!("unknown option: --{name}"));
        }
        found.insert(name.to_string(), value);
    }
    Ok(found)
}

fn dispatch(args: &[String]) -> Result<()> {
    let repo = repo();
    let (command, rest) = args
        .split_first()
        .ok_or("usage: cargo xtask <install|deploy|dist|srpm|desktop-binaries>")?;
    match command.as_str() {
        "deploy" => {
            let values = options(rest, &["source", "target", "backups", "server-sha256"])?;
            let get = |name: &str| {
                values.get(name).ok_or_else(|| format!("deploy requires --{name}"))
            };
            deploy::deploy(
                Path::new(get("source")?),
                Path::new(get("target")?),
                Path::new(get("backups")?),
                get("server-sha256")?,
            )
        }
        "install" => {
            let values = options(rest, &["prefix", "libexecdir", "destdir"])?;
            let value = |name: &str, default: &str| {
                values
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| default.to_string())
            };
            install::install(
                &repo,
                &value("prefix", "/usr/local"),
                &value("libexecdir", "/usr/local/libexec"),
                &value("destdir", ""),
            )
        }
        "dist" => {
            options(rest, &[])?;
            dist::dist(&repo).map(|archive| println!("{}", archive.display()))
        }
        "srpm" => {
            let values = options(rest, &["outdir"])?;
            let outdir = values.get("outdir").ok_or("srpm needs --outdir")?;
            srpm::srpm(&repo, Path::new(outdir))
        }
        "desktop-binaries" => {
            options(rest, &[])?;
            for name in install::DESKTOP_BINARIES {
                println!("{name}");
            }
            Ok(())
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

/// A private directory for a test, removed when the test ends.
#[cfg(test)]
pub struct TestDir(pub PathBuf);

#[cfg(test)]
impl TestDir {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("axia-xtask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

#[cfg(test)]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::options;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn options_accept_both_forms_and_an_empty_value() {
        let args = strings(&[
            "--prefix",
            "/usr",
            "--libexecdir=/usr/libexec",
            "--destdir",
            "",
        ]);
        let found = options(&args, &["prefix", "libexecdir", "destdir"]).unwrap();
        assert_eq!(found["prefix"], "/usr");
        assert_eq!(found["libexecdir"], "/usr/libexec");
        assert_eq!(found["destdir"], "");
    }

    #[test]
    fn options_refuse_unknown_flags_positionals_and_missing_values() {
        assert!(options(&strings(&["--other", "x"]), &["prefix"]).is_err());
        assert!(options(&strings(&["loose"]), &["prefix"]).is_err());
        assert!(options(&strings(&["--prefix"]), &["prefix"]).is_err());
    }
}
