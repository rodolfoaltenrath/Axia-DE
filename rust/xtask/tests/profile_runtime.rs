//! Manual comparison of the three release archives in an actual private session.
#![cfg(target_os = "linux")]
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[test]
#[ignore = "requires three profile-check reports, build dependencies and a private staged installation"]
fn release_profiles_link_and_run_private_sessions() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let results =
        PathBuf::from(std::env::var_os("AXIA_PROFILE_RESULTS").expect("AXIA_PROFILE_RESULTS"));
    let stage = PathBuf::from(std::env::var_os("AXIA_PROFILE_STAGE").expect("AXIA_PROFILE_STAGE"));
    // Require an explicit marker before replacing any staged executable.
    assert!(
        stage.join(".axia-profile-stage").is_file(),
        "use a disposable marked stage"
    );
    let mut reports = vec![];
    for variant in ["fedora", "thin", "cgu8"] {
        let build: Value =
            serde_json::from_slice(&fs::read(results.join(format!("{variant}.json"))).unwrap())
                .unwrap();
        assert_eq!(build["exit_code"], 0);
        let executable = repo.join("server/build/axia-server");
        // Archives precede these links; removing only the generated executable
        // prevents Make from reusing a link against the preceding variant.
        if executable.exists() {
            fs::remove_file(&executable).unwrap();
        }
        let log_path = results.join(format!("{variant}-link.log"));
        let log = fs::File::create(&log_path).unwrap();
        let status = Command::new("make")
            .current_dir(repo)
            .args([
                "-j2",
                "-C",
                "server",
                "build/axia-server",
                "GPU=1",
                "CFLAGS=-O2 -g -D_FORTIFY_SOURCE=3 -fstack-protector-strong",
            ])
            .arg(format!(
                "RUST_TARGET_DIR={}",
                build["target_directory"].as_str().unwrap()
            ))
            .env("RUSTFLAGS", build["rustflags"].as_str().unwrap())
            .env(
                "CARGO_PROFILE_RELEASE_LTO",
                build["profile"]["lto"].as_str().unwrap(),
            )
            .env(
                "CARGO_PROFILE_RELEASE_CODEGEN_UNITS",
                build["profile"]["codegen_units"].as_str().unwrap(),
            )
            .env("CARGO_BUILD_JOBS", "2")
            .env("CARGO_INCREMENTAL", "0")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTC_WRAPPER")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .status()
            .unwrap();
        assert!(
            status.success(),
            "{variant} link failed: {}",
            log_path.display()
        );
        fs::copy(&executable, stage.join("bin/axia-server")).unwrap();
        let probe = results.join(format!("{variant}-idle.json"));
        let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["idle-check", "--installation"])
            .arg(&stage)
            .arg("--output")
            .arg(&probe)
            .output()
            .unwrap();
        let idle: Value = serde_json::from_slice(&fs::read(&probe).unwrap()).unwrap();
        reports.push(json!({"variant":variant,"server_bytes":fs::metadata(&executable).unwrap().len(),"idle":idle}));
        fs::write(
            results.join("runtime.json"),
            serde_json::to_string_pretty(&reports).unwrap() + "\n",
        )
        .unwrap();
        assert!(
            result.status.success(),
            "{variant}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(reports.last().unwrap()["idle"]["status"], "passed");
        println!("{variant}: {}", reports.last().unwrap()["idle"]["summary"]);
    }
}
