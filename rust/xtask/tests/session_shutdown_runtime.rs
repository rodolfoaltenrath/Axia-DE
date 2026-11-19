//! The installed shell must exit successfully with native apps still mounted.
#![cfg(target_os = "linux")]
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Process(std::process::Child);
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}
fn own(path: &std::path::Path) {
    if unsafe { libc::geteuid() } == 0 {
        let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::chown(path.as_ptr(), 65534, 65534) }, 0);
    }
}

#[test]
#[ignore = "requires built desktop and bundles; required by both desktop CI renderers"]
fn installed_shell_exits_cleanly_with_restored_native_apps() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = Workspace(PathBuf::from(format!(
        "/tmp/axia-shutdown-{}-{stamp}",
        std::process::id()
    )));
    fs::create_dir(&workspace.0).unwrap();
    fs::set_permissions(&workspace.0, fs::Permissions::from_mode(0o755)).unwrap();
    let staged = workspace.0.join("stage");
    let install = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(&repo)
        .args([
            "install",
            "--prefix",
            "/usr",
            "--libexecdir",
            "/usr/libexec",
            "--destdir",
        ])
        .arg(&staged)
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let root = staged.join("usr/libexec/axia");
    let theme = root.join("themes/default/theme.bundle.js");
    let mut bundle = fs::read(&theme).unwrap();
    bundle.extend_from_slice(b"\nsetTimeout(() => { const sdk = __lordos.require('@lordos/sdk'); sdk.wm.open('forcequit'); sdk.wm.open('appabout', {args:{key:'calculator'}}); }, 700);\n");
    fs::write(&theme, &bundle).unwrap();
    // This is a private development installation, without a production manifest.
    fs::remove_file(root.join("code-manifest.json")).unwrap();
    for signal in [false, true] {
        let home = workspace.0.join(if signal { "signal" } else { "timeout" });
        fs::create_dir(&home).unwrap();
        for path in [
            home.clone(),
            home.join("runtime"),
            home.join(".config"),
            home.join(".config/axia"),
        ] {
            if path != home {
                fs::create_dir(&path).unwrap();
            }
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            own(&path);
        }
        let settings = home.join(".config/axia/settings.json");
        let apps = ["store", "settings", "files", "calculator"];
        let saved: Vec<_> = apps.iter().map(|app| serde_json::json!({"id":format!("app:{app}"),
            "f":{"x":20,"y":30,"w":700,"h":500},"args":if *app=="settings" {serde_json::json!({"section":"wallpaper"})} else {serde_json::json!({})}})).collect();
        fs::write(
            &settings,
            serde_json::to_vec(&serde_json::json!({"schema":1,"debugTiming":true,
            "sessionRestore":"always","sessionSaved":{"clean":true,"windows":saved}}))
            .unwrap(),
        )
        .unwrap();
        own(&settings);
        let stderr_path = home.join("stderr.log");
        let mut command = Command::new(root.join("bin/axia-server"));
        command
            .current_dir(root.join("bin"))
            .args([
                "--headless",
                "800x600",
                "--timeout",
                if signal { "20" } else { "5" },
                "--theme",
                "../themes/default/theme.bundle.js",
            ])
            .env("HOME", &home)
            .env("USER", "nobody")
            .env("XDG_RUNTIME_DIR", home.join("runtime"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env(
                "XDG_DATA_DIRS",
                format!(
                    "{}:/usr/local/share:/usr/share",
                    staged.join("usr/share").display()
                ),
            )
            .env("RUST_BACKTRACE", "1")
            .env_remove("SUDO_USER")
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(fs::File::create(&stderr_path).unwrap());
        unsafe {
            let root_user = libc::geteuid() == 0;
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if root_user
                    && (libc::setgroups(0, std::ptr::null()) != 0
                        || libc::setgid(65534) != 0
                        || libc::setuid(65534) != 0)
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut process = Process(command.spawn().unwrap());
        let log_path = home.join(".cache/axia/session.log");
        let start = Instant::now();
        let mut sent = false;
        let status = loop {
            if let Some(status) = process.0.try_wait().unwrap() {
                break status;
            }
            if signal && !sent && start.elapsed() >= Duration::from_secs(5) {
                assert_eq!(
                    unsafe { libc::kill(process.0.id() as i32, libc::SIGTERM) },
                    0
                );
                sent = true;
            }
            assert!(
                start.elapsed() < Duration::from_secs(25),
                "shell did not exit"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        let log = fs::read_to_string(&log_path).unwrap_or_default();
        let stderr = fs::read_to_string(&stderr_path).unwrap();
        if let Some(output) = std::env::var_os("AXIA_SHUTDOWN_OUTPUT") {
            fs::create_dir_all(&output).unwrap();
            let name = if signal { "signal" } else { "timeout" };
            fs::write(
                PathBuf::from(&output).join(format!("{name}-session.log")),
                &log,
            )
            .unwrap();
            fs::write(
                PathBuf::from(output).join(format!("{name}-stderr.log")),
                &stderr,
            )
            .unwrap();
        }
        assert!(
            status.success(),
            "signal={signal}, {status}:\n{stderr}\n{log}"
        );
        assert!(!signal || sent, "signal case exited before SIGTERM was sent");
        assert!(!stderr.contains("panicked"), "{stderr}");
        for app in apps.into_iter().chain(["forcequit", "appabout"]) {
            assert!(
                log.contains(&format!("{app} window committed")),
                "missing {app}: {log}"
            );
        }
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
        assert_eq!(saved["sessionSaved"]["clean"], true, "{saved}");
    }
}
