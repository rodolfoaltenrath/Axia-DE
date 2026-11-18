use super::*;
use std::{
    ffi::CString,
    io,
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = PathBuf::from(format!("/var/tmp/axia-idle-{}-{stamp}", std::process::id()));
        at(&path, fs::create_dir(&path))?;
        let workspace = Self(path);
        at(
            &workspace.0,
            fs::set_permissions(&workspace.0, fs::Permissions::from_mode(0o755)),
        )?;
        Ok(workspace)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Keep the process group owned until all measurements and log copies finish.
// Helpers must not survive either successful collection or an early failure.
struct Session(Child);
impl Session {
    fn alive(&mut self) -> Result<()> {
        if self.0.try_wait().map_err(|e| e.to_string())?.is_some() {
            Err("private session exited early; inspect stderr.log".into())
        } else {
            Ok(())
        }
    }
    fn wait(&mut self, timeout: Duration) -> Result<std::process::ExitStatus> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.0.try_wait().map_err(|e| e.to_string())? {
                return Ok(status);
            }
            if start.elapsed() >= timeout {
                return Err("private session did not terminate".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // This group was created specifically for this probe, never inherited.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGTERM);
        }
        let _ = self.wait(Duration::from_secs(3));
        // The tracer may exit while a helper remains in its group.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

fn identity() -> Result<(u32, u32)> {
    let uid = unsafe { libc::geteuid() };
    if uid != 0 {
        return Ok((uid, unsafe { libc::getegid() }));
    }
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut buffer = vec![0u8; 65536];
    let mut result = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwnam_r(
            c"nobody".as_ptr(),
            entry.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return Err("cannot resolve nobody for private session".into());
    }
    let entry = unsafe { entry.assume_init() };
    Ok((entry.pw_uid, entry.pw_gid))
}

fn own(path: &Path, uid: u32, gid: u32) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let name = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    if unsafe { libc::chown(name.as_ptr(), uid, gid) } != 0 {
        return Err(format!(
            "{}: {}",
            path.display(),
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn copy_tree(source: &Path, target: &Path, root: &Path) -> Result<()> {
    let resolved = at(source, fs::canonicalize(source))?;
    if !resolved.starts_with(root) {
        return Err(format!(
            "installation symlink escapes source: {}",
            source.display()
        ));
    }
    let metadata = at(source, fs::metadata(source))?;
    if metadata.is_dir() {
        // Directory symlinks can form cycles; installed resources need none.
        if at(source, fs::symlink_metadata(source))?
            .file_type()
            .is_symlink()
        {
            return Err("directory symlinks are unsupported in the probe installation".into());
        }
        at(target, fs::create_dir(target))?;
        at(
            target,
            fs::set_permissions(target, fs::Permissions::from_mode(0o755)),
        )?;
        for entry in at(source, fs::read_dir(source))? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_tree(&entry.path(), &target.join(entry.file_name()), root)?;
        }
    } else if metadata.is_file() {
        at(target, fs::copy(source, target))?;
        let mode = if metadata.mode() & 0o111 != 0 {
            0o755
        } else {
            0o644
        };
        at(
            target,
            fs::set_permissions(target, fs::Permissions::from_mode(mode)),
        )?;
    } else {
        return Err(format!(
            "unsupported installation entry: {}",
            source.display()
        ));
    }
    Ok(())
}

pub(super) fn collect_all(config: &Config, report: &mut Value) -> Result<()> {
    let source = at(&config.installation, fs::canonicalize(&config.installation))?;
    for name in [
        "bin/axia-server",
        "themes/default/theme.bundle.js",
        "assets/fonts",
    ] {
        if !source.join(name).exists() {
            return Err(format!("incomplete installation: {name}"));
        }
    }
    let workspace = Workspace::new()?;
    let installation = workspace.0.join("installation");
    copy_tree(&source, &installation, &source)?;
    let artifacts = config.output.with_file_name(format!(
        "{}-samples",
        config
            .output
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    for index in 1..=config.samples {
        let run = workspace.0.join(format!("sample-{index}"));
        at(&run, fs::create_dir(&run))?;
        at(
            &run,
            fs::set_permissions(&run, fs::Permissions::from_mode(0o755)),
        )?;
        let destination = artifacts.join(format!("sample-{index}"));
        let result = collect(config, &installation, &run);
        at(&destination, fs::create_dir_all(&destination))?;
        for name in ["poll.trace", "stderr.log", "home/.cache/axia/session.log"] {
            let log = run.join(name);
            if log.is_file() {
                at(
                    &log,
                    fs::copy(&log, destination.join(log.file_name().unwrap())),
                )?;
            }
        }
        report["samples"].as_array_mut().unwrap().push(result?);
    }
    Ok(())
}

fn epoch() -> Result<f64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs_f64())
}

fn collect(config: &Config, installation: &Path, run: &Path) -> Result<Value> {
    let (uid, gid) = identity()?;
    let home = run.join("home");
    for path in [
        home.clone(),
        home.join(".config"),
        home.join(".config/axia"),
        run.join("runtime"),
    ] {
        at(&path, fs::create_dir(&path))?;
        own(&path, uid, gid)?;
        at(
            &path,
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)),
        )?;
    }
    let settings = home.join(".config/axia/settings.json");
    at(
        &settings,
        fs::write(
            &settings,
            r#"{"welcomeShown":true,"theme":"default","mode":"dark","sessionRestore":"off"}"#,
        ),
    )?;
    own(&settings, uid, gid)?;
    let trace = run.join("poll.trace");
    at(&trace, fs::write(&trace, ""))?;
    own(&trace, uid, gid)?;
    let stderr = run.join("stderr.log");
    let timeout = (config.warmup.as_secs_f64() + config.duration.as_secs_f64() + 15.)
        .ceil()
        .to_string();
    let args: Vec<String> = vec![
        "-qq".into(),
        "-ttt".into(),
        "-T".into(),
        "-e".into(),
        "trace=poll,ppoll,epoll_wait,epoll_pwait".into(),
        "-o".into(),
        trace.display().to_string(),
        installation.join("bin/axia-server").display().to_string(),
        "--headless".into(),
        "1180x700".into(),
        "--timeout".into(),
        timeout,
        "--theme".into(),
        installation
            .join("themes/default/theme.bundle.js")
            .display()
            .to_string(),
        "--fonts".into(),
        installation.join("assets/fonts").display().to_string(),
    ];
    let mut command = Command::new("strace");
    command
        .args(&args)
        .current_dir(installation.join("bin"))
        .stdout(Stdio::null())
        .stderr(at(&stderr, fs::File::create(&stderr))?)
        .process_group(0)
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", run.join("runtime"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("LORD_GPU", "0")
        .env("AXIA_NET_OFFLINE", "1");
    for key in [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "PIPEWIRE_REMOTE",
        "PULSE_SERVER",
        "LORDOS_SESSIOND",
        "LORD_HWSCAN_ROOT",
    ] {
        command.env_remove(key);
    }
    let root = unsafe { libc::geteuid() } == 0;
    // Only async-signal-safe system calls between fork and exec; no allocation.
    unsafe {
        command.pre_exec(move || {
            if root
                && (libc::setgroups(0, std::ptr::null()) != 0
                    || libc::setgid(gid) != 0
                    || libc::setuid(uid) != 0)
            {
                return Err(io::Error::last_os_error());
            }
            if libc::setpriority(libc::PRIO_PROCESS, 0, 19) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut session = Session(command.spawn().map_err(|e| format!("strace: {e}"))?);
    thread::sleep(config.warmup);
    session.alive()?;
    let children_path = PathBuf::from(format!("/proc/{0}/task/{0}/children", session.0.id()));
    let children = at(&children_path, fs::read_to_string(&children_path))?;
    let children = children.split_whitespace().collect::<Vec<_>>();
    if children.len() != 1 {
        return Err("cannot identify private compositor under strace".into());
    }
    let pid: i32 = children[0].parse().map_err(|_| "invalid compositor pid")?;
    let process = PathBuf::from(format!("/proc/{pid}"));
    if at(&process, fs::read_to_string(process.join("comm")))?.trim() != "axia-server" {
        return Err("strace child is not axia-server".into());
    }
    let start = Instant::now();
    let since = epoch()?;
    let mut rss = Vec::new();
    while start.elapsed() < config.duration {
        session.alive()?;
        let status = at(&process, fs::read_to_string(process.join("status")))?;
        let kb = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("missing compositor RSS")?;
        rss.push(kb as f64 * 1024.);
        thread::sleep(Duration::from_secs(1).min(config.duration.saturating_sub(start.elapsed())));
    }
    let elapsed = start.elapsed().as_secs_f64();
    let until = epoch()?;
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let status = session.wait(Duration::from_secs(8))?;
    if !status.success() {
        return Err(format!("private session/strace exit status: {status}"));
    }
    let log = home.join(".cache/axia/session.log");
    let text = at(&log, fs::read_to_string(&log))?;
    if !text.contains("js: theme ")
        || !text.contains(" loaded in ")
        || [
            "js: exception:",
            "js: TypeError:",
            "js: ReferenceError:",
            "js: SyntaxError:",
        ]
        .iter()
        .any(|s| text.contains(s))
    {
        return Err("theme did not load correctly; inspect session.log".into());
    }
    let count = poll_count(&at(&trace, fs::read_to_string(&trace))?, since, until)?;
    let max = rss
        .iter()
        .copied()
        .reduce(f64::max)
        .ok_or("empty RSS sample set")?;
    let rss_median = median(&mut rss);
    let mut command = vec!["strace".to_owned()];
    command.extend(args);
    Ok(
        json!({"uid":uid,"duration_seconds":elapsed,"poll_calls":count,"poll_calls_per_second":count as f64/elapsed,
        "rss_bytes_median":rss_median,"rss_bytes_max":max,"rss_samples":rss.len(),"command":command,"exit_code":0}),
    )
}
