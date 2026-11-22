//! Checks that the built and staged artifacts form an independent desktop.
//!
//!   cargo xtask desktop-check [--require-gpu]
//!   cargo xtask clipboard-x11-check
//!
//! `desktop-check` stages the installation in a temporary directory, verifies the code manifest and
//! that no OS component is linked, and then runs private headless sessions of the staged
//! axia-server (without root or a seat) with each integration fixture in turn: the app restore,
//! the native clipboard peer, the wallpaper bridge, the clipboard history, the Dock previews and
//! an isolated app on the Rust scheduler. It validates neither DRM/KMS nor physical input.
//! `clipboard-x11-check` does the same for XWayland transfers, with an independent Xlib peer.
//!
//! Needs the built bundles and server (`cargo xtask build-theme`, `make -C server`), `nm`, `make`,
//! and for the X11 check Xwayland and the X11 development library.

use crate::{at, install, sha256, Result};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

const NOBODY: u32 = 65534;
const ERRORS: [&str; 5] = ["ReferenceError:", "TypeError:", "SyntaxError:", "react (error)", "code refused"];

/// A private temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(prefix: &str) -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!("{prefix}{}-{nonce:x}", std::process::id()));
        at(&path, fs::DirBuilder::new().mode(0o700).create(&path))?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

fn chown(path: &Path) {
    if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) {
        // SAFETY: `c` is a valid NUL-terminated path.
        unsafe { libc::chown(c.as_ptr(), NOBODY, NOBODY) };
    }
}

fn chown_tree(path: &Path) {
    chown(path);
    if path.is_dir() {
        for entry in super::entries(path) {
            chown_tree(&entry.path());
        }
    }
}

/// When running as root, the session runs as the unprivileged `nobody` account.
fn unprivileged(command: &mut Command) {
    if is_root() {
        // SAFETY: only async-signal-safe libc calls between fork and exec.
        unsafe {
            command.pre_exec(|| {
                libc::setgroups(0, std::ptr::null());
                libc::setgid(NOBODY);
                libc::setuid(NOBODY);
                Ok(())
            });
        }
    }
}

struct Output {
    stdout: String,
    stderr: String,
    ok: bool,
}

fn finish(mut child: Child, out: &Path, err: &Path, timeout: Duration, what: &str) -> Result<Output> {
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{what} did not finish in {} s", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let read = |p: &Path| String::from_utf8_lossy(&fs::read(p).unwrap_or_default()).into_owned();
    Ok(Output { stdout: read(out), stderr: read(err), ok: status.success() })
}

/// Runs a command to the end, capturing its output; kills it after `timeout`.
fn run_for(scratch: &Scratch, command: &mut Command, timeout: Duration, what: &str) -> Result<Output> {
    let (out, err) = (scratch.0.join("run.out"), scratch.0.join("run.err"));
    let child = command
        .stdout(at(&out, fs::File::create(&out))?)
        .stderr(at(&err, fs::File::create(&err))?)
        .spawn()
        .map_err(|e| format!("{what}: {e}"))?;
    finish(child, &out, &err, timeout, what)
}

