#![cfg(target_os = "linux")]
//! CLI regressions use this Rust executable as the measured build/child.
use std::{
    fs,
    path::PathBuf,
    process::{Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

struct Output(PathBuf);
impl Output {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("axia-build-memory-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn command(&self, role: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command
            .args(["build-memory", "--output"])
            .arg(self.0.join("report.json"))
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "build_fixture", "--nocapture"])
            .env("AXIA_BUILD_MEMORY_FIXTURE", role)
            .env("AXIA_BUILD_MEMORY_PID", self.0.join("pid"));
        command
    }
    fn report(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.0.join("report.json")).unwrap()).unwrap()
    }
    fn assert_code(&self, status: ExitStatus, code: i32) -> serde_json::Value {
        assert_eq!(status.code(), Some(code));
        let report = self.report();
        assert_eq!(report["exit_code"], code);
        report
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn build_fixture() {
    let Ok(role) = std::env::var("AXIA_BUILD_MEMORY_FIXTURE") else {
        return;
    };
    match role.as_str() {
        "fail" => {
            thread::sleep(Duration::from_millis(400));
            std::process::exit(7);
        }
        "signal" => {
            unsafe {
                libc::raise(libc::SIGTERM);
            }
            unreachable!();
        }
        "rustc" => {
            fs::write("/proc/self/comm", "rustc").unwrap();
            let mut bytes = vec![0u8; 16 * 1024 * 1024];
            for page in bytes.chunks_mut(4096) {
                page[0] = 1;
            }
            fs::write(
                std::env::var_os("AXIA_BUILD_MEMORY_PID").unwrap(),
                std::process::id().to_string(),
            )
            .unwrap();
            let milliseconds = std::env::var("AXIA_BUILD_MEMORY_CHILD_MS")
                .map(|value| value.parse().unwrap())
                .unwrap_or(800);
            thread::sleep(Duration::from_millis(milliseconds));
            std::hint::black_box(bytes);
        }
        "tree" | "interrupt" => {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "build_fixture", "--nocapture"])
                .env("AXIA_BUILD_MEMORY_FIXTURE", "rustc");
            if role == "interrupt" {
                command.env("AXIA_BUILD_MEMORY_CHILD_MS", "10000");
            }
            let mut child = command.spawn().unwrap();
            assert!(child.wait().unwrap().success());
        }
        _ => panic!("unknown fixture role"),
    }
}

#[test]
fn failure_code_and_report_are_preserved() {
    let output = Output::new("failure");
    let result = output.command("fail").output().unwrap();
    let report = output.assert_code(result.status, 7);
    assert!(report["peak_process_tree_rss_bytes"].as_u64().unwrap() > 0);
    assert_eq!(report["peak_single_rustc_rss_bytes"], 0);
    assert_eq!(report["sample_interval_seconds"], 0.2);
}

#[test]
fn signal_exit_is_mapped_to_the_shell_convention() {
    let output = Output::new("signal");
    let result = output.command("signal").output().unwrap();
    output.assert_code(result.status, 143);
}

#[test]
fn descendants_and_compiler_rss_are_measured() {
    let output = Output::new("tree");
    let result = output.command("tree").output().unwrap();
    let report = output.assert_code(result.status, 0);
    assert!(report["peak_single_rustc_rss_bytes"].as_u64().unwrap() >= 16 * 1024 * 1024);
    assert!(
        report["peak_process_tree_rss_bytes"].as_u64().unwrap()
            >= report["peak_single_rustc_rss_bytes"].as_u64().unwrap()
    );
}

#[test]
fn missing_program_still_writes_a_diagnostic_report() {
    let output = Output::new("missing");
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["build-memory", "--output"])
        .arg(output.0.join("report.json"))
        .arg("--")
        .arg(output.0.join("missing"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let report = output.report();
    assert!(report["exit_code"].is_null());
    assert!(report["error"].is_string());
}

#[test]
fn cancellation_stops_the_build_group_and_writes_a_report() {
    let output = Output::new("cancel");
    let mut wrapper = output
        .command("interrupt")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let pid_file = output.0.join("pid");
    let descendant = loop {
        if let Ok(text) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = text.parse::<i32>() {
                break pid;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "fixture did not start"
        );
        thread::sleep(Duration::from_millis(10));
    };
    unsafe {
        libc::kill(wrapper.id() as i32, libc::SIGTERM);
    }
    let report = output.assert_code(wrapper.wait().unwrap(), 143);
    assert_eq!(report["interrupted_signal"], libc::SIGTERM);
    // A killed grandchild can remain a zombie briefly until init reaps it.
    let start = Instant::now();
    loop {
        let stat = fs::read_to_string(format!("/proc/{descendant}/stat"));
        if stat.is_err()
            || stat
                .unwrap()
                .rsplit_once(')')
                .unwrap()
                .1
                .trim_start()
                .starts_with('Z')
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "build descendant survived cancellation"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
