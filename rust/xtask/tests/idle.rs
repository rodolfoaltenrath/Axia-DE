//! Exercise the public command and its report, without a script test runner.
use std::{fs, path::PathBuf, process::Command};

struct Output(PathBuf);
impl Output {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("axia-idle-cli-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn report(&self, name: &str) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.0.join(name)).unwrap()).unwrap()
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn probe(installation: &std::path::Path, output: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .arg("idle-check")
        .arg("--installation")
        .arg(installation)
        .arg("--output")
        .arg(output);
    command
}

#[test]
fn invalid_calibration_exits_nonzero_with_a_report() {
    let output = Output::new("missing");
    let result = probe(&output.0.join("missing"), &output.0.join("report.json"))
        .args(["--enforce", "false"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(output.report("report.json")["status"], "invalid");
}

#[cfg(target_os = "linux")]
#[test]
fn early_session_failure_preserves_diagnostics() {
    let output = Output::new("early-exit");
    let installation = output.0.join("installation");
    for path in ["bin", "themes/default", "assets/fonts"] {
        fs::create_dir_all(installation.join(path)).unwrap();
    }
    // A real executable that exits immediately, without a shell fixture.
    fs::copy("/usr/bin/false", installation.join("bin/axia-server")).unwrap();
    fs::write(installation.join("themes/default/theme.bundle.js"), "").unwrap();
    let result = probe(&installation, &output.0.join("report.json"))
        .args([
            "--warmup",
            "0.05",
            "--duration",
            "0.05",
            "--samples",
            "1",
            "--enforce",
            "false",
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(output.report("report.json")["status"], "invalid");
    assert!(output
        .0
        .join("report-samples/sample-1/stderr.log")
        .is_file());
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires strace and AXIA_IDLE_TEST_INSTALLATION pointing to a staged desktop"]
fn real_session_calibration_and_enforcement() {
    let installation = PathBuf::from(
        std::env::var_os("AXIA_IDLE_TEST_INSTALLATION").expect("AXIA_IDLE_TEST_INSTALLATION"),
    );
    let output = Output::new("real");
    // Three normal samples prove the default budget against the actual desktop.
    let result = probe(&installation, &output.0.join("normal.json"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report = output.report("normal.json");
    assert_eq!(report["status"], "passed");
    assert_eq!(report["samples"].as_array().unwrap().len(), 3);
    println!("real idle summary: {}", report["summary"]);
    for (name, enforce, expected_exit) in [
        ("enforced.json", "true", 1),
        ("calibration.json", "false", 0),
    ] {
        let result = probe(&installation, &output.0.join(name))
            .args([
                "--warmup",
                "2",
                "--duration",
                "3",
                "--samples",
                "1",
                "--max-polls-per-second",
                "0.01",
                "--enforce",
                enforce,
            ])
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(expected_exit),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report = output.report(name);
        assert_eq!(report["status"], "exceeded");
        assert!(report["summary"]["exceeded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "poll_calls_per_second"));
        let stem = std::path::Path::new(name)
            .file_stem()
            .unwrap()
            .to_string_lossy();
        assert!(output
            .0
            .join(format!("{stem}-samples/sample-1/poll.trace"))
            .is_file());
    }
    if let Some(destination) = std::env::var_os("AXIA_IDLE_TEST_OUTPUT") {
        fs::create_dir_all(&destination).unwrap();
        for name in ["normal.json", "enforced.json", "calibration.json"] {
            fs::copy(output.0.join(name), PathBuf::from(&destination).join(name)).unwrap();
        }
    }
}
