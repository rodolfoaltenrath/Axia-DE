//! Real Rust subprocesses, isolated homes, sockets, signals and XDG tools.
use axia_session::{environment as policy, Environment};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::{
        fs::{symlink, MetadataExt, PermissionsExt},
        net::UnixListener,
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Private(PathBuf);
impl Private {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "axia-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn env(&self) -> Environment {
        let mut env: Environment = std::env::vars_os().collect();
        for (key, value) in [
            ("HOME", self.0.clone()),
            ("XDG_CONFIG_HOME", self.0.join(".config")),
            ("XDG_CACHE_HOME", self.0.join("cache")),
        ] {
            env.insert(key.into(), value.into_os_string());
        }
        env.insert("LC_ALL".into(), "C".into());
        env.insert("LANG".into(), "C".into());
        env
    }
}
impl Drop for Private {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_for(mut ready: impl FnMut() -> bool, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "private session deadline exceeded"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
struct Running(Option<Child>);
fn spawn(command: &mut Command) -> Child {
    // Parallel tests may fork while another thread has a copied fixture open
    // for writing. The inherited CLOEXEC descriptor closes at that fork's
    // exec; wait only for this transient Linux ETXTBSY, never retry a running
    // process or an application failure.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match command.spawn() {
            Ok(child) => return child,
            Err(error)
                if error.raw_os_error() == Some(libc::ETXTBSY) && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(error) => panic!("{command:?}: {error}"),
        }
    }
}
impl Running {
    fn finish(mut self) -> Output {
        wait_for(
            || self.0.as_mut().unwrap().try_wait().unwrap().is_some(),
            Duration::from_secs(8),
        );
        self.0.take().unwrap().wait_with_output().unwrap()
    }
    fn signal(&self, signal: i32) {
        assert_eq!(
            unsafe { libc::kill(self.0.as_ref().unwrap().id() as i32, signal) },
            0
        );
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct Stage {
    _private: Private,
    root: PathBuf,
    home: PathBuf,
    commands: PathBuf,
}
fn own(path: &Path) {
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    let path_c =
        std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str())).unwrap();
    assert_eq!(unsafe { libc::lchown(path_c.as_ptr(), 65534, 65534) }, 0);
    if path.is_dir() && !path.is_symlink() {
        for entry in fs::read_dir(path).unwrap() {
            own(&entry.unwrap().path());
        }
    }
}
impl Stage {
    fn new() -> Self {
        let private = Private::new();
        let root = private.0.join("bundle with spaces");
        let home = private.0.join("home");
        let commands = private.0.join("commands");
        for path in [
            root.join("bin"),
            root.join("themes/default"),
            home.join("runtime"),
            home.join(".config/axia"),
            commands.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::set_permissions(home.join("runtime"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(home.join(".config/user-dirs.dirs"), "").unwrap();
        fs::write(
            home.join(".config/axia/settings.json"),
            "{\"theme\":\"default\"}",
        )
        .unwrap();
        fs::write(root.join("themes/default/theme.bundle.js"), "test theme").unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_fedora-session"),
            root.join("fedora-session"),
        )
        .unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_axia-session"),
            root.join("bin/axia-session"),
        )
        .unwrap();
        symlink(root.join("bin/axia-session"), commands.join("axia-session")).unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_axia-session-fixture"),
            root.join("bin/axia-server"),
        )
        .unwrap();
        for name in [
            "dbus-run-session",
            "dbus-update-activation-environment",
            "notify-send",
            "xdg-user-dirs-update",
        ] {
            fs::hard_link(root.join("bin/axia-server"), commands.join(name)).unwrap();
        }
        own(&private.0);
        Self {
            _private: private,
            root,
            home,
            commands,
        }
    }
    fn start(&self, desktop: bool, bus: bool, extra: &[(&str, &str)]) -> Running {
        let executable = if desktop {
            self.root.join("fedora-session")
        } else {
            self.commands.join("axia-session")
        };
        let mut command = Command::new(executable);
        if !desktop {
            command.args(["--fixture-argument", "argument with spaces"]);
        }
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_RUNTIME_DIR", self.home.join("runtime"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_CACHE_HOME", self.home.join("cache"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.commands.display()))
            .env("LANG", "pt_BR.UTF-8")
            .env("DISPLAY", ":999")
            .envs(extra.iter().copied())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if bus {
            command.env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/private-test-bus");
        }
        if unsafe { libc::geteuid() } == 0 {
            command.uid(65534).gid(65534);
        }
        Running(Some(spawn(&mut command)))
    }
    fn record(&self, name: &str) -> Value {
        let path = self.home.join(name);
        wait_for(
            || {
                fs::read(&path)
                    .ok()
                    .is_some_and(|s| serde_json::from_slice::<Value>(&s).is_ok())
            },
            Duration::from_secs(5),
        );
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }
}

