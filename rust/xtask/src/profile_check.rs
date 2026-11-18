//! Reproduce one cold shell build without changing the project's release profile.
use crate::{at, options, Result};
use serde_json::json;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

fn output(command: &mut Command) -> Result<String> {
    let result = command.output().map_err(|e| format!("{command:?}: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "{command:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

pub fn execute(repo: &Path, args: &[String]) -> Result<()> {
    let values = options(args, &["variant", "target-dir", "output"])?;
    let required = |name| {
        values
            .get(name)
            .ok_or_else(|| format!("profile-check requires --{name}"))
    };
    let variant = required("variant")?;
    let (lto, units) = match variant.as_str() {
        "fedora" => ("false", "1"),
        "thin" => ("thin", "1"),
        "cgu8" => ("false", "8"),
        _ => return Err("--variant must be fedora, thin or cgu8".into()),
    };
    let target = Path::new(required("target-dir")?);
    if !target.is_absolute() {
        return Err("--target-dir must be absolute".into());
    }
    if target.exists() {
        return Err(
            "profile-check requires a new target directory; refusing artifact reuse".into(),
        );
    }
    let destination = Path::new(required("output")?);
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    at(parent, fs::create_dir_all(parent))?;
    let flags = output(Command::new("rpm").args(["--eval", "%{build_rustflags}"]))?;
    if flags.is_empty() || flags.contains("%{") {
        return Err("Fedora build_rustflags are unavailable".into());
    }
    let rustflags = if units == "1" {
        flags.clone()
    } else {
        format!("{flags} -Ccodegen-units={units}")
    };
    let revision = output(
        Command::new("git")
            .current_dir(repo)
            .args(["rev-parse", "HEAD"]),
    )?;
    let dirty = output(
        Command::new("git")
            .current_dir(repo)
            .args(["status", "--porcelain"]),
    )?;
    let rustc = output(Command::new("rustc").arg("-vV"))?;
    let cargo = output(Command::new("cargo").arg("--version"))?;
    if let Some(parent) = target.parent() {
        at(parent, fs::create_dir_all(parent))?;
    }
    at(target, fs::create_dir(target))?;
    let log_path = destination.with_extension("log");
    let memory_path = destination.with_extension("memory.json");
    let log = at(&log_path, fs::File::create(&log_path))?;
    let stderr = at(&log_path, log.try_clone())?;
    let command = [
        "build",
        "--manifest-path",
        "rust/Cargo.toml",
        "--package",
        "axia-shell",
        "--release",
        "--locked",
        "--offline",
        "--message-format=json-render-diagnostics",
        "--target-dir",
    ];
    println!(
        "Building {variant}: lto={lto}, codegen-units={units}; {}",
        log_path.display()
    );
    let status = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .current_dir(repo)
        .args(["build-memory", "--output"])
        .arg(&memory_path)
        .arg("--")
        .arg("cargo")
        .args(command)
        .arg(target)
        .env("RUSTFLAGS", &rustflags)
        .env("CARGO_BUILD_JOBS", "2")
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_PROFILE_RELEASE_LTO", lto)
        .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", units)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .status()
        .map_err(|e| e.to_string())?;
    let mut compiled = vec![];
    let mut fresh = vec![];
    for line in at(&log_path, fs::read_to_string(&log_path))?.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value["reason"] == "compiler-artifact" {
            if value["fresh"] == true {
                fresh.push(value["target"]["name"].clone());
            } else {
                compiled.push(value["target"]["name"].clone());
            }
        }
    }
    let memory = fs::read(&memory_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let archive = target.join("release/libaxia_shell.a");
    let report = json!({"variant":variant,"revision":revision,"working_tree_changes":dirty,
        "rustc":rustc,"cargo":cargo,"fedora_rustflags":flags,"rustflags":rustflags,
        "profile":{"lto":lto,"codegen_units":units,"jobs":2,"incremental":false},
        "target_directory":target,"archive_bytes":fs::metadata(&archive).ok().map(|m|m.len()),
        "exit_code":status.code(),"memory":memory,"compiled":compiled,"fresh":fresh,
        "scope":"cold axia-shell release target, warm OS cache; not a full CI or runtime comparison"});
    at(
        destination,
        fs::write(
            destination,
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n",
        ),
    )?;
    if !status.success() {
        return Err(format!(
            "{variant} build failed; inspect {}",
            destination.display()
        ));
    }
    if !fresh.is_empty() {
        return Err("unexpected fresh artifacts in a cold build; inspect report".into());
    }
    println!("{variant}: {}", report["memory"]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_target_is_refused_without_modifying_it() {
        let temp = crate::TestDir::new("profile-existing");
        let sentinel = temp.0.join("keep");
        fs::write(&sentinel, "existing artifacts").unwrap();
        let report = temp.0.join("report.json");
        let args = vec![
            "--variant".into(),
            "fedora".into(),
            "--target-dir".into(),
            temp.0.to_str().unwrap().into(),
            "--output".into(),
            report.to_str().unwrap().into(),
        ];
        assert!(execute(&temp.0, &args)
            .unwrap_err()
            .contains("refusing artifact reuse"));
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "existing artifacts");
        assert!(!report.exists());
    }
    #[test]
    fn relative_targets_and_unknown_variants_are_rejected() {
        let repo = crate::TestDir::new("profile-options");
        for variant in ["fedora", "unknown"] {
            let args = [
                "--variant",
                variant,
                "--target-dir",
                "relative",
                "--output",
                "unused",
            ]
            .map(str::to_owned);
            assert!(execute(&repo.0, &args).is_err());
        }
    }
}
