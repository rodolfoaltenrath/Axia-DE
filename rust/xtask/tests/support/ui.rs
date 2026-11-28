//! Real compositor checks; Rust owns assertions, process lifetime and evidence.
#![cfg(target_os = "linux")]
use serde_json::Value;
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Session(std::process::Child);
impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGTERM);
        }
        let start = Instant::now();
        while matches!(self.0.try_wait(), Ok(None)) && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(20));
        }
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
// A real PTY must use a login shell from passwd, never a test-only command override.
struct ShellUser(Option<String>);
impl ShellUser {
    fn new(required: bool) -> Self {
        if !required || unsafe { libc::geteuid() } != 0 {
            return Self(None);
        }
        let name = format!("axia-term-{}", std::process::id());
        let status = Command::new("useradd")
            .args(["--system", "--no-create-home", "--user-group", "--shell", "/bin/sh", &name])
            .status().expect("useradd is required for root-run terminal integration");
        assert!(status.success(), "create the isolated terminal test account");
        Self(Some(name))
    }
    fn identity(&self) -> (u32, u32) {
        if let Some(name) = &self.0 {
            let name = std::ffi::CString::new(name.as_str()).unwrap();
            let pw = unsafe { libc::getpwnam(name.as_ptr()) };
            assert!(!pw.is_null());
            unsafe { ((*pw).pw_uid, (*pw).pw_gid) }
        } else {
            (65534, 65534)
        }
    }
}
impl Drop for ShellUser {
    fn drop(&mut self) {
        if let Some(name) = &self.0 {
            let _ = Command::new("userdel").arg(name).status();
        }
    }
}
fn owned(path: &Path, uid: u32, gid: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    if unsafe { libc::geteuid() } == 0 {
        let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::chown(name.as_ptr(), uid, gid) }, 0);
    }
}

#[allow(dead_code)] // Each integration chooses the plain or pixel-checked entry point.
pub fn run_ui(
    repo: &Path,
    kind: &str,
    driver_source: &str,
    mode: &str,
    width: u32,
    image_name: &str,
) -> Value {
    run_ui_with_capture(repo, kind, driver_source, mode, width, image_name, |_| {})
}

pub fn run_ui_with_capture(
    repo: &Path,
    kind: &str,
    driver_source: &str,
    mode: &str,
    width: u32,
    image_name: &str,
    check_capture: impl FnOnce(&Path),
) -> Value {
    run_ui_config(
        repo,
        kind,
        driver_source,
        mode,
        width,
        image_name,
        18,
        check_capture,
        |_| {},
    )
}

#[allow(dead_code)]
pub fn run_ui_with_capture_timeout(
    repo: &Path, kind: &str, driver_source: &str, mode: &str, width: u32,
    image_name: &str, seconds: u64, check_capture: impl FnOnce(&Path),
) -> Value {
    run_ui_config(repo, kind, driver_source, mode, width, image_name, seconds, check_capture, |_| {})
}

#[allow(dead_code)]
pub fn run_ui_with_timeout(
    repo: &Path,
    kind: &str,
    driver_source: &str,
    mode: &str,
    width: u32,
    image_name: &str,
    seconds: u64,
) -> Value {
    run_ui_config(
        repo,
        kind,
        driver_source,
        mode,
        width,
        image_name,
        seconds,
        |_| {},
        |_| {},
    )
}

#[allow(dead_code)]
pub fn run_ui_with_home(
    repo: &Path, kind: &str, driver_source: &str, mode: &str, width: u32,
    image_name: &str, prepare_home: impl FnOnce(&Path),
) -> Value {
    run_ui_config(repo, kind, driver_source, mode, width, image_name, 24, |_| {}, prepare_home)
}