#[test]
fn theme_selection_uses_home_and_rejects_invalid_or_oversized_settings() {
    let private = Private::new();
    let config = private.0.join(".config/axia");
    let bundle = private.0.join("themes/custom/theme.bundle.js");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(bundle.parent().unwrap()).unwrap();
    fs::write(&bundle, "test").unwrap();
    let mut env = private.env();
    env.insert(
        "XDG_CONFIG_HOME".into(),
        private.0.join("different-config").into(),
    );
    assert_eq!(policy::theme(&env, &private.0), None);
    fs::write(config.join("settings.json"), "{\"theme\":\"custom\"}").unwrap();
    assert_eq!(policy::theme(&env, &private.0), Some(bundle));
    for value in [
        json!("../other"),
        json!("/tmp/theme"),
        json!(""),
        json!("a".repeat(65)),
        Value::Null,
        json!([]),
        json!("missing"),
        json!("classic"),
    ] {
        fs::write(
            config.join("settings.json"),
            json!({"theme":value}).to_string(),
        )
        .unwrap();
        assert_eq!(policy::theme(&env, &private.0), None);
    }
    for text in [
        "{broken".into(),
        "null".into(),
        "[]".into(),
        format!("{{\"theme\":\"{}\"}}", "a".repeat(1024 * 1024)),
    ] {
        fs::write(config.join("settings.json"), text).unwrap();
        assert_eq!(policy::theme(&env, &private.0), None);
    }
}

#[test]
fn logs_are_private_rotated_and_fail_without_overwriting_other_files() {
    let private = Private::new();
    let env = private.env();
    let mut file = policy::session_log(&env).unwrap();
    writeln!(file, "previous session").unwrap();
    drop(file);
    let mut file = policy::session_log(&env).unwrap();
    writeln!(file, "new session").unwrap();
    drop(file);
    let directory = private.0.join("cache/axia");
    assert_eq!(
        fs::read_to_string(directory.join("session.previous.log")).unwrap(),
        "previous session\n"
    );
    assert_eq!(
        fs::metadata(directory.join("session.log")).unwrap().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::metadata(&directory).unwrap().mode() & 0o777, 0o700);
    let mut bad = env.clone();
    let path = private.0.join("not-directory");
    fs::write(&path, "keep").unwrap();
    bad.insert("XDG_CACHE_HOME".into(), path.clone().into());
    assert!(policy::session_log(&bad).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "keep");
}

#[test]
fn socket_scan_and_xwayland_detection_require_owned_real_sockets() {
    let private = Private::new();
    let _wayland = UnixListener::bind(private.0.join("wayland-0")).unwrap();
    fs::write(private.0.join("wayland-0.lock"), "stale").unwrap();
    fs::write(private.0.join("wayland-1"), "file").unwrap();
    assert_eq!(policy::sockets(&private.0), ["wayland-0".into()].into());
    fs::create_dir(private.0.join(".X11-unix")).unwrap();
    let _x = UnixListener::bind(private.0.join(".X11-unix/X77")).unwrap();
    fs::write(
        private.0.join(".X77-lock"),
        format!(" {}\n", std::process::id()),
    )
    .unwrap();
    let uid = unsafe { libc::getuid() };
    assert_eq!(
        policy::x_display(std::process::id(), &private.0, uid),
        ":77"
    );
    assert!(policy::x_display(std::process::id() + 1, &private.0, uid).is_empty());
    assert!(policy::x_display(std::process::id(), &private.0, uid + 1).is_empty());
    fs::remove_file(private.0.join(".X11-unix/X77")).unwrap();
    fs::write(private.0.join(".X11-unix/X77"), "file").unwrap();
    assert!(policy::x_display(std::process::id(), &private.0, uid).is_empty());
}

#[test]
fn direct_launcher_execs_relocatable_server_with_or_without_a_bus() {
    for bus in [true, false] {
        let stage = Stage::new();
        let child = stage.start(false, bus, &[("AXIA_TEST_SLEEP_MS", "10")]);
        let record = stage.record("child.json");
        let output = child.finish();
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(
            record["cwd"],
            stage.root.join("bin").to_string_lossy().as_ref()
        );
        assert_eq!(record["env"]["XDG_CURRENT_DESKTOP"], "Axia");
        assert_eq!(record["env"]["XDG_SESSION_TYPE"], "wayland");
        assert_eq!(
            record["args"],
            json!(["--fixture-argument", "argument with spaces"])
        );
        assert_eq!(
            record["env"]["DBUS_SESSION_BUS_ADDRESS"],
            "unix:path=/private-test-bus"
        );
    }
}

