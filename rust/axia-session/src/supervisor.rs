use crate::{environment, Environment};
use std::{
    ffi::{CString, OsStr, OsString},
    fs::File,
    io::{self, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

fn status_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(128 + status.signal().unwrap_or(1))
}

/// A bounded auxiliary command. The caller pumps session signals while waiting.
pub(crate) fn bounded(
    command: &mut Command,
    timeout: Duration,
    mut pump: impl FnMut() -> io::Result<()>,
) -> io::Result<ExitStatus> {
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    let result = loop {
        if let Err(error) = pump() {
            break Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(status)
                } else {
                    Err(io::Error::other(format!("command exited with {status}")))
                }
            }
            Err(error) => break Err(error),
            Ok(None) => (),
        }
        if Instant::now() >= deadline {
            break Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "session helper timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

// Signals are consumed synchronously. No global handler, worker thread, polling
// of /proc while idle, or asynchronous access to a recycled process ID.
struct Signals {
    fd: OwnedFd,
    previous: libc::sigset_t,
}
impl Signals {
    fn new() -> io::Result<Self> {
        let mut mask = unsafe { std::mem::zeroed::<libc::sigset_t>() };
        let mut previous = unsafe { std::mem::zeroed::<libc::sigset_t>() };
        unsafe {
            libc::sigemptyset(&mut mask);
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGCHLD] {
                libc::sigaddset(&mut mask, signal);
            }
            let code = libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut previous);
            if code != 0 {
                return Err(io::Error::from_raw_os_error(code));
            }
            let fd = libc::signalfd(-1, &mask, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK);
            if fd < 0 {
                let error = io::Error::last_os_error();
                libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
                return Err(error);
            }
            Ok(Self {
                fd: OwnedFd::from_raw_fd(fd),
                previous,
            })
        }
    }
    fn prepare<'a>(&self, command: &'a mut Command) -> &'a mut Command {
        let mask = self.previous;
        // exec children must receive the original mask, including the server
        // and commands started after the supervisor began handling SIGCHLD.
        unsafe {
            command.pre_exec(move || {
                let code = libc::pthread_sigmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut());
                if code == 0 {
                    Ok(())
                } else {
                    Err(io::Error::from_raw_os_error(code))
                }
            });
        }
        command
    }
    fn drain(&self, server: &mut Child) -> io::Result<()> {
        loop {
            let mut info = std::mem::MaybeUninit::<libc::signalfd_siginfo>::uninit();
            let size = std::mem::size_of::<libc::signalfd_siginfo>();
            let n = unsafe { libc::read(self.fd.as_raw_fd(), info.as_mut_ptr().cast(), size) };
            if n < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Ok(());
                }
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if n as usize != size {
                return Err(io::Error::other("short session signal record"));
            }
            let signal = unsafe { info.assume_init().ssi_signo } as i32;
            if matches!(signal, libc::SIGTERM | libc::SIGINT) && server.try_wait()?.is_none() {
                // An exited child is not reaped until try_wait; therefore its
                // PID cannot be reused between the check and this kill.
                unsafe {
                    libc::kill(server.id() as i32, signal);
                }
            }
        }
    }
    fn wait(
        &self,
        server: &mut Child,
        watch: Option<&OwnedFd>,
        timeout: Option<Duration>,
    ) -> io::Result<()> {
        let mut fds = [
            libc::pollfd {
                fd: self.fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: watch.map_or(-1, AsRawFd::as_raw_fd),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ms = timeout.map_or(-1, |d| d.as_millis().min(i32::MAX as u128).max(1) as i32);
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, ms) } < 0
            && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted
        {
            return Err(io::Error::last_os_error());
        }
        self.drain(server)?;
        if let Some(watch) = watch {
            let mut buffer = [0u8; 4096];
            while unsafe { libc::read(watch.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) }
                > 0
            {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_success_failure_and_timeout_are_bounded() {
        assert!(bounded(
            &mut Command::new("/usr/bin/true"),
            Duration::from_secs(1),
            || Ok(())
        )
        .unwrap()
        .success());
        assert!(bounded(
            &mut Command::new("/usr/bin/false"),
            Duration::from_secs(1),
            || Ok(())
        )
        .is_err());
        let start = Instant::now();
        let error = bounded(
            Command::new("/usr/bin/sleep").arg("60"),
            Duration::from_millis(30),
            || Ok(()),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn helper_shutdown_terminates_and_reaps_the_owned_child() {
        let mut child = Command::new("/usr/bin/sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        stop(&mut child);
        assert!(child.try_wait().unwrap().is_some());
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous, std::ptr::null_mut());
        }
    }
}

fn watch_runtime(runtime: &Path) -> io::Result<OwnedFd> {
    let path = CString::new(runtime.as_os_str().as_bytes())?;
    let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    if unsafe {
        libc::inotify_add_watch(
            fd.as_raw_fd(),
            path.as_ptr(),
            libc::IN_CREATE | libc::IN_MOVED_TO,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

fn stop(child: &mut Child) {
    if !matches!(child.try_wait(), Ok(None)) {
        return;
    }
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
}
struct Session {
    server: Child,
    helpers: Vec<Child>,
}
impl Drop for Session {
    fn drop(&mut self) {
        for helper in &mut self.helpers {
            stop(helper);
        }
        stop(&mut self.server);
    }
}

fn diagnostic(log: &mut Option<File>, message: impl std::fmt::Display) {
    if let Some(log) = log {
        let _ = writeln!(log, "Axia: {message}");
    } else {
        eprintln!("Axia: {message}");
    }
}

pub(crate) fn run(root: &Path, mut env: Environment, arguments: Vec<OsString>) -> io::Result<i32> {
    for (key, value) in [
        ("GDK_BACKEND", "wayland"),
        ("MOZ_ENABLE_WAYLAND", "1"),
        ("DISPLAY", ""),
        ("GTK_USE_PORTAL", "1"),
    ] {
        env.insert(key.into(), value.into());
    }
    if let Err(error) = environment::initialize_user_dirs(&env) {
        eprintln!("Axia: could not initialize standard user folders: {error}");
    }
    let mut log = match environment::session_log(&env) {
        Ok(log) => Some(log),
        Err(error) => {
            eprintln!("Axia: could not open session log: {error}");
            None
        }
    };
    let runtime = std::path::PathBuf::from(&env[OsStr::new("XDG_RUNTIME_DIR")]);
    let before = environment::sockets(&runtime);
    let mut watch = watch_runtime(&runtime).ok();
    let signals = Signals::new()?;
    let mut command = Command::new(root.join("bin/axia-server"));
    command.current_dir(root.join("bin")).envs(&env);
    if let Some(theme) = environment::theme(&env, root) {
        command.arg("--theme").arg(theme);
    }
    command.args(arguments);
    if let Some(log) = &log {
        command.stdout(log.try_clone()?).stderr(log.try_clone()?);
    }
    let mut session = Session {
        server: signals.prepare(&mut command).spawn()?,
        helpers: vec![],
    };
    let startup_deadline = Instant::now() + Duration::from_secs(20);
    let mut startup_done = false;
    let mut next_update = None;
    loop {
        signals.drain(&mut session.server)?;
        if let Some(status) = session.server.try_wait()? {
            return Ok(status_code(status));
        }
        if !startup_done {
            let new = environment::sockets(&runtime)
                .difference(&before)
                .next()
                .cloned();
            if let Some(wayland) = new {
                env.insert("WAYLAND_DISPLAY".into(), wayland);
                env.insert(
                    "DISPLAY".into(),
                    environment::x_display(session.server.id(), Path::new("/tmp"), unsafe {
                        libc::getuid()
                    }),
                );
                let mut publish = Command::new("dbus-update-activation-environment");
                publish
                    .args([
                        "--systemd",
                        "DISPLAY",
                        "WAYLAND_DISPLAY",
                        "XDG_CURRENT_DESKTOP",
                        "XDG_SESSION_DESKTOP",
                        "XDG_SESSION_TYPE",
                        "GDK_BACKEND",
                        "MOZ_ENABLE_WAYLAND",
                        "GTK_USE_PORTAL",
                    ])
                    .envs(&env);
                // Publication failures must not abort an otherwise valid desktop.
                if let Err(error) = bounded(
                    signals.prepare(&mut publish),
                    Duration::from_secs(10),
                    || signals.drain(&mut session.server),
                ) {
                    diagnostic(
                        &mut log,
                        format_args!("could not publish session environment: {error}"),
                    );
                }
                let agent = Path::new("/usr/libexec/polkit-mate-authentication-agent-1");
                if agent.is_file() && session.server.try_wait()?.is_none() {
                    let mut command = Command::new(agent);
                    command.envs(&env);
                    if let Some(log) = &log {
                        command.stdout(log.try_clone()?).stderr(log.try_clone()?);
                    }
                    match signals.prepare(&mut command).spawn() {
                        Ok(child) => session.helpers.push(child),
                        Err(error) => diagnostic(
                            &mut log,
                            format_args!("could not start polkit agent: {error}"),
                        ),
                    }
                }
                startup_done = true;
            } else if Instant::now() >= startup_deadline {
                startup_done = true;
            }
            if startup_done {
                watch = None;
                next_update = Some(Instant::now() + Duration::from_secs(30));
            }
        }
        if next_update.is_some_and(|at| Instant::now() >= at) {
            if environment::executable_replaced(session.server.id()) {
                diagnostic(&mut log, "the running executable was replaced; log out and back in to use the installed build.");
                let (title, body) = environment::update_message(&env);
                let mut notify = Command::new("notify-send");
                notify
                    .args([
                        "--app-name=Axia DE",
                        "--icon=system-software-update",
                        title,
                        body,
                    ])
                    .envs(&env)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                let _ = bounded(signals.prepare(&mut notify), Duration::from_secs(3), || {
                    signals.drain(&mut session.server)
                });
                next_update = None;
            } else {
                next_update = Some(Instant::now() + Duration::from_secs(30));
            }
        }
        // A bounded publisher/notifier may have consumed SIGCHLD while the
        // compositor exited. Check its cached/waitable status before sleeping;
        // there need not be another signal to wake this poll.
        if let Some(status) = session.server.try_wait()? {
            return Ok(status_code(status));
        }
        let deadline = if startup_done {
            next_update
        } else {
            Some(startup_deadline)
        };
        // inotify normally wakes discovery. If unavailable, retain the old
        // bounded 100 ms startup scan; once ready, only signals/30 s checks wake.
        let timeout = deadline.map(|at| at.saturating_duration_since(Instant::now()));
        let timeout = if !startup_done && watch.is_none() {
            Some(timeout.unwrap().min(Duration::from_millis(100)))
        } else {
            timeout
        };
        signals.wait(&mut session.server, watch.as_ref(), timeout)?;
    }
}
