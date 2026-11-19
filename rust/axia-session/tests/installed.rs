//! Manual integration with the actual staged compositor and signed bundles.
use serde_json::json;
use std::{
    fs,
    io::Read,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Private(PathBuf);
impl Drop for Private {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Group(Child);
impl Drop for Group {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.wait();
        }
    }
}
fn own(path: &Path) {
    if unsafe { libc::geteuid() } == 0 {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::chown(c.as_ptr(), 65534, 65534) }, 0);
        if path.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                own(&entry.unwrap().path());
            }
        }
    }
}
fn wrapper_rss(pid: u32, depth: usize) -> Option<u64> {
    if depth == 0 {
        return None;
    }
    let base = PathBuf::from(format!("/proc/{pid}"));
    if fs::read_to_string(base.join("comm")).ok()?.trim() == "fedora-session" {
        return fs::read_to_string(base.join("status"))
            .ok()?
            .lines()
            .find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
                    .map(|kb| kb * 1024)
            });
    }
    fs::read_to_string(base.join(format!("task/{pid}/children")))
        .ok()?
        .split_whitespace()
        .filter_map(|s| s.parse::<u32>().ok())
        .find_map(|child| wrapper_rss(child, depth - 1))
}

#[test]
#[ignore = "requires AXIA_SESSION_TEST_STAGE containing the staged native session binaries and desktop"]
fn installed_rust_session_starts_the_real_native_calculator() {
    let stage = PathBuf::from(
        std::env::var_os("AXIA_SESSION_TEST_STAGE").expect("AXIA_SESSION_TEST_STAGE"),
    );
    for relative in ["fedora-session", "bin/axia-session"] {
        let mut magic = [0; 4];
        fs::File::open(stage.join(relative))
            .unwrap()
            .read_exact(&mut magic)
            .unwrap();
        assert_eq!(&magic, b"\x7fELF", "legacy session script remains");
    }
    assert!(
        !stage.join("bin/axia-session-fixture").exists(),
        "test fixture was installed"
    );
    let home = Private(
        std::env::temp_dir().join(format!("axia-installed-session-{}", std::process::id())),
    );
    fs::create_dir(&home.0).unwrap();
    for path in ["runtime", ".config/axia"] {
        fs::create_dir_all(home.0.join(path)).unwrap();
    }
    fs::set_permissions(home.0.join("runtime"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(home.0.join(".config/user-dirs.dirs"), "").unwrap();
    fs::write(home.0.join(".config/axia/settings.json"), json!({"schema":1,"theme":"default","welcomeShown":true,"debugTiming":true,"sessionRestore":"always","sessionSaved":{"clean":true,"windows":[{"id":"app:calculator","f":{"x":20,"y":30,"w":400,"h":400}}]}}).to_string()).unwrap();
    own(&home.0);
    let mut command = Command::new(stage.join("fedora-session"));
    command
        .args(["--headless", "1180x700", "--timeout", "4"])
        .env("HOME", &home.0)
        .env("XDG_CONFIG_HOME", home.0.join(".config"))
        .env("XDG_CACHE_HOME", home.0.join(".cache"))
        .env("XDG_STATE_HOME", home.0.join(".local/state"))
        .env("XDG_DATA_HOME", home.0.join(".local/share"))
        .env("XDG_RUNTIME_DIR", home.0.join("runtime"))
        .env("LORD_GPU", "0")
        .env("AXIA_NET_OFFLINE", "1")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env_remove("PULSE_SERVER")
        .env_remove("PIPEWIRE_REMOTE")
        .env_remove("XAUTHORITY")
        .env_remove("DBUS_SYSTEM_BUS_ADDRESS")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if unsafe { libc::geteuid() } == 0 {
        command
            .uid(65534)
            .gid(65534)
            .env("USER", "nobody")
            .env("LOGNAME", "nobody");
    }
    let mut group = Group(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut rss = vec![];
    let status = loop {
        if let Some(status) = group.0.try_wait().unwrap() {
            break status;
        }
        if let Some(value) = wrapper_rss(group.0.id(), 5) {
            rss.push(value);
        }
        assert!(Instant::now() < deadline, "native session failed to exit");
        std::thread::sleep(Duration::from_millis(100));
    };
    let log = fs::read_to_string(home.0.join(".cache/axia/session.log")).unwrap_or_default();
    let report = json!({"exit_code":status.code(),"wrapper_rss_samples_bytes":rss,"calculator_committed":log.contains("timing: calculator window committed"),"scope":"private software session; not physical presentation latency"});
    if let Some(output) = std::env::var_os("AXIA_SESSION_TEST_OUTPUT") {
        let output = PathBuf::from(output);
        fs::create_dir_all(&output).unwrap();
        fs::write(
            output.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        fs::write(output.join("session.log"), &log).unwrap();
    }
    assert!(status.success(), "{report}\n{log}");
    assert!(report["calculator_committed"] == true, "{report}\n{log}");
    for error in [
        "ReferenceError:",
        "TypeError:",
        "SyntaxError:",
        "code refused",
        "react (error)",
    ] {
        assert!(!log.contains(error), "{error}\n{log}");
    }
    assert!(!rss.is_empty(), "native wrapper was not observed");
    println!("{report}");
}