#[test]
fn desktop_publishes_wayland_without_xwayland_and_rotates_real_child_logs() {
    for bus in [true, false] {
        let stage = Stage::new();
        let child = stage.start(true, bus, &[("AXIA_TEST_SOCKET", "1")]);
        let published = stage.record("dbus-update-activation-environment.json");
        let child_record = stage.record("child.json");
        let output = child.finish();
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(published["env"]["WAYLAND_DISPLAY"], "wayland-7");
        assert_eq!(published["env"]["DISPLAY"], "");
        assert_eq!(
            child_record["args"],
            json!(["--theme", stage.root.join("themes/default/theme.bundle.js")])
        );
        let log = fs::read_to_string(stage.home.join("cache/axia/session.log")).unwrap();
        assert!(log.contains("test compositor stdout") && log.contains("test compositor stderr"));
        let child = stage.start(true, true, &[("AXIA_TEST_SLEEP_MS", "10")]);
        assert!(child.finish().status.success());
        assert!(stage.home.join("cache/axia/session.previous.log").is_file());
    }
}

#[test]
fn publication_and_log_failures_do_not_prevent_the_session() {
    let stage = Stage::new();
    fs::write(stage.home.join("cache"), "keep").unwrap();
    own(&stage.home);
    let child = stage.start(
        true,
        true,
        &[("AXIA_TEST_SOCKET", "1"), ("AXIA_TEST_PUBLISH_EXIT", "7")],
    );
    stage.record("dbus-update-activation-environment.json");
    let output = child.finish();
    assert!(output.status.success(), "{:?}", output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("could not open session log"));
    assert!(stderr.contains("could not publish session environment"));
    assert_eq!(
        fs::read_to_string(stage.home.join("cache")).unwrap(),
        "keep"
    );
}