fn run_ui_config(
    repo: &Path,
    kind: &str,
    driver_source: &str,
    mode: &str,
    width: u32,
    image_name: &str,
    seconds: u64,
    check_capture: impl FnOnce(&Path),
    prepare_home: impl FnOnce(&Path),
) -> Value {
    let shell_user = ShellUser::new(kind == "terminal");
    let (uid, gid) = shell_user.identity();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    // Files rejects /var as a volume root. Keep private fixture volumes under
    // /tmp so mounting/ejecting exercises the production path policy too.
    let scratch = Scratch(PathBuf::from(format!(
        "/tmp/axia-{kind}-{}-{stamp}",
        std::process::id()
    )));
    fs::create_dir(&scratch.0).unwrap();
    fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o755)).unwrap();
    let home = scratch.0.join("home");
    for path in [
        home.clone(),
        home.join("runtime"),
        home.join(".config"),
        home.join(".config/axia"),
    ] {
        fs::create_dir(&path).unwrap();
        owned(&path, uid, gid);
    }
    let permissions = home.join(".config/axia/permisos.json");
    fs::write(
        &permissions,
        r#"{"apps":{"screenshots":{"screen":{"state":"allow"}}}}"#,
    )
    .unwrap();
    owned(&permissions, uid, gid);
    prepare_home(&home);
    let image = home.join(image_name);
    let driver = scratch.0.join("probe.jsx");
    fs::write(
        &driver,
        driver_source
            .replace("@ROOT@", repo.to_str().unwrap())
            .replace("@MODE@", mode)
            .replace("@WIDTH@", &width.to_string())
            .replace("@IMAGE@", &serde_json::to_string(&image).unwrap()),
    )
    .unwrap();
    let bundle = scratch.0.join("probe.bundle.js");
    if let Some(prebuilt) = std::env::var_os("AXIA_UI_PREBUILT") {
        let config = serde_json::json!({"mode":mode,"width":width,"image":image,"kind":kind});
        fs::write(
            &bundle,
            format!(
                "globalThis.__AXIA_UI_TEST__={config};\n{}",
                fs::read_to_string(prebuilt).unwrap()
            ),
        )
        .unwrap();
    } else {
        let output = Command::new(repo.join("node_modules/.bin/esbuild"))
            .current_dir(repo)
            .arg(&driver)
            .args([
                "--bundle",
                "--format=iife",
                "--jsx=automatic",
                "--target=es2020",
            ])
            .arg(format!("--outfile={}", bundle.display()))
            .arg(format!(
                "--alias:@lordos/runtime={}/runtime/index.js",
                repo.display()
            ))
            .arg(format!(
                "--alias:@lordos/sdk={}/sdk/index.js",
                repo.display()
            ))
            .args([
                "--define:process.env.NODE_ENV=\"production\"",
                "--define:__LORD_STANDALONE__=true",
                "--define:__AXIA_APP_HOST__=false",
            ])
            .env("NODE_PATH", repo.join("node_modules"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let server = std::env::var_os("AXIA_UI_SERVER")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("server/build/axia-server"));
    let fonts = std::env::var_os("AXIA_UI_FONTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("assets/fonts"));
    let mut command = Command::new(server);
    command
        .current_dir(repo.join("server"))
        .args([
            "--headless",
            &format!("{width}x700"),
            "--timeout",
            &seconds.to_string(),
            "--theme",
        ])
        .arg(&bundle)
        .arg("--fonts")
        .arg(fonts)
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", home.join("runtime"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("LORD_GPU", "0")
        .env("AXIA_NET_OFFLINE", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("PULSE_SERVER")
        .stdout(Stdio::null())
        .stderr(fs::File::create(scratch.0.join("stderr.log")).unwrap())
        .process_group(0);
    if unsafe { libc::geteuid() } == 0 {
        unsafe {
            command.pre_exec(move || {
                if libc::setgroups(0, std::ptr::null()) != 0
                    || libc::setgid(gid) != 0
                    || libc::setuid(uid) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let client = if kind.starts_with("switcher_") {
        std::env::var_os("AXIA_UI_FULLSCREEN_CLIENT").map(|executable| {
            let mut client = Command::new(executable);
            client.arg("switcher-check").arg("--client-home").arg(&home).process_group(0)
                .stdout(Stdio::null()).stderr(fs::File::create(scratch.0.join("client.log")).unwrap());
            if unsafe { libc::geteuid() } == 0 {
                unsafe { client.pre_exec(|| {
                    if libc::setgroups(0,std::ptr::null())!=0 || libc::setgid(65534)!=0 || libc::setuid(65534)!=0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                }); }
            }
            Session(client.spawn().unwrap())
        })
    } else { None };
    let mut session = Session(command.spawn().unwrap());
    let log_path = home.join(".cache/axia/session.log");
    let deadline = Instant::now() + Duration::from_secs(seconds + 2);
    loop {
        let text = fs::read_to_string(&log_path).unwrap_or_default();
        if text.contains(&format!("{kind}-observation "))
            || text.contains(&format!("{kind} driver:"))
            || session.0.try_wait().unwrap().is_some()
            || Instant::now() >= deadline
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    drop(session);
    drop(client);
    let log = fs::read_to_string(&log_path).unwrap_or_default();
    if kind.starts_with("switcher_") && !log.contains(&format!("{kind} driver:")) {
        let client = fs::read_to_string(scratch.0.join("client.log")).unwrap_or_default();
        assert!(client.contains("keyboard isolation: passed"),"Wayland keyboard isolation: {client}\n{log}");
    }
    if let Some(destination) =
        std::env::var_os(&format!("AXIA_{}_OUTPUT", kind.to_ascii_uppercase()))
    {
        let destination = PathBuf::from(destination).join(format!("{mode}-{width}"));
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("session.log"), &log).unwrap();
        fs::copy(scratch.0.join("stderr.log"), destination.join("stderr.log")).unwrap();
        if scratch.0.join("client.log").exists() { fs::copy(scratch.0.join("client.log"),destination.join("client.log")).unwrap(); }
        if image.is_file() {
            fs::copy(&image, destination.join(image_name)).unwrap();
        }
    }
    for error in [
        &format!("{kind} driver:"),
        "TypeError:",
        "ReferenceError:",
        "SyntaxError:",
        "react (error)",
    ] {
        assert!(!log.contains(error), "{mode}/{width}: {log}");
    }
    let report: Value = serde_json::from_str(
        log.lines()
            .find_map(|line| {
                line.split_once(&format!("{kind}-observation "))
                    .map(|(_, value)| value)
            })
            .expect(&log),
    )
    .unwrap();
    let png = fs::read(&image).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), width);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 700);
    check_capture(&image);
    report
}