fn run(command: &mut Command) -> Result<String> {
    let output = command.output().map_err(|e| format!("{command:?}: {e}"))?;
    if !output.status.success() {
        return Err(format!("{command:?} failed: {}\n{}", output.status, String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ---- PNG, to feed the wallpaper bridge (one valid, one broken, one that claims a huge size) ----

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn chunk(kind: &[u8; 4], value: &[u8]) -> Vec<u8> {
    let mut body = kind.to_vec();
    body.extend_from_slice(value);
    let mut out = (value.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
    out
}

fn png(w: u32, h: u32, pixels: &[u8]) -> Vec<u8> {
    let mut header = w.to_be_bytes().to_vec();
    header.extend_from_slice(&h.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut zlib = ZlibEncoder::new(Vec::new(), Compression::default());
    zlib.write_all(pixels).expect("writing to memory");
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &header));
    out.extend(chunk(b"IDAT", &zlib.finish().expect("writing to memory")));
    out.extend(chunk(b"IEND", &[]));
    out
}

// ---- the staged installation ----

struct Stage {
    root: PathBuf,
    /// `usr/libexec/axia` inside the staging directory
    staged: PathBuf,
    manifest: Value,
}

impl Stage {
    fn bin(&self) -> PathBuf {
        self.staged.join("bin")
    }
    fn server(&self) -> PathBuf {
        self.staged.join("bin/axia-server")
    }

    fn write_manifest(&self) -> Result<()> {
        let path = self.staged.join("code-manifest.json");
        at(&path, fs::write(&path, self.manifest.to_string()))
    }

    /// Replaces a staged file and its entry in the (disposable) code manifest.
    fn replace(&mut self, rel: &str, data: &[u8]) -> Result<()> {
        let path = self.staged.join(rel);
        at(&path, fs::write(&path, data))?;
        let files = self.manifest["files"].as_array_mut().ok_or("the manifest has no files")?;
        let entry = files.iter_mut().find(|e| e["path"] == rel).ok_or_else(|| format!("{rel} is not in the manifest"))?;
        entry["size"] = json!(data.len());
        entry["sha256"] = json!(sha256::hex(data));
        self.write_manifest()
    }
}

fn stage(repo: &Path, scratch: &Scratch) -> Result<Stage> {
    install::install(repo, "/usr", "/usr/libexec", &scratch.0.to_string_lossy())?;
    let staged = scratch.0.join("usr/libexec/axia");
    let text = at(&staged, fs::read_to_string(staged.join("code-manifest.json")))?;
    let manifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(Stage { root: scratch.0.clone(), staged, manifest })
}

/// A private home and the environment of a user session without a seat or a bus.
struct Home {
    dir: PathBuf,
    env: HashMap<String, String>,
}

fn home(dir: &Path, staged_share: &Path) -> Result<Home> {
    use std::os::unix::fs::DirBuilderExt;
    let runtime = dir.join("runtime");
    at(&runtime, fs::DirBuilder::new().mode(0o700).create(&runtime))?;
    let config = dir.join(".config/axia");
    at(&config, fs::create_dir_all(&config))?;
    let mut env: HashMap<String, String> = std::env::vars().collect();
    let d = |p: &str| dir.join(p).to_string_lossy().into_owned();
    for (k, v) in [
        ("HOME", dir.to_string_lossy().into_owned()),
        ("USER", "nobody".into()),
        ("XDG_RUNTIME_DIR", d("runtime")),
        ("XDG_DATA_HOME", d(".local/share")),
        ("XDG_CONFIG_HOME", d(".config")),
        ("XDG_DATA_DIRS", format!("{}:/usr/local/share:/usr/share", staged_share.display())),
        ("XDG_CACHE_HOME", d(".cache")),
        ("XDG_STATE_HOME", d(".local/state")),
    ] {
        env.insert(k.into(), v);
    }
    for key in ["SUDO_USER", "DBUS_SESSION_BUS_ADDRESS", "WAYLAND_DISPLAY", "DISPLAY"] {
        env.remove(key);
    }
    if is_root() {
        for p in [dir.to_path_buf(), runtime, dir.join(".config"), config] {
            chown(&p);
        }
    }
    Ok(Home { dir: dir.to_path_buf(), env })
}

impl Home {
    fn prefs(&self) -> PathBuf {
        self.dir.join(".config/axia/settings.json")
    }
    fn write_prefs(&self, value: Value) -> Result<()> {
        let path = self.prefs();
        at(&path, fs::write(&path, value.to_string()))?;
        if is_root() {
            chown(&path);
        }
        Ok(())
    }
    fn log(&self) -> String {
        String::from_utf8_lossy(&fs::read(self.dir.join(".cache/axia/session.log")).unwrap_or_default()).into_owned()
    }
    fn server(&self, stage: &Stage, args: &[&str]) -> Command {
        let mut command = Command::new(stage.server());
        command.args(args).current_dir(stage.bin()).env_clear().envs(&self.env).stdin(Stdio::null());
        unprivileged(&mut command);
        command
    }
}

const THEME: &str = "../themes/default/theme.bundle.js";

/// Runs a private headless session to its end; a failure carries stderr and the session log.
fn session(scratch: &Scratch, home: &Home, stage: &Stage, size: &str, seconds: &str, timeout: u64) -> Result<String> {
    let mut command = home.server(stage, &["--headless", size, "--timeout", seconds, "--theme", THEME]);
    let output = run_for(scratch, &mut command, Duration::from_secs(timeout), "the headless session")?;
    if !output.ok {
        return Err(format!("{}\n{}", output.stderr, home.log()));
    }
    Ok(home.log())
}

fn no_errors(log: &str, errors: &[&str]) -> Result<()> {
    for error in errors {
        ensure!(!log.contains(error), "{error} in the session log:\n{log}");
    }
    Ok(())
}

fn passed(log: &str, lines: &[&str]) -> Result<()> {
    for line in lines {
        ensure!(log.contains(line), "missing \"{line}\" in the session log:\n{log}");
    }
    Ok(())
}

pub fn execute(repo: &Path, args: &[String]) -> Result<ExitCode> {
    let require_gpu = args.iter().any(|a| a == "--require-gpu");
    if let Some(bad) = args.iter().find(|a| *a != "--require-gpu") {
        return Err(format!("unexpected argument: {bad}"));
    }
    check_artifacts(repo, require_gpu)?;
    let work = Scratch::new("axia-stage-")?;
    let mut stage = stage(repo, &work)?;
    check_staging(repo, &stage)?;
    fs::set_permissions(&stage.root, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    let share = stage.staged.parent().and_then(Path::parent).ok_or("bad staging layout")?.join("share");
    let tmp = Scratch::new("axia-home-")?;
    let home = home(&tmp.0, &share)?;
    let run_dir = Scratch::new("axia-run-")?;
    restore_apps(&run_dir, &home, &mut stage)?;
    clipboard_native(repo, &run_dir, &home, &stage)?;
    wallpaper(repo, &run_dir, &home, &mut stage)?;
    clipboard_history(repo, &run_dir, &home, &mut stage)?;
    dock_previews(repo, &run_dir, &home, &mut stage)?;
    scheduler_app(repo, &run_dir, &home, &mut stage)?;
    println!("desktop: no OS backend, manifest valid, installation isolated, headless startup passed");
    Ok(ExitCode::SUCCESS)
}

fn check_artifacts(repo: &Path, require_gpu: bool) -> Result<()> {
    let server = repo.join("server/build/axia-server");
    let symbols = run(Command::new("nm").arg(&server))?;
    if require_gpu {
        ensure!(symbols.contains(" U eglInitialize"), "RPM compositor was built without GPU support");
    }
    for symbol in [
        "pkg_act_install", "pkg_act_update", "install_act_run", "net_act_restore", "act_langpack", "js_pkg_register",
        "js_imager_register", "pin_load", "pkg_act_source_add", "share_act_mount", "clean_act_apt", "js_clean_register",
        "clean_start", "ssh_act_root",
    ] {
        ensure!(!symbols.contains(symbol), "OS component linked: {symbol}");
    }
    ensure!(symbols.contains("libseat_open_seat"), "desktop must use a host seat backend");
    let manifest_path = repo.join("code-manifest.json");
    let manifest: Value = serde_json::from_str(&at(&manifest_path, fs::read_to_string(&manifest_path))?).map_err(|e| e.to_string())?;
    for file in manifest["files"].as_array().ok_or("the manifest has no files")? {
        let rel = file["path"].as_str().unwrap_or("");
        let data = at(&repo.join(rel), fs::read(repo.join(rel)))?;
        ensure!(file["size"] == data.len(), "{rel}: the size does not match code-manifest.json");
        ensure!(file["sha256"] == sha256::hex(&data), "{rel}: the hash does not match code-manifest.json");
        ensure!(!rel.starts_with("themes/greeter/"), "{rel}: a greeter theme is not part of the desktop");
    }
    let bundle_path = repo.join("themes/default/theme.bundle.js");
    let bundle = String::from_utf8_lossy(&at(&bundle_path, fs::read(&bundle_path))?).into_owned();
    ensure!(!bundle.contains("pkg-install-file"), "Debian store bundled into desktop");
    ensure!(!bundle.contains("install-run"), "OS installer bundled into desktop");
    Ok(())
}

fn check_staging(repo: &Path, stage: &Stage) -> Result<()> {
    let staged = &stage.staged;
    let installed: std::collections::BTreeSet<String> =
        super::entries(&staged.join("bin")).iter().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let expected: std::collections::BTreeSet<String> =
        install::DESKTOP_BINARIES.iter().filter(|n| repo.join("server/build").join(n).is_file()).map(|n| n.to_string()).collect();
    ensure!(installed == expected, "the installed binaries differ from the desktop list: {installed:?} vs {expected:?}");
    for name in ["axia-lan", "axia-bench", "axia-sysreport", "axia-testbench"] {
        ensure!(staged.join("bin").join(name).is_file(), "missing desktop helper: {name}");
    }
    ensure!(!staged.join("assets/cleaner").exists(), "the OS cleaner was installed");
    ensure!(!staged.join("bin/axia-clean").exists(), "the OS cleaner was installed");
    ensure!(stage.root.join("usr/share/licenses/axia-de/OFL-Noto.txt").is_file(), "missing the Noto license");
    ensure!(!stage.root.join("etc").exists(), "the installation writes to /etc");
    ensure!(!stage.root.join("usr/lib/systemd").exists(), "the installation writes systemd units");
    let desktop_file = stage.root.join("usr/share/wayland-sessions/axia.desktop");
    ensure!(desktop_file.is_file(), "missing the session entry");
    for theme in ["Axia", "Axia-dark"] {
        for file in ["index.theme", "gtk-3.0/gtk.css"] {
            let installed = stage.root.join("usr/share/themes").join(theme).join(file);
            let source = repo.join("assets/gtk").join(theme).join(file);
            ensure!(fs::read(&installed).ok() == fs::read(&source).ok(), "{theme}/{file}: not installed as it is");
        }
    }
    let desktop = at(&desktop_file, fs::read_to_string(&desktop_file))?;
    ensure!(desktop.contains("Exec=/usr/libexec/axia/fedora-session"), "the session entry does not start fedora-session");
    ensure!(desktop.contains("DesktopNames=Axia;"), "the session entry has the wrong desktop name");
    let wrapper = staged.join("fedora-session");
    ensure!(wrapper.is_file() && fs::metadata(&wrapper).map_err(|e| e.to_string())?.permissions().mode() & 0o111 != 0, "fedora-session is not executable");
    let report = run(Command::new(stage.server()).arg("--check-code").arg(staged))?;
    ensure!(!report.contains("FAILED"), "the staged code does not match its manifest:\n{report}");
    Ok(())
}

/// The real app windows through the session restore: a shell-only startup would miss the Store and
/// Settings integration errors. Force Quit and About are opened after the theme starts.
fn restore_apps(run_dir: &Scratch, home: &Home, stage: &mut Stage) -> Result<()> {
    let saved: Vec<Value> = ["store", "settings", "files", "calculator"]
        .iter()
        .map(|app| {
            let mut window = json!({"id": format!("app:{app}"), "f": {"x": 20, "y": 30, "w": 700, "h": 500}});
            if *app == "settings" {
                window["args"] = json!({"section": "wallpaper"});
            }
            window
        })
        .collect();
    home.write_prefs(json!({"schema": 1, "debugTiming": true, "externalWindowStyle": true, "sessionRestore": "always",
                            "sessionSaved": {"clean": true, "windows": saved}}))?;
    let rel = "themes/default/theme.bundle.js";
    let original = at(&stage.staged.join(rel), fs::read(stage.staged.join(rel)))?;
    let mut fixture = original.clone();
    fixture.extend_from_slice(b"\nsetTimeout(() => { const sdk = __lordos.require('@lordos/sdk'); sdk.wm.open('forcequit'); sdk.wm.open('appabout', {args:{key:'calculator'}}); }, 700);\n");
    stage.replace(rel, &fixture)?;
    let result = session(run_dir, home, stage, "800x600", "5", 25);
    stage.replace(rel, &original)?;
    let log = result?;
    let keyfile = home.dir.join(".config/glib-2.0/settings/keyfile");
    let toolkit = at(&keyfile, fs::read_to_string(&keyfile))?;
    ensure!(toolkit.contains("gtk-theme='Adwaita"), "standard GTK theme was not selected");
    ensure!(toolkit.contains("button-layout=':minimize,maximize,close'"), "standard GTK button order was not synchronized");
    passed(&log, &[&format!("js: theme {THEME} loaded")])?;
    no_errors(&log, &ERRORS)?;
    for app in ["store", "settings", "files", "forcequit", "calculator", "appabout"] {
        passed(&log, &[&format!("timing: {app} window committed")])?;
    }
    Ok(())
}

/// A private compositor and an independent Wayland peer (never the live session).
fn clipboard_native(repo: &Path, run_dir: &Scratch, home: &Home, stage: &Stage) -> Result<()> {
    run(Command::new("make").arg("-C").arg(repo.join("server")).arg("build/clipboard-native-protocol"))?;
    home.write_prefs(json!({"schema": 1, "sessionRestore": "off", "welcomeShown": true}))?;
    let err = run_dir.0.join("native.err");
    let mut server = home.server(stage, &["--headless", "1000x700", "--timeout", "25", "--theme", THEME]);
    let mut child = server
        .stdout(Stdio::null())
        .stderr(at(&err, fs::File::create(&err))?)
        .spawn()
        .map_err(|e| format!("axia-server: {e}"))?;
    let outcome = (|| -> Result<()> {
        let sock = home.dir.join("runtime/wayland-0");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !sock.exists() {
            ensure!(child.try_wait().map_err(|e| e.to_string())?.is_none() && Instant::now() < deadline, "native clipboard: private compositor did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut peer = Command::new(repo.join("server/build/clipboard-native-protocol"));
        peer.env_clear().envs(&home.env).env("WAYLAND_DISPLAY", "wayland-0").current_dir(stage.bin()).stdin(Stdio::null());
        unprivileged(&mut peer);
        let output = run_for(run_dir, &mut peer, Duration::from_secs(18), "the native clipboard peer")?;
        ensure!(output.ok, "{}{}", output.stdout, output.stderr);
        ensure!(output.stdout.contains("clipboard native protocol: passed"), "{}", output.stdout);
        Ok(())
    })();
    let ended = stop(&mut child);
    let stderr = String::from_utf8_lossy(&fs::read(&err).unwrap_or_default()).into_owned();
    outcome?;
    match ended {
        Stopped::Stalled => return Err("native clipboard: session exit stalled".into()),
        Stopped::Failed => return Err(stderr),
        Stopped::Clean => {}
    }
    let path = home.dir.join(".local/share/axia/clipboard/history.json");
    let history: Value = serde_json::from_str(&at(&path, fs::read_to_string(&path))?).map_err(|e| e.to_string())?;
    let leaked = history["items"].as_array().is_some_and(|items| items.iter().any(|i| i["text"] == "password test"));
    ensure!(!leaked, "Wayland secret reached disk history");
    println!("clipboard native protocol: passed (large text, MIME forwarding, owner exit, stale offers, PRIMARY, secrets and PNG)");
    Ok(())
}

enum Stopped {
    Clean,
    Failed,
    Stalled,
}

/// SIGTERM, then up to five seconds for the process to leave on its own.
fn stop(child: &mut Child) -> Stopped {
    // SAFETY: the pid belongs to our child.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return if status.success() { Stopped::Clean } else { Stopped::Failed },
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Stopped::Stalled;
            }
        }
    }
}

/// A real PNG with a space in its source name: the bridge must copy, validate and thumbnail it, then
/// persist a theme-independent wallpaper selection; also a broken and a huge one.
fn wallpaper(repo: &Path, run_dir: &Scratch, home: &Home, stage: &mut Stage) -> Result<()> {
    let wallpaper = png(400, 240, &[b"\0".as_slice(), &[0x20u8, 0x60, 0xa0, 0xff].repeat(400)].concat().repeat(240));
    for (name, data) in [("wallpaper input.png", wallpaper.clone()), ("bad image.png", b"broken PNG".to_vec()), ("huge image.png", png(8192, 8192, b""))] {
        let path = home.dir.join(name);
        at(&path, fs::write(&path, data))?;
        if is_root() {
            chown(&path);
        }
    }
    // the real Rust -> C -> QuickJS -> SDK path, not a JS mock
    let rel = "themes/default/theme.bundle.js";
    let fixture = at(&repo.join("tests/switcher-smoke.bundle.js"), fs::read(repo.join("tests/switcher-smoke.bundle.js")))?;
    stage.replace(rel, &fixture)?;
    home.write_prefs(json!({"schema": 1, "shortcutProfile": "macos"}))?;
    let log = session(run_dir, home, stage, "1000x700", "5", 25)?;
    passed(&log, &["session power integration: passed", "host power dialog rendering: passed", "wallpaper integration: passed"])?;
    let prefs: Value = serde_json::from_str(&at(&home.prefs(), fs::read_to_string(home.prefs()))?).map_err(|e| e.to_string())?;
    let image = &prefs["wallpaperUser"][0];
    ensure!(prefs["wallpaper"] == image["id"], "the wallpaper selection was not stored");
    let src = PathBuf::from(image["src"].as_str().ok_or("the managed wallpaper has no src")?);
    ensure!(fs::read(&src).ok().as_deref() == Some(wallpaper.as_slice()), "the managed copy differs from the source image");
    let thumb = at(Path::new(image["thumb"].as_str().unwrap_or("")), fs::read(image["thumb"].as_str().unwrap_or("")))?;
    ensure!(thumb.len() >= 24, "the thumbnail is not a PNG");
    let (width, height) = (u32::from_be_bytes(thumb[16..20].try_into().unwrap()), u32::from_be_bytes(thumb[20..24].try_into().unwrap()));
    ensure!(width > 0 && width <= 320 && height > 0 && height <= 180, "the thumbnail is {width}x{height}");
    ensure!(fs::metadata(&src).map_err(|e| e.to_string())?.permissions().mode() & 0o777 == 0o600, "the managed wallpaper is not private");
    // the source can disappear without losing the managed wallpaper
    at(&home.dir, fs::remove_file(home.dir.join("wallpaper input.png")))?;
    ensure!(src.is_file(), "the managed wallpaper went with its source");
    no_errors(&log, &["ReferenceError:", "TypeError:", "SyntaxError:", "react (error)", "switcher smoke:"])
}

/// A real legacy history, including a PNG, through the full C/Rust/JS path.
fn clipboard_history(repo: &Path, run_dir: &Scratch, home: &Home, stage: &mut Stage) -> Result<()> {
    let dir = home.dir.join(".local/share/axia/clipboard");
    at(&dir, fs::create_dir_all(&dir))?;
    at(&dir, fs::write(dir.join("img-legacy.png"), png(1, 1, b"\0\x20\x60\xa0\xff")))?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    let history = json!({"version": 1, "items": [
        {"id": "legacy-text", "kind": "text", "type": "text", "text": "Olá, histórico antigo 👋", "origin": "terminal", "time": now, "pinned": true},
        {"id": "legacy-image", "kind": "image", "type": "image", "image": dir.join("img-legacy.png"), "origin": "linux", "time": now, "pinned": false}]});
    at(&dir, fs::write(dir.join("history.json"), history.to_string()))?;
    if is_root() {
        for p in [dir.clone(), dir.join("img-legacy.png"), dir.join("history.json")] {
            chown(&p);
        }
    }
    let fixture = at(&repo.join("tests/clipboard-smoke.bundle.js"), fs::read(repo.join("tests/clipboard-smoke.bundle.js")))?;
    stage.replace("themes/default/theme.bundle.js", &fixture)?;
    let log = session(run_dir, home, stage, "1000x700", "15", 20)?;
    passed(&log, &["clipboard integration: passed"])?;
    no_errors(&log, &["clipboard smoke:", "ReferenceError:", "TypeError:", "SyntaxError:", "react (error)"])
}

/// Dock policy, real native thumbnail updates and idle shutdown.
fn dock_previews(repo: &Path, run_dir: &Scratch, home: &Home, stage: &mut Stage) -> Result<()> {
    let fixture = at(&repo.join("tests/dock-preview-smoke.bundle.js"), fs::read(repo.join("tests/dock-preview-smoke.bundle.js")))?;
    stage.replace("themes/default/theme.bundle.js", &fixture)?;
    let log = session(run_dir, home, stage, "1000x700", "18", 20)?;
    passed(&log, &[
        "dock preview integration: passed",
        "dock preview sizing and close integration: passed",
        "dock disabled hover integration: passed",
        "dock feedback integration: passed",
        "dock native animation integration: passed",
        "scheduler compositor integration: passed",
    ])?;
    no_errors(&log, &["dock preview smoke:", "ReferenceError:", "TypeError:", "SyntaxError:", "react (error)"])
}

/// A real isolated app using the same Rust scheduler, driven by actual Wayland callbacks. Only this
/// disposable install's calendar package and its code manifest are changed.
fn scheduler_app(repo: &Path, run_dir: &Scratch, home: &Home, stage: &mut Stage) -> Result<()> {
    let app = at(&repo.join("server/tests/scheduler_app.js"), fs::read(repo.join("server/tests/scheduler_app.js")))?;
    stage.replace("assets/sysapps/calendar/app.js", &app)?;
    let mut theme = at(&repo.join("themes/default/theme.bundle.js"), fs::read(repo.join("themes/default/theme.bundle.js")))?;
    theme.extend_from_slice(b"\nsetTimeout(() => { if (!lord.apps.launchPackage('calendar')) console.error('scheduler app: launch failed'); }, 400);\n");
    stage.replace("themes/default/theme.bundle.js", &theme)?;
    home.write_prefs(json!({"schema": 3, "sessionRestore": "off", "welcomeShown": true}))?;
    let log = session(run_dir, home, stage, "1000x700", "6", 25)?;
    passed(&log, &["scheduler isolated integration: passed"])?;
    no_errors(&log, &["scheduler app:", "[error]", "ReferenceError:", "TypeError:", "SyntaxError:", "react (error)"])?;
    let pid = log
        .find("app 'calendar' started as an isolated process (pid ")
        .map(|i| &log[i + "app 'calendar' started as an isolated process (pid ".len()..])
        .and_then(|rest| rest.split(')').next())
        .ok_or_else(|| format!("the calendar app did not start:\n{log}"))?;
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        // reaped by the host
        Err(_) => Ok(()),
        Ok(stat) => {
            let fields: Vec<&str> = stat.rsplit_once(") ").map_or("", |(_, rest)| rest).split_whitespace().collect();
            ensure!(fields.first() == Some(&"Z") && fields.get(49) == Some(&"0"), "scheduler app did not exit cleanly: {log}");
            Ok(())
        }
    }
}

/// The cost of mouse moves over the Dock (server/tests/dock_bench.jsx), in a private headless
/// session of a staged installation. Run from the repository root inside a build tree
/// (`cargo xtask build-theme` and `make -C server` first).
pub fn dock_bench(repo: &Path, args: &[String]) -> Result<ExitCode> {
    ensure!(args.is_empty(), "dock-bench takes no arguments");
    let work = Scratch::new("axia-dock-bench-")?;
    let mut stage = stage(repo, &work)?;
    let bundle = at(&repo.join("tests/dock-bench.bundle.js"), fs::read(repo.join("tests/dock-bench.bundle.js")))?;
    stage.replace("themes/default/theme.bundle.js", &bundle)?;
    fs::set_permissions(&stage.root, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    let share = stage.staged.parent().and_then(Path::parent).ok_or("bad staging layout")?.join("share");
    let tmp = Scratch::new("axia-dock-home-")?;
    let home = home(&tmp.0, &share)?;
    let run_dir = Scratch::new("axia-dock-run-")?;
    // the session may end with a failure of its own; the bench lines are what counts
    let mut command = home.server(&stage, &["--headless", "1920x1080", "--timeout", "20", "--theme", THEME]);
    let _ = run_for(&run_dir, &mut command, Duration::from_secs(60), "the dock bench session");
    let log = home.log();
    let lines: Vec<&str> = log.lines().filter(|l| l.contains("dock bench")).collect();
    ensure!(!lines.is_empty(), "no \"dock bench\" lines in the session log");
    for line in lines {
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Clipboard transfers through XWayland in a temporary installation. The display comes from the
/// private session log, never from the live desktop.
pub fn x11_check(repo: &Path, args: &[String]) -> Result<ExitCode> {
    ensure!(args.is_empty(), "clipboard-x11-check takes no arguments");
    run(Command::new("make").arg("-C").arg(repo.join("server")).arg("build/clipboard-x11-protocol"))?;
    let work = Scratch::new("axia-x11-stage-")?;
    let stage = stage(repo, &work)?;
    fs::set_permissions(&stage.root, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    let tmp = Scratch::new("axia-x11-home-")?;
    let share = stage.staged.parent().and_then(Path::parent).ok_or("bad staging layout")?.join("share");
    let home = home(&tmp.0, &share)?;
    home.write_prefs(json!({"schema": 1, "sessionRestore": "off", "welcomeShown": true}))?;
    if is_root() {
        chown_tree(&home.dir);
    }
    let run_dir = Scratch::new("axia-x11-run-")?;
    let err = run_dir.0.join("server.err");
    let mut server = home.server(&stage, &["--headless", "1000x700", "--timeout", "30", "--theme", THEME]);
    let mut child = server.stdout(Stdio::null()).stderr(at(&err, fs::File::create(&err))?).spawn().map_err(|e| format!("axia-server: {e}"))?;
    let outcome = (|| -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let display = loop {
            let content = home.log();
            if let Some(i) = content.find("xwayland: DISPLAY=") {
                let rest = &content[i + "xwayland: DISPLAY=".len()..];
                let digits: String = rest.chars().skip(1).take_while(char::is_ascii_digit).collect();
                if rest.starts_with(':') && !digits.is_empty() {
                    break format!(":{digits}");
                }
            }
            ensure!(child.try_wait().map_err(|e| e.to_string())?.is_none() && Instant::now() < deadline, "private compositor did not start");
            std::thread::sleep(Duration::from_millis(20));
        };
        let mut peer = Command::new(repo.join("server/build/clipboard-x11-protocol"));
        peer.env_clear().envs(&home.env).env("DISPLAY", display).stdin(Stdio::null());
        unprivileged(&mut peer);
        let output = run_for(&run_dir, &mut peer, Duration::from_secs(22), "the X11 clipboard peer")?;
        println!("{}{}", output.stdout, output.stderr);
        ensure!(output.ok, "{}", home.log());
        Ok(())
    })();
    let ended = stop(&mut child);
    let stderr = String::from_utf8_lossy(&fs::read(&err).unwrap_or_default()).into_owned();
    outcome?;
    match ended {
        Stopped::Stalled => Err("private XWayland session did not exit".into()),
        Stopped::Failed => Err(stderr),
        Stopped::Clean => Ok(ExitCode::SUCCESS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn a_generated_png_is_a_png_with_the_stated_size() {
        let data = png(3, 2, &[0u8; 2 * (1 + 3 * 4)]);
        assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&data[16..24], [0, 0, 0, 3, 0, 0, 0, 2]);
        assert!(data.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]));
        let decoded = image::load_from_memory(&data).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (3, 2));
        // the broken fixtures: a huge size with no data refuses to decode, as the bridge must refuse it
        assert!(image::load_from_memory(&png(8192, 8192, b"")).is_err());
    }

    #[test]
    fn replacing_a_staged_file_keeps_the_manifest_in_step() {
        let dir = crate::TestDir::new("desktop-check-stage");
        let staged = dir.0.join("usr/libexec/axia");
        fs::create_dir_all(staged.join("themes/default")).unwrap();
        let mut stage = Stage {
            root: dir.0.clone(),
            staged: staged.clone(),
            manifest: json!({"files": [{"path": "themes/default/theme.bundle.js", "size": 1, "sha256": "x"}]}),
        };
        stage.replace("themes/default/theme.bundle.js", b"new bundle").unwrap();
        let manifest: Value = serde_json::from_str(&fs::read_to_string(staged.join("code-manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["files"][0]["size"], 10);
        assert_eq!(manifest["files"][0]["sha256"], sha256::hex(b"new bundle"));
        assert!(stage.replace("missing.js", b"x").is_err());
    }

    #[test]
    fn the_checks_report_the_missing_and_the_forbidden() {
        assert!(passed("a b c", &["b"]).is_ok() && passed("a", &["z"]).unwrap_err().contains("missing \"z\""));
        assert!(no_errors("fine", &ERRORS).is_ok() && no_errors("TypeError: x", &ERRORS).unwrap_err().contains("TypeError:"));
    }
}