#[test]
fn early_server_failure_preserves_exit_status_without_waiting_for_discovery() {
    let stage = Stage::new();
    let start = Instant::now();
    let output = stage
        .start(
            true,
            true,
            &[("AXIA_TEST_SLEEP_MS", "0"), ("AXIA_TEST_EXIT", "7")],
        )
        .finish();
    assert_eq!(output.status.code(), Some(7));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn server_exit_during_environment_publication_does_not_leave_the_supervisor_waiting() {
    let stage = Stage::new();
    let start = Instant::now();
    let child = stage.start(
        true,
        true,
        &[
            ("AXIA_TEST_SOCKET", "1"),
            ("AXIA_TEST_SLEEP_MS", "100"),
            ("AXIA_TEST_HELPER_SLEEP_MS", "400"),
            ("AXIA_TEST_EXIT", "7"),
        ],
    );
    stage.record("dbus-update-activation-environment.json");
    assert_eq!(child.finish().status.code(), Some(7));
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
fn termination_is_forwarded_during_discovery_and_after_publication() {
    for (socket, signal) in [("0", libc::SIGINT), ("1", libc::SIGTERM)] {
        let stage = Stage::new();
        let child = stage.start(
            true,
            true,
            &[
                ("AXIA_TEST_SOCKET", socket),
                ("AXIA_TEST_SLEEP_MS", "60000"),
            ],
        );
        let record = stage.record("child.json");
        if socket == "1" {
            stage.record("dbus-update-activation-environment.json");
        }
        child.signal(signal);
        let output = child.finish();
        assert_eq!(output.status.code(), Some(128 + signal), "{:?}", output);
        assert!(
            !Path::new(&format!("/proc/{}", record["pid"].as_u64().unwrap())).exists(),
            "server was left running"
        );
    }
}

#[test]
fn user_directories_are_initialized_once_and_custom_or_disabled_choices_survive() {
    let private = Private::new();
    let env = private.env();
    policy::initialize_user_dirs(&env).unwrap();
    let config = private.0.join(".config/user-dirs.dirs");
    let before = fs::read(&config).unwrap();
    for name in [
        "Downloads",
        "Documents",
        "Desktop",
        "Pictures",
        "Music",
        "Videos",
    ] {
        assert!(private.0.join(name).is_dir(), "{name}");
    }
    policy::initialize_user_dirs(&env).unwrap();
    assert_eq!(before, fs::read(&config).unwrap());
    let content = "XDG_DOCUMENTS_DIR=\"$HOME/Custom/Missing\"\nXDG_DOWNLOAD_DIR=\"$HOME\"\n";
    fs::write(&config, content).unwrap();
    policy::initialize_user_dirs(&env).unwrap();
    assert_eq!(fs::read_to_string(config).unwrap(), content);
}

#[test]
fn existing_localized_folders_keep_names_and_contents() {
    let private = Private::new();
    for name in ["Music", "Pictures", "Documentos"] {
        fs::create_dir(private.0.join(name)).unwrap();
        fs::write(private.0.join(name).join("keep.txt"), "keep").unwrap();
    }
    policy::initialize_user_dirs(&private.env()).unwrap();
    let config = fs::read_to_string(private.0.join(".config/user-dirs.dirs")).unwrap();
    for (key, name) in [
        ("MUSIC", "Music"),
        ("PICTURES", "Pictures"),
        ("DOCUMENTS", "Documentos"),
    ] {
        assert!(config.contains(&format!("XDG_{key}_DIR=\"$HOME/{name}\"")));
        assert_eq!(
            fs::read_to_string(private.0.join(name).join("keep.txt")).unwrap(),
            "keep"
        );
    }
    assert!(!private.0.join("Documents").exists());
}

#[test]
fn replacement_detection_ignores_live_and_missing_processes() {
    assert!(!policy::executable_replaced(std::process::id()));
    assert!(!policy::executable_replaced(u32::MAX));
    let private = Private::new();
    let executable = private.0.join("sleep");
    fs::copy("/usr/bin/sleep", &executable).unwrap();
    let child = Running(Some(spawn(Command::new(&executable).arg("60"))));
    let pid = child.0.as_ref().unwrap().id();
    assert!(!policy::executable_replaced(pid));
    fs::copy("/usr/bin/sleep", private.0.join("replacement")).unwrap();
    fs::rename(private.0.join("replacement"), executable).unwrap();
    assert!(policy::executable_replaced(pid));
}

#[test]
fn update_messages_follow_locale_precedence_with_english_fallback() {
    let mut env = Environment::new();
    assert!(policy::update_message(&env).0.contains("was updated"));
    env.insert("LANG".into(), "pt_BR.UTF-8".into());
    assert!(policy::update_message(&env).0.contains("foi atualizado"));
    env.insert("LC_MESSAGES".into(), "es_ES.UTF-8".into());
    assert!(policy::update_message(&env).0.contains("se ha actualizado"));
    env.insert("LC_ALL".into(), "C".into());
    assert!(policy::update_message(&env).0.contains("was updated"));
}

#[test]
fn installed_replacement_is_advisory_even_when_notification_fails() {
    let stage = Stage::new();
    let child = stage.start(
        true,
        true,
        &[
            ("AXIA_TEST_SOCKET", "1"),
            ("AXIA_TEST_SLEEP_MS", "60000"),
            ("AXIA_TEST_PUBLISH_EXIT", "7"),
        ],
    );
    stage.record("dbus-update-activation-environment.json");
    fs::copy(
        env!("CARGO_BIN_EXE_axia-session-fixture"),
        stage.root.join("bin/replacement"),
    )
    .unwrap();
    fs::rename(
        stage.root.join("bin/replacement"),
        stage.root.join("bin/axia-server"),
    )
    .unwrap();
    wait_for(
        || stage.home.join("notify-send.json").is_file(),
        Duration::from_secs(40),
    );
    let notify = stage.record("notify-send.json");
    assert!(notify["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s == "Axia DE foi atualizado"));
    assert!(child.0.as_ref().unwrap().id() > 0);
    assert!(Path::new(&format!("/proc/{}", child.0.as_ref().unwrap().id())).exists());
    child.signal(libc::SIGTERM);
    assert_eq!(child.finish().status.code(), Some(143));
}

#[test]
fn missing_publication_program_does_not_abort_the_desktop() {
    let stage = Stage::new();
    fs::remove_file(stage.commands.join("dbus-update-activation-environment")).unwrap();
    let path = stage.commands.to_str().unwrap();
    let child = stage.start(true, true, &[("AXIA_TEST_SOCKET", "1"), ("PATH", path)]);
    let output = child.finish();
    assert!(output.status.success(), "{:?}", output);
    let log = fs::read_to_string(stage.home.join("cache/axia/session.log")).unwrap();
    assert!(log.contains("could not publish session environment"));
}

#[test]
fn missing_login_runtime_is_rejected_before_starting_a_compositor() {
    for desktop in [false, true] {
        let stage = Stage::new();
        let output = stage
            .start(desktop, true, &[("XDG_RUNTIME_DIR", "")])
            .finish();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("login session"));
        assert!(!stage.home.join("child.json").exists());
    }
}

#[test]
#[ignore = "requires root in the isolated Fedora CI container"]
fn both_entrypoints_reject_root_before_starting_a_compositor() {
    assert_eq!(
        unsafe { libc::geteuid() },
        0,
        "run this test in the rootless Fedora container"
    );
    for binary in [
        env!("CARGO_BIN_EXE_axia-session"),
        env!("CARGO_BIN_EXE_fedora-session"),
    ] {
        let output = Command::new(binary)
            .env_remove("XDG_RUNTIME_DIR")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("regular user"));
    }
}
